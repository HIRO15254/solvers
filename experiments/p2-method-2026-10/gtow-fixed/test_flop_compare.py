"""Regression tests for benchmark preflop reach and chip-contribution helpers."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from flop_compare import hero_contribution, hero_details


class FlopCompareTests(unittest.TestCase):
    def setUp(self):
        self.labels = ["raise-to:2500", "raise-to:18000", "call:15500"]
        self.nodes = {
            (): {"actor": 5, "actions": [self.labels[0]],
                 "probabilities": [[0.5] for _ in range(169)]},
            tuple(self.labels[:1]): {"actor": 3, "actions": [self.labels[1]],
                                    "probabilities": [[0.25] for _ in range(169)]},
            tuple(self.labels[:2]): {"actor": 5, "actions": [self.labels[2]],
                                    "probabilities": [[0.75] for _ in range(169)]},
        }

    def test_caller_reach_position_and_contribution(self):
        player, reach, invested, cost, side = hero_details(self.nodes, self.labels, "caller")
        self.assertEqual((player, invested, cost, side), (5, 2.5, 15.5, "p1ip"))
        self.assertEqual(reach, [0.375] * 169)
        self.assertEqual(hero_contribution(self.nodes, self.labels, "caller"), 18.0)

    def test_last_raiser_reach_position_and_contribution(self):
        player, reach, invested, cost, side = hero_details(self.nodes, self.labels, "raiser")
        self.assertEqual((player, invested, cost, side), (3, 0.0, 18.0, "p1oop"))
        self.assertEqual(reach, [0.25] * 169)
        self.assertEqual(hero_contribution(self.nodes, self.labels, "raiser"), 18.0)

    def test_blind_contributions(self):
        labels = ["raise-to:2500", "call:2000"]
        for seat, blind, call in ((1, 0.5, "call:2000"), (2, 1.0, "call:1500")):
            labels[1] = call
            nodes = {
                (): {"actor": 3, "actions": [labels[0]],
                     "probabilities": [[1.0] for _ in range(169)]},
                tuple(labels[:1]): {"actor": seat, "actions": [call],
                                    "probabilities": [[1.0] for _ in range(169)]},
            }
            self.assertEqual(hero_details(nodes, labels, "caller")[2], blind)
            self.assertEqual(hero_contribution(nodes, labels, "caller"), 2.5)


if __name__ == "__main__":
    unittest.main()
