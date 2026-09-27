"""Pure source and inverse checks; no Rust compiler or process clock is invoked."""
import json
import unittest

import prepare


class CpuAdapterTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.original = prepare.SOURCE.read_bytes()
        cls.helper = (prepare.HERE / "cpu_clock.rs.inc").read_text(encoding="utf-8")
        cls.generated = prepare.transform(cls.original, cls.helper)

    def test_exact_pins_and_inverse(self):
        receipt = json.loads((prepare.HERE / "provenance.json").read_text(encoding="utf-8"))
        self.assertEqual(prepare.pin(self.original), receipt["source"])
        self.assertEqual(prepare.pin(self.helper.encode()), receipt["helper"])
        self.assertEqual(prepare.pin((prepare.HERE / "prepare.py").read_bytes()), receipt["generator"])
        self.assertEqual(self.generated, (prepare.HERE / "solve.rs").read_bytes())
        self.assertEqual(prepare.pin(self.generated), receipt["generated"]["solve.rs"])
        self.assertEqual(prepare.restore(self.generated, self.helper), self.original)

    def test_unrelated_source_and_generated_solver_mutations_rejected(self):
        with self.assertRaisesRegex(ValueError, "Frozen Cloud32 adapter differs"):
            prepare.transform(self.original.replace(b"solver.run(iterations)", b"solver.run(1)"), self.helper)
        with self.assertRaisesRegex(ValueError, "Restored adapter differs"):
            prepare.restore(self.generated.replace(b"solver.run(iterations)", b"solver.run(1)"), self.helper)
        with self.assertRaisesRegex(ValueError, "CPU inverse anchor differs"):
            prepare.restore(self.generated.replace(b"let cfr_cpu_started = process_cpu_ns();", b"let cfr_cpu_started = 0;"), self.helper)

    def test_fixture_state_writer_and_original_quality_serialization_unchanged(self):
        before, after = self.original.decode(), self.generated.decode()
        # These complete sections include the typed fixture, full F32 stream,
        # original invocation, and quality/result JSON formats.
        for left, right in (("fn fixture(", "fn main("),
                            ('    let args: Vec<String>', '    event("cfr", "started");'),
                            ('    let quality = format!(', '    event("probe", "completed");')):
            original_section = before.split(left, 1)[1].split(right, 1)[0]
            actual_section = after.split(left, 1)[1].split(right, 1)[0]
            if left == '    let quality = format!(':
                actual_section = actual_section.split('    write_json(\n        &out.join("cpu.json"),', 1)[0]
            self.assertEqual(actual_section, original_section)

    def test_cpu_brackets_original_work_without_extra_solver_calls(self):
        text = self.generated.decode()
        for call in ("solver.run(iterations)", "solver.expected_value(p)", "solver.best_response_value(p)", "solver.exploitability()"):
            self.assertEqual(text.count(call), self.original.decode().count(call))
        self.assertLess(text.index("let cfr_cpu_started"), text.index("solver.run(iterations)"))
        self.assertGreater(text.index("let cfr_cpu_seconds"), text.index("let cfr_seconds"))
        self.assertLess(text.index("let quality_cpu_started"), text.index("solver.expected_value(p)"))
        self.assertGreater(text.index("let quality_cpu_seconds"), text.index("let quality_seconds"))
        self.assertGreater(text.index('&out.join("cpu.json")'), text.index('&out.join("result.json")'))


if __name__ == "__main__":
    unittest.main()
