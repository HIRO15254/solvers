"""Recheck copied range bytes and exact decimal arithmetic; no solver or network."""

from decimal import Decimal, Inexact, localcontext
import hashlib
import json
from pathlib import Path
import re


HERE = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check():
    evidence = json.loads((HERE / "range-integrity.json").read_text(encoding="utf-8"))
    observed = json.loads((HERE / "observed.json").read_text(encoding="utf-8"), parse_float=Decimal)
    require(evidence["case_id"] == observed["case_id"] == "HU-R0-017", "case identity mismatch")
    require(evidence["board"] == observed["board"] == ["Kh", "Kc", "5s", "2d", "3s"], "board mismatch")
    board = set(observed["board"])
    ranges, results = {}, {}
    with localcontext() as context:
        context.prec = 80
        context.traps[Inexact] = True
        for seat in ("oop", "ip"):
            expected = evidence[seat]
            require(expected["file"] == observed["root_ranges"][seat] == f"{seat}-range.txt",
                    f"{seat} range filename mismatch")
            data = (HERE / expected["file"]).read_bytes()
            require(data.endswith(b"\n") and not data.endswith(b"\n\n") and b"\r" not in data,
                    f"{seat} must retain exactly one final LF")
            text = data[:-1].decode("utf-8")
            require(len(text) == observed["root_ranges"]["reported_copy_raw_characters"][seat],
                    f"{seat} copied string length differs from the browser observation")
            require(text.isascii(), f"{seat} range transfer checksum expects ASCII")
            fnv = 2166136261
            for byte in text.encode("ascii"):
                fnv = ((fnv ^ byte) * 16777619) & 0xffffffff
            raw_fnv1a32 = f"{fnv:08x}"
            require(raw_fnv1a32 == observed["root_ranges"]["reported_copy_raw_fnv1a32"][seat],
                    f"{seat} raw checksum differs from the CUA clipboard verification")
            require("\n" not in text, f"{seat} unexpected internal newline")
            parsed, seen = [], set()
            for item in text.split(","):
                combo, raw_weight = item.split(":")
                combo, weight = combo.strip(), Decimal(raw_weight.strip())
                require(re.fullmatch(r"[2-9TJQKA][cdhs][2-9TJQKA][cdhs]", combo) is not None,
                        f"{seat} invalid combo: {combo}")
                cards = frozenset((combo[:2], combo[2:]))
                require(len(cards) == 2, f"{seat} repeats a card: {combo}")
                require(cards not in seen, f"{seat} duplicate unordered combo: {combo}")
                require(not cards & board, f"{seat} board collision: {combo}")
                require(weight.is_finite() and weight > 0, f"{seat} invalid weight: {combo}")
                seen.add(cards)
                parsed.append((cards, weight))
            weight_sum = sum(weight for _, weight in parsed)
            actual = {
                "file": expected["file"], "file_bytes": len(data), "copy_raw_characters": len(text),
                "raw_fnv1a32": raw_fnv1a32,
                "positive_combos": len(parsed), "weight_sum": str(weight_sum),
                "weight_min": str(min(weight for _, weight in parsed)),
                "weight_max": str(max(weight for _, weight in parsed)),
                "sha256": hashlib.sha256(data).hexdigest(), "duplicate_unordered_combos": 0,
                "board_collisions": 0, "nonfinite_or_nonpositive_weights": 0,
            }
            require(actual == expected, f"{seat} integrity record differs from file/arithmetic")
            # Consistency away from a rounding boundary; no tie rule is inferred.
            displayed = observed["root_ui"][f"{seat}_combos_rounded"]
            require(abs(weight_sum - displayed) < Decimal("0.05"),
                    f"{seat} decimal mass is inconsistent with the displayed 0.1 step")
            ranges[seat], results[seat] = parsed, actual
        pairs, mass = 0, Decimal(0)
        for oop_cards, oop_weight in ranges["oop"]:
            for ip_cards, ip_weight in ranges["ip"]:
                if not oop_cards & ip_cards:
                    pairs += 1
                    mass += oop_weight * ip_weight
        product_count = len(ranges["oop"]) * len(ranges["ip"])
        product_mass = Decimal(results["oop"]["weight_sum"]) * Decimal(results["ip"]["weight_sum"])
        actual_joint = {
            "positive_pair_product_count": product_count, "compatible_positive_pairs": pairs,
            "incompatible_positive_pairs": product_count - pairs, "compatible_weight_sum": str(mass),
            "unrestricted_product_weight_sum": str(product_mass),
            "incompatible_weight_sum": str(product_mass - mass),
        }
        for key, value in actual_joint.items():
            require(evidence["joint"][key] == value, f"joint {key} mismatch")
        frequencies = observed["root_ui"]["oop_frequency_percent"]
        frequency_step = observed["display_steps"]["frequency_percentage_points"]
        require(abs(sum(frequencies.values()) - 100) <= len(frequencies) * frequency_step / 2,
                "displayed OOP action frequencies exceed their rounding allowance")
    require(observed["condition_match"] == "unverified"
            and observed["quality_status"] == "not_evaluated"
            and observed["acceptance"] is None and observed["comparison_threshold"] is None,
            "partial acquisition must not certify reference conditions/quality")
    return {"case_id": "HU-R0-017", "range_integrity": "verified",
            "oop_positive_combos": results["oop"]["positive_combos"],
            "ip_positive_combos": results["ip"]["positive_combos"], **actual_joint,
            "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None}


if __name__ == "__main__":
    print(json.dumps(check(), indent=2))
