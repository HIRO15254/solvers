"""Summarize the L0 evaluations of the legacy 20bb solutions (benchmark B3).

Reads the `p2-l0-evaluation` JSON files written by `l0_eval` and prints Markdown tables: seat gains,
NashConv, diagnostics and timings, and the sensitivity of the 300k seed-0 result to the thread count,
the larger-showdown seed and the T3 seed. Only the Python standard library is used.
"""

import argparse
import json
from pathlib import Path

# Engine seat order of the six-max benchmark.
POSITIONS = ["BTN", "SB", "BB", "UTG", "HJ", "CO"]
RUNS = [
    ("20bb_cd", "30k", 0),
    ("20bb_cd_s1", "30k", 1),
    ("20bb_300k_s0", "300k", 0),
    ("20bb_300k_s1", "300k", 1),
]


def load(directory, name):
    return json.load(open(Path(directory) / f"{name}.json", encoding="utf-8"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", help="directory with the l0_eval outputs")
    args = parser.parse_args()

    print("| run | sweep | seed | " + " | ".join(POSITIONS) + " | NashConv | defaulted mass (max) | k≥4 reach (max) | evaluation |")
    print("|---|---|---|" + "---|" * (len(POSITIONS) + 4))
    rows = [(name, sweeps, seed, load(args.directory, name)) for name, sweeps, seed in RUNS]
    rows.append(("uniform", "-", "-", load(args.directory, "uniform")))
    for name, sweeps, seed, result in rows:
        gains = " | ".join(f"{seat['gain']:.4f}" for seat in result["seats"])
        defaulted = max(seat["defaulted_mass"] for seat in result["seats"])
        k4 = max(sum(seat["reach_by_active_count"][4:]) for seat in result["seats"])
        seconds = result["timings"]["evaluation"]
        print(f"| `{name}` | {sweeps} | {seed} | {gains} | {result['nash_conv']:.4f} | {defaulted:.1e} | {k4:.2%} | {seconds:.0f}秒 |")

    base = load(args.directory, "20bb_300k_s0")
    print()
    print("| variant of 20bb_300k_s0 | max |Δg_i| | Δ NashConv | identical seats |")
    print("|---|---|---|---|")
    for name in ("20bb_300k_s0_threads8", "20bb_300k_s0_k4seed1", "20bb_300k_s0_t3seed1"):
        other = load(args.directory, name)
        deltas = [abs(a["gain"] - b["gain"]) for a, b in zip(base["seats"], other["seats"], strict=True)]
        identical = base["seats"] == other["seats"]
        print(f"| `{name}` | {max(deltas):.2e} | {other['nash_conv'] - base['nash_conv']:+.2e} | {identical} |")

    print()
    for name, _, _, result in rows[:4]:
        print(f"## {name}: top local gains")
        for seat in result["seats"]:
            for item in seat["top_infosets"][:3]:
                row = ", ".join(f"{p:.2f}" for p in item["profile_row"])
                path = " / ".join(item["path"]) or "root"
                print(f"- {POSITIONS[seat['seat']]} {item['class']} after `{path}`: {item['local_gain']:.5f}"
                      f" (profile [{row}], best `{item['best_action']}`)")


if __name__ == "__main__":
    main()
