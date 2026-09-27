"""Check all timing records and report every predeclared local condition."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import statistics

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
WORKERS = (1, 2, 4, 8, 16)
CASES = ("narrow", "expanded")
ARMS = ("baseline", "flat")


def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def pin(path):
    digest = hashlib.sha256()
    size = 0
    with Path(path).open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
            size += len(block)
    return {"bytes": size, "sha256": digest.hexdigest()}


def need(value, message):
    if not value:
        raise ValueError(message)


def checked(record):
    path = Path(record["path"])
    need(pin(path) == {k: record[k] for k in ("bytes", "sha256")}, f"Pin changed: {path}")
    return path


def distribution(values):
    need(len(values) == 3 and all(v > 0 for v in values), "Expected 3 positive samples")
    return {"samples": values, "median": statistics.median(values), "min": min(values),
            "max": max(values), "max_over_min": max(values) / min(values)}


def summarize(samples):
    rows = []
    for case in CASES:
        for arm in ARMS:
            for workers in WORKERS:
                group = sorted((s for s in samples if (s["case"], s["arm"], s["workers"]) ==
                                (case, arm, workers)), key=lambda s: s["round"])
                need([s["round"] for s in group] == [1, 2, 3], "Missing/duplicate measurement")
                metrics = {key: distribution([s[key] for s in group]) for key in
                           ("cfr_seconds", "quality_seconds", "combined_seconds", "build_seconds",
                            "state_write_seconds", "process_seconds", "root_peak_bytes", "job_peak_bytes")}
                rows.append({"case": case, "arm": arm, "workers": workers, "metrics": metrics})
    lookup = {(r["case"], r["arm"], r["workers"]): r for r in rows}
    for row in rows:
        for key in ("cfr_seconds", "quality_seconds", "combined_seconds"):
            serial = lookup[row["case"], row["arm"], 1]["metrics"][key]["median"]
            speedup = serial / row["metrics"][key]["median"]
            row["metrics"][key].update(speedup=speedup, efficiency=speedup / row["workers"])
    guards = {"serial_within_5_percent": True, "eight_workers_at_least_10_percent_faster": True,
              "root_peak_within_10_percent": True, "all_combined_spreads_within_15_percent": True}
    comparisons = []
    for case in CASES:
        for workers in WORKERS:
            base, flat = (lookup[case, arm, workers]["metrics"] for arm in ARMS)
            ratio = flat["combined_seconds"]["median"] / base["combined_seconds"]["median"]
            peak_ratio = flat["root_peak_bytes"]["max"] / base["root_peak_bytes"]["max"]
            comparisons.append({"case": case, "workers": workers, "combined_ratio": ratio,
                                "root_peak_max_ratio": peak_ratio})
            if workers == 1:
                guards["serial_within_5_percent"] &= ratio <= 1.05
            if workers == 8:
                guards["eight_workers_at_least_10_percent_faster"] &= ratio <= 0.9
            guards["root_peak_within_10_percent"] &= peak_ratio <= 1.1
    guards["all_combined_spreads_within_15_percent"] = all(
        row["metrics"]["combined_seconds"]["max_over_min"] <= 1.15 for row in rows)
    decision = ("inconclusive_timing_variability" if not guards["all_combined_spreads_within_15_percent"]
                else "local_candidate_guard_passed" if all(guards.values()) else "local_candidate_guard_failed")
    return {"rows": rows, "comparisons": comparisons, "guards": guards, "decision": decision}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("execution", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    receipt = read(args.execution)
    need(receipt["status"] == "completed", "Incomplete matrix cannot be reported as complete")
    need(len(receipt["stages"]) == 80, "Expected 80 stages")
    seen, samples = set(), []
    for stage in receipt["stages"]:
        identity = (stage["case"], stage["arm"], stage["workers"], stage["round"])
        need(identity not in seen, "Duplicate stage")
        seen.add(identity)
        need(stage["status"] == "completed" and stage["wrapper_exit_code"] == 0, "Stage failed")
        need(stage["warmup"] == (stage["round"] == 0), "Warmup label differs")
        need(stage["fullstate_and_quality_bytes_equal_canonical"], "State/quality equality absent")
        record = read(checked(stage["record"]))
        need(record["state"] == "completed" and record["child_exit_code"] == 0 and
             record["supervisor_exit_code"] == 0 and record["identity_unchanged"] and
             record["cleanup_complete"] and not record["forced"], "Bounded record failed")
        need(record["bounded_job_settings"]["job_memory_limit_bytes"] == 536870912,
             "Job cap differs")
        for output in stage["outputs"].values():
            checked(output)
        result = read(stage["outputs"]["result.json"]["path"])
        need(result == stage["result"], "Embedded result differs")
        need(result["case"] == stage["case"] and result["threads"] == stage["workers"] and
             result["iterations"] == stage["iterations"], "Result condition mismatch")
        if stage["round"] == 0:
            continue
        row = {k: stage[k] for k in ("case", "arm", "workers", "round", "iterations")}
        row.update({k: result[k] for k in ("cfr_seconds", "quality_seconds", "build_seconds", "state_write_seconds")})
        row.update(combined_seconds=result["cfr_seconds"] + result["quality_seconds"],
                   process_seconds=record["elapsed_seconds"],
                   root_peak_bytes=record["measurement"]["root_os_peak_resident_bytes"],
                   job_peak_bytes=record["measurement"]["job_os_peak_commit_bytes"])
        samples.append(row)
    need(seen == {(case, arm, workers, round_) for case in CASES for arm in ARMS
                  for workers in WORKERS for round_ in range(4)}, "Matrix differs")
    output = {"schema": "r1-flat-ev-local-timing-analysis/v1", "execution": pin(args.execution),
              "analyzer": pin(Path(__file__)), "all_80_stages_valid": True,
              "samples": samples, **summarize(samples),
              "scope": "Same-host fixed-iteration exploratory comparison, 8 physical/16 logical CPUs; background load; no 32-vCPU or convergence acceptance."}
    need(not args.output.exists(), "Refusing to overwrite an analysis")
    args.output.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: output[k] for k in ("all_80_stages_valid", "decision", "guards")}))


if __name__ == "__main__":
    main()
