#!/usr/bin/env python3
"""Verify retained bytes and recorded outcomes; does not rerun Rust or a solver."""
import copy
import gzip
import hashlib
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent


def verify():
    manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
    originals = {}
    for name, ref in manifest["files"].items():
        path = (HERE / name).resolve()
        assert path.is_relative_to(HERE), name
        raw = path.read_bytes()
        assert len(raw) == ref["bytes"] and hashlib.sha256(raw).hexdigest() == ref["sha256"], name
        decoded = gzip.decompress(raw) if ref["encoding"] == "gzip" else raw
        assert len(decoded) == ref["original_bytes"], name
        assert hashlib.sha256(decoded).hexdigest() == ref["original_sha256"], name
        originals[ref["original_path"]] = decoded

    def read(path):
        return json.loads(originals[path])

    def bound(ref):
        raw = originals[ref["path"]]
        assert len(raw) == ref["bytes"]
        assert hashlib.sha256(raw).hexdigest() == ref["sha256"]

    checks = {}
    for name, item in manifest["checks"].items():
        record = read(item["record"])
        assert record["schema"] == "solvers.supervised-run/v1"
        assert record["identity_unchanged"] and record["identity_before"] == record["identity_after"]
        assert record["cleanup_complete"] and not record["errors"]
        actual = [record[key] for key in ("state", "stop_reason", "supervisor_exit_code", "child_exit_code")]
        assert actual == item["expected_outcome"], name
        for ref in record["outputs"].values():
            bound(ref)
        rows = [json.loads(line) for line in originals[record["outputs"]["samples"]["path"]].splitlines()]
        assert len(rows) == record["measurement"]["sample_count"]
        assert rows[-1] == record["last_sample"] and rows[-1]["pids"] == []
        assert rows[-1]["tree_resident_bytes"] == 0
        stdout = originals[record["outputs"]["stdout"]["path"]].decode("utf-8")
        summaries = [list(map(int, groups)) for groups in re.findall(
            r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", stdout)]
        assert summaries == item["test_summaries"], name
        checks[name] = {"outcome": actual, "test_summaries": summaries}

    campaign = read(manifest["campaign_result"])
    bound(campaign["plan"])
    assert campaign["status"] == "verified" and campaign["first_failure"] is None
    assert campaign["r1_acceptance"] is None
    assert [stage["label"] for stage in campaign["stages"]] == [
        "interrupted", "resumed", "straight", "compare", "audit-resumed", "audit-straight"]
    for stage in campaign["stages"]:
        bound(stage["record"])
    python = read(manifest["python_check_record"])
    assert python["returncode"] == 0 and python["identity_before"] == python["identity_after"]
    bound(python["stdout"])
    bound(python["stderr"])
    assert b"Ran 5 tests" in originals[python["stderr"]["path"]]
    tools = read(manifest["checks"]["retained/python/tools"]["record"])
    assert b"Ran 39 tests" in originals[tools["outputs"]["stderr"]["path"]]
    comparison = campaign["comparison"]
    assert comparison["status"] == "pass" and all(comparison["checks"].values())
    m, n = comparison["interrupted_iteration"], comparison["target_iteration"]
    assert 0 < m < n == 1000 and comparison["resumed_iterations"] == n - m
    assert campaign["run_artifacts"]["interrupted"] == campaign["interrupted_artifacts_after"]
    for role in campaign["run_artifacts"].values():
        for ref in role.values():
            bound(ref)
    dirs = manifest["run_directories"]
    assert originals[dirs["resumed"] + "\\checkpoint.ckpt"] == originals[dirs["straight"] + "\\checkpoint.ckpt"]
    saved = [read(path) for path in manifest["saved_profile_reports"]]
    normalized = []
    for report in saved:
        assert report["recomputed"] == campaign["saved_profile_values"]
        report = copy.deepcopy(report)
        for key in ("input_hash_secs", "load_secs", "eval_secs"):
            report.pop(key)
        for key in ("path", "bytes", "blake3"):
            report["artifact"].pop(key)
        report["pre_save_metadata"].pop("wall_secs")
        normalized.append(report)
    assert json.dumps(normalized[0], sort_keys=True) == json.dumps(normalized[1], sort_keys=True)
    return {"schema": "r1.hu-checkpoint-retained-verification/v1", "status": "retained_bytes_verified",
            "file_count": len(originals), "checks": checks, "interrupted_iteration": m,
            "target_iteration": n, "saved_profile_values": campaign["saved_profile_values"],
            "solver_rerun": False, "external_quality": "not_evaluated", "r1_acceptance": None}


if __name__ == "__main__":
    print(json.dumps(verify(), indent=2))
