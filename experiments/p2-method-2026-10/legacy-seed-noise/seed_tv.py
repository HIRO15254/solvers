"""Compare two P2 strategy exports of the same public tree (seed-to-seed noise).

Inputs are CSV exports produced by `solvers export <solution.mwsol> strategy --format csv`
and `solvers export <solution.mwsol> tree --format csv`. Only the Python standard
library is used.

Reported quantities:
- reach-weighted total variation (TV) of the action distribution per depth, where
  depth is the number of preflop actions before the node. Reach is propagated with
  profile A's average strategy, and every class has the same prior 1/169 (classes
  are not weighted by combo count).
- coverage of (node, class) pairs that only one export stores, and unweighted TV
  statistics per depth.
- unweighted TV at the first-in node of each position.
"""

from __future__ import annotations

import argparse
import collections
import csv

ROOT = "0" * 32
NUM_CLASSES = 169
POSITIONS = {0: "BTN", 1: "SB", 2: "BB", 3: "UTG", 4: "HJ", 5: "CO"}


def load_by_history(path):
    data = collections.defaultdict(dict)
    actors = {}
    with open(path, newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            cls = int(row["bucket_path"].strip("[]").split(",")[0])
            data[row["history"]].setdefault(cls, {})[row["action"]] = float(row["probability"])
            actors[row["history"]] = int(row["actor"])
    return data, actors


def load_by_node(path):
    data = collections.defaultdict(dict)
    with open(path, newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            cls = int(row["bucket_path"].strip("[]").split(",")[0])
            data[(row["history"], int(row["actor"]))][(cls, row["action"])] = float(row["probability"])
    return data


def load_tree(path):
    parent, label = {}, {}
    children = collections.defaultdict(list)
    with open(path, newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            parent[row["history"]] = row["parent"]
            label[row["history"]] = (int(row["actor"]), row["action"])
            children[row["parent"]].append(row["history"])
    return parent, label, children


def depth_of(history, parent):
    depth = 0
    while history != ROOT and history in parent:
        history = parent[history]
        depth += 1
    return depth


def reach_weighted(tree, path_a, path_b, seats):
    parent, label, children = tree
    a, actor_of = load_by_history(path_a)
    b, _ = load_by_history(path_b)
    prior = [1.0 / NUM_CLASSES] * NUM_CLASSES
    reach = {ROOT: [[1.0] * NUM_CLASSES for _ in range(seats)]}
    order = [ROOT]
    index = 0
    while index < len(order):
        history = order[index]
        index += 1
        for child in children.get(history, []):
            seat, action = label[child]
            values = [row[:] for row in reach[history]]
            strategy = a.get(history, {})
            for cls in range(NUM_CLASSES):
                probability = strategy.get(cls, {}).get(action)
                values[seat][cls] *= probability if probability is not None else 0.0
            reach[child] = values
            order.append(child)

    numerator = collections.defaultdict(float)
    denominator = collections.defaultdict(float)
    missing = collections.defaultdict(float)
    for history, strategy_a in a.items():
        if history not in reach:
            continue
        seat = actor_of[history]
        values = reach[history]
        opponents = 1.0
        for other in range(seats):
            if other != seat:
                opponents *= sum(prior[c] * values[other][c] for c in range(NUM_CLASSES))
        depth = depth_of(history, parent)
        strategy_b = b.get(history, {})
        for cls in range(NUM_CLASSES):
            weight = prior[cls] * values[seat][cls] * opponents
            if weight <= 0:
                continue
            if cls in strategy_a and cls in strategy_b:
                actions = set(strategy_a[cls]) | set(strategy_b[cls])
                tv = 0.5 * sum(abs(strategy_a[cls].get(x, 0) - strategy_b[cls].get(x, 0)) for x in actions)
                numerator[depth] += weight * tv
                denominator[depth] += weight
            else:
                missing[depth] += weight
    print("reach-weighted TV (reach from A, uniform class prior)")
    for depth in sorted(denominator):
        print(
            f"depth {depth:2d}: TV={numerator[depth] / denominator[depth]:.4f}"
            f"  reach mass={denominator[depth]:.4f}  mass stored in one export only={missing[depth]:.2e}"
        )
    print(f"overall TV {sum(numerator.values()) / sum(denominator.values()):.4f}")


def coverage_and_first_in(tree, path_a, path_b):
    parent, label, _ = tree
    a = load_by_node(path_a)
    b = load_by_node(path_b)

    def path(history):
        out = []
        while history != ROOT and history in parent:
            out.append(label[history])
            history = parent[history]
        return list(reversed(out))

    by_depth = collections.defaultdict(list)
    counts = collections.Counter()
    for node in set(a) | set(b):
        da, db = a.get(node, {}), b.get(node, {})
        classes_a = {c for c, _ in da}
        classes_b = {c for c, _ in db}
        counts["classes_only_in_one"] += len(classes_a ^ classes_b)
        counts["classes_in_both"] += len(classes_a & classes_b)
        depth = depth_of(node[0], parent)
        for cls in classes_a & classes_b:
            actions = {action for (c, action) in da if c == cls}
            by_depth[depth].append(0.5 * sum(abs(da.get((cls, x), 0) - db.get((cls, x), 0)) for x in actions))
    print(f"(node, class) pairs: in both={counts['classes_in_both']} in one only={counts['classes_only_in_one']}")
    print("unweighted TV over pairs stored in both")
    for depth in sorted(by_depth):
        values = sorted(by_depth[depth])
        mean = sum(values) / len(values)
        print(
            f"depth {depth:2d}: pairs={len(values):7d} mean={mean:.3f}"
            f" median={values[len(values) // 2]:.3f} p90={values[int(len(values) * 0.9)]:.3f}"
        )
    print("first-in nodes (all previous actions fold), unweighted TV over classes stored in A")
    for node in sorted(a):
        if all(action == "fold" for _, action in path(node[0])):
            da, db = a[node], b.get(node, {})
            diffs = []
            for cls in {c for c, _ in da}:
                actions = {action for (c, action) in da if c == cls}
                diffs.append(0.5 * sum(abs(da.get((cls, x), 0) - db.get((cls, x), 0)) for x in actions))
            name = POSITIONS.get(node[1], str(node[1]))
            print(f"first-in {name}: classes={len(diffs)} mean={sum(diffs) / len(diffs):.3f} max={max(diffs):.3f}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tree", required=True, help="tree view CSV")
    parser.add_argument("--a", required=True, help="strategy view CSV of profile A")
    parser.add_argument("--b", required=True, help="strategy view CSV of profile B")
    parser.add_argument("--seats", type=int, default=6)
    args = parser.parse_args()
    tree = load_tree(args.tree)
    reach_weighted(tree, args.a, args.b, args.seats)
    coverage_and_first_in(tree, args.a, args.b)


if __name__ == "__main__":
    main()
