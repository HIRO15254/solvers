"""Verify retained EV-scratch validation using only the proof's pinned bytes.

No retained Python source is executed. Nonzero command/test outcomes remain
failures even when their evidence is internally consistent. This verifies
correctness checks and containment records, not isolated performance.
"""
from __future__ import annotations

import argparse
import ast
import gzip
import hashlib
import io
import json
import math
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import tarfile

HERE = Path(__file__).resolve().parent
PREFIX = "experiments/hu-postflop-r1/validation/ev-scratch/"
RAW_NAMES = {"receipt.json", "record.json", "record.samples.jsonl", "record.stderr.log",
             "record.stdout.log", "source.diff", "value_scratch.rs",
             "wrapper.stderr.log", "wrapper.stdout.log"}
STAGES = {
    "fmt": (["fmt", "--all", "--check"], 30),
    "clippy": (["clippy", "--locked", "--offline", "--workspace", "--all-targets", "--", "-D", "warnings"], 300),
    "tests": (["test", "--locked", "--offline", "--workspace", "--", "--test-threads=4"], 1800),
    "edge-tests": (["test", "--locked", "--offline", "-p", "engine", "--test", "value_scratch", "--", "--test-threads=1"], 120),
    "release-values": (["test", "--locked", "--offline", "--release", "-p", "engine", "--test", "parallel", "--test", "value_scratch", "--", "--test-threads=1"], 300),
    "release-oracle": (["test", "--locked", "--offline", "--release", "-p", "holdem", "--test", "oracle_diff", "--", "--include-ignored", "--test-threads=1"], 300),
    "release-postflop": (["test", "--locked", "--offline", "--release", "-p", "holdem", "--test", "postflop", "--", "--include-ignored", "--test-threads=1", "flop_solve_is_zero_sum", "allin_runout_matches_direct_equity", "iso_quotient_matches_full_tree_per_hand", "i16_storage_matches_f32_on_small_turn_spot"], 300),
}
SUMMARY = re.compile(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;")


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def relative(name):
    name = name.replace("\\", "/")
    path = PurePosixPath(name)
    if not name or path.is_absolute() or ".." in path.parts or ":" in name or str(path) != name:
        raise ValueError(f"Unsafe relative path: {name!r}")
    return name


def within_windows(name, root):
    return relative(str(PureWindowsPath(name).relative_to(PureWindowsPath(root))))


def calibration(source):
    tree = ast.parse(source.decode("utf-8"))
    for item in tree.body:
        if isinstance(item, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "CALIBRATION" for t in item.targets):
            value = ast.literal_eval(item.value)
            if isinstance(value, str):
                return value
    raise ValueError("Pinned check.py has no literal calibration command")


def inspect_run(raw, source, original_dir):
    """Return all observed outcomes; raise only for unreadable proof data."""
    receipt = json.loads(raw["receipt.json"])
    record = json.loads(raw["record.json"])
    stage = receipt["stage"]
    errors = []

    def require(ok, message):
        if not ok:
            errors.append(message)

    require(set(raw) == RAW_NAMES, "Unexpected or missing raw files")
    require(receipt.get("status") == "completed" and receipt.get("exit_code") == 0, "Validation driver did not pass")
    before = {relative(k): v for k, v in receipt["source_before"].items()}
    after = {relative(k): v for k, v in receipt.get("source_after", {}).items()}
    require(before == after and receipt.get("source_unchanged") is True, "Receipt source changed")
    require(all(name in source and pin(source[name]) == expected for name, expected in before.items()), "Receipt source snapshot mismatch")
    require(raw["value_scratch.rs"] == source["crates/engine/tests/value_scratch.rs"], "Copied regression test differs")
    require(receipt.get("source_diff_file") == "source.diff" and receipt.get("new_test_source_file") == "value_scratch.rs", "Receipt sidecar names differ")
    require(re.fullmatch(r"[0-9a-f]{40}", receipt.get("head", "")) is not None, "Missing source revision")
    root = record["cwd"]
    command = record["argv"]
    if stage == "calibrate":
        timeout = 15
        expected_tail = ["-B", "-c", calibration(source[PREFIX + "check.py"])]
        require(PureWindowsPath(command[0]).name.lower() == "python.exe", "Calibration executable differs")
    else:
        expected_tail, timeout = STAGES[stage]
        require(PureWindowsPath(command[0]).name.lower() == "cargo.exe", "Cargo executable differs")
    require(command[1:] == expected_tail and record.get("resolved_argv") == command, "Stage command differs")
    join = lambda name: str(PureWindowsPath(root) / name)
    expected_wrapper = [receipt["command"][0], "-B", join(PREFIX + "run_bounded.py"),
                        "--record", str(PureWindowsPath(original_dir) / "record.json"), "--cwd", root,
                        "--timeout-seconds", str(timeout), "--grace-seconds", "0.5", "--poll-seconds", "0.2",
                        "--memory-limit-bytes", "1006632960", "--min-free-memory-bytes", "1610612736",
                        "--disk-reserve-bytes", "1073741824"]
    identity_names = ["crates/engine/src/solver.rs", "Cargo.lock"]
    if stage in ("release-oracle", "release-postflop"):
        identity_names.append(PREFIX + "extra.py")
    identity_names.append(PREFIX + "check.py")
    for name in identity_names:
        expected_wrapper += ["--identity-file", join(name)]
    expected_wrapper += ["--", *command]
    require(receipt["command"] == expected_wrapper, "Supervisor invocation differs")
    expected_env = {"CARGO_TARGET_DIR": join("target/r1-local-tests"), "CARGO_BUILD_JOBS": "1",
                    "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "RUSTFLAGS": "",
                    "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "4" if stage == "tests" else "1",
                    "RUSTC": receipt["environment"].get("RUSTC")}
    require(receipt["environment"] == expected_env and PureWindowsPath(expected_env["RUSTC"]).name.lower() == "rustc.exe", "Build environment differs")
    limits = {"timeout_seconds": timeout, "grace_seconds": 0.5, "kill_wait_seconds": 5.0, "poll_seconds": 0.2,
              "memory_limit_bytes": 1006632960, "hard_job_commit_limit_bytes": 1073741824,
              "minimum_available_commit_before_launch_bytes": 2147483648, "root_priority_class": 16384,
              "min_free_memory_bytes": 1610612736, "disk_reserve_bytes": 1073741824}
    require(record.get("limits") == limits, "Resource limits differ")
    require(record.get("bounded_job_settings") == {"limit_flags": 8704, "job_memory_limit_bytes": 1073741824,
                                                   "root_priority_class": 16384, "verified_before_resume": True}, "Queried Job settings differ")
    require(record.get("schema") == "solvers.supervised-run/v1" and record.get("shell") is False, "Supervisor schema/shell differs")
    require(record.get("state") == "completed" and record.get("stop_reason") == "completed", "Supervisor did not complete normally")
    require(record.get("child_exit_code") == 0 and record.get("supervisor_exit_code") == 0, "Nonzero supervised exit")
    require(record.get("cleanup_complete") is True and record.get("forced") is False, "Cleanup incomplete or forced")
    require(record.get("events") == [] and record.get("errors") == [] and record.get("stop_requested_at") is None, "Supervisor errors/stop events")
    require(record.get("containment", {}).get("kind") == "windows_job_kill_on_close_suspended_assignment", "Containment differs")
    require(record.get("host_before", {}).get("commit_available_bytes", 0) >= 2147483648, "Insufficient launch commit reserve")
    require(record.get("host_before", {}).get("available_bytes", 0) >= 1610612736, "Insufficient launch physical reserve")
    require(record.get("disk_free_before_bytes", 0) >= 1073741824, "Insufficient launch disk reserve")
    identities = record.get("identity_before", [])
    require(identities and identities == record.get("identity_after") and record.get("identity_unchanged") is True, "Supervisor identities changed")
    identity_by_path = {str(PureWindowsPath(item["path"])): item for item in identities}
    require(len(identity_by_path) == len(identities), "Duplicate supervisor identity")
    require(command[0] in identity_by_path and receipt["command"][0] in identity_by_path, "Executable identity missing")
    require(all(join(name) in identity_by_path for name in identity_names), "Required source identity missing")
    for item in identities:
        try:
            name = within_windows(item["path"], root)
        except ValueError:
            continue
        require(name in source and pin(source[name]) == {k: item[k] for k in ("bytes", "sha256")}, f"Identity snapshot mismatch: {name}")
    provenance = record["research_wrapper"]
    for key in ("wrapper", "transformer", "base"):
        name = within_windows(provenance[key + "_path"], root)
        require(name in source and pin(source[name])["sha256"] == provenance[key + "_sha256"], f"{key} provenance differs")
    for key, name in (("stdout", "record.stdout.log"), ("stderr", "record.stderr.log"), ("samples", "record.samples.jsonl")):
        item = record["outputs"][key]
        require(item["path"] == str(PureWindowsPath(original_dir) / name) and pin(raw[name]) == {k: item[k] for k in ("bytes", "sha256")}, f"Raw {key} pin mismatch")
    samples = [json.loads(line) for line in raw["record.samples.jsonl"].splitlines() if line]
    measure = record["measurement"]
    require(len(samples) == measure.get("sample_count") and len(samples) >= 2, "Sample count differs")
    if samples:
        elapsed = [s["elapsed_seconds"] for s in samples]
        require(all(math.isfinite(t) and t >= 0 for t in elapsed) and elapsed == sorted(elapsed), "Invalid sample times")
        require(samples[-1] == record.get("last_sample") and samples[-1]["pids"] == [], "Final containment sample differs")
        require(max(s["tree_resident_bytes"] for s in samples) == measure["sampled_peak_tree_resident_bytes"], "Sampled resident peak differs")
        require(max(len(s["pids"]) for s in samples) == measure["max_observed_processes"], "Sampled process count differs")
        require(all(s["host_available_memory_bytes"] >= 1610612736 and s["disk_free_bytes"] >= 1073741824
                    and 0 <= s["tree_resident_bytes"] <= 1006632960 for s in samples), "A sampled resource trigger was crossed")
        require(all(measure[key] == samples[-1][key] for key in ("root_os_peak_resident_bytes", "root_os_peak_source", "job_os_peak_commit_bytes")), "Final kernel peak accounting differs")
        gap = max((b - a for a, b in zip(elapsed, elapsed[1:])), default=0)
        require(math.isclose(gap, measure["max_sample_gap_seconds"], abs_tol=1e-7), "Sample maximum gap differs")
        require(elapsed[-1] <= record["elapsed_seconds"] <= timeout + 5.5, "Recorded duration exceeds bound")
    output = (raw["record.stdout.log"] + b"\n" + raw["record.stderr.log"]).decode("utf-8", errors="replace")
    matches = SUMMARY.findall(output)
    totals = {key: sum(int(row[i]) for row in matches) for i, key in enumerate(("passed", "failed", "ignored", "measured", "filtered"), 1)}
    require(not any(row[0] == "FAILED" for row in matches) and totals["failed"] == 0, "Raw test failures")
    if stage in ("tests", "edge-tests", "release-values", "release-oracle", "release-postflop"):
        require(bool(matches) and totals["passed"] > 0, "No successful raw test summaries")
    if stage == "calibrate":
        observations = [json.loads(line) for line in raw["record.stdout.log"].splitlines() if line]
        require(len(observations) == 2, "Calibration observation count differs")
        if len(observations) == 2:
            small, large = observations
            require(small == {"requested_commit_bytes": 65536, "success": True, "error": 0, "freed": True}, "Small commit was not allocated and freed")
            require(large.get("requested_commit_bytes") == 1073807360 and large.get("success") is False
                    and large.get("freed") is None and large.get("error", 0) != 0, "Over-cap commit was not denied")
        # A rejected allocation can raise PeakJobMemoryUsed above the cap.
        # The queried settings and actual allocation outcome are the evidence.
    return {"stage": stage, "original_dir": original_dir, "status": "pass" if not errors else "fail",
            "errors": errors, "test_summaries": len(matches), "test_totals": totals,
            "elapsed_seconds": record.get("elapsed_seconds"), "job_peak_commit_bytes": measure.get("job_os_peak_commit_bytes"),
            "sampled_peak_resident_bytes": measure.get("sampled_peak_tree_resident_bytes")}


def verify(proof):
    proof = proof.resolve()
    manifest = json.loads((proof / "manifest.json").read_text(encoding="utf-8"))
    if manifest["schema"] != "solvers.ev-scratch-validation/v1":
        raise ValueError("Unsupported proof schema")
    expected_paths = {"manifest.json"}

    def unpack(item):
        name = relative(item["path"])
        if name in expected_paths:
            raise ValueError(f"Duplicate payload: {name}")
        expected_paths.add(name)
        data = (proof / name).read_bytes()
        if pin(data) != item["compressed"]:
            raise ValueError(f"Compressed pin mismatch: {name}")
        raw = gzip.decompress(data)
        if pin(raw) != item["original"]:
            raise ValueError(f"Uncompressed pin mismatch: {name}")
        return raw

    source = {}
    source_tar = unpack(manifest["source_snapshot"])
    with tarfile.open(fileobj=io.BytesIO(source_tar), mode="r:") as archive:
        for member in archive:
            name = relative(member.name)
            if not member.isfile() or name in source:
                raise ValueError("Invalid or duplicate source archive member")
            source[name] = archive.extractfile(member).read()
    if {name: pin(data) for name, data in source.items()} != manifest["source_files"]:
        raise ValueError("Source snapshot pins differ")
    results = []
    for run in manifest["runs"]:
        raw = {relative(name): unpack(item) for name, item in run["files"].items()}
        result = inspect_run(raw, source, run["original_dir"])
        if result["stage"] != run["stage"]:
            raise ValueError("Manifest stage differs from receipt")
        results.append(result)
    actual_paths = {p.relative_to(proof).as_posix() for p in proof.rglob("*") if p.is_file()}
    if actual_paths != expected_paths:
        raise ValueError("Unmanifested or missing proof files")
    return {"status": "pass" if results and all(r["status"] == "pass" for r in results) else "fail",
            "source_files": len(source), "payload_files": len(expected_paths) - 1, "runs": results,
            "scope": "Retained command, source, raw test outcomes and containment evidence; no isolated performance claim"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("proof", type=Path)
    args = parser.parse_args()
    try:
        report = verify(args.proof)
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        report = {"status": "fail", "integrity_error": str(error)}
    print(json.dumps(report, indent=2))
    raise SystemExit(0 if report["status"] == "pass" else 1)


if __name__ == "__main__":
    main()
