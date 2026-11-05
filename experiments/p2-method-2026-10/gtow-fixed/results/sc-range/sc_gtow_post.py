"""GTO Wizard's own preflop leaf value vs its flop solutions, per hand class, for two SRP lines.

usage: python sc_gtow_post.py <preflop raw dir> <flops dir> [--json OUT]

<preflop raw dir> holds GTO Wizard's `v4/solutions/spot-solution` responses per preflop node (the crawl behind
`b4s-8h/references/gtowizard-simple-100bb-preflop-tree-2026-10-09.json`); <flops dir>/<line>/<board>.json holds the
flop-root responses of the same solution (Cash6m50zSimple 100bb) for the line, one per sampled flop. Both stay
local-only (GTO Wizard values).

Values are per hand in bb as "pot share minus the chips the seat put in preflop".
- Flop: each flop's class `hand_ev` (the seat's expected pot share after rake) weighted by the class's range combos
  on that flop (own-card removal), flops equally weighted (a raw-count proportional sample). +- is the flop
  jackknife standard error.
- Preflop, caller: the call EV of the class (fold = 0) at the caller's node, shifted to the same scale.
- Preflop, opener: implied by the class's open EV. The open EV is the sum over the opponents' replies of
  P(reply | opener's cards) x value; the 3bet branches use the opener's own continuation EV at the next node,
  and the call branch is solved for the leaf value. The replies' probabilities spread the opponents' class
  ranges and strategies over combos with card removal against the opener's hand, each other and the seats that
  folded before the open (their hands are sampled from their fold ranges; a combo's weight is multiplied by the
  probability that neither of its cards is dead). Without the folded seats the implied values move by up to
  0.3 bb for small cards and the opener's range no longer adds up with the caller's to the pot.
"""
import argparse
import functools
import itertools
import json
from pathlib import Path

import numpy as np

RANKS = "23456789TJQKA"
COMBOS = list(itertools.combinations(range(52), 2))
SAMPLES = 4000
SC = ["T9s", "98s", "87s", "76s", "65s", "54s"]
# reply: (probability factors as (node, action) pairs, kind, value) - kind "win" (opponents fold, value = net chips
# won), "leaf" (the call), or "node" (the opener's continuation EV at that node). "folded": the nodes of the seats
# that folded before the open.
LINES = {
    "L04": {
        "open": ("F-F-F", "R2.5"), "opener": "BTN", "caller": "BB", "call_node": "F-F-F-R2.5-F",
        "opener_in": 2.5, "caller_in": 2.5, "posted": {"BTN": 0.0, "BB": 1.0}, "folded": ["root", "F", "F-F"],
        "replies": [
            ([("F-F-F-R2.5", "F"), ("F-F-F-R2.5-F", "F")], "win", 1.5),
            ([("F-F-F-R2.5", "F"), ("F-F-F-R2.5-F", "C")], "leaf", None),
            ([("F-F-F-R2.5", "F"), ("F-F-F-R2.5-F", "R13")], "node", "F-F-F-R2.5-F-R13"),
            ([("F-F-F-R2.5", "R12"), ("F-F-F-R2.5-R12", "F")], "node", "F-F-F-R2.5-R12-F"),
            ([("F-F-F-R2.5", "R12"), ("F-F-F-R2.5-R12", "R23")], "node", "F-F-F-R2.5-R12-R23"),
        ],
    },
    "L02": {
        "open": ("F-F-F-F", "R3"), "opener": "SB", "caller": "BB", "call_node": "F-F-F-F-R3",
        "opener_in": 3.0, "caller_in": 3.0, "posted": {"SB": 0.5, "BB": 1.0},
        "folded": ["root", "F", "F-F", "F-F-F"],
        "replies": [
            ([("F-F-F-F-R3", "F")], "win", 1.0),
            ([("F-F-F-F-R3", "C")], "leaf", None),
            ([("F-F-F-F-R3", "R10")], "node", "F-F-F-F-R3-R10"),
        ],
    },
}


def class_of(a, b):
    ra, rb = sorted((a // 4, b // 4), reverse=True)
    if ra == rb:
        return RANKS[ra] * 2
    return RANKS[ra] + RANKS[rb] + ("s" if a % 4 == b % 4 else "o")


@functools.lru_cache(maxsize=None)
def load_node(raw, name):
    d = json.loads((raw / f"{name}.json").read_text(encoding="utf-8"))
    hero = next(p for p in d["players_info"] if p["player"]["is_hero"])
    classes = list(hero["simple_hand_counters"])
    acts = {a["action"]["code"]: (np.array(a["strategy"]), np.array(a["evs"])) for a in d["action_solutions"]}
    return classes, np.array(hero["range"]), acts


def alive(raw, hand, folded, holds, combo_class, rng):
    """Per combo, the probability that neither card is held by a seat that folded before the open."""
    dead = np.zeros((SAMPLES, 52), dtype=bool)
    dead[:, list(COMBOS[hand])] = True
    for node in folded:
        _, weight, acts = load_node(raw, node)
        p = (weight * acts["F"][0])[combo_class]
        p = p / p.sum()
        pick = np.full(SAMPLES, -1)
        todo = np.arange(SAMPLES)
        while len(todo):
            draw = rng.choice(len(COMBOS), size=len(todo), p=p)
            ok = ~(dead[todo] & holds[draw]).any(axis=1)
            pick[todo[ok]] = draw[ok]
            todo = todo[~ok]
        dead |= holds[pick]
    first = [a for a, _ in COMBOS]
    second = [b for _, b in COMBOS]
    return 1.0 - (dead[:, first] | dead[:, second]).mean(axis=0)


def reply_probabilities(raw, replies, folded, classes, seed=0):
    """P(reply | opener class); the opener holds one representative combo per class."""
    rng = np.random.default_rng(seed)
    index = {c: i for i, c in enumerate(classes)}
    combo_class = np.array([index[class_of(a, b)] for a, b in COMBOS])
    holds = np.zeros((len(COMBOS), 52), dtype=bool)
    for i, (a, b) in enumerate(COMBOS):
        holds[i, a] = holds[i, b] = True
    overlap = (holds.astype(float) @ holds.T.astype(float)) > 0
    rep = {}
    for i in range(len(COMBOS)):
        rep.setdefault(combo_class[i], i)
    out = np.zeros((len(replies), len(classes)))
    for c, h in rep.items():
        free = ~overlap[h] * alive(raw, h, folded, holds, combo_class, rng)
        for k, (factors, _, _) in enumerate(replies):
            weights = []
            for node, action in factors:
                _, weight, acts = load_node(raw, node)
                weights.append((weight * acts[action][0])[combo_class] * free)
            if len(weights) == 1:
                out[k, c] = weights[0].sum() / free.sum()
            else:
                w1, w2 = weights
                out[k, c] = (w1.sum() * w2.sum() - w1 @ overlap @ w2) / (free.sum() ** 2 - free @ overlap @ free)
    return out


def flop_matrix(folder, seat, classes):
    """(range combos, pot share) per flop x class from the flop-root responses."""
    combos, share = [], []
    for f in sorted(folder.glob("*.json")):
        d = json.loads(f.read_text(encoding="utf-8"))
        p = next(x for x in d["players_info"] if x["player"]["position"] == seat)
        counters = p["simple_hand_counters"]
        combos.append([counters[c]["total_combos"] for c in classes])
        share.append([counters[c]["hand_ev"] for c in classes])
    return np.array(combos), np.array(share)


def flop_value(combos, share, keep):
    n = combos[keep].sum(axis=0)
    with np.errstate(invalid="ignore", divide="ignore"):
        return (combos[keep] * share[keep]).sum(axis=0) / n


def jackknife(combos, share, statistic):
    """Statistic over all flops and its leave-one-flop-out standard error."""
    flops = len(combos)
    full = statistic(flop_value(combos, share, np.ones(flops, dtype=bool)))
    loo = np.array([statistic(flop_value(combos, share, np.arange(flops) != i)) for i in range(flops)])
    return full, np.sqrt((flops - 1) / flops * ((loo - loo.mean(axis=0)) ** 2).sum(axis=0))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("raw")
    ap.add_argument("flops")
    ap.add_argument("--json")
    args = ap.parse_args()
    raw, flops = Path(args.raw), Path(args.flops)
    result = {}
    for line, spec in LINES.items():
        opener = spec["opener"]
        classes, open_range, open_acts = load_node(raw, spec["open"][0])
        _, call_range, call_acts = load_node(raw, spec["call_node"])
        probs = reply_probabilities(raw, spec["replies"], spec["folded"], classes)
        rest = np.zeros(len(classes))
        p_leaf = None
        for k, (_, kind, value) in enumerate(spec["replies"]):
            if kind == "win":
                rest += probs[k] * (value + spec["posted"][opener])
            elif kind == "node":
                _, _, acts = load_node(raw, value)
                cont = sum(s * e for s, e in acts.values())
                # The opener's EV at the node is from folding there (fold = 0, i.e. net -chips in) on.
                rest += probs[k] * (cont - spec["opener_in"] + spec["posted"][opener])
            else:
                p_leaf = probs[k]
        # open EV (fold = 0) = rest + p_leaf * (leaf net + posted); leaf net = share - opener_in
        opener_pre = (open_acts[spec["open"][1]][1] - rest) / p_leaf - spec["posted"][opener]
        caller_pre = call_acts["C"][1] - spec["posted"]["BB"]
        combos = np.array([6 if len(c) == 2 else 4 if c[2] == "s" else 12 for c in classes])
        sc = np.array([c in SC for c in classes])
        premium = np.array([c in ("AA", "KK", "QQ", "AKs", "AKo") for c in classes])
        result[line] = {}
        print(f"## {line}")
        for seat, pre, weight in ((opener, opener_pre, open_range * open_acts[spec["open"][1]][0] * p_leaf),
                                  ("BB", caller_pre, call_range * call_acts["C"][0])):
            n, share = flop_matrix(flops / line, seat, classes)
            put = spec["opener_in"] if seat == opener else spec["caller_in"]
            mass = weight * combos
            flop, se = jackknife(n, share, lambda v: v - put)
            groups = {}
            for name, mask in (("range", mass > 0), ("SC", sc & (mass > 0)), ("premium", premium & (mass > 0)),
                               ("other", ~sc & ~premium & (mass > 0))):
                w = np.where(mask, mass, 0.0)
                if not mask.any():
                    continue
                g = jackknife(n, share, lambda v, w=w: float(np.nansum(w * (v - put - pre)) / w.sum()))
                groups[name] = {"difference": float(g[0]), "se": float(g[1])}
            result[line][seat] = {
                "flops": len(n), "groups": groups,
                "classes": {c: {"weight": float(weight[i]), "preflop": float(pre[i]), "flop": float(flop[i]),
                                "se": float(se[i]), "difference": float(flop[i] - pre[i])}
                            for i, c in enumerate(classes) if mass[i] > 0},
            }
            print(f"{seat} ({len(n)} flops): " + ", ".join(
                f"{k} {v['difference']:+.3f} ± {v['se']:.3f}" for k, v in groups.items()))
            for c in SC:
                if c in result[line][seat]["classes"]:
                    r = result[line][seat]["classes"][c]
                    print(f"  {c}  weight {r['weight']:.3f}  preflop {r['preflop']:+.3f}  flop {r['flop']:+.3f} "
                          f"± {r['se']:.3f}  difference {r['difference']:+.3f}")
    if args.json:
        Path(args.json).write_text(json.dumps(result, indent=1) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
