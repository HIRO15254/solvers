"""Audit copied Whole/action weights; do not fill or normalize a reference policy.

Decimal tokens are converted to exact rationals. Optional uncertainty intervals
are a user-supplied hypothesis, not a claim about the UI export's precision.
"""
from __future__ import annotations

import argparse
from decimal import Decimal, InvalidOperation
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import re
import sys

HERE = Path(__file__).resolve().parent
RANKS = "23456789TJQKA"
SUITS = "cdhs"
SCHEMA = "r1.reference-profile-capture/v1"


def require(ok, message):
    if not ok:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(data):
    return json.loads(data.decode("utf-8-sig"), object_pairs_hook=unique_object,
                      parse_constant=lambda x: (_ for _ in ()).throw(ValueError(x)))


def rational(text):
    require(isinstance(text, str) and len(text) <= 100, "invalid decimal token")
    require(re.fullmatch(r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?", text),
            f"invalid decimal: {text}")
    try:
        value = Decimal(text)
    except InvalidOperation as exc:
        raise ValueError("invalid decimal") from exc
    require(value.is_finite() and abs(value.as_tuple().exponent) <= 1000,
            "nonfinite or excessive decimal exponent")
    return Fraction(value)


def number(value):
    """An exact, explicitly rational representation, including nonterminating ratios."""
    return {"numerator": str(value.numerator), "denominator": str(value.denominator)}


def card_key(card):
    return RANKS.index(card[0]) * 4 + SUITS.index(card[1])


def combo_key(text):
    require(re.fullmatch(r"[2-9TJQKA][cdhs][2-9TJQKA][cdhs]", text),
            f"invalid combo: {text}")
    cards = [text[:2], text[2:]]
    require(cards[0] != cards[1], f"repeated card: {text}")
    return "".join(sorted(cards, key=card_key))


def parse_range(data, board):
    require(len(data) <= 256 * 1024, "range file too large")
    text = data.decode("utf-8-sig").strip()
    result = {}
    if not text:
        return result
    for item in text.split(","):
        parts = item.split(":")
        require(len(parts) == 2, "expected combo:weight")
        key = combo_key(parts[0].strip())
        require(key not in result, f"duplicate unordered combo: {key}")
        weight = rational(parts[1].strip())
        require(0 <= weight <= 1, f"weight outside [0,1]: {key}")
        require(weight == 0 or not {key[:2], key[2:]} & board,
                f"positive board collision: {key}")
        result[key] = weight
    return result


class Evidence:
    def __init__(self, root):
        self.root = root.resolve()
        self.verified = {}

    def read(self, ref):
        require(isinstance(ref, dict) and set(ref) == {"path", "bytes", "sha256"},
                "invalid file reference")
        require(isinstance(ref["path"], str) and ref["path"], "invalid reference path")
        path = self.root / ref["path"]
        require(not Path(ref["path"]).is_absolute() and path.resolve().is_relative_to(self.root),
                "reference escapes evidence root")
        require(type(ref["bytes"]) is int and 0 <= ref["bytes"] <= 2 * 1024 * 1024,
                "invalid reference byte count")
        require(isinstance(ref["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", ref["sha256"]),
                "invalid reference SHA-256")
        data = path.read_bytes()
        require(len(data) == ref["bytes"] and hashlib.sha256(data).hexdigest() == ref["sha256"],
                f"file identity mismatch: {ref['path']}")
        previous = self.verified.setdefault(ref["path"], dict(ref))
        require(previous == ref, "conflicting identity for the same path")
        return data


def interval(value, epsilon):
    return max(Fraction(0), value - epsilon), min(Fraction(1), value + epsilon)


def conditional_bounds(whole, actions, epsilon):
    """Conservative bounds for Ai/W, requiring sum(Ai)=W simultaneously.

    Every token must be present. A missing token has no implied interval.
    The intervals and product interpretation are hypotheses; no point strategy
    or least-squares repair is produced. Bounds need not be jointly attainable.
    """
    wl, wu = interval(whole, epsilon)
    bounds = [interval(a, epsilon) for a in actions]
    lower = sum((a for a, _ in bounds), Fraction(0))
    upper = sum((b for _, b in bounds), Fraction(0))
    wl, wu = max(wl, lower), min(wu, upper)
    if wl > wu:
        return {"feasible": False, "probability_intervals": None}
    result = {"feasible": True, "whole_interval": [number(wl), number(wu)],
              "probability_intervals": None}
    if wu == 0:
        result["reason"] = "only_zero_whole_ratio_undefined"
        return result
    if wl == 0:
        result["reason"] = "whole_may_be_zero_ratio_not_identified"
        return result
    ratios = []
    for original_low, original_high in bounds:
        low = max(original_low, wl - (upper - original_high))
        high = min(original_high, wu - (lower - original_low))
        ratios.append([number(max(Fraction(0), low / wu)),
                       number(min(Fraction(1), high / wl))])
    result["probability_intervals"] = ratios
    result["reason"] = "conditional_enclosure_not_a_selected_policy"
    return result


def expected_menus(observed):
    require(observed["case_id"] == "HU-R0-019", "case identity mismatch")
    require(observed["board"] == ["Qs", "7h", "2c", "4d", "9s"], "board identity mismatch")
    menus = {}
    seats = [observed["seats"][s] for s in ("oop", "ip")]
    for node in observed["observed_menus"]:
        history = node["history"]
        require(history not in menus, "duplicate observed history")
        actions = node["actions"]
        require(actions and len(actions) == len(set(actions)), "duplicate/empty observed actions")
        depth = len(history.split(" / ")) if history else 0
        require(node["actor"] == seats[depth % 2], "actor/history mismatch")
        menus[history] = node
    require("" in menus, "missing observed root")
    children = []
    for history, node in menus.items():
        for action in node["actions"]:
            child = f"{history} / {action}" if history else action
            terminal = action in ("fold", "call") or (history == "check" and action == "check")
            require((child not in menus) if terminal else (child in menus), "observed tree not closed")
            if not terminal:
                children.append(child)
    require(sorted(children) == sorted(set(menus) - {""}), "observed graph not a tree")
    return menus


def prior_own_action(history, actor, menus):
    parts = history.split(" / ") if history else []
    for i in range(len(parts) - 1, -1, -1):
        ancestor = " / ".join(parts[:i])
        if menus[ancestor]["actor"] == actor:
            return ancestor, parts[i]
    return None


def audit(manifest, evidence_root, assumed_error=None):
    require(isinstance(manifest, dict), "manifest must be an object")
    required = {"schema", "case_id", "observed", "root_ranges", "captures"}
    require(required <= set(manifest) <= required | {"notes", "capture_metadata", "supplemental_files"}, "manifest fields differ")
    require(manifest["schema"] == SCHEMA and manifest["case_id"] == "HU-R0-019", "wrong manifest schema/case")
    require(assumed_error is None or 0 <= assumed_error <= 1, "invalid assumed absolute error")
    evidence = Evidence(evidence_root)
    observed = load_json(evidence.read(manifest["observed"]))
    require(isinstance(manifest.get("supplemental_files", []), list), "supplemental_files must be a list")
    for ref in manifest.get("supplemental_files", []):
        evidence.read(ref)
    menus = expected_menus(observed)
    board = set(observed["board"])
    require(set(manifest["root_ranges"]) == {"oop", "ip"}, "both root ranges required")
    roots = {observed["seats"][s]: parse_range(evidence.read(manifest["root_ranges"][s]), board)
             for s in ("oop", "ip")}
    require(all(any(w > 0 for w in r.values()) for r in roots.values()), "empty root support")
    captures = {}
    require(isinstance(manifest["captures"], list), "captures must be a list")
    for capture in manifest["captures"]:
        require(set(capture) == {"history", "actor", "whole", "actions"}, "capture fields differ")
        h = capture["history"]
        require(h in menus and h not in captures, "unknown/duplicate captured history")
        require(capture["actor"] == menus[h]["actor"], "captured actor mismatch")
        whole = None if capture["whole"] is None else parse_range(evidence.read(capture["whole"]), board)
        require(isinstance(capture["actions"], list), "actions must be a list")
        actions = {}
        for a in capture["actions"]:
            require(set(a) == {"label", "range"}, "action fields differ")
            require(a["label"] in menus[h]["actions"] and a["label"] not in actions,
                    "unknown/duplicate action label")
            actions[a["label"]] = None if a["range"] is None else parse_range(evidence.read(a["range"]), board)
        captures[h] = {"whole": whole, "actions": actions}
    nodes = []
    for history, menu in menus.items():
        actor, labels = menu["actor"], menu["actions"]
        root = roots[actor]
        support = {c for c, w in root.items() if w > 0}
        capture = captures.get(history, {"whole": None, "actions": {}})
        whole, actions = capture["whole"], capture["actions"]
        previous = prior_own_action(history, actor, menus)
        if previous is None:
            expected = root
            previous_label = "root_range"
        else:
            parent, action = previous
            expected = captures.get(parent, {}).get("actions", {}).get(action)
            previous_label = {"history": parent, "action": action}
        rows = []
        for combo in sorted(support):
            w = None if whole is None else whole.get(combo)
            av = [None if actions.get(a) is None else actions[a].get(combo) for a in labels]
            row = {"combo": combo, "whole": None if w is None else number(w),
                   "missing_actions": [a for a, v in zip(labels, av) if v is None],
                   "action_weights": [None if v is None else number(v) for v in av]}
            p = None if expected is None else expected.get(combo)
            row["prior_own_product_comparison"] = {"available": p is not None and w is not None,
                "prior_weight": None if p is None else number(p),
                "whole_minus_prior": None if p is None or w is None else number(w - p),
                "exact_equal": None if p is None or w is None else p == w}
            if assumed_error is not None and p is not None and w is not None:
                row["prior_own_product_comparison"]["conditional_intervals_overlap"] = abs(p - w) <= 2 * assumed_error
            row["literal_product_ratios"] = None
            if w is None:
                row["classification"] = "whole_missing_no_zero_inference"
            elif any(v is None for v in av):
                row["classification"] = "action_token_missing_no_zero_inference"
            else:
                total = sum(av, Fraction(0))
                row["sum_actions_minus_whole"] = number(total - w)
                if w == 0:
                    row["classification"] = "explicit_zero_whole" if total == 0 else "zero_whole_positive_action"
                elif total != w:
                    row["classification"] = "nonclosing_literal_products_no_renormalization"
                else:
                    row["classification"] = "literal_products_close"
                    row["literal_product_ratios"] = [number(v / w) for v in av]
                if assumed_error is not None:
                    row["conditional_rounding_model"] = conditional_bounds(w, av, assumed_error)
            rows.append(row)
        outside = {}
        for label, values in [("whole", whole)] + [(a, actions.get(a)) for a in labels]:
            outside[label] = None if values is None else {
                "copy_content_class": "empty_copy" if not values else "tokens",
                "token_count": len(values), "positive_tokens": sum(w > 0 for w in values.values()),
                "positive_outside_root_support": sorted(c for c, w in values.items() if w > 0 and c not in support),
                "explicit_zero_tokens": sum(w == 0 for w in values.values())}
        counts = {}
        for row in rows:
            counts[row["classification"]] = counts.get(row["classification"], 0) + 1
        comparisons = [row["prior_own_product_comparison"] for row in rows]
        differences = {}
        for row in rows:
            value = row["prior_own_product_comparison"]["whole_minus_prior"]
            if value is not None:
                differences[row["combo"]] = abs(Fraction(int(value["numerator"]), int(value["denominator"])))
        maximum = max(differences.values(), default=None)
        own_reach_summary = {
            "comparable_combos": len(differences),
            "exact_equal_combos": sum(x["exact_equal"] is True for x in comparisons),
            "different_combos": sum(x["exact_equal"] is False for x in comparisons),
            "unavailable_combos": sum(not x["available"] for x in comparisons),
            "max_absolute_difference": None if maximum is None else number(maximum),
            "max_difference_combos": sorted(c for c, value in differences.items() if value == maximum) if maximum else [],
        }
        if assumed_error is not None:
            own_reach_summary["conditional_nonoverlap_combos"] = sum(
                x.get("conditional_intervals_overlap") is False for x in comparisons)
        nodes.append({"history": history, "actor": actor, "action_order": labels,
            "whole_file_present": whole is not None, "missing_action_files": [a for a in labels if actions.get(a) is None],
            "prior_own_product_source": previous_label, "root_support_count": len(support),
            "own_reach_comparison_summary": own_reach_summary,
            "token_inventory": outside, "classification_counts": counts, "rows": rows})
    return {"schema": "r1.reference-profile-audit/v1", "case_id": "HU-R0-019",
        "manifest_sha256_canonical": hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(",", ":"),
            ensure_ascii=False).encode()).hexdigest(),
        "raw_files": list(evidence.verified.values()), "expected_nodes": len(menus),
        "captured_nodes": len(captures), "nodes": nodes,
        "assumed_absolute_error": None if assumed_error is None else number(assumed_error),
        "interpretation": {
            "product_semantics": "unverified; action=own realization reach times conditional action probability is a hypothesis",
            "literal_ratios": "conditional on copied tokens being exact products; no missing-token fill or normalization",
            "rounding_model": "conditional only, not an observed export precision guarantee; no omission model",
            "constraint_scope": "per-combo node intervals and pairwise own-history comparisons only; no claim of a globally feasible shared-variable tree",
            "off_policy_br": "joint profile reach zero does not remove BR requirements; true zero own realization reach of the fixed player can, but omission/rounded zero is not proof",
            "reference_profile_evaluation": "not_performed", "condition_match": "unverified",
            "quality_status": "not_evaluated", "acceptance": None}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--evidence-root", type=Path, default=HERE.parent)
    parser.add_argument("--assumed-absolute-error", help="Optional hypothetical per-present-token error, e.g. 5e-13")
    args = parser.parse_args()
    try:
        raw = args.manifest.read_bytes()
        require(len(raw) <= 2 * 1024 * 1024, "manifest too large")
        report = audit(load_json(raw), args.evidence_root,
                       None if args.assumed_absolute_error is None else rational(args.assumed_absolute_error))
        report["manifest_sha256"] = hashlib.sha256(raw).hexdigest()
        report["checker_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 0
    except (ValueError, KeyError, TypeError, OSError, UnicodeError) as exc:
        print(f"invalid evidence: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
