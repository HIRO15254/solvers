"""Negative controls for partial capture integrity; no browser or solver."""
import unittest
from unittest.mock import patch

import check_menus as checker


class CaptureValidation(unittest.TestCase):
    def test_missing_roots_cannot_be_closed_graph(self):
        observed = {"case_id": "HU-R0-002", "board": ["Ks", "7h", "2d", "3c", "8d"],
                    "condition_match": "unverified", "quality_status": "not_evaluated",
                    "acceptance": None, "observed_menus": []}
        with patch.object(checker.Path, "glob", return_value=[]):
            with self.assertRaisesRegex(ValueError, "both captured"):
                checker.assemble(observed)

    def test_physical_allin_duplicates_rejected_in_both_formats(self):
        for raw in ("R2|97.5|97.5,RAI", "R2|BTN|97.5|F,C,R97.5,RAI97.5"):
            with self.subTest(raw=raw), self.assertRaisesRegex(ValueError, "physical duplicates"):
                checker.parse_record(raw)

    def test_numeric_allin_history_alias_is_explicitly_rejected(self):
        with self.assertRaisesRegex(ValueError, "history alias"):
            checker.parse_record("R97.5|BTN|97.5|F,C")

    def test_equal_decimal_raise_targets_are_physical_duplicates(self):
        for raw in ("R2|97.5|4,4.0,RAI", "R2|BTN|97.5|F,C,R4,Raise4.00,Allin97.5"):
            with self.subTest(raw=raw), self.assertRaisesRegex(ValueError, "physical duplicates"):
                checker.parse_record(raw)
        with self.assertRaisesRegex(ValueError, "history alias"):
            checker.parse_record("R2.0|BTN|97.5|F,C")
        result = checker.parse_record("R2|BTN|97.5|F,C,R97.50(40%)")
        self.assertEqual(result[-1], {"allin 97.5": 40})

    def test_wrong_actor_and_remaining_are_rejected(self):
        for raw in ("X-R2-R7|BB|95.5|F,C,Allin97.5", "X-R2-R7|BTN|97.5|F,C,Allin97.5"):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                checker.parse_record(raw)

    def test_minimum_full_raise_checked_with_short_allin_exception(self):
        for raw in ("R2|97.5|3,RAI", "R2-R7|95.5|8,RAI", "R2-R3|BB|95.5|F,C",
                    "R0.5|BTN|97.5|F,C"):
            with self.subTest(raw=raw), self.assertRaisesRegex(ValueError, "minimum full raise"):
                checker.parse_record(raw)
        checker.parse_record("R2-R7-R17-R50.5|BB|80.5|F,C,Allin97.5")
        checker.parse_record("R2-R7-R23.5-R68-RAI|BTN|29.5|F,C")

    def test_conflicting_percent_labels_rejected(self):
        row = {"actions": ["fold", "call", "allin 97.5"],
               "action_ui_percent_labels": {"allin 97.5": 40},
               "additional_menu_observations": [{"action_ui_percent_labels": {"allin 97.5": 40}}]}
        checker.check_percentages(row, {"allin 97.5": 40})
        with self.assertRaisesRegex(ValueError, "conflicts"):
            checker.check_percentages(row, {"allin 97.5": 41})
        legacy = {"actions": row["actions"], "extra_ui_labels": {"allin_97_5_percent": 40}}
        with self.assertRaisesRegex(ValueError, "conflicts"):
            checker.check_percentages(legacy, {"allin 97.5": 41})

    def test_check_counts_towards_actor_parity_and_literal_percent_preserved(self):
        h, _, actor, remaining, actions, labels = checker.parse_record("X-R2-R7-R17-R37|BTN|80.5|F,C,Allin97.5(76%)")
        self.assertEqual((h, actor, remaining), ("X-R2-R7-R17-R37", 1, 80.5))
        self.assertEqual(actions, ["fold", "call", "allin 97.5"])
        self.assertEqual(labels, {"allin 97.5": 76})


if __name__ == "__main__":
    unittest.main()
