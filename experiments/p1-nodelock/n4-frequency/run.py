#!/usr/bin/env python3
"""Run the N4 river comparison; execute from the repository root on Linux."""
import json
import os
import pathlib
import subprocess

HERE = pathlib.Path("experiments/p1-nodelock/n4-frequency")
RUNS = pathlib.Path(os.environ.get("N4_RUNS", "runs/n4-frequency"))
BIN = pathlib.Path(os.environ["CARGO_TARGET_DIR"]) / "release"
SOLVER = str(BIN / "solvers")
HELPER = str(BIN / "examples/frequency_action_ev")
TARGETS = (30, 50, 80)


def run(*args, capture=False):
    return subprocess.run(args, check=True, text=True, stdout=subprocess.PIPE if capture else None).stdout


def solve(config, name):
    out = RUNS / name
    with (RUNS / f"{name}.log").open("w") as log:
        subprocess.run((SOLVER, "solve", str(config), "--out", str(out)), check=True, text=True, stdout=log)
    summary = json.loads((out / "run.json").read_text())
    actions = json.loads(run(SOLVER, "export", str(out / "solution.sol"), "actions", capture=True))
    achieved = next(row["frequency"] for row in actions if row["action"] == "check")
    return summary, achieved


def naive_config(rows, target):
    # Starting with the baseline mixture, move available probability toward the
    # requested side in ascending per-unit EV loss. Split the last combo exactly.
    total = sum(row["weight"] for row in rows)
    current = sum(row["weight"] * row["check"] for row in rows) / total
    increase = target > current
    remaining = abs(target - current) * total
    for row in sorted(rows, key=lambda r: ((r["bet_ev"] - r["check_ev"]) if increase else (r["check_ev"] - r["bet_ev"]), r["combo"])):
        capacity = (1 - row["check"]) if increase else row["check"]
        delta = min(capacity, remaining / row["weight"])
        row["check"] += delta if increase else -delta
        remaining -= delta * row["weight"]
    if remaining > 1e-6:
        raise ValueError("unreachable target")
    combos = ", ".join(f'"{r["combo"]}" = {{ "check" = {r["check"]:.9f}, "bet 1" = {1-r["check"]:.9f} }}' for r in rows)
    return (HERE / "river.toml").read_text() + '\n[[profile.lock]]\nnode = "root"\ncombos = { ' + combos + ' }\n'


def main():
    RUNS.mkdir(parents=True, exist_ok=False)
    rows = json.loads(run(HELPER, str(HERE / "river.toml"), capture=True))
    (RUNS / "baseline-action-ev.json").write_text(json.dumps(rows, indent=2) + "\n")
    results = []
    for t in TARGETS:
        target = t / 100
        naive = RUNS / f"naive-{t:03}.toml"
        naive.write_text(naive_config([dict(row) for row in rows], target))
        for method, config in (("frequency", HERE / f"frequency-{t:03}.toml"), ("naive", naive)):
            summary, achieved = solve(config, f"{method}-{t:03}")
            results.append({"target": target, "method": method, "achieved": achieved, "error": achieved - target, "g_holder": summary["explP0"], "g_opponent": summary["explP1"], "true_ev_holder": summary["evP0"], "true_ev_opponent": summary["evP1"], "profile": summary.get("profile")})
    text = json.dumps(results, indent=2) + "\n"
    (RUNS / "results.json").write_text(text)
    print("RESULTS_JSON_BEGIN\n" + text + "RESULTS_JSON_END")


if __name__ == "__main__":
    main()
