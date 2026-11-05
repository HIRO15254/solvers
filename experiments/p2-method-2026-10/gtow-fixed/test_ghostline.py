"""Fast regression tests for active-opponent reach and snapshot persistence."""
import unittest

from ghostline import analyze, category, persistence


class GhostlineTests(unittest.TestCase):
    def test_combo_weighted_reach_and_folded_seat_exclusion(self):
        rows = [[0.2, 0.8] for _ in range(169)]
        rows[0] = [0.7, 0.3]  # AA has six of 1326 unconditional combos.
        nodes = {
            (): {"actor": 3, "actions": ["raise-to:2000", "fold"], "probabilities": rows},
            ("raise-to:2000",): {"actor": 4, "actions": ["call:2000", "fold"],
                                  "probabilities": [[0.5, 0.5] for _ in range(169)]},
            ("fold",): {"actor": 4, "actions": ["call:2000", "fold"],
                         "probabilities": [[0.5, 0.5] for _ in range(169)]},
            ("raise-to:2000", "call:2000"): {
                "actor": 3, "actions": ["check", "raise-to:4000"],
                "probabilities": [[0.5, 0.5] for _ in range(169)]},
        }
        reference = {"classes": [], "nodes": []}
        result, near, reached, _ = analyze(nodes, reference)
        self.assertEqual(result["categories"]["c"]["count"], 3 * 169)
        expected = 0.2 + 0.5 * 6 / 1326
        self.assertAlmostEqual(result["categories"]["c"]["actor_reach_mass"], 2 + expected)
        self.assertAlmostEqual(result["categories"]["c"]["joint_reach_mass"], 1 + 1.5 * expected)
        folded = result["top20_c"][0]
        self.assertEqual(folded["path"], ["fold"])
        self.assertAlmostEqual(folded["own_reach"], 1.0)
        self.assertAlmostEqual(folded["opponents_reach"], 1.0)
        self.assertEqual(near, reached)
        self.assertEqual(category(0, 0), "a")
        self.assertEqual(category(1, 0), "b")
        self.assertEqual(category(1e-6, 1e-6), "c")

    def test_persistence_detects_departure_even_with_uniform_endpoints(self):
        a, b = ((), "AA"), ((), "KK")
        states = {f"it{i:06d}": ({a, b}, {a, b}, {a: (0.5, 0.5), b: (0.5, 0.5)})
                  for i in range(1000, 6001, 1000)}
        states["it003000"] = ({a}, {a}, {a: (0.5, 0.5)})
        result = persistence(states)
        self.assertEqual(result["c_at_1000_stays_near_uniform_all_snapshots"], 1)
        self.assertEqual(result["c_at_1000_leaves_near_uniform_at_least_once"], 1)
        self.assertEqual(result["c_at_6000_near_uniform_all_snapshots"], 1)


if __name__ == "__main__":
    unittest.main()
