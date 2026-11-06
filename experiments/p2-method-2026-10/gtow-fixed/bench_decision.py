"""Decision loss of the L1 class errors under GTO Wizard's preflop (bb/100).

usage: python bench_decision.py <reach.json> <GTOW reference> <GTOW profile> --bands FILE --p1 DIR
       --variant NAME ROOT VARIANT [--variant ...] [--multi DIR ...] --json OUT --markdown OUT

At every GTO Wizard decision node a benchmark line passes, with actor h, action a and class c:
Q_L1(a, c) = Q_GTOW(a, c) + sum over the line leaves after a where h stays active of
P_self(leaf | a, c) x P_opp(leaf | a) x ebar(leaf, h, c), with P_self h's later GTO Wizard class strategy,
P_opp the other seats' later `total_frequency` and ebar the bench_metric.py mean card error (0 when P1 has no
value). P1 is taken as the truth, so the true action values are Q_GTOW; leaves outside the 36 lines count as 0.
The loss is D = 100 x sum_node line_probability x sum_c range_c n(c) / range_combos x
(Q_GTOW(best_GTOW) - Q_GTOW(best_L1)). Ties within 1e-9 go to the larger Q_GTOW, then the earlier action.
D_dev / D_held use ebar from the dev / held flop halves; D_stable counts only (node, class) cells that both
halves move to the same action (loss at the dev choice). D_multi replaces ebar by the total error
L1 - P1 multi-size on the leaves that have multi-size P1.
"""
import argparse
import json
from pathlib import Path
import sys

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "b4s-8h"))
import bench_metric  # noqa: E402
from bench_reach import reference_paths  # noqa: E402
from compare import combos, label  # noqa: E402

TIE = 1e-9


def ebar_map(data, parts):
    """{(leaf index, seat): {class name: ebar}} for cells bench_metric uses."""
    out = {}
    for b, p in zip(data.bands, parts):
        for r, row in enumerate(b["rows"]):
            out[(row["index"], row["seat"])] = {name: float(p["ebar"][r, c])
                                               for c, name in enumerate(data.classes) if p["use"][r, c]}
    return out


def multi_map(data, root, variant, multi_dirs):
    """ebar of L1 - P1 multi-size on the leaves with multi-size P1."""
    out = {}
    for b in data.bands:
        loaded = bench_metric.load_multi(data, multi_dirs, b)
        if loaded is None:
            continue
        rows, flops, multi = loaded
        ix = np.ix_(rows, range(169), flops)
        values, mass = data.l1(root, variant, b)
        w = b["w"][rows]
        present = ~np.isnan(b["p1"][ix]).all(-1) & ~np.isnan(multi).all(-1)
        part = bench_metric.error_part(values[ix] - multi, mass[ix], w, present)
        for i, r in enumerate(rows):
            row = b["rows"][r]
            out[(row["index"], row["seat"])] = {name: float(part["ebar"][i, c])
                                               for c, name in enumerate(data.classes) if part["use"][i, c]}
    return out


class Tree:
    """GTO Wizard decisions on the benchmark lines and the line leaves after each action."""

    def __init__(self, reach, reference, profile):
        self.reference = reference
        self.nodes = {tuple(n["path"]): n for n in profile["nodes"]}
        self.ref = reference_paths(reference, self.nodes)
        self.ref_classes = reference["classes"]
        self.n = [combos(c) for c in self.ref_classes]
        self.leaves = [(leaf["index"], leaf["line"].split("/"), {int(s) for s in leaf["seats"]})
                       for leaf in reach["leaves"]]
        self.decisions = []
        seen = set()
        for _, labels, _ in self.leaves:
            for k in range(len(labels)):
                path = tuple(labels[:k])
                if path in seen:
                    continue
                seen.add(path)
                ref = self.ref.get(path)
                if ref is None:
                    raise ValueError(f"no GTO Wizard node for {'/'.join(path)}")
                trunk = self.nodes[path]["actions"]
                mapped = [label(a["code"], trunk, nearest=True) for a in ref["actions"]]
                if None in mapped or len(set(mapped)) != len(mapped):
                    continue
                self.decisions.append((path, ref, mapped))

    def ref_action(self, path, choice):
        ref = self.ref[path]
        trunk = self.nodes[path]["actions"]
        return next(a for a in ref["actions"] if label(a["code"], trunk, nearest=True) == choice)

    def downstream(self, path, choice, actor):
        """[(leaf index, P_opp, P_self per reference class)] of the line leaves after (path, choice) where actor
        stays active."""
        out = []
        k = len(path)
        for index, labels, seats in self.leaves:
            if tuple(labels[:k]) != path or labels[k] != choice or actor not in seats:
                continue
            opp = 1.0
            own = np.ones(len(self.ref_classes))
            for j in range(k + 1, len(labels)):
                sub = tuple(labels[:j])
                action = self.ref_action(sub, labels[j])
                if self.nodes[sub]["actor"] == actor:
                    own *= np.array(action["strategy"])
                else:
                    opp *= action["total_frequency"]
            out.append((index, opp, own))
        return out


def best(q, tie_break):
    """Index of the max of q; ties within TIE go to the larger tie_break, then the earlier action."""
    top = q.max()
    cand = [i for i in range(len(q)) if q[i] >= top - TIE]
    return max(cand, key=lambda i: (tie_break[i], -i))


def decide(tree, errors):
    """Per decision: (weights per class, Q_GTOW (A,C), chosen L1 action per class, best GTOW per class)."""
    out = []
    for path, ref, mapped in tree.decisions:
        actor = tree.nodes[path]["actor"]
        q = np.array([a["ev"] for a in ref["actions"]], dtype=float)
        delta = np.zeros_like(q)
        for a, choice in enumerate(mapped):
            for index, opp, own in tree.downstream(path, choice, actor):
                e = errors.get((index, actor), {})
                delta[a] += opp * own * np.array([e.get(c, 0.0) for c in tree.ref_classes])
        weights = ref["line_probability"] * np.array(ref["range"]) * np.array(tree.n) / ref["range_combos"]
        ql1 = q + delta
        g = [best(q[:, c], q[:, c]) for c in range(q.shape[1])]
        l1 = [best(ql1[:, c], q[:, c]) for c in range(q.shape[1])]
        out.append(dict(path=path, q=q, weights=weights, gtow=g, l1=l1))
    return out


def loss(decisions, choice=None, keep=None):
    total = 0.0
    for k, d in enumerate(decisions):
        pick = d["l1"] if choice is None else choice[k]
        for c, w in enumerate(d["weights"]):
            if w and (keep is None or keep[k][c]):
                total += w * (d["q"][d["gtow"][c], c] - d["q"][pick[c], c])
    return 100 * total


def changed(d, i):
    """The L1 choice loses value against the GTO Wizard best action for class i (ties are not changes)."""
    return d["q"][d["gtow"][i], i] - d["q"][d["l1"][i], i] > TIE


def summarize(tree, full, dev, held, multi=None):
    a, b, c = decide(tree, full), decide(tree, dev), decide(tree, held)
    stable_keep = [[x["l1"][i] == y["l1"][i] and changed(x, i) for i in range(len(x["l1"]))]
                   for x, y in zip(b, c)]
    flip = sum(w for d in a for i, w in enumerate(d["weights"]) if changed(d, i))
    out = dict(D=loss(a), D_dev=loss(b), D_held=loss(c), D_stable=loss(b, keep=stable_keep), flip_share=float(flip),
               flips=[dict(path="/".join(d["path"]), cls=tree.ref_classes[i], weight=float(d["weights"][i]),
                           loss=100 * float(d["weights"][i] * (d["q"][d["gtow"][i], i] - d["q"][d["l1"][i], i])))
                      for d in a for i in range(len(d["l1"])) if changed(d, i) and d["weights"][i]])
    if multi is not None:
        out["D_multi"] = loss(decide(tree, multi))
    return out


def markdown(results, multi_leaves):
    lines = ["# 判断の損失（bb/100、GTO WizardのPreflop）", "",
             "|案|D|D 調整用|D 検証用|D 安定|行動が変わった割合|D 複数size|",
             "|---|---:|---:|---:|---:|---:|---:|"]
    for name, r in results.items():
        multi = f"{r['D_multi']:.4f}" if "D_multi" in r else "—"
        lines.append(f"|{name}|{r['D']:.4f}|{r['D_dev']:.4f}|{r['D_held']:.4f}|{r['D_stable']:.4f}|"
                     f"{r['flip_share']:.4f}|{multi}|")
    if multi_leaves:
        lines += ["", "複数sizeの誤差を使ったleaf × seat：" + ", ".join(multi_leaves)]
    lines += ["", "36本のline以外のleafの誤差は0とみなす（限界）。", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("reach")
    parser.add_argument("reference")
    parser.add_argument("profile")
    parser.add_argument("--bands", required=True)
    parser.add_argument("--p1", action="append", required=True)
    parser.add_argument("--variant", nargs=3, action="append", required=True, metavar=("NAME", "ROOT", "VARIANT"))
    parser.add_argument("--multi", action="append", default=[])
    parser.add_argument("--json", required=True)
    parser.add_argument("--markdown", required=True)
    args = parser.parse_args()
    reach = json.loads(Path(args.reach).read_text(encoding="utf-8"))
    tree = Tree(reach, json.loads(Path(args.reference).read_text(encoding="utf-8")),
                json.loads(Path(args.profile).read_text(encoding="utf-8")))
    sets = {half: bench_metric.Data(reach, bench_metric.read_bands(args.bands, half), args.p1)
            for half in (None, "dev", "held")}
    results = {}
    multi_leaves = []
    for name, root, variant in args.variant:
        maps = {}
        for half, data in sets.items():
            _, parts = bench_metric.evaluate(data, [(name, root, variant)], data.classes)
            maps[half] = ebar_map(data, parts[name])
        multi = None
        if args.multi:
            replaced = multi_map(sets[None], root, variant, args.multi)
            multi = {**maps[None], **replaced}
            multi_leaves = [f"L{i:02d}/{s}" for i, s in sorted(replaced)]
        results[name] = summarize(tree, maps[None], maps["dev"], maps["held"], multi)
    out = dict(format="p2-bench-decision", version=1, variants=results, multi_leaves=multi_leaves)
    Path(args.json).write_text(json.dumps(out, indent=1) + "\n", encoding="utf-8", newline="\n")
    Path(args.markdown).write_text(markdown(results, multi_leaves), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
