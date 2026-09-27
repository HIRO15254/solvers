"""Exact arithmetic diagnostics over a declared common reference reach measure.

No policy reconstruction, solver, default quality threshold, or external pass.
One record compares one aligned node/actor. Evidence hashes bind input bytes,
not the truth of a claimed derivation or finite-game identity.
"""
from __future__ import annotations

import argparse
from decimal import Decimal, InvalidOperation
from fractions import Fraction as Q
import hashlib
import json
from pathlib import Path
import re
import sys

SCHEMA = "r1.frequency-diagnostic-input/v1"
MAX_BYTES = 2 * 1024**2
MAX_HANDS = 1326
MAX_ACTIONS = 128
SEATS = {"OOP", "IP"}


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def fields(value, expected, name):
    require(isinstance(value, dict) and set(value) == set(expected), f"{name}: fields differ")


def decode(raw):
    require(len(raw) <= MAX_BYTES, "input exceeds 2 MiB")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result
    def bad(value):
        raise ValueError("nonfinite JSON token: " + value)
    return json.loads(raw.decode("utf-8-sig"), object_pairs_hook=unique, parse_constant=bad)


def text(value, name):
    require(isinstance(value, str) and 0 < len(value) <= 512 and bool(value.strip()), name + ": missing text")
    return value


def sha(value):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value), "invalid SHA-256")
    return value


def number(value):
    """Decimal or exact fraction; null is missing, never an implicit zero."""
    if value is None:
        return None
    if isinstance(value, dict):
        fields(value, {"numerator", "denominator"}, "rational")
        require(all(isinstance(v, str) and re.fullmatch(r"-?\d{1,100}", v) for v in value.values()), "invalid rational integers")
        denominator = int(value["denominator"])
        require(denominator > 0, "nonpositive denominator")
        return Q(int(value["numerator"]), denominator)
    require(isinstance(value, str) and len(value) <= 100 and re.fullmatch(r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?", value), "expected decimal string or rational")
    try:
        decimal = Decimal(value)
    except InvalidOperation as error:
        raise ValueError("invalid decimal") from error
    require(decimal.is_finite() and abs(decimal.as_tuple().exponent) <= 400, "nonfinite or excessive exponent")
    return Q(decimal)


def nonnegative(value):
    result = number(value)
    require(result is None or result >= 0, "negative reach/weight")
    return result


def ratio(value):
    if value is None:
        return None
    return {"numerator": str(value.numerator), "denominator": str(value.denominator)}


def file_pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def verify_evidence(reference, root, verified):
    if reference is None:
        return False
    fields(reference, {"path", "bytes", "sha256"}, "evidence")
    name = text(reference["path"], "evidence path")
    require(not Path(name).is_absolute() and not re.match(r"^[A-Za-z]:", name), "absolute evidence path")
    path = (root / name).resolve()
    require(path.is_relative_to(root) and path.is_file(), "missing or escaping evidence")
    require(type(reference["bytes"]) is int and 0 <= reference["bytes"] <= MAX_BYTES, "evidence size unsupported")
    sha(reference["sha256"])
    require(path.stat().st_size == reference["bytes"], "evidence length differs")
    with path.open("rb") as stream:
        raw = stream.read(MAX_BYTES + 1)
    require(file_pin(raw) == {k: reference[k] for k in ("bytes", "sha256")}, "evidence bytes differ")
    require(name not in verified or verified[name] == reference, "conflicting evidence identity")
    verified[name] = reference
    return True


def mapping(value, support, name):
    require(isinstance(value, dict), name + ": expected mapping")
    allowed = {str(h) for h in support}
    require(set(value) <= allowed, name + ": unknown/noncanonical combo ID")
    return value


def rounding(value):
    if value is None:
        return None
    fields(value, {"mode", "quantum"}, "rounding")
    require(value["mode"] == "nearest", "only declared nearest rounding is supported")
    quantum = nonnegative(value["quantum"])
    require(quantum is not None and quantum <= 1, "invalid probability quantum")
    return quantum / 2


def profile(value, support):
    fields(value, {"game_sha256", "node_id", "actor", "strategy_kind", "actions", "reach", "policy", "rounding", "evidence"}, "profile")
    if value["game_sha256"] is not None:
        sha(value["game_sha256"])
    text(value["node_id"], "node_id")
    require(isinstance(value["actor"], str) and value["actor"] in SEATS, "invalid actor")
    require(isinstance(value["strategy_kind"], str) and value["strategy_kind"] in {
        "live_average", "resumed_live_average", "stored_quantized", "reference_profile", "synthetic"
    }, "unsupported strategy kind")
    actions = value["actions"]
    require(isinstance(actions, list) and 0 < len(actions) <= MAX_ACTIONS, "invalid action count")
    for action in actions:
        text(action, "canonical action ID")
    require(len(actions) == len(set(actions)), "duplicate action ID")
    raw_reach = mapping(value["reach"], support, "reach")
    raw_policy = mapping(value["policy"], support, "policy")
    reach, policy, missing, sums = {}, {}, {}, {}
    for hand in support:
        key = str(hand)
        r = raw_reach.get(key)
        if r is not None:
            fields(r, {"actor", "compatible_opponent"}, "hand reach")
            reach[hand] = (nonnegative(r["actor"]), nonnegative(r["compatible_opponent"]))
        else:
            reach[hand] = (None, None)
        raw = raw_policy.get(key)
        if raw is None:
            raw = {}
        require(isinstance(raw, dict) and set(raw) <= set(actions), "unknown policy action")
        row = {a: number(raw.get(a)) for a in actions}
        require(all(p is None or 0 <= p <= 1 for p in row.values()), "probability outside [0,1]")
        missing[hand] = [a for a, p in row.items() if p is None]
        sums[hand] = sum(row.values(), Q(0)) if not missing[hand] else None
        # Rounding is not permission to renormalize an incomplete/nonstochastic row.
        require(sums[hand] is None or sums[hand] == 1, "complete policy row does not sum exactly to one")
        policy[hand] = row
    return {"actions": actions, "reach": reach, "policy": policy, "missing": missing,
            "row_sums": sums, "rounding_radius": rounding(value["rounding"])}


def diagnose(record, evidence_root):
    fields(record, {"schema", "case_id", "condition_match", "support", "support_evidence", "weight_evidence", "chance_weight", "w_ref", "profiles"}, "record")
    require(record["schema"] == SCHEMA, "unsupported schema")
    text(record["case_id"], "case_id")
    require(isinstance(record["condition_match"], str) and record["condition_match"] in {
        "confirmed", "unverified", "mismatch"
    }, "invalid condition_match")
    support = record["support"]
    require(isinstance(support, list) and 0 < len(support) <= MAX_HANDS, "invalid support count")
    require(all(type(h) is int and 0 <= h < 1326 for h in support), "invalid global combo ID")
    require(support == sorted(set(support)), "support must be unique global combo IDs in ascending order")
    fields(record["profiles"], {"own", "reference"}, "profiles")
    root, verified, blockers = Path(evidence_root).resolve(), {}, []
    for key in ("support_evidence", "weight_evidence"):
        if not verify_evidence(record[key], root, verified):
            blockers.append(key + " missing")
    raw_profiles = record["profiles"]
    parsed = {side: profile(raw_profiles[side], support) for side in ("own", "reference")}
    for side in parsed:
        if not verify_evidence(raw_profiles[side]["evidence"], root, verified):
            blockers.append(side + " policy evidence missing")
    own, ref = raw_profiles["own"], raw_profiles["reference"]
    for key in ("node_id", "actor"):
        if own[key] != ref[key]:
            blockers.append(key + " mismatch")
    if own["game_sha256"] is None or ref["game_sha256"] is None:
        blockers.append("game identity missing")
    elif own["game_sha256"] != ref["game_sha256"]:
        blockers.append("finite game identity mismatch")
    if record["condition_match"] == "mismatch":
        blockers.append("declared condition mismatch")
    actions_own, actions_ref = set(own["actions"]), set(ref["actions"])
    action_match = actions_own == actions_ref
    if not action_match:
        blockers.append("legal action sets differ")
    actions = sorted(actions_ref)
    chance = mapping(record["chance_weight"], support, "chance_weight")
    weights = mapping(record["w_ref"], support, "w_ref")
    mass, own_mass, missing_mass, missing_own, zero, rows = Q(0), Q(0), [], [], [], []
    comparable_mass, unavailable_mass, candidates = Q(0), Q(0), []
    weighted_tv = Q(0)
    weighted_lower = Q(0)
    frequencies = {a: [Q(0), Q(0)] for a in actions}
    radius_own, radius_ref = (parsed[s]["rounding_radius"] for s in ("own", "reference"))
    radius = None if radius_own is None or radius_ref is None else radius_own + radius_ref
    positive_count = 0
    for hand in support:
        key = str(hand)
        q, w = nonnegative(chance.get(key)), nonnegative(weights.get(key))
        require(q is None or q <= 1, "chance weight outside [0,1]")
        own_reach, ref_reach = parsed["own"]["reach"][hand], parsed["reference"]["reach"][hand]
        complete_ref = q is not None and w is not None and all(x is not None for x in ref_reach)
        if complete_ref:
            require(w == ref_reach[0] * ref_reach[1] * q, "w_ref is not the declared compatible joint mass")
            mass += w
        else:
            missing_mass.append(hand)
        own_w = None if q is None or any(x is None for x in own_reach) else own_reach[0] * own_reach[1] * q
        if own_w is None:
            missing_own.append(hand)
        else:
            own_mass += own_w
        missing_policy = {s: parsed[s]["missing"][hand] for s in parsed}
        row = {"combo_id": hand, "w_ref": ratio(w), "own_joint_mass": ratio(own_w),
               "own_zero_reach": None if own_w is None else own_w == 0,
               "policy_missing_actions": missing_policy, "status": "not_evaluated", "tv_pp": None}
        if not complete_ref:
            row["reason"] = "reference reach or chance weight missing"
        elif w == 0:
            zero.append(hand)
            row.update(status="not_applicable", reason="zero compatible reference mass")
        else:
            positive_count += 1
            if not action_match or any(missing_policy.values()):
                unavailable_mass += w
                row["reason"] = "action mismatch or missing positive-mass policy"
            else:
                comparable_mass += w
                tv = Q(0)
                for action in actions:
                    po, pr = (parsed[s]["policy"][hand][action] for s in ("own", "reference"))
                    delta = abs(po - pr)
                    lower = None if radius is None else max(Q(0), delta - radius)
                    tv += delta / 2
                    frequencies[action][0] += w * po
                    frequencies[action][1] += w * pr
                    if lower is not None:
                        weighted_lower += w * lower / 2
                    candidates.append({"combo_id": hand, "action_id": action, "own_probability": ratio(po),
                                       "reference_probability": ratio(pr), "w_ref": ratio(w), "own_joint_mass": ratio(own_w),
                                       "own_display_interval": None if radius_own is None else {
                                           "lower": ratio(po - radius_own), "upper": ratio(po + radius_own)},
                                       "reference_display_interval": None if radius_ref is None else {
                                           "lower": ratio(pr - radius_ref), "upper": ratio(pr + radius_ref)},
                                       "absolute_difference_pp": ratio(100 * delta),
                                       "rounding_only_lower_bound_pp": ratio(None if lower is None else 100 * lower),
                                       "_difference": delta})
                weighted_tv += w * tv
                row.update(status="computed", tv_pp=ratio(100 * tv), reason=None)
        rows.append(row)
    if missing_mass:
        blockers.append("reference mass incomplete")
    if unavailable_mass:
        blockers.append("positive reference mass has unavailable policy")
    status = "not_evaluated" if blockers else "not_applicable" if mass == 0 else "computed"
    # Never report a weighted diagnostic over a silently reduced subset.
    aggregate = None
    if status == "computed":
        aggregate = {"tv_pp": ratio(100 * weighted_tv / mass),
                     "rounding_only_tv_lower_bound_pp": ratio(None if radius is None else 100 * weighted_lower / mass),
                     "actions": [{"action_id": a, "own_frequency": ratio(v[0] / mass), "reference_frequency": ratio(v[1] / mass),
                                  "absolute_difference_pp": ratio(100 * abs(v[0] - v[1]) / mass),
                                  "rounding_only_lower_bound_pp": ratio(None if radius is None else 100 * max(Q(0), abs(v[0] - v[1]) / mass - radius))}
                                 for a, v in sorted(frequencies.items())]}
    # Misaligned scope invalidates local comparisons too. Policy-only missingness
    # may retain complete hand rows, clearly labelled as a partial top list.
    scope_blockers = [b for b in blockers if b not in {"reference mass incomplete", "positive reference mass has unavailable policy"}]
    if scope_blockers:
        candidates = []
        comparable_mass = Q(0)
        for row in rows:
            if row["status"] == "computed":
                row.update(status="not_evaluated", tv_pp=None, reason="scope or provenance incomplete")
    candidates.sort(key=lambda v: (-v["_difference"], v["combo_id"], v["action_id"]))
    maximum = ratio(100 * candidates[0]["_difference"]) if candidates else None
    for item in candidates:
        del item["_difference"]
    return {"schema": "r1.frequency-diagnostic-result/v1", "case_id": record["case_id"],
            "node_id": ref["node_id"], "actor": ref["actor"], "status": status, "condition_match": record["condition_match"],
            "quality_status": "not_evaluated", "acceptance": None, "comparison_threshold": None,
            "profile_kinds": {s: raw_profiles[s]["strategy_kind"] for s in parsed}, "issues": blockers,
            "rounding": {s: raw_profiles[s]["rounding"] for s in parsed}, "numeric_error_upper_bound_pp": None,
            "verified_evidence": list(verified.values()),
            "action_sets": {"match": action_match, "own_only": sorted(actions_own - actions_ref), "reference_only": sorted(actions_ref - actions_own)},
            "coverage": {"root_support_hands": len(support), "positive_reference_hands": positive_count,
                         "zero_reference_hands": zero, "missing_reference_mass_hands": missing_mass,
                         "reference_mass": ratio(None if missing_mass else mass), "known_reference_mass": ratio(mass),
                         "comparable_reference_mass": ratio(comparable_mass), "unavailable_policy_reference_mass": ratio(unavailable_mass),
                         "own_mass": ratio(None if missing_own else own_mass), "known_own_mass": ratio(own_mass), "missing_own_mass_hands": missing_own,
                         "positive_mass_cutoff": None, "cutoff_excluded_positive_mass": ratio(Q(0)),
                         "excluded_positive_mass": ratio(None if missing_mass else mass - comparable_mass)},
            "range_weighted": aggregate, "hands": rows,
            "hand_action": {"candidate_count": None if missing_mass or not action_match else positive_count * len(actions),
                            "compared_count": len(candidates), "display_count": min(20, len(candidates)),
                            "complete": status == "computed", "maximum_observed_difference_pp": maximum, "top20": candidates[:20]},
            "limits": ["Arithmetic over explicitly supplied conditional policies and a materialized reference joint measure; evidence identity is not semantic proof.",
                       "Own reach never replaces w_ref. Explicit own policy at zero own reach is compared off policy under the reference measure.",
                       "Unknown precision gives null rounding lower bounds; known rounding bounds are not numeric-error or quality margins.",
                       "No missing policy, reach or legal action is completed, normalized, thresholded or treated as zero. No external quality pass is produced."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--evidence-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    try:
        require(args.input.stat().st_size <= MAX_BYTES, "input exceeds 2 MiB")
        raw = args.input.read_bytes()
        result = diagnose(decode(raw), args.evidence_root)
        result["input"] = file_pin(raw)
        result["validator"] = file_pin(Path(__file__).read_bytes())
        encoded = (json.dumps(result, indent=2, sort_keys=True) + "\n").encode()
        with args.out.open("xb") as stream:
            stream.write(encoded)
        print(json.dumps({"status": result["status"], "quality_status": result["quality_status"], "issues": result["issues"]}))
        return 0 if result["status"] == "computed" else 1
    except (ValueError, OSError, UnicodeError, TypeError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
