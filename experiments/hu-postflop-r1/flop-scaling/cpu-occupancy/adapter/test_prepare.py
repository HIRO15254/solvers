"""Tiny source/inverse checks; no compiler, workload, or process clock calls."""
import importlib.util
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("cpu_occupancy_prepare", Path(__file__).with_name("prepare.py"))
p = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(p)


class AdapterTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.original = p.SOURCE.read_bytes()
        cls.helper = p.CLOCK_SOURCE.read_bytes().decode()
        cls.generated = p.transform(cls.original, cls.helper)

    def test_every_generated_file_and_pin_matches(self):
        for name, data in p.expected_outputs().items():
            self.assertEqual((p.HERE / name).read_bytes(), data, name)
        receipt = json.loads((p.HERE / "provenance.json").read_text())
        self.assertEqual(receipt["baseline_solver"]["sha256"], p.BASELINE_SHA)
        self.assertEqual(receipt["generated"]["cpu_clock.rs.inc"]["sha256"], p.CLOCK_SHA)
        self.assertEqual(p.restore(self.generated, self.helper), self.original)

    def test_source_helper_and_unrelated_solver_mutations_rejected(self):
        with self.assertRaisesRegex(ValueError, "Frozen adapter"):
            p.transform(self.original.replace(b"solver.run(iterations)", b"solver.run(1)"), self.helper)
        with self.assertRaisesRegex(ValueError, "Frozen adapter"):
            p.transform(self.original, self.helper.replace("CLOCK_PROCESS_CPUTIME_ID", "WRONG_CLOCK"))
        with self.assertRaisesRegex(ValueError, "Restored adapter"):
            p.restore(self.generated.replace(b"chance_depth: 2", b"chance_depth: 1"), self.helper)
        with self.assertRaisesRegex(ValueError, "inverse anchor"):
            p.restore(self.generated.replace(b"let phase_wall_started = Instant::now();", b"let phase_wall_started = started;"), self.helper)

    def test_fixture_state_and_original_json_sections_unchanged(self):
        before, after = self.original.decode(), self.generated.decode()
        for left, right in (("fn fixture(", "fn main("),
                            ('    let args: Vec<String>', '    event("cfr", "started");'),
                            ('    let quality = format!(', '    event("probe", "completed");')):
            expected = before.split(left, 1)[1].split(right, 1)[0]
            actual = after.split(left, 1)[1].split(right, 1)[0]
            if left == '    let args: Vec<String>':
                actual = actual.replace('    let cpu_allowed_list = allowed_cpu_list()?;\n', '')
            if left == '    let quality = format!(':
                actual = actual.split('    assert_eq!(cpu_allowed_list,', 1)[0]
            self.assertEqual(actual, expected)

    def test_cpu_and_wall_bracket_identical_calls(self):
        text = self.generated.decode()
        for call in ("solver.run(iterations)", "solver.expected_value(p)", "solver.best_response_value(p)", "solver.exploitability()"):
            self.assertEqual(text.count(call), self.original.decode().count(call))
        for name, call in (("ev", "solver.expected_value(p)"), ("br", "solver.best_response_value(p)")):
            pos = text.index(call)
            self.assertLess(text.rfind("let phase_cpu_started", 0, pos), text.rfind("let phase_wall_started", 0, pos))
            self.assertLess(text.rfind("let phase_wall_started", 0, pos), pos)
            self.assertLess(pos, text.index(f"{name}_wall_seconds[p.index()] ="))
            self.assertLess(text.index(f"{name}_wall_seconds[p.index()] ="), text.index(f"{name}_cpu_seconds[p.index()] ="))
        self.assertLess(text.index("let exploitability_wall_started"), text.index("solver.exploitability()"))
        self.assertLess(text.index("let exploitability_wall_seconds"), text.index("let exploitability_cpu_seconds"))
        self.assertLess(text.index("let cfr_cpu_started"), text.index("solver.run(iterations)"))
        self.assertGreater(text.index("let quality_cpu_seconds"), text.index("let quality_seconds"))

    def test_schema_original_wall_copies_and_affinity_outside_timing(self):
        text = self.generated.decode()
        self.assertIn(p.SCHEMA, text)
        for name in ("ev", "br"):
            self.assertIn(f'let mut {name}_wall_seconds = [0.0_f64; 2];', text)
            self.assertIn(f'\\"{name}_wall_seconds\\":{{{name}_wall_seconds:?}}', text)
        self.assertIn(r'\"cfr_wall_seconds\":{cfr_seconds}', text)
        self.assertIn(r'\"quality_wall_seconds\":{quality_seconds}', text)
        self.assertLess(text.index("let cpu_allowed_list ="), text.index("let cfr_cpu_started"))
        self.assertGreater(text.index("assert_eq!(cpu_allowed_list,"), text.index("let quality_cpu_seconds"))
        self.assertGreater(text.index('&out.join("cpu.json")'), text.index('&out.join("result.json")'))
        self.assertIn(self.helper, text)


if __name__ == "__main__":
    unittest.main()
