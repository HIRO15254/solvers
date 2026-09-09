#!/usr/bin/env python3
"""Diagnostic weighted comparison of Simple GTOW 169-class data and research rows."""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

RESEARCH_SCHEMA = "solvers.multiway-average-sampling-research/v1"
DRAW_RESEARCH_SCHEMA = "solvers.multiway-draw-abstraction-research/v1"
UNREACHED_BUCKET = 2**32 - 1
EXPECTED_PLAYERS = {"UTG": 3, "HJ": 4, "CO": 5, "BTN": 0, "SB": 1}
EXPECTED_ACTIVE_OPPONENTS = {
    position: 5 - ordinal for ordinal, position in enumerate(EXPECTED_PLAYERS)
}
EXPECTED_REFERENCE_HISTORIES = {
    "UTG": [], "HJ": ["fold"], "CO": ["fold", "fold"],
    "BTN": ["fold", "fold", "fold"], "SB": ["fold", "fold", "fold", "fold"],
}


def combo_weight(hand: str) -> int:
    if len(hand) == 2 or hand[1] == hand[0]:
        return 6
    return 4 if hand.endswith("s") else 12


def canonical_hands() -> list[str]:
    ranks = "AKQJT98765432"
    result = [""] * 169
    for row, hi in enumerate(ranks):
        for col, lo in enumerate(ranks):
            if row == col:
                label = hi + lo
            elif col > row:
                label = hi + lo + "s"
            else:
                label = lo + hi + "o"
            result[row * 13 + col] = label
    return result


def finite_probability(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{label}: probability must be numeric")
    value = float(value)
    if not math.isfinite(value) or not 0 <= value <= 1:
        raise ValueError(f"{label}: probability must be finite and in [0, 1]")
    return value


def history_bytes(value: object, label: str) -> list[int]:
    if (not isinstance(value, list) or len(value) != 16
            or any(isinstance(item, bool) or not isinstance(item, int) or not 0 <= item <= 255
                   for item in value)):
        raise ValueError(f"{label}: history must be sixteen bytes")
    return value


def validate_research_schema(research: dict) -> None:
    """Accept only explicit, internally consistent research representations."""
    if research.get("schemaVersion") == RESEARCH_SCHEMA:
        if "researchAbstraction" in research:
            raise ValueError("abstraction experiment requires its dedicated schema")
        return
    if research.get("schemaVersion") != DRAW_RESEARCH_SCHEMA:
        raise ValueError("research schema mismatch")
    meta = research.get("researchAbstraction")
    if not isinstance(meta, dict) or set(meta) != {
        "kind", "baseTableBuckets", "effectiveBuckets", "transformVersion"
    }:
        raise ValueError("invalid research abstraction metadata")
    kind = meta["kind"]
    if kind not in ("ehs2", "draw-aware"):
        raise ValueError("unknown research abstraction")
    version = "draw-flags/v1" if kind == "draw-aware" else "identity/v1"
    if meta["transformVersion"] != version:
        raise ValueError("research abstraction transform version mismatch")
    for field in ("baseTableBuckets", "effectiveBuckets"):
        counts = meta[field]
        if not isinstance(counts, dict) or set(counts) != {"flop", "turn", "river"}:
            raise ValueError("invalid research bucket counts")
        if any(isinstance(n, bool) or not isinstance(n, int) or not 1 <= n <= 65535
               for n in counts.values()):
            raise ValueError("invalid research bucket count")
    for street in ("flop", "turn", "river"):
        multiplier = 4 if kind == "draw-aware" and street != "river" else 1
        if meta["effectiveBuckets"][street] != multiplier * meta["baseTableBuckets"][street]:
            raise ValueError("research effective bucket count mismatch")


def reference_nodes(reference: dict) -> dict[str, dict]:
    nodes = reference.get("nodes")
    if not isinstance(nodes, list) or len(nodes) != 5:
        raise ValueError("reference must contain exactly five nodes")
    by_position: dict[str, dict] = {}
    for node in nodes:
        if not isinstance(node, dict):
            raise ValueError("reference node must be an object")
        position = node.get("position")
        if position not in EXPECTED_PLAYERS or position in by_position:
            raise ValueError("reference positions must be the five unique unopened seats")
        if node.get("history") != EXPECTED_REFERENCE_HISTORIES[position]:
            raise ValueError(f"{position}: reference history is not the unopened fold path")
        by_position[position] = node
    if set(by_position) != set(EXPECTED_PLAYERS):
        raise ValueError("reference positions must cover UTG/HJ/CO/BTN/SB")
    return by_position


def compare(reference: dict, research: dict, strict_menu: bool = False) -> dict:
    if reference.get("solution", {}).get("gametype") != "Cash6m50zSimple":
        raise ValueError("reference is not the GTOW Simple solution")
    validate_research_schema(research)
    if research.get("solverStateVersion") != 4:
        raise ValueError("research must use solver state version 4")
    result = research.get("result")
    if not isinstance(result, dict):
        raise ValueError("research result must be an object")
    histories = result.get("histories")
    if not isinstance(histories, list) or len(histories) != 5:
        raise ValueError("research must contain exactly five unopened history rows")
    output = {"schema": "multiway-simple-reference-comparison/v1",
              "diagnostic_only": True, "nodes": []}
    if "researchAbstraction" in research:
        output["research_abstraction"] = research["researchAbstraction"]
    by_player: dict[int, dict] = {}
    for outer in histories:
        if not isinstance(outer, dict):
            raise ValueError("research history must be an object")
        rows = outer.get("strategies")
        outer_history = history_bytes(outer.get("history"), "research history")
        if not isinstance(rows, list) or len(rows) != 169:
            raise ValueError("each research history must contain exactly 169 strategy rows")
        players = set()
        for row in rows:
            if not isinstance(row, dict) or not isinstance(row.get("key"), dict):
                raise ValueError("research strategy row/key must be an object")
            player = row["key"].get("player")
            players.add(player)
            if row["key"].get("history") != outer_history:
                raise ValueError("strategy key history does not match its outer history")
        if len(players) != 1:
            raise ValueError("research histories have duplicate/ambiguous player identities")
        player = next(iter(players))
        if isinstance(player, bool) or player not in EXPECTED_PLAYERS.values() or player in by_player:
            raise ValueError("research history has an unexpected or duplicate player")
        by_player[player] = outer
    if set(by_player) != set(EXPECTED_PLAYERS.values()):
        raise ValueError("research histories must cover players 3,4,5,0,1 exactly")

    canonical = canonical_hands()
    canonical_index = {hand: index for index, hand in enumerate(canonical)}
    for position, ref in reference_nodes(reference).items():
        history = by_player[EXPECTED_PLAYERS[position]]
        rows = history["strategies"]
        by_bucket: dict[int, dict] = {}
        for row in rows:
            key = row["key"]
            bucket_path = key.get("bucket_path")
            if key.get("street") != 0 or not isinstance(bucket_path, list) or len(bucket_path) != 4:
                raise ValueError(f"{position}: invalid preflop bucket path")
            bucket = bucket_path[0]
            if (isinstance(bucket, bool) or not isinstance(bucket, int) or not 0 <= bucket < 169
                    or any(isinstance(value, bool) or not isinstance(value, int)
                           or value != UNREACHED_BUCKET for value in bucket_path[1:])):
                raise ValueError(f"{position}: invalid bucket index or postflop tail")
            if key.get("player") != EXPECTED_PLAYERS[position]:
                raise ValueError(f"{position}: strategy player identity mismatch")
            if key.get("active_opponents") != EXPECTED_ACTIVE_OPPONENTS[position]:
                raise ValueError(f"{position}: active-opponents identity mismatch")
            if bucket in by_bucket:
                raise ValueError(f"{position}: duplicate bucket")
            by_bucket[bucket] = row

        hands = ref.get("raise_by_hand")
        if not isinstance(hands, dict) or len(hands) != 169 or set(hands) != set(canonical):
            raise ValueError(f"{position}: reference must contain canonical 169 hand classes")
        missing = [hand for hand, index in canonical_index.items() if index not in by_bucket]
        if missing:
            raise ValueError(f"{position}: missing research hands {missing[:3]}")

        row_menus = []
        for bucket, row in sorted(by_bucket.items()):
            if row.get("status") != "average-observed":
                raise ValueError(f"{position}: only average-observed rows are accepted")
            actions = row.get("actions")
            if not isinstance(actions, list) or not actions:
                raise ValueError(f"{position} bucket {bucket}: missing actions")
            labels: set[str] = set()
            total = 0.0
            for action in actions:
                if not isinstance(action, dict):
                    raise ValueError(f"{position} bucket {bucket}: invalid action row")
                label = action.get("action")
                if not isinstance(label, str) or label in labels:
                    raise ValueError(f"{position} bucket {bucket}: invalid action label")
                labels.add(label)
                total += finite_probability(action.get("probability"), f"{position} bucket {bucket}")
            if abs(total - 1.0) > 1e-5:
                raise ValueError(f"{position} bucket {bucket}: probabilities do not sum to one")
            row_menus.append(frozenset(labels))

        menu = ref.get("menu")
        if (not isinstance(menu, list) or any(not isinstance(label, str) for label in menu)
                or len(set(menu)) != len(menu)):
            raise ValueError(f"{position}: invalid reference menu")
        reference_menu = set(menu)
        local_menu_match = all(row_menu == reference_menu for row_menu in row_menus)
        row_menu_mismatches = [
            bucket for bucket, row_menu in zip(sorted(by_bucket), row_menus)
            if row_menu != reference_menu
        ]
        research_menu = set().union(*row_menus)
        if strict_menu and not local_menu_match:
            raise ValueError(f"{position}: row menu mismatch")
        base = {"position": position, "history": ref["history"],
                "menu_match": local_menu_match, "local_menu_match": local_menu_match,
                "global_model_equivalence_unverified": True,
                "reference_menu": sorted(reference_menu), "research_menu": sorted(research_menu),
                "row_menu_mismatches": row_menu_mismatches,
                "distinct_row_menus": sorted([sorted(menu) for menu in set(row_menus)])}
        if not local_menu_match:
            base.update(comparable=False,
                        reason="row menu mismatch; normal-raise metrics intentionally omitted")
            output["nodes"].append(base)
            continue

        raise_labels = [label for label in reference_menu
                        if label.startswith("raise-to:") and "all-in" not in label]
        if len(raise_labels) != 1:
            raise ValueError(f"{position}: reference menu must have one non-jam raise")
        target_label = raise_labels[0]
        diffs = []
        weight_total = abs_sum = sq_sum = 0.0
        for hand, target in hands.items():
            target = finite_probability(target, f"{position} {hand}")
            row = by_bucket[canonical_index[hand]]
            actions = {action["action"]: action["probability"] for action in row["actions"]}
            if target_label not in actions:
                raise ValueError(f"{position} {hand}: normal raise action missing")
            value = finite_probability(actions[target_label], f"{position} {hand}")
            diff = value - target
            weight = combo_weight(hand)
            weight_total += weight
            abs_sum += weight * abs(diff)
            sq_sum += weight * diff * diff
            diffs.append({"hand": hand, "reference": target, "research": value,
                          "delta": diff, "weight": weight})
        base.update(comparable=True, weighted_mae=abs_sum / weight_total,
                    weighted_rmse=math.sqrt(sq_sum / weight_total),
                    top_hand_differences=sorted(diffs, key=lambda x: abs(x["delta"]), reverse=True)[:10])
        output["nodes"].append(base)
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("research", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--strict-menu", action="store_true")
    args = parser.parse_args()
    result = compare(json.loads(args.reference.read_text(encoding="utf-8")),
                     json.loads(args.research.read_text(encoding="utf-8")), args.strict_menu)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"nodes": len(result["nodes"]), "diagnostic_only": True}))


if __name__ == "__main__":
    main()
