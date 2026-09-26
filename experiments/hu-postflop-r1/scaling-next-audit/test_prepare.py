import unittest

import prepare


class PreparationTests(unittest.TestCase):
    def test_one_byte_only(self):
        original = (prepare.analyze.ROOT / prepare.ORIGINAL).read_bytes()
        changed = prepare.candidate(original)
        self.assertEqual(len(original), len(changed))
        self.assertEqual(sum(a != b for a, b in zip(original, changed)), 1)
        self.assertEqual(prepare.analyze.identity(original)["sha256"], prepare.ORIGINAL_SHA)

    def test_rejects_absent_or_duplicate_control(self):
        for data in (b"[run]\npar_chance_depth = 1\n", b"par_chance_depth = 2\n" * 2):
            with self.assertRaises(AssertionError):
                prepare.candidate(data)

    def test_small_chance_still_consumes_depth(self):
        # Two-child root is below fanout threshold; its 12-child descendant
        # must still be disabled by depth=1 and enabled by depth=2.
        nodes = [(1, 0, 2, 1), (1, 0, 12, 3), (2, 0, 0, 0)] + [(2, 0, 0, 0)] * 12
        for depth, expected in ((0, 0), (1, 0), (2, 1)):
            counts = prepare.independent_chance_counts(nodes, depth)
            self.assertEqual(counts["forks"], expected)
            primary = prepare.analyze.scheduling([3, 3], nodes, depth)
            self.assertEqual(primary["eligible_chance_nodes_per_traversal"], expected)

    def test_fanout_boundary(self):
        for children, expected in ((11, 0), (12, 1)):
            nodes = [(1, 0, children, 1)] + [(2, 0, 0, 0)] * children
            self.assertEqual(prepare.independent_chance_counts(nodes, 1)["forks"], expected)


if __name__ == "__main__":
    unittest.main()
