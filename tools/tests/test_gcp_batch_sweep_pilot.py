import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock


TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
MODULE_PATH = TOOLS / "gcp_batch_sweep_pilot.py"
SPEC = importlib.util.spec_from_file_location("gcp_batch_sweep_pilot", MODULE_PATH)
pilot = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = pilot
SPEC.loader.exec_module(pilot)


class GcpBatchSweepPilotTests(unittest.TestCase):
    def test_renders_only_the_two_requested_batch_algorithms(self):
        fixture = (
            Path(__file__).resolve().parents[2]
            / "examples/bench_multiway/6max_100bb_nl50_partial_reference.toml"
        )
        base = fixture.read_text(encoding="utf-8")
        for name, batch in pilot.CASES:
            with self.subTest(name=name):
                text = pilot.render_case(base, batch)
                self.assertIn(f"batch_sweeps = {batch}", text)
                self.assertIn('kind = "none"\n\n[solver.pruning]\nkind = "none"', text)
                self.assertIn("flop = 32\nturn = 32\nriver = 32", text)
                self.assertIn("threads = 8", text)
                self.assertIn('memory = "48GiB"', text)

    def test_resource_overrides_are_rendered(self):
        fixture = (
            Path(__file__).resolve().parents[2]
            / "examples/bench_multiway/6max_100bb_nl50_partial_reference.toml"
        )
        text = pilot.render_case(
            fixture.read_text(encoding="utf-8"), 8, threads=24, memory="160GiB"
        )
        self.assertIn("threads = 24", text)
        self.assertIn('memory = "160GiB"', text)

    def test_validators_reject_incomplete_outputs(self):
        with self.assertRaises(RuntimeError):
            pilot.read_object(Path("missing.json"))

        with mock.patch.object(
            pilot,
            "read_object",
            return_value={"status": "time-limit", "sweeps": pilot.SWEEPS},
        ):
            with self.assertRaises(RuntimeError):
                pilot.validate_solve(Path("unused"), 8)

        incomplete_audit = {
            "schemaVersion": "solvers.multiway-checkpoint-audit/v1",
            "sweeps": pilot.SWEEPS,
            "evaluationSamplesPerSeed": 4096,
            "evaluationSeeds": [101, 202],
            "evaluations": [{}, {}],
            "nodes": [],
        }
        with mock.patch.object(pilot, "read_object", return_value=incomplete_audit):
            with self.assertRaises(RuntimeError):
                pilot.validate_audit(Path("unused"))


if __name__ == "__main__":
    unittest.main()
