"""Offline mutations of transferred graph facts and their retained checks."""

from copy import deepcopy
from decimal import Decimal
import json
import unittest
from unittest.mock import patch

import check_menus as checker


class MenuChecks(unittest.TestCase):
    def setUp(self):
        self.record = checker.read_json(checker.HERE / "menus.json")
        self.packed = deepcopy(self.record["packed"])
        self.observed = checker.read_json(checker.HERE / "observed.json")

    def check(self):
        return checker.validate_graph(self.packed, self.observed)

    def row(self, history):
        return next(row for row in self.packed["rows"] if row[3] == history)

    def test_actual_transfer_closure_and_counts(self):
        raw = checker.compact_bytes(self.packed)
        self.assertEqual(len(raw), 6963)
        self.assertEqual(checker.fnv1a32(raw), "05b8ce21")
        result = self.check()
        self.assertEqual((result["decision_observations"], result["unique_menus"], result["action_edges"],
                          result["decision_edges"], result["derived_terminals"]), (72, 18, 212, 71, 141))
        self.assertEqual(result["terminal_kind_counts"], {"call": 70, "check_check": 1, "fold": 70})
        self.assertEqual((result["maximum_decision_depth"], result["maximum_terminal_depth"]), (5, 6))
        self.assertTrue(all(not item["terminal_ui_visited"] and not item["settlement_observed"]
                            for item in result["derived_terminal_table"]))

    def test_independent_actor_stack_replay_and_short_allins(self):
        # Integer half-bb accounting; independent of the Decimal replay helper.
        short = 0
        for _, title, _, history, menu in self.packed["rows"]:
            paid, last_raise = [0, 0], 0
            tokens = history.split("-") if history else []
            for index, token in enumerate(tokens):
                if token == "X":
                    continue
                total = 172 if token == "RAI" else int(Decimal(token[1:]) * 2)
                last_raise = total - max(paid)
                paid[index % 2] = total
            self.assertEqual(Decimal(title.split()[1]) * 2, 172 - paid[len(tokens) % 2])
            for identifier, _ in self.packed["menus"][menu]:
                if "_RAI_" in identifier and 172 - max(paid) < last_raise:
                    short += 1
        self.assertEqual(short, 8)

    def test_missing_nonterminal_child_rejected(self):
        self.packed["rows"].remove(self.row("R3-R16-R38-RAI"))
        with self.assertRaisesRegex(ValueError, "missing decision child"):
            self.check()

    def test_duplicate_history_rejected(self):
        self.packed["rows"].append(deepcopy(self.packed["rows"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate decision history"):
            self.check()

    def test_unreferenced_and_duplicate_menu_rejected(self):
        extra = deepcopy(self.packed["menus"][0])
        self.packed["menus"].append(extra)
        with self.assertRaisesRegex(ValueError, "duplicate deduplicated menu"):
            self.check()
        extra[-1][-1] = "Allin 86 (283%)"
        with self.assertRaisesRegex(ValueError, "unreferenced menu"):
            self.check()

    def test_orphan_observation_rejected(self):
        self.packed["rows"].append(["hs_14_river_BB_active", "BB 82", "14", "R4-RAI", 4])
        # It is a syntactically valid state but not an observed root action.
        with self.assertRaisesRegex(ValueError, "orphan"):
            self.check()

    def test_terminal_history_cannot_become_decision(self):
        for history in ("X-X", "R3-C", "RAI-F"):
            with self.subTest(history=history), self.assertRaisesRegex(ValueError, "terminal"):
                checker.state(history)

    def test_selected_card_actor_spot_and_stack_rejected(self):
        for column, value, message in [(0, "hs_13_river_BB_active", "selected card"),
                                       (2, "14", "selected card"), (1, "BTN 85", "visible actor stack")]:
            with self.subTest(column=column):
                original = deepcopy(self.packed)
                self.row("R3")[column] = value
                with self.assertRaisesRegex(ValueError, message):
                    self.check()
                self.packed = original

    def test_action_index_text_and_amount_rejected(self):
        for value in (["hspotcrd_action_R3_2", "Bet 3 (10%)"],
                      ["hspotcrd_action_R3_1", "Raise 3 (10%)"],
                      ["hspotcrd_action_R3_1", "Bet 4 (10%)"]):
            with self.subTest(value=value):
                self.packed["menus"][0][1] = value
                with self.assertRaisesRegex(ValueError, "action id/index|text/amount"):
                    self.check()

    def test_raise_to_cannot_be_amount_added_or_exceed_stack(self):
        for history in ("R3-R16-R15", "R3-X", "R87", "R86"):
            with self.subTest(history=history), self.assertRaises(ValueError):
                checker.state(history)
        _, _, paid = checker.state("R3-R16-R38-RAI")
        self.assertEqual(paid, [Decimal(38), Decimal(86)])

    def test_fold_call_required_allin_has_no_raise(self):
        self.packed["menus"][4] = [["hspotcrd_action_C_0", "Call"]]
        with self.assertRaisesRegex(ValueError, "required fold/call"):
            self.check()

    def test_game_url_and_empty_root_parameter_rejected(self):
        base = self.packed["base_urls"][0]
        for changed in (base.replace("depth=100", "depth=75"), base + "&history_spot=12",
                        base + "&depth=100"):
            with self.subTest(url=changed):
                self.packed["base_urls"][0] = changed
                with self.assertRaisesRegex(ValueError, "base URL"):
                    self.check()
        self.packed["base_urls"][0] = base
        self.packed["rows"][0][3] = ""
        with self.assertRaisesRegex(ValueError, "absent, not empty"):
            self.check()

    def test_ui_percent_label_has_no_invented_rounding_rule(self):
        self.packed["menus"][0][1][1] = "Bet 3 (11%)"
        # Graph structure still makes sense, but raw transfer check detects any such edit.
        self.check()
        self.assertNotEqual(checker.fnv1a32(checker.compact_bytes(self.packed)), checker.COPY_FNV)

    def test_recorded_transfer_changes_fail_before_graph_acceptance(self):
        self.record["packed"]["menus"][0][1][1] = "Bet 3 (11%)"
        original = checker.read_json
        with patch.object(checker, "read_json", side_effect=lambda p: self.record if p.name == "menus.json" else original(p)):
            with self.assertRaisesRegex(ValueError, "packed transfer mismatch"):
                checker.calculate()

    def test_capture_bound_and_quality_cannot_be_silently_promoted(self):
        original = checker.read_json
        self.record["capture_completed_before_utc"] = None
        with patch.object(checker, "read_json", side_effect=lambda p: self.record if p.name == "menus.json" else original(p)):
            with self.assertRaisesRegex(ValueError, "capture bound"):
                checker.calculate()
        self.observed["acceptance"] = True
        with patch.object(checker, "read_json", side_effect=lambda p: self.observed if p.name == "observed.json" else original(p)):
            with self.assertRaisesRegex(ValueError, "must not certify quality"):
                checker.calculate()

    def test_retained_result_matches_all_pins_and_derivation(self):
        expected = json.loads((checker.HERE / "menu-check.json").read_text(encoding="utf-8"))
        self.assertEqual(expected, checker.calculate())
        self.assertIsNone(expected["acceptance"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
