"""Small offline checks of pin rejection and the candidate's change boundary."""
import unittest

import prepare


class PreparationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.original = (prepare.ROOT / prepare.SOURCE).read_bytes()
        cls.generator = (prepare.ROOT / prepare.GENERATOR).read_bytes()
        cls.changed, _ = prepare.checked_transform(cls.original, cls.generator)

    def test_rejects_different_source_before_transform(self):
        with self.assertRaisesRegex(ValueError, "source pin"):
            prepare.checked_transform(self.original + b"\n", self.generator)

    def test_rejects_different_generator_before_execution(self):
        with self.assertRaisesRegex(ValueError, "generator pin"):
            prepare.checked_transform(self.original, self.generator + b"\n")

    def test_exact_boundary_and_formatted_candidate(self):
        for changed in (self.changed, (prepare.HERE / "solver.rs").read_bytes()):
            self.assertTrue(prepare.scope_check(self.original, changed)["outside_regions_byte_identical"])

    def test_detects_ev_combine_change_outside_chance(self):
        changed = self.changed.replace(b"scratch.put(sigma);", b"drop(sigma);", 1)
        with self.assertRaisesRegex(ValueError, "outside helper"):
            prepare.scope_check(self.original, changed)


if __name__ == "__main__":
    unittest.main()
