"""Actual frozen capture integrity plus clearly separated synthetic mutations."""

from copy import deepcopy
from decimal import Decimal
import unittest
from unittest.mock import patch
from urllib.parse import parse_qsl, urlsplit

import check_menus as checker


def fixture():
    observed = checker.read_json(checker.HERE / "observed.json")
    menu = [["hspotcrd_action_X_0", "Check"], ["hspotcrd_action_R2_1", "Bet 2 (33%)"],
            ["hspotcrd_action_R4.5_2", "Bet 4.5 (75%)"], ["hspotcrd_action_R9_3", "Bet 9 (150%)"],
            ["hspotcrd_action_RAI_4", "Allin 97 (1617%)"]]
    response = [["hspotcrd_action_F_0", "Fold"], ["hspotcrd_action_C_1", "Call"],
                ["hspotcrd_action_RAI_2", "Allin 97 (100%)"]]
    rows = []
    for start in ("", "X"):
        bettor = 0 if not start else 1
        rows.append([start, 10 + bool(start), ("SB", "BB")[bettor] + " 97", 0, "X-R2-RAI"])
        for action, amount in (("R2", Decimal(2)), ("R4.5", Decimal("4.5")), ("R9", Decimal(9)), ("RAI", Decimal(97))):
            path = "-".join(filter(None, (start, action)))
            spot = 11 + bool(start)
            rows.append([path, spot, ("SB", "BB")[1 - bettor] + " 97", 2 if action == "RAI" else 1,
                         path if action == "RAI" else path + "-RAI"])
            if action != "RAI":
                rows.append([path + "-RAI", spot + 1, f"{('SB', 'BB')[bettor]} {97 - amount}", 2, path + "-RAI"])
    return {"case_id": checker.CASE, "schema": checker.SCHEMA, "base_url": observed["url"],
            "capture_window": {"after_utc": checker.CAPTURE_AFTER, "completed_before_utc": checker.CAPTURE_BEFORE,
                               "exact_per_node_times": None, "method": "Synthetic test graph, not a captured menu record"},
            "menu_sets": [menu, response, response[:2]], "rows": rows}, observed


class MenuChecks(unittest.TestCase):
    def setUp(self):
        self.document, self.observed = fixture()

    def check(self):
        return checker.analyze(self.document, self.observed)

    def test_synthetic_complete_graph_counts_derive_without_final_capture_count(self):
        result = self.check()
        self.assertEqual((result["decision_nodes"], result["action_edges"], result["derived_terminal_nodes"]), (16, 44, 29))
        self.assertEqual(result["terminal_kind_counts"], {"call": 14, "fold": 14, "check_check": 1})
        self.assertEqual(result["public_nodes"], 45)
        self.assertGreater(result["rows_with_unselected_suffix"], 0)
        self.assertTrue(all(not row["terminal_ui_visited"] and not row["settlement_observed"] for row in result["derived_terminals"]))

    def test_root_nonempty_suffix_and_empty_root_history_are_both_valid(self):
        actor, paid, suffix = checker.selected_state("", 10, "X-R2-R7-R17-R51")
        self.assertEqual((actor, paid, suffix), (0, [0, 0], ["X", "R2", "R7", "R17", "R51"]))
        self.assertEqual(checker.selected_state("", 10, ""), (0, [0, 0], []))

    def test_selected_prefix_of_deeper_raw_history_and_cumulative_raise(self):
        actor, paid, suffix = checker.selected_state("X-R2-R7-R17", 14, "X-R2-R7-R17-R51")
        self.assertEqual((actor, paid, suffix), (0, [7, 17], ["R51"]))
        self.assertEqual(checker.selected_state("R9-RAI", 12, "R9-RAI")[1], [9, 97])

    def test_bad_depth_spot_type_and_prefix_are_rejected(self):
        for args in (("X-R2", 11, "X-R2"), ("X-R2", 12, "X"), ("R2", 11, "R9"),
                     ("", "10", ""), ("", 9, ""), ("X", 11, None), ("X", 11, "X-")):
            with self.subTest(args=args), self.assertRaises(ValueError):
                checker.selected_state(*args)

    def test_terminal_or_illegal_selected_prefix_rejected(self):
        for path in ("R2-F", "R2-C", "X-X", "R9-X", "R9-R7", "R98", "R97"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                checker.selected_state(path, 10 + len(path.split("-")), path)

    def test_url_keeps_suffix_but_replaces_spot_and_canonical_url_strips_it(self):
        result = self.check()
        root = next(row for row in result["decision_nodes_table"] if row["path"] == "")
        raw = parse_qsl(urlsplit(root["observed_url_reconstructed"]).query)
        selected = parse_qsl(urlsplit(root["canonical_selected_url"]).query)
        self.assertEqual([value for key, value in raw if key == "history_spot"], ["10"])
        self.assertEqual(dict(raw)["river_actions"], "X-R2-RAI")
        self.assertNotIn("river_actions", dict(selected))

    def test_url_suffix_never_supplies_missing_child_observation(self):
        self.document["rows"] = [row for row in self.document["rows"] if row[0] != "X-R2-RAI"]
        with self.assertRaisesRegex(ValueError, "missing observed nonterminal child"):
            self.check()

    def test_duplicate_path_and_orphan_are_rejected(self):
        original = deepcopy(self.document)
        self.document["rows"].append(self.document["rows"][0][:])
        with self.assertRaisesRegex(ValueError, "duplicate selected"):
            self.check()
        self.document = original
        self.document["rows"].append(["R3-RAI", 12, "SB 94", 2, "R3-RAI"])
        with self.assertRaisesRegex(ValueError, "orphan"):
            self.check()

    def test_title_action_id_text_and_duplicate_tokens_rejected(self):
        self.document["rows"][0][2] = "BB 97"
        with self.assertRaisesRegex(ValueError, "title"):
            self.check()
        self.document, _ = fixture()
        for entry in (["hspotcrd_action_R2_2", "Bet 2 (33%)"], ["hspotcrd_action_R2_1", "Bet 3 (33%)"],
                      ["hspotcrd_action_R2_1", "Raise 2 (33%)"]):
            self.document["menu_sets"][0][1] = entry
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                self.check()
        self.document, _ = fixture()
        self.document["menu_sets"][0][2] = ["hspotcrd_action_R2_2", "Bet 2 (33%)"]
        with self.assertRaisesRegex(ValueError, "duplicate menu action"):
            self.check()

    def test_short_allin_and_literal_percentages(self):
        result = checker.decode_menu(self.document["menu_sets"][1], 0, [Decimal(10), Decimal(70)], "R10-R70")
        self.assertEqual(result[-1], ["RAI", None])
        self.document["menu_sets"][1][-1][-1] = "Allin 97 (12345%)"
        self.check()  # Label is retained literally, not treated as an exact size formula.

    def test_full_raise_boundary_uses_prior_increment_not_added_contribution(self):
        good = "R2-R14.5-R27"
        self.assertEqual(checker.selected_state(good, 13, good)[1], [Decimal(27), Decimal("14.5")])
        for bad in ("R2-R3.99", "R2-R14.5-R26.99", "X-R2-R14.5-R26.99"):
            with self.subTest(path=bad), self.assertRaisesRegex(ValueError, "minimum full"):
                checker.selected_state(bad, 10 + len(bad.split("-")), bad)

    def test_deep_short_allin_97_below_102_is_allowed_but_96_5_is_not(self):
        prefix = "R2-R14.5-R32-R67"
        actor, paid, increment = checker.live_state(checker.tokens(prefix))
        self.assertEqual((actor, paid, increment), (0, [Decimal(32), Decimal(67)], Decimal(35)))
        self.assertEqual(checker.decode_menu(self.document["menu_sets"][1], actor, paid, prefix)[-1], ["RAI", None])
        history = prefix + "-RAI"
        self.assertEqual(checker.selected_state(history, 15, history)[1], [Decimal(97), Decimal(67)])
        invalid_menu = deepcopy(self.document["menu_sets"][1])
        invalid_menu[-1] = ["hspotcrd_action_R96.5_2", "Raise 96.5 (100%)"]
        with self.assertRaisesRegex(ValueError, "minimum full"):
            checker.decode_menu(invalid_menu, actor, paid, prefix)
        with self.assertRaisesRegex(ValueError, "minimum full"):
            checker.selected_state(prefix + "-R96.5", 15, prefix + "-R96.5")

    def test_initial_one_bb_assumption_and_menu_minimum_reject_subminimum(self):
        self.assertEqual(checker.selected_state("R1", 11, "R1")[1], [Decimal(1), Decimal(0)])
        with self.assertRaisesRegex(ValueError, "minimum full"):
            checker.selected_state("R0.99", 11, "R0.99")
        menu = [["hspotcrd_action_X_0", "Check"], ["hspotcrd_action_R0.99_1", "Bet 0.99 (16%)"]]
        with self.assertRaisesRegex(ValueError, "minimum full"):
            checker.decode_menu(menu, 0, [Decimal(0), Decimal(0)], "")

    def test_menu_actor_and_paid_are_bound_to_replayed_history(self):
        with self.assertRaisesRegex(ValueError, "contradicts selected history"):
            checker.decode_menu(self.document["menu_sets"][1], 1, [Decimal(70), Decimal(10)], "R10-R70")

    def test_final_transfer_pin_is_unclaimed_until_supplied(self):
        raw = checker.compact_bytes(self.document)
        with patch.object(checker, "CAPTURE_PIN", None):
            self.assertEqual(checker.transfer_check(self.document)["browser_pin_comparison"], "not_supplied")
        with patch.object(checker, "CAPTURE_PIN", (len(raw), checker.fnv1a32(raw))):
            self.assertEqual(checker.transfer_check(self.document)["browser_pin_comparison"], "verified")
            altered = deepcopy(self.document)
            altered["rows"][0][4] = ""
            with self.assertRaisesRegex(ValueError, "transfer pin"):
                checker.transfer_check(altered)

    def test_actual_capture_roundtrip_transfer_and_complete_graph(self):
        raw = (checker.HERE / "menus.json").read_bytes()
        document = checker.read_json(checker.HERE / "menus.json")
        self.assertEqual(raw, checker.compact_bytes(document) + b"\n")
        self.assertEqual((len(raw) - 1, checker.fnv1a32(raw[:-1])), (10717, "a2150af3"))
        result = checker.calculate()
        self.assertEqual((result["decision_nodes"], result["unique_menus"], result["action_edges"],
                          result["decision_edges"], result["derived_terminal_nodes"], result["public_nodes"]),
                         (120, 27, 356, 119, 237, 357))
        self.assertEqual(result["terminal_kind_counts"], {"call": 118, "fold": 118, "check_check": 1})
        self.assertEqual((result["maximum_decision_depth"], result["maximum_terminal_depth"],
                          result["maximum_aggressions"], result["short_allin_edges"]), (6, 7, 5, 4))
        self.assertEqual(result["transfer"]["browser_pin_comparison"], "verified")
        self.assertEqual([r[4] for r in document["rows"][:5]], ["X-R2-R7-R17-R51"] * 5)
        self.assertEqual([r["unselected_url_suffix"] for r in result["decision_nodes_table"][:5]],
                         [["X", "R2", "R7", "R17", "R51"], ["R2", "R7", "R17", "R51"],
                          ["R7", "R17", "R51"], ["R17", "R51"], ["R51"]])
        self.assertIsNone(result["acceptance"])
        self.assertEqual(result["native_validation"], "not_executed")
        self.assertEqual(result["solve"], "not_executed")

    def test_capture_time_shape_order_and_unsupported_precision_rejected(self):
        original = self.document["capture_window"]
        changes = [("after_utc", "2026-09-26T21:55:42+00:00"), ("after_utc", "2026-09-26T22:15:00Z"),
                   ("completed_before_utc", "2026-09-27T22:14:54Z"), ("completed_before_utc", "2026-09-26T25:14:54Z"),
                   ("exact_per_node_times", []), ("method", "")]
        for key, value in changes:
            window = deepcopy(original)
            window[key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                checker.validate_capture_window(window)

    def test_changed_base_or_external_quality_claim_rejected(self):
        self.document["base_url"] += "&river_actions=X"
        with self.assertRaisesRegex(ValueError, "base URL"):
            self.check()
        self.document, _ = fixture()
        self.observed["acceptance"] = True
        with self.assertRaisesRegex(ValueError, "external quality"):
            self.check()


if __name__ == "__main__":
    unittest.main(verbosity=2)
