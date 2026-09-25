"""Validator tests with hand-transcribed tree rows, not solver-run evidence."""

import copy
from decimal import Decimal
import unittest

import check_diagnostic as check


def fixture_rows():
    # Independent transcription in engine units/labels. The script under
    # test must translate the observed BB-labelled menus to these rows.
    entries = [
        ("", "oop", 4050, ["check", "bet 1350", "bet 5500"]),
        ("x", "ip", 4050, ["check", "bet 1350", "bet 5500"]),
        ("r1350", "ip", 5400, ["fold", "call", "raise to 3700", "raise to 5500"]),
        ("r1350r3700", "oop", 9100, ["fold", "call", "raise to 5500"]),
        ("r1350r3700r5500", "ip", 13250, ["fold", "call"]),
        ("xr1350", "oop", 5400, ["fold", "call", "raise to 3700", "raise to 5500"]),
        ("xr1350r3700", "ip", 9100, ["fold", "call", "raise to 5500"]),
        ("xr1350r3700r5500", "oop", 13250, ["fold", "call"]),
        ("r5500", "ip", 9550, ["fold", "call"]),
        ("r1350r5500", "oop", 10900, ["fold", "call"]),
        ("xr5500", "oop", 9550, ["fold", "call"]),
        ("xr1350r5500", "ip", 10900, ["fold", "call"]),
    ]
    return [{"history": history, "actor": actor, "pot": pot, "actions": actions,
             "street": "river", "stored": True} for history, actor, pot, actions in entries]


class DiagnosticChecks(unittest.TestCase):
    def setUp(self):
        self.observed = check.read_json(check.HERE / "observed.json")

    def test_config_preserves_copied_range_identity(self):
        result = check.check_config(check.HERE / "diagnostic.toml", self.observed)
        self.assertEqual(result["ranges"]["oop"]["positive_combos"], 130)
        self.assertEqual(result["ranges"]["ip"]["raw_weight_sum"], "0.4475484")

    def test_all_observed_menus_match_explicit_transcription(self):
        result = check.check_tree(fixture_rows(), self.observed)
        self.assertEqual(result["decision_nodes"], 12)
        self.assertEqual(result["terminal_nodes"], 21)
        self.assertEqual(result["public_nodes"], 33)

    def test_rejects_missing_duplicate_and_altered_menus(self):
        for field, value in [("actor", "ip"), ("pot", 4051), ("stored", False),
                             ("actions", ["check", "bet 1350", "bet 5499"]),
                             ("actions", ["bet 1350", "check", "bet 5500"])]:
            with self.subTest(field=field, value=value):
                rows = fixture_rows()
                rows[0][field] = value
                with self.assertRaises(ValueError):
                    check.check_tree(rows, self.observed)
        with self.assertRaises(ValueError):
            check.check_tree(fixture_rows()[:-1], self.observed)
        with self.assertRaises(ValueError):
            check.check_tree(fixture_rows() + [fixture_rows()[0]], self.observed)
        observed = copy.deepcopy(self.observed)
        observed["observed_menus"][2]["actions"].append("raise to 40")
        with self.assertRaises(ValueError):
            check.expected_tree(observed)

    def test_ev_units_do_not_become_reference_acceptance(self):
        _, counts = check.expected_tree(self.observed)
        summary = {"board": "Qs 7h 2c 4d 9s", "pot": 4050, "effective_stack": 5500,
                   "min_bet": 100, "nodes": 33, "stored_nodes": 12, "streets_stored": "full",
                   "iterations": 300, "storage": "f32", "ev_oop": 1600, "ev_ip": 2390,
                   "expl_oop": 0.04, "expl_ip": 0.06, "nash_conv": 0.1}
        result = check.compare_summary(summary, self.observed, counts)
        self.assertEqual(result["root_ev"]["oop"]["signed_difference_bb"], Decimal("-0.58"))
        self.assertEqual(result["root_ev"]["ip"]["signed_difference_bb"], Decimal("0.58"))
        self.assertEqual(result["solver_nash_conv_bb"], Decimal("0.001"))
        self.assertEqual(result["condition_match"], "unverified")
        self.assertEqual(result["quality_status"], "not_evaluated")
        self.assertIsNone(result["acceptance"])
        self.assertIsNone(result["comparison_threshold"])
        summary["nodes"] = 34
        with self.assertRaises(ValueError):
            check.compare_summary(summary, self.observed, counts)


if __name__ == "__main__":
    unittest.main()
