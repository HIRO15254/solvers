#!/usr/bin/env python3
"""Verify retained workspace-test evidence offline; do not run Cargo or a solver."""
import datetime as dt
import gzip
import hashlib
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
SOURCE = "3d36aa8e43ceb25f398e281986dda96ea1df7cac"
SUMMARY = re.compile(
    r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"(\d+) measured; (\d+) filtered out;"
)


def identity(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def verify():
    manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["schema"] == "r1.windows-workspace-retention/v1"
    assert manifest["source_commit"] == SOURCE
    assert set(manifest["attempts"]) == {"768m-timeout", "warm-4threads"}
    assert len(manifest["files"]) == 12
    originals = {}
    for name, entry in manifest["files"].items():
        path = (HERE / name).resolve()
        assert path.is_relative_to(HERE), name
        raw = path.read_bytes()
        assert identity(raw) == {key: entry[key] for key in ("bytes", "sha256")}, name
        assert entry["encoding"] == "gzip"
        decoded = gzip.decompress(raw)
        assert identity(decoded) == {
            "bytes": entry["original_bytes"], "sha256": entry["original_sha256"]
        }, name
        assert entry["original_path"] not in originals, name
        originals[entry["original_path"]] = decoded

    def read(path):
        return json.loads(originals[path])

    def bound(ref):
        assert identity(originals[ref["path"]]) == {
            key: ref[key] for key in ("bytes", "sha256")
        }, ref["path"]

    checks = {}
    source_sets = []
    for name, item in manifest["attempts"].items():
        plan, record = read(item["plan"]), read(item["record"])
        assert plan["schema"] == "r1.workspace-validation-plan/v1"
        assert plan["source_commit"] == SOURCE
        assert len(plan["source_files"]) == 199
        source_sets.append(plan["source_files"])
        assert record["schema"] == "solvers.supervised-run/v1"
        assert record["state"] not in ("created", "running", "starting")
        assert record["identity_unchanged"] is True
        assert record["identity_before"] == record["identity_after"]
        assert record["cleanup_complete"] is True and record["errors"] == []
        assert record["forced"] is False
        assert record["containment"]["kind"] == "windows_job_kill_on_close_suspended_assignment"
        outcome = [record[key] for key in (
            "state", "stop_reason", "supervisor_exit_code", "child_exit_code")]
        assert outcome == item["expected_outcome"]
        assert plan["argv"] == record["argv"] == record["resolved_argv"]
        assert plan["limits"] == record["limits"]
        assert dt.datetime.fromisoformat(plan["created_at"]) < dt.datetime.fromisoformat(record["started_at"])
        assert dt.datetime.fromisoformat(record["started_at"]) < dt.datetime.fromisoformat(record["ended_at"])
        pins = {ref["path"]: ref for ref in record["identity_before"]}
        bound(pins[item["plan"]])
        for tool in ("cargo", "rustc", "python"):
            assert pins[plan[tool]["path"]] == plan[tool]
        for ref in record["outputs"].values():
            bound(ref)
        after = read(item["post_source_check"])
        assert after["schema"] == "r1.workspace-post-source-check/v1"
        assert after["source_commit"] == SOURCE and after["files_checked"] == 199
        assert after["all_match_plan"] is True
        assert after["source_files"] == plan["source_files"]
        assert after["source_root"] == record["cwd"]
        assert after["plan"]["path"] == item["plan"]
        bound(after["plan"])
        assert dt.datetime.fromisoformat(after["checked_at"]) > dt.datetime.fromisoformat(record["ended_at"])

        rows = [json.loads(line) for line in originals[record["outputs"]["samples"]["path"]].splitlines()]
        measure = record["measurement"]
        assert len(rows) == measure["sample_count"] and rows
        assert rows[-1] == record["last_sample"]
        assert rows[-1]["pids"] == [] and rows[-1]["tree_resident_bytes"] == 0
        elapsed = [row["elapsed_seconds"] for row in rows]
        assert all(a <= b for a, b in zip(elapsed, elapsed[1:]))
        assert measure["sampled_peak_tree_resident_bytes"] == max(row["tree_resident_bytes"] for row in rows)
        assert measure["max_observed_processes"] == max(len(row["pids"]) for row in rows)
        assert abs(measure["max_sample_gap_seconds"] - max(b - a for a, b in zip(elapsed, elapsed[1:]))) < 1e-6
        assert rows[-1]["root_os_peak_resident_bytes"] == measure["root_os_peak_resident_bytes"]
        assert rows[-1]["job_os_peak_commit_bytes"] == measure["job_os_peak_commit_bytes"]
        limits = record["limits"]
        assert max(row["tree_resident_bytes"] for row in rows) <= limits["memory_limit_bytes"]
        assert min(row["host_available_memory_bytes"] for row in rows) >= limits["min_free_memory_bytes"]
        assert min(row["disk_free_bytes"] for row in rows) >= limits["disk_reserve_bytes"]
        stdout = originals[record["outputs"]["stdout"]["path"]].decode("utf-8")
        summaries = [list(map(int, groups)) for groups in SUMMARY.findall(stdout)]
        totals = [sum(counts[index] for counts in summaries) for index in range(5)]
        assert summaries == item["test_summaries"]
        assert totals == item["completed_test_summary_totals"]
        assert not re.search(r"test result: FAILED|^test .* \.\.\. FAILED\s*$", stdout, re.MULTILINE)
        if name == "768m-timeout":
            assert outcome == ["timeout", "timeout", 124, 3221225786]
            assert item["workspace_complete"] is False and totals == [273, 0, 6, 0, 0]
            assert record["events"][0]["kind"] == "stop_requested"
            assert record["events"][0]["reason"] == "timeout"
            assert record["events"][0]["elapsed_seconds"] >= limits["timeout_seconds"]
            assert record["events"][1]["kind"] == "graceful" and record["events"][1]["delivered"] is True
        elif name == "warm-4threads":
            assert item["workspace_complete"] is True
            assert outcome == ["completed", "completed", 0, 0]
            assert record["events"] == []
            assert record["elapsed_seconds"] < limits["timeout_seconds"]
            assert totals == [898, 0, 30, 0, 0] and len(summaries) == 54
        checks[name] = {
            "outcome": outcome, "workspace_complete": item["workspace_complete"],
            "completed_test_summary_totals": totals, "sample_count": len(rows),
            "cleanup_complete": True, "source_files_matched_after_run": 199,
            "elapsed_seconds": record["elapsed_seconds"],
            "sampled_peak_tree_resident_bytes": measure["sampled_peak_tree_resident_bytes"],
        }
    assert all(source_set == source_sets[0] for source_set in source_sets)
    if "warm-4threads" in manifest["attempts"]:
        later = read(manifest["attempts"]["warm-4threads"]["plan"])
        assert later["previous_attempt"]["path"] == manifest["attempts"]["768m-timeout"]["record"]
        bound(later["previous_attempt"])
    return {
        "schema": "r1.windows-workspace-retained-verification/v1",
        "status": "retained_bytes_verified", "file_count": len(originals), "checks": checks,
        "reran_cargo_or_solver": False, "linux_signal_tests": "not_evaluated",
        "external_quality": "not_evaluated", "r1_acceptance": None,
    }


if __name__ == "__main__":
    print(json.dumps(verify(), indent=2))
