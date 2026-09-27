"""Small source/derivation regressions; these do not execute Rust or a solver."""
from pathlib import Path
import ast
import hashlib
import json
import unittest

import prepare


class AdapterSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.original = prepare.BASE.read_bytes()
        cls.old = cls.original.decode("utf-8")
        cls.new = (prepare.HERE / "solve.rs").read_text(encoding="utf-8")

    def test_pin_forward_and_exact_inverse(self):
        self.assertEqual(hashlib.sha256(self.original).hexdigest(), prepare.BASE_SHA)
        self.assertEqual(prepare.transform(self.old), self.new)
        self.assertEqual(prepare.transform(self.new, reverse=True).encode(), self.original)
        provenance = json.loads((prepare.HERE / "provenance.json").read_text())
        self.assertEqual(provenance["generated"], prepare.pin(self.new.encode()))
        self.assertEqual(provenance["marker_helper"], prepare.pin((prepare.HERE / "markers.rs.inc").read_bytes()))

    def test_missing_or_ambiguous_anchor_fails_closed(self):
        anchor = "    let gains = pool.install(|| solver.exploitability()).0;"
        with self.assertRaises(ValueError):
            prepare.transform(self.old.replace(anchor, "    // missing gain call"))
        with self.assertRaises(ValueError):
            prepare.transform(self.old + "\n" + anchor)
        with self.assertRaises(ValueError):
            prepare.transform(self.new.replace("    let cfr_end_ns = monotonic_ns();", ""), reverse=True)

    def test_fixture_state_and_output_json_templates_unchanged(self):
        def fixture_and_writer(source):
            return source[source.index("fn fixture("):source.index("fn main(")]
        self.assertEqual(fixture_and_writer(self.old), fixture_and_writer(self.new))
        # Preserve every original JSON format template, including float/bit values.
        templates = [line for line in self.old.splitlines() if '"schema' in line]
        self.assertEqual(len(templates), 4)
        for template in templates:
            self.assertEqual(self.new.count(template), 1)
        for operation in ("solver.run(iterations)", "solver.expected_value(p)",
                          "solver.best_response_value(p)", "solver.exploitability()"):
            self.assertEqual(self.new.count(operation), self.old.count(operation))
        self.assertEqual(self.new.count("chance_depth: 2,"), 1)

    def test_cfr_marker_bounds_only_original_pool_work(self):
        body = self.new.split("    let cfr_start_ns = monotonic_ns();\n", 1)[1].split("    let cfr_end_ns = monotonic_ns();", 1)[0]
        self.assertEqual(body, '    pool.install(|| {\n        assert_eq!(rayon::current_num_threads(), threads);\n        solver.run(iterations);\n    });\n')
        self.assertLess(self.new.index('let cfr_end_ns'), self.new.index('let state_start_ns'))
        self.assertLess(self.new.index('let state_end_ns'), self.new.index('let quality_start_ns'))
        self.assertLess(self.new.index('let quality_end_ns'), self.new.index('&out.join("phases.json")'))
        self.assertEqual(self.new.count("= monotonic_ns();"), 12)  # EV/BR each execute twice.

    def test_clock_integer_safety_and_finite_scope(self):
        helper = (prepare.HERE / "markers.rs.inc").read_text()
        for fragment in ("const CLOCK_MONOTONIC: std::ffi::c_int = 1;",
                         ".checked_mul(1_000_000_000)", ".checked_add(value.tv_nsec as u64)",
                         "(0..1_000_000_000).contains(&value.tv_nsec)",
                         "start < end", "bounds[0][1] <= bounds[1][0]",
                         "bounds[1][1] <= bounds[2][0]", "bounds[2][0] <= bounds[3][0]",
                         "pair[0][1] <= pair[1][0]", "bounds[7][1] <= bounds[2][1]"):
            self.assertIn(fragment, helper)
        self.assertIn('assert_eq!(iterations, 64,', self.new)
        self.assertIn('assert!([1, 16, 32].contains(&threads)', self.new)
        self.assertIn('target_env = "gnu"', self.new)
        for file in ("prepare.py", "test_prepare.py"):
            ast.parse((prepare.HERE / file).read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
