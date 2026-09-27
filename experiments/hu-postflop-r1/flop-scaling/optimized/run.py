"""Bounded release correctness matrix; never interprets local timings as speedup."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
WRAPPER = HERE.parent / "native-preflight/run_bounded.py"
ADAPTER = HERE.parent / "native-solve/solve.rs"


def pin(path):
    h, size = hashlib.sha256(), 0
    with path.open("rb") as f:
        while b := f.read(1024 * 1024):
            h.update(b)
            size += len(b)
    return {"bytes": size, "sha256": h.hexdigest()}


def equal_stream(left, right):
    while True:
        a, b = left.read(1024 * 1024), right.read(1024 * 1024)
        if a != b:
            return False
        if not a:
            return True


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--baseline-build", required=True, type=Path)
    ap.add_argument("--flat-build", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()
    out = args.out.resolve()
    if not out.is_relative_to(ROOT / "runs") or out == ROOT / "runs":
        raise ValueError("Use a fresh runs/ child")
    out.mkdir(parents=True, exist_ok=False)
    builds = {}
    for arm, path in (("baseline", args.baseline_build), ("flat", args.flat_build)):
        doc = json.loads(path.read_text(encoding="utf-8"))
        assert doc["all_passed"] and doc["arm"] == arm
        binary = Path(doc["binary"]["path"])
        assert pin(binary) == {k: doc["binary"][k] for k in ("bytes", "sha256")}
        assert all(pin(ROOT / name) == p for name, p in doc["snapshot_pins"].items())
        builds[arm] = (path.resolve(), binary)
    conditions = [(case, arm, workers) for case in ("narrow", "expanded")
                  for arm, workers in (("baseline", 1), ("baseline", 2), ("flat", 1), ("flat", 2))]
    plan = {"scope": "Full-street Flop two-iteration F32/DCFR release correctness, no convergence or timing acceptance",
            "conditions": conditions, "exact_iterations": 2, "wall_seconds_each": 60,
            "job_commit_bytes": 536870912, "rss_trigger_bytes": 469762048,
            "stop_on_failure_or_mismatch": True, "build_receipts": {a: {"path": str(v[0]), **pin(v[0])} for a, v in builds.items()},
            "adapter": pin(ADAPTER), "runner": pin(Path(__file__))}
    (out / "plan.json").write_text(json.dumps(plan, indent=2) + "\n", encoding="utf-8")
    results, references = [], {}
    for case, arm, workers in conditions:
        run = out / f"{case}-{arm}-{workers}"
        run.mkdir()
        fixture = HERE.parent / f"fixtures/{case}.toml"
        command = [sys.executable, "-B", str(WRAPPER), "--record", str(run / "record.json"),
                   "--cwd", str(ROOT), "--timeout-seconds", "60", "--grace-seconds", "0.2",
                   "--poll-seconds", "0.1", "--memory-limit-bytes", "469762048",
                   "--min-free-memory-bytes", "1610612736", "--disk-reserve-bytes", "1073741824",
                   "--identity-file", str(fixture), "--identity-file", str(ADAPTER),
                   "--identity-file", str(builds[arm][0]), "--", str(builds[arm][1]),
                   case, str(workers), "2", str(run / "output")]
        p = subprocess.run(command, capture_output=True)
        (run / "wrapper.stdout.log").write_bytes(p.stdout)
        (run / "wrapper.stderr.log").write_bytes(p.stderr)
        item = {"case": case, "arm": arm, "workers": workers, "command": command, "exit_code": p.returncode}
        results.append(item)
        try:
            if p.returncode:
                raise RuntimeError("Bounded solve failed; no retry")
            result = json.loads((run / "output/result.json").read_text(encoding="utf-8"))
            assert result["status"] == "completed"
            state, quality = run / "output/state.bin", run / "output/quality.json"
            item["state"] = pin(state)
            item["quality"] = pin(quality)
            if case not in references:
                references[case] = (state, quality)
            with state.open("rb") as a, references[case][0].open("rb") as b:
                item["state_equals_case_baseline1"] = equal_stream(a, b)
            item["quality_equals_case_baseline1"] = quality.read_bytes() == references[case][1].read_bytes()
            assert item["state_equals_case_baseline1"] and item["quality_equals_case_baseline1"]
            if case == "narrow":
                debug = HERE.parent / "native-solve/proof01"
                with state.open("rb") as a, gzip.open(debug / "shared-state.bin.gz", "rb") as b:
                    item["state_equals_previous_debug"] = equal_stream(a, b)
                item["quality_equals_previous_debug"] = quality.read_bytes() == (debug / "baseline-1/output/quality.json").read_bytes()
                assert item["state_equals_previous_debug"] and item["quality_equals_previous_debug"]
        except Exception as exc:
            item["failure"] = str(exc)
            raise
        finally:
            (out / "execution.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({k: v for k, v in item.items() if k != "command"}), flush=True)


if __name__ == "__main__":
    main()
