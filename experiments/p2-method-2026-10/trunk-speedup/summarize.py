"""Summarize the S4-1b-2 runs as Markdown tables.

Python standard library only.

usage: python summarize.py <results dir>
"""
import json
import re
import statistics
import sys
from pathlib import Path

ITERATION = re.compile(r"Iteration (\d+): wall ([\d.]+)s; reaches ([\d.]+); t2 ([\d.]+); t3 ([\d.]+); k4 ([\d.]+); "
                       r"update ([\d.]+); evaluation ([\d.]+)")
EXACT_B3 = Path(__file__).resolve().parent.parent / "trunk-solver/results/b3-trial/b3_200.json"


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def iterations(path):
    with open(path, encoding="utf-8") as handle:
        return [tuple(float(x) for x in m.groups()) for m in map(ITERATION.search, handle) if m]


def b3(directory):
    approximate = load(directory / "b3-s256/b3_s256.json")
    exact = load(EXACT_B3)
    assert approximate["options"]["k4_samples"] == 256 and approximate["k4"] == {"samples": 2048, "seed": 0}
    print("### B3: NashConv of the average profile (L0 model, K4 2048, seed 0)")
    print()
    print("| iteration | exact solver | solver K4 256 | ratio |")
    print("|---|---|---|---|")
    for a, b in zip(exact["checkpoints"], approximate["checkpoints"]):
        assert a["iteration"] == b["iteration"]
        print(f"| {a['iteration']} | {a['nash_conv']:.4g} | {b['nash_conv']:.4g} | {b['nash_conv'] / a['nash_conv']:.3f} |")
    final = approximate["checkpoints"][-1]["seats"], exact["checkpoints"][-1]["seats"]
    print()
    print("seat gains at 200: solver K4 256 " + ", ".join(f"{s['gain']:.2e}" for s in final[0])
          + "; exact " + ", ".join(f"{s['gain']:.2e}" for s in final[1]))
    print()
    rows = iterations(directory / "b3-s256/b3_s256.log")
    assert len(rows) == approximate["iterations"]
    print("| iterations | median seconds excluding evaluation | K4 | T3 |")
    print("|---|---|---|---|")
    for low, high in ((1, 1), (2, 10), (11, 50), (51, 100), (101, 150), (151, 200)):
        selected = [r for r in rows if low <= r[0] <= high]
        print(f"| {low}–{high} | {statistics.median(r[1] - r[7] for r in selected):.1f} | "
              f"{statistics.median(r[5] for r in selected):.1f} | {statistics.median(r[4] for r in selected):.2f} |")
    timings = approximate["timings"]
    print()
    print(f"total {timings['total']:.0f} s; solve phases (s): "
          + ", ".join(f"{k} {v:.0f}" for k, v in timings["solve"].items()))
    print()


def b4(directory, name):
    print(f"### B4 ({name}, seconds)")
    print()
    print("| run | iteration | wall | reaches | T2 | T3 | K4 | update | final evaluation |")
    print("|---|---|---|---|---|---|---|---|---|")
    for path in sorted((directory / name).glob("b4_*.log")):
        run = load(path.with_suffix(".json"))
        rows = iterations(path)
        assert len(rows) == run["iterations"]
        for i, wall, reach, t2, t3, k4, update, evaluation in rows:
            print(f"| {path.stem} | {i:.0f} | {wall:.1f} | {reach:.2f} | {t2:.2f} | {t3:.2f} | {k4:.2f} | {update:.3f} | "
                  f"{evaluation:.1f} |")
        metrics = load(path.with_suffix(".metrics.json"))
        final = run["checkpoints"][-1]
        print(f"| {path.stem} | NashConv after {final['iteration']}: {final['nash_conv']:.6f} | "
              f"run {metrics['wall_seconds']:.0f} | peak {metrics['peak_working_set_mib']:.0f} MiB | | | | | |")
    print()


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    results = Path(sys.argv[1])
    b3(results)
    for name in sorted(p.name for p in results.iterdir() if p.is_dir() and p.name.startswith("b4")):
        b4(results, name)
