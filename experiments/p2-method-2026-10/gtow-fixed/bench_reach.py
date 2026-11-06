"""Reach and class weights of the 36 benchmark leaves under a fixed preflop profile.

usage: python bench_reach.py <profile> <lines file> --out <reach.json>
       [--reference GTOW.json [--extras-out FILE] [--margin 0.5] [--extra-weight 0.001]]

Per leaf (L01..L36, the order of the `;`-joined lines file) and active seat: band (number of raises: 1 srp,
2 3bet, 3 4bet, 4+ 5bet), role (the last raiser is `raiser`), and per class (p1_leaf.classes() order) n(c), the
own reach before the seat's last action, the own reach to the leaf, and the weight
w = n(c) x reach before the last action x the probability that every other seat takes its line actions
(each action's frequency over that seat's range at the time).

With --reference (GTO Wizard preflop): border classes have reach before the last action > 1e-6, leaf reach
<= 1e-6 and GTO Wizard EV(best) - EV(line action) <= margin at that decision. Their combos and weights are
summed per band; --extras-out writes `LINE SEAT CLASS WEIGHT` rows for p1_leaf.py --extra.
"""
import argparse
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "b4s-8h"))
from compare import combos, label, our_index  # noqa: E402
from flop_compare import hero_details  # noqa: E402
from p1_leaf import SEATS, classes  # noqa: E402

NAMES = list(classes())
N = [combos(c) for c in NAMES]
BANDS = {1: "srp", 2: "3bet", 3: "4bet"}


def band(labels):
    raises = sum(1 for x in labels if x.startswith("raise-to:"))
    return BANDS.get(raises, "5bet")


def read_lines(path):
    return [x for x in Path(path).read_text(encoding="utf-8").strip().split(";") if x]


def reference_paths(reference, nodes):
    """{trunk path tuple: reference node} for reference nodes reachable in the trunk (nearest raise sizes)."""
    out = {}
    for node in reference["nodes"]:
        path = []
        for code in node["history"]:
            parent = nodes.get(tuple(path))
            choice = label(code, parent["actions"], nearest=True) if parent else None
            if choice is None:
                break
            path.append(choice)
        else:
            out.setdefault(tuple(path), node)
    return out


def leaf_reach(nodes, labels):
    """Per seat on the line: (reach before its last action, reach to the leaf, index of its last action)."""
    reach = {}
    last = {}
    for k, x in enumerate(labels):
        last[nodes[tuple(labels[:k])]["actor"]] = k
    before = {}
    for k, x in enumerate(labels):
        node = nodes[tuple(labels[:k])]
        seat = node["actor"]
        a = node["actions"].index(x)
        r = reach.setdefault(seat, [1.0] * 169)
        if k == last[seat]:
            before[seat] = list(r)
        for c, name in enumerate(NAMES):
            r[c] *= node["probabilities"][our_index(name)][a]
    return {s: (before[s], reach[s], last[s]) for s in reach}


def line_probability(nodes, labels, hero):
    """Probability that every seat other than hero takes its actions on the line."""
    reach = {}
    p = 1.0
    for k, x in enumerate(labels):
        node = nodes[tuple(labels[:k])]
        seat = node["actor"]
        a = node["actions"].index(x)
        r = reach.setdefault(seat, [1.0] * 169)
        if seat != hero:
            den = sum(n * r[c] for c, n in enumerate(N))
            num = sum(n * r[c] * node["probabilities"][our_index(NAMES[c])][a] for c, n in enumerate(N))
            p *= num / den if den else 0.0
        for c, name in enumerate(NAMES):
            r[c] *= node["probabilities"][our_index(name)][a]
    return p


def build(profile, lines, reference=None, margin=0.5, extra_weight=0.001):
    nodes = {tuple(n["path"]): n for n in profile["nodes"]}
    ref_paths = reference_paths(reference, nodes) if reference else {}
    ref_index = {c: k for k, c in enumerate(reference["classes"])} if reference else {}
    leaves = []
    border = {}
    extras = []
    for i, line in enumerate(lines, 1):
        labels = line.split("/")
        b = band(labels)
        per_seat = leaf_reach(nodes, labels)
        seats = {}
        for role in ("caller", "raiser"):
            seat = hero_details(nodes, labels, role)[0]
            before, leaf, k = per_seat[seat]
            opp = line_probability(nodes, labels, seat)
            weight = [N[c] * before[c] * opp for c in range(169)]
            entry = dict(position=SEATS[seat], role=role, n=N, reach_before=before, reach_leaf=leaf,
                         opponent_probability=opp, weight=weight)
            if reference:
                ref = ref_paths.get(tuple(labels[:k]))
                found = []
                if ref is not None:
                    actions = nodes[tuple(labels[:k])]["actions"]
                    mine = [a for a in ref["actions"] if label(a["code"], actions, nearest=True) == labels[k]]
                    if mine:
                        for c, name in enumerate(NAMES):
                            if before[c] > 1e-6 and leaf[c] <= 1e-6:
                                j = ref_index[name]
                                best = max(a["ev"][j] for a in ref["actions"])
                                if best - mine[0]["ev"][j] <= margin:
                                    found.append(c)
                entry["border"] = [NAMES[c] for c in found]
                total = border.setdefault(b, dict(combos=0, weight=0.0))
                total["combos"] += sum(N[c] for c in found)
                total["weight"] += sum(weight[c] for c in found)
                extras += [f"{line} {SEATS[seat]} {NAMES[c]} {extra_weight}" for c in found]
            seats[str(seat)] = entry
        leaves.append(dict(leaf=f"L{i:02d}", index=i, line=line, band=b, seats=seats))
    result = dict(format="p2-bench-reach", version=1, classes=NAMES, leaves=leaves)
    if reference:
        result.update(border=border, margin=margin, extra_weight=extra_weight)
    return result, extras


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("profile")
    parser.add_argument("lines")
    parser.add_argument("--out", required=True)
    parser.add_argument("--reference")
    parser.add_argument("--extras-out")
    parser.add_argument("--margin", type=float, default=0.5)
    parser.add_argument("--extra-weight", type=float, default=0.001)
    args = parser.parse_args()
    profile = json.loads(Path(args.profile).read_text(encoding="utf-8"))
    reference = json.loads(Path(args.reference).read_text(encoding="utf-8")) if args.reference else None
    result, extras = build(profile, read_lines(args.lines), reference, args.margin, args.extra_weight)
    result["profile"] = args.profile
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(result) + "\n", encoding="utf-8", newline="\n")
    if args.extras_out:
        Path(args.extras_out).write_text("".join(x + "\n" for x in extras), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
