"""Choose K representative flops with weights: weighted k-medoids of the 1755 distinct flops.

usage: python flop_subset.py <flop_features csv> <K> <output> [--restarts 8] [--seed 0]

A flop is the vector of its 169 hand classes' mean 128-bucket EHS2 flop strength (flop_features example), so
two flops are close when every hand class is about as strong on both. Each distinct flop counts with its number of
raw flops. The flops are first split into strata by suits (monotone, two-tone, rainbow) and pairing (none,
paired, trips), which the class means barely see; each stratum gets a share of K proportional to its raw flops (at least one), and its
medoids are chosen by weighted k-medoids. A medoid's weight is the raw flops of its cluster. The output has one
`card card card weight` line per flop, the input of `trunk_solve --fixed-flops` / `--l1-flop-subset` and
`l1_leaf_diag --flops`. The idea follows PioSOLVER's representative flop subsets (choose a small weighted set that
reproduces equities over all flops); this is an independent implementation by clustering.
"""
import argparse
import csv
from pathlib import Path

import numpy as np


def load(path):
    rows = list(csv.reader(open(path, encoding="utf-8", newline="\n")))
    flops = [r[0] for r in rows[1:]]
    raw = np.array([float(r[1]) for r in rows[1:]])
    x = np.array([[float(v) if v else np.nan for v in r[2:]] for r in rows[1:]])
    x = np.where(np.isnan(x), np.nanmean(x, axis=0), x)
    return flops, raw, x


def kmedoids(x, w, k, rng, iterations=100):
    n = len(x)
    squared = (x * x).sum(1)
    distance = np.sqrt(np.maximum(squared[:, None] + squared[None, :] - 2 * x @ x.T, 0.0))
    # Weighted k-means++ seeding.
    medoids = [rng.choice(n, p=w / w.sum())]
    for _ in range(1, k):
        d = distance[:, medoids].min(1) ** 2 * w
        medoids.append(rng.choice(n, p=d / d.sum()))
    medoids = np.array(medoids)
    for _ in range(iterations):
        assign = distance[:, medoids].argmin(1)
        changed = False
        for j in range(k):
            members = np.flatnonzero(assign == j)
            cost = (distance[np.ix_(members, members)] * w[members][:, None]).sum(0)
            best = members[cost.argmin()]
            if best != medoids[j]:
                medoids[j] = best
                changed = True
        if not changed:
            break
    assign = distance[:, medoids].argmin(1)
    cost = (distance[np.arange(n), medoids[assign]] * w).sum() / w.sum()
    return medoids, assign, cost


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("features")
    parser.add_argument("k", type=int)
    parser.add_argument("output")
    parser.add_argument("--groups", help="also write `card card card group` for every flop (--l1-flop-groups)")
    parser.add_argument("--restarts", type=int, default=8)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    flops, raw, x = load(args.features)
    rng = np.random.default_rng(args.seed)
    strata = np.array([stratum(f.split()) for f in flops])
    keys = sorted(set(strata))
    minimum = len(keys)
    if args.k < minimum:
        raise SystemExit(f"K must be at least the {minimum} strata")
    mass = {s: raw[strata == s].sum() for s in keys}
    share = {s: 1 + (args.k - minimum) * mass[s] / raw.sum() for s in keys}
    count = {s: int(share[s]) for s in keys}
    for s in sorted(keys, key=lambda s: share[s] - count[s], reverse=True)[: args.k - sum(count.values())]:
        count[s] += 1
    lines, total = [], 0.0
    member_of = {}
    for s in keys:
        members = np.flatnonzero(strata == s)
        k = min(count[s], len(members))
        medoids, assign, cost = min(
            (kmedoids(x[members], raw[members], k, rng) for _ in range(args.restarts)), key=lambda r: r[2]
        )
        total += cost * mass[s]
        for j in range(k):
            medoid = flops[members[medoids[j]]]
            lines.append((int(raw[members][assign == j].sum()), medoid))
            for i in members[assign == j]:
                member_of[flops[i]] = medoid
    lines.sort(reverse=True)
    Path(args.output).write_text("".join(f"{f} {w}\n" for w, f in lines), encoding="utf-8", newline="\n")
    if args.groups:
        group = {f: g for g, (_, f) in enumerate(lines)}
        Path(args.groups).write_text("".join(f"{f} {group[member_of[f]]}\n" for f in flops), encoding="utf-8", newline="\n")
    print(f"K {len(lines)}: mean distance to medoid {total / raw.sum():.2f} (128-bucket strength units, 169 classes)")


def stratum(flop):
    suits = len({c[1] for c in flop})
    ranks = len({c[0] for c in flop})
    return f"{suits}{ranks}"


if __name__ == "__main__":
    main()
