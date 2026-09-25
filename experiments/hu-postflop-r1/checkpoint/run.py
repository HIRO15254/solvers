#!/usr/bin/env python3
"""One bounded real interrupt/resume contrast; uses the existing OS supervisor."""
from __future__ import annotations

import argparse
import copy
import datetime as dt
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import re
import signal
import sys
import time
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SPEC = importlib.util.spec_from_file_location("checkpoint_supervisor", ROOT / "tools/run_supervised.py")
S = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(S)


def require(ok, message):
    if not ok:
        raise ValueError(message)


def read(path):
    return decode_json(Path(path).read_text(encoding="utf-8"))


def decode_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    result = json.loads(raw, object_pairs_hook=pairs)
    def finite(value):
        if isinstance(value, float):
            require(math.isfinite(value), "nonfinite JSON")
        elif isinstance(value, dict):
            for child in value.values():
                finite(child)
        elif isinstance(value, list):
            for child in value:
                finite(child)
    finite(result)
    return result


def number(value):
    return type(value) in (int, float) and math.isfinite(value)


def same_values(left, right):
    """Preserve signed floating zero as well as every unexcluded JSON field."""
    if type(left) is not type(right):
        return False
    if isinstance(left, float):
        return left.hex() == right.hex()
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(same_values(left[k], right[k]) for k in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(same_values(a, b) for a, b in zip(left, right))
    return left == right


def path_key(value):
    # Rust canonicalize uses an extended Windows prefix; the supplied CLI path need not.
    value = str(Path(value).resolve(strict=True))
    if os.name == "nt":
        if value.startswith("\\\\?\\UNC\\"):
            value = "\\\\" + value[8:]
        elif value.startswith("\\\\?\\"):
            value = value[4:]
    return os.path.normcase(value)


def verify(ref):
    require(S.identity(Path(ref["path"])) == ref, "changed file: " + ref["path"])


def refs_in(value):
    if isinstance(value, dict):
        if set(value) == {"path", "bytes", "sha256"}:
            yield value
        else:
            for child in value.values():
                yield from refs_in(child)
    elif isinstance(value, list):
        for child in value:
            yield from refs_in(child)


def tree_identity(directory):
    result = {}
    for path in sorted(directory.rglob("*")):
        require(not path.is_symlink(), "unexpected artifact symlink")
        if path.is_file():
            result[path.relative_to(directory).as_posix()] = S.identity(path)
    require(result, "missing run artifacts")
    return result


class CheckpointTrigger:
    """Observe a header only; the independent Rust audit decodes the final payload."""
    def __init__(self, checkpoint, target, send=lambda: signal.raise_signal(signal.SIGINT)):
        self.checkpoint, self.target, self.send = checkpoint, target, send
        self.record = {"requested": False, "observed_iteration": None, "header_hex": None,
                       "requested_at": None, "request": "SIGINT_to_supervisor",
                       "enabled": True, "disabled_reason": None, "first_failure": None}

    def disable(self, reason):
        self.record["enabled"] = False
        if self.record["disabled_reason"] is None:
            self.record["disabled_reason"] = reason

    def failed(self, error):
        if self.record["first_failure"] is None:
            self.record["first_failure"] = {"type": type(error).__name__, "reason": str(error), "at": S.utc_now()}
        self.disable("observation_failed")

    def observe(self):
        if not self.record["enabled"] or self.record["requested"]:
            return
        try:
            try:
                with self.checkpoint.open("rb") as stream:
                    header = stream.read(50)
            except FileNotFoundError:
                return
            require(len(header) == 50 and header[:8] == b"SLVRCKPT"
                    and int.from_bytes(header[8:10], "little") == 1, "invalid published checkpoint header")
            iteration = int.from_bytes(header[42:50], "little")
            require(0 < iteration < self.target, "checkpoint already final or empty; interruption contrast not established")
            self.record.update(requested=True, observed_iteration=iteration,
                               header_hex=header.hex(), requested_at=S.utc_now())
            self.disable("signal_requested")
            self.send()
        except BaseException as error:
            self.failed(error)
            raise


def observed_backend(base, trigger):
    class Observed(base):
        def sample(self):
            try:
                result = super().sample()
                trigger.observe()
                return result
            except BaseException as error:
                trigger.failed(error)
                raise

        def kill(self):
            # Supervisor cleanup samples again, after its signal handlers may be restored.
            trigger.disable("process_cleanup")
            return super().kill()
    return Observed


def stage_limits(plan, timeout):
    return {"timeout_seconds": timeout, **{key: plan["limits"][key] for key in
            ("grace_seconds", "kill_wait_seconds", "poll_seconds", "min_free_memory_bytes", "disk_reserve_bytes")},
            "memory_limit_bytes": plan["limits"]["memory_bytes"]}


def check_record(record, interrupted, expected_limits=None):
    require(record["schema"] == "solvers.supervised-run/v1" and record["errors"] == []
            and record["shell"] is False and record["forced"] is False
            and record["cleanup_complete"] is True and record["identity_unchanged"] is True
            and record["identity_before"] == record["identity_after"], "supervisor integrity/cleanup failed")
    expected = ("interrupted", "signal:SIGINT", 130, 130) if interrupted else ("completed", "completed", 0, 0)
    actual = tuple(record[key] for key in ("state", "stop_reason", "supervisor_exit_code", "child_exit_code"))
    require(actual == expected, "unexpected supervised completion: " + repr(actual))
    if interrupted:
        graceful = [event for event in record["events"] if event["kind"] == "graceful"]
        require(record["stop_requested_at"] is not None and len(graceful) == 1
                and graceful[0]["delivered"] is True
                and graceful[0]["method"] in ("SIGINT_process_group", "CTRL_BREAK_dedicated_console"),
                "one real delivered child signal required")
    else:
        require(record["stop_requested_at"] is None, "ordinary stage requested a stop")
    require(record["last_sample"]["pids"] == [] and record["last_sample"]["tree_resident_bytes"] == 0,
            "final containment not empty")
    for ref in refs_in(record):
        verify(ref)
    limits = record["limits"]
    require(set(limits) == {"timeout_seconds", "grace_seconds", "kill_wait_seconds", "poll_seconds",
            "memory_limit_bytes", "min_free_memory_bytes", "disk_reserve_bytes"}
            and all(number(v) and v > 0 for v in limits.values()), "missing/nonfinite stage limits")
    if expected_limits is not None:
        require(limits == expected_limits, "actual stage limits differ")
    raw = Path(record["outputs"]["samples"]["path"]).read_bytes()
    require(raw.endswith(b"\n"), "truncated samples")
    samples = [decode_json(line) for line in raw.splitlines()]
    measurement = record["measurement"]
    require(samples and measurement["sample_count"] == len(samples)
            and samples[-1] == record["last_sample"], "raw sample count/last sample differs")
    previous = 0.0
    for sample in samples:
        elapsed = sample["elapsed_seconds"]
        require(number(elapsed) and elapsed >= previous, "invalid sample clock")
        previous = elapsed
        require(sample["pids"] == sorted(set(sample["pids"]))
                and all(type(pid) is int and pid > 0 for pid in sample["pids"]), "invalid sampled process IDs")
        for key in ("tree_resident_bytes", "host_available_memory_bytes", "disk_free_bytes"):
            require(type(sample[key]) is int and sample[key] >= 0, "invalid sampled resource")
        require(sample["tree_resident_bytes"] <= limits["memory_limit_bytes"]
                and sample["host_available_memory_bytes"] >= limits["min_free_memory_bytes"]
                and sample["disk_free_bytes"] >= limits["disk_reserve_bytes"],
                "resource violation, including during intentional SIGINT")
        for key in ("root_os_peak_resident_bytes", "job_os_peak_commit_bytes"):
            value = sample[key]
            require(value is None or (type(value) is int and value >= 0), "invalid OS peak")
        if sample["root_os_peak_resident_bytes"] is not None:
            require(sample["root_os_peak_resident_bytes"] <= limits["memory_limit_bytes"], "root RSS exceeded memory limit")
    require(measurement["sampled_peak_tree_resident_bytes"] == max(s["tree_resident_bytes"] for s in samples)
            and measurement["max_observed_processes"] == max(len(s["pids"]) for s in samples), "resource aggregate differs")
    gaps = [b["elapsed_seconds"] - a["elapsed_seconds"] for a, b in zip(samples, samples[1:])]
    # Both are differences of the same monotonic readings; no performance tolerance is applied.
    require(measurement["max_sample_gap_seconds"] == max(gaps, default=0.0), "sample gap aggregate differs")
    for key in ("root_os_peak_resident_bytes", "root_os_peak_source", "job_os_peak_commit_bytes"):
        present = [s[key] for s in samples if s[key] is not None]
        require(measurement[key] == (present[-1] if present else None), "OS peak aggregate differs")
    require(record["host_before"]["available_bytes"] >= limits["min_free_memory_bytes"]
            and record["disk_free_before_bytes"] >= limits["disk_reserve_bytes"], "preflight reserve violated")
    elapsed = record["elapsed_seconds"]
    require(number(elapsed) and elapsed >= previous, "invalid stage elapsed time")
    stops = [e for e in record["events"] if e["kind"] == "stop_requested"]
    if interrupted:
        require(len(stops) == 1 and stops[0]["reason"] == "signal:SIGINT", "unexpected stop requests")
        stopped = stops[0]["elapsed_seconds"]
        require(number(stopped) and 0 <= stopped < limits["timeout_seconds"]
                and elapsed <= stopped + limits["grace_seconds"], "SIGINT masked a time limit")
    else:
        require(not stops and elapsed <= limits["timeout_seconds"], "ordinary stage exceeded time limit")


def supervised(plan, directory, argv, timeout, trigger=None):
    limits = plan["limits"]
    base_name = "WindowsProcess" if os.name == "nt" else "LinuxProcess"
    base = getattr(S, base_name)
    if trigger is not None:
        setattr(S, base_name, observed_backend(base, trigger))
    args = ["--record", str(directory / "supervisor.json"), "--cwd", plan["source_root"],
            "--stdout", str(directory / "stdout.log"), "--stderr", str(directory / "stderr.log"),
            "--samples", str(directory / "samples.jsonl"), "--disk-path", plan["out"],
            "--timeout-seconds", str(timeout), "--grace-seconds", str(limits["grace_seconds"]),
            "--kill-wait-seconds", str(limits["kill_wait_seconds"]), "--poll-seconds", str(limits["poll_seconds"]),
            "--memory-limit-bytes", str(limits["memory_bytes"]),
            "--min-free-memory-bytes", str(limits["min_free_memory_bytes"]),
            "--disk-reserve-bytes", str(limits["disk_reserve_bytes"])]
    for ref in plan["frozen_files"]:
        args += ["--identity-file", ref["path"]]
    try:
        code = S.main([*args, "--", *argv])
    finally:
        setattr(S, base_name, base)
        if trigger is not None:
            S.atomic_json(directory / "trigger.json", trigger.record, initial=True)
    record = read(directory / "supervisor.json")
    check_record(record, trigger is not None, stage_limits(plan, timeout))
    require(code == (130 if trigger else 0), "supervisor return code differs")
    require(record["containment"] == base.containment, "actual process containment differs")
    require(record["argv"] == record["resolved_argv"] == argv and record["cwd"] == plan["source_root"],
            "actual command differs")
    require(all(ref in record["identity_before"] for ref in plan["frozen_files"]), "missing frozen stage identity")
    if trigger:
        require(trigger.record["requested"] and trigger.record["first_failure"] is None, "signal observation failed")
    return record


COMPARISON_CHECKS = {
    "input_identities_unchanged", "config_bytes_exact", "nonfinal_canceled_checkpoint",
    "resumed_progress_prefix_exact", "all_progress_numeric_fields_equal",
    "final_checkpoint_raw_bytes_equal", "final_checkpoint_full_decoded_state_equal",
    "final_full_sol_except_wall_equal",
}
RUN_FILES = {"run.toml", "manifest.json", "events.jsonl", "progress.jsonl", "run.json",
             "checkpoint.ckpt", "solution.sol"}


def bind_comparison(report, runs, artifacts, trigger):
    require(report["schema"] == "r1.hu-checkpoint-audit/v1" and report["status"] == "pass",
            "comparator did not pass")
    m, n = report["interrupted_iteration"], report["target_iteration"]
    require(type(m) is int and type(n) is int and 0 < m < n == 1000 and m % 10 == 0
            and report["resumed_iterations"] == n - m and report["source_storage"] == "f32",
            "invalid interrupted/final iteration contrast")
    require(set(report["checks"]) == COMPARISON_CHECKS and all(v is True for v in report["checks"].values()),
            "missing/failed comparator checks")
    config_hash = report["config_blake3"]
    require(isinstance(config_hash, str) and re.fullmatch("[0-9a-f]{64}", config_hash), "invalid config hash")
    require(set(report["runs"]) == set(runs), "comparator run roles differ")
    sol_refs = {}
    for role, directory in runs.items():
        item = report["runs"][role]
        require(path_key(item["directory"]) == path_key(directory), "comparator directory differs")
        refs = item["files"]
        expected = {path_key(directory / name): name for name in RUN_FILES}
        require(len(refs) == len(expected) and {path_key(ref["path"]) for ref in refs} == set(expected),
                "comparator file set differs")
        for ref in refs:
            require(set(ref) == {"path", "bytes", "blake3"} and re.fullmatch("[0-9a-f]{64}", ref["blake3"]),
                    "invalid comparator identity")
            name = expected[path_key(ref["path"])]
            pin = artifacts[role][name]
            verify(pin)
            require(ref["bytes"] == pin["bytes"] and path_key(ref["path"]) == path_key(pin["path"]),
                    "comparator file differs from frozen actual artifact")
            if name == "solution.sol":
                sol_refs[role] = ref
        for name, magic, version in (("checkpoint.ckpt", b"SLVRCKPT", 1), ("solution.sol", b"SLVRSOLV", 3)):
            with (directory / name).open("rb") as stream:
                header = stream.read(50)
            require(len(header) == 50 and header[:8] == magic and int.from_bytes(header[8:10], "little") == version
                    and header[10:42].hex() == config_hash
                    and int.from_bytes(header[42:50], "little") == (m if role == "interrupted" else n),
                    "comparator metadata does not match actual artifact header")
    header = bytes.fromhex(trigger["header_hex"])
    require(trigger["requested"] is True and trigger["first_failure"] is None and len(header) == 50
            and header[10:42].hex() == config_hash
            and int.from_bytes(header[42:50], "little") == trigger["observed_iteration"]
            and 0 < trigger["observed_iteration"] <= m, "signal observation is not bound to interrupted state")
    return sol_refs


def audit_values(report, sol_ref, config_hash):
    require(report["schema"] == "solvers.research.hu-saved-profile-audit/v1" and report["threads"] == 1,
            "wrong saved-profile audit")
    artifact = report["artifact"]
    require(artifact["mode"] == "full" and artifact["iterations"] == 1000
            and artifact["format_version"] == 3 and artifact["source_storage"] == "f32"
            and artifact["config_blake3"] == config_hash,
            "saved-profile audit not final Full")
    require(path_key(artifact["path"]) == path_key(sol_ref["path"])
            and artifact["bytes"] == sol_ref["bytes"] and artifact["blake3"] == sol_ref["blake3"],
            "saved audit not bound to comparator's unchanged actual SOL")
    require(report["recomputed"]["profile"] == "stored_quantized", "wrong evaluated profile")
    for key in ("ev", "br", "gains"):
        require(len(report["recomputed"][key]) == 2 and all(number(x) for x in report["recomputed"][key]),
                "nonfinite/missing saved values")
    require(number(report["recomputed"]["nash_conv"]), "nonfinite saved NC")
    result = copy.deepcopy(report)
    for key in ("input_hash_secs", "load_secs", "eval_secs"):
        require(number(result[key]) and result[key] >= 0, "invalid excluded audit timing")
        result.pop(key)
    for key in ("path", "blake3", "bytes"):
        result["artifact"].pop(key)
    wall = result["pre_save_metadata"].pop("wall_secs")
    require(number(wall) and wall >= 0, "invalid excluded SOL wall time")
    return result


def freeze(args):
    require(sys.platform == "win32", "this campaign requires Windows Job containment; Linux outer containment is not implemented")
    out = args.out.resolve()
    require(not out.exists() and out.parent.is_dir(), "new output under an existing parent required")
    protocol = read(HERE / "protocol.json")
    config = tomllib.loads((HERE / "river.toml").read_text(encoding="utf-8"))
    require(config["schema"] == "solvers.postflop/v1" and config["run"] == {
        "iterations": 1000, "check_every": 10, "threads": 1, "storage": "f32"}, "fixed run target changed")
    build_plan_ref, build_record_ref = S.identity(args.build_plan), S.identity(args.build_record)
    build_plan, build = read(args.build_plan), read(args.build_record)
    require(build_plan["schema"] == "r1.hu-checkpoint-build-plan/v1", "wrong build plan")
    check_record(build, False)
    require(all(build["limits"][key if key != "memory_bytes" else "memory_limit_bytes"] == value
                for key, value in build_plan["limits"].items()), "build limits differ from its plan")
    require(build["argv"] == build["resolved_argv"] == build_plan["build"]["argv"]
            and build["cwd"] == build_plan["source_root"] and build_plan_ref in build["identity_before"],
            "build command/source plan binding differs")
    source_root = Path(build_plan["source_root"])
    require(source_root.is_absolute() and source_root == ROOT, "execute with this source checkout")
    refs = [build_plan_ref, build_record_ref, S.identity(Path(__file__)), S.identity(Path(S.__file__)),
            S.identity(Path(sys.executable)), S.identity(HERE / "protocol.json"), S.identity(HERE / "river.toml")]
    for name, pin in build_plan["source_files"].items():
        path = source_root / name
        require(path.resolve().is_relative_to(source_root), "source path escapes checkout")
        ref = S.identity(path)
        require(all(ref[key] == pin[key] for key in ("bytes", "sha256")), "source changed after build plan: " + name)
        refs.append(ref)
    binaries = {key: S.identity(Path(path)) for key, path in build_plan["build"]["binaries"].items()}
    require(set(binaries) == {"solvers", "comparator", "saved_profile"}, "three actual executables required")
    refs += list(binaries.values()) + list(refs_in(build_plan)) + list(refs_in(build))
    unique = {}
    for ref in refs:
        verify(ref)
        require(ref["path"] not in unique or unique[ref["path"]] == ref, "conflicting frozen identity")
        unique[ref["path"]] = ref
    require(0 < args.memory_bytes <= 2 * 1024**3 and args.min_free_memory_bytes >= 2 * 1024**3
            and args.disk_reserve_bytes >= 4 * 1024**3, "finite conservative resource limits required")
    limits = {**protocol["limits"], "memory_bytes": args.memory_bytes,
              "min_free_memory_bytes": args.min_free_memory_bytes, "disk_reserve_bytes": args.disk_reserve_bytes}
    require(protocol["target_iteration"] == 1000 and protocol["check_every"] == 10
            and protocol["order"] == ["interrupted", "resumed", "straight", "compare", "audit-resumed", "audit-straight"],
            "fixed schedule changed")
    require(protocol["limits"] == {"campaign_seconds": 600, "solve_seconds": 60, "audit_seconds": 30,
            "grace_seconds": 15, "kill_wait_seconds": 5, "poll_seconds": 0.02}, "protocol limits changed")
    require(not any(Path(ref["path"]).is_relative_to(out) for ref in refs), "output overlaps frozen inputs")
    return {"schema": "r1.hu-checkpoint-plan/v1", "created_at": S.utc_now(), "out": str(out),
            "source_root": str(source_root), "runtime": {"python": sys.version, "platform": platform.platform()},
            "limits": limits, "protocol": protocol, "binaries": binaries,
            "frozen_files": [unique[key] for key in sorted(unique)], "r1_acceptance": None}


def execute(plan, invoke=supervised, clock=time.monotonic):
    out = Path(plan["out"])
    out.mkdir(exist_ok=False)
    S.atomic_json(out / "plan.json", plan, initial=True)
    plan["frozen_files"].append(S.identity(out / "plan.json"))
    result = {"schema": "r1.hu-checkpoint-campaign/v1", "status": "running", "started_at": S.utc_now(),
              "plan": S.identity(out / "plan.json"), "stages": [], "first_failure": None,
              "performance": "not_evaluated", "external_quality": "not_evaluated", "r1_acceptance": None}
    S.atomic_json(out / "result.json", result, initial=True)
    deadline = clock() + plan["limits"]["campaign_seconds"]
    current = None
    artifacts = {}
    trigger_record = None
    try:
        runs = {key: out / key / "run" for key in ("interrupted", "resumed", "straight")}
        binary = {key: ref["path"] for key, ref in plan["binaries"].items()}
        commands = [
            ("interrupted", [binary["solvers"], "solve", str(HERE / "river.toml"), "--out", str(runs["interrupted"]), "--sol-streets", "full"]),
            ("resumed", [binary["solvers"], "resume", str(runs["interrupted"]), "--out", str(runs["resumed"])]),
            ("straight", [binary["solvers"], "solve", str(HERE / "river.toml"), "--out", str(runs["straight"]), "--sol-streets", "full"]),
            ("compare", [binary["comparator"], "--interrupted", str(runs["interrupted"]), "--resumed", str(runs["resumed"]), "--straight", str(runs["straight"])]),
            ("audit-resumed", [binary["saved_profile"], "--sol", str(runs["resumed"] / "solution.sol"), "--threads", "1"]),
            ("audit-straight", [binary["saved_profile"], "--sol", str(runs["straight"] / "solution.sol"), "--threads", "1"]),
        ]
        for label, argv in commands:
            timeout = plan["limits"]["solve_seconds" if label in runs else "audit_seconds"]
            require(clock() + timeout + 21 < deadline, "insufficient campaign time for stage and cleanup")
            for ref in plan["frozen_files"]:
                verify(ref)
            for role, pins in artifacts.items():
                require(tree_identity(runs[role]) == pins, "completed run artifact set changed: " + role)
            current = out / label
            current.mkdir()
            trigger = CheckpointTrigger(runs["interrupted"] / "checkpoint.ckpt", 1000) if label == "interrupted" else None
            require(clock() + timeout + 21 < deadline, "insufficient campaign time after input verification")
            record = invoke(plan, current, argv, timeout, trigger)
            require(clock() < deadline, "campaign deadline exceeded")
            for ref in plan["frozen_files"]:
                verify(ref)
            result["stages"].append({"label": label, "record": S.identity(current / "supervisor.json")})
            for role, pins in artifacts.items():
                require(tree_identity(runs[role]) == pins, "later stage changed completed run: " + role)
            if label in runs:
                artifacts[label] = tree_identity(runs[label])
                plan["frozen_files"].extend(artifacts[label].values())
                result["run_artifacts"] = copy.deepcopy(artifacts)
            if trigger is not None:
                trigger_record = copy.deepcopy(trigger.record)
            # Completed evidence becomes an immutable input of every later stage too.
            plan["frozen_files"].append(S.identity(current / "supervisor.json"))
            plan["frozen_files"].extend(record["outputs"].values())
            if trigger is not None:
                plan["frozen_files"].append(S.identity(current / "trigger.json"))
            S.atomic_json(out / "result.json", result)
        comparison = read(out / "compare/stdout.log")
        sol_refs = bind_comparison(comparison, runs, artifacts, trigger_record)
        saved = {label: read(out / ("audit-" + label) / "stdout.log") for label in ("resumed", "straight")}
        values = {role: audit_values(saved[role], sol_refs[role], comparison["config_blake3"]) for role in saved}
        require(same_values(values["resumed"], values["straight"]), "saved-profile evaluations differ")
        for ref in plan["frozen_files"]:
            verify(ref)
        for role, pins in artifacts.items():
            require(tree_identity(runs[role]) == pins, "final artifact set differs: " + role)
        require(clock() < deadline, "campaign deadline exceeded during final verification")
        result.update(status="verified", comparison=comparison,
                      saved_profile_values=saved["resumed"]["recomputed"],
                      interrupted_artifacts_after=tree_identity(runs["interrupted"]))
    except BaseException as error:
        result.update(status="failed", first_failure={"type": type(error).__name__, "reason": str(error),
                      "stage": str(current) if current else None})
    finally:
        result["ended_at"] = S.utc_now()
        S.atomic_json(out / "result.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("build-plan", "build-record", "out"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--memory-bytes", type=int, required=True)
    parser.add_argument("--min-free-memory-bytes", type=int, required=True)
    parser.add_argument("--disk-reserve-bytes", type=int, default=4 * 1024**3)
    args = parser.parse_args()
    result = execute(freeze(args))
    print(json.dumps({"status": result["status"], "stages": len(result["stages"]), "first_failure": result["first_failure"]}))
    return 0 if result["status"] == "verified" else 1


if __name__ == "__main__":
    raise SystemExit(main())
