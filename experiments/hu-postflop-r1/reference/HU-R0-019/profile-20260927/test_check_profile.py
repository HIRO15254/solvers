"""Synthetic copy evidence tests; no browser, solver, or external data acquisition."""
import copy
from fractions import Fraction as F
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import check_profile as c


class ArithmeticTests(unittest.TestCase):
    def test_scientific_and_tiny_values_remain_exact(self):
        self.assertEqual(c.rational("5.63e-10"), F(563, 10**12))
        self.assertEqual(c.rational("0.082884370422") / c.rational("0.0996"), F(13814061737, 16600000000))
        self.assertEqual(c.rational("-0"), 0)

    def test_malformed_nonfinite_and_large_exponents_rejected(self):
        for text in ["NaN", "Inf", "-Infinity", "1_000", "1e-1001", "", "0x01"]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                c.rational(text)

    def test_range_physical_cards_and_weight_validation(self):
        for text in [b"AsAh:.2,AhAs:.3", b"AsAs:.2", b"QsAh:.1", b"AsAh:-.1", b"AsAh:1.1", b"AsAh:NaN"]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                c.parse_range(text, {"Qs"})
        self.assertEqual(c.parse_range(b"", set()), {})
        self.assertEqual(c.parse_range(b"AsAh:0", set()), {"AhAs": F(0)})

    def test_zero_is_not_a_ratio(self):
        self.assertEqual(c.conditional_bounds(F(0), [F(0), F(0)], F(0))["reason"],
                         "only_zero_whole_ratio_undefined")
        result = c.conditional_bounds(F(0), [F(0), F(0)], F(1, 100))
        self.assertTrue(result["feasible"])
        self.assertIsNone(result["probability_intervals"])
        self.assertEqual(result["reason"], "whole_may_be_zero_ratio_not_identified")

    def test_rounding_intersection_boundary(self):
        # W in [.45,.55], two actions each in [.15,.25]: overlap [.45,.50].
        self.assertTrue(c.conditional_bounds(F(1, 2), [F(1, 5)] * 2, F(1, 20))["feasible"])
        self.assertFalse(c.conditional_bounds(F(1, 2), [F(1, 10)] * 2, F(1, 20))["feasible"])
        # Closed endpoint W=.4 and sum(A)=.4 is feasible.
        self.assertTrue(c.conditional_bounds(F(1, 2), [F(1, 10)] * 2, F(1, 10))["feasible"])

    def test_probability_enclosures_cover_independent_finite_grid(self):
        epsilon = F(1, 10)
        for whole in [F(1, 5), F(1, 2), F(4, 5)]:
            for a in [F(0), F(1, 5), F(1, 2)]:
                copied = [a, whole - a] if whole >= a else [a, F(0)]
                result = c.conditional_bounds(whole, copied, epsilon)
                for i in range(11):
                    for j in range(11):
                        true = [F(i, 10), F(j, 10)]
                        total = sum(true)
                        if total == 0 or abs(total - whole) > epsilon or any(abs(x-y) > epsilon for x,y in zip(true,copied)):
                            continue
                        self.assertTrue(result["feasible"])
                        if result["probability_intervals"] is None:
                            continue
                        for value, pair in zip(true, result["probability_intervals"]):
                            low, high = [F(int(x["numerator"]), int(x["denominator"])) for x in pair]
                            self.assertLessEqual(low, value / total)
                            self.assertGreaterEqual(high, value / total)

    def test_three_four_actions_clamping_and_tiny_scale(self):
        for copied, error, actual in [
                ([F(0), F(0), F(1)], F(1,100), [F(1,200), F(1,200), F(99,100)]),
                ([F(1,4)] * 4, F(1,100), [F(6,25), F(13,50), F(1,4), F(1,4)]),
                ([F(1,10**12)] * 4, F(1,10**13), [F(1,10**12)] * 4)]:
            whole = sum(copied)
            result = c.conditional_bounds(whole, copied, error)
            self.assertTrue(result["feasible"])
            for value, pair in zip(actual, result["probability_intervals"]):
                low, high = [F(int(x["numerator"]), int(x["denominator"])) for x in pair]
                self.assertLessEqual(low, value / sum(actual))
                self.assertGreaterEqual(high, value / sum(actual))

    def test_nearest_own_action_for_both_seats_at_depth(self):
        path = ["check", "bet 13.5", "raise to 37", "allin 55"]
        menus = {" / ".join(path[:i]): {"actor": ["BB", "BTN"][i % 2]} for i in range(5)}
        self.assertEqual(c.prior_own_action(" / ".join(path[:3]), "BTN", menus),
                         ("check", "bet 13.5"))
        self.assertEqual(c.prior_own_action(" / ".join(path), "BB", menus),
                         ("check / bet 13.5", "raise to 37"))


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="profile-test-", dir=Path(__file__).parent)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.serial = 0
        # Deliberately small synthetic closed tree, not a claim about actual menus.
        self.observed = {"case_id": "HU-R0-019", "board": ["Qs", "7h", "2c", "4d", "9s"],
            "seats": {"oop": "BB", "ip": "BTN"}, "observed_menus": [
                {"history": "", "actor": "BB", "actions": ["bet 13.5", "allin 55"]},
                {"history": "bet 13.5", "actor": "BTN", "actions": ["call", "raise to 37"]},
                {"history": "bet 13.5 / raise to 37", "actor": "BB", "actions": ["fold", "call"]},
                {"history": "allin 55", "actor": "BTN", "actions": ["fold", "call"]}]}
        self.manifest = {"schema": c.SCHEMA, "case_id": "HU-R0-019",
            "observed": self.file(json.dumps(self.observed).encode()),
            "root_ranges": {"oop": self.file(b"AsAh:.4"), "ip": self.file(b"KcKd:.5")},
            "captures": [
                self.node("", "BB", b"AsAh:.4", [("bet 13.5", b"AsAh:.1"), ("allin 55", b"AsAh:.3")]),
                self.node("bet 13.5", "BTN", b"KcKd:.5", [("call", b"KcKd:.2"), ("raise to 37", b"KcKd:.3")]),
                self.node("bet 13.5 / raise to 37", "BB", b"AsAh:.1", [("fold", b"AsAh:.02"), ("call", b"AsAh:.08")]),
                self.node("allin 55", "BTN", b"KcKd:.5", [("fold", b"KcKd:.1"), ("call", b"KcKd:.4")])]}

    def file(self, data):
        self.serial += 1
        path = f"raw-{self.serial}.txt"
        (self.root / path).write_bytes(data)
        return {"path": path, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}

    def node(self, history, actor, whole, actions):
        return {"history": history, "actor": actor, "whole": self.file(whole),
                "actions": [{"label": label, "range": self.file(raw)} for label, raw in actions]}

    def check(self, error=None):
        return c.audit(self.manifest, self.root, error)

    def row(self, node=0, error=None):
        return self.check(error)["nodes"][node]["rows"][0]

    def test_products_not_multiplied_twice(self):
        report = self.check()
        self.assertEqual(report["captured_nodes"], 4)
        row = report["nodes"][2]["rows"][0]
        self.assertTrue(row["prior_own_product_comparison"]["exact_equal"])
        self.assertEqual(row["whole"], c.number(F(1, 10)))
        self.assertEqual(row["literal_product_ratios"], [c.number(F(1,5)), c.number(F(4,5))])
        self.assertEqual(report["interpretation"]["quality_status"], "not_evaluated")
        self.assertIsNone(report["interpretation"]["acceptance"])

    def test_locally_closing_child_with_different_own_reach_is_visible(self):
        node = self.manifest["captures"][2]
        node["whole"] = self.file(b"AsAh:.2")
        for a in node["actions"]:
            a["range"] = self.file(b"AsAh:.1")
        report = self.check(F(5,10**13))["nodes"][2]
        row = report["rows"][0]
        self.assertEqual(row["classification"], "literal_products_close")
        self.assertEqual(row["prior_own_product_comparison"]["prior_weight"], c.number(F(1,10)))
        self.assertEqual(row["prior_own_product_comparison"]["whole_minus_prior"], c.number(F(1,10)))
        summary = report["own_reach_comparison_summary"]
        self.assertEqual(summary["comparable_combos"], 1)
        self.assertEqual(summary["different_combos"], 1)
        self.assertEqual(summary["conditional_nonoverlap_combos"], 1)
        self.assertEqual(summary["max_absolute_difference"], c.number(F(1,10)))

    def test_missing_token_is_not_explicit_zero(self):
        action = self.manifest["captures"][0]["actions"][0]
        action["range"] = self.file(b"")
        self.assertEqual(self.row()["classification"], "action_token_missing_no_zero_inference")
        inventory = self.check()["nodes"][0]["token_inventory"]["bet 13.5"]
        self.assertEqual(inventory["copy_content_class"], "empty_copy")
        self.assertEqual(inventory["token_count"], 0)
        self.assertNotIn("bet 13.5", self.check()["nodes"][0]["missing_action_files"])
        action["range"] = self.file(b"AsAh:0")
        row = self.row()
        self.assertEqual(row["classification"], "nonclosing_literal_products_no_renormalization")
        self.assertEqual(row["missing_actions"], [])
        self.assertIsNone(row["literal_product_ratios"])

    def test_missing_action_file_and_whole_are_reported(self):
        self.manifest["captures"][0]["actions"].pop()
        self.assertEqual(self.check()["nodes"][0]["missing_action_files"], ["allin 55"])
        self.manifest["captures"][0]["whole"] = None
        self.assertEqual(self.row()["classification"], "whole_missing_no_zero_inference")
        self.manifest["captures"].pop()
        self.assertEqual(self.check()["captured_nodes"], 3)

    def test_no_silent_normalization_and_conditional_model_separate(self):
        self.manifest["captures"][0]["actions"][0]["range"] = self.file(b"AsAh:.100000000001")
        row = self.row(error=F(5,10**13))
        self.assertEqual(row["sum_actions_minus_whole"], c.number(F(1,10**12)))
        self.assertIsNone(row["literal_product_ratios"])
        self.assertTrue(row["conditional_rounding_model"]["feasible"])
        self.assertNotIn("conditional_rounding_model", self.row())

    def test_explicit_whole_zero_remains_unidentified(self):
        node = self.manifest["captures"][2]
        node["whole"] = self.file(b"AsAh:0")
        for a in node["actions"]:
            a["range"] = self.file(b"AsAh:0")
        row = self.row(2, F(1,100))
        self.assertEqual(row["classification"], "explicit_zero_whole")
        self.assertFalse(row["prior_own_product_comparison"]["exact_equal"])
        self.assertIsNone(row["literal_product_ratios"])
        self.assertIsNone(row["conditional_rounding_model"]["probability_intervals"])

    def test_positive_outside_root_support_is_a_gap(self):
        self.manifest["captures"][0]["whole"] = self.file(b"AsAh:.4,KcKd:.1")
        inventory = self.check()["nodes"][0]["token_inventory"]["whole"]
        self.assertEqual(inventory["positive_outside_root_support"], ["KcKd"])

    def test_identity_tamper_and_path_escape_rejected(self):
        ref = self.manifest["captures"][0]["whole"]
        (self.root / ref["path"]).write_bytes(b"AsAh:.3")
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            self.check()
        ref["path"] = "../outside.txt"
        with self.assertRaisesRegex(ValueError, "escapes"):
            self.check()

    def test_supplemental_raw_is_hashed_without_policy_inference(self):
        ref = self.file(b"AsAh:.123")
        self.manifest["supplemental_files"] = [ref]
        report = self.check()
        self.assertIn(ref, report["raw_files"])
        self.assertEqual(report["captured_nodes"], 4)
        (self.root / ref["path"]).write_bytes(b"AsAh:.124")
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            self.check()

    def test_actor_labels_duplicates_and_duplicate_json_rejected(self):
        original = copy.deepcopy(self.manifest)
        self.manifest["captures"][0]["actor"] = "BTN"
        with self.assertRaisesRegex(ValueError, "actor mismatch"):
            self.check()
        self.manifest = copy.deepcopy(original)
        self.manifest["captures"][0]["actions"][0]["label"] = "wrong"
        with self.assertRaisesRegex(ValueError, "action label"):
            self.check()
        self.manifest = copy.deepcopy(original)
        self.manifest["captures"].append(self.manifest["captures"][0])
        with self.assertRaisesRegex(ValueError, "duplicate captured"):
            self.check()
        with self.assertRaisesRegex(ValueError, "duplicate JSON"):
            c.load_json(b'{"a":1,"a":2}')

    def test_only_local_not_global_interval_claim(self):
        # Both BTN roots share the same latent own reach. Individually overlapping
        # broad root intervals do not prove all shared-variable constraints jointly.
        report = self.check(F(1, 10))
        self.assertIn("no claim of a globally feasible", report["interpretation"]["constraint_scope"])
        self.assertIn("joint profile reach zero does not remove BR", report["interpretation"]["off_policy_br"])
        self.assertNotIn("br_ready", report)


if __name__ == "__main__":
    unittest.main()
