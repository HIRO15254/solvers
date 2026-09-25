"""Check supplied HU-R0-016 range bytes and decimal mass; no UI/network/solver.

Requires observed.json and both raw range files. No candidate facts are embedded
here. --write-integrity creates a new report only after the supplied clipboard
lengths/FNV checks and all arithmetic checks pass; it never overwrites evidence.
"""
from decimal import Decimal, Inexact, localcontext
import argparse
import hashlib
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
CASE = "HU-R0-016"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_json(path):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result
    def invalid(value):
        raise ValueError("nonfinite JSON: " + value)
    return json.loads(path.read_text(encoding="utf-8"), parse_float=Decimal,
                      object_pairs_hook=pairs, parse_constant=invalid)


def fnv1a32(raw):
    value = 2166136261
    for byte in raw:
        value = ((value ^ byte) * 16777619) & 0xffffffff
    return f"{value:08x}"


def inspect_range(data, seat, observed, board):
    require(data.endswith(b"\n") and not data.endswith(b"\n\n") and b"\r" not in data,
            seat + " must preserve one appended LF")
    raw = data[:-1]
    require(b"\n" not in raw and raw.isascii(), seat + " copy text must be one ASCII line")
    text = raw.decode("ascii")
    source = observed["root_ranges"]
    require(source[seat] == f"{seat}-range.txt", "unexpected range path")
    require(len(text) == source["reported_copy_raw_characters"][seat], seat + " copied character count differs")
    fingerprint = fnv1a32(raw)
    require(fingerprint == source["reported_copy_raw_fnv1a32"][seat], seat + " copied FNV fingerprint differs")
    parsed = {}
    for entry in text.split(","):
        combo, weight = entry.split(":")
        combo, weight = combo.strip(), Decimal(weight.strip())
        require(re.fullmatch(r"[2-9TJQKA][cdhs][2-9TJQKA][cdhs]", combo), "invalid combo: " + combo)
        cards = frozenset((combo[:2], combo[2:]))
        require(len(cards) == 2 and cards not in parsed, "duplicate card/unordered combo: " + combo)
        require(not cards.intersection(board), "board collision: " + combo)
        require(weight.is_finite() and 0 < weight <= 1, "nonfinite/nonpositive/>1 combo weight: " + combo)
        parsed[cards] = weight
    require(bool(parsed), seat + " range is empty")
    result = {"file": f"{seat}-range.txt", "file_bytes": len(data), "copy_raw_characters": len(text),
              "raw_fnv1a32": fingerprint, "positive_combos": len(parsed),
              "weight_sum": str(sum(parsed.values())), "weight_min": str(min(parsed.values())),
              "weight_max": str(max(parsed.values())), "sha256": hashlib.sha256(data).hexdigest(),
              "duplicate_unordered_combos": 0, "board_collisions": 0,
              "invalid_weights": 0}
    displayed = observed.get("root_ui", {}).get(seat + "_combos_rounded")
    if displayed is not None:
        difference = sum(parsed.values()) - Decimal(str(displayed))
        result["displayed_weight_sum_difference"] = str(difference)
        displayed_step = observed.get("display_steps", {}).get("weighted_combos")
        if displayed_step is not None:
            step = Decimal(str(displayed_step))
            require(step.is_finite() and step > 0, "invalid displayed combo step")
            require(abs(difference) <= step/2, "copied mass inconsistent with displayed combo step")
            result["display_check_scope"] = "display-step consistency only; tie rule/internal precision unverified"
        else:
            result["display_check_scope"] = "signed display difference only; display step/rounding rule unverified"
    return parsed, result


def calculate():
    observed = read_json(HERE / "observed.json")
    require(observed["case_id"] == CASE and observed["street"] == "turn", "wrong case/street")
    board = observed["board"]
    require(len(board) == len(set(board)) == 4 and all(re.fullmatch(r"[2-9TJQKA][cdhs]", card) for card in board),
            "invalid observed Turn board")
    require(observed["condition_match"] == "unverified" and observed["quality_status"] == "not_evaluated"
            and observed["acceptance"] is None and observed["comparison_threshold"] is None
            and observed["reference_exploitability"] is None,
            "range acquisition must not certify reference conditions/quality")
    ranges, results = {}, {}
    with localcontext() as context:
        context.prec = 100
        context.traps[Inexact] = True
        for seat in ("oop", "ip"):
            ranges[seat], results[seat] = inspect_range((HERE / f"{seat}-range.txt").read_bytes(), seat, observed, set(board))
        compatible, mass = 0, Decimal(0)
        for oop_cards, oop_weight in ranges["oop"].items():
            for ip_cards, ip_weight in ranges["ip"].items():
                if not oop_cards.intersection(ip_cards):
                    compatible += 1
                    mass += oop_weight * ip_weight
        require(mass > 0, "zero compatible joint mass")
        count = len(ranges["oop"]) * len(ranges["ip"])
        unrestricted = Decimal(results["oop"]["weight_sum"]) * Decimal(results["ip"]["weight_sum"])
        joint = {"positive_pair_product_count": count, "compatible_positive_pairs": compatible,
                 "incompatible_positive_pairs": count-compatible, "compatible_weight_sum": str(mass),
                 "unrestricted_product_weight_sum": str(unrestricted), "incompatible_weight_sum": str(unrestricted-mass),
                 "definition": "sum(w_oop*w_ip) over disjoint private cards, excluding observed board; no renormalization or future chance weights"}
        frequencies = observed.get("root_ui", {}).get("oop_frequency_percent")
        if frequencies is not None:
            values = [Decimal(str(value)) for value in frequencies.values()]
            require(values and all(value.is_finite() and 0 <= value <= 100 for value in values),
                    "invalid displayed action frequencies")
            displayed_step = observed.get("display_steps", {}).get("frequency_percentage_points")
            if displayed_step is not None:
                step = Decimal(str(displayed_step))
                require(step.is_finite() and step > 0, "invalid displayed frequency step")
                require(abs(sum(values)-100) <= len(values)*step/2, "displayed frequencies exceed their display-step allowance")
    return {"case_id": CASE, "board": board,
            "arithmetic": "Python Decimal, precision 100 with Inexact trap; original decimal weights, no renormalization",
            "hash_scope": "entire ASCII file bytes including the single appended LF",
            "copy_transfer_verification": "Raw character count and FNV-1a32 supplied by collecting root; independently recomputed before arithmetic",
            **results, "joint": joint}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write-integrity", action="store_true", help="create new range-integrity.json; requires supplied observations and ranges")
    args = parser.parse_args()
    result = calculate()
    path = HERE / "range-integrity.json"
    if args.write_integrity:
        with path.open("x", encoding="utf-8", newline="\n") as output:
            output.write(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    else:
        require(read_json(path) == result, "retained range-integrity.json differs from actual bytes/arithmetic")
    print(json.dumps({"case_id": CASE, "range_integrity": "verified",
                      "oop_positive_combos": result["oop"]["positive_combos"],
                      "ip_positive_combos": result["ip"]["positive_combos"],
                      "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None}))


if __name__ == "__main__":
    main()
