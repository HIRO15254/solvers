"""Verify 006 Copy bytes and exact compatible-pair mass; no UI/solver/network.

Adapted from the repository's HU-R0-022 range checker. Metadata is acquisition
evidence supplied by the collecting root, not independently observed UI state.
"""

from decimal import Decimal, Inexact, localcontext
import argparse
import hashlib
import json
from pathlib import Path
import re
from urllib.parse import parse_qsl, urlsplit

HERE = Path(__file__).resolve().parent
CASE = "HU-R0-006"
BOARD = ["9s", "8s", "7d", "2c", "Jd"]
COPY_PINS = {"oop": (8417, "07f300b7"), "ip": (7817, "488d4640")}
ROOT_ACTIONS = ["check", "bet 2", "bet 4.5", "bet 9", "allin 97"]


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


def parse_range(raw, board):
    """Reject duplicates rather than merging/double-counting unordered hands."""
    require(raw and raw.isascii() and b"\n" not in raw and b"\r" not in raw,
            "Copy text must be one nonempty ASCII line")
    require(len(board) == len(set(board)) and all(
        re.fullmatch(r"[2-9TJQKA][cdhs]", card) for card in board), "invalid board")
    parsed = {}
    for entry in raw.decode("ascii").split(","):
        pieces = entry.split(":")
        require(len(pieces) == 2, "invalid range token")
        combo, text = (piece.strip() for piece in pieces)
        require(re.fullmatch(r"[2-9TJQKA][cdhs][2-9TJQKA][cdhs]", combo), "invalid combo: " + combo)
        require(re.fullmatch(r"(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?", text),
                "invalid numeric weight: " + combo)
        weight = Decimal(text)
        cards = frozenset((combo[:2], combo[2:]))
        require(len(cards) == 2, "same card used twice: " + combo)
        require(cards not in parsed, "duplicate unordered combo: " + combo)
        require(not cards.intersection(board), "board collision: " + combo)
        require(weight.is_finite() and 0 < weight <= 1, "nonpositive or greater-than-one weight: " + combo)
        parsed[cards] = weight
    return parsed


def inspect_range(data, seat, observed):
    require(data.endswith(b"\n") and not data.endswith(b"\n\n") and b"\r" not in data,
            seat + " must preserve exactly one appended LF")
    raw = data[:-1]
    source = observed["root_ranges"]
    require(source[seat] == f"{seat}-range.txt", "unexpected range filename")
    expected_length, expected_fnv = COPY_PINS[seat]
    require(source["reported_copy_raw_characters"][seat] == expected_length
            and source["reported_copy_raw_fnv1a32"][seat] == expected_fnv, "acquisition pins changed")
    require(raw.isascii() and len(raw) == expected_length, seat + " copied character count differs")
    require(fnv1a32(raw) == expected_fnv, seat + " copied FNV differs")
    parsed = parse_range(raw, observed["board"])
    return parsed, {"file": f"{seat}-range.txt", "file_bytes": len(data),
        "copy_raw_characters": len(raw), "raw_fnv1a32": fnv1a32(raw),
        "sha256": hashlib.sha256(data).hexdigest(), "positive_combos": len(parsed),
        "weight_sum": str(sum(parsed.values())), "weight_min": str(min(parsed.values())),
        "weight_max": str(max(parsed.values())), "duplicate_unordered_combos": 0,
        "repeated_private_cards": 0, "board_collisions": 0, "invalid_weights": 0}


def joint_mass(oop, ip):
    compatible_count, incompatible_count = 0, 0
    compatible_mass = incompatible_mass = Decimal(0)
    for oop_cards, oop_weight in oop.items():
        for ip_cards, ip_weight in ip.items():
            weight = oop_weight * ip_weight
            if oop_cards.intersection(ip_cards):
                incompatible_count += 1
                incompatible_mass += weight
            else:
                compatible_count += 1
                compatible_mass += weight
    product_count = len(oop) * len(ip)
    product_mass = sum(oop.values()) * sum(ip.values())
    require(compatible_count + incompatible_count == product_count, "pair count partition")
    require(compatible_mass + incompatible_mass == product_mass, "pair mass partition")
    require(compatible_mass > 0, "zero compatible joint mass")
    return {"positive_pair_product_count": product_count,
        "compatible_positive_pairs": compatible_count, "incompatible_positive_pairs": incompatible_count,
        "compatible_weight_sum": str(compatible_mass), "incompatible_weight_sum": str(incompatible_mass),
        "unrestricted_product_weight_sum": str(product_mass),
        "definition": "sum(w_oop*w_ip) over disjoint retained private cards; no normalization, chance weights or folded-seat card distribution"}


def validate_observation(observed):
    require(observed["case_id"] == CASE and observed["street"] == "river"
            and observed["board"] == BOARD, "wrong case/street/board")
    require(observed["seats"] == {"oop": "SB", "ip": "BB"}, "seat mapping")
    require(observed["root_actor"] == "SB" and observed["pot_bb"] == 6
            and observed["stacks_behind_bb"] == [97, 97], "root pot/stack/actor changed")
    menu = observed["observed_menus"]
    require(len(menu) == 1 and menu[0]["history"] == "" and menu[0]["actor"] == "SB"
            and menu[0]["actions"] == ROOT_ACTIONS, "record must retain only the observed root menu")
    require(list(observed["root_ui"]["oop_frequency_percent"]) == ROOT_ACTIONS, "display action keys/order mismatch")
    require(observed["continuation_menu_record"]["graph_closure_evaluated"] is False,
            "range-only evidence cannot establish menu closure")
    capture = observed["capture_window"]
    require(capture["after_utc"] == "2026-09-26T21:41:50Z"
            and capture["completed_before_utc"] == "2026-09-26T21:49:31Z", "capture bounds changed")
    freshness = observed["root_ranges"]["copy_freshness"]
    require(freshness["whole_range_menu_shown"] is False
            and freshness["explicit_whole_range_option_selected"] is False, "unobserved Whole selection claimed")
    parsed = urlsplit(observed["url"])
    params = parse_qsl(parsed.query, keep_blank_values=True)
    require(parsed.scheme == "https" and parsed.netloc == "app.gtowizard.com"
            and parsed.path == "/solutions" and not parsed.fragment and len(dict(params)) == len(params), "invalid captured URL")
    expected = {"soltab": "range", "solution_type": "gwiz", "gmfs_solution_tab": "ai_sols",
                "gametype": "Cash6m50zGGGeneral", "depth": "100", "gmfft_sort_key": "0",
                "gmfft_sort_order": "desc", "stratab": "strategy_ev", "history_spot": "10",
                "preflop_actions": "F-F-F-F-R3-C", "board": "9s8s7d2cJd", "flop_actions": "X-X", "turn_actions": "X-X"}
    require(dict(params) == expected and observed["preflop_actions"] == expected["preflop_actions"], "captured URL game/history mismatch")
    require(observed["comparison_scope"] == "diagnostic_only"
            and observed["condition_match"] == "unverified"
            and observed["quality_status"] == "not_evaluated"
            and observed["acceptance"] is None and observed["comparison_threshold"] is None
            and observed["reference_exploitability"] is None, "range acquisition must not certify quality")


def calculate(directory=HERE):
    observed = read_json(directory / "observed.json")
    validate_observation(observed)
    ranges, results = {}, {}
    with localcontext() as context:
        context.prec = 100
        context.traps[Inexact] = True
        for seat in ("oop", "ip"):
            ranges[seat], results[seat] = inspect_range(
                (directory / f"{seat}-range.txt").read_bytes(), seat, observed)
        joint = joint_mass(ranges["oop"], ranges["ip"])
        displayed = [Decimal(str(x)) for x in observed["root_ui"]["oop_frequency_percent"].values()]
        require(len(displayed) == 5 and all(x.is_finite() and 0 <= x <= 100 for x in displayed),
                "invalid displayed frequencies")
        require(sum(displayed) == 100, "retained root displayed frequencies no longer sum to 100")
    observed_raw = (directory / "observed.json").read_bytes()
    return {"schema": "r1.reference-ranges/v1", "case_id": CASE, "board": BOARD,
        "observed": {"file": "observed.json", "bytes": len(observed_raw),
                     "sha256": hashlib.sha256(observed_raw).hexdigest()},
        "arithmetic": "Decimal precision 100 with Inexact trap; both compatible and incompatible mass summed independently",
        "byte_scope": "original ASCII Copy text plus exactly one appended LF; no rewrite or renormalization",
        "transfer_check_scope": "reported browser character count and FNV independently recalculated; UI provenance remains collecting root's report",
        **results, "joint": joint, "condition_match": "unverified",
        "quality_status": "not_evaluated", "acceptance": None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="create ranges.json only if absent")
    args = parser.parse_args()
    result = calculate()
    path = HERE / "ranges.json"
    if args.write:
        with path.open("x", encoding="utf-8", newline="\n") as output:
            output.write(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    else:
        require(read_json(path) == result, "retained ranges.json differs from bytes/arithmetic")
    print(json.dumps({"case_id": CASE, "range_integrity": "verified",
        "oop_positive_combos": result["oop"]["positive_combos"],
        "ip_positive_combos": result["ip"]["positive_combos"],
        "compatible_positive_pairs": result["joint"]["compatible_positive_pairs"],
        "condition_match": "unverified", "quality_status": "not_evaluated"}))


if __name__ == "__main__":
    main()
