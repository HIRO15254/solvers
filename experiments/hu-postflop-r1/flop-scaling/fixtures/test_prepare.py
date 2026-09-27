import copy
import math
import unittest
from collections import Counter
from fractions import Fraction

import prepare


def independent_tree_counts():
    """Street-by-street frontier, versus prepare's recursively cached whole tree."""
    frontier = Counter({0: 1})
    total = Counter()
    for street, name in enumerate(prepare.STREETS):
        next_frontier = Counter()

        def finish(paid, multiplicity):
            if street == 2:
                total["showdown_terminals"] += multiplicity
            else:
                fanout = 49 - street
                total[f"{name}_chance_nodes"] += multiplicity
                total["deal_edges"] += multiplicity * fanout
                next_frontier[paid] += multiplicity * fanout

        for baseline, multiplicity in frontier.items():
            if baseline == 900:
                finish(baseline, multiplicity)
                continue
            # Local street wages, explicit path stack, no memoization or shared resolver.
            todo = [(0, 0, 0, 0, False, 0)]
            while todo:
                a, b, actor, aggression, checked, previous_increment = todo.pop()
                wages = [a, b]
                other = 1 - actor
                outstanding = wages[other] - wages[actor]
                total[f"{name}_action_nodes_p{actor}"] += multiplicity
                actions = 2 if outstanding else 1
                if outstanding:
                    total[f"{name}_fold_terminals"] += multiplicity
                    finish(baseline + wages[other], multiplicity)
                elif checked:
                    finish(baseline + a, multiplicity)
                else:
                    todo.append((a, b, other, aggression, True, previous_increment))
                maximum = 900 - baseline
                if aggression < 2 and maximum - wages[actor] > outstanding:
                    pot_after_call = 200 + 2 * baseline + a + b + outstanding
                    wager = wages[actor] + outstanding + math.floor(Fraction(3 * pot_after_call, 4) + Fraction(1, 2))
                    wager = min(max(wager, wages[other] + max(previous_increment, 10)), maximum)
                    assert wager > wages[other]
                    total[f"{name}_{'raise' if outstanding else 'bet'}_available_nodes"] += multiplicity
                    increment = wager - wages[other]
                    wages[actor] = wager
                    todo.append((*wages, other, aggression + 1, checked, increment))
                    actions += 1
                total[f"{name}_action_edges_p{actor}"] += multiplicity * actions
        frontier = next_frontier
    return dict(sorted(total.items()))


class FixturesTests(unittest.TestCase):
    def config(self, name="narrow"):
        return prepare.tomllib.loads(prepare.config_bytes(name).decode())

    def test_unit_class_expansion(self):
        for text, expected in (("AA", 6), ("AKs", 4), ("AKo", 12), ("TT+", 30),
                               ("JJ-99", 18), ("AQs-ATs", 12)):
            self.assertEqual(len(prepare.expand(text)), expected)

    def test_reject_unhandled_or_overlapping_input(self):
        for text in ("AA,AA", "TT+,JJ", "AA:0", "AKx", "T9s-54s"):
            with self.assertRaises(AssertionError):
                prepare.expand(text)

    def test_supports_and_exact_compatibility(self):
        for name, raw, live, mass in (("narrow", (42, 38), (34, 30), 870),
                                     ("expanded", (74, 184), (63, 160), 8700)):
            facts = prepare.range_facts(self.config(name)["game"])
            self.assertEqual(tuple(facts[s]["input_combos"] for s in ("oop", "ip")), raw)
            self.assertEqual(tuple(facts[s]["positive_root_combos"] for s in ("oop", "ip")), live)
            self.assertEqual(facts["joint"]["compatible_pair_mass"], mass)
            self.assertEqual(facts["joint"]["cartesian_pair_mass"], mass + facts["joint"]["incompatible_pair_mass"])

    def test_duplicate_board_rejected(self):
        game = self.config()["game"]
        game["board"] = "Qs Qs 2h"
        with self.assertRaises(AssertionError):
            prepare.range_facts(game)

    def test_small_is_strict_subset(self):
        for p in (0, 1):
            self.assertLess(prepare.expand(prepare.SPECS["narrow"][p]), prepare.expand(prepare.SPECS["expanded"][p]))

    def test_only_range_fields_differ(self):
        old, new = self.config("narrow"), self.config("expanded")
        for c in (old, new):
            prepare.validate_config(c)
            del c["game"]["oop_range"], c["game"]["ip_range"]
        self.assertEqual(old, new)

    def test_later_street_cap_cannot_silently_remove_raise(self):
        config = self.config()
        config["game"]["tree"]["max_aggressive_actions"]["turn"] = 1
        with self.assertRaises(AssertionError):
            prepare.validate_config(config)

    def test_no_transferred_quality_target(self):
        config = self.config()
        config["run"]["target_nash_conv"] = 0.0367
        with self.assertRaises(AssertionError):
            prepare.validate_config(config)

    def test_positive_half_rounds_up_and_no_bb_quantization(self):
        # 75% of2 =1.5 rounds to2 chips, not bankers rounding or10-chip snapping.
        self.assertEqual(prepare.legal_wager(2, 100, 1, 0, (0, 0), 0, 0, 0), 2)
        self.assertEqual(prepare.legal_wager(6, 100, 1, 0, (0, 0), 0, 0, 0), 5)

    def test_raise_to_minimum_cap_and_allin(self):
        self.assertEqual(prepare.legal_wager(200, 900, 10, 0, (150, 0), 1, 1, 150), 525)
        self.assertIsNone(prepare.legal_wager(200, 900, 10, 0, (150, 525), 0, 2, 375))
        self.assertEqual(prepare.legal_wager(200, 900, 10, 525, (525, 525), 0, 0, 0), 900)
        self.assertIsNone(prepare.legal_wager(200, 900, 10, 525, (900, 525), 1, 1, 375))

    def test_street_frontier_count_agrees_independently(self):
        actual = prepare.count_tree(self.config()["game"])
        self.assertEqual(independent_tree_counts(), actual["counts"])
        self.assertEqual(actual["nodes"], 367662)
        self.assertEqual(actual["edges"], actual["nodes"] - 1)
        for street in prepare.STREETS:
            self.assertGreater(actual["counts"][f"{street}_raise_available_nodes"], 0)

    def test_saved_inputs_are_deterministic(self):
        for name in prepare.SPECS:
            self.assertEqual((prepare.HERE / f"{name}.toml").read_bytes(), prepare.config_bytes(name))


if __name__ == "__main__":
    unittest.main()
