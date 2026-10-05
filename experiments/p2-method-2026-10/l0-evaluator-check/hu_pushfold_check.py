"""Independent check of the L0 evaluator on heads-up push/fold (benchmark B1).

The heads-up L0 model is the input game itself: the deal is proportional to the product of
both range weights over disjoint combos. With class-level strategies, the button (seat 0, posts
the small blind) either folds or moves all in, and the big blind (seat 1) folds or calls.
This script computes each seat's value, best-response value and gain in big blinds from

- the class table exported by `trunk_tables --export-classes` (class, name, n, rep_combo), and
- the exact showdown counts exported by `trunk_tables --export-t2`
  (`w_win`, `w_tie`, `w_lose` per hero class and villain class),

using closed-form push/fold formulas written for this script only. It shares no game, tree,
settlement or best-response code with the Rust evaluator. Only the Python standard library is used.

Subcommands:
- `profile`: write a class profile JSON (uniform, seeded random, or push/call the top classes).
- `profile-from-strategy`: convert a legacy solution's strategy CSV export
  (`solvers export <solution.mwsol> strategy --format csv`) into a class profile JSON. Classes absent
  from the export get the uniform row, as in the Rust evaluator.
- `fictitious-play`: write the average profile of fictitious play from uniform strategies, a profile
  near the push/fold equilibrium.
- `evaluate`: evaluate a class profile and write the result JSON.
- `compare`: compare this script's result with the Rust evaluator's result JSON.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import random
import struct

BOARDS_PER_PAIR = math.comb(48, 5)
SMALL_BLIND = 0.5
BIG_BLIND = 1.0


def load_classes(path):
    with open(path, newline="", encoding="utf-8") as handle:
        rows = sorted(csv.DictReader(handle), key=lambda row: int(row["class"]))
    if [int(row["class"]) for row in rows] != list(range(169)):
        raise SystemExit("class table must list classes 0..168")
    return [row["name"] for row in rows], [int(row["n"]) for row in rows]


def load_t2(path, sizes):
    """Return K[c][d] and the expected win/lose counts T2[c][d] over villain combos."""
    k = [[0] * 169 for _ in range(169)]
    win = [[0.0] * 169 for _ in range(169)]
    lose = [[0.0] * 169 for _ in range(169)]
    seen = 0
    with open(path, newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            c, d = int(row["hero_class"]), int(row["villain_class"])
            if int(row["n_hero"]) != sizes[c]:
                raise SystemExit(f"class size mismatch for class {c}")
            counts = [int(row[name]) for name in ("w_win", "w_tie", "w_lose")]
            total = sizes[c] * int(row["k"]) * BOARDS_PER_PAIR
            if sum(counts) != total:
                raise SystemExit(f"counts of ({c}, {d}) do not sum to n*K*C(48,5)")
            k[c][d] = int(row["k"])
            win[c][d] = counts[0] / (sizes[c] * BOARDS_PER_PAIR)
            lose[c][d] = counts[2] / (sizes[c] * BOARDS_PER_PAIR)
            seen += 1
    if seen != 169 * 169:
        raise SystemExit(f"expected {169 * 169} rows, found {seen}")
    for c in range(169):
        if sum(k[c]) != 1225:
            raise SystemExit(f"K row {c} does not sum to 1225")
    return k, win, lose


def push_fold_values(stack, weights, push, call, k, win, lose):
    """Per-class values of both seats, scaled by the opponent's disjoint weight mass."""
    w0, w1 = weights
    button, button_br, big, big_br = [], [], [], []
    for c in range(169):
        fold = -SMALL_BLIND * sum(k[c][d] * w1[d] for d in range(169))
        shove = 0.0
        for d in range(169):
            steal = k[c][d] * w1[d] * (1.0 - call[d]) * BIG_BLIND
            showdown = w1[d] * call[d] * stack * (win[c][d] - lose[c][d])
            shove += steal + showdown
        button.append((1.0 - push[c]) * fold + push[c] * shove)
        button_br.append(max(fold, shove))
    for d in range(169):
        walk = sum(w0[c] * k[d][c] * (1.0 - push[c]) * SMALL_BLIND for c in range(169))
        fold = sum(w0[c] * k[d][c] * push[c] * -BIG_BLIND for c in range(169))
        called = sum(w0[c] * push[c] * stack * (win[d][c] - lose[d][c]) for c in range(169))
        big.append(walk + (1.0 - call[d]) * fold + call[d] * called)
        big_br.append(walk + max(fold, called))
    return button, button_br, big, big_br


def evaluate(args):
    names, sizes = load_classes(args.classes)
    k, win, lose = load_t2(args.t2, sizes)
    profile = json.load(open(args.profile, encoding="utf-8"))
    push, call = read_profile(profile)
    weights = (read_weights(args.button_weights), read_weights(args.big_blind_weights))
    button, button_br, big, big_br = push_fold_values(args.stack, weights, push, call, k, win, lose)
    w0, w1 = weights
    mass = sum(sizes[c] * w0[c] * sum(k[c][d] * w1[d] for d in range(169)) for c in range(169))
    if mass <= 0:
        raise SystemExit("ranges have no disjoint combo pair")
    seats = []
    for seat, (values, best, own) in enumerate(((button, button_br, w0), (big, big_br, w1))):
        value = sum(sizes[c] * own[c] * values[c] for c in range(169)) / mass
        best_response = sum(sizes[c] * own[c] * best[c] for c in range(169)) / mass
        seats.append({"seat": seat, "value": value, "best_response": best_response, "gain": best_response - value})
    result = {
        "format": "p2-hu-pushfold-check",
        "version": 1,
        "stack_bb": args.stack,
        "profile": args.profile,
        "seats": seats,
        "nash_conv": sum(seat["gain"] for seat in seats),
        "zero_sum_residual": seats[0]["value"] + seats[1]["value"],
    }
    with open(args.output, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(result, handle, indent=2)
        handle.write("\n")
    print(json.dumps({key: result[key] for key in ("nash_conv", "zero_sum_residual")}))


def read_weights(spec):
    if spec in (None, "random"):
        return [1.0] * 169
    values = [float(x) for x in open(spec, encoding="utf-8").read().split()]
    if len(values) != 169:
        raise SystemExit(f"{spec}: expected 169 class weights")
    return values


def read_profile(profile):
    """Return push[c] and call[d] from a p2-class-profile document."""
    if profile.get("format") != "p2-class-profile" or profile.get("version") != 1:
        raise SystemExit("profile must be p2-class-profile version 1")
    push = call = None
    for node in profile["nodes"]:
        aggressive = [i for i, label in enumerate(node["actions"]) if label != "fold"]
        if len(node["actions"]) != 2 or "fold" not in node["actions"] or len(aggressive) != 1:
            raise SystemExit(f"unexpected actions {node['actions']}")
        column = aggressive[0]
        probabilities = [row[column] / sum(row) for row in node["probabilities"]]
        if node["path"] == [] and node["actor"] == 0:
            push = probabilities
        elif len(node["path"]) == 1 and node["actor"] == 1:
            call = probabilities
        else:
            raise SystemExit(f"unexpected node {node['path']} for push/fold")
    if push is None or call is None:
        raise SystemExit("profile must contain the button root and the big-blind response")
    return push, call


def write_profile(args):
    tree = json.load(open(args.tree, encoding="utf-8"))
    nodes = [node for node in tree["nodes"] if node.get("actor") is not None]
    rng = random.Random(args.seed)
    names, sizes = load_classes(args.classes)
    order = sorted(range(169), key=lambda c: args.ranking.index(names[c]) if args.ranking else c)
    out = []
    for node in nodes:
        rows = []
        for c in range(169):
            if args.kind == "uniform":
                rows.append([1.0 / len(node["actions"])] * len(node["actions"]))
            elif args.kind == "random":
                values = [rng.random() for _ in node["actions"]]
                rows.append([value / sum(values) for value in values])
            else:
                fraction = args.push if node["path"] == [] else args.call
                aggressive = order.index(c) < round(fraction * 169)
                rows.append([0.0 if label == "fold" else 1.0 for label in node["actions"]] if aggressive
                            else [1.0 if label == "fold" else 0.0 for label in node["actions"]])
        out.append({"path": node["path"], "actor": node["actor"], "actions": node["actions"], "probabilities": rows})
    with open(args.output, "w", encoding="utf-8", newline="\n") as handle:
        json.dump({"format": "p2-class-profile", "version": 1, "nodes": out}, handle)
        handle.write("\n")


def profile_from_strategy(args):
    tree = json.load(open(args.tree, encoding="utf-8"))
    nodes = [node for node in tree["nodes"] if node.get("actor") is not None]
    by_actor = {node["actor"]: node for node in nodes}
    if len(nodes) != 2 or sorted(by_actor) != [0, 1]:
        raise SystemExit("expected the push/fold tree: one decision node for each seat")
    rows = {0: {}, 1: {}}
    with open(args.strategy, newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            if int(row["street"]) != 0:
                raise SystemExit("push/fold solutions have no postflop strategy")
            actor = int(row["actor"])
            expected_root = actor == 0
            if (row["history"] == "0" * 32) != expected_root:
                raise SystemExit(f"seat {actor} strategy at unexpected history {row['history']}")
            c = json.loads(row["bucket_path"])[0]
            # The export prints the stored f32 in its shortest form; recover that exact f32.
            probability = struct.unpack("<f", struct.pack("<f", float(row["probability"])))[0]
            rows[actor].setdefault(c, {})[row["action"]] = probability
    out = []
    for actor in (0, 1):
        node = by_actor[actor]
        probabilities = []
        for c in range(169):
            stored = rows[actor].get(c)
            if stored is None:
                probabilities.append([1.0 / len(node["actions"])] * len(node["actions"]))
                continue
            unknown = set(stored) - set(node["actions"])
            if unknown:
                raise SystemExit(f"unknown actions {sorted(unknown)} for seat {actor}")
            probabilities.append([stored.get(label, 0.0) for label in node["actions"]])
        out.append({"path": node["path"], "actor": actor, "actions": node["actions"], "probabilities": probabilities})
    defaulted = {actor: 169 - len(rows[actor]) for actor in (0, 1)}
    with open(args.output, "w", encoding="utf-8", newline="\n") as handle:
        json.dump({"format": "p2-class-profile", "version": 1, "nodes": out}, handle)
        handle.write("\n")
    print(json.dumps({"defaulted_classes": defaulted}))


def fictitious_play(args):
    tree = json.load(open(args.tree, encoding="utf-8"))
    nodes = [node for node in tree["nodes"] if node.get("actor") is not None]
    names, sizes = load_classes(args.classes)
    k, win, lose = load_t2(args.t2, sizes)
    push = [0.5] * 169
    call = [0.5] * 169
    for t in range(1, args.iterations + 1):
        # Pure best responses to the current averages; ties fold.
        best_push = []
        for c in range(169):
            fold = -SMALL_BLIND * sum(k[c])
            shove = sum(k[c][d] * (1.0 - call[d]) * BIG_BLIND + call[d] * args.stack * (win[c][d] - lose[c][d])
                        for d in range(169))
            best_push.append(1.0 if shove > fold else 0.0)
        best_call = []
        for d in range(169):
            fold = sum(k[d][c] * push[c] * -BIG_BLIND for c in range(169))
            called = sum(push[c] * args.stack * (win[d][c] - lose[d][c]) for c in range(169))
            best_call.append(1.0 if called > fold else 0.0)
        step = 1.0 / (t + 1)
        push = [(1.0 - step) * p + step * q for p, q in zip(push, best_push)]
        call = [(1.0 - step) * p + step * q for p, q in zip(call, best_call)]
    out = []
    for node in nodes:
        probabilities = push if node["path"] == [] else call
        rows = [[1.0 - probabilities[c] if label == "fold" else probabilities[c] for label in node["actions"]]
                for c in range(169)]
        out.append({"path": node["path"], "actor": node["actor"], "actions": node["actions"], "probabilities": rows})
    with open(args.output, "w", encoding="utf-8", newline="\n") as handle:
        json.dump({"format": "p2-class-profile", "version": 1, "nodes": out}, handle)
        handle.write("\n")
    combos = sum(sizes)
    print(json.dumps({
        "push_fraction": sum(n * p for n, p in zip(sizes, push)) / combos,
        "call_fraction": sum(n * p for n, p in zip(sizes, call)) / combos,
    }))


def compare(args):
    mine = json.load(open(args.python, encoding="utf-8"))
    theirs = json.load(open(args.rust, encoding="utf-8"))
    worst = 0.0
    for a, b in zip(mine["seats"], theirs["seats"], strict=True):
        for key in ("value", "best_response", "gain"):
            difference = abs(a[key] - b[key])
            worst = max(worst, difference)
            print(f"seat {a['seat']} {key}: python={a[key]:.12f} rust={b[key]:.12f} diff={difference:.3e}")
    difference = abs(mine["nash_conv"] - theirs["nash_conv"])
    worst = max(worst, difference)
    print(f"nash_conv: python={mine['nash_conv']:.12f} rust={theirs['nash_conv']:.12f} diff={difference:.3e}")
    if worst > args.tolerance:
        raise SystemExit(f"FAIL: largest difference {worst:.3e} exceeds {args.tolerance:.1e}")
    print(f"PASS: largest difference {worst:.3e}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    profile = commands.add_parser("profile")
    profile.add_argument("--tree", required=True, help="decision-node export of the Rust evaluator")
    profile.add_argument("--classes", required=True)
    profile.add_argument("--kind", choices=("uniform", "random", "top"), required=True)
    profile.add_argument("--seed", type=int, default=0)
    profile.add_argument("--push", type=float, default=0.5, help="top: fraction of classes that move all in")
    profile.add_argument("--call", type=float, default=0.3, help="top: fraction of classes that call")
    profile.add_argument("--ranking", nargs="*", default=None, help="top: class names from strongest")
    profile.add_argument("--output", required=True)
    converted = commands.add_parser("profile-from-strategy")
    converted.add_argument("--tree", required=True, help="decision-node export of the Rust evaluator")
    converted.add_argument("--strategy", required=True, help="strategy CSV export of a legacy solution")
    converted.add_argument("--output", required=True)
    play = commands.add_parser("fictitious-play")
    play.add_argument("--tree", required=True, help="decision-node export of the Rust evaluator")
    play.add_argument("--classes", required=True)
    play.add_argument("--t2", required=True)
    play.add_argument("--stack", type=float, required=True, help="starting stack of both seats in BB")
    play.add_argument("--iterations", type=int, default=400)
    play.add_argument("--output", required=True)
    evaluation = commands.add_parser("evaluate")
    evaluation.add_argument("--classes", required=True)
    evaluation.add_argument("--t2", required=True)
    evaluation.add_argument("--profile", required=True)
    evaluation.add_argument("--stack", type=float, required=True, help="starting stack of both seats in BB")
    evaluation.add_argument("--button-weights", default=None, help="file with 169 class weights (default random)")
    evaluation.add_argument("--big-blind-weights", default=None, help="file with 169 class weights (default random)")
    evaluation.add_argument("--output", required=True)
    comparison = commands.add_parser("compare")
    comparison.add_argument("--python", required=True)
    comparison.add_argument("--rust", required=True)
    comparison.add_argument("--tolerance", type=float, default=1e-9)
    args = parser.parse_args()
    handlers = {
        "profile": write_profile,
        "profile-from-strategy": profile_from_strategy,
        "fictitious-play": fictitious_play,
        "evaluate": evaluate,
        "compare": compare,
    }
    handlers[args.command](args)


if __name__ == "__main__":
    main()
