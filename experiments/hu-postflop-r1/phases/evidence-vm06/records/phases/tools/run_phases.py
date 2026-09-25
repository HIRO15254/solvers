#!/usr/bin/env python3
"""Reuse the frozen snapshot03 campaign with separate instrumented on/off evidence."""
from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
RUNNER_SHA256 = "39cd41ddf3ba9cea9247a12093dc509cb3ce299c9a31ee5f3e97e04ce6e8030e"
SCHEMA = "r1.phase-campaign/v1"
VERSIONS = {"baseline": "baseline9632", "candidate": "candidate03"}


def load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_runner(plan):
    path = Path(plan["runner"]["path"])
    if plan["runner"]["sha256"] != RUNNER_SHA256 or hashlib.sha256(path.read_bytes()).hexdigest() != RUNNER_SHA256:
        raise ValueError("runner is not the frozen snapshot03 runner")
    return load(path, "r1_original_campaign")


def unique(records):
    return list({item["path"]: item for item in records}.values())


def research_identities(plan):
    research = plan["phase_research"]
    return unique([research["original_plan"], research["original_pilot"],
                   *research["instrumentation_files"].values(),
                   *[plan[version]["source_evidence"] for version in VERSIONS]])


def check_research_evidence(runner, plan):
    """Check frozen evidence without requiring the original machine or binaries."""
    research = plan.get("phase_research", {})
    if research.get("schema") != SCHEMA or research.get("instrumented_pilot_performed") is not False:
        raise ValueError("unsupported or misleading phase research plan")
    for item in research_identities(plan):
        runner.verify_identity(item)
    if runner.identity(Path(__file__)) != research["instrumentation_files"]["wrapper"]:
        raise ValueError("running wrapper differs from frozen wrapper")
    if runner.identity(HERE / "validate_phase.py") != research["instrumentation_files"]["validator"]:
        raise ValueError("running validator differs from frozen validator")
    original = read(Path(research["original_plan"]["path"]))
    for field in original.keys() - {"created_at", "baseline", "candidate"}:
        if plan[field] != original[field]:
            raise ValueError(f"original frozen condition changed: {field}")
    if research["original_pilot"] != original["pilot"]:
        raise ValueError("original pilot identity changed")
    for version, source_version in VERSIONS.items():
        for field in original[version].keys() - {"binary", "source_evidence"}:
            if plan[version][field] != original[version][field]:
                raise ValueError(f"original version condition changed: {version}.{field}")
        manifest = read(Path(plan[version]["source_evidence"]["path"]))
        if manifest["source_version"] != source_version or manifest["schema"] != "r1.phase.source/v1":
            raise ValueError(f"wrong instrumented source for {version}")
        if manifest["instrumentation_id"] != research["instrumentation_id"]:
            raise ValueError("instrumentation identities differ")
        if plan[version]["sol_version"] != original[version]["sol_version"]:
            raise ValueError("artifact format expectation changed")


def check_research_plan(runner, plan):
    runner.check_plan(plan)
    check_research_evidence(runner, plan)


def freeze(args):
    original_path = args.original_plan.resolve(strict=True)
    original = read(original_path)
    runner = load_runner(original)
    runner.check_plan(original)
    runner.verify_identity(original["pilot"])
    pilot = read(Path(original["pilot"]["path"]))
    if pilot.get("kind") != "pilot":
        raise ValueError("original pilot is not a pilot record")
    if ([case["case"] for case in original["cases"]] != list(runner.CASES)
            or original["baseline"]["sol_version"] != 1 or original["candidate"]["sol_version"] != 3):
        raise ValueError("expected the frozen River/Turn/Flop v1/v3 campaign")
    versions = read(HERE / "source-versions.json")
    plan = copy.deepcopy(original)
    plan["created_at"] = runner.utc_now()
    ids = set()
    for version, source_version in VERSIONS.items():
        manifest_path = getattr(args, version + "_manifest").resolve(strict=True)
        manifest = read(manifest_path)
        if manifest.get("schema") != "r1.phase.source/v1" or manifest.get("source_version") != source_version:
            raise ValueError(f"wrong phase source manifest: {manifest_path}")
        if manifest["before"] != versions[source_version]["build_inputs"]:
            raise ValueError("instrumented source manifest has a different starting source")
        for path, expected in manifest["after"].items():
            if Path(path).is_absolute() or ".." in Path(path).parts:
                raise ValueError("unsafe source manifest path")
            if hashlib.sha256((manifest_path.parent / path).read_bytes()).hexdigest() != expected:
                raise ValueError(f"instrumented source copy changed: {path}")
        ids.add(manifest["instrumentation_id"])
        plan[version]["binary"] = runner.identity(getattr(args, version + "_binary"))
        plan[version]["source_evidence"] = runner.identity(manifest_path)
        plan[version]["instrumented"] = True
    if len(ids) != 1:
        raise ValueError("baseline and candidate must use the same instrumentation")
    expected_id = hashlib.sha256((HERE / "apply_instrumentation.py").read_bytes() + b"\0"
                                + (HERE / "r1_phase.rs.in").read_bytes() + b"\0"
                                + (HERE / "source-versions.json").read_bytes()).hexdigest()
    if ids != {expected_id}:
        raise ValueError("source copies were built with different instrumentation files")
    plan["phase_research"] = {
        "schema": SCHEMA, "original_plan": runner.identity(original_path),
        "original_pilot": copy.deepcopy(original["pilot"]), "instrumented_pilot_performed": False,
        "original_versions": {version: copy.deepcopy(original[version]) for version in VERSIONS},
        "instrumentation_id": ids.pop(),
        "instrumentation_files": {key: runner.identity(HERE / name) for key, name in {
            "wrapper": "run_phases.py", "validator": "validate_phase.py",
            "applier": "apply_instrumentation.py", "module_template": "r1_phase.rs.in",
            "source_versions": "source-versions.json"}.items()},
        "modes": ["on", "off"],
        "phase_scope": "solve stages only; export and resume have R1_PHASE_OUTPUT unset",
        "pilot_scope": "conditions inherited from original uninstrumented pilot; no new pilot claimed",
        "comparison_scope": "three alternating pairs per case in each mode; modes run as separate campaigns",
    }
    check_research_plan(runner, plan)
    runner.write_json(args.out.resolve(), plan, exclusive=True)
    return 0


def validate_stage_phase(runner, validator, plan, version, stage_directory, stage, mode):
    path = stage_directory / "phase.json"
    record = {"mode": mode, "status": "disabled" if mode == "off" else "unavailable",
              "record": None, "summary": None}
    if mode == "off":
        if path.exists():
            raise ValueError("disabled solve unexpectedly created a phase record")
    elif path.is_file():
        record["record"] = runner.identity(path)
        data = read(path)
        record["status"] = data.get("status", "invalid")
        if runner.completed(stage):
            record["summary"] = validator.validate(data, read(Path(plan[version]["source_evidence"]["path"])))
            record["status"] = "validated"
    if mode == "on" and runner.completed(stage) and record["status"] != "validated":
        raise ValueError("successful solve has no valid completed phase record")
    return record


def wrapped_supervisor(runner, validator, plan, mode, original_supervise):
    binaries = {str(Path(plan[v]["binary"]["path"]).resolve()): v for v in VERSIONS}

    def supervise(binary, argv, directory, bounds, identities):
        check_research_plan(runner, plan)
        version = binaries.get(str(Path(binary).resolve()))
        if version is None:
            raise ValueError("unfrozen binary")
        previous = os.environ.get("R1_PHASE_OUTPUT")
        is_solve = bool(argv) and argv[0] == "solve"
        if mode == "on" and is_solve:
            os.environ["R1_PHASE_OUTPUT"] = str((directory / "phase.json").resolve())
        else:
            os.environ.pop("R1_PHASE_OUTPUT", None)
        try:
            extra = [Path(item["path"]) for item in research_identities(plan)]
            stage = original_supervise(binary, argv, directory, bounds, list(dict.fromkeys([*identities, *extra])))
        finally:
            if previous is None:
                os.environ.pop("R1_PHASE_OUTPUT", None)
            else:
                os.environ["R1_PHASE_OUTPUT"] = previous
        check_research_plan(runner, plan)
        if is_solve:
            try:
                record = validate_stage_phase(runner, validator, plan, version, directory, stage, mode)
            except (ValueError, KeyError, OSError) as error:
                runner.write_json(directory / "phase-validation.json", {"mode": mode, "status": "invalid", "error": str(error)})
                raise
            runner.write_json(directory / "phase-validation.json", record)
            stage["r1_phase"] = record
        return stage
    return supervise


def phase_analysis(runner, validator, report, mode):
    output = {"schema": SCHEMA, "kind": "phase_analysis", "mode": mode,
              "campaign_state": report["state"], "cases": [], "saved_profile_br": "not_evaluated",
              "limitations": ["leaf ns only; inclusive CFR envelope is not additive",
                              "total excludes final phase JSON and process startup/exit",
                              "external minus internal is a scope difference, not a measured phase",
                              "existing pipeline phase_timings nulls remain unchanged"]}
    for case in dict.fromkeys(row["case"] for row in report["runs"]):
        entry = {"case": case, "versions": {}}
        for version in VERSIONS:
            rows = [r for r in report["runs"] if r["case"] == case and r["version"] == version]
            valid = [r for r in rows if r["solve"].get("r1_phase", {}).get("status") == "validated"]
            summaries = [r["solve"]["r1_phase"]["summary"] for r in valid]
            for summary in summaries:
                validator.validate_leaf_names(summary["leaves"])
            names = sorted(validator.REQUIRED) if summaries else []
            entry["versions"][version] = {
                "runs": len(rows), "validated_phase_records": len(valid),
                "external_solve_seconds": runner.describe([r["solve"]["elapsed_seconds"] for r in rows if runner.completed(r["solve"])]),
                "phase_total_ns": runner.describe([s["total_ns"] for s in summaries]),
                "external_minus_internal_ns": runner.describe([r["solve"]["elapsed_seconds"] * 1e9 - r["solve"]["r1_phase"]["summary"]["total_ns"] for r in valid]),
                "leaves": {name: {"duration_ns": runner.describe([s["leaves"][name]["duration_ns"] for s in summaries]),
                                  "calls": runner.describe([s["leaves"][name]["calls"] for s in summaries])} for name in names},
            }
        output["cases"].append(entry)
    return output


def run(args):
    plan_path = args.plan.resolve(strict=True)
    plan = read(plan_path)
    runner = load_runner(plan)
    check_research_plan(runner, plan)
    validator = load(HERE / "validate_phase.py", "r1_phase_validator")
    out = args.out.resolve()
    if out.exists():
        raise ValueError("campaign output already exists")
    session_path = out.with_name(out.name + ".phase-session.json")
    session = {"schema": SCHEMA, "mode": args.mode, "state": "running", "started_at": runner.utc_now(),
               "plan": runner.identity(plan_path), "output_directory": str(out), "pipeline_exit_code": None}
    runner.write_json(session_path, session, exclusive=True)
    original = runner.supervise
    previous_rayon = os.environ.get("RAYON_NUM_THREADS")
    os.environ["RAYON_NUM_THREADS"] = "8"
    runner.supervise = wrapped_supervisor(runner, validator, plan, args.mode, original)
    try:
        result = runner.comparison(SimpleNamespace(plan=plan_path, out=out))
        session.update(state="completed", pipeline_exit_code=result)
        return result
    except BaseException as error:
        session.update(state="interrupted", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        runner.supervise = original
        if previous_rayon is None:
            os.environ.pop("RAYON_NUM_THREADS", None)
        else:
            os.environ["RAYON_NUM_THREADS"] = previous_rayon
        session["ended_at"] = runner.utc_now()
        runner.write_json(session_path, session)
        if (out / "comparison.json").is_file():
            runner.write_json(out / "phase-analysis.json", phase_analysis(runner, validator, read(out / "comparison.json"), args.mode))


def row_map(report, expected):
    rows = {(r["case"], r["version"], r["repetition"]): r for r in report["runs"]}
    if len(rows) != len(report["runs"]) or set(rows) != expected or report["state"] != "completed":
        raise ValueError("comparison is incomplete, duplicated, or has a different runlist")
    return rows


def analyze(args):
    plan = read(args.plan)
    runner = load_runner(plan)
    check_research_evidence(runner, plan)
    expected = {(c["case"], v, rep) for c in plan["cases"] for v in VERSIONS for rep in (1, 2, 3)}
    paths = {"on": args.on, "off": args.off, "original": args.original}
    reports = {name: read(path) for name, path in paths.items()}
    rows = {name: row_map(report, expected) for name, report in reports.items()}
    validator = load(HERE / "validate_phase.py", "r1_phase_analysis_validator")
    for row in rows["on"].values():
        phase = row["solve"].get("r1_phase", {})
        if phase.get("status") == "validated":
            runner.verify_identity(phase["record"])
            summary = validator.validate(read(Path(phase["record"]["path"])),
                                         read(Path(plan[row["version"]]["source_evidence"]["path"])))
            if summary != phase["summary"]:
                raise ValueError("stored phase summary differs from validated raw record")
    result = {"schema": SCHEMA, "kind": "calibration", "plan": runner.identity(args.plan),
              "reports": {name: runner.identity(path) for name, path in paths.items()},
              "saved_profile_br": "not_evaluated", "comparisons": [], "cases": [],
              "caveat": "separate mode campaigns with warm/uncontrolled caches; differences include order/noise, not solely instrumentation cost"}
    for name in ("on", "off"):
        if reports[name]["plan"]["sha256"] != result["plan"]["sha256"]:
            raise ValueError("on/off report used a different instrumented plan")
    if reports["original"]["plan"]["sha256"] != plan["phase_research"]["original_plan"]["sha256"]:
        raise ValueError("original report used a different original plan")
    base_eligible = {name: all(c["performance_comparison_eligible"] for c in runner.analyze_report(report)["cases"])
                     for name, report in reports.items()}
    result["original_pipeline_comparison_eligible"] = base_eligible
    for key in sorted(expected):
        group = {name: mapping[key] for name, mapping in rows.items()}
        checks = {"live_final_equal": all(r["live_final"] == group["original"]["live_final"] for r in group.values()),
                  "summary_equal": all(runner.equal_summary(r["summary"], group["original"]["summary"]) for r in group.values()),
                  "saved_export_equal": all(runner.equal_profile(r["profile"], group["original"]["profile"]) for r in group.values()),
                  "run_config_equal": all(r["artifacts"].get("run.toml", {}).get("sha256") for r in group.values())
                                      and len({r["artifacts"]["run.toml"]["sha256"] for r in group.values()}) == 1,
                  "on_phase_validated": group["on"]["solve"].get("r1_phase", {}).get("status") == "validated",
                  "off_phase_disabled": group["off"]["solve"].get("r1_phase", {}).get("status") == "disabled"}
        result["comparisons"].append({"case": key[0], "version": key[1], "repetition": key[2], "checks": checks,
                                      "status": "pass" if all(checks.values()) else "not_evaluated"})
    for case in plan["cases"]:
        for version in VERSIONS:
            values = {name: [mapping[(case["case"], version, rep)]["solve"]["elapsed_seconds"] for rep in (1, 2, 3)] for name, mapping in rows.items()}
            stats = {name: runner.describe(value) for name, value in values.items()}
            result["cases"].append({"case": case["case"], "version": version, "external_solve_seconds": stats,
                "on_over_off_median_ratio": stats["on"]["median"] / stats["off"]["median"] if stats["on"] and stats["off"] and stats["off"]["median"] else None,
                "off_over_original_median_ratio": stats["off"]["median"] / stats["original"]["median"] if stats["off"] and stats["original"] and stats["original"]["median"] else None})
    result["phase_analysis_on"] = phase_analysis(runner, validator, reports["on"], "on")
    result["eligible"] = all(base_eligible.values()) and all(r["status"] == "pass" for r in result["comparisons"])
    runner.write_json(args.out, result, exclusive=True)
    return 0 if result["eligible"] else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    command = sub.add_parser("freeze")
    command.add_argument("--original-plan", required=True, type=Path)
    for version in VERSIONS:
        command.add_argument("--" + version + "-binary", required=True, type=Path)
        command.add_argument("--" + version + "-manifest", required=True, type=Path)
    command.add_argument("--out", required=True, type=Path)
    command = sub.add_parser("run")
    command.add_argument("--plan", required=True, type=Path)
    command.add_argument("--mode", choices=("on", "off"), required=True)
    command.add_argument("--out", required=True, type=Path)
    command = sub.add_parser("analyze")
    for name in ("plan", "on", "off", "original", "out"):
        command.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        return {"freeze": freeze, "run": run, "analyze": analyze}[args.command](args)
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        print(f"phase campaign: {type(error).__name__}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
