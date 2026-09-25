#!/usr/bin/env python3
"""Freeze and audit all 18 original Full policies from a fixed v1/v3 campaign.

freeze --plan PLAN --comparison REPORT --baseline-audit EXE --baseline-source BUILD_RESULT
       --candidate-audit EXE --candidate-source BUILD_RESULT --b3sum /usr/bin/b3sum --out NEW_DIR
run --plan NEW_DIR/plan.json --out NEW_RUN_DIR

Python 3.11+ stdlib; the independent BLAKE3 command is supervised and pinned.
No build, cloud action, retry, quality retuning, or resume evaluation occurs.
Audit load time/RSS are diagnostic only, not original pipeline performance.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import shutil
import sys
import time
import tomllib

HERE = Path(__file__).resolve().parent
SCHEMA = "r1.saved-profile-campaign/v1"
AUDIT_SCHEMA = "solvers.research.hu-saved-profile-audit/v1"
RUNNER_SHA256 = "39cd41ddf3ba9cea9247a12093dc509cb3ce299c9a31ee5f3e97e04ce6e8030e"
ABS_TOLERANCE = 1e-10
REL_TOLERANCE = 1e-12
VERSIONS = {"baseline": 1, "candidate": 3}
BUILD_ROLES = {"baseline": "baseline_example", "candidate": "current_example"}


def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def load_runner(plan):
    identity = plan["runner"]
    path = Path(identity["path"])
    if identity["sha256"] != RUNNER_SHA256 or hashlib.sha256(path.read_bytes()).hexdigest() != RUNNER_SHA256:
        raise ValueError("expected frozen snapshot03 campaign runner")
    spec = importlib.util.spec_from_file_location("saved_profile_original_runner", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def finite(value):
    return type(value) in (int, float) and math.isfinite(value)


def verify_file(runner, item):
    # Pipeline identities compare the full dict; artifact records also carry
    # headers and this campaign adds independent BLAKE3 evidence.
    runner.verify_identity({key: item[key] for key in ("path", "sha256", "bytes")})


def verify_audit_build(runner, sources):
    """Require both selected examples to come from one completed bounded build."""
    if set(sources) != set(VERSIONS):
        raise ValueError("audit build must identify baseline and candidate")
    evidence = sources["baseline"]["source_evidence"]
    if sources["candidate"]["source_evidence"] != evidence:
        raise ValueError("both audit examples must identify the same build result")
    runner.verify_identity(evidence)
    build = read(evidence["path"])
    if (build.get("schema") != "r1.audit-pair-build/v1" or build.get("status") != "completed"
            or build.get("purpose") != "saved_profile_quality_only"):
        raise ValueError("audit source evidence is not a completed quality-only pair build")
    identities = build["identities"]
    for name in ("audit-source.tar.gz", "baseline-source.tar.gz", "baseline_adapter_manifest"):
        runner.verify_identity(identities[name])
    for version, role in BUILD_ROLES.items():
        selected = sources[version]
        if selected.get("build_role") != role or selected["binary"] != identities[role]:
            raise ValueError(f"selected {version} audit binary differs from its completed build role")
        runner.verify_identity(selected["binary"])


def keys(document, allowed, context):
    if not isinstance(document, dict) or set(document) - set(allowed):
        raise ValueError(f"unsupported fields in {context}: {set(document) - set(allowed) if isinstance(document, dict) else document}")


def effective_fixture(document):
    """Only the three fixed script/DCFR/no-rake/chip-EV fixture configurations.

    Fill explicit parser defaults; reject all unknown fields. This intentionally
    does not implement a general game normalizer or ignore runtime extensions.
    """
    doc = copy.deepcopy(document)
    keys(doc, {"schema", "game", "algorithm", "run", "rake", "utility"}, "config")
    if doc.get("schema") != "solvers.postflop/v1":
        raise ValueError("unsupported config schema")
    game = doc["game"]
    keys(game, {"board", "oop_range", "ip_range", "pot", "effective_stack", "min_bet",
                "iso_merging", "preflop_aggressor", "tree"}, "game")
    for key in ("board", "oop_range", "ip_range", "pot", "effective_stack", "tree"):
        if key not in game:
            raise ValueError(f"missing game field: {key}")
    for key, value in {"min_bet": 1, "iso_merging": False, "preflop_aggressor": "none"}.items():
        game.setdefault(key, value)
    tree = game["tree"]
    keys(tree, {"kind", "script", "include_allin", "max_aggressive_actions", "params"}, "tree")
    if tree.get("kind") != "script" or not isinstance(tree.get("script"), str):
        raise ValueError("only inline script fixtures are supported")
    tree["script"] = tree["script"].strip()
    tree.setdefault("include_allin", False)
    tree.setdefault("params", {})
    if tree["params"] != {}:
        raise ValueError("fixture script parameters are unsupported")
    caps = tree.setdefault("max_aggressive_actions", {})
    keys(caps, {"flop", "turn", "river"}, "max_aggressive_actions")
    for street in ("flop", "turn", "river"):
        caps.setdefault(street, 2)
    algorithm = doc.setdefault("algorithm", {"schedule": "dcfr"})
    keys(algorithm, {"schedule", "alpha", "beta", "gamma", "pow4_reset"}, "algorithm")
    if algorithm.get("schedule") != "dcfr":
        raise ValueError("only fixed DCFR fixtures are supported")
    for key, value in {"alpha": 1.5, "beta": 0.0, "gamma": 3.0, "pow4_reset": True}.items():
        algorithm.setdefault(key, value)
    run = doc["run"]
    keys(run, {"iterations", "check_every", "target_nash_conv", "threads", "storage",
               "par_chance_depth", "par_min_children"}, "run")
    for key in ("iterations", "check_every", "target_nash_conv", "threads", "storage"):
        if key not in run:
            raise ValueError(f"missing run field: {key}")
    run.setdefault("par_chance_depth", 2)
    run.setdefault("par_min_children", 12)
    doc.setdefault("rake", {"kind": "none"})
    doc.setdefault("utility", {"kind": "chip-ev"})
    if (doc["rake"] != {"kind": "none"} or doc["utility"] != {"kind": "chip-ev"}
            or run["threads"] != 8 or run["target_nash_conv"] != 0.04 or run["storage"] != "f32"):
        raise ValueError("audit campaign requires the fixed 8-thread F32 no-rake chip-EV target 0.04")
    return doc


def effective_identity(document):
    raw = json.dumps(document, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    return hashlib.sha256(raw).hexdigest()


def parse_b3_stdout(data):
    text = data.decode("utf-8").strip()
    if re.fullmatch(r"[0-9a-f]{64}", text) is None:
        raise ValueError("b3sum --no-names did not return exactly one lowercase BLAKE3 digest")
    return text


def supervised(runner, binary, argv, directory, bounds, identities):
    previous_phase = os.environ.pop("R1_PHASE_OUTPUT", None)
    previous_threads = os.environ.get("RAYON_NUM_THREADS")
    os.environ["RAYON_NUM_THREADS"] = "8"
    try:
        return runner.supervise(binary, argv, directory, bounds, identities)
    finally:
        if previous_phase is not None:
            os.environ["R1_PHASE_OUTPUT"] = previous_phase
        if previous_threads is None:
            os.environ.pop("RAYON_NUM_THREADS", None)
        else:
            os.environ["RAYON_NUM_THREADS"] = previous_threads


def original_rows(runner, original, comparison):
    if comparison.get("state") != "completed":
        raise ValueError("original comparison did not complete")
    expected = [(case["case"], version, index // 2 + 1)
                for case in original["cases"] for index, version in enumerate(original["order"])]
    actual = [(row["case"], row["version"], row["repetition"]) for row in comparison["runs"]]
    if len(expected) != 18 or actual != expected or set(VERSIONS) != {v for _, v, _ in actual}:
        raise ValueError("expected all 18 original solves in the frozen case/pair order")
    if not all(c["performance_comparison_eligible"] for c in runner.analyze_report(comparison)["cases"]):
        raise ValueError("original comparison did not pass its fixed profile/quality checks")
    return comparison["runs"]


def freeze(args):
    started = time.monotonic()
    original_path = args.plan.resolve(strict=True)
    original = read(original_path)
    runner = load_runner(original)
    runner.check_plan(original)
    if [c["case"] for c in original["cases"]] != list(runner.CASES):
        raise ValueError("expected River/Turn/Flop frozen order")
    for version, expected in VERSIONS.items():
        if original[version]["sol_version"] != expected:
            raise ValueError("expected baseline v1 and candidate v3")
    comparison_path = args.comparison.resolve(strict=True)
    comparison = read(comparison_path)
    if comparison["plan"] != runner.identity(original_path):
        raise ValueError("comparison does not identify this original plan")
    rows = original_rows(runner, original, comparison)
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    b3path = args.b3sum or Path(shutil.which("b3sum") or "")
    b3identity = runner.identity(b3path)
    sources = {v: {"binary": runner.identity(getattr(args, v + "_audit")),
                   "source_evidence": runner.identity(getattr(args, v + "_source")),
                   "format_version": fmt, "build_role": BUILD_ROLES[v]} for v, fmt in VERSIONS.items()}
    verify_audit_build(runner, sources)
    plan = {"schema": SCHEMA, "kind": "frozen_audit_plan", "created_at": runner.utc_now(),
            "original_plan": runner.identity(original_path), "original_comparison": runner.identity(comparison_path),
            "original_runner": original["runner"], "supervisor": original["supervisor"],
            "audit_runner": runner.identity(Path(__file__)), "host": original["host"],
            "limits": original["limits"], "campaign_seconds": original["campaign_seconds"],
            "audit_versions": sources, "threads": 8, "b3sum": {"binary": b3identity}, "runs": [],
            "pair_tolerance": {"absolute": ABS_TOLERANCE, "relative": REL_TOLERANCE,
                               "formula": "abs(a-b) <= absolute + relative * max(abs(a), abs(b))",
                               "scope": "numeric record equivalence only; never relaxes NC target or implies external acceptance"},
            "quality_scope": "stored_quantized Full policy in the fixed synthetic finite game; no external reference acceptance"}
    status = {"schema": SCHEMA, "state": "preparing", "started_at": runner.utc_now()}
    runner.write_json(out / "preparation.json", status, exclusive=True)
    identities = [Path(__file__), original_path, comparison_path, Path(b3identity["path"]),
                  *[Path(s["source_evidence"]["path"]) for s in sources.values()]]
    bounds = {**original["limits"], "_campaign_deadline": started + original["campaign_seconds"]}
    try:
        version_stage = supervised(runner, Path(b3identity["path"]), ["--version"], out / "hashes/tool-version", bounds, identities)
        if not runner.completed(version_stage):
            raise ValueError("BLAKE3 tool version check did not complete")
        plan["b3sum"]["version_stage"] = version_stage
        plan["b3sum"]["version_text"] = Path(version_stage["stdout"]["path"]).read_text(encoding="utf-8").strip()
        for row in rows:
            case = next(c for c in original["cases"] if c["case"] == row["case"])
            expected_config = effective_fixture(tomllib.loads(Path(case["config"]["path"]).read_text(encoding="utf-8")))
            artifact = copy.deepcopy(row["artifacts"]["solution.sol"])
            run_config = copy.deepcopy(row["artifacts"]["run.toml"])
            for item in (artifact, run_config, case["config"]):
                verify_file(runner, item)
            observed_config = effective_fixture(tomllib.loads(Path(run_config["path"]).read_text(encoding="utf-8")))
            if expected_config != observed_config:
                raise ValueError("artifact run.toml differs semantically from the frozen original config")
            header = runner.artifact_header(Path(artifact["path"]))
            if header != artifact["header"] or header["version"] != VERSIONS[row["version"]] or header["iteration"] != case["iterations"]:
                raise ValueError("artifact header disagrees with the frozen original solve")
            item = {"case": row["case"], "version": row["version"], "repetition": row["repetition"],
                    "artifact": artifact, "run_config": run_config, "frozen_config": case["config"],
                    "effective_config": observed_config, "effective_config_sha256": effective_identity(observed_config),
                    "expected_iterations": case["iterations"], "target_nash_conv": case["target_nash_conv"],
                    "expected_nodes": row["summary"]["nodes"], "expected_stored_nodes": row["summary"]["stored_nodes"],
                    "hash_stages": {}}
            if row["summary"]["streets_stored"] != "full" or item["target_nash_conv"] != 0.04:
                raise ValueError("only original Full artifacts at the fixed NC target are supported")
            name = f"{row['case']}-{row['repetition']}-{row['version']}"
            for label, frozen in (("artifact", artifact), ("run_config", run_config)):
                stage = supervised(runner, Path(b3identity["path"]), ["--no-names", "--", frozen["path"]],
                                   out / "hashes" / name / label, bounds, [Path(frozen["path"]), *identities])
                if not runner.completed(stage):
                    raise ValueError(f"independent input hash did not complete: {name}/{label}")
                verify_file(runner, frozen)
                frozen["blake3"] = parse_b3_stdout(Path(stage["stdout"]["path"]).read_bytes())
                item["hash_stages"][label] = stage
            if run_config["blake3"] != header["embedded_config_blake3"]:
                raise ValueError("independently hashed run.toml differs from artifact embedded config hash")
            plan["runs"].append(item)
        for case in original["cases"]:
            if len({r["effective_config_sha256"] for r in plan["runs"] if r["case"] == case["case"]}) != 1:
                raise ValueError("cross-pair effective configs differ")
        runner.check_plan(original)
        for v in sources.values():
            runner.verify_identity(v["binary"])
            runner.verify_identity(v["source_evidence"])
        verify_audit_build(runner, sources)
        runner.verify_identity(plan["audit_runner"])
        runner.verify_identity(b3identity)
        runner.write_json(out / "plan.json", plan, exclusive=True)
        status["state"] = "frozen"
    except BaseException as error:
        status.update(state="failed", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        status["ended_at"] = runner.utc_now()
        runner.write_json(out / "preparation.json", status)
    return 0


def validate_report(report, frozen):
    if report.get("schema") != AUDIT_SCHEMA or report.get("value_basis") != "subgame_start_utility":
        raise ValueError("unsupported saved-profile report schema or value basis")
    artifact = report["artifact"]
    expected = frozen["artifact"]
    config = frozen["effective_config"]
    if (Path(artifact["path"]).resolve() != Path(expected["path"]).resolve()
            or artifact["blake3"] != expected["blake3"] or artifact["bytes"] != expected["bytes"]
            or artifact["config_blake3"] != frozen["run_config"]["blake3"]
            or artifact["format_version"] != VERSIONS[frozen["version"]]
            or artifact["mode"] != "full" or artifact["iterations"] != frozen["expected_iterations"]
            or artifact["source_storage"] != config["run"]["storage"]
            or artifact["node_count"] != frozen["expected_nodes"]
            or artifact["stored_nodes"] != frozen["expected_stored_nodes"]):
        raise ValueError("audit artifact identity/header/counts differ from frozen input")
    if (report["threads"] != 8 or report["par_chance_depth"] != config["run"]["par_chance_depth"]
            or report["par_min_children"] != config["run"]["par_min_children"]
            or report["pot_chips"] != config["game"]["pot"]
            or report["effective_stack_chips"] != config["game"]["effective_stack"]
            or report["rake"] != config["rake"] or report["utility"] != config["utility"]
            or report["zero_sum_terminal_utility"] is not True):
        raise ValueError("audit game/utility/worker settings differ from frozen fixture")
    values = report["recomputed"]
    if values["profile"] != "stored_quantized":
        raise ValueError("quality requires a recomputed stored_quantized policy")
    for name in ("ev", "br", "gains"):
        if not isinstance(values[name], list) or len(values[name]) != 2 or not all(finite(x) for x in values[name]):
            raise ValueError(f"invalid finite two-seat values: {name}")
    if not finite(values["nash_conv"]):
        raise ValueError("nonfinite recomputed NC")
    if len(report["ev_offset"]) != 2 or not all(finite(x) for x in report["ev_offset"]):
        raise ValueError("invalid EV offset")
    for name in ("input_hash_secs", "load_secs", "eval_secs"):
        if not finite(report[name]) or report[name] < 0:
            raise ValueError(f"invalid diagnostic timing: {name}")
    gains = [values["br"][p] - values["ev"][p] for p in (0, 1)]
    nc = gains[0] + gains[1]
    if gains != values["gains"] or nc != values["nash_conv"]:
        raise ValueError("reported gains/NC disagree with exact JSON-restored f64 arithmetic")
    roundoff_bounds = [ABS_TOLERANCE + REL_TOLERANCE * max(abs(values["br"][p]), abs(values["ev"][p]))
                      for p in (0, 1)]
    for seat, (gain, bound) in enumerate(zip(gains, roundoff_bounds)):
        if gain < -bound:
            raise ValueError(f"BR gain below roundoff bound: seat {seat}, gain={gain}, lower_bound={-bound}")
    nc_lower_bound = -sum(roundoff_bounds)
    if nc < nc_lower_bound:
        raise ValueError(f"NC below combined roundoff bound: nash_conv={nc}, lower_bound={nc_lower_bound}")
    return {"status": "valid", "strategy_kind": "stored_quantized", "gains": gains, "nash_conv": nc,
            "gain_roundoff_lower_bounds": [-bound for bound in roundoff_bounds],
            "nash_conv_roundoff_lower_bound": nc_lower_bound,
            "target_nash_conv": frozen["target_nash_conv"], "inequality": "<",
            "quality_status": "pass" if nc < frozen["target_nash_conv"] else "fail",
            "exploitability_pct_pot": 50 * nc / report["pot_chips"],
            "pre_save_metadata_used_for_quality": False}


def compare_pair(left, right):
    if left.get("validation", {}).get("status") != "valid" or right.get("validation", {}).get("status") != "valid":
        return {"status": "not_evaluated"}
    a, b = left["report"], right["report"]
    fields = ("threads", "par_chance_depth", "par_min_children", "pot_chips", "effective_stack_chips",
              "rake", "utility", "zero_sum_terminal_utility", "value_basis", "ev_offset")
    conditions = (left["effective_config_sha256"] == right["effective_config_sha256"]
                  and all(a[f] == b[f] for f in fields)
                  and all(a["artifact"][f] == b["artifact"][f] for f in
                          ("iterations", "mode", "source_storage", "stored_nodes", "node_count")))
    numbers = {name: list(zip(a["recomputed"][name], b["recomputed"][name])) for name in ("ev", "br", "gains")}
    numbers["nash_conv"] = [(a["recomputed"]["nash_conv"], b["recomputed"]["nash_conv"])]
    exact = all(x == y for pairs in numbers.values() for x, y in pairs)
    equivalent = all(abs(x - y) <= ABS_TOLERANCE + REL_TOLERANCE * max(abs(x), abs(y))
                     for pairs in numbers.values() for x, y in pairs)
    status = "exact" if conditions and exact else "numerically_equivalent" if conditions and equivalent else "different"
    return {"status": status, "same_effective_conditions": conditions, "exact_values": exact,
            "within_predeclared_record_tolerance": equivalent,
            "raw_config_hash_equal": a["artifact"]["config_blake3"] == b["artifact"]["config_blake3"],
            "absolute_differences": {name: [abs(x-y) for x, y in pairs] for name, pairs in numbers.items()},
            "both_saved_profile_targets_pass": all(r["validation"]["quality_status"] == "pass" for r in (left, right)),
            "scope": "numeric comparison of stored policies in this finite synthetic game; not external acceptance"}


def check_plan(runner, plan):
    if plan.get("schema") != SCHEMA or plan.get("kind") != "frozen_audit_plan" or len(plan["runs"]) != 18:
        raise ValueError("unsupported audit plan")
    if plan["audit_runner"] != runner.identity(Path(__file__)):
        raise ValueError("audit runner changed after freeze")
    if plan["pair_tolerance"]["absolute"] != ABS_TOLERANCE or plan["pair_tolerance"]["relative"] != REL_TOLERANCE:
        raise ValueError("pair tolerance changed")
    for item in (plan["original_plan"], plan["original_comparison"], plan["original_runner"], plan["supervisor"], plan["b3sum"]["binary"]):
        runner.verify_identity(item)
    original = read(plan["original_plan"]["path"])
    comparison = read(plan["original_comparison"]["path"])
    runner.check_plan(original)
    original_runlist = original_rows(runner, original, comparison)
    if plan["limits"] != original["limits"] or plan["campaign_seconds"] != original["campaign_seconds"] or plan["threads"] != 8:
        raise ValueError("audit limits/threads differ from frozen original")
    verify_audit_build(runner, plan["audit_versions"])
    for row, old in zip(plan["runs"], original_runlist):
        if any(row[key] != old[key] for key in ("case", "version", "repetition")):
            raise ValueError("audit runlist differs from original order")
        for name, old_name in (("artifact", "solution.sol"), ("run_config", "run.toml")):
            if any(row[name][key] != old["artifacts"][old_name][key] for key in ("path", "sha256", "bytes")):
                raise ValueError("audit input differs from original artifact")
        for name in ("artifact", "run_config", "frozen_config"):
            verify_file(runner, row[name])
        config = effective_fixture(tomllib.loads(Path(row["run_config"]["path"]).read_text(encoding="utf-8")))
        frozen_config = effective_fixture(tomllib.loads(Path(row["frozen_config"]["path"]).read_text(encoding="utf-8")))
        if (config != frozen_config or row["effective_config"] != config
                or row["effective_config_sha256"] != effective_identity(config)
                or row["target_nash_conv"] != 0.04 or row["target_nash_conv"] != config["run"]["target_nash_conv"]
                or row["expected_iterations"] != config["run"]["iterations"]):
            raise ValueError("frozen effective condition or quality target changed")


def summarize(plan, runs, *, campaign_state):
    pairs = []
    for case in dict.fromkeys(r["case"] for r in plan["runs"]):
        for repetition in (1, 2, 3):
            pair = {r["version"]: r for r in runs if r["case"] == case and r["repetition"] == repetition}
            verdict = compare_pair(pair["baseline"], pair["candidate"]) if set(pair) == set(VERSIONS) else {"status": "not_evaluated"}
            pairs.append({"case": case, "repetition": repetition, **verdict})
    passed = (campaign_state == "completed" and len(runs) == 18
              and all(r.get("validation", {}).get("quality_status") == "pass" for r in runs)
              and all(p["status"] in ("exact", "numerically_equivalent") for p in pairs))
    return {"schema": SCHEMA, "kind": "saved_profile_analysis", "campaign_state": campaign_state,
            "runs": len(runs), "pairs": pairs,
            "saved_profile_comparison_eligible": passed,
            "saved_profile_quality": "pass" if passed else "not_evaluated_or_failed",
            "external_reference_acceptance": "not_evaluated", "pair_tolerance": plan["pair_tolerance"],
            "performance_scope": "audit adapter timing/RSS are diagnostic only; excluded from original solve performance"}


def run(args):
    started = time.monotonic()
    plan_path = args.plan.resolve(strict=True)
    plan = read(plan_path)
    runner = load_runner({"runner": plan["original_runner"]})
    check_plan(runner, plan)
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    report = {"schema": SCHEMA, "kind": "saved_profile_audits", "plan": runner.identity(plan_path),
              "state": "running", "started_at": runner.utc_now(), "runs": []}
    runner.write_json(out / "audits.json", report, exclusive=True)
    bounds = {**plan["limits"], "_campaign_deadline": started + plan["campaign_seconds"]}
    try:
        for frozen in plan["runs"]:
            check_plan(runner, plan)
            name = f"{frozen['case']}-{frozen['repetition']}-{frozen['version']}"
            version = plan["audit_versions"][frozen["version"]]
            stage = supervised(runner, Path(version["binary"]["path"]),
                ["--sol", frozen["artifact"]["path"], "--threads", "8"], out / name, bounds,
                [Path(__file__), plan_path, Path(version["source_evidence"]["path"]),
                 Path(frozen["artifact"]["path"]), Path(frozen["run_config"]["path"]),
                 Path(frozen["frozen_config"]["path"]), Path(plan["original_comparison"]["path"])])
            result = {key: frozen[key] for key in ("case", "version", "repetition", "effective_config_sha256")}
            result.update(stage=stage, run_status=runner.run_status(stage), report=None,
                          validation={"status": "not_evaluated", "quality_status": "not_evaluated"})
            if runner.completed(stage):
                try:
                    result["report"] = read(stage["stdout"]["path"])
                    result["validation"] = validate_report(result["report"], frozen)
                except (OSError, ValueError, KeyError, TypeError) as error:
                    result["validation"] = {"status": "invalid", "quality_status": "not_evaluated", "error": str(error)}
            report["runs"].append(result)
            runner.write_json(out / name / "validation.json", result)
            runner.write_json(out / "audits.json", report)
            check_plan(runner, plan)
        report["state"] = "completed"
    except BaseException as error:
        report.update(state="interrupted", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        report["ended_at"] = runner.utc_now()
        runner.write_json(out / "audits.json", report)
        runner.write_json(out / "analysis.json", summarize(plan, report["runs"], campaign_state=report["state"]))
    return 0 if summarize(plan, report["runs"], campaign_state=report["state"])["saved_profile_comparison_eligible"] else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    command = commands.add_parser("freeze")
    for name in ("plan", "comparison", "baseline-audit", "baseline-source", "candidate-audit", "candidate-source", "out"):
        command.add_argument("--" + name, required=True, type=Path)
    command.add_argument("--b3sum", type=Path, help="independent b3sum executable; default PATH lookup")
    command = commands.add_parser("run")
    command.add_argument("--plan", required=True, type=Path)
    command.add_argument("--out", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        return {"freeze": freeze, "run": run}[args.command](args)
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        print(f"saved-profile campaign: {type(error).__name__}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
