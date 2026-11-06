"""Per-leaf flop groups weighted by the leaf's own ranges (2026-10-29; the groups of notes/27-leaf-own.md).

usage: python experiments/p2-method-2026-10/gtow-fixed/results/leaf-own/leaf_groups.py <reach json> <out dir>
         [--bands 3bet,4bet,5bet] [--k 49] [--jobs 4]
The groups of notes/27: reach-r16g49q.json (the bench leaves) -> leafgroups/r16g49q, and the reach of all 181 leaves
(bench_reach.py on the same profile with the lines of leaf-sizes.tsv, reach-all-r16g49q.json) with --bands
srp,3bet,4bet,5bet -> leafgroups/r16g49q-all. Built in runs/sc-x; the groups and the reach are kept in
.cache/p2-trunk/gtow-fixed/leaf-own/ (inputs.sha256).

For each leaf of the reach json (bench_reach.py), every class column of ../_goal/fixed/flops.csv is multiplied by
sqrt(169 * w), w = the sum over both seats of reach_leaf * n normalised to 1 per seat, then halved; flop_subset.py
groups the flops (K groups, strata by suits and pairing). Output: <out dir>/<leaf>.txt (groups) and <leaf>.csv.
"""
import argparse
import csv
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[5]
FEATURES = ROOT.parent / "_goal/fixed/flops.csv"
SUBSET = ROOT / "experiments/p2-method-2026-10/gtow-fixed/flop_subset.py"


def weights(leaf):
    w = np.zeros(169)
    for seat in leaf["seats"].values():
        mass = np.array(seat["reach_leaf"]) * np.array(seat["n"])
        w += mass / mass.sum()
    return w / w.sum()


def write_features(w, path):
    rows = list(csv.reader(open(FEATURES, encoding="utf-8", newline="")))
    scale = np.sqrt(169 * w)
    with open(path, "w", encoding="utf-8", newline="") as fh:
        out = csv.writer(fh, lineterminator="\n")
        out.writerow(rows[0])
        for r in rows[1:]:
            out.writerow(r[:2] + [f"{float(v) * s:.4f}" if v else "" for v, s in zip(r[2:], scale)])


def main():
    p = argparse.ArgumentParser()
    p.add_argument("reach")
    p.add_argument("out")
    p.add_argument("--bands", default="3bet,4bet,5bet")
    p.add_argument("--k", type=int, default=49)
    p.add_argument("--jobs", type=int, default=4)
    a = p.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    bands = set(a.bands.split(","))
    leaves = [x for x in json.load(open(a.reach, encoding="utf-8"))["leaves"] if x["band"] in bands]

    def build(leaf):
        w = weights(leaf)
        features = out / f"{leaf['leaf']}.csv"
        write_features(w, features)
        r = subprocess.run(
            [sys.executable, str(SUBSET), str(features), str(a.k), str(out / f"{leaf['leaf']}-subset.txt"),
             "--groups", str(out / f"{leaf['leaf']}.txt")],
            capture_output=True, text=True)
        if r.returncode:
            raise RuntimeError(f"{leaf['leaf']}: {r.stderr[-500:]}")
        return leaf["leaf"], leaf["line"], 1 / (w * w).sum(), r.stdout.strip()

    with ThreadPoolExecutor(a.jobs) as pool:
        results = list(pool.map(build, leaves))
    with open(out / "index.tsv", "w", encoding="utf-8", newline="\n") as fh:
        for name, line, effective, log in results:
            fh.write(f"{name}\t{line}\t{effective:.1f}\t{log}\n")
            print(name, f"{effective:.1f}", log)


if __name__ == "__main__":
    main()
