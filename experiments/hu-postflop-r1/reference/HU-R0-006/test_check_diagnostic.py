"""Static regressions and a separate half-BB BFS; no native solver execution."""

from collections import deque
from copy import deepcopy
from decimal import Decimal
from fractions import Fraction
import tomllib
import unittest

import check_diagnostic as checker


def independent_half_bb_tree():
    """Fixed menu transcription, no production checker parser/model helpers.

    Units are half BB, not the checker's 100 chips. Breadth-first traversal uses
    immutable tuples; targets must already be legal (no clamp/dedup repair).
    """
    targets = {(1, 4): (14, 21, 29), (1, 9): (24, 35), (1, 18): (42, 59),
               (2, 10): (34, 48), (2, 15): (54, 75), (2, 17): (48, 67),
               (2, 24): (90, 124), (2, 25): (64, 89), (2, 26): (76, 105),
               (2, 41): (124,), (3, 20): (74, 102), (3, 27): (102,),
               (3, 30): (114,), (3, 34): (102,), (3, 35): (134,)}
    queue = deque([("", (0, 0), 0, 0, 0)])
    decisions, terminals, shorts = {}, {}, []
    while queue:
        history, paid, actor, count, previous = queue.popleft()
        opponent = 1 - actor
        owed = paid[opponent] - paid[actor]
        sizes = ((4, 9, 18, 194) if not owed else targets.get((count, owed), ()) + (194,))
        if count >= 5 or paid[opponent] == 194:
            sizes = ()
        assert len(sizes) == len(set(sizes)) and tuple(sorted(sizes)) == sizes
        for target in sizes:
            assert target > max(paid)
            assert target == 194 or target >= paid[opponent] + (previous or 2)
        actions = (["F", "C"] if owed else ["X"]) + [
            "RAI" if value == 194 else "R" + (str(value // 2) if value % 2 == 0 else f"{value // 2}.5")
            for value in sizes]
        assert history not in decisions
        decisions[history] = {"actor": ("SB", "BB")[actor], "contributions_chips": [v * 50 for v in paid],
            "pot_chips": (12 + sum(paid)) * 50, "remaining_stack_chips": (194 - paid[actor]) * 50,
            "aggressions": count, "to_call_chips": owed * 50, "actions": actions}
        for action in actions:
            child = history + ("-" if history else "") + action
            updated = list(paid)
            if action == "C":
                updated[actor] = paid[opponent]
            kind = {"F": "fold", "C": "call"}.get(action)
            if action == "X" and history == "X":
                kind = "check_check"
            if kind:
                terminals[child] = {"kind": kind, "actor": ("SB", "BB")[actor],
                                    "contributions_chips": [value * 50 for value in updated]}
            elif action == "X":
                queue.append((child, paid, opponent, count, previous))
            else:
                target = 194 if action == "RAI" else int(Fraction(action[1:]) * 2)
                increment = target - paid[opponent]
                if increment < previous:
                    assert target == 194
                    shorts.append(child)
                updated[actor] = target
                queue.append((child, tuple(updated), opponent, count + 1, increment))
    return decisions, terminals, shorts


class DiagnosticChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = tomllib.loads((checker.HERE / "diagnostic.toml").read_text(encoding="utf-8"), parse_float=Decimal)
        cls.observed = checker.check_ranges.read_json(checker.HERE / "observed.json")
        cls.graph = checker.check_menus.calculate()

    def test_all_observed_menus_and_raw_ranges_match(self):
        result = checker.calculate()
        graph = result["static_graph_check"]
        self.assertEqual({key: graph[key] for key in ("decision_nodes", "action_edges", "terminal_nodes", "public_nodes")},
                         {"decision_nodes": 120, "action_edges": 356, "terminal_nodes": 237, "public_nodes": 357})
        self.assertEqual(result["range_counts"], {"oop": 545, "ip": 514})
        self.assertEqual(graph["terminal_kind_counts"], {"call": 118, "check_check": 1, "fold": 118})
        self.assertEqual(graph["short_allin_edges"], 4)
        self.assertEqual(graph["maximum_aggressions"], 5)
        self.assertEqual(len(graph["active_rule_hits"]), 17)
        self.assertTrue(all(graph["active_rule_hits"]))

    def test_independent_half_bb_bfs_matches_config_dfs_and_capture(self):
        expected, terminals, shorts = independent_half_bb_tree()
        actual, actual_terminals, _, _ = checker.model_tree(self.config)
        self.assertEqual(actual, expected)
        self.assertEqual(actual_terminals, terminals)
        self.assertEqual(sum(len(row["actions"]) for row in expected.values()), 356)
        for row in self.graph["decision_nodes_table"]:
            independent = expected[row["path"]]
            self.assertEqual(independent["actions"], [token for token, _ in row["actions"]])
            self.assertEqual(independent["actor"], row["actor"])
            self.assertEqual(independent["pot_chips"], row["pot_chips"])
            self.assertEqual(independent["contributions_chips"], row["contributions_chips"])
        self.assertEqual(set(shorts), {"R2-R14.5-R32-R67-RAI", "X-R2-R14.5-R32-R67-RAI",
                                     "R9-R21-R62-RAI", "X-R9-R21-R62-RAI"})

    def test_selector_collision_resolved_only_by_allin_legality(self):
        decisions, _, _, _ = checker.model_tree(self.config)
        result = checker.condition_collisions(decisions)
        self.assertEqual(result["distinct_aggressions_to_call"], 54)
        self.assertEqual(result["distinct_aggressions_pot_to_call"], 55)
        self.assertEqual(decisions["R2-R14.5-R32-R67"]["actions"], ["F", "C", "RAI"])
        self.assertEqual(decisions["R9-R21-R62-RAI"]["actions"], ["F", "C"])
        self.assertEqual(decisions["R9-R29.5-R62-RAI"]["actions"], ["F", "C"])

    def test_hand_transcribed_raise_to_and_call_contributions(self):
        decisions, terminals, _, _ = checker.model_tree(self.config)
        row = decisions["R2-R7-R17-R37"]
        self.assertEqual(row["contributions_chips"], [1700, 3700])
        self.assertEqual(row["pot_chips"], 6000)
        self.assertEqual(row["remaining_stack_chips"], 8000)
        self.assertEqual(row["actions"], ["F", "C", "RAI"])
        self.assertEqual(terminals["R2-R7-C"]["contributions_chips"], [700, 700])
        self.assertEqual(terminals["R9-RAI-C"]["contributions_chips"], [9700, 9700])
        self.assertEqual(terminals["X-X"]["contributions_chips"], [0, 0])

    def test_condition_amount_and_missing_rule_mutations_rejected(self):
        source = self.config["game"]["tree"]["script"]
        changes = [("to_call == 1250", "to_call == 1450"), ("3200c", "3300c"),
                   ("when aggressions == 3 && to_call == 1750", "when aggressions == 2 && to_call == 1750"),
                   ("  replace raise [a]\n", ""), ("[6700c, a]", "[a]"),
                   ("[200c, 450c, 900c, a]", "[200c, a]")]
        for before, after in changes:
            with self.subTest(before=before):
                self.assertIn(before, source)
                config = deepcopy(self.config)
                config["game"]["tree"]["script"] = source.replace(before, after)
                with self.assertRaises(ValueError):
                    checker.compare_graph(config, self.graph)

    def test_invalid_target_is_not_silently_clamped(self):
        config = deepcopy(self.config)
        config["game"]["tree"]["script"] = config["game"]["tree"]["script"].replace("700c", "300c")
        with self.assertRaisesRegex(ValueError, "silently clamped"):
            checker.model_tree(config)

    def test_unsupported_dsl_is_rejected(self):
        for source in ("", "turn {\n replace bet [200c]\n}", "river {\n add bet [200c]\n}",
                       "river {\n replace bet [10]\n}", "river {\n when history == X { replace bet [a] }\n}"):
            with self.subTest(source=source), self.assertRaises(ValueError):
                checker.parse_script(source)

    def test_embedded_raw_range_mutations_rejected(self):
        for seat in ("oop", "ip"):
            field = seat + "_range"
            for variant in (self.config["game"][field] + " ", ",".join(reversed(self.config["game"][field].split(",")))):
                config = deepcopy(self.config)
                config["game"][field] = variant
                with self.subTest(seat=seat), self.assertRaisesRegex(ValueError, "embedded range bytes"):
                    checker.check_config(config, self.observed)

    def test_config_shape_limits_and_no_quality_target(self):
        changes = [("game", "pot", 650), ("game", "effective_stack", 9600), ("game", "pot", 600.0),
                   ("game", "min_bet", 50), ("game", "iso_merging", True), ("game", "board", "9s 8s 7d 2c Jh"),
                   ("run", "threads", 4), ("run", "threads", True), ("run", "target_nash_conv", Decimal(".1")),
                   ("run", "max_time", "60s"), ("rake", "cap", 400), ("rake", "rounding", "down")]
        for section, key, value in changes:
            with self.subTest(section=section, key=key):
                config = deepcopy(self.config)
                config[section][key] = value
                with self.assertRaises(ValueError):
                    checker.check_config(config, self.observed)

    def test_hidden_tree_or_wrong_cap_rejected(self):
        for key, value in (("source", "elsewhere.tree"), ("params", {"x": 1}), ("allin_threshold", Decimal(".85")),
                           ("include_allin", True), ("max_aggressive_actions", {"flop": 0, "turn": 0, "river": 4})):
            with self.subTest(key=key):
                config = deepcopy(self.config)
                config["game"]["tree"][key] = value
                with self.assertRaises(ValueError):
                    checker.check_config(config, self.observed)

    def test_missing_reordered_or_wrong_observation_rejected(self):
        for kind in ("missing", "order", "actor", "stack", "pot", "duplicate"):
            graph = deepcopy(self.graph)
            rows = graph["decision_nodes_table"]
            if kind == "missing":
                rows.pop()
            elif kind == "order":
                rows[0]["actions"].reverse()
            elif kind == "actor":
                rows[0]["actor"] = "BB"
            elif kind == "stack":
                rows[0]["remaining_actor_stack_chips"] = 9600
            elif kind == "pot":
                rows[0]["pot_chips"] = 650
            else:
                rows.append(deepcopy(rows[0]))
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                checker.compare_graph(self.config, graph)

    def test_fraction_rake_on_independent_terminals(self):
        _, terminals, _ = independent_half_bb_tree()
        result = checker.rake_comparison(terminals)
        fractions = {}
        for history, terminal in terminals.items():
            paid = terminal["contributions_chips"]
            fractions[history] = (min(Fraction(600 + sum(paid), 20), 800),
                                  min(Fraction(600 + 2 * min(paid), 20), 800))
        for row in result["all_terminals"]:
            total, matched = fractions[row["history"]]
            self.assertEqual(Fraction(row["total_rake_chips"]), total)
            self.assertEqual(Fraction(row["conditional_matched_rake_chips"]), matched)
        self.assertEqual(fractions["RAI-F"], (515, 30))
        self.assertEqual(fractions["R2-F"], (40, 30))
        self.assertEqual(fractions["R9-R21-R62-RAI-F"], (800, 650))
        self.assertEqual(result["terminals_with_different_rake"], 118)
        self.assertEqual(result["different_terminal_kinds"], {"fold": 118})
        self.assertEqual(Fraction(result["maximum_difference_chips"]), 485)
        self.assertEqual(sum(a == 800 for a, _ in fractions.values()), 68)
        self.assertEqual(sum(b == 800 for _, b in fractions.values()), 60)

    def test_no_native_or_external_acceptance_claim(self):
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
