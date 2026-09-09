from copy import deepcopy
import importlib.util
import math
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "cmp", Path(__file__).parents[1] / "multiway_simple_reference_compare.py"
)
cmp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cmp)


POSITIONS = ("UTG", "HJ", "CO", "BTN", "SB")
PLAYERS = (3, 4, 5, 0, 1)


def ref():
    hands = {hand: index / 168 for index, hand in enumerate(cmp.canonical_hands())}
    return {
        "solution": {"gametype": "Cash6m50zSimple"},
        "nodes": [
            {
                "position": position,
                "history": ["fold"] * POSITIONS.index(position),
                "menu": ["fold", "raise-to:2000"],
                "raise_by_hand": hands,
            }
            for position in POSITIONS
        ],
    }


def research(values=None):
    if values is None:
        values = [index / 168 for index in range(169)]
    histories = []
    for player_index, player in enumerate(PLAYERS):
        outer_history = [player_index] * 16
        rows = []
        for bucket, value in enumerate(values):
            rows.append(
                {
                    "key": {
                        "history": outer_history[:],
                        "player": player,
                        "street": 0,
                        "active_opponents": 5 - player_index,
                        "bucket_path": [bucket, 2**32 - 1, 2**32 - 1, 2**32 - 1],
                    },
                    "status": "average-observed",
                    "actions": [
                        {"action": "fold", "probability": 1.0 - value},
                        {"action": "raise-to:2000", "probability": value},
                    ],
                }
            )
        histories.append({"history": outer_history, "strategies": rows})
    return {
        "schemaVersion": "solvers.multiway-average-sampling-research/v1",
        "solverStateVersion": 4,
        "result": {"histories": histories},
    }


def expect_value_error(fn):
    try:
        fn()
    except ValueError:
        return
    raise AssertionError("expected ValueError")


def test_weighted_metrics_and_top_diffs():
    out = cmp.compare(ref(), research())
    assert all(node["weighted_mae"] == 0 for node in out["nodes"])
    assert all(node["weighted_rmse"] == 0 for node in out["nodes"])
    assert all(node["local_menu_match"] for node in out["nodes"])
    assert all(node["global_model_equivalence_unverified"] for node in out["nodes"])


def test_reference_and_research_order_are_not_used_for_class_mapping():
    reference = ref()
    reference["nodes"] = [
        dict(node, raise_by_hand=dict(reversed(list(node["raise_by_hand"].items()))))
        for node in reversed(reference["nodes"])
    ]
    value = research()
    value["result"]["histories"] = list(reversed(value["result"]["histories"]))
    out = cmp.compare(reference, value)
    assert all(node["weighted_mae"] == 0 for node in out["nodes"])


def test_bucket_zero_and_one_keep_their_content_addressed_values():
    value = research()
    rows = value["result"]["histories"][0]["strategies"]
    rows[0]["actions"][1]["probability"] = 1.0
    rows[0]["actions"][0]["probability"] = 0.0
    rows[1]["actions"][1]["probability"] = 0.0
    rows[1]["actions"][0]["probability"] = 1.0
    out = cmp.compare(ref(), value)
    assert out["nodes"][0]["weighted_mae"] > 0
    assert out["nodes"][0]["top_hand_differences"][0]["hand"] in ("AA", "AKs")


def test_combo_weights_and_position_specific_raise_sizes():
    reference, value = ref(), research()
    sizes = (2000, 2000, 2300, 2500, 3000)
    for node, history, size in zip(reference["nodes"], value["result"]["histories"], sizes):
        node["menu"][1] = f"raise-to:{size}"
        for row in history["strategies"]:
            row["actions"][1]["action"] = f"raise-to:{size}"
    # AA has six physical combinations; change only its UTG probability by 1.
    row = value["result"]["histories"][0]["strategies"][0]
    row["actions"][0]["probability"] = 0.0
    row["actions"][1]["probability"] = 1.0
    out = cmp.compare(reference, value, strict_menu=True)
    assert math.isclose(out["nodes"][0]["weighted_mae"], 6 / 1326)
    assert math.isclose(out["nodes"][0]["weighted_rmse"], math.sqrt(6 / 1326))
    assert all(node["weighted_mae"] == 0 for node in out["nodes"][1:])


def test_row_menu_difference_and_strict_mode_fail_closed():
    value = research()
    actions = value["result"]["histories"][0]["strategies"][0]["actions"]
    actions[0]["probability"] = 0.25
    actions[1]["probability"] = 0.5
    actions.append({"action": "call:500", "probability": 0.25})
    out = cmp.compare(ref(), value)
    assert out["nodes"][0]["local_menu_match"] is False
    assert out["nodes"][0]["comparable"] is False
    expect_value_error(lambda: cmp.compare(ref(), value, strict_menu=True))


def test_duplicate_and_malformed_buckets_fail_closed():
    duplicate = research()
    duplicate["result"]["histories"][0]["strategies"][1]["key"]["bucket_path"][0] = 0
    expect_value_error(lambda: cmp.compare(ref(), duplicate))
    malformed = research()
    malformed["result"]["histories"][0]["strategies"][0]["key"]["bucket_path"] = []
    expect_value_error(lambda: cmp.compare(ref(), malformed))


def test_history_player_state_and_tail_contracts_fail_closed():
    mismatch = research()
    mismatch["result"]["histories"][0]["strategies"][0]["key"]["history"][0] = 9
    expect_value_error(lambda: cmp.compare(ref(), mismatch))
    wrong_state = research()
    wrong_state["solverStateVersion"] = 3
    expect_value_error(lambda: cmp.compare(ref(), wrong_state))
    wrong_active = research()
    wrong_active["result"]["histories"][1]["strategies"][0]["key"]["active_opponents"] = 99
    expect_value_error(lambda: cmp.compare(ref(), wrong_active))


def test_reference_positions_and_probability_values_fail_closed():
    duplicate_position = ref()
    duplicate_position["nodes"][1]["position"] = "UTG"
    expect_value_error(lambda: cmp.compare(duplicate_position, research()))
    bad_target = ref()
    bad_target["nodes"][0]["raise_by_hand"]["AA"] = float("nan")
    expect_value_error(lambda: cmp.compare(bad_target, research()))
    bad_probability = research()
    bad_probability["result"]["histories"][0]["strategies"][0]["actions"][1]["probability"] = True
    expect_value_error(lambda: cmp.compare(ref(), bad_probability))


def test_draw_schema_preserves_strategy_metrics_and_requires_valid_metadata():
    value = research()
    expected = cmp.compare(ref(), value)
    value["schemaVersion"] = cmp.DRAW_RESEARCH_SCHEMA
    meta = {"kind": "draw-aware", "baseTableBuckets": {"flop": 32, "turn": 32, "river": 128},
            "effectiveBuckets": {"flop": 128, "turn": 128, "river": 128}, "transformVersion": "draw-flags/v1"}
    value["researchAbstraction"] = meta
    actual = cmp.compare(ref(), value)
    assert actual["nodes"] == expected["nodes"]
    assert actual["research_abstraction"] == meta
    for field, replacement in (("kind", "unknown"), ("transformVersion", "draw-flags/v0"),
                               ("effectiveBuckets", {"flop": 32, "turn": 32, "river": 128}),
                               ("baseTableBuckets", {"flop": True, "turn": 32, "river": 128})):
        bad = deepcopy(value)
        bad["researchAbstraction"][field] = replacement
        expect_value_error(lambda: cmp.compare(ref(), bad))
    mislabeled = deepcopy(value)
    mislabeled["schemaVersion"] = cmp.RESEARCH_SCHEMA
    expect_value_error(lambda: cmp.compare(ref(), mislabeled))
