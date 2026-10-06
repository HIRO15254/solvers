"""Summarize l0_real outputs as Markdown tables (Python standard library only).

usage: python summarize.py b1 <dir>   heads-up agreement of the Monte Carlo values with the exact L0 values
       python summarize.py b3 <dir>   per-seat L0 errors, per-k differences and real gains of the L0 best responses
"""
import json
import sys
from pathlib import Path

SIX_MAX = ["BTN", "SB", "BB", "UTG", "HJ", "CO"]
HEADS_UP = ["BTN/SB", "BB"]
B3_RUNS = ["20bb_cd", "20bb_cd_s1", "20bb_300k_s0", "20bb_300k_s1", "uniform"]


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def names(data):
    return HEADS_UP if len(data["real"]["seats"]) == 2 else SIX_MAX


def b1(directory):
    print("| case | seat | L0 value | real value | z | L0 gain | real gain of the L0 BR | z |")
    print("|---|---|---|---|---|---|---|---|")
    worst = 0.0
    for path in sorted(Path(directory).glob("b1_*.json")):
        if path.name.endswith(".profile.json") or path.name.endswith("_tree.json"):
            continue
        data = load(path)
        for name, l0, real in zip(names(data), data["l0"]["seats"], data["real"]["seats"]):
            value, gain = real["value"], real["l0_best_response_gain"]
            z_value = (value["mean"] - l0["value"]) / value["stderr"]
            z_gain = (gain["mean"] - l0["gain"]) / gain["stderr"] if gain["stderr"] > 0 else 0.0
            worst = max(worst, abs(z_value), abs(z_gain))
            print(f"| {path.stem} | {name} | {l0['value']:.6f} | {value['mean']:.6f} ± {value['stderr']:.6f} | "
                  f"{z_value:+.2f} | {l0['gain']:.6f} | {gain['mean']:.6f} ± {gain['stderr']:.6f} | {z_gain:+.2f} |")
    print()
    print(f"largest |z|: {worst:.2f}")
    print()
    seeds = sorted(Path(directory).glob("seeds_10bb_uniform_s*.json"))
    if seeds:
        print("10bb uniform profile on independent deal seeds:")
        print()
        print("| deal seed | deals | BTN/SB value | z | BTN/SB real gain of the L0 BR | z |")
        print("|---|---|---|---|---|---|")
        values = []
        for path in seeds:
            data = load(path)
            l0, real = data["l0"]["seats"][0], data["real"]["seats"][0]
            value, gain = real["value"], real["l0_best_response_gain"]
            values.append((value["mean"] - l0["value"], value["stderr"]))
            print(f"| {data['real']['seed']} | {data['real']['deals']} | {value['mean']:+.6f} ± {value['stderr']:.6f} | "
                  f"{(value['mean'] - l0['value']) / value['stderr']:+.2f} | {gain['mean']:.6f} ± {gain['stderr']:.6f} | "
                  f"{(gain['mean'] - l0['gain']) / gain['stderr']:+.2f} |")
        mean = sum(d for d, _ in values) / len(values)
        stderr = sum(e * e for _, e in values) ** 0.5 / len(values)
        print()
        print(f"pooled value difference: {mean:+.6f} ± {stderr:.6f} (z {mean / stderr:+.2f})")


def b3(directory):
    runs = [name for name in B3_RUNS if (Path(directory) / f"{name}.json").exists()]
    for name in runs:
        data = load(Path(directory) / f"{name}.json")
        real = data["real"]
        print(f"### {name}")
        print()
        print(f"deals {real['deals']}, deal seed {real['seed']}, real {data['timings']['real']:.0f} s "
              f"({real['deals'] / data['timings']['real']:,.0f} deals/s), L0 {data['timings']['l0']:.0f} s")
        print()
        print("| seat | L0 value | real value | real − L0 | L0 gain | real gain of the L0 BR | real / L0 |")
        print("|---|---|---|---|---|---|---|")
        for seat, l0, r in zip(names(data), data["l0"]["seats"], real["seats"]):
            value, gain = r["value"], r["l0_best_response_gain"]
            ratio = gain["mean"] / l0["gain"] if l0["gain"] > 0 else float("nan")
            print(f"| {seat} | {l0['value']:.4f} | {value['mean']:.4f} ± {value['stderr']:.4f} | "
                  f"{value['mean'] - l0['value']:+.4f} | {l0['gain']:.4f} | {gain['mean']:.4f} ± {gain['stderr']:.4f} | "
                  f"{ratio:.2f} |")
        l0_sum = sum(s["value"] for s in data["l0"]["seats"])
        gain_sum = real["l0_best_response_gain_sum"]
        nash_conv = data["l0"]["nash_conv"]
        print(f"| sum | {l0_sum:.4f} | {real['value_sum']['mean']:.1e} | {real['value_sum']['mean'] - l0_sum:+.4f} | "
              f"{nash_conv:.4f} | {gain_sum['mean']:.4f} ± {gain_sum['stderr']:.4f} | {gain_sum['mean'] / nash_conv:.2f} |")
        print()
        seats = len(real["seats"])
        print("real − L0 by the number k of seats in the hand at the terminal:")
        print()
        print("| seat | " + " | ".join(f"k={k}" for k in range(1, seats + 1)) + " |")
        print("|---|" + "---|" * seats)
        totals = [0.0] * (seats + 1)
        for seat, l0, r in zip(names(data), data["l0"]["seats"], real["seats"]):
            cells = []
            for k in range(1, seats + 1):
                estimate = r["value_by_active_count"][k]
                difference = estimate["mean"] - l0["value_by_active_count"][k]
                totals[k] += difference
                cells.append(f"{difference:+.4f} ({estimate['stderr']:.4f})")
            print(f"| {seat} | " + " | ".join(cells) + " |")
        print("| sum | " + " | ".join(f"{totals[k]:+.4f}" for k in range(1, seats + 1)) + " |")
        print()
    sensitivity(directory)


def sensitivity(directory):
    """Compare the 300k seed-0 run with its 8-thread and deal-seed-1 reruns."""
    base_path = Path(directory) / "20bb_300k_s0.json"
    if not base_path.exists():
        return
    base = load(base_path)
    threads = Path(directory) / "20bb_300k_s0_threads8.json"
    if threads.exists():
        other = load(threads)
        same = all(base[key] == other[key] for key in ("l0", "real", "game_fingerprint", "tables", "k4"))
        print(f"8 threads: L0 and real sections {'bit-identical' if same else 'DIFFER'} to 16 threads")
        print()
    seed = Path(directory) / "20bb_300k_s0_dealseed1.json"
    if seed.exists():
        other = load(seed)
        print("deal seed 1 − deal seed 0, in units of the combined standard error:")
        print()
        print("| seat | value | z | real gain of the L0 BR | z |")
        print("|---|---|---|---|---|")
        for name, a, b in zip(names(base), base["real"]["seats"], other["real"]["seats"]):
            cells = []
            for key in ("value", "l0_best_response_gain"):
                difference = b[key]["mean"] - a[key]["mean"]
                combined = (a[key]["stderr"] ** 2 + b[key]["stderr"] ** 2) ** 0.5
                cells.append(f"{difference:+.4f} | {difference / combined:+.2f}")
            print(f"| {name} | " + " | ".join(cells) + " |")
        a, b = base["real"]["l0_best_response_gain_sum"], other["real"]["l0_best_response_gain_sum"]
        difference = b["mean"] - a["mean"]
        print(f"| sum of gains | | | {difference:+.4f} | "
              f"{difference / (a['stderr'] ** 2 + b['stderr'] ** 2) ** 0.5:+.2f} |")
        print()


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    {"b1": b1, "b3": b3}[sys.argv[1]](sys.argv[2])
