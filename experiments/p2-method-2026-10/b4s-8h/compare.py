"""Compare B4 Simple class profiles with the GTO Wizard Simple 100bb preflop tree reference.

usage: python compare.py [--nodes N] [--detail HISTORY]... [--json OUT] <label>=<p2-class-profile JSON> [...]

For every reference node (135 preflop decisions whose GTO Wizard line probability is at least 0.0005) the
profile's node is found by translating the GTO Wizard history (F, C, R<bb>, RAI) into the tree's action labels.
Two measures per node, both on GTO Wizard's range of the actor there (class weight = combos x range weight):

- distance: the total variation between the two action distributions per class, 0.5 * sum_a |ours - GTOW|.
- EV loss: what our action choice costs by GTO Wizard's own per-class action EVs,
  sum_a ours(a) * (max_b EV(b) - EV(a)), in bb per hand reaching the node. It is small where GTO Wizard is
  nearly indifferent, so it tells costly differences from harmless ones. GTO Wizard's own strategy is scored the
  same way as a check (its rounding and mixing give a small loss).

Nodes are summarised by group (open = no raise yet, vs open = facing one raise, vs 3bet, vs 4bet+): the distance
as a mean weighted by line probability and as a plain mean, the EV loss as the sum of line probability x loss in
bb/100 hands (each node's deviation alone, with GTO Wizard's ranges and play elsewhere). Action frequencies are
also given on GTO Wizard's range. Python standard library only.
"""
import argparse
import json
from pathlib import Path

REFERENCE = Path(__file__).resolve().parent / "references/gtowizard-simple-100bb-preflop-tree-2026-10-09.json"
RANKS = "AKQJT98765432"
GROUPS = ["open", "vs open", "vs 3bet", "vs 4bet+"]


def our_index(name):
    """Class index of a 169-class name in the profiles' order (pairs on the diagonal, suited above)."""
    a, b = RANKS.index(name[0]), RANKS.index(name[1])
    if len(name) == 2:
        return a * 13 + a
    hi, lo = min(a, b), max(a, b)
    return hi * 13 + lo if name[2] == "s" else lo * 13 + hi


def combos(name):
    return 6 if len(name) == 2 else 4 if name[2] == "s" else 12


def label(code, actions, nearest=False):
    """The tree's action label for a GTO Wizard action code at a node with these labels, or None. With nearest,
    a raise size the tree lacks maps to the tree's closest non-all-in raise."""
    if code == "F":
        return "fold" if "fold" in actions else None
    if code == "C":
        return next((a for a in actions if a.startswith("call") or a == "check"), None)
    if code == "RAI":
        return next((a for a in actions if a.endswith(":all-in")), None)
    amount = round(float(code[1:]) * 1000)
    exact = next((a for a in actions if a.split(":")[:2] == ["raise-to", str(amount)]), None)
    if exact or not nearest:
        return exact
    raises = [a for a in actions if a.startswith("raise-to:") and not a.endswith(":all-in")]
    return min(raises, key=lambda a: abs(int(a.split(":")[1]) - amount), default=None)


def group(history):
    raises = sum(1 for h in history if h.startswith("R"))
    return GROUPS[min(raises, 3)]


def compare(reference, nodes):
    """Per reference node: dict with path, distance, frequencies; matched is False when the tree lacks the node
    or its menu differs in kind; approximate is True when a raise size had to be mapped to the nearest one."""
    classes = reference["classes"]
    index = [our_index(c) for c in classes]
    weights_base = [combos(c) for c in classes]
    out = []
    for node in reference["nodes"]:
        path = []
        ok = True
        approximate = False
        for code in node["history"]:
            parent = nodes.get(tuple(path))
            choice = label(code, parent["actions"], nearest=True) if parent else None
            if choice is None:
                ok = False
                break
            approximate |= choice != label(code, parent["actions"])
            path.append(choice)
        ours = nodes.get(tuple(path)) if ok else None
        labels = [label(a["code"], ours["actions"], nearest=True) for a in node["actions"]] if ours else []
        if ours is None or None in labels or len(set(labels)) != len(ours["actions"]) or len(labels) != len(set(labels)):
            out.append({"node": node, "path": path, "matched": False, "labels": labels})
            continue
        approximate |= labels != [label(a["code"], ours["actions"]) for a in node["actions"]]
        columns = [ours["actions"].index(x) for x in labels]
        weights = [w * r for w, r in zip(weights_base, node["range"])]
        total = sum(weights)
        distance = 0.0
        loss = 0.0
        loss_reference = 0.0
        freq_ours = [0.0] * len(columns)
        freq_ref = [0.0] * len(columns)
        classes_out = []
        for k, w in enumerate(weights):
            if w == 0:
                continue
            row = ours["probabilities"][index[k]]
            mine = [row[col] for col in columns]
            theirs = [a["strategy"][k] for a in node["actions"]]
            evs = [a["ev"][k] for a in node["actions"]]
            best = max(evs)
            tv = sum(abs(x - y) for x, y in zip(mine, theirs)) / 2
            cost = sum(x * (best - e) for x, e in zip(mine, evs))
            for j in range(len(columns)):
                freq_ours[j] += w * mine[j]
                freq_ref[j] += w * theirs[j]
            distance += w * tv
            loss += w * cost
            loss_reference += w * sum(y * (best - e) for y, e in zip(theirs, evs))
            classes_out.append((classes[k], w / total, tv, mine, theirs, cost, evs))
        out.append({
            "node": node, "path": path, "matched": True, "approximate": approximate, "labels": labels,
            "distance": distance / total, "loss": loss / total, "loss_reference": loss_reference / total,
            "ours": [f / total for f in freq_ours], "reference": [f / total for f in freq_ref],
            "classes": classes_out,
        })
    return out


def load(path):
    return {tuple(n["path"]): n for n in json.loads(Path(path).read_text(encoding="utf-8"))["nodes"]}


def summary(results):
    rows = {}
    for g in GROUPS + ["all"]:
        sel = [r for r in results if r["matched"] and (g == "all" or group(r["node"]["history"]) == g)]
        p = sum(r["node"]["line_probability"] for r in sel)
        rows[g] = {
            "nodes": len(sel),
            "weighted": sum(r["node"]["line_probability"] * r["distance"] for r in sel) / p if p else None,
            "mean": sum(r["distance"] for r in sel) / len(sel) if sel else None,
            "loss_bb100": 100 * sum(r["node"]["line_probability"] * r["loss"] for r in sel),
            "loss_reference_bb100": 100 * sum(r["node"]["line_probability"] * r["loss_reference"] for r in sel),
        }
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference", type=Path, default=REFERENCE,
                        help="GTO Wizard reference JSON (override for flat cloud uploads)")
    parser.add_argument("--nodes", type=int, default=0, help="print the N most likely nodes per profile")
    parser.add_argument("--worst", type=int, default=0,
                        help="print the N nodes contributing most EV loss (line probability x loss) per profile")
    parser.add_argument("--detail", action="append", default=[],
                        help="print the classes costing most EV at this GTO Wizard history "
                             "(e.g. F-F-F-R2.5-F; 'root' for the first decision); repeatable")
    parser.add_argument("--json", help="write the summaries and per-node distances as JSON")
    parser.add_argument("profiles", nargs="+")
    args = parser.parse_args()
    reference = json.loads(args.reference.read_text(encoding="utf-8"))
    results = {}
    for spec in args.profiles:
        name, path = spec.split("=", 1)
        results[name] = compare(reference, load(path))
    names = list(results)
    print("| group | nodes | " + " | ".join(f"{n} distance weighted / mean / EV loss bb/100" for n in names)
          + " | GTOW's own EV loss bb/100 |")
    print("|---|---|" + "---|" * len(names) + "---|")
    sums = {n: summary(r) for n, r in results.items()}
    for g in GROUPS + ["all"]:
        print(f"| {g} | {sums[names[0]][g]['nodes']} | "
              + " | ".join(f"{sums[n][g]['weighted']:.3f} / {sums[n][g]['mean']:.3f} / {sums[n][g]['loss_bb100']:.2f}"
                           for n in names)
              + f" | {sums[names[0]][g]['loss_reference_bb100']:.2f} |")
    unmatched = [r for r in results[names[0]] if not r["matched"]]
    approximate = [r for r in results[names[0]] if r.get("approximate")]
    print(f"\nmatched {len(results[names[0]]) - len(unmatched)} of {len(results[names[0]])} reference nodes; "
          f"{len(approximate)} only with a raise size mapped to the nearest one")
    for r in approximate:
        print(f"- approximate: {'-'.join(r['node']['history'])} ({r['node']['actor']}, p={r['node']['line_probability']:.4f})"
              f" path {r['path']}, labels {r['labels']}")
    for r in unmatched:
        print(f"- unmatched: {'-'.join(r['node']['history']) or 'root'} ({r['node']['actor']}, "
              f"p={r['node']['line_probability']:.4f}): path {r['path']}, labels {r['labels']}")
    listings = [(args.nodes, "most likely nodes", lambda r: -r["node"]["line_probability"]),
                (args.worst, "nodes contributing most EV loss", lambda r: -r["node"]["line_probability"] * r["loss"])]
    for count, title, key in listings:
        for n in names if count else []:
            print(f"\n{n}: the {count} {title} (frequencies on GTO Wizard's range, ours / GTOW)\n")
            print("| line p | actor | history | distance | EV loss bb/hand at node | contribution bb/100 | actions |")
            print("|---|---|---|---|---|---|---|")
            for r in sorted((r for r in results[n] if r["matched"]), key=key)[:count]:
                acts = ", ".join(f"{a['code']} {o:.2f}/{g:.2f}"
                                 for a, o, g in zip(r["node"]["actions"], r["ours"], r["reference"]))
                print(f"| {r['node']['line_probability']:.4f} | {r['node']['actor']} | "
                      f"{'-'.join(r['node']['history']) or 'root'} | {r['distance']:.3f} | {r['loss']:.4f} | "
                      f"{100 * r['node']['line_probability'] * r['loss']:.3f} | {acts} |")
    for history in args.detail:
        key = [] if history == "root" else history.split("-")
        for n in names:
            r = next(r for r in results[n] if r["node"]["history"] == key)
            codes = [a["code"] for a in r["node"]["actions"]]
            print(f"\n{n} at {history} ({r['node']['actor']}): the 20 classes costing most EV "
                  f"(ours / GTOW strategy and GTO Wizard's EV in bb per action {', '.join(codes)})\n")
            print("| class | range share | TV | EV loss bb | share of node loss | ours | GTOW | GTOW EV |")
            print("|---|---|---|---|---|---|---|---|")
            for c, w, tv, mine, theirs, cost, evs in sorted(r["classes"], key=lambda x: -x[1] * x[5])[:20]:
                share = w * cost / r["loss"] if r["loss"] else 0.0
                print(f"| {c} | {w:.4f} | {tv:.2f} | {cost:.3f} | {share:.3f} | "
                      f"{' '.join(f'{x:.2f}' for x in mine)} | {' '.join(f'{x:.2f}' for x in theirs)} | "
                      f"{' '.join(f'{x:.3f}' for x in evs)} |")
    if args.json:
        Path(args.json).write_text(json.dumps({
            n: {"summary": sums[n], "nodes": [
                {"history": r["node"]["history"], "matched": r["matched"], "approximate": r.get("approximate"),
                 "distance": r.get("distance"), "loss": r.get("loss"), "loss_reference": r.get("loss_reference"),
                 "ours": r.get("ours"), "reference": r.get("reference")} for r in rs]}
            for n, rs in results.items()}, indent=1) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
