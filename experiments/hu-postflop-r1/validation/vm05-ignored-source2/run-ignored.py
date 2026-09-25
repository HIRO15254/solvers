#!/usr/bin/env python3
"""Run five fixed ignored checks on source2; root launches this on the Linux VM.

Usage: python3 run-ignored.py /opt/r1/ignored-source2 --source-label source2
The output directory must not exist. No solver campaign or source edit is made.
The process deadline includes compilation; each check needs its entire reserved
600 seconds plus cleanup time before it may start. An outer systemd service with
KillMode=control-group remains useful for supervisor death / VM shutdown.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import time


SOURCE = Path("/opt/r1/current")
TEST_TIMEOUT = 600.0
TOTAL_TIMEOUT = 1800.0
CLEANUP_TIMEOUT = 10.0
TESTS = (
    ("oracle_diff", "multistreet_engine_matches_scalar_oracle"),
    ("postflop", "iso_quotient_matches_full_tree_per_hand"),
    ("postflop", "member_branch_matches_suit_permuted_rep_branch"),
    ("postflop", "i16_storage_matches_f32_on_small_turn_spot"),
    ("rake_icm", "pure_hu_icm_postflop_solve_matches_chip_ev"),
)
STOP_SIGNAL = None


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def atomic_json(path, value):
    fd, temporary = tempfile.mkstemp(prefix=".checks-", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            json.dump(value, stream, indent=2, allow_nan=False)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def command(test_file, test_name):
    return [
        "cargo", "test", "--locked", "--release", "-p", "holdem",
        "--test", test_file, test_name, "--", "--ignored", "--exact",
        "--test-threads=1",
    ]


def passed_exactly_one(log_text, test_name):
    # A successful Cargo exit with a misspelled filter (zero tests) is not a pass.
    named_test = re.search(
        rf"^test {re.escape(test_name)} \.\.\. ok\s*$", log_text, re.MULTILINE
    )
    summary = re.search(
        r"^test result: ok\. 1 passed; 0 failed; 0 ignored;", log_text, re.MULTILINE
    )
    return named_test is not None and summary is not None


def stop_process_group(process, deadline):
    """Kill the session's entire group, including the test launched by Cargo."""
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    remaining = max(0.0, min(CLEANUP_TIMEOUT, deadline - time.monotonic()))
    try:
        process.wait(timeout=remaining)
    except subprocess.TimeoutExpired:
        return False
    return True


def request_stop(signum, _frame):
    global STOP_SIGNAL
    STOP_SIGNAL = signum


def run_check(check, output, environment, deadline):
    before = time.monotonic()
    # Recheck after the pre-launch report write, which also consumes the budget.
    if STOP_SIGNAL is not None:
        check["status"] = "not_run_interrupted"
        return
    if deadline - before < TEST_TIMEOUT + CLEANUP_TIMEOUT:
        check["status"] = "not_run_budget"
        return
    log_path = output / check["log"]
    check.update(status="running", started_utc=utc_now())
    process = None
    try:
        with log_path.open("wb") as stream:
            try:
                process = subprocess.Popen(
                    check["argv"], cwd=SOURCE, env=environment,
                    stdin=subprocess.DEVNULL, stdout=stream,
                    stderr=subprocess.STDOUT, start_new_session=True,
                    close_fds=True,
                )
                check["pid"] = process.pid
                command_deadline = min(before + TEST_TIMEOUT, deadline - CLEANUP_TIMEOUT)
                while True:
                    if STOP_SIGNAL is not None:
                        check["status"] = "interrupted"
                        check["signal"] = STOP_SIGNAL
                        break
                    remaining = command_deadline - time.monotonic()
                    if remaining <= 0:
                        check["status"] = "timed_out"
                        break
                    try:
                        process.wait(timeout=min(1.0, remaining))
                        check["status"] = "completed"
                        break
                    except subprocess.TimeoutExpired:
                        continue
            except OSError as error:
                check.update(status="launch_failed", error=str(error))
            finally:
                if process is not None:
                    if check["status"] != "completed":
                        check["leader_reaped_after_group_kill"] = stop_process_group(process, deadline)
                    check["returncode"] = process.returncode
    finally:
        # Return code alone cannot distinguish a timeout kill from another signal.
        check["seconds"] = time.monotonic() - before
        check["finished_utc"] = utc_now()
        if log_path.exists():
            check["log_artifact"] = {
                "status": "present", "bytes": log_path.stat().st_size,
                "sha256": digest(log_path),
            }
    if check["status"] == "completed":
        if check["returncode"] != 0:
            check["status"] = "failed"
        elif passed_exactly_one(log_path.read_text(encoding="utf-8", errors="replace"), check["test"]):
            check["status"] = "passed"
        else:
            check["status"] = "expected_test_not_confirmed"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--source-label", default="source2")
    args = parser.parse_args()
    if os.name != "posix":
        parser.error("this fixed VM runner requires POSIX process groups")
    if not SOURCE.is_dir():
        parser.error(f"source directory does not exist: {SOURCE}")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    environment = os.environ.copy()
    environment.update(
        CARGO_HOME="/opt/r1/cargo", RUSTUP_HOME="/opt/r1/rustup",
        RUSTUP_TOOLCHAIN="1.97.0", CARGO_TARGET_DIR="/opt/r1/target/candidate",
        CARGO_BUILD_JOBS="4", CARGO_TERM_COLOR="never", RUST_TEST_COLOR="never",
    )
    environment["PATH"] = "/opt/r1/cargo/bin:" + environment.get("PATH", "")
    checks = [
        {
            "test_binary": binary, "test": test, "argv": command(binary, test),
            "status": "not_run", "returncode": None, "seconds": None,
            "log": f"{index:02d}-{test}.log",
            "log_artifact": {"status": "not_created"},
        }
        for index, (binary, test) in enumerate(TESTS, start=1)
    ]
    started = time.monotonic()
    deadline = started + TOTAL_TIMEOUT
    report = {
        "schema": "r1-ignored-checks/v1", "source_label": args.source_label,
        "source_root": str(SOURCE), "started_utc": utc_now(), "status": "running",
        "limits_seconds": {"per_test": TEST_TIMEOUT, "total": TOTAL_TIMEOUT, "cleanup": CLEANUP_TIMEOUT},
        "environment": {key: environment.get(key) for key in (
            "HOME", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN",
            "CARGO_TARGET_DIR", "CARGO_BUILD_JOBS",
        )},
        # These hashes supplement the source2 label; they do not certify a full snapshot.
        "source_file_sha256": {
            path: digest(SOURCE / path) for path in (
                "Cargo.lock", ".cargo/config.toml", "crates/holdem/tests/oracle_diff.rs",
                "crates/holdem/tests/postflop.rs", "crates/holdem/tests/rake_icm.rs",
            )
        },
        "runner_sha256": digest(Path(__file__)), "checks": checks,
    }
    signal.signal(signal.SIGTERM, request_stop)
    signal.signal(signal.SIGINT, request_stop)
    destination = output / "checks.json"
    atomic_json(destination, report)
    try:
        for check in checks:
            remaining = deadline - time.monotonic()
            check["remaining_before_seconds"] = max(0.0, remaining)
            if STOP_SIGNAL is not None:
                check["status"] = "not_run_interrupted"
            elif any(item["status"] in ("timed_out", "interrupted", "launch_failed") for item in checks):
                # Reaping Cargo's leader is not proof that every descendant has
                # disappeared. Never overlap another check after an abnormal stop.
                check["status"] = "not_run_previous_abnormal_exit"
            elif remaining < TEST_TIMEOUT + CLEANUP_TIMEOUT:
                check["status"] = "not_run_budget"
            else:
                # A supervisor crash leaves an explicit incomplete record, not a pass.
                check["status"] = "starting"
                atomic_json(destination, report)
                run_check(check, output, environment, deadline)
            atomic_json(destination, report)
            print(json.dumps(check), flush=True)
    except Exception as error:
        report["runner_error"] = f"{type(error).__name__}: {error}"
        for check in checks:
            if check["status"] in ("starting", "running"):
                check["status"] = "runner_failed"
            elif check["status"] == "not_run":
                check["status"] = "not_run_runner_failed"
    report.update(
        status="passed" if all(check["status"] == "passed" for check in checks) else "failed",
        seconds=time.monotonic() - started, finished_utc=utc_now(), signal=STOP_SIGNAL,
    )
    atomic_json(destination, report)
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
