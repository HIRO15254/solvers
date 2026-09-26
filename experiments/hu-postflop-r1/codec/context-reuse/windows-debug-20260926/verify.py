#!/usr/bin/env python3
"""Verify retained diagnostic evidence and Git source bytes; no build or solver."""
import copy
import datetime as dt
import gzip
import hashlib
import io
import json
from pathlib import Path, PureWindowsPath
import re
import statistics
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[4]
REVISION = "2fecc099b9911511a0938fb2700bbb124bc1046e"
SUMMARY = re.compile(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;")


def identity(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def join(root, name):
    return str(PureWindowsPath(root) / name)


def time(value):
    return dt.datetime.fromisoformat(value)


def verify():
    manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["schema"] == "r1.context-reuse-windows-retention/v1"
    assert manifest["source_revision"] == REVISION
    assert len(manifest["blobs"]) == 123 and len(manifest["originals"]) == 194
    assert manifest["counts"] == {"fresh_stages": 24, "fresh_samples": 18, "input_files": 3}
    for name, expected in manifest["blobs"].items():
        path = (HERE / name).resolve()
        assert path.is_relative_to(HERE) and expected["encoding"] == "gzip"
        assert identity(path.read_bytes()) == {k: expected[k] for k in ("bytes", "sha256")}
        size, digest = 0, hashlib.sha256()
        with gzip.open(path, "rb") as stream:
            while chunk := stream.read(1024 * 1024):
                size += len(chunk)
                digest.update(chunk)
        assert {"bytes": size, "sha256": digest.hexdigest()} == expected["decoded"], name

    source_bytes, binary_aliases = {}, {}

    def raw(path):
        if path in source_bytes:
            return source_bytes[path]
        blob = manifest["originals"][binary_aliases.get(path, path)]
        assert manifest["blobs"][blob]["decoded"]["bytes"] < 8 * 1024 * 1024
        return gzip.decompress((HERE / blob).read_bytes())

    def read(path):
        return json.loads(raw(path))

    def content_identity(path):
        if path in source_bytes:
            return identity(source_bytes[path])
        blob = manifest["originals"][binary_aliases.get(path, path)]
        return manifest["blobs"][blob]["decoded"]

    def bound(ref):
        assert content_identity(ref["path"]) == {k: ref[k] for k in ("bytes", "sha256")}, ref["path"]

    root = manifest["original_root"]
    fresh = join(root, "fresh")
    provenance = manifest["source_git"]
    source = read(provenance["source_manifest"])
    assert source["schema"] == "r1.context-reuse-source-copy/v1"
    assert source["source_revision"] == provenance["revision"] == REVISION
    assert provenance["source_files"] == len(source["before"]) == len(source["after"]) == 197
    assert provenance["changed_files"] == source["changed_files"] == ["crates/formats/src/sol_indexed.rs"]
    # The immutable Git base replaces another duplicate archive of 197 files.
    archive = subprocess.check_output(
        ["git", "archive", "--format=tar", REVISION, "Cargo.toml", "Cargo.lock", "crates"], cwd=REPO)
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as stream:
        base = {member.name: stream.extractfile(member).read() for member in stream if member.isfile()}
    assert {name: identity(value) for name, value in base.items()} == source["before"]
    for name, value in base.items():
        source_bytes[join(provenance["baseline_root"], name)] = value
        candidate = raw(join(provenance["candidate_root"], name)) if name in source["changed_files"] else value
        assert identity(candidate) == source["after"][name]
        source_bytes[join(provenance["candidate_root"], name)] = candidate
        assert b"R1_SOL_WRITE_PHASE_OUTPUT" not in value and b"R1_SOL_WRITE_PHASE_OUTPUT" not in candidate
    bound(source["source_pin_file"])
    original_pins = read(source["source_pin_file"]["path"])["roles"]["candidate"]
    assert original_pins["revision"] == REVISION and original_pins["files"] == source["before"]
    assert content_identity(join(provenance["candidate_root"], source["patch"]["path"])) == {
        k: source["patch"][k] for k in ("bytes", "sha256")}

    plan, result = read(join(fresh, "plan.json")), read(join(fresh, "result.json"))
    assert plan["schema"] == "r1.context-reuse-fresh-diagnostic-plan/v1"
    assert result["schema"] == "r1.context-reuse-fresh-diagnostic-result/v1"
    assert result["status"] == "diagnostic_complete"
    assert result["r1_acceptance"] is None and result["performance_acceptance"] == "not_evaluated"
    bound(result["plan"])
    bound(plan["source_manifest"])
    bound(plan["replaces_invalid_attempt"])
    assert plan["source_files"] == {"baseline": source["before"], "candidate": source["after"]}
    assert plan["sources"] == {"baseline": provenance["baseline_root"], "candidate": provenance["candidate_root"]}
    assert set(plan["targets"]) == {"baseline", "candidate"}
    assert plan["targets"]["baseline"] != plan["targets"]["candidate"] and plan["fresh_targets_before"] is True
    assert plan["r1_acceptance"] is None
    assert list(plan["inputs"]) == ["river", "turn", "flop"]
    for ref in plan["inputs"].values():
        bound(ref)
    for role, pair in result["binaries"].items():
        assert role in ("baseline", "candidate")
        assert pair["frozen"]["path"] == join(fresh, "binaries/" + role + ".exe")
        assert pair["compiled"]["path"] == join(plan["targets"][role], "debug/examples/sol_codec_bench.exe")
        assert {k: pair["compiled"][k] for k in ("bytes", "sha256")} == {k: pair["frozen"][k] for k in ("bytes", "sha256")}
        bound(pair["frozen"])
        binary_aliases[pair["compiled"]["path"]] = pair["frozen"]["path"]
        assert b"R1_SOL_WRITE_PHASE_OUTPUT" not in raw(pair["frozen"]["path"])
    assert set(result["binaries"]) == {"baseline", "candidate"}
    assert result["binaries"]["baseline"]["frozen"]["sha256"] != result["binaries"]["candidate"]["frozen"]["sha256"]

    tools, records = {}, {}

    def verify_record(path, failed):
        record = read(path)
        assert record["schema"] == "solvers.supervised-run/v1"
        assert record["identity_unchanged"] is True and record["identity_before"] == record["identity_after"]
        assert record["cleanup_complete"] is True
        assert record["containment"]["kind"] == "windows_job_kill_on_close_suspended_assignment"
        assert record["argv"] == record["resolved_argv"]
        assert time(record["started_at"]) < time(record["ended_at"])
        for ref in record["identity_before"]:
            if ref["path"] in manifest["originals"] or ref["path"] in source_bytes or ref["path"] in binary_aliases:
                bound(ref)
            else:
                assert PureWindowsPath(ref["path"]).name in ("cargo.exe", "rustc.exe", "python.exe")
                if ref["path"] in tools:
                    assert tools[ref["path"]] == ref
                tools[ref["path"]] = ref
        for ref in record["outputs"].values():
            bound(ref)
        rows = [json.loads(line) for line in raw(record["outputs"]["samples"]["path"]).splitlines()]
        measure, limits = record["measurement"], record["limits"]
        assert rows and len(rows) == measure["sample_count"] and rows[-1] == record["last_sample"]
        assert rows[-1]["pids"] == [] and rows[-1]["tree_resident_bytes"] == 0
        elapsed = [row["elapsed_seconds"] for row in rows]
        assert all(a <= b for a, b in zip(elapsed, elapsed[1:]))
        assert max(row["tree_resident_bytes"] for row in rows) == measure["sampled_peak_tree_resident_bytes"]
        assert max(len(row["pids"]) for row in rows) == measure["max_observed_processes"]
        assert abs(max(b-a for a, b in zip(elapsed, elapsed[1:])) - measure["max_sample_gap_seconds"]) < 1e-6
        assert measure["sampled_peak_tree_resident_bytes"] <= limits["memory_limit_bytes"]
        assert min(row["host_available_memory_bytes"] for row in rows) >= limits["min_free_memory_bytes"]
        assert min(row["disk_free_bytes"] for row in rows) >= limits["disk_reserve_bytes"]
        assert record["elapsed_seconds"] < limits["timeout_seconds"]
        actual = [record[k] for k in ("state", "stop_reason", "supervisor_exit_code", "child_exit_code")]
        if failed:
            assert actual == ["failed", "descendants_after_root_exit", 1, 0]
            assert record["forced"] is True
            assert record["errors"] == [{"where": "graceful_signal", "type": "RuntimeError", "message": "CTRL_BREAK helper failed: [WinError 6] AttachConsole"}]
            assert [e["kind"] for e in record["events"]] == ["stop_requested", "forced_termination"]
            assert record["events"][0]["reason"] == "descendants_after_root_exit"
        else:
            assert actual == ["completed", "completed", 0, 0]
            assert record["forced"] is False and record["errors"] == [] and record["events"] == []
        return record

    # Preserve the original invalid shared-target trial, including its wrong binary.
    old_plan, old_result = read(join(root, "diagnostic-plan.json")), read(join(root, "diagnostic-result.json"))
    bound(old_result["plan"])
    assert old_result["status"] == "failed" and old_result["first_failure"] == "build-candidate"
    assert old_result["r1_acceptance"] is None
    assert old_plan["source_files"] == plan["source_files"]
    assert [s["label"] for s in old_result["stages"]] == ["build-baseline", "build-candidate"]
    assert manifest["failed_shared_attempt"] == {
        "no_tests_or_samples": True, "frozen_binaries": ["baseline.exe"],
        "records": ["build-baseline.json", "build-candidate.json"]}
    assert set(old_result["binaries"]) == {"baseline"}
    old_binary = old_result["binaries"]["baseline"]["frozen"]
    bound(old_binary)
    assert old_binary["sha256"] == "8248720a768f2a8a74c92b5bf66c8f698c105d20e62e2fe18a03babec32e9628"
    assert b"R1_SOL_WRITE_PHASE_OUTPUT" in raw(old_binary["path"])
    for item in old_result["stages"]:
        bound(item["record"])
        old_record = verify_record(item["record"]["path"], item["label"] == "build-candidate")
        expected = next(s for s in old_plan["stages"] if s["label"] == item["label"])
        assert old_record["argv"] == expected["argv"] and old_record["cwd"] == expected["cwd"]
        assert item["supervisor_exit"] == old_record["supervisor_exit_code"]
        assert item["source_unchanged"] is True
    assert time(old_result["ended_at"]) < time(plan["created_at"])

    stages = {s["label"]: s for s in plan["stages"]}
    assert len(stages) == 21
    planned_samples = [s for s in plan["stages"] if s["kind"] == "sample"]
    expected_samples = [f"{case}-{repeat}-{role}" for case in ("river", "turn", "flop")
                        for repeat in (1, 2, 3) for role in (("baseline", "candidate") if repeat % 2 else ("candidate", "baseline"))]
    assert [s["label"] for s in planned_samples] == expected_samples
    suffix = "-cleanup-confirmation"
    initial = ["build-baseline", "build-candidate", "formats-tests"]
    expected_labels = [label + end for label in initial for end in ("", suffix)] + expected_samples
    assert [s["label"] for s in result["stages"]] == expected_labels
    previous_end = time(plan["created_at"])
    tests, data = {}, {case: {role: [] for role in ("baseline", "candidate")} for case in plan["inputs"]}
    canonical_blobs, canonical_metadata = {}, {}
    for item in result["stages"]:
        label = item["label"]
        base_label = label.removesuffix(suffix)
        stage = stages[base_label]
        role = stage["role"]
        bound(item["record"])
        assert item["record"]["path"] == join(fresh, "records/" + label + ".json")
        record = verify_record(item["record"]["path"], label in initial)
        records[label] = record
        assert item["supervisor_exit"] == record["supervisor_exit_code"] and item["source_unchanged"] is True
        assert record["argv"] == stage["argv"] and record["cwd"] == stage["cwd"]
        assert previous_end < time(record["started_at"])
        previous_end = time(record["ended_at"])
        assert record["limits"]["timeout_seconds"] == (30 if label.endswith(suffix) else stage["timeout"])
        for key in ("min_free_memory_bytes", "disk_reserve_bytes"):
            assert record["limits"][key] == plan[key]
        assert record["limits"]["memory_limit_bytes"] == plan["sample_memory_bytes" if stage["kind"] == "sample" else "build_memory_bytes"]
        assert {k: record["limits"][k] for k in ("grace_seconds", "kill_wait_seconds", "poll_seconds")} == {
            "grace_seconds": 5, "kill_wait_seconds": 5, "poll_seconds": 0.05}
        identities = {ref["path"]: ref for ref in record["identity_before"]}
        assert result["plan"] == identities[join(fresh, "plan.json")]
        assert plan["source_manifest"] == identities[plan["source_manifest"]["path"]]
        if stage["kind"] != "sample":
            for name, expected in plan["source_files"][role].items():
                actual = identities[join(plan["sources"][role], name)]
                assert {k: actual[k] for k in ("bytes", "sha256")} == expected
            assert record["argv"][record["argv"].index("--target-dir") + 1] == plan["targets"][role]
        if label.endswith(suffix):
            assert item["compiled_binary_unchanged"] == result["binaries"][role]["compiled"]
            bound(item["compiled_binary_unchanged"])
            assert record["argv"] == records[base_label]["argv"]
        if stage["kind"] == "test":
            stdout = raw(record["outputs"]["stdout"]["path"]).decode("utf-8")
            counts = [list(map(int, group)) for group in SUMMARY.findall(stdout)]
            assert counts == [[58, 0, 0, 0, 0], [6, 0, 0, 0, 0], [13, 0, 0, 0, 0], [0, 0, 0, 0, 0]]
            for test in ("large_small_large_frames_do_not_carry_history_or_pledged_size", "reused_context_matches_fresh_frames_at_stream_buffer_boundaries"):
                assert "::" + test + " ... ok" in stdout
            assert "test result: FAILED" not in stdout
            tests[label] = {"passed": 77, "failed": 0, "supervisor_exit": record["supervisor_exit_code"]}
        if stage["kind"] != "sample":
            continue
        case = stage["case"]
        expected_binary = result["binaries"][role]["frozen"]
        expected_input = plan["inputs"][case]
        # Cross-match every real sample's executable and input before AND after.
        assert record["argv"] == [expected_binary["path"], expected_input["path"], "stream-write", "1", join(fresh, "samples/" + label)]
        for boundary in ("identity_before", "identity_after"):
            refs = {ref["path"]: ref for ref in record[boundary]}
            assert refs[record["argv"][0]] == expected_binary
            assert refs[record["argv"][1]] == expected_input
        bound(item["sample"])
        assert set(item["artifacts"]) == {"result.json", "rewritten.sol", "canonical.bin", "root-canonical.bin"}
        for name, ref in item["artifacts"].items():
            assert ref["path"] == join(fresh, "samples/" + label + "/" + name)
            bound(ref)
        assert item["sample"] == item["artifacts"]["result.json"]
        sample = read(item["sample"]["path"])
        assert sample == read(record["outputs"]["stdout"]["path"])
        assert sample["schema"] == "r1.sol-codec-sample/v1" and sample["status"] == "completed"
        assert sample["operation"] == "stream-write" and sample["iterations"] == 1
        assert item["operation_seconds"] == sample["timing"]["operation_seconds"] == sample["timing"]["operation_seconds_per_iteration"] > 0
        assert sample["input"]["bytes"] == expected_input["bytes"]
        assert raw(item["artifacts"]["rewritten.sol"]["path"]) == raw(expected_input["path"])
        assert sample["rewritten"]["bytes"] == expected_input["bytes"]
        assert sample["rewritten"]["blake3"] == sample["input"]["blake3"]
        for name, field in (("canonical.bin", "canonical"), ("root-canonical.bin", "root_canonical")):
            ref = item["artifacts"][name]
            assert sample[field]["bytes"] == ref["bytes"] and sample[field]["file"] == name
            key = (case, name)
            blob = manifest["originals"][ref["path"]]
            if key in canonical_blobs:
                assert canonical_blobs[key] == blob
            canonical_blobs[key] = blob
        normalized = copy.deepcopy(sample)
        normalized.pop("timing")
        if case in canonical_metadata:
            assert canonical_metadata[case] == normalized
        canonical_metadata[case] = normalized
        data[case][role].append({"label": label, "operation_seconds": item["operation_seconds"],
            "sampled_peak_tree_resident_bytes": record["measurement"]["sampled_peak_tree_resident_bytes"]})
    assert previous_end < time(result["ended_at"])
    assert (time(result["ended_at"]) - time(plan["created_at"])).total_seconds() < plan["overall_timeout_seconds"]
    measured = {}
    for case, roles in data.items():
        measured[case] = {}
        for role, values in roles.items():
            assert len(values) == 3
            times = [v["operation_seconds"] for v in values]
            peaks = [v["sampled_peak_tree_resident_bytes"] for v in values]
            median = statistics.median(times)
            assert median == result["descriptive_medians"][case][role]
            measured[case][role] = {"operation_seconds": times, "median_seconds": median,
                "sampled_peak_tree_resident_bytes": peaks, "median_sampled_peak_bytes": statistics.median(peaks)}
    assert all(result[k] is True for k in ("rewrites_equal_inputs", "canonical_and_root_equal", "source_unchanged"))
    return {"schema": "r1.context-reuse-retained-verification/v1", "status": "retained_bytes_verified",
        "source_revision": REVISION, "source_files_per_role": 197, "blob_count": len(manifest["blobs"]),
        "original_paths": len(manifest["originals"]), "fresh_stages": 24, "sample_binary_and_input_identity_matches": 18,
        "invalid_shared_target_trial": "failed_not_performance_evidence", "initial_cleanup_failures": initial,
        "tests": tests, "measured": measured, "all_rewrites_equal_inputs": True, "canonical_and_root_equal": True,
        "source_dependency": "Git revision " + REVISION + " must exist locally",
        "reran_build_tests_or_solver": False, "performance_acceptance": "not_evaluated", "r1_acceptance": None}


if __name__ == "__main__":
    print(json.dumps(verify(), indent=2))
