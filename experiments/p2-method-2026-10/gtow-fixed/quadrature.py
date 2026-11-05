"""Quadrature error of K weighted representative flops (B01 pre-check, no P2 solve).

usage: python quadrature.py <random report csv> <medoid report csv> <subsets dir> [--ks 8,25,49,95]
       [--side ev_oop|ev_ip] [--exact ALL_FLOPS_CSV] [--features FLOPS_CSV] [--json OUT]

The random report holds a P1 leaf (`solvers report`) on randomly dealt flops, the medoid report the same
config on every representative flop of subsets/k<K>.txt. For each K:

- `weighted`: the subset's estimate sum_g W_g EV(medoid_g) / sum_g W_g, the value a game restricted to the
  K weighted flops would see, against the random flops' mean (its error is limited by the sample's error).
- `paired`: each random flop's EV is replaced by its group medoid's EV (k<K>-groups.txt). The mean of
  EV(medoid(f)) - EV(f) over the sample estimates the same bias with a paired standard error, and the mean
  absolute value is the per-flop substitution error a shared group strategy cannot avoid.

With --exact (the same config on all 1,755 canonical flops) the exact mean weights each canonical flop by
its raw flops (counted here by suit isomorphism over the 22,100 deals), and `exact_mae` repeats the paired
absolute error over every flop instead of the sample. (The exact paired bias equals `weighted − exact`.)

With --features (the flop_features CSV: raw flops and the 169 classes' mean 128-bucket EHS2 strength per
flop), `fitted` re-weights the same K representatives by nonnegative least squares so that their weighted
class strengths match the all-flop mean, PioSOLVER's idea of matching equities rather than clustering.
The fit never sees an EV, so the leaf EVs are a held-out test of the weights.

All values are the side's leaf EV in bb, uniform over raw flops (the random flops are uniform deals).
"""
import argparse
import csv
import itertools
import json
import statistics
from pathlib import Path

import numpy as np
from scipy.optimize import nnls

RANKS = "23456789TJQKA"
PERMS = [dict(zip("cdhs", p)) for p in itertools.permutations("cdhs")]


def key(cards):
    return tuple(sorted(cards, key=lambda c: (RANKS.index(c[0]), c[1])))


def canonical(cards):
    """The suit-isomorphism class of a flop: the least sorted relabeling over the 24 suit permutations."""
    return min(key([c[0] + m[c[1]] for c in cards]) for m in PERMS)


def raw_counts():
    deck = [r + s for r in RANKS for s in "cdhs"]
    counts = {}
    for cards in itertools.combinations(deck, 3):
        k = canonical(cards)
        counts[k] = counts.get(k, 0) + 1
    return counts


def report(path, side):
    rows = list(csv.DictReader(open(path, encoding="utf-8", newline="")))
    return [(r["board"].split(), float(r[side])) for r in rows]


def mean_se(values):
    m = statistics.mean(values)
    return m, statistics.stdev(values) / len(values) ** 0.5


def features(path):
    rows = list(csv.reader(open(path, encoding="utf-8", newline="")))[1:]
    raw = np.array([float(r[1]) for r in rows])
    x = np.array([[float(v) if v else np.nan for v in r[2:]] for r in rows])
    x = np.where(np.isnan(x), np.nanmean(x, axis=0), x)
    return {canonical(r[0].split()): k for k, r in enumerate(rows)}, raw, x


def fit(index, raw, x, order):
    """Nonnegative weights summing to one whose weighted class strengths match the all-flop mean."""
    target = raw @ x / raw.sum()
    a = x[[index[m] for m in order]].T
    scale = a.std() * 10
    a = np.vstack([a, np.full(len(order), scale)])
    w, _ = nnls(a, np.append(target, scale))
    return w / w.sum()



def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("random")
    parser.add_argument("medoids")
    parser.add_argument("subsets")
    parser.add_argument("--ks", default="8,25,49,95")
    parser.add_argument("--side", default="ev_oop", choices=("ev_oop", "ev_ip"))
    parser.add_argument("--exact")
    parser.add_argument("--features")
    parser.add_argument("--json")
    args = parser.parse_args()
    sample = report(args.random, args.side)
    medoid_ev = {canonical(cards): v for cards, v in report(args.medoids, args.side)}
    mean, se = mean_se([v for _, v in sample])
    result = {"side": args.side, "n": len(sample), "random_mean": mean, "random_se": se, "k": {}}
    print(f"{args.side}: random mean {mean:.3f} ± {se:.3f} (n {len(sample)})")
    exact = None
    if args.exact:
        raw = raw_counts()
        exact = {}
        for cards, v in report(args.exact, args.side):
            k = canonical(cards)
            exact[k] = (v, raw[k])
        assert len(exact) == len(raw) == 1755, (len(exact), len(raw))
        total = sum(w for _, w in exact.values())
        result["exact_mean"] = sum(v * w for v, w in exact.values()) / total
        print(f"exact mean {result['exact_mean']:.3f} (1755 flops, {total} raw); "
              f"random − exact {mean - result['exact_mean']:+.3f}")
    feats = features(args.features) if args.features else None
    print("| K | weighted | weighted − random | weighted − exact | paired bias | paired MAE | exact MAE | "
          "fitted − exact | fitted weights > 0 | groups in sample |")
    print("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    subsets = Path(args.subsets)
    for k in map(int, args.ks.split(",")):
        reps = [line.split() for line in (subsets / f"k{k}.txt").read_text(encoding="utf-8").splitlines() if line]
        # Group ids index the medoids in k<K>.txt order (flop_subset.py).
        order = [canonical(r[:3]) for r in reps]
        weights = [float(r[3]) for r in reps]
        weighted = sum(w * medoid_ev[m] for m, w in zip(order, weights)) / sum(weights)
        groups = {}
        for line in (subsets / f"k{k}-groups.txt").read_text(encoding="utf-8").splitlines():
            if line:
                *cards, g = line.split()
                groups[canonical(cards)] = int(g)

        def medoid_of(cards):
            return medoid_ev[order[groups[canonical(cards)]]]

        diffs = [medoid_of(cards) - v for cards, v in sample]
        bias, bias_se = mean_se(diffs)
        mae = statistics.mean(abs(d) for d in diffs)
        hit = len({groups[canonical(cards)] for cards, _ in sample})
        row = {"weighted": weighted, "weighted_minus_random": weighted - mean,
               "paired_bias": bias, "paired_se": bias_se, "paired_mae": mae, "groups_hit": hit}
        cells = ["—", "—", "—", "—"]
        reference = result.get("exact_mean", mean)
        if exact:
            total = sum(w for _, w in exact.values())
            row["weighted_minus_exact"] = weighted - reference
            row["exact_mae"] = sum(abs(medoid_of(list(f)) - v) * w for f, (v, w) in exact.items()) / total
            cells[:2] = [f"{row['weighted_minus_exact']:+.3f}", f"{row['exact_mae']:.3f}"]
        if feats:
            fitted = fit(*feats, order)
            row["fitted"] = float(sum(w * medoid_ev[m] for m, w in zip(order, fitted)))
            row["fitted_minus_reference"] = row["fitted"] - reference
            row["fitted_support"] = int((fitted > 1e-9).sum())
            cells[2:] = [f"{row['fitted_minus_reference']:+.3f}", str(row["fitted_support"])]
        print(f"| {k} | {weighted:.3f} | {weighted - mean:+.3f} ± {se:.3f} | {cells[0]} | "
              f"{bias:+.3f} ± {bias_se:.3f} | {mae:.3f} | {cells[1]} | {cells[2]} | {cells[3]} | {hit}/{k} |")
        result["k"][k] = row
    if args.json:
        Path(args.json).write_text(json.dumps(result, indent=1) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
