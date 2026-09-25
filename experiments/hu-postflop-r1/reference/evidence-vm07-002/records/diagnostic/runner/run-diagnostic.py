#!/usr/bin/env python3
"""Run a separate observed river input archive against an explicitly pinned binary.

Root transfers/extracts the input archive and launches this after the main
pipeline stops. This runner never builds, transfers, extracts, or retries.
Use an outer systemd cgroup (KillMode=control-group, RuntimeMaxSec=1830,
TimeoutStopSec=15, MemoryMax=40G); the per-stage supervisor is not a cgroup.
The 30-second config stop remains a soft solver condition, not an acceptance
threshold. Execution and diagnostic comparison have separate reports.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
import time


REPO = Path("/opt/r1/candidate-v3")
BINARY = Path("/opt/r1/target/candidate-v3/release/solvers")
REQUIRED_INPUTS = {
    "diagnostic.toml", "observed.json", "oop-range.txt", "ip-range.txt",
    "range-integrity.json", "check_diagnostic.py",
}
ALLOWED_INPUTS = REQUIRED_INPUTS | {"README.md", "test_check_diagnostic.py", "check_ranges.py"}
TOTAL_SECONDS = 1800.0
LIMITS = {
    "timeout_seconds": 600, "grace_seconds": 5, "kill_wait_seconds": 5,
    "poll_seconds": 0.1, "memory_limit_bytes": 40 * 1024**3,
    "min_free_memory_bytes": 8 * 1024**3, "disk_reserve_bytes": 10 * 1024**3,
}


def load_supervisor(path):
    spec = importlib.util.spec_from_file_location("diagnostic_supervisor", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def checked_identity(supervisor, path, expected_sha256=None):
    result = supervisor.identity(path)
    if expected_sha256 is not None and result["sha256"] != expected_sha256:
        raise ValueError(f"SHA-256 mismatch: {path}")
    return result


def verify_archive(archive, input_dir, supervisor, case_id="HU-R0-019"):
    """Compare selected regular archive bytes with separately extracted inputs."""
    if archive.stat().st_size > 16 * 1024**2:
        raise ValueError("diagnostic input archive exceeds 16 MiB")
    records, prefixes = {}, set()
    required, allowed = set(REQUIRED_INPUTS), set(ALLOWED_INPUTS)
    if case_id == "HU-R0-002":
        required.update({"build_diagnostic.py", "check_menus.py", "check_ranges.py"})
        required.update(f"menu-capture-{index:02d}.{extension}"
                        for index in range(1, 20) for extension in ("json", "txt"))
        allowed.update(required)
        allowed.update({"test_check_menus.py", "diagnostic-input-check.json"})
    total_bytes = 0
    with tarfile.open(archive, "r:gz") as stream:
        for member in stream:
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or "\\" in member.name:
                raise ValueError(f"invalid diagnostic archive member: {member.name}")
            if member.isdir():
                continue
            if not member.isfile() or name.name not in allowed:
                raise ValueError(f"unexpected diagnostic archive member: {member.name}")
            if name.name in records or member.size > 1024**2:
                raise ValueError(f"duplicate or oversized archive member: {member.name}")
            total_bytes += member.size
            if total_bytes > 8 * 1024**2:
                raise ValueError("diagnostic input contents exceed 8 MiB")
            prefixes.add(str(name.parent))
            path = input_dir / name.name
            if path.is_symlink() or not path.is_file():
                raise ValueError(f"diagnostic input is not a regular file: {path}")
            identity = supervisor.identity(path)
            with stream.extractfile(member) as contents:
                digest = hashlib.sha256(contents.read()).hexdigest()
            if identity["sha256"] != digest or identity["bytes"] != member.size:
                raise ValueError(f"archive/extracted input mismatch: {name.name}")
            records[name.name] = {**identity, "archive_member": member.name}
    if not required.issubset(records) or len(prefixes) != 1:
        raise ValueError("archive must contain required files together in one directory")
    return records


def concurrent_workloads():
    """A point-in-time check; root must also keep other launchers stopped."""
    found = []
    for process in Path("/proc").iterdir():
        if not process.name.isdigit() or int(process.name) == os.getpid():
            continue
        try:
            raw = (process / "cmdline").read_bytes()
        except (FileNotFoundError, ProcessLookupError):
            continue
        argv = [os.fsdecode(part) for part in raw.split(b"\0") if part]
        if argv and (Path(argv[0]).name == "solvers" or any(
            Path(arg).name == "run_campaign.py" for arg in argv[1:]
        )):
            found.append({"pid": int(process.name), "argv": argv})
    return found


def stage_plan(binary, input_dir, output, source_id):
    config = input_dir / "diagnostic.toml"
    checker = input_dir / "check_diagnostic.py"
    run = output / "run"
    solution = run / "solution.sol"
    tree, summary = output / "tree.json", output / "summary.json"
    return [
        ("input_check", [sys.executable, str(checker), "--check-inputs",
                         "--output", str(output / "input-check.json")]),
        ("config_validate", [str(binary), "validate", str(config)]),
        ("solve", [str(binary), "solve", str(config), "--out", str(run), "--sol-streets", "full"]),
        ("export_tree", [str(binary), "export", str(solution), "tree", "--node", "all",
                         "--output", str(tree)]),
        ("export_summary", [str(binary), "export", str(solution), "summary", "--output", str(summary)]),
        ("compare", [sys.executable, str(checker), "--tree", str(tree), "--summary", str(summary),
                     "--run-config", str(run / "run.toml"), "--source-id", source_id,
                     "--output", str(output / "diagnostic-comparison.json")]),
    ]


def successful(stage):
    return (stage.get("state") == "completed" and stage.get("supervisor_exit_code") == 0
            and stage.get("child_exit_code") == 0 and stage.get("cleanup_complete") is True
            and stage.get("identity_unchanged") is True)


def supervise_stage(supervisor, stage, output, immutable, deadline, repo=REPO):
    required = sum(LIMITS[key] for key in ("timeout_seconds", "grace_seconds", "kill_wait_seconds"))
    if deadline - time.monotonic() < required:
        stage["state"] = "not_run_budget"
        return
    overlaps = concurrent_workloads()
    if overlaps:
        stage.update(state="not_run_concurrent_workload", concurrent_workloads=overlaps)
        return
    for expected in immutable:
        checked_identity(supervisor, Path(expected["path"]), expected["sha256"])
    directory = output / "stages" / stage["name"]
    directory.mkdir(parents=True, exist_ok=False)
    record = directory / "supervisor.json"
    arguments = ["--record", str(record), "--cwd", str(repo),
                 "--stdout", str(directory / "stdout.log"), "--stderr", str(directory / "stderr.log"),
                 "--disk-path", str(output)]
    for name, value in LIMITS.items():
        arguments += ["--" + name.replace("_", "-"), str(value)]
    for expected in immutable:
        arguments += ["--identity-file", expected["path"]]
    # Budget accounting includes preflight identity work and workload detection.
    if deadline - time.monotonic() < required:
        stage["state"] = "not_run_budget"
        return
    code = supervisor.main([*arguments, "--", *stage["argv"]])
    stage["supervisor_exit_code"] = code
    if not record.is_file():
        stage["state"] = "supervisor_record_missing"
        return
    result = read_json(record)
    stage.update({key: result.get(key) for key in (
        "state", "stop_reason", "child_exit_code", "cleanup_complete", "identity_unchanged",
        "elapsed_seconds", "measurement", "outputs",
    )})
    stage["supervisor_record"] = supervisor.identity(record)


def sha256_argument(value):
    if re.fullmatch(r"[0-9a-fA-F]{64}", value) is None:
        raise argparse.ArgumentTypeError("expected a complete SHA-256 hex digest")
    return value.lower()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case-id", choices=("HU-R0-002", "HU-R0-017", "HU-R0-019"), default="HU-R0-019")
    parser.add_argument("--repo", type=Path, default=REPO)
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--input-archive", type=Path, required=True)
    parser.add_argument("--input-archive-sha256", type=sha256_argument, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=BINARY)
    parser.add_argument("--binary-sha256", type=sha256_argument, required=True)
    parser.add_argument("--source-id", required=True)
    args = parser.parse_args(argv)
    if sys.platform != "linux":
        parser.error("this VM runner requires Linux /proc and process groups")
    started = time.monotonic()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    repo = args.repo.resolve(strict=True)
    supervisor_path = repo / "tools" / "run_supervised.py"
    supervisor = load_supervisor(supervisor_path)
    # Identity checks below record missing inputs as preflight failures.
    input_dir, binary = args.input_dir.resolve(), args.binary.resolve()
    archive = args.input_archive.resolve()
    stages = [{"name": name, "argv": command, "state": "not_run", "child_exit_code": None}
              for name, command in stage_plan(binary, input_dir, output, args.source_id)]
    report = {"schema": "solvers.r1-diagnostic-execution/v1", "case_id": args.case_id,
              "source_id": args.source_id, "state": "running", "started_utc": supervisor.utc_now(),
              "limits": LIMITS, "total_seconds_limit": TOTAL_SECONDS, "stages": stages,
              "concurrency_check": "point-in-time /proc check; root keeps other launchers stopped",
              "artifacts": {}}
    comparison = {"schema": "solvers.r1-diagnostic-comparison-status/v1", "case_id": args.case_id,
                  "state": "not_run", "condition_match": "unverified", "quality_status": "not_evaluated",
                  "acceptance": None, "comparison_threshold": None,
                  "reason": "legacy reference rake/accuracy conditions remain uncertified"}
    execution_path, comparison_path = output / "execution.json", output / "comparison-status.json"
    supervisor.atomic_json(execution_path, report, initial=True)
    supervisor.atomic_json(comparison_path, comparison, initial=True)
    immutable = []
    try:
        report["runner"] = supervisor.identity(Path(__file__))
        report["supervisor"] = supervisor.identity(supervisor_path)
        report["binary"] = checked_identity(supervisor, binary, args.binary_sha256)
        report["input_archive"] = checked_identity(supervisor, archive, args.input_archive_sha256)
        report["inputs"] = verify_archive(archive, input_dir, supervisor, args.case_id)
        if read_json(input_dir / "observed.json").get("case_id") != args.case_id:
            raise ValueError("observed input case does not match requested diagnostic")
        immutable = [report[key] for key in ("runner", "supervisor", "binary", "input_archive")]
        immutable += list(report["inputs"].values())
        for index, stage in enumerate(stages):
            stage["state"] = "starting"
            supervisor.atomic_json(execution_path, report)
            supervise_stage(supervisor, stage, output, immutable, started + TOTAL_SECONDS, repo)
            supervisor.atomic_json(execution_path, report)
            print(json.dumps({"stage": stage["name"], "state": stage["state"]}), flush=True)
            if not successful(stage):
                for later in stages[index + 1:]:
                    later["state"] = "not_run_prior_failure"
                break
        computational = stages[:-1]
        report["state"] = "completed" if all(successful(stage) for stage in computational) else "failed"
        if successful(stages[-1]):
            actual = read_json(output / "diagnostic-comparison.json")
            if not (actual.get("case_id") == args.case_id
                    and actual.get("condition_match") == "unverified"
                    and actual.get("quality_status") == "not_evaluated"
                    and actual.get("acceptance") is None
                    and actual.get("tree_check", {}).get("status") == "all_observed_menus_match"):
                comparison.update(state="failed", error="diagnostic checker returned unexpected comparison semantics")
            else:
                comparison.update(state="menus_matched_ev_diagnostic_only",
                                  report=supervisor.identity(output / "diagnostic-comparison.json"))
        elif stages[-1]["state"].startswith("not_run"):
            comparison["state"] = "not_run"
        else:
            comparison["state"] = "failed"
    except BaseException as error:
        report.update(state="failed", error=f"{type(error).__name__}: {error}")
        comparison["state"] = "failed" if stages[-1]["state"] != "not_run" else "not_run"
        for stage in stages:
            if stage["state"] == "starting":
                stage["state"] = "runner_error"
            elif stage["state"] == "not_run":
                stage["state"] = "not_run_runner_error"
    finally:
        # Preserve absent output and execution status separately; partial artifacts
        # after timeout/error are evidence, never successful diagnostic outputs.
        for name in ("input-check.json", "tree.json", "summary.json", "diagnostic-comparison.json",
                     "run/run.toml", "run/run.json", "run/solution.sol", "run/checkpoint.ckpt",
                     "run/progress.jsonl"):
            path = output / name
            try:
                report["artifacts"][name] = ({"status": "present", **supervisor.identity(path)}
                                             if path.is_file() else {"status": "not_created"})
            except OSError as error:
                report["artifacts"][name] = {"status": "unreadable", "error": str(error)}
                report["state"] = "failed"
        report.update(seconds=time.monotonic() - started, finished_utc=supervisor.utc_now())
        supervisor.atomic_json(comparison_path, comparison)
        supervisor.atomic_json(execution_path, report)
    return 0 if report["state"] == "completed" and comparison["state"] == "menus_matched_ev_diagnostic_only" else 1


if __name__ == "__main__":
    raise SystemExit(main())
