"""Hand-transcribed validator fixtures; these are not actual solver evidence."""

import copy
from decimal import Decimal
import unittest
from unittest import mock

import check_diagnostic as check


def fixture_rows():
    entries = [
        ("", "oop", 2050, ["check", "bet 700", "bet 2050", "bet 6500"]),
        ("x", "ip", 2050, ["check", "bet 700", "bet 2050", "bet 6500"]),
        ("r700", "ip", 2750, ["fold", "call", "raise to 1900", "raise to 2600", "raise to 6500"]),
        ("r700r1900", "oop", 4650, ["fold", "call", "raise to 3950", "raise to 6500"]),
        ("r700r1900r3950", "ip", 7900, ["fold", "call", "raise to 6500"]),
        ("r700r1900r3950r6500", "oop", 12500, ["fold", "call"]),
        ("r700r1900r6500", "ip", 10450, ["fold", "call"]),
        ("r700r2600", "oop", 5350, ["fold", "call", "raise to 6500"]),
        ("r700r2600r6500", "ip", 11150, ["fold", "call"]),
        ("r700r6500", "oop", 9250, ["fold", "call"]),
        ("r2050", "ip", 4100, ["fold", "call", "raise to 4200", "raise to 6500"]),
        ("r2050r4200", "oop", 8300, ["fold", "call", "raise to 6500"]),
        ("r2050r4200r6500", "ip", 12750, ["fold", "call"]),
        ("r2050r6500", "oop", 10600, ["fold", "call"]),
        ("r6500", "ip", 8550, ["fold", "call"]),
        ("xr700", "oop", 2750, ["fold", "call", "raise to 1900", "raise to 2600", "raise to 6500"]),
        ("xr700r1900", "ip", 4650, ["fold", "call", "raise to 3950", "raise to 6500"]),
        ("xr700r1900r3950", "oop", 7900, ["fold", "call", "raise to 6500"]),
        ("xr700r1900r3950r6500", "ip", 12500, ["fold", "call"]),
        ("xr700r1900r6500", "oop", 10450, ["fold", "call"]),
        ("xr700r2600", "ip", 5350, ["fold", "call", "raise to 6500"]),
        ("xr700r2600r6500", "oop", 11150, ["fold", "call"]),
        ("xr700r6500", "ip", 9250, ["fold", "call"]),
        ("xr2050", "oop", 4100, ["fold", "call", "raise to 4200", "raise to 6500"]),
        ("xr2050r4200", "ip", 8300, ["fold", "call", "raise to 6500"]),
        ("xr2050r4200r6500", "oop", 12750, ["fold", "call"]),
        ("xr2050r6500", "ip", 10600, ["fold", "call"]),
        ("xr6500", "oop", 8550, ["fold", "call"]),
    ]
    return [{"history": history, "actor": actor, "pot": pot, "actions": actions,
             "street": "river", "stored": True} for history, actor, pot, actions in entries]


class DiagnosticChecks(unittest.TestCase):
    def setUp(self):
        self.observed = check.read_json(check.HERE / "observed.json")

    def test_copied_range_identity_and_fixed_diagnostic_budget(self):
        result = check.check_config(check.HERE / "diagnostic.toml", self.observed)
        self.assertEqual(result["ranges"]["oop"]["positive_combos"], 352)
        self.assertEqual(result["ranges"]["ip"]["raw_weight_sum"], "53.4459435")
        raw = (check.HERE / "diagnostic.toml").read_text()
        for before, after in (("target_nash_conv = 0.1", "target_nash_conv = 0.2"),
                              ('max_time = "30s"', 'max_time = "60s"'),
                              ("threads = 1", "threads = 8"),
                              ("to_call == 1200", "to_call == 1900")):
            with self.subTest(after=after):
                fake = mock.Mock()
                fake.read_text.return_value = raw.replace(before, after)
                with self.assertRaises(ValueError):
                    check.check_config(fake, self.observed)

    def test_complete_observed_graph_matches_explicit_tree_transcription(self):
        result = check.check_tree(fixture_rows(), self.observed)
        self.assertEqual(result["decision_nodes"], 28)
        self.assertEqual(result["decision_action_edges"], 80)
        self.assertEqual(result["terminal_nodes"], 53)
        self.assertEqual(result["public_nodes"], 81)

    def test_missing_duplicate_reordered_and_changed_export_rows_rejected(self):
        for field, value in (("actor", "ip"), ("pot", 2051), ("stored", False),
                             ("actions", ["check", "bet 701", "bet 2050", "bet 6500"]),
                             ("actions", ["bet 700", "check", "bet 2050", "bet 6500"])):
            with self.subTest(field=field, value=value):
                rows = fixture_rows()
                rows[0][field] = value
                with self.assertRaises(ValueError):
                    check.check_tree(rows, self.observed)
        for rows in (fixture_rows()[:-1], fixture_rows() + [fixture_rows()[0]]):
            with self.assertRaises(ValueError):
                check.check_tree(rows, self.observed)

    def test_missing_child_disconnected_history_actor_and_call_amount_rejected(self):
        for mode in ("missing", "disconnected", "actor", "call", "frontier", "quality"):
            with self.subTest(mode=mode):
                observed = copy.deepcopy(self.observed)
                if mode == "missing":
                    observed["observed_menus"][0]["actions"].append("bet 8")
                elif mode == "disconnected":
                    observed["observed_menus"][-1]["history"] = "bet 8/allin 65"
                elif mode == "actor":
                    observed["observed_menus"][0]["actor"] = "BTN"
                elif mode == "call":
                    observed["observed_menus"][-1]["call_remaining_bb"] = 45
                elif mode == "frontier":
                    observed["unobserved_menu_frontier"]["histories"] = ["bet 8"]
                else:
                    observed["quality_status"] = "pass"
                with self.assertRaises(ValueError):
                    check.expected_tree(observed)

    def test_ev_units_and_summary_counts_never_certify_reference_quality(self):
        _, counts = check.expected_tree(self.observed)
        summary = {"board": "Kh Kc 5s 2d 3s", "pot": 2050, "effective_stack": 6500,
                   "min_bet": 100, "nodes": 81, "stored_nodes": 28, "streets_stored": "full",
                   "iterations": 300, "storage": "f32", "ev_oop": 800, "ev_ip": 1190,
                   "expl_oop": 0.04, "expl_ip": 0.06, "nash_conv": 0.1}
        result = check.compare_summary(summary, self.observed, counts)
        self.assertEqual(result["root_ev"]["oop"]["signed_difference_bb"], Decimal("0.32"))
        self.assertEqual(result["root_ev"]["ip"]["signed_difference_bb"], Decimal("-0.32"))
        self.assertEqual(result["solver_nash_conv_bb"], Decimal("0.001"))
        self.assertEqual(result["condition_match"], "unverified")
        self.assertEqual(result["quality_status"], "not_evaluated")
        self.assertIsNone(result["acceptance"])
        self.assertIsNone(result["comparison_threshold"])
        for key, bad in (("nodes", 82), ("stored_nodes", 27), ("ev_oop", "NaN")):
            with self.subTest(key=key):
                changed = {**summary, key: bad}
                with self.assertRaises(ValueError):
                    check.compare_summary(changed, self.observed, counts)


if __name__ == "__main__":
    unittest.main()
