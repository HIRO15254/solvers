#!/usr/bin/env python3
"""Prospectively freeze and apply a scoped external root-EV comparison policy.

Stdlib only. No solver/browser/network execution and no default thresholds.
Condition approval and numerical bounds are reviewed input evidence, not facts
inferred from close EVs. Missing evidence never becomes zero or a quality pass.
"""
from __future__ import annotations

import argparse
import datetime as dt
from decimal import Decimal, InvalidOperation, localcontext
import hashlib
import json
import math
from pathlib import Path
import re
import sys

SEATS = ("OOP", "IP")
CONDITIONS = {"variant_seats_units", "history_board_pot_stack", "both_ranges_card_removal",
              "full_continuation_tree", "rake_utility_settlement", "abstraction_recall",
              "ev_basis_conversion", "reference_version"}
CORRECTNESS = {"independent_rules_ev_br", "same_finite_game_br", "numeric_error_bound",
               "storage_roundtrip"}
MAX_JSON = 2 * 1024**2
MAX_FILE = 64 * 1024**2
SCOPE = "root seat EV and same-finite-game internal BR for this case/profile only"


class Invalid(ValueError):
    pass


class Missing(ValueError):
    pass


def need(condition, reason):
    if not condition:
        raise Missing(reason)


def valid(condition, reason):
    if not condition:
        raise Invalid(reason)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def validator_hash():
    return digest(Path(__file__).read_bytes())


def decode(raw):
    valid(len(raw) <= MAX_JSON, "JSON record exceeds 2 MiB")
    def pairs(items):
        result = {}
        for key, value in items:
            valid(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result
    def bad(value):
        raise Invalid("nonfinite JSON token: " + value)
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=pairs, parse_constant=bad)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise Invalid(str(error)) from error


def keys(value, required, optional=()):
    need(isinstance(value, dict), "missing object")
    absent = set(required) - value.keys()
    need(not absent, "missing fields: " + ",".join(sorted(absent)))
    valid(not (value.keys() - set(required) - set(optional)), "unsupported fields: " +
          ",".join(sorted(value.keys() - set(required) - set(optional))))


def number(value):
    need(value is not None, "missing numeric value")
    valid(isinstance(value, str) and len(value) <= 100, "numeric fields require decimal strings")
    try:
        result = Decimal(value)
    except InvalidOperation as error:
        raise Invalid("invalid decimal") from error
    valid(result.is_finite() and abs(result.adjusted()) <= 100, "nonfinite or excessive decimal exponent")
    return result


def evaluation_number(value):
    result = number(value)
    converted = float(value)
    valid(math.isfinite(converted), "metrics exceed f64 range")
    # Arithmetic and threshold comparisons must describe the same f64 value.
    # Decimal-only tails must not turn an equality into a strict-limit pass.
    valid(result == Decimal(str(converted)), "evaluation metric is not a canonical f64 round-trip value")
    return result


def nonempty_text(value, name):
    need(value is not None, name + " missing")
    valid(isinstance(value, str), name + " must be a string")
    need(bool(value.strip()), name + " empty")


def stamp(value):
    need(value is not None, "missing timestamp")
    try:
        result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except (ValueError, AttributeError) as error:
        raise Invalid("invalid timestamp") from error
    valid(result.tzinfo is not None, "timestamp needs timezone")
    return result


def sha_value(value):
    need(value is not None, "missing identity")
    valid(isinstance(value, str) and re.fullmatch("[0-9a-f]{64}", value), "invalid SHA-256")
    return value


class Evidence:
    def __init__(self, root):
        self.root = Path(root).resolve()
        self.verified = {}

    def raw(self, ref):
        keys(ref, {"path", "bytes", "sha256"})
        valid(isinstance(ref["path"], str) and not Path(ref["path"]).is_absolute(), "evidence path must be relative")
        path = (self.root / ref["path"]).resolve()
        valid(path.is_relative_to(self.root), "evidence escapes root")
        valid(type(ref["bytes"]) is int and 0 <= ref["bytes"] <= MAX_FILE, "evidence size unsupported")
        sha_value(ref["sha256"])
        need(path.is_file(), "evidence unavailable: " + ref["path"])
        valid(path.stat().st_size == ref["bytes"], "evidence size differs: " + ref["path"])
        with path.open("rb") as stream:
            raw = stream.read(MAX_FILE + 1)
        valid(len(raw) == ref["bytes"] and digest(raw) == ref["sha256"], "evidence hash/size differs: " + ref["path"])
        self.verified[ref["path"]] = ref
        return raw

    def json(self, ref):
        return decode(self.raw(ref))


def statuses(checks, required, evidence):
    keys(checks, required)
    for key in sorted(required):
        keys(checks[key], {"status", "evidence"})
        need(checks[key]["status"] == "pass", "unconfirmed check: " + key)
        evidence.raw(checks[key]["evidence"])


def condition_record(record, evidence):
    keys(record, {"schema", "case_id", "condition_match", "approved_at", "approval_evidence",
                  "missing_fields", "differences", "own_game", "reference_game", "checks", "scope"})
    valid(record["schema"] == "r1.external-conditions/v1", "unsupported condition schema")
    nonempty_text(record["case_id"], "condition case_id")
    need(record["condition_match"] == "confirmed" and record["missing_fields"] == []
         and record["differences"] == [], "finite-game conditions incomplete or different")
    stamp(record["approved_at"])
    evidence.raw(record["approval_evidence"])
    evidence.raw(record["own_game"]); evidence.raw(record["reference_game"])
    need(record["own_game"]["sha256"] == record["reference_game"]["sha256"], "canonical finite-game identities differ")
    statuses(record["checks"], CONDITIONS, evidence)
    scope = record["scope"]
    keys(scope, {"variant", "seats", "utility_unit", "ev_basis", "starting_pot", "chips_per_bb",
                 "economics", "constant_utility_sum"})
    valid(scope["variant"] == "NLHE" and scope["seats"] == list(SEATS), "unsupported game/seat order")
    valid(scope["utility_unit"] in {"chips", "prize"} and scope["ev_basis"] == "subgame_start_utility", "unsupported utility/basis")
    valid(number(scope["starting_pot"]) > 0 and number(scope["chips_per_bb"]) > 0, "invalid pot/conversion")
    valid(scope["economics"] in {"constant_sum", "general_sum"}, "unknown economics")
    if scope["economics"] == "constant_sum":
        number(scope["constant_utility_sum"])
    else:
        valid(scope["constant_utility_sum"] is None, "general-sum cannot assert constant payoff")
    return scope


def metric_record(record, conditions, role, evidence):
    keys(record, {"schema", "case_id", "role", "run_status", "started_at", "ended_at", "source",
                  "binary", "config", "numeric_model", "game_sha256", "utility_unit", "ev_basis", "strategy_kind",
                  "joint_reach_mass", "ev", "br", "gains", "nash_conv", "ev_rounding"})
    valid(record["schema"] == "r1.external-evaluation/v1" and record["role"] == role, "wrong evaluation schema/role")
    nonempty_text(record["case_id"], "evaluation case_id")
    need(record["run_status"] == "completed", "evaluation did not complete")
    valid(stamp(record["started_at"]) < stamp(record["ended_at"]), "invalid evaluation interval")
    need(record["case_id"] == conditions["case_id"] and record["game_sha256"] == conditions["own_game"]["sha256"], "evaluation game/case differs")
    scope = conditions["scope"]
    need(record["utility_unit"] == scope["utility_unit"] and record["ev_basis"] == scope["ev_basis"], "evaluation units/basis differ")
    valid(record["strategy_kind"] in {"live_average", "stored_quantized"}, "metadata is not evaluated policy")
    for name in ("source", "binary", "config", "numeric_model"):
        evidence.raw(record[name])
    valid(number(record["joint_reach_mass"]) >= 0, "negative reach")
    for field in ("ev", "br", "gains", "ev_rounding"):
        keys(record[field], set(SEATS))
    for field in ("ev", "br", "gains"):
        for seat in SEATS:
            evaluation_number(record[field][seat])
    evaluation_number(record["nash_conv"])
    # Production reports use JSON round-trip f64. Reproduce that arithmetic,
    # rather than demanding Decimal subtraction equality of rounded operands.
    floats = {field: [float(record[field][seat]) for seat in SEATS] for field in ("ev", "br", "gains")}
    valid(all(math.isfinite(x) for values in floats.values() for x in values), "metrics exceed f64 range")
    valid(floats["gains"] == [b - e for b, e in zip(floats["br"], floats["ev"])]
          and float(record["nash_conv"]) == sum(floats["gains"]), "gain/NC arithmetic differs")
    return record


def interval(value, rounding):
    value = number(value)
    need(isinstance(rounding, dict), "missing display precision")
    if rounding.get("mode") == "nearest":
        keys(rounding, {"mode", "quantum"})
        q = number(rounding["quantum"])
        valid(q >= 0, "negative rounding quantum")
        return value - q / 2, value + q / 2
    if rounding.get("mode") == "explicit_interval":
        keys(rounding, {"mode", "lower", "upper"})
        lower, upper = number(rounding["lower"]), number(rounding["upper"])
        valid(lower <= value <= upper, "display outside rounding interval")
        return lower, upper
    raise Missing("unknown rounding rule; provide documented explicit interval")


def reference_record(record, conditions, evidence):
    keys(record, {"schema", "case_id", "game_sha256", "utility_unit", "ev_basis", "observed_at",
                  "version_evidence", "solution_precision", "joint_reach_mass", "ev", "ev_rounding"})
    valid(record["schema"] == "r1.external-reference/v1", "unsupported reference schema")
    nonempty_text(record["case_id"], "reference case_id")
    need(record["case_id"] == conditions["case_id"] and record["game_sha256"] == conditions["reference_game"]["sha256"], "reference game/case differs")
    need(record["utility_unit"] == conditions["scope"]["utility_unit"] and record["ev_basis"] == conditions["scope"]["ev_basis"], "reference units/basis differ")
    stamp(record["observed_at"]); evidence.raw(record["version_evidence"])
    precision = record["solution_precision"]
    keys(precision, {"status", "description", "evidence"})
    need(precision["status"] == "documented", "individual solution precision unknown")
    nonempty_text(precision["description"], "solution precision description")
    evidence.raw(precision["evidence"])
    valid(number(record["joint_reach_mass"]) >= 0, "negative reference reach")
    keys(record["ev"], set(SEATS)); keys(record["ev_rounding"], set(SEATS))
    for seat in SEATS:
        interval(record["ev"][seat], record["ev_rounding"][seat])


def limit_record(value, unit):
    keys(value, {"operator", "value", "unit"})
    valid(value["operator"] in {"<", "<="} and value["unit"] == unit, "unsupported threshold operator/unit")
    valid(number(value["value"]) >= 0, "negative threshold")


def comparison_pass(value, limit):
    boundary = number(limit["value"])
    return value < boundary if limit["operator"] == "<" else value <= boundary


def numeric_consistency(record, scope, bounds):
    need(all(number(record["gains"][seat]) >= -number(bounds[seat]) for seat in SEATS),
         "negative gain beyond independently supplied numeric bound")
    if scope["economics"] == "constant_sum":
        # Preserve asymmetric display intervals. A symmetric radius would
        # admit a certified sum outside every possible displayed EV sum.
        intervals = [interval(record["ev"][seat], record["ev_rounding"][seat]) for seat in SEATS]
        error = sum(number(bounds[seat]) for seat in SEATS)
        total = number(scope["constant_utility_sum"])
        need(sum(x[0] for x in intervals) - error <= total <= sum(x[1] for x in intervals) + error,
             "reported EV sum contradicts constant-sum certificate")


def inspect_calibration(calibration, evidence):
    keys(calibration, {"schema", "case_id", "basis", "candidate_results_used", "inputs", "strategy_kind",
                       "root_ev_margins", "internal_quality", "threshold_rationale", "threshold_rationale_evidence"})
    valid(calibration["schema"] == "r1.external-calibration/v1", "unsupported calibration schema")
    nonempty_text(calibration["case_id"], "calibration case_id")
    need(calibration["basis"] == "baseline_only" and calibration["candidate_results_used"] is False, "candidate-informed calibration is not prospective")
    nonempty_text(calibration["threshold_rationale"], "threshold rationale")
    evidence.raw(calibration["threshold_rationale_evidence"])
    keys(calibration["inputs"], {"conditions", "reference", "baseline", "correctness"})
    records = {name: evidence.json(ref) for name, ref in calibration["inputs"].items()}
    cond, ref, base, correct = (records[k] for k in ("conditions", "reference", "baseline", "correctness"))
    scope = condition_record(cond, evidence)
    need(calibration["case_id"] == cond["case_id"], "calibration case differs")
    reference_record(ref, cond, evidence)
    metric_record(base, cond, "baseline", evidence)
    need(number(base["joint_reach_mass"]) > 0, "zero baseline joint reach cannot calibrate root EV")
    need(base["strategy_kind"] == calibration["strategy_kind"], "baseline profile differs")
    keys(correct, {"schema", "case_id", "status", "completed_at", "baseline_record_sha256",
                   "game_sha256", "source_sha256", "numeric_model_sha256", "checks", "numeric_error_upper_bound", "bound_evidence"})
    valid(correct["schema"] == "r1.external-correctness/v1", "unsupported correctness schema")
    nonempty_text(correct["case_id"], "correctness case_id")
    need(correct["status"] == "pass" and correct["case_id"] == cond["case_id"], "baseline correctness unavailable")
    need(correct["baseline_record_sha256"] == calibration["inputs"]["baseline"]["sha256"]
         and correct["game_sha256"] == cond["own_game"]["sha256"]
         and correct["source_sha256"] == base["source"]["sha256"]
         and correct["numeric_model_sha256"] == base["numeric_model"]["sha256"], "baseline correctness identity differs")
    statuses(correct["checks"], CORRECTNESS, evidence)
    keys(correct["numeric_error_upper_bound"], set(SEATS))
    evidence.raw(correct["bound_evidence"])
    for seat in SEATS:
        valid(number(correct["numeric_error_upper_bound"][seat]) >= 0, "negative numeric error bound")
        interval(base["ev"][seat], base["ev_rounding"][seat])
    numeric_consistency(base, scope, correct["numeric_error_upper_bound"])
    valid(stamp(base["ended_at"]) <= stamp(correct["completed_at"]), "correctness predates baseline completion")
    keys(calibration["root_ev_margins"], set(SEATS))
    for margin in calibration["root_ev_margins"].values():
        limit_record(margin, scope["utility_unit"])
    quality = calibration["internal_quality"]
    keys(quality, {"nash_conv", "seat_gains"})
    limit_record(quality["nash_conv"], scope["utility_unit"])
    if scope["economics"] == "general_sum":
        keys(quality["seat_gains"], set(SEATS))
        for limit in quality["seat_gains"].values():
            limit_record(limit, scope["utility_unit"])
    else:
        valid(quality["seat_gains"] is None, "constant-sum criterion uses the explicit NC limit")
    return records


def withheld(reason, *, invalid=False):
    return {"schema": "r1.external-comparison-result/v1", "state": "invalid" if invalid else "not_evaluated",
            "quality_status": "not_evaluated", "overall_r1_acceptance": "not_evaluated",
            "missing_or_invalid": [reason], "checks": {}, "ev_diff": None,
            "external_frequency_diagnostic": "not_evaluated"}


def _freeze(calibration, evidence, now=None):
    now = now or dt.datetime.now(dt.timezone.utc)
    valid(isinstance(now, dt.datetime) and now.tzinfo is not None, "clock needs timezone")
    records = inspect_calibration(calibration, evidence)
    need(now > max(stamp(records["conditions"]["approved_at"]), stamp(records["correctness"]["completed_at"]),
                   stamp(records["reference"]["observed_at"])), "calibration evidence postdates publication")
    result = {"schema": "r1.external-threshold/v1",
            "issued_at": now.isoformat(), "validator_sha256": validator_hash(),
            "calibration": calibration, "calibration_sha256": digest(encode(calibration)),
            "scope": SCOPE,
            "metric": "absolute_raw_root_ev_difference", "frequency_policy": "diagnostic_only",
            "overall_r1_acceptance": "not_evaluated"}
    result["threshold_version"] = "sha256:" + digest(encode(result))
    return result


def _compare(threshold, candidate, evidence, now=None):
    now = now or dt.datetime.now(dt.timezone.utc)
    valid(isinstance(now, dt.datetime) and now.tzinfo is not None, "clock needs timezone")
    need(threshold is not None, "prospective threshold_version unavailable")
    keys(threshold, {"schema", "threshold_version", "issued_at", "validator_sha256", "calibration",
                     "calibration_sha256", "scope", "metric", "frequency_policy", "overall_r1_acceptance"}, {"verified_evidence"})
    valid(threshold["schema"] == "r1.external-threshold/v1", "unsupported threshold schema")
    valid(threshold["validator_sha256"] == validator_hash(), "threshold validator hash differs")
    identity = digest(encode(threshold["calibration"]))
    core = {key: value for key, value in threshold.items() if key not in {"threshold_version", "verified_evidence"}}
    valid(threshold["calibration_sha256"] == identity and threshold["threshold_version"] == "sha256:" + digest(encode(core)), "threshold/calibration changed")
    valid(threshold["metric"] == "absolute_raw_root_ev_difference" and threshold["frequency_policy"] == "diagnostic_only"
          and threshold["overall_r1_acceptance"] == "not_evaluated" and threshold["scope"] == SCOPE, "unsupported acceptance scope")
    cal = threshold["calibration"]
    records = inspect_calibration(cal, evidence)
    cond, ref, base, correct = (records[k] for k in ("conditions", "reference", "baseline", "correctness"))
    issued = stamp(threshold["issued_at"])
    need(issued <= now, "threshold publication is in the future")
    need(issued > max(stamp(cond["approved_at"]), stamp(correct["completed_at"]), stamp(ref["observed_at"])), "threshold predates calibration evidence")
    metric_record(candidate, cond, "candidate", evidence)
    need(stamp(candidate["ended_at"]) <= now, "candidate completion is in the future")
    need(issued < stamp(candidate["started_at"]), "threshold was not issued before candidate comparison")
    need(candidate["config"]["sha256"] == base["config"]["sha256"], "candidate normalized config differs")
    need(candidate["numeric_model"]["sha256"] == base["numeric_model"]["sha256"], "candidate numerical model requires its own bound calibration")
    need(candidate["strategy_kind"] == cal["strategy_kind"], "candidate profile kind differs")
    for record in (ref, candidate):
        if number(record["joint_reach_mass"]) == 0:
            result = withheld("zero joint reach: root external EV is not applicable")
            result["compared_at"] = now.isoformat()
            result["checks"]["external_ev"] = "not_applicable"
            return result
    differences, seat_passes = {}, []
    scope = cond["scope"]
    for seat in SEATS:
        own, reference = number(candidate["ev"][seat]), number(ref["ev"][seat])
        own_interval = interval(candidate["ev"][seat], candidate["ev_rounding"][seat])
        ref_interval = interval(ref["ev"][seat], ref["ev_rounding"][seat])
        difference = own - reference
        gap = max(Decimal(0), own_interval[0] - ref_interval[1], ref_interval[0] - own_interval[1])
        passed = comparison_pass(abs(difference), cal["root_ev_margins"][seat])
        seat_passes.append(passed)
        differences[seat] = {"signed": str(difference), "absolute": str(abs(difference)),
            "rounding_adjusted_lower_bound": str(gap), "unit": scope["utility_unit"],
            "absolute_pct_starting_pot": str(100 * abs(difference) / number(scope["starting_pot"])) if scope["utility_unit"] == "chips" else None,
            "own_interval": [str(x) for x in own_interval], "reference_interval": [str(x) for x in ref_interval],
            "numeric_error_upper_bound_separate": correct["numeric_error_upper_bound"][seat],
            "status": "pass" if passed else "fail"}
    gains = {seat: number(candidate["gains"][seat]) for seat in SEATS}
    numeric_consistency(candidate, scope, correct["numeric_error_upper_bound"])
    nc = number(candidate["nash_conv"])
    internal = comparison_pass(nc, cal["internal_quality"]["nash_conv"])
    if scope["economics"] == "general_sum":
        internal = internal and all(comparison_pass(gains[seat], cal["internal_quality"]["seat_gains"][seat]) for seat in SEATS)
    exploit = nc / 2 if scope["economics"] == "constant_sum" else None
    return {"schema": "r1.external-comparison-result/v1", "state": "evaluated",
            "case_id": cond["case_id"], "threshold_version": threshold["threshold_version"],
            "candidate_canonical_record_sha256": digest(encode(candidate)),
            "threshold_canonical_record_sha256": digest(encode(threshold)),
            "issued_at": threshold["issued_at"], "candidate_started_at": candidate["started_at"],
            "compared_at": now.isoformat(),
            "quality_status": "pass" if internal and all(seat_passes) else "fail",
            "quality_scope": threshold["scope"], "overall_r1_acceptance": "not_evaluated",
            "checks": {"reference_conditions": "pass", "baseline_correctness": "pass", "internal_br": "pass" if internal else "fail",
                       "external_ev": "pass" if all(seat_passes) else "fail"},
            "ev_diff": differences, "g_i": candidate["gains"], "nash_conv": candidate["nash_conv"],
            "exploitability": str(exploit) if exploit is not None else None,
            "exploitability_pct_pot": str(100 * exploit / number(scope["starting_pot"])) if exploit is not None and scope["utility_unit"] == "chips" else None,
            "external_frequency_diagnostic": "not_evaluated", "missing_or_invalid": []}


def encode(value):
    return (json.dumps(value, ensure_ascii=False, allow_nan=False, sort_keys=True, indent=2) + "\n").encode()


def read_json(path):
    with path.open("rb") as stream:
        return decode(stream.read(MAX_JSON + 1))


def freeze(calibration, evidence, now=None):
    with localcontext() as context:
        context.prec = 500
        return _freeze(calibration, evidence, now)


def compare(threshold, candidate, evidence, now=None):
    with localcontext() as context:
        context.prec = 500
        return _compare(threshold, candidate, evidence, now)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("freeze", "compare"))
    parser.add_argument("--evidence-root", type=Path, required=True)
    parser.add_argument("--calibration", type=Path)
    parser.add_argument("--threshold", type=Path)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    evidence = Evidence(args.evidence_root)
    compared_at = dt.datetime.now(dt.timezone.utc) if args.mode == "compare" else None
    try:
        with localcontext() as context:
            context.prec = 120
            if args.mode == "freeze":
                valid(args.calibration is not None and args.threshold is None and args.candidate is None, "freeze needs only --calibration")
                result = freeze(read_json(args.calibration), evidence)
            else:
                valid(args.candidate is not None and args.calibration is None, "compare needs --candidate and optional --threshold")
                result = compare(read_json(args.threshold) if args.threshold else None,
                                 read_json(args.candidate), evidence, compared_at)
        exit_code = 0 if result.get("quality_status", "pass") == "pass" else 1
    except Missing as error:
        result, exit_code = withheld(str(error)), 1
    except (Invalid, OSError, KeyError, TypeError, ArithmeticError) as error:
        result, exit_code = withheld(str(error), invalid=True), 2
    result["verified_evidence"] = list(evidence.verified.values())
    if compared_at is not None:
        result["compared_at"] = compared_at.isoformat()
    result["validator_sha256"] = validator_hash()
    try:
        with args.out.open("xb") as stream:
            stream.write(encode(result))
    except OSError as error:
        print(f"cannot publish new record: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"state": result.get("state", "threshold_issued"), "quality_status": result.get("quality_status", "not_evaluated"), "out": str(args.out)}))
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
