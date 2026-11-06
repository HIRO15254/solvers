"""Re-weight a representative flop subset so its class strengths match the all-flop mean (B01 weights).

usage: python fit_subset.py <flop_features csv> <subsets/k<K>.txt> <output> [--blend 0.9]

The weights are quadrature.fit (nonnegative least squares on the 169 classes' mean 128-bucket EHS2 strength,
never on an EV) blended with the cluster weights, blend * fitted + (1 - blend) * cluster, so every
representative keeps a positive weight and still trains its own group. The output keeps the input's flops
and order (line i is group i) with weights in raw flops summing to 22,100, the input of
`trunk_solve --l1-flop-subset` and the weighted `--fixed-flops`.
"""
import argparse
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from quadrature import canonical, features, fit  # noqa: E402


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("features")
    parser.add_argument("subset")
    parser.add_argument("output")
    parser.add_argument("--blend", type=float, default=0.9)
    args = parser.parse_args()
    assert 0.0 <= args.blend < 1.0, "blend must keep every cluster weight positive"
    reps = [line.split() for line in Path(args.subset).read_text(encoding="utf-8").splitlines() if line]
    cluster = np.array([float(r[3]) for r in reps])
    total = cluster.sum()
    weights = (args.blend * fit(*features(args.features), [canonical(r[:3]) for r in reps])
               + (1 - args.blend) * cluster / total) * total
    Path(args.output).write_text("".join(f"{' '.join(r[:3])} {w:.6f}\n" for r, w in zip(reps, weights)),
                                 encoding="utf-8", newline="\n")
    print(f"{len(reps)} flops, weights {weights.min():.1f}..{weights.max():.1f}, sum {weights.sum():.1f}")


if __name__ == "__main__":
    main()
