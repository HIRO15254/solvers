#!/usr/bin/env python3
"""Validate a retrospective criterion index, without issuing R1 acceptance.

Only hashes small Git-backed evidence and checks explicit gates. It does not
execute retained code, replay solvers, or replace the raw-bundle validators.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import sys
import tomllib

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def document(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def invalid(value):
        raise ValueError("nonfinite JSON constant: " + value)
    return json.loads(raw.decode("utf-8"), object_pairs_hook=pairs, parse_constant=invalid)


def instant(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "timestamp needs timezone")
    return result


def read_reference(item):
    path = (REPO / item["path"]).resolve()
    require(path.is_relative_to(REPO.resolve()), "reference escapes repository")
    return path.read_bytes()


def validate_index(index, reader=read_reference):
    require(index["schema"] == "r1.retrospective-criterion-index/v1", "wrong schema")
    require(index["publication_kind"] == "retrospective_evidence_index"
            and index["new_thresholds_issued"] is False, "index cannot issue retroactive thresholds")
    require(index["formal_overall_quality_status"] == "not_evaluated"
            and index["r1_complete"] is False, "overall acceptance is unsupported")
    refs = {}
    for key, item in index["references"].items():
        raw = reader(item)
        require(len(raw) == item["bytes"] and sha(raw) == item["sha256"], "reference changed: " + key)
        refs[key] = raw
    require(sha(Path(__file__).read_bytes()) == index["validator_sha256"], "validator differs from published index")
    docs = {key: document(refs[key]) for key in (
        "source02_manifest", "first_pilot", "first_plan", "first_comparison",
        "source03_plan", "current_plan", "current_comparison", "current_report",
        "audit_plan", "audits", "source06_manifest", "source06_test_summary", "source06_test_record")}
    pilot, plan, comparison = (docs[key] for key in ("first_pilot", "first_plan", "first_comparison"))
    require(instant(pilot["created_at"]) < instant(plan["created_at"]) < instant(comparison["created_at"]), "pilot/freeze/comparison order changed")
    require(index["first_history"] == {"pilot_created_at": pilot["created_at"],
        "plan_created_at": plan["created_at"], "comparison_created_at": comparison["created_at"]}, "historical dates differ")
    require(instant(index["published_at_utc"]) > instant(docs["audits"]["ended_at"]), "publication backdated before evidence")
    manifest = {item["path"]: item for item in docs["source02_manifest"]["files"]}
    require(docs["source02_manifest"]["archive_sha256"] == index["source02_archive"]["sha256"], "snapshot archive mismatch")
    for item in index["source02_members"]:
        raw = refs[item["reference"]]
        original = manifest[item["archive_member"]]
        require(len(raw) == original["bytes"] and sha(raw) == original["sha256"], "snapshot member differs")
    require(sha(refs["first_runner"]) == plan["runner"]["sha256"] == pilot["runner"]["sha256"], "predeclared validator differs")
    require(b'live_final["nash_conv"] < target' in refs["first_runner"], "strict target validator missing")
    require(b"NashConv < 0.04" in refs["first_readme"], "predeclared target text missing")
    expected = [("river", 100, .04), ("turn", 100, .04), ("flop", 50, .04)]
    for name in ("first_plan", "source03_plan", "current_plan"):
        value = docs[name]
        require([(x["case"], x["iterations"], x["target_nash_conv"]) for x in value["cases"]] == expected, "fixed cases changed")
        require(value["order"] == ["baseline", "candidate"] * 3 and value["repetitions_per_binary"] == 3, "fixed order changed")
        require(value["resume_scope"] == "restore completed iteration cap; no additional CFR iterations", "resume scope broadened")
        require(value["profile_equality"] == plan["profile_equality"] and value["summary_equality_fields"] == plan["summary_equality_fields"], "equality checks changed")
    old, current = docs["source03_plan"], docs["current_plan"]
    for field in set(old) - {"candidate", "cases", "created_at"}:
        require(old[field] == current[field], "current fixed condition differs: " + field)
    for a, b in zip(old["cases"], current["cases"]):
        require(a["config"]["sha256"] == b["config"]["sha256"], "current input bytes changed")
    for case, _, target in expected:
        cfg = tomllib.loads(refs["first_config_" + case].decode())
        require(cfg["run"]["target_nash_conv"] == target and cfg["run"]["threads"] == 8
                and cfg["game"]["pot"] == 20 and cfg["game"]["effective_stack"] == 60, "pilot config differs")
    criterion = index["synthetic_criterion"]
    require(criterion["metric"] == "nash_conv" and criterion["unit"] == "chips"
            and criterion["operator"] == "<" and criterion["value"] == .04
            and criterion["starting_pot_chips"] == 20
            and criterion["external_acceptance"] is False, "criterion broadened")
    require(index["performance_acceptance_threshold"] is None and index["external_threshold_version"] is None, "posthoc/unavailable threshold filled")
    report = docs["current_report"]
    require(report["status"] == "verified_scoped_comparison" and report["formal_external_24_case_acceptance"] == "not_evaluated", "report scope changed")
    require(report["verifier"]["sha256"] == sha(refs["current_verifier"]), "upstream report validator changed")
    require(report["coverage"] == {"original_solves": 18, "exact_export_pairs": 9, "resume_checks": 6,
            "saved_profile_audits": 18, "exact_saved_profile_pairs": 9}, "scoped coverage differs")
    for name, reference in (("plan", "current_plan"), ("comparison", "current_comparison"),
                            ("audit_plan", "audit_plan"), ("audits", "audits")):
        require(report["evidence_roots"][name]["sha256"] == sha(refs[reference]), "upstream report input differs")
    require(docs["current_comparison"]["state"] == docs["audits"]["state"] == "completed", "incomplete campaign")
    require(instant(docs["audit_plan"]["created_at"]) <= instant(docs["audits"]["started_at"]), "audit criteria issued after audit")
    require(docs["audit_plan"]["pair_tolerance"]["absolute"] == 1e-10
            and docs["audit_plan"]["pair_tolerance"]["relative"] == 1e-12, "audit tolerance changed")
    for case in report["cases"]:
        require(case["target_nash_conv"] == .04 and case["saved_profile"]["nash_conv"] < .04
                and len(case["pairs"]) == 3 and all(pair["saved_profile_exact"] and pair["exported_tree_strategy_ev_exact"] for pair in case["pairs"]), "scoped quality check differs")
    record = docs["source06_test_record"]
    require(record["state"] == "completed" and record["child_exit_code"] == record["supervisor_exit_code"] == 0
            and record["cleanup_complete"] is True and record["identity_unchanged"] is True, "workspace test was not successful")
    require(record["outputs"]["stdout"]["sha256"] == sha(refs["source06_test_stdout"]), "test output differs")
    require(docs["source06_test_summary"]["source06_archive_sha256"] == current["candidate"]["source_evidence"]["sha256"], "test/product source differs")
    require(docs["source06_manifest"]["archive_sha256"] == current["candidate"]["source_evidence"]["sha256"], "test source manifest differs")
    test_sources = {item["path"]: item for item in docs["source06_manifest"]["files"]}
    for key in ("variants_test_source", "hu_resume_test_source", "checkpoint_wire_test_source"):
        require(test_sources[index["references"][key]["path"]]["sha256"] == sha(refs[key]), "test source not in executed snapshot")
    test_log = refs["source06_test_stdout"].decode()
    for name in index["required_regression_tests"]:
        require(f"test {name} ... ok" in test_log, "regression evidence missing: " + name)
    required_missing = {"external_fixture_condition_match", "external_threshold_calibration",
                        "external_suite_quality", "formal_t1_06_certification"}
    gates = {gate["id"]: gate for gate in index["acceptance_gates"]}
    require(len(gates) == len(index["acceptance_gates"]), "duplicate acceptance gate")
    require(required_missing <= gates.keys() and all(gates[key]["status"] == "not_evaluated" for key in required_missing), "missing acceptance evidence promoted")
    require({row["requirement"] for row in index["requirement_evidence"]} == {f"F1-0{x}" for x in range(1, 8)}, "requirement inventory incomplete")
    for row in index["requirement_evidence"]:
        require(row["evidence"] and all(key in refs for key in row["evidence"]), "unbound requirement evidence")
    # Future attachments remain evidence only. They cannot silently change this
    # historical index's scope or fill any gate: issue a new reviewed version.
    for item in index["future_external_records"]:
        raw = reader(item)
        require(len(raw) == item["bytes"] and sha(raw) == item["sha256"], "future external reference changed")
        document(raw)
    return {"schema": "r1.acceptance-index-validation/v1", "status": "index_consistent",
            "formal_overall_quality_status": "not_evaluated", "r1_complete": False,
            "references_verified": len(refs), "historical_internal_criterion_predeclared": True,
            "saved_profile_pairs_exact": 9, "continued_hu_regression": "raked River F32 3 to 6 iterations; no signal/interruption equivalence claim",
            "missing_acceptance_gates": sorted(required_missing),
            "future_attachments_verified": len(index["future_external_records"]),
            "scope": "hashes and scoped gates only; does not replay raw-bundle or solver verification"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", type=Path, default=HERE / "criterion-index.json")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    try:
        raw = args.index.read_bytes()
        result = validate_index(document(raw))
        result["index_sha256"] = sha(raw)
        result["validator_sha256"] = sha(Path(__file__).read_bytes())
        encoded = json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
        if args.out:
            with args.out.open("x", encoding="utf-8", newline="\n") as stream:
                stream.write(encoded)
        else:
            print(encoded, end="")
        return 0
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"acceptance index invalid: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
