"""Small source-only checks; these do not compile or execute Rust."""
import unittest

import prepare


class SourceChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.original = prepare.read_original(prepare.ROOT)
        cls.tests = (prepare.HERE / 'tests.rs.in').read_bytes()

    def test_frozen_outputs_reproduce(self):
        first = prepare.outputs(self.original)
        self.assertEqual(first, prepare.outputs(self.original))
        for name, content in first[1].items():
            self.assertEqual((prepare.HERE / name).read_bytes(), content)

    def test_changed_source_or_storage_contract_is_rejected(self):
        for name in prepare.SOURCES:
            changed = dict(self.original)
            changed[name] += b'\n'
            with self.subTest(name=name), self.assertRaises(ValueError):
                prepare.transform(changed, self.tests)

    def test_accumulator_or_old_take_change_is_rejected(self):
        candidate = prepare.transform(self.original, self.tests)
        for name, old, new in (
            (prepare.SOLVER, b'let mut node_cfv = scratch.take(num_hands);',
             b'let mut node_cfv = scratch.take_for_overwrite(num_hands);'),
            (prepare.SCRATCH, b'        buf.clear();\n', b''),
        ):
            changed = dict(candidate)
            self.assertIn(old, changed[name])
            changed[name] = changed[name].replace(old, new, 1)
            with self.subTest(name=name), self.assertRaises(ValueError):
                prepare.verify_scope(self.original, changed, self.tests)

    def test_average_strategy_callers_stay_zero_initialized(self):
        candidate = prepare.transform(self.original, self.tests)
        solver = candidate[prepare.SOLVER].decode()
        self.assertEqual(solver.count('let mut sigma = scratch.take(sref.len());'), 3)
        self.assertEqual(solver.count('scratch.take_for_overwrite('), 2)
        self.assertEqual(prepare.read_original(prepare.ROOT), self.original)


if __name__ == '__main__':
    unittest.main()
