"""Lightweight static fixture checks, not native solver or quality evidence."""

from copy import deepcopy
from decimal import Decimal
import tomllib
import unittest

import check_diagnostic as checker


class DiagnosticChecks(unittest.TestCase):
    def setUp(self):
        self.config = tomllib.loads((checker.HERE / "diagnostic.toml").read_text(encoding="utf-8"), parse_float=Decimal)
        self.observed = checker.check_ranges.read_json(checker.HERE / "observed.json")
        self.graph = checker.check_menus.calculate()

    def test_all_72_observed_menus_and_ranges_match(self):
        result = checker.calculate()
        graph = result["static_graph_check"]
        self.assertEqual({key: graph[key] for key in ("decision_nodes", "action_edges", "terminal_nodes", "public_nodes")},
                         {"decision_nodes": 72, "action_edges": 212, "terminal_nodes": 141, "public_nodes": 213})
        self.assertEqual(result["range_counts"], {"oop": 150, "ip": 86})
        self.assertEqual(graph["short_allin_edges"], 8)
        self.assertEqual(len(graph["active_rule_hits"]), 9)
        self.assertTrue(all(graph["active_rule_hits"]))

    def test_hand_transcribed_nodes_check_raise_to_not_added_amount(self):
        decisions, _, _, _ = checker.model_tree(self.config)
        expected = {
            "": ("BB", [0, 0], 3050, 8600, ["X", "R3", "R10.5", "R18", "R25.5", "R46", "RAI"]),
            "R3-R16": ("BB", [300, 1600], 4950, 8300, ["F", "C", "R38", "R50.5", "RAI"]),
            "X-R3-R16": ("BTN", [1600, 300], 4950, 8300, ["F", "C", "R38", "R50.5", "RAI"]),
            "R10.5-R28.5": ("BB", [1050, 2850], 6950, 7550, ["F", "C", "R59", "RAI"]),
            "R46": ("BTN", [4600, 0], 7650, 8600, ["F", "C", "RAI"]),
            "R46-RAI": ("BB", [4600, 8600], 16250, 4000, ["F", "C"]),
        }
        for history, fields in expected.items():
            row = decisions[history]
            self.assertEqual(tuple(row[key] for key in ("actor", "contributions_chips", "pot_chips", "remaining_stack_chips", "actions")), fields)

    def test_condition_amount_order_and_missing_rule_changes_rejected(self):
        source = self.config["game"]["tree"]["script"]
        changes = [("to_call == 1300", "to_call == 1600"), ("3800c", "3900c"),
                   ("when aggressions == 2 && to_call == 1800", "when aggressions == 3 && to_call == 1800"),
                   ("  replace raise [a]\n", ""), ("[5400c, a]", "[a]"),
                   ("[300c, 1050c, 1800c, 2550c, 4600c, a]", "[300c, a]")]
        for before, after in changes:
            with self.subTest(before=before):
                config = deepcopy(self.config)
                config["game"]["tree"]["script"] = source.replace(before, after)
                with self.assertRaises(ValueError):
                    checker.compare_graph(config, self.graph)

    def test_unsupported_dsl_is_rejected_not_approximated(self):
        for source in ("", "turn {\n replace bet [300c]\n}", "river {\n add bet [300c]\n}",
                       "river {\n replace bet [10]\n}", "river {\n when history == X { replace bet [a] }\n}"):
            with self.subTest(source=source), self.assertRaises(ValueError):
                checker.parse_script(source)

    def test_raw_range_reorder_or_weight_edits_rejected(self):
        for seat in ("oop", "ip"):
            field = seat + "_range"
            for variant in (self.config["game"][field] + " ", ",".join(reversed(self.config["game"][field].split(",")))):
                config = deepcopy(self.config)
                config["game"][field] = variant
                with self.assertRaisesRegex(ValueError, "embedded range bytes"):
                    checker.check_config(config, self.observed)

    def test_config_shape_limits_and_no_quality_target(self):
        changes = [("game", "pot", 2850), ("game", "effective_stack", 8500), ("game", "pot", 3050.0),
                   ("game", "min_bet", 50), ("game", "iso_merging", True), ("game", "board", "Qd 9h 4s 2c Jh"),
                   ("run", "threads", 4), ("run", "threads", True), ("run", "target_nash_conv", Decimal(".1")),
                   ("run", "max_time", "60s"), ("rake", "cap", 60), ("rake", "rounding", "down")]
        for section, key, value in changes:
            with self.subTest(section=section, key=key):
                config = deepcopy(self.config)
                config[section][key] = value
                with self.assertRaises(ValueError):
                    checker.check_config(config, self.observed)

    def test_external_source_or_hidden_tree_changes_rejected(self):
        for key, value in (("source", "elsewhere.tree"), ("params", {"x": 1}), ("allin_threshold", Decimal(".85")),
                           ("include_allin", True), ("max_aggressive_actions", {"flop": 0, "turn": 0, "river": 3})):
            with self.subTest(key=key):
                config = deepcopy(self.config)
                config["game"]["tree"][key] = value
                with self.assertRaises(ValueError):
                    checker.check_config(config, self.observed)

    def test_missing_reordered_or_wrong_actor_observation_rejected(self):
        for kind in ("missing", "order", "actor", "stack"):
            graph = deepcopy(self.graph)
            if kind == "missing":
                graph["decision_nodes"].pop()
            elif kind == "order":
                graph["decision_nodes"][0]["actions"].reverse()
            elif kind == "actor":
                graph["decision_nodes"][0]["actor"] = "BTN"
            else:
                graph["decision_nodes"][0]["remaining_actor_stack_bb"] = "85"
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                checker.compare_graph(self.config, graph)

    def test_call_matches_contributions_and_check_check_keeps_pot(self):
        _, terminals, _, _ = checker.model_tree(self.config)
        self.assertEqual(terminals["R3-R16-C"]["contributions_chips"], [1600, 1600])
        self.assertEqual(terminals["R46-RAI-C"]["contributions_chips"], [8600, 8600])
        self.assertEqual(terminals["X-X"]["contributions_chips"], [0, 0])

    def test_total_versus_matched_rake_is_not_equivalent(self):
        _, terminals, _, _ = checker.model_tree(self.config)
        result = checker.rake_comparison(terminals)
        by_history = {row["history"]: row for row in result["all_terminals"]}
        expected = {"R3-F": (Decimal("167.5"), Decimal("152.5"), Decimal(15)),
                    "RAI-F": (Decimal(400), Decimal("152.5"), Decimal("247.5")),
                    "X-X": (Decimal("152.5"), Decimal("152.5"), Decimal(0))}
        for history, values in expected.items():
            self.assertEqual(tuple(Decimal(by_history[history][key]) for key in
                ("total_rake_chips", "conditional_matched_rake_chips", "difference_chips")), values)
        self.assertTrue(all(Decimal(row["difference_chips"]) == 0 for row in result["all_terminals"] if row["kind"] != "fold"))
        self.assertGreater(result["terminals_with_different_rake"], 0)

    def test_static_result_explicitly_excludes_native_and_external_acceptance(self):
        result = checker.calculate()
        for key in ("native_dsl_validation", "native_tree_export_comparison", "solve"):
            self.assertEqual(result[key], "not_executed")
        self.assertEqual(result["purpose"], "diagnostic_only")
        self.assertEqual(result["condition_match"], "unverified")
        self.assertEqual(result["quality_status"], "not_evaluated")
        self.assertIsNone(result["acceptance"])
        self.assertIsNone(result["comparison_threshold"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
