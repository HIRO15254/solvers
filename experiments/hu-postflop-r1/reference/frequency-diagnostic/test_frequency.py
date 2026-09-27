"""Small synthetic arithmetic and missing-data regressions; no native execution."""
from __future__ import annotations

import copy
from fractions import Fraction as Q
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("frequency_diagnostic", HERE / "frequency.py")
f = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(f)


def fixture():
    return f.decode((HERE / "synthetic-input.json").read_bytes())


def value(item):
    return None if item is None else Q(int(item["numerator"]), int(item["denominator"]))


def run(record=None):
    return f.diagnose(fixture() if record is None else record, HERE)


class FrequencyTests(unittest.TestCase):
    def test_common_weight_not_own_weight(self):
        result = run()
        self.assertEqual(result["status"], "computed")
        self.assertEqual(value(result["coverage"]["reference_mass"]), 4)
        self.assertEqual(value(result["coverage"]["own_mass"]), 5)
        self.assertEqual(value(result["range_weighted"]["tv_pp"]), Q(225, 4))
        for action in result["range_weighted"]["actions"]:
            self.assertEqual(value(action["absolute_difference_pp"]), Q(75, 4))
        self.assertEqual(result["quality_status"], "not_evaluated")
        self.assertIsNone(result["acceptance"])
        self.assertIsNone(result["comparison_threshold"])

    def test_aggregate_cancellation_does_not_hide_tv(self):
        record = fixture()
        record["profiles"]["reference"]["reach"]["17"]["actor"] = ".2"
        record["w_ref"]["17"] = "1"
        record["profiles"]["reference"]["policy"]["0"] = {"check": "0", "bet_to:100": "1"}
        record["profiles"]["reference"]["policy"]["17"] = {"check": "1", "bet_to:100": "0"}
        result = run(record)
        self.assertEqual(value(result["range_weighted"]["tv_pp"]), 100)
        self.assertEqual([value(a["absolute_difference_pp"]) for a in result["range_weighted"]["actions"]], [0, 0])
        self.assertEqual(value(result["hand_action"]["maximum_observed_difference_pp"]), 100)

    def test_missing_positive_action_is_not_explicit_zero(self):
        record = fixture()
        del record["profiles"]["own"]["policy"]["0"]["bet_to:100"]
        result = run(record)
        self.assertEqual(result["status"], "not_evaluated")
        self.assertIsNone(result["range_weighted"])
        self.assertEqual(value(result["coverage"]["excluded_positive_mass"]), 1)
        self.assertEqual(value(result["coverage"]["unavailable_policy_reference_mass"]), 1)
        self.assertEqual(result["hand_action"]["candidate_count"], 4)
        self.assertEqual(result["hand_action"]["compared_count"], 2)
        self.assertFalse(result["hand_action"]["complete"])
        record["profiles"]["own"]["policy"]["0"]["bet_to:100"] = "0"
        self.assertEqual(run(record)["status"], "computed")

    def test_null_missing_and_absent_policy_are_equivalent_missingness(self):
        record = fixture()
        record["profiles"]["reference"]["policy"]["0"] = None
        a = run(record)
        del record["profiles"]["reference"]["policy"]["0"]
        self.assertEqual(a, run(record))

    def test_zero_compatible_reference_hands_are_na(self):
        result = run()
        self.assertEqual(result["coverage"]["zero_reference_hands"], [42, 1325])
        self.assertEqual([r["status"] for r in result["hands"]], ["computed", "computed", "not_applicable", "not_applicable"])
        self.assertEqual(result["hand_action"]["candidate_count"], 4)
        self.assertEqual(value(result["coverage"]["cutoff_excluded_positive_mass"]), 0)

    def test_all_zero_node_has_no_zero_valued_diagnostic(self):
        record = fixture()
        for h in record["support"]:
            record["profiles"]["reference"]["reach"][str(h)]["actor"] = "0"
            record["w_ref"][str(h)] = "0"
        result = run(record)
        self.assertEqual(result["status"], "not_applicable")
        self.assertIsNone(result["range_weighted"])
        self.assertIsNone(result["hand_action"]["maximum_observed_difference_pp"])

    def test_explicit_own_policy_at_zero_own_reach_uses_reference_measure(self):
        record = fixture()
        record["profiles"]["own"]["reach"]["0"]["actor"] = "0"
        result = run(record)
        self.assertTrue(result["hands"][0]["own_zero_reach"])
        self.assertEqual(value(result["range_weighted"]["tv_pp"]), Q(225, 4))
        del record["profiles"]["own"]["policy"]["0"]
        self.assertEqual(run(record)["status"], "not_evaluated")

    def test_unknown_own_reach_is_separate_from_common_reference_measure(self):
        record = fixture()
        record["profiles"]["own"]["reach"]["0"] = None
        result = run(record)
        self.assertEqual(result["status"], "computed")
        self.assertIsNone(result["coverage"]["own_mass"])
        self.assertEqual(result["coverage"]["missing_own_mass_hands"], [0])

    def test_unknown_reference_mass_never_renormalizes_known_subset(self):
        record = fixture()
        record["profiles"]["reference"]["reach"]["0"]["compatible_opponent"] = None
        result = run(record)
        self.assertEqual(result["status"], "not_evaluated")
        self.assertIsNone(result["coverage"]["reference_mass"])
        self.assertIsNone(result["coverage"]["excluded_positive_mass"])
        self.assertIsNone(result["range_weighted"])
        self.assertIsNone(result["hand_action"]["candidate_count"])

    def test_weight_factors_must_match_including_chance(self):
        record = fixture()
        record["chance_weight"]["0"] = "1"
        with self.assertRaisesRegex(ValueError, "joint mass"):
            run(record)

    def test_no_low_mass_cutoff(self):
        record = fixture()
        record["profiles"]["reference"]["reach"]["42"] = {"actor": "1e-320", "compatible_opponent": "2"}
        record["w_ref"]["42"] = "1e-320"
        record["profiles"]["own"]["policy"]["42"] = {"check": "1", "bet_to:100": "0"}
        record["profiles"]["reference"]["policy"]["42"] = {"check": "0", "bet_to:100": "1"}
        result = run(record)
        self.assertEqual(result["coverage"]["positive_reference_hands"], 3)
        self.assertGreater(value(result["coverage"]["reference_mass"]), 4)
        self.assertEqual(value(result["hand_action"]["maximum_observed_difference_pp"]), 100)

    def test_action_order_is_irrelevant_but_set_mismatch_is_not(self):
        record = fixture()
        record["profiles"]["own"]["actions"].reverse()
        self.assertEqual(run(record)["range_weighted"], run()["range_weighted"])
        record["profiles"]["own"]["actions"].append("raise_to:200")
        result = run(record)
        self.assertEqual(result["action_sets"]["own_only"], ["raise_to:200"])
        self.assertIsNone(result["range_weighted"])
        self.assertEqual(result["hand_action"]["compared_count"], 0)

    def test_scope_and_missing_provenance_block_local_metrics(self):
        changes = [("game_sha256", "f" * 64), ("node_id", "another/node"), ("actor", "IP"), ("evidence", None)]
        for field, changed in changes:
            with self.subTest(field=field):
                record = fixture()
                record["profiles"]["own"][field] = changed
                result = run(record)
                self.assertEqual(result["status"], "not_evaluated")
                self.assertEqual(result["hand_action"]["compared_count"], 0)
                self.assertEqual(value(result["coverage"]["comparable_reference_mass"]), 0)
                self.assertEqual(value(result["coverage"]["excluded_positive_mass"]), 4)
        record = fixture()
        record["condition_match"] = "mismatch"
        self.assertIsNone(run(record)["range_weighted"])

    def test_top20_counts_and_tie_order(self):
        record = fixture()
        record["support"] = list(range(11))
        record["w_ref"] = {str(h): "1" for h in record["support"]}
        record["chance_weight"] = {str(h): "1" for h in record["support"]}
        for side, first in (("own", "0"), ("reference", "1")):
            record["profiles"][side]["reach"] = {str(h): {"actor": "1", "compatible_opponent": "1"} for h in record["support"]}
            record["profiles"][side]["policy"] = {str(h): {"check": first, "bet_to:100": str(1 - int(first))} for h in record["support"]}
        result = run(record)["hand_action"]
        self.assertEqual((result["candidate_count"], result["compared_count"], result["display_count"]), (22, 22, 20))
        self.assertEqual([(v["combo_id"], v["action_id"]) for v in result["top20"]], [(h, a) for h in range(10) for a in ("bet_to:100", "check")])

    def test_rounding_unknown_and_declared_bounds(self):
        self.assertIsNone(run()["range_weighted"]["rounding_only_tv_lower_bound_pp"])
        record = fixture()
        record["profiles"]["own"]["rounding"] = {"mode": "nearest", "quantum": ".02"}
        record["profiles"]["reference"]["rounding"] = {"mode": "nearest", "quantum": ".04"}
        result = run(record)
        self.assertEqual(value(result["range_weighted"]["rounding_only_tv_lower_bound_pp"]), Q(213, 4))
        self.assertEqual(value(result["range_weighted"]["actions"][0]["rounding_only_lower_bound_pp"]), Q(63, 4))
        self.assertEqual(value(result["hand_action"]["top20"][0]["own_display_interval"]["lower"]), Q(-1, 100))
        self.assertIsNone(result["numeric_error_upper_bound_pp"])

    def test_nonstochastic_or_non_nearest_input_is_not_repaired(self):
        record = fixture()
        record["profiles"]["own"]["policy"]["0"]["check"] = ".999"
        with self.assertRaisesRegex(ValueError, "sum exactly"):
            run(record)
        record = fixture()
        record["profiles"]["own"]["rounding"] = {"mode": "truncate", "quantum": ".01"}
        with self.assertRaisesRegex(ValueError, "nearest"):
            run(record)

    def test_exact_quantized_rational_policy(self):
        record = fixture()
        row = {"check": {"numerator": "1", "denominator": "65535"}, "bet_to:100": {"numerator": "65534", "denominator": "65535"}}
        for side in ("own", "reference"):
            record["profiles"][side]["policy"]["0"] = copy.deepcopy(row)
            record["profiles"][side]["policy"]["17"] = copy.deepcopy(row)
            record["profiles"][side]["strategy_kind"] = "stored_quantized"
        self.assertEqual(value(run(record)["range_weighted"]["tv_pp"]), 0)

    def test_evidence_tamper_and_escape_are_rejected(self):
        for key, changed in (("sha256", "0" * 64), ("bytes", 1), ("path", "../outside.json"), ("path", "C:/outside.json")):
            with self.subTest(key=key, changed=changed):
                record = fixture()
                record["support_evidence"][key] = changed
                with self.assertRaises(ValueError):
                    run(record)

    def test_invalid_shapes_duplicates_and_nonfinite_numbers(self):
        for raw in (b'{"a":1,"a":2}', b'{"x":NaN}', b'{"x":Infinity}'):
            with self.assertRaises(ValueError):
                f.decode(raw)
        for bad in ("NaN", "Infinity", "1e999", 0.5, True, {"numerator": "1", "denominator": "0"}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                f.number(bad)
        for changed in ([0, 0], [17, 0], [1326], [True]):
            record = fixture()
            record["support"] = changed
            with self.assertRaises(ValueError):
                run(record)
        record = fixture()
        record["profiles"]["own"]["policy"]["0"]["unknown"] = "0"
        with self.assertRaisesRegex(ValueError, "unknown policy action"):
            run(record)


if __name__ == "__main__":
    unittest.main(verbosity=2)
