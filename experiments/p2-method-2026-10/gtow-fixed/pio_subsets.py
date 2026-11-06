"""Turn the published PioSOLVER-derived flop subsets used by GTO Wizard into --l1-flop-groups files.

usage: python pio_subsets.py <flop_features csv> <output dir> [--html saved page]

GTO Wizard's article "Poker Subsets and Abstractions" (https://blog.gtowizard.com/poker-subsets-and-abstractions/)
credits PioSOLVER for its subsets and links their flop lists at https://justpaste.it/8yjcr (sizes 25, 49, 85,
184; no weights). The lists carry no license, so per LICENSE-POLICY.md they are not copied into the repository:
this script downloads (or reads a saved copy of) the page and writes, outside the repository, for each size K
  kK.txt         `card card card weight` per subset flop, the raw flops assigned to it;
  kK-groups.txt  `card card card group` for every one of the 1755 isomorphic flops (--l1-flop-groups),
assigning each flop to the nearest subset flop of its suit/pairing stratum (flop_subset.stratum) by the
flop_features distance, or to the nearest subset flop overall when the subset has none in that stratum.
"""

import argparse
import re
import sys
import urllib.request
from itertools import permutations
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
from flop_subset import load, stratum  # noqa: E402

URL = "https://justpaste.it/8yjcr"
RANKS = "23456789TJQKA"


def canonical(cards):
    best = None
    for p in permutations("cdhs"):
        relabel = dict(zip("cdhs", p))
        key = tuple(sorted(((RANKS.index(c[0]), relabel[c[1]]) for c in cards), reverse=True))
        best = key if best is None or key < best else best
    return best


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("features")
    parser.add_argument("output")
    parser.add_argument("--html")
    args = parser.parse_args()
    if args.html:
        page = Path(args.html).read_text(encoding="utf-8")
    else:
        request = urllib.request.Request(URL, headers={"User-Agent": "Mozilla/5.0"})
        page = urllib.request.urlopen(request).read().decode("utf-8")
    flops, raw, x = load(args.features)
    index = {canonical(f.split()): i for i, f in enumerate(flops)}
    strata = np.array([stratum(f.split()) for f in flops])
    out = Path(args.output)
    out.mkdir(parents=True, exist_ok=True)
    for size, body in re.findall(r"(\d+)\D{0,40}?\[([^\]]*)\]", page):
        boards = re.findall(r"([2-9TJQKA][cdhs])([2-9TJQKA][cdhs])([2-9TJQKA][cdhs])", body)
        if len(boards) != int(size):
            continue
        members = np.array(sorted({index[canonical(b)] for b in boards}))
        assert len(members) == int(size), f"subset {size} repeats an isomorphic flop"
        distance = np.linalg.norm(x[:, None, :] - x[None, members, :], axis=2)
        same = strata[:, None] == strata[None, members]
        has = same.any(1)
        distance = np.where(same | ~has[:, None], distance, np.inf)
        assign = distance.argmin(1)
        weights = np.bincount(assign, weights=raw, minlength=len(members))
        cost = (distance[np.arange(len(flops)), assign] * raw).sum() / raw.sum()
        order = np.argsort(-weights, kind="stable")
        group = np.empty(len(members), dtype=int)
        group[order] = np.arange(len(members))
        (out / f"k{size}.txt").write_text(
            "".join(f"{flops[members[j]]} {int(weights[j])}\n" for j in order), encoding="utf-8", newline="\n"
        )
        (out / f"k{size}-groups.txt").write_text(
            "".join(f"{f} {group[a]}\n" for f, a in zip(flops, assign)), encoding="utf-8", newline="\n"
        )
        print(f"K {size}: mean distance to subset flop {cost:.2f}; strata without a member {sorted(set(strata[~has]))}")


if __name__ == "__main__":
    main()
