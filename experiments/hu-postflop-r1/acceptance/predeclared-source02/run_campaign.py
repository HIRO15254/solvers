#!/usr/bin/env python3
"""Baseline-only pilot, frozen paired runlist, and conservative R1 IO comparison.

Python 3.11+ standard library. Executes only local binaries through the repository
supervisor; cloud lifecycle and a Linux cgroup/systemd boundary are the caller's
responsibility. Never runs a build, downloads files, or retries failed commands.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import sys
import tempfile
import time
import tomllib

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
SUPERVISOR = REPO / "tools" / "run_supervised.py"
SCHEMA = "solvers.r1-pipeline/v1"
CASES = ("river", "turn", "flop")
SUMMARY_FIELDS = ("board", "pot", "effective_stack", "min_bet", "iterations",
                  "ev_oop", "ev_ip", "expl_oop", "expl_ip", "nash_conv",
                  "storage", "streets_stored", "nodes", "stored_nodes")


def identity(path: Path) -> dict:
    path = path.resolve(strict=True)
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return {"path": str(path), "sha256": digest.hexdigest(), "bytes": path.stat().st_size}


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def artifact_header(path: Path):
    with path.open("rb") as stream:
        header = stream.read(50)
    if len(header) != 50 or header[:8] not in (b"SLVRSOLV", b"SLVRCKPT"):
        raise ValueError(f"invalid artifact header: {path}")
    return {"version": int.from_bytes(header[8:10], "little"),
            "embedded_config_blake3": header[10:42].hex(),
            "iteration": int.from_bytes(header[42:50], "little")}


def write_json(path: Path, value, *, exclusive=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=".pipeline-", delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(value, stream, indent=2, sort_keys=True, allow_nan=False)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        if exclusive:
            os.link(temporary, path)
        else:
            os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def utc_now():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def host_identity():
    result = {"hostname": platform.node(), "platform": platform.platform(),
              "machine": platform.machine(), "logical_cpus": os.cpu_count(),
              "python": sys.version, "rayon_num_threads": os.environ.get("RAYON_NUM_THREADS")}
    for key, path in (("boot_id", "/proc/sys/kernel/random/boot_id"),
                      ("cpuinfo", "/proc/cpuinfo")):
        if Path(path).is_file():
            result[key] = Path(path).read_text(encoding="utf-8").strip()
    return result


def limits(args):
    if not math.isfinite(args.timeout_seconds) or args.timeout_seconds <= 0:
        raise ValueError("timeout must be finite and positive")
    if min(args.memory_limit_bytes, args.min_free_memory_bytes, args.disk_reserve_bytes) < 0:
        raise ValueError("resource limits must be nonnegative")
    return {"timeout_seconds": args.timeout_seconds, "grace_seconds": 5,
            "kill_wait_seconds": 5, "poll_seconds": 0.05,
            "memory_limit_bytes": args.memory_limit_bytes,
            "min_free_memory_bytes": args.min_free_memory_bytes,
            "disk_reserve_bytes": args.disk_reserve_bytes}


def supervisor_module():
    spec = importlib.util.spec_from_file_location("r1_supervisor", SUPERVISOR)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def supervise(binary: Path, argv: list[str], directory: Path, bounds: dict,
              identities: list[Path]) -> dict:
    deadline = bounds.get("_campaign_deadline")
    required = bounds["timeout_seconds"] + bounds["grace_seconds"] + bounds["kill_wait_seconds"]
    if deadline is not None and time.monotonic() + required >= deadline:
        raise RuntimeError("campaign deadline: insufficient time for another complete supervised stage")
    directory.mkdir(parents=True, exist_ok=False)
    record = directory / "supervisor.json"
    command = ["--record", str(record), "--cwd", str(REPO),
               "--stdout", str(directory / "stdout.log"),
               "--stderr", str(directory / "stderr.log"),
               "--disk-path", str(directory)]
    for key, value in bounds.items():
        if not key.startswith("_"):
            command += ["--" + key.replace("_", "-"), str(value)]
    for path in [Path(__file__).resolve(), *identities]:
        command += ["--identity-file", str(path)]
    command += ["--", str(binary), *argv]
    print(f"{utc_now()} {directory.name}: {' '.join(argv)}", flush=True)
    code = supervisor_module().main(command)
    if not record.is_file():
        raise RuntimeError(f"supervisor produced no record: exit={code}, directory={directory}")
    result = read_json(record)
    if result["cleanup_complete"] is not True:
        raise RuntimeError(f"cleanup not verified; refusing another process: {record}")
    if result["state"] in {"interrupted", "supervisor_error"}:
        raise RuntimeError(f"campaign stopped after {result['state']}: {record}")
    return {"record": identity(record), "exit_code": code, "state": result["state"],
            "stop_reason": result["stop_reason"], "elapsed_seconds": result.get("elapsed_seconds"),
            "measurement": result["measurement"], "cleanup_complete": result["cleanup_complete"],
            "identity_unchanged": result["identity_unchanged"],
            "stdout": result["outputs"]["stdout"]}


def completed(stage):
    return (stage["state"] == "completed" and stage["exit_code"] == 0
            and stage["cleanup_complete"] is True and stage["identity_unchanged"] is True)


def run_status(stage):
    if completed(stage):
        return "completed"
    return {"timeout": "timeout", "resource_exceeded": "resource_exceeded",
            "interrupted": "canceled"}.get(stage["state"], "computation_failed")


def finite(value):
    return isinstance(value, (float, int)) and not isinstance(value, bool) and math.isfinite(value)


def quality(live_final, target):
    if (live_final is None or not finite(live_final.get("nash_conv"))
            or not all(finite(value) for value in live_final["g_i"])):
        return {"status": "not_evaluated", "reason": "final BR summary unavailable"}
    return {"status": "pass" if live_final["nash_conv"] < target else "fail",
            "nash_conv": live_final["nash_conv"], "g_i": live_final["g_i"],
            "target_nash_conv": target,
            "inequality": "<", "strategy_kind": "live_average",
            "source": "run.json final live solver result",
            "exploitability_pct_pot": 50 * live_final["nash_conv"] / live_final["pot"],
            "scope": "internal finite synthetic game only"}


def perform_solve(binary, config, directory, bounds, identities):
    directory.mkdir(parents=True, exist_ok=False)
    stage = supervise(binary, ["solve", str(config), "--out", str(directory / "run"),
                                "--sol-streets", "full"], directory / "solve", bounds,
                      [config, *identities])
    result = {"run_status": run_status(stage), "solve": stage,
              "summary_read": None, "summary": None, "live_final": None,
              "presave_consistency": "not_evaluated", "artifacts": {},
              "quality_status": "not_evaluated", "saved_profile_br": "not_evaluated",
              "phase_timings": {"initialization": None, "cfr": None, "br_final": None,
                                "return": None, "persistence": None},
              "missing_fields": ["saved_quantized_profile_br", "monotonic_internal_phase_spans",
                                 "external_reference_condition_match_and_thresholds"]}
    for name in ("solution.sol", "checkpoint.ckpt", "run.toml", "run.json", "progress.jsonl"):
        path = directory / "run" / name
        if path.is_file():
            result["artifacts"][name] = identity(path)
    write_json(directory / "result.json", result, exclusive=True)
    if completed(stage):
        for name in ("solution.sol", "checkpoint.ckpt"):
            result["artifacts"][name]["header"] = artifact_header(directory / "run" / name)
        live = read_json(directory / "run" / "run.json")
        document = tomllib.loads(config.read_text(encoding="utf-8"))
        result["live_final"] = {"nash_conv": live["nashConv"], "iterations": live["iterations"],
                                "g_i": [live["explP0"], live["explP1"]],
                                "pot": document["game"]["pot"]}
        write_json(directory / "result.json", result)
        artifact = directory / "run" / "solution.sol"
        exported = supervise(binary, ["export", str(artifact), "summary"],
                             directory / "summary", {**bounds, "poll_seconds": 0.001},
                             [artifact, config, *identities])
        result["summary_read"] = exported
        if completed(exported):
            result["summary"] = read_json(Path(exported["stdout"]["path"]))
            summary = result["summary"]
            consistent = (summary["nash_conv"] == live["nashConv"]
                          and summary["iterations"] == live["iterations"]
                          and [summary["expl_oop"], summary["expl_ip"]] == result["live_final"]["g_i"])
            result["presave_consistency"] = "pass" if consistent else "fail"
    write_json(directory / "result.json", result)
    return result


def pilot(args):
    started = time.monotonic()
    campaign_seconds = getattr(args, "campaign_seconds", 1800.0)
    if not math.isfinite(campaign_seconds) or campaign_seconds <= 0:
        raise ValueError("campaign-seconds must be finite and positive")
    if len(set(args.cases)) != len(args.cases):
        raise ValueError("each case may be selected only once")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    binary = args.baseline.resolve(strict=True)
    source = args.baseline_source.resolve(strict=True)
    report = {"schema": SCHEMA, "kind": "pilot", "created_at": utc_now(),
              "baseline": {"binary": identity(binary), "source_evidence": identity(source),
                           "revision": "9632d8b", "sol_version": 1},
              "runner": identity(Path(__file__)), "supervisor": identity(SUPERVISOR),
              "host": host_identity(), "limits": limits(args), "cases": [],
              "state": "running", "campaign_seconds": campaign_seconds}
    write_json(out / "pilot.json", report, exclusive=True)
    bounds = {**report["limits"], "_campaign_deadline": started + campaign_seconds}
    try:
        for case in args.cases:
            config = HERE / "configs" / f"{case}.toml"
            document = tomllib.loads(config.read_text(encoding="utf-8"))
            result = perform_solve(binary, config, out / case, bounds, [source])
            result.update({"case": case, "config": identity(config),
                           "target_nash_conv": document["run"]["target_nash_conv"]})
            result["internal_target"] = quality(result["live_final"], result["target_nash_conv"])
            if (result["run_status"] == "completed"
                    and result["artifacts"]["solution.sol"]["header"]["version"] != 1):
                raise ValueError("baseline pilot must produce .sol v1")
            report["cases"].append(result)
            write_json(out / case / "result.json", result)
            write_json(out / "pilot.json", report)
        report["state"] = "completed"
    except BaseException as error:
        report["state"] = "interrupted"
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        report["ended_at"] = utc_now()
        write_json(out / "pilot.json", report)
    return 0 if all(row["run_status"] == "completed" for row in report["cases"]) else 1


def verify_identity(expected):
    actual = identity(Path(expected["path"]))
    if actual != expected:
        raise ValueError(f"identity changed: {expected['path']}")


def freeze(args):
    campaign_seconds = getattr(args, "campaign_seconds", 3600.0)
    if not math.isfinite(campaign_seconds) or campaign_seconds <= 0:
        raise ValueError("campaign-seconds must be finite and positive")
    pilot_path = args.pilot.resolve(strict=True)
    report = read_json(pilot_path)
    if report.get("kind") != "pilot":
        raise ValueError("--pilot must name a pilot.json record")
    for item in (report["baseline"]["binary"], report["baseline"]["source_evidence"]):
        verify_identity(item)
    chosen = args.cases or [row["case"] for row in report["cases"]
                            if row["internal_target"]["status"] == "pass"
                            and row["presave_consistency"] == "pass"]
    if not chosen:
        raise ValueError("no completed baseline pilot reached its preset quality target")
    if len(set(chosen)) != len(chosen):
        raise ValueError("each case may be selected only once")
    destination = args.out.resolve()
    destination.mkdir(parents=True, exist_ok=False)
    cases = []
    for case in chosen:
        row = next((row for row in report["cases"] if row["case"] == case), None)
        if (row is None or row["run_status"] != "completed"
                or row["internal_target"]["status"] != "pass" or row["presave_consistency"] != "pass"):
            raise ValueError(f"{case}: pilot did not complete and reach the preset target")
        verify_identity(row["config"])
        raw = Path(row["config"]["path"]).read_text(encoding="utf-8")
        iterations = row["summary"]["iterations"]
        # Only the iteration cap is pilot-calibrated. The quality target, check
        # cadence, finite game, storage, and thread count remain unchanged.
        raw, count = re.subn(r"(?m)^iterations = \d+$", f"iterations = {iterations}", raw)
        if count != 1:
            raise ValueError("expected exactly one iteration cap")
        config = destination / f"{case}.toml"
        config.write_text(raw, encoding="utf-8", newline="\n")
        cases.append({"case": case, "config": identity(config), "iterations": iterations,
                      "target_nash_conv": row["target_nash_conv"]})
    plan = {"schema": SCHEMA, "kind": "frozen_plan", "created_at": utc_now(),
            "pilot": identity(pilot_path), "baseline": report["baseline"],
            "candidate": {"binary": identity(args.candidate),
                          "source_evidence": identity(args.candidate_source), "sol_version": 2},
            "runner": identity(Path(__file__)), "supervisor": identity(SUPERVISOR),
            "host": report["host"], "limits": report["limits"], "cases": cases,
            "repetitions_per_binary": 3, "order": ["baseline", "candidate"] * 3,
            "campaign_seconds": campaign_seconds,
            "summary_poll_seconds": 0.001,
            "profile_equality": "exact JSON export bytes, all stored nodes with positive own reach",
            "summary_equality_fields": SUMMARY_FIELDS,
            "quality_scope": "internal synthetic game; not R0 external reference certification",
            "resume_scope": "restore completed iteration cap; no additional CFR iterations",
            "cache_policy": "warm/uncontrolled OS caches; no cache eviction or cold-start claim"}
    write_json(destination / "plan.json", plan, exclusive=True)
    return 0


def export_profile(binary, run_directory, directory, bounds, identities):
    artifact = run_directory / "solution.sol"
    result = {}
    for view in ("tree", "strategy", "ev"):
        stage = supervise(binary, ["export", str(artifact), view, "--node", "all"],
                          directory / view, bounds, [artifact, *identities])
        result[view] = stage
        write_json(directory / "profile.json", result)
        if not completed(stage):
            break
    return result


def equal_summary(left, right):
    return (left is not None and right is not None
            and all(key in left and key in right and left[key] == right[key]
                    for key in SUMMARY_FIELDS))


def equal_profile(left, right):
    return all(key in left and key in right and completed(left[key]) and completed(right[key])
               and left[key]["stdout"]["sha256"] == right[key]["stdout"]["sha256"]
               for key in ("tree", "strategy", "ev"))


def resume_check(binary, original, result, directory, bounds, identities):
    # A completed, fixed iteration cap must load and republish the exact same
    # profile. This is intentionally not an interrupted-run continuation test.
    stage = supervise(binary, ["resume", str(original), "--out", str(directory / "run")],
                      directory / "resume", bounds, identities)
    answer = {"scope": "completed-cap restore, zero additional CFR iterations",
              "stage": stage, "status": "not_evaluated"}
    if not completed(stage):
        return answer
    artifact = directory / "run" / "solution.sol"
    summary = supervise(binary, ["export", str(artifact), "summary"],
                        directory / "summary", {**bounds, "poll_seconds": 0.001},
                        [artifact, *identities])
    answer["summary_read"] = summary
    if not completed(summary):
        return answer
    answer["summary"] = read_json(Path(summary["stdout"]["path"]))
    answer["profile"] = export_profile(binary, directory / "run", directory, bounds, identities)
    answer["status"] = "pass" if (equal_summary(result["summary"], answer["summary"])
                                     and equal_profile(result["profile"], answer["profile"])) else "fail"
    return answer


def check_plan(plan):
    if plan.get("kind") != "frozen_plan" or plan.get("schema") != SCHEMA:
        raise ValueError("unsupported plan")
    if plan.get("order") != ["baseline", "candidate"] * 3:
        raise ValueError("the frozen order must be three alternating baseline/candidate pairs")
    for version in ("baseline", "candidate"):
        verify_identity(plan[version]["binary"])
        verify_identity(plan[version]["source_evidence"])
    verify_identity(plan["runner"])
    verify_identity(plan["supervisor"])
    for case in plan["cases"]:
        verify_identity(case["config"])
        document = tomllib.loads(Path(case["config"]["path"]).read_text(encoding="utf-8"))
        if (document["run"]["threads"] != 8 or document["run"]["iterations"] != case["iterations"]
                or document["run"]["target_nash_conv"] != case["target_nash_conv"]):
            raise ValueError("frozen config disagrees with plan")
    current = host_identity()
    for key in ("hostname", "boot_id", "machine", "logical_cpus"):
        if key in plan["host"] and current.get(key) != plan["host"][key]:
            raise ValueError(f"pilot and comparison host disagree: {key}")


def comparison(args):
    started = time.monotonic()
    plan_path = args.plan.resolve(strict=True)
    plan = read_json(plan_path)
    check_plan(plan)
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    report = {"schema": SCHEMA, "kind": "comparison", "created_at": utc_now(),
              "plan": identity(plan_path), "host": host_identity(), "runs": [],
              "state": "running", "quality_status": "not_evaluated"}
    write_json(out / "comparison.json", report, exclusive=True)
    bounds = {**plan["limits"], "_campaign_deadline": started + plan["campaign_seconds"]}
    try:
        for case in plan["cases"]:
            for index, version in enumerate(plan["order"]):
                check_plan(plan)
                repetition = index // 2 + 1
                directory = out / f"{case['case']}-{repetition}-{version}"
                binary = Path(plan[version]["binary"]["path"])
                config = Path(case["config"]["path"])
                inputs = [plan_path, Path(plan[version]["source_evidence"]["path"])]
                result = perform_solve(binary, config, directory, bounds, inputs)
                result.update({"case": case["case"], "version": version, "repetition": repetition,
                               "expected_iterations": case["iterations"], "profile": {},
                               "expected_sol_version": plan[version]["sol_version"], "resume": None})
                result["internal_target"] = quality(result["live_final"], case["target_nash_conv"])
                report["runs"].append(result)
                write_json(directory / "result.json", result)
                write_json(out / "comparison.json", report)
                # A solve timeout/resource stop stays not_evaluated even if a
                # cooperative stop happened to leave an artifact behind.
                if result["run_status"] == "completed" and result["summary"] is not None:
                    result["profile"] = export_profile(binary, directory / "run", directory,
                                                       bounds, inputs)
                    if repetition == 1 and all(completed(v) for v in result["profile"].values()) \
                            and len(result["profile"]) == 3:
                        result["resume"] = resume_check(binary, directory / "run", result,
                                                         directory / "restore", bounds, inputs)
                write_json(directory / "result.json", result)
                write_json(out / "comparison.json", report)
                # Containment failure must stop the campaign, never start the
                # next experiment next to a possibly surviving solver tree.
                if result["solve"]["cleanup_complete"] is not True:
                    raise RuntimeError("cleanup not verified; refusing another run")
        report["state"] = "completed"
    except BaseException as error:
        report["state"] = "interrupted"
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        report["ended_at"] = utc_now()
        write_json(out / "comparison.json", report)
    analysis = analyze_report(report)
    write_json(out / "analysis.json", analysis, exclusive=True)
    return 0 if all(row["performance_comparison_eligible"] for row in analysis["cases"]) else 1


def describe(values):
    if not values or not all(finite(value) for value in values):
        return None
    return {"n": len(values), "median": statistics.median(values),
            "min": min(values), "max": max(values), "values": values}


def analyze_report(report):
    answer = {"schema": SCHEMA, "kind": "analysis", "quality_status": "not_evaluated",
              "saved_profile_br": "not_evaluated", "cases": [],
              "limits": ["synthetic fixtures, not external R0 reference certification",
                         "export equality covers positive-own-reach rows; zero-reach rows are omitted",
                         "exported EV is presave_snapshot; exported strategy is stored_quantized",
                         "no cold cache claim; process time includes startup and shutdown",
                         "internal phase spans unavailable; summary.wall_secs is not total time",
                         "resume check restores completed cap, not interrupted continuation"]}
    for case in dict.fromkeys(row["case"] for row in report["runs"]):
        rows = [row for row in report["runs"] if row["case"] == case]
        pairs = []
        for repetition in range(1, 4):
            pair = {row["version"]: row for row in rows if row["repetition"] == repetition}
            if set(pair) != {"baseline", "candidate"}:
                pairs.append({"repetition": repetition, "status": "not_evaluated"})
                continue
            left, right = pair["baseline"], pair["candidate"]
            checks = {"both_completed": all(row["run_status"] == "completed" for row in pair.values()),
                      "internal_target": all(row["internal_target"]["status"] == "pass" for row in pair.values()),
                      "presave_consistency": all(row["presave_consistency"] == "pass" for row in pair.values()),
                      "artifact_version": all(row["artifacts"].get("solution.sol", {}).get("header", {}).get("version")
                                              == row["expected_sol_version"] for row in pair.values()),
                      "same_fixed_iteration": all(row["summary"] is not None and
                          row["summary"]["iterations"] == row["expected_iterations"] for row in pair.values()),
                      "same_presave_summary": equal_summary(left["summary"], right["summary"]),
                      "same_saved_exports": equal_profile(left["profile"], right["profile"])}
            if repetition == 1:
                checks["checkpoint_restore"] = all(row["resume"] is not None and
                    row["resume"]["status"] == "pass" for row in pair.values())
            pairs.append({"repetition": repetition, "checks": checks,
                          "status": "pass" if all(checks.values()) else "not_evaluated"})
        eligible = len(rows) == 6 and all(pair["status"] == "pass" for pair in pairs)
        measurements = {}
        for version in ("baseline", "candidate"):
            subset = [row for row in rows if row["version"] == version]
            measurements[version] = {}
            for stage in ("solve", "summary_read"):
                completed_rows = [row[stage] for row in subset if row[stage] is not None and completed(row[stage])]
                measurements[version][stage] = {
                    "elapsed_seconds": describe([row["elapsed_seconds"] for row in completed_rows]),
                    "root_os_peak_resident_bytes": describe([row["measurement"]["root_os_peak_resident_bytes"] for row in completed_rows]),
                    "root_os_peak_sources": sorted({row["measurement"]["root_os_peak_source"] for row in completed_rows
                                                     if row["measurement"]["root_os_peak_source"] is not None}),
                    "sampled_peak_tree_resident_bytes": describe([row["measurement"]["sampled_peak_tree_resident_bytes"] for row in completed_rows])}
            for artifact in ("solution.sol", "checkpoint.ckpt"):
                measurements[version][artifact + "_bytes"] = describe([
                    row["artifacts"][artifact]["bytes"] for row in subset if artifact in row["artifacts"]])
        answer["cases"].append({"case": case, "pairs": pairs, "performance_comparison_eligible": eligible,
                                "measurements": measurements})
    return answer


def analyze(args):
    write_json(args.out, analyze_report(read_json(args.report)), exclusive=True)
    return 0


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    sub = result.add_subparsers(dest="command", required=True)
    p = sub.add_parser("pilot")
    p.add_argument("--baseline", type=Path, required=True)
    p.add_argument("--baseline-source", type=Path, required=True,
                   help="retained source manifest or source archive, hashed without uploading")
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--cases", nargs="+", choices=CASES, default=list(CASES))
    p.add_argument("--timeout-seconds", type=float, default=600)
    p.add_argument("--campaign-seconds", type=float, default=1800,
                   help="refuse new stages that cannot finish within this monotonic campaign window")
    p.add_argument("--memory-limit-bytes", type=int, default=40 * 1024**3)
    p.add_argument("--min-free-memory-bytes", type=int, default=8 * 1024**3)
    p.add_argument("--disk-reserve-bytes", type=int, default=10 * 1024**3)
    p = sub.add_parser("freeze")
    p.add_argument("--pilot", type=Path, required=True)
    p.add_argument("--candidate", type=Path, required=True)
    p.add_argument("--candidate-source", type=Path, required=True)
    p.add_argument("--campaign-seconds", type=float, default=3600,
                   help="comparison campaign window, frozen before candidate measurements")
    p.add_argument("--cases", nargs="+", choices=CASES)
    p.add_argument("--out", type=Path, required=True)
    p = sub.add_parser("run")
    p.add_argument("--plan", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    p = sub.add_parser("analyze")
    p.add_argument("--report", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    return result


def main(argv=None):
    args = parser().parse_args(argv)
    os.environ["RAYON_NUM_THREADS"] = "8"
    try:
        return {"pilot": pilot, "freeze": freeze, "run": comparison,
                "analyze": analyze}[args.command](args)
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        print(f"campaign: {type(error).__name__}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
