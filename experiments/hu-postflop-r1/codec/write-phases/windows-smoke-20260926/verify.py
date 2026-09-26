#!/usr/bin/env python3
"""Offline byte/source/cleanup/phase checks; never execute the retained binary."""
import copy
import datetime as dt
import gzip
import hashlib
import io
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import tarfile

HERE = Path(__file__).resolve().parent
REVISION = "2fecc099b9911511a0938fb2700bbb124bc1046e"


def identity(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def verify():
    manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["schema"] == "r1.writer-windows-smoke-retention/v1"
    assert manifest["source_revision"] == REVISION
    assert len(manifest["blobs"]) == 45 and len(manifest["originals"]) == 53
    for name, expected in manifest["blobs"].items():
        path = (HERE / name).resolve()
        assert path.is_relative_to(HERE) and expected["encoding"] == "gzip"
        assert identity(path.read_bytes()) == {key: expected[key] for key in ("bytes", "sha256")}
        size, digest = 0, hashlib.sha256()
        with gzip.open(path, "rb") as stream:
            while chunk := stream.read(1024 * 1024):
                size += len(chunk)
                digest.update(chunk)
        assert {"bytes": size, "sha256": digest.hexdigest()} == expected["decoded"], name

    members = {}
    bundle = manifest["source_bundle"]
    with tarfile.open(HERE / bundle["blob"], "r:gz") as archive:
        for member in archive:
            path = PurePosixPath(member.name)
            assert member.isfile() and not path.is_absolute() and ".." not in path.parts
            assert member.name not in members
            members[member.name] = archive.extractfile(member).read()
    assert len(members) == bundle["member_count"] == 200
    source_bytes = {}

    def raw(path):
        if path in source_bytes:
            return source_bytes[path]
        name = manifest["originals"][path]
        assert manifest["blobs"][name]["decoded"]["bytes"] < 8 * 1024 * 1024
        return gzip.decompress((HERE / name).read_bytes())

    def read(path):
        return json.loads(raw(path))

    def observed_identity(path):
        if path in source_bytes:
            return identity(source_bytes[path])
        return manifest["blobs"][manifest["originals"][path]]["decoded"]

    def bound(ref):
        assert observed_identity(ref["path"]) == {key: ref[key] for key in ("bytes", "sha256")}, ref["path"]

    def join(root, name):
        return str(PureWindowsPath(root) / name)

    root = manifest["original_root"]
    source = read(bundle["source_manifest"])
    assert source["schema"] == "r1.write-phase-source-copy/v1"
    assert source["revision"] == REVISION and source["mode"] == "instrumented" and source["role"] == "candidate"
    assert len(source["before"]) == len(source["after"]) == bundle["source_files"] == 197
    changed = sorted(name for name in source["before"] if source["before"][name] != source["after"].get(name))
    assert changed == bundle["changed_files"] and len(changed) == 3
    assert set(members) == {"instrumented/" + name for name in source["after"]} | {
        "original-changed/" + name for name in changed}
    for name, expected in source["after"].items():
        value = members["instrumented/" + name]
        assert identity(value) == expected
        source_bytes[join(bundle["instrumented_root"], name)] = value
        original = members["original-changed/" + name] if name in changed else value
        assert identity(original) == source["before"][name]
        source_bytes[join(bundle["original_source_root"], name)] = original
    preparation = read(join(root, "preparation.json"))
    assert preparation["source_revision"] == REVISION
    assert preparation["source_files"] == source["before"]
    bound(preparation["source_pins"])
    candidate = read(preparation["source_pins"]["path"])["roles"]["candidate"]
    assert candidate["revision"] == REVISION and candidate["files"] == source["before"]
    bound(preparation["input"])
    tool_root = str(PureWindowsPath(preparation["source_pins"]["path"]).parent)
    for name, expected in source["instrumentation"].items():
        assert observed_identity(join(tool_root, name)) == expected

    stages = {}
    external_tools = {}
    for name in ("copy", "build", "confirm", "off", "on", "validate"):
        record = read(join(root, name + ".json"))
        assert record["schema"] == "solvers.supervised-run/v1"
        assert record["cleanup_complete"] is True and record["identity_unchanged"] is True
        assert record["identity_before"] == record["identity_after"]
        assert record["containment"]["kind"] == "windows_job_kill_on_close_suspended_assignment"
        assert record["forced"] is False
        for ref in record["identity_before"]:
            if ref["path"] in manifest["originals"] or ref["path"] in source_bytes:
                bound(ref)
            else:
                assert PureWindowsPath(ref["path"]).name in ("cargo.exe", "rustc.exe", "rustfmt.exe", "python.exe")
                if ref["path"] in external_tools:
                    assert ref == external_tools[ref["path"]]
                external_tools[ref["path"]] = ref
        for ref in record["outputs"].values():
            bound(ref)
        rows = [json.loads(line) for line in raw(record["outputs"]["samples"]["path"]).splitlines()]
        measure = record["measurement"]
        assert len(rows) == measure["sample_count"] and rows[-1] == record["last_sample"]
        assert rows[-1]["pids"] == [] and rows[-1]["tree_resident_bytes"] == 0
        elapsed = [row["elapsed_seconds"] for row in rows]
        assert all(a <= b for a, b in zip(elapsed, elapsed[1:]))
        assert max(row["tree_resident_bytes"] for row in rows) == measure["sampled_peak_tree_resident_bytes"]
        assert max(len(row["pids"]) for row in rows) == measure["max_observed_processes"]
        assert abs(max(b-a for a, b in zip(elapsed, elapsed[1:])) - measure["max_sample_gap_seconds"]) < 1e-6
        limits = record["limits"]
        assert max(row["tree_resident_bytes"] for row in rows) <= limits["memory_limit_bytes"]
        assert min(row["host_available_memory_bytes"] for row in rows) >= limits["min_free_memory_bytes"]
        assert min(row["disk_free_bytes"] for row in rows) >= limits["disk_reserve_bytes"]
        outcome = [record[key] for key in ("state", "stop_reason", "supervisor_exit_code", "child_exit_code")]
        if name == "build":
            assert outcome == ["failed", "descendants_after_root_exit", 1, 0]
            assert len(record["errors"]) == 1 and record["errors"][0]["where"] == "graceful_signal"
            assert record["errors"][0]["message"] == "CTRL_BREAK helper failed: [WinError 6] AttachConsole"
            assert len(record["events"]) == 1 and record["events"][0]["reason"] == "descendants_after_root_exit"
        else:
            assert outcome == ["completed", "completed", 0, 0]
            assert record["errors"] == [] and record["events"] == []
        stages[name] = {"outcome": outcome, "sample_count": len(rows),
                        "cleanup_complete": True, "elapsed_seconds": record["elapsed_seconds"]}

    build, confirmation, smoke = [read(join(root, name + ".json")) for name in ("build-plan", "confirm-plan", "smoke-plan")]
    for tool in build["toolchain"].values():
        assert tool["identity"] == external_tools[tool["identity"]["path"]]
    assert {key: source["formatter"][key] for key in ("path", "bytes", "sha256")} == external_tools[source["formatter"]["path"]]
    assert dt.datetime.fromisoformat(read(join(root, "build.json"))["ended_at"]) < dt.datetime.fromisoformat(confirmation["created_at"])
    assert dt.datetime.fromisoformat(read(join(root, "confirm.json"))["ended_at"]) < dt.datetime.fromisoformat(smoke["created_at"])
    assert build["source_files"] == confirmation["source_files"] == smoke["source_files"] == source["after"]
    assert build["argv"] == confirmation["argv"]
    assert build["environment"] == confirmation["environment"]
    for name, plan in (("build", build), ("confirm", confirmation)):
        record = read(join(root, name + ".json"))
        assert plan["argv"] == record["argv"] and plan["cwd"] == record["cwd"]
        assert dt.datetime.fromisoformat(plan["created_at"]) < dt.datetime.fromisoformat(record["started_at"])
    bound(build["source_manifest"])
    bound(confirmation["prior_supervisor"])
    bound(confirmation["prior_plan"])
    bound(confirmation["binary_before"])
    bound(smoke["build_confirmation"])
    bound(smoke["source_manifest"])
    bound(smoke["input"])
    bound(smoke["validator"])
    bound(smoke["binary"])
    build_result = read(join(root, "build-result.json"))
    assert build_result["supervisor_exit"] == 1 and "binary" not in build_result
    assert build_result["source_files_after"] == source["after"] and build_result["source_unchanged"] is True
    confirm_result = read(join(root, "confirm-result.json"))
    assert confirm_result["supervisor_exit"] == 0 and confirm_result["source_unchanged"] is True
    assert confirm_result["binary_unchanged"] is True
    assert confirmation["binary_before"] == confirm_result["binary"] == smoke["binary"]
    assert [stage["role"] for stage in smoke["stages"]] == ["off", "on", "validate"]
    for stage in smoke["stages"]:
        record = read(join(root, stage["role"] + ".json"))
        assert stage["argv"] == record["argv"]
        assert stage["timeout_seconds"] == record["limits"]["timeout_seconds"]
        assert all(value == record["limits"][key] for key, value in smoke["limits"].items())
        assert dt.datetime.fromisoformat(smoke["created_at"]) < dt.datetime.fromisoformat(record["started_at"])
    assert smoke["stages"][0]["phase_environment"] is None
    assert smoke["stages"][1]["phase_environment"] == join(root, "on/phase.json")
    result = read(join(root, "smoke-result.json"))
    assert result["status"] == "runtime_smoke_verified" and result["r1_acceptance"] is None
    assert all(result[key] is True for key in ("rewritten_sol_equal_to_input", "canonical_and_root_equal", "source_unchanged"))
    bound(result["plan"])
    for stage in result["stages"]:
        assert stage["supervisor_exit"] == 0 and stage["source_unchanged"] is True
        bound(stage["record"])
    for ref in result["artifacts"].values():
        bound(ref)
    assert len(result["artifacts"]) == 9
    assert join(root, "off/phase.json") not in manifest["originals"]
    original_sol = raw(smoke["input"]["path"])
    for role in ("off", "on"):
        assert raw(join(root, role + "/output/rewritten.sol")) == original_sol
    for name in ("canonical.bin", "root-canonical.bin"):
        assert manifest["originals"][join(root, "off/output/" + name)] == manifest["originals"][join(root, "on/output/" + name)]
    off, on = [read(join(root, role + "/output/result.json")) for role in ("off", "on")]
    normalized = [copy.deepcopy(value) for value in (off, on)]
    for value in normalized:
        assert value["status"] == "completed" and value["operation"] == "stream-write" and value["iterations"] == 1
        value.pop("timing")
    assert normalized[0] == normalized[1]

    # Execute only the retained, identity-checked existing offline validator.
    namespace = {"__name__": "retained_writer_phase_validator"}
    exec(compile(raw(smoke["validator"]["path"]), "retained_validate.py", "exec"), namespace)
    assert original_sol[:10] == b"SLVRSOLV\x03\x00"
    groups = int.from_bytes(original_sol[98:106], "little")
    directory = 106 + int.from_bytes(original_sol[50:58], "little")
    assert 0 < groups <= 1000000 and directory + groups * 64 <= len(original_sol)
    compressed = sum(int.from_bytes(original_sol[directory + i*64 + 24:directory + i*64 + 28], "little") for i in range(groups))
    phase = namespace["validate_phase"](read(join(root, "on/phase.json")),
        expected_groups=groups, expected_file_bytes=len(original_sol), expected_compressed_bytes=compressed,
        operation_seconds=on["timing"]["operation_seconds"])
    assert phase["status"] == "phase_record_valid_not_campaign_acceptance"
    recorded_validation = read(read(join(root, "validate.json"))["outputs"]["stdout"]["path"])
    assert phase == recorded_validation
    return {"schema": "r1.writer-windows-smoke-retained-verification/v1", "status": "retained_bytes_verified",
            "source_revision": REVISION, "source_files": 197, "source_changed_files": changed,
            "blob_count": len(manifest["blobs"]), "original_aliases": len(manifest["originals"]),
            "stages": stages, "sol_bytes": len(original_sol), "sol_rewrites_byte_identical": True,
            "canonical_and_root_byte_identical": True, "phase": phase,
            "reran_build_or_solver": False, "linux_126_process_campaign": "not_evaluated",
            "calibration": "not_evaluated", "performance_acceptance": "not_evaluated", "r1_acceptance": None}


if __name__ == "__main__":
    print(json.dumps(verify(), indent=2))
