#!/usr/bin/env python3
"""Preflight or explicitly run the bounded local algorithm screen."""
from __future__ import annotations

import argparse
import copy
import json
import math
import os
import subprocess
import time
import tomllib
from pathlib import Path

from gcp_reference_pilot import durable_text, sha, utc
from multiway_simple_reference_compare import DRAW_RESEARCH_SCHEMA, validate_research_schema

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "runs/multiway-convergence-round5-20260909/local-algorithm-screen/manifest.json"


def read_json(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object: {path}")
    return value


def hex64(value: object) -> bool:
    return isinstance(value, str) and len(value) == 64 and all(c in "0123456789abcdef" for c in value)


def validate_estimate(value: object) -> None:
    if not isinstance(value, dict) or not all(math.isfinite(value.get(k, math.nan)) for k in ("mean", "stderr")):
        raise ValueError("invalid ProfileEstimate")
    ci = value.get("ci95")
    if not isinstance(ci, list) or len(ci) != 2 or not all(isinstance(x, (int, float)) and math.isfinite(x) for x in ci):
        raise ValueError("invalid ProfileEstimate.ci95")


def validate_histories(result: dict, count: int = 5, rows: int = 169) -> None:
    histories = result.get("histories")
    if not isinstance(histories, list) or len(histories) != count:
        raise ValueError("history count mismatch")
    for history in histories:
        strategies = history.get("strategies") if isinstance(history, dict) else None
        if not isinstance(strategies, list) or len(strategies) != rows:
            raise ValueError("strategy-row count mismatch")
        for row in strategies:
            status, actions = row.get("status"), row.get("actions")
            if status == "zero-average-mass-omitted" and actions is None:
                continue
            if status != "average-observed" or not isinstance(actions, list) or not actions:
                raise ValueError("invalid average row status/actions")
            probabilities = [action.get("probability") for action in actions if isinstance(action, dict)]
            if len(probabilities) != len(actions) or not all(isinstance(x, (int, float)) and math.isfinite(x) and 0 <= x <= 1 for x in probabilities):
                raise ValueError("invalid action probability")
            if not math.isclose(sum(probabilities), 1.0, abs_tol=2e-5):
                raise ValueError("average row is not normalized")


def validate_output(value: dict, arm: dict, common: dict, source_hash: str) -> None:
    validate_research_schema(value)
    if "research_abstraction" in arm:
        if value.get("schemaVersion") != DRAW_RESEARCH_SCHEMA or value.get("researchAbstraction") != arm["research_abstraction"]:
            raise ValueError("research abstraction experiment mismatch")
        if value["researchAbstraction"]["effectiveBuckets"] != common["expected_tree"]["buckets"]:
            raise ValueError("research bucket budget differs from manifest")
    elif value.get("schemaVersion") != "solvers.multiway-average-sampling-research/v1":
        raise ValueError("unexpected abstraction experiment output")
    if value.get("sourceRevision") != source_hash or value.get("config") != arm["config"]:
        raise ValueError("source/config identity mismatch")
    for key in ("executableBlake3", "effectiveConfigBlake3", "configurationFingerprint", "abstractionFingerprint"):
        if not hex64(value.get(key)):
            raise ValueError(f"invalid {key}")
    if value.get("solverStateVersion") != 4 or not math.isfinite(value.get("elapsedSecs", math.nan)):
        raise ValueError("invalid solverStateVersion/elapsedSecs")
    result = value.get("result")
    if not isinstance(result, dict):
        raise ValueError("result must be an object")
    metrics = result.get("metrics")
    if value.get("threads") != common["threads"] or result.get("threads") != common["threads"] or result.get("variant") != "uniform-one":
        raise ValueError("thread/variant mismatch")
    if not isinstance(metrics, dict) or metrics.get("sweeps") != common["sweeps"] or metrics.get("traversals") != common["sweeps"] * 6:
        raise ValueError("sweep/traversal mismatch")
    if not hex64(result.get("current_regret_fingerprint")):
        raise ValueError("invalid current_regret_fingerprint")
    validate_histories(result)
    evaluations = result.get("evaluations")
    if not isinstance(evaluations, list) or [x.get("seed") for x in evaluations] != common["evaluation_seeds"]:
        raise ValueError("evaluation seed mismatch")
    for entry in evaluations:
        profile = entry.get("result")
        if not isinstance(profile, dict) or profile.get("samples") != common["evaluation_samples"]:
            raise ValueError("ProfileEvaluation sample mismatch")
        if not isinstance(profile.get("total_deal_attempts"), int) or profile["total_deal_attempts"] < common["evaluation_samples"]:
            raise ValueError("ProfileEvaluation deal-attempt mismatch")
        seats, gains = profile.get("seats"), profile.get("deviation_gain_lower_bound")
        if not isinstance(seats, list) or len(seats) != 6 or not isinstance(gains, list) or len(gains) != 6:
            raise ValueError("ProfileEvaluation seat/gain shape mismatch")
        for estimate in seats + gains:
            validate_estimate(estimate)


def validate_control_gate(manifest: dict) -> dict | None:
    gate = manifest.get("control_gate")
    if gate is None:
        return None
    if not isinstance(gate, dict) or set(gate) != {"control_arm", "required_for", "reference"}:
        raise ValueError("invalid control_gate fields")
    control_name, required_for, reference = gate["control_arm"], gate["required_for"], gate["reference"]
    arms = manifest.get("arms")
    if not isinstance(control_name, str) or not control_name or not isinstance(arms, list):
        raise ValueError("invalid control_gate control_arm")
    arm_names = [arm.get("name") for arm in arms if isinstance(arm, dict)]
    if len(arm_names) != len(arms) or len(set(arm_names)) != len(arm_names) or control_name not in arm_names:
        raise ValueError("control_gate names do not match unique manifest arms")
    if (not isinstance(required_for, list) or not required_for or
            any(not isinstance(name, str) for name in required_for) or
            len(set(required_for)) != len(required_for) or control_name in required_for or
            any(name not in arm_names for name in required_for)):
        raise ValueError("invalid control_gate required_for")
    if not isinstance(reference, dict) or set(reference) != {"path", "sha256"}:
        raise ValueError("invalid control_gate reference")
    reference_path = reference.get("path")
    if not isinstance(reference_path, str) or not reference_path or Path(reference_path).is_absolute() or not hex64(reference.get("sha256")):
        raise ValueError("invalid control_gate reference identity")
    path = (ROOT / reference_path).resolve()
    try:
        path.relative_to(ROOT.resolve())
    except ValueError as error:
        raise ValueError("control_gate reference must stay within repository") from error
    if not path.is_file() or sha(path) != reference["sha256"]:
        raise ValueError("control_gate reference SHA-256 mismatch")
    value = read_json(path)
    if (value.get("solverStateVersion") != 4 or
            not hex64(value.get("configurationFingerprint")) or
            not hex64(value.get("abstractionFingerprint")) or
            not isinstance(value.get("result"), dict)):
        raise ValueError("control_gate reference is malformed")
    if any(arm_names.index(control_name) >= arm_names.index(name) for name in required_for):
        raise ValueError("control_gate control arm must precede gated arms")
    return gate


def control_evidence(manifest: dict, gated_arm: dict) -> dict | None:
    gate = manifest.get("control_gate")
    if gate is None or gated_arm["name"] not in gate["required_for"]:
        return None
    control = next(arm for arm in manifest["arms"] if arm["name"] == gate["control_arm"])
    record_path = ROOT / control["execution_record"]
    stdout_path = ROOT / control["stdout"]
    stderr_path = ROOT / control["stderr"]
    partial_path = stdout_path.with_suffix(".json.partial")
    if (not record_path.is_file() or not stdout_path.is_file() or
            not stderr_path.is_file() or partial_path.exists()):
        raise ValueError(f"control output is incomplete for gated arm: {gated_arm['name']}")
    record = read_json(record_path)
    expected = {
        "command": control["argv"],
        "binary_sha256": manifest["executable"]["sha256"],
        "config_sha256": control["config_sha256"],
        "stdout_sha256": sha(stdout_path),
    }
    if record.get("status") != "complete" or record.get("returncode") != 0 or any(record.get(k) != v for k, v in expected.items()):
        raise ValueError(f"control execution record mismatch for gated arm: {gated_arm['name']}")
    if sha(ROOT / manifest["executable"]["path"]) != expected["binary_sha256"] or sha(ROOT / control["config"]) != expected["config_sha256"]:
        raise ValueError(f"control binary/config changed for gated arm: {gated_arm['name']}")
    candidate = read_json(stdout_path)
    validate_output(candidate, control, manifest["common"], manifest["source"]["immutable_source_archive_hash"])
    reference_path = ROOT / gate["reference"]["path"]
    if sha(reference_path) != gate["reference"]["sha256"]:
        raise ValueError("control_gate reference changed after preflight")
    reference = read_json(reference_path)
    if (candidate.get("result") != reference.get("result") or
            candidate.get("configurationFingerprint") != reference.get("configurationFingerprint") or
            candidate.get("abstractionFingerprint") != reference.get("abstractionFingerprint") or
            candidate.get("solverStateVersion") != 4 or reference.get("solverStateVersion") != 4):
        raise ValueError(f"control output differs from frozen reference for gated arm: {gated_arm['name']}")
    return {
        "control_arm": control["name"],
        "control_execution_record": control["execution_record"],
        "control_execution_record_sha256": sha(record_path),
        "control_stdout": control["stdout"],
        "control_stdout_sha256": expected["stdout_sha256"],
        "binary_sha256": expected["binary_sha256"],
        "config_sha256": expected["config_sha256"],
        "command": expected["command"],
        "reference": gate["reference"],
        "configurationFingerprint": candidate["configurationFingerprint"],
        "abstractionFingerprint": candidate["abstractionFingerprint"],
        "solverStateVersion": candidate["solverStateVersion"],
    }


def preflight(manifest: dict) -> tuple[Path, list[dict]]:
    if manifest.get("schema") != "solvers.multiway-algorithm-screen-plan/v1":
        raise ValueError("manifest schema mismatch")
    validate_control_gate(manifest)
    binary = ROOT / manifest["executable"]["path"]
    if sha(binary) != manifest["executable"]["sha256"]:
        raise ValueError("binary SHA-256 mismatch")
    common = manifest["common"]
    fixture = ROOT / manifest["source"]["base_fixture"]
    if sha(fixture) != manifest["source"]["base_fixture_sha256"]:
        raise ValueError("base fixture SHA-256 mismatch")
    fixture_rules = tomllib.loads(fixture.read_text(encoding="utf-8"))["game"]["tree"]["rules"]
    comparable = None
    for arm in manifest["arms"]:
        config = ROOT / arm["config"]
        if sha(config) != arm["config_sha256"]:
            raise ValueError(f"config SHA-256 mismatch: {arm['name']}")
        parsed = tomllib.loads(config.read_text(encoding="utf-8"))
        if parsed["solver"]["seed"] != common["seed"] or parsed["solver"]["batch_sweeps"] != arm["algorithm"]["batch_sweeps"]:
            raise ValueError(f"config algorithm mismatch: {arm['name']}")
        if parsed["solver"]["discount"] != arm["algorithm"]["discount"] or parsed["run"]["max_sweeps"] != common["sweeps"]:
            raise ValueError(f"config discount/sweeps mismatch: {arm['name']}")
        if parsed["run"]["resources"] != {"threads": common["threads"], "memory": common["memory"]}:
            raise ValueError(f"config resources mismatch: {arm['name']}")
        if parsed["game"]["tree"]["max_aggressive_actions"] != common["expected_tree"]["max_aggressive_actions"] or parsed["game"]["abstraction"]["buckets"] != common["expected_tree"]["buckets"]:
            raise ValueError(f"config tree mismatch: {arm['name']}")
        rules = parsed["game"]["tree"].get("rules")
        if rules != fixture_rules:
            raise ValueError(f"config tree.rules differ from frozen fixture: {arm['name']}")
        expected_tree = common["expected_tree"]
        if "allow_limp" in expected_tree and parsed["game"]["tree"].get("allow_limp") != expected_tree["allow_limp"]:
            raise ValueError(f"config allow_limp mismatch: {arm['name']}")
        if "limp_raise_size" in expected_tree:
            limp = [rule for rule in rules if rule.get("when") == "limpers > 0 && aggressions == 0" and rule.get("action") == "raise"]
            if len(limp) != 1 or limp[0].get("sizes", [None])[0] != expected_tree["limp_raise_size"]:
                raise ValueError(f"config limp rule mismatch: {arm['name']}")
        normalized = copy.deepcopy(parsed)
        normalized["solver"]["batch_sweeps"] = 0
        normalized["solver"]["discount"] = {}
        if comparable is None:
            comparable = normalized
        elif normalized != comparable:
            raise ValueError(f"arm settings differ outside batch/discount: {arm['name']}")
        if arm["argv"][0] != manifest["executable"]["path"] or arm["argv"][2] != arm["config"]:
            raise ValueError(f"command identity mismatch: {arm['name']}")
        if "research_abstraction" in arm:
            meta = arm["research_abstraction"]
            validate_research_schema({"schemaVersion": DRAW_RESEARCH_SCHEMA, "researchAbstraction": meta})
            if meta["effectiveBuckets"] != common["expected_tree"]["buckets"]:
                raise ValueError("research bucket budget differs from manifest")
            if arm["argv"].count("--abstraction") != 1:
                raise ValueError("research command requires one abstraction selector")
            selector = arm["argv"].index("--abstraction")
            if selector + 1 >= len(arm["argv"]) or arm["argv"][selector + 1] != meta["kind"]:
                raise ValueError("research command abstraction mismatch")
    return binary, manifest["arms"]


def run_arm(manifest_path: Path, manifest_sha256: str, manifest: dict, binary: Path, arm: dict) -> None:
    if sha(manifest_path) != manifest_sha256 or read_json(manifest_path) != manifest:
        raise ValueError("manifest changed after preflight")
    if sha(binary) != manifest["executable"]["sha256"] or sha(ROOT / arm["config"]) != arm["config_sha256"]:
        raise ValueError(f"binary/config changed after preflight: {arm['name']}")
    paths = {key: ROOT / arm[key] for key in ("stdout", "stderr", "execution_record")}
    partial = paths["stdout"].with_suffix(".json.partial")
    expected = {"command": arm["argv"], "binary_sha256": sha(binary), "config_sha256": arm["config_sha256"]}
    evidence = control_evidence(manifest, arm)
    if paths["execution_record"].is_file() and paths["stdout"].is_file() and paths["stderr"].is_file() and not partial.exists():
        record = read_json(paths["execution_record"])
        if (record.get("status") != "complete" or record.get("returncode") != 0 or
                record.get("stdout_sha256") != sha(paths["stdout"]) or
                any(record.get(k) != v for k, v in expected.items()) or
                (evidence is not None and record.get("control_gate_evidence") != evidence)):
            raise ValueError(f"completed record mismatch: {arm['name']}")
        validate_output(read_json(paths["stdout"]), arm, manifest["common"], manifest["source"]["immutable_source_archive_hash"])
        print(json.dumps({"arm": arm["name"], "status": "reused"}), flush=True)
        return
    if any(path.exists() for path in (*paths.values(), partial)):
        raise ValueError(f"partial or unrecorded prior attempt: {arm['name']}")
    record = {**expected, "status": "starting", "started_utc": utc()}
    if evidence is not None:
        record["control_gate_evidence"] = evidence
    durable_text(paths["execution_record"], json.dumps(record, indent=2) + "\n")
    started = time.monotonic()
    try:
        with partial.open("wb") as out, paths["stderr"].open("wb") as err:
            process = subprocess.Popen(arm["argv"], cwd=ROOT, stdout=out, stderr=err)
            record.update(status="running", pid=process.pid)
            durable_text(paths["execution_record"], json.dumps(record, indent=2) + "\n")
            try:
                returncode = process.wait(timeout=manifest["common"]["external_process_timeout_seconds"])
            except subprocess.TimeoutExpired:
                process.kill(); returncode = process.wait(); record["timed_out"] = True
    except Exception:
        record.update(status="failed", wall_seconds=time.monotonic() - started, finished_utc=utc())
        durable_text(paths["execution_record"], json.dumps(record, indent=2) + "\n")
        raise
    record.update(returncode=returncode, wall_seconds=time.monotonic() - started, finished_utc=utc())
    try:
        if returncode != 0 or record.get("timed_out"):
            raise RuntimeError("process failed or timed out")
        validate_output(read_json(partial), arm, manifest["common"], manifest["source"]["immutable_source_archive_hash"])
        os.replace(partial, paths["stdout"])
        record["stdout_sha256"] = sha(paths["stdout"])
        record["status"] = "complete"
    except Exception:
        record["status"] = "failed"
        durable_text(paths["execution_record"], json.dumps(record, indent=2) + "\n")
        raise
    durable_text(paths["execution_record"], json.dumps(record, indent=2) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--arms", help="comma-separated arm names; default is manifest order")
    parser.add_argument("--run", action="store_true", help="execute after preflight (default only checks)")
    args = parser.parse_args()
    manifest_path = args.manifest.resolve()
    manifest = read_json(manifest_path)
    manifest_sha256 = sha(manifest_path)
    binary, all_arms = preflight(manifest)
    selected = args.arms.split(",") if args.arms else [arm["name"] for arm in all_arms]
    if len(set(selected)) != len(selected) or any(name not in {arm["name"] for arm in all_arms} for name in selected):
        parser.error("--arms must be unique known arm names")
    arms = [next(arm for arm in all_arms if arm["name"] == name) for name in selected]
    print(json.dumps({"status": "preflight-passed", "arms": selected, "run": args.run}), flush=True)
    if args.run:
        for arm in arms:
            run_arm(manifest_path, manifest_sha256, manifest, binary, arm)


if __name__ == "__main__":
    main()
