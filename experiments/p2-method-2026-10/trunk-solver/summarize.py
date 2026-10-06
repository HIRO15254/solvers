"""Summarize the S4-1b-1 runs of run.sh as Markdown tables.

Python standard library only.

usage: python summarize.py <dir>
"""
import json
import re
import sys
from pathlib import Path

ITERATION = re.compile(r"Iteration (\d+): wall ([\d.]+)s; reaches ([\d.]+); t2 ([\d.]+); t3 ([\d.]+); k4 ([\d.]+); "
                       r"update ([\d.]+); evaluation ([\d.]+)")


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def b1(directory):
    print("### B1")
    print()
    print("| stack | iterations | NashConv | seat gains | solve s | l0_eval − solver | Python − solver | Python seat gains − solver |")
    print("|---|---|---|---|---|---|---|---|")
    trajectories = []
    for s in (5, 10, 20):
        run = load(directory / f"b1_{s}bb.json")
        final = run["checkpoints"][-1]
        evaluation = load(directory / f"b1_{s}bb.l0eval.json")
        python = load(directory / f"b1_{s}bb.py.json")
        assert run["reached_target"] and final["iteration"] == run["iterations"]
        gains = ", ".join(f"{seat['gain']:.3e}" for seat in final["seats"])
        seat_diff = max(abs(p["gain"] - q["gain"]) for p, q in zip(python["seats"], final["seats"]))
        print(f"| {s}bb | {run['iterations']} | {final['nash_conv']:.6e} | {gains} | {final['seconds']:.3f} | "
              f"{evaluation['nash_conv'] - final['nash_conv']:+.1e} | {python['nash_conv'] - final['nash_conv']:+.1e} | "
              f"{seat_diff:.1e} |")
        trajectories.append((s, run["checkpoints"]))
    print()
    print("NashConv of the average profile at each checkpoint:")
    print()
    iterations = [c["iteration"] for c in trajectories[0][1]]
    print("| stack | " + " | ".join(str(i) for i in iterations) + " |")
    print("|---|" + "---|" * len(iterations))
    for s, checkpoints in trajectories:
        assert [c["iteration"] for c in checkpoints] == iterations
        print(f"| {s}bb | " + " | ".join(f"{c['nash_conv']:.3g}" for c in checkpoints) + " |")
    print()


def b3(directory):
    print("### B3 (seconds)")
    print()
    print("| K4 samples | iteration | wall | reaches | T2 | T3 | K4 | update | final evaluation |")
    print("|---|---|---|---|---|---|---|---|---|")
    for k4 in (2048, 256):
        run = load(directory / f"b3_k{k4}.json")
        assert run["k4"]["samples"] == k4
        with open(directory / f"b3_k{k4}.log", encoding="utf-8") as handle:
            rows = [m.groups() for m in map(ITERATION.search, handle) if m]
        assert len(rows) == run["iterations"]
        for i, wall, reach, t2, t3, k, update, evaluation in rows:
            print(f"| {k4} | {i} | {float(wall):.1f} | {float(reach):.2f} | {float(t2):.2f} | {float(t3):.2f} | "
                  f"{float(k):.2f} | {float(update):.3f} | {float(evaluation):.1f} |")
        final = run["checkpoints"][-1]
        print(f"| {k4} | NashConv after {final['iteration']} | {final['nash_conv']:.4f} | | | | | | |")
    print()
    print(f"terminals by active count: {load(directory / 'b3_k2048.json')['terminals']['counts_by_k']}")
    print()


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    b1(Path(sys.argv[1]))
    b3(Path(sys.argv[1]))
