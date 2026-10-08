"""Summarize run_eval_saved.sh: per solution and sampling, the primary metric's in-sample and held-out values over the
eval seeds (mean and standard deviation) and with 8192 boards.

usage: python summarize_eval.py <dir>   (reads <name>-<sampling>-<boards>-s<seed>.json, p2-trunk-evaluation v1)
Python standard library only.
"""
import json
import re
import statistics
import sys
from collections import defaultdict
from pathlib import Path


def main():
    runs = defaultdict(dict)
    for path in sorted(Path(sys.argv[1]).glob("*.json")):
        m = re.fullmatch(r"(.+)-(random|stratified)-(\d+)-s(\d+)", path.stem)
        if not m:
            continue
        d = json.loads(path.read_text(encoding="utf-8"))
        assert d["format"] == "p2-trunk-evaluation"
        e = d["evaluation"]
        runs[(m.group(1), m.group(2), int(m.group(3)))][int(m.group(4))] = (
            e["nash_conv"], e["held_nash_conv"], e["auxiliary_nash_conv"], e["evaluation_seconds"])
    print("Values in bb/hand. With several seeds: mean ± standard deviation over the seeds.\n")
    print("| solution | sampling | boards | seeds | in-sample | held-out | in-sample − held-out | midpoint | auxiliary | "
          "s/evaluation |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for (name, sampling, boards), seeds in sorted(runs.items()):
        cols = list(zip(*seeds.values()))
        derived = [[a - b for a, b in zip(cols[0], cols[1])], [(a + b) / 2 for a, b in zip(cols[0], cols[1])]]

        def cell(values, digits=5):
            if len(values) == 1:
                return f"{values[0]:.{digits}f}"
            return f"{statistics.fmean(values):.{digits}f} ± {statistics.stdev(values):.{digits}f}"
        print(f"| {name} | {sampling} | {boards} | {len(seeds)} | {cell(cols[0])} | {cell(cols[1])} | "
              f"{cell(derived[0])} | {cell(derived[1])} | {cell(cols[2], 3)} | {statistics.fmean(cols[3]):.0f} |")


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    main()
