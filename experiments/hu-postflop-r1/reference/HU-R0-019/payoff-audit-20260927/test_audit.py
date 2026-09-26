"""Small independent graph/arithmetic regressions; no Rust or solver execution."""

import copy
from decimal import Decimal as D
import json
import tomllib
import unittest

import audit


class PayoffAuditTests(unittest.TestCase):
    def setUp(self):
        self.observed = json.loads((audit.CASE / "observed.json").read_text(), parse_float=D)
        self.config = tomllib.loads((audit.CASE / "diagnostic.toml").read_text(), parse_float=D)

    def rows(self):
        return audit.derive(self.observed, self.config)[0]

    def test_all_terminal_histories_against_hand_written_contributions(self):
        # Independent literal fixture: (BB paid, BTN paid, folder or None).
        expected = {
            "check / check": (0, 0, None),
            "check / bet 13.5 / fold": (0, 1350, "BB"),
            "check / bet 13.5 / call": (1350, 1350, None),
            "check / bet 13.5 / raise to 37 / fold": (3700, 1350, "BTN"),
            "check / bet 13.5 / raise to 37 / call": (3700, 3700, None),
            "check / bet 13.5 / raise to 37 / allin 55 / fold": (3700, 5500, "BB"),
            "check / bet 13.5 / raise to 37 / allin 55 / call": (5500, 5500, None),
            "check / bet 13.5 / allin 55 / fold": (5500, 1350, "BTN"),
            "check / bet 13.5 / allin 55 / call": (5500, 5500, None),
            "check / allin 55 / fold": (0, 5500, "BB"),
            "check / allin 55 / call": (5500, 5500, None),
            "bet 13.5 / fold": (1350, 0, "BTN"),
            "bet 13.5 / call": (1350, 1350, None),
            "bet 13.5 / raise to 37 / fold": (1350, 3700, "BB"),
            "bet 13.5 / raise to 37 / call": (3700, 3700, None),
            "bet 13.5 / raise to 37 / allin 55 / fold": (5500, 3700, "BTN"),
            "bet 13.5 / raise to 37 / allin 55 / call": (5500, 5500, None),
            "bet 13.5 / allin 55 / fold": (1350, 5500, "BB"),
            "bet 13.5 / allin 55 / call": (5500, 5500, None),
            "allin 55 / fold": (5500, 0, "BTN"),
            "allin 55 / call": (5500, 5500, None),
        }
        actual = {row["history"]: tuple(int(row["contribution_from_root"]["chips"][s])
                  for s in audit.SEATS) + (row["folder"],) for row in self.rows()}
        self.assertEqual(actual, expected)

    def test_outcome_values_from_independent_simplified_formulas(self):
        for row in self.rows():
            c = [int(row["contribution_from_root"]["chips"][s]) for s in audit.SEATS]
            for value in row["outcomes"]:
                if row["folder"]:
                    folder = audit.SEATS.index(row["folder"])
                    want = [0, 0]
                    want[folder] = -c[folder]
                    want[1 - folder] = 3990 + c[folder]
                elif value["outcome"] == "tie":
                    want = [1995, 1995]
                elif value["outcome"] == "win_BB":
                    want = [3990 + c[0], -c[1]]
                else:
                    want = [-c[0], 3990 + c[1]]
                self.assertEqual([D(value["public_subgame_utility"]["chips"][s]) for s in audit.SEATS], want)
                self.assertEqual([D(value["solver_utility"]["chips"][s]) for s in audit.SEATS],
                                 [x - 2025 for x in want])

    def test_decision_origin_uses_pre_action_contribution_not_final_payment(self):
        row = next(r for r in self.rows() if r["history"] == "bet 13.5 / raise to 37 / call")
        win = next(v for v in row["outcomes"] if v["outcome"] == "win_BB")
        self.assertEqual(D(win["public_subgame_utility"]["chips"]["BB"]), 7690)
        self.assertEqual(D(win["decision_origin_utility"]["chips"]["BB"]), 9040)
        self.assertEqual(D(row["decision_origin_minus_public_offset"]["chips"]["BB"]), 1350)

    def test_refund_is_not_lost_from_matched_payoff(self):
        row = next(r for r in self.rows() if r["history"] == "allin 55 / fold")
        self.assertEqual(row["matched_pot_chips"], 4050)
        self.assertEqual(row["terminal_pot_chips"], 9550)
        self.assertEqual(row["uncalled_refund_if_matched"]["chips"], {"BB": "5500", "BTN": "0"})
        self.assertEqual(D(row["outcomes"][0]["conditional_matched_public_utility"]["chips"]["BB"]), 3990)

    def test_reject_changed_actor(self):
        self.observed["observed_menus"][3]["actor"] = "BTN"
        with self.assertRaisesRegex(ValueError, "actor contradicts"):
            self.rows()

    def test_reject_missing_or_duplicate_decision(self):
        original = copy.deepcopy(self.observed)
        self.observed["observed_menus"].pop()
        with self.assertRaisesRegex(ValueError, "missing/revisited"):
            self.rows()
        self.observed = original
        self.observed["observed_menus"].append(copy.deepcopy(original["observed_menus"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate history"):
            self.rows()

    def test_reject_impossible_actions(self):
        for action, pattern in [("call", "without outstanding"), ("bet 56", "invalid wager"),
                                ("allin 54", "not stack total"), ("raise to 37", "wager label")]:
            with self.subTest(action=action):
                original = copy.deepcopy(self.observed)
                self.observed["observed_menus"][0]["actions"][0] = action
                with self.assertRaisesRegex(ValueError, pattern):
                    self.rows()
                self.observed = original

    def test_reject_terminal_with_decision_child_and_orphan(self):
        original = copy.deepcopy(self.observed)
        self.observed["observed_menus"].append({"history": "check / check", "actor": "BB", "actions": ["check"]})
        with self.assertRaisesRegex(ValueError, "decision follows terminal"):
            self.rows()
        self.observed = original
        self.observed["observed_menus"].append({"history": "bet 42", "actor": "BTN", "actions": ["fold", "call"]})
        with self.assertRaisesRegex(ValueError, "unreachable"):
            self.rows()

    def test_reject_changed_economics_and_fractional_chip(self):
        self.config["rake"]["cap"] = 61
        with self.assertRaisesRegex(ValueError, "fixed diagnostic rake"):
            self.rows()
        with self.assertRaisesRegex(ValueError, "exact integer chip"):
            audit.chips("0.001")

    def test_pins_and_nonacceptance_scope(self):
        result = audit.build()
        self.assertFalse(result["external_conditions_confirmed"])
        self.assertFalse(result["external_quality_evaluated"])
        self.assertEqual(result["counts"]["terminal_nodes"], 21)


if __name__ == "__main__":
    unittest.main(verbosity=2)
