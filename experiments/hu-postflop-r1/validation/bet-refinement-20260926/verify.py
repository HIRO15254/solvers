#!/usr/bin/env python3
"""Check this retained local correctness evidence; do not execute solver code."""
import datetime as dt
import gzip
import hashlib
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent


def verify():
    manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
    files = {}
    for name, entry in manifest["files"].items():
        path = HERE / name
        assert path.resolve().is_relative_to(HERE), name
        raw = path.read_bytes()
        assert len(raw) == entry["bytes"], name
        assert hashlib.sha256(raw).hexdigest() == entry["sha256"], name
        decoded = gzip.decompress(raw) if entry["encoding"] == "gzip" else raw
        assert len(decoded) == entry["original_bytes"], name
        assert hashlib.sha256(decoded).hexdigest() == entry["original_sha256"], name
        files[entry["original_path"]] = decoded
    checks = {}
    for name, item in manifest["checks"].items():
        record = json.loads(files[item["record"]])
        assert record["identity_unchanged"] is True
        assert record["identity_before"] == record["identity_after"]
        assert record["cleanup_complete"] is True and record["errors"] == []
        assert record["stop_reason"] == item["expected_stop_reason"]
        assert record["supervisor_exit_code"] == item["expected_exit_code"]
        assert record["child_exit_code"] == item["expected_child_exit_code"]
        for ref in record["outputs"].values():
            raw = files[ref["path"]]
            assert len(raw) == ref["bytes"]
            assert hashlib.sha256(raw).hexdigest() == ref["sha256"]
        rows = [json.loads(x) for x in files[record["outputs"]["samples"]["path"]].splitlines()]
        assert len(rows) == record["measurement"]["sample_count"]
        if rows:
            assert rows[-1] == record["last_sample"]
            assert rows[-1]["pids"] == []
            assert rows[-1]["tree_resident_bytes"] == 0
        stdout = files[record["outputs"]["stdout"]["path"]].decode("utf-8")
        counts = [tuple(map(int, x)) for x in re.findall(
            r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", stdout)]
        if name == "targeted":
            assert counts == [(4, 0, 0)]
            assert "test coarse_bet_profile_lifts_completely_and_expanded_br_uses_the_fine_domain ... " in stdout
            assert "expanded BR=12.776580459770" in stdout
        elif name.startswith("workspace"):
            assert counts == [], "these retained attempts stopped during compilation"
        checks[name] = {
            "stop_reason": record["stop_reason"],
            "supervisor_exit_code": record["supervisor_exit_code"],
            "cleanup_complete": True,
            "test_summaries": counts,
        }
    source = (HERE / "oracle_river.rs.snapshot").read_bytes()
    plan = json.loads((HERE / "plan.json").read_text(encoding="utf-8"))
    source_pin = {"bytes": len(source), "sha256": hashlib.sha256(source).hexdigest()}
    assert source_pin == plan["source_files"]["crates/holdem/tests/oracle_river.rs"]
    targeted = json.loads(files[manifest["checks"]["targeted"]["record"]])
    bound_source = [ref for ref in targeted["identity_before"]
                    if ref["path"].replace("\\", "/").endswith("/crates/holdem/tests/oracle_river.rs")]
    assert len(bound_source) == 1
    assert {key: bound_source[0][key] for key in ("bytes", "sha256")} == source_pin
    # The 198-file plan was created after the targeted test, before later checks.
    assert dt.datetime.fromisoformat(targeted["ended_at"]) < dt.datetime.fromisoformat(plan["created_at"])
    for name in ("fmt", "clippy", "workspace-command-stop", "workspace-memory-stop", "workspace-preflight-stop"):
        record = json.loads(files[manifest["checks"][name]["record"]])
        assert dt.datetime.fromisoformat(plan["created_at"]) < dt.datetime.fromisoformat(record["started_at"])
    return {"schema": "r1.bet-refinement-evidence-verification/v1", "status": "retained_bytes_verified",
            "checks": checks, "workspace_tests": "not_completed_resource_limited",
            "source_manifest_scope": "after targeted test; before fmt, clippy and workspace attempts",
            "external_quality": "not_evaluated", "r1_acceptance": None}


if __name__ == "__main__":
    print(json.dumps(verify(), indent=2))
