import importlib.util
import json
import sys
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).resolve().parents[1] / "gcp_multiway_experiment.py"
SPEC = importlib.util.spec_from_file_location("gcp_multiway_experiment", MODULE_PATH)
experiment = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = experiment
SPEC.loader.exec_module(experiment)


class GcpMultiwayExperimentTests(unittest.TestCase):
    def test_pilot_render_changes_only_scheduled_algorithm_keys(self):
        base = experiment.BASE_CONFIG.read_text(encoding="utf-8")
        variant = experiment.Variant("pilot", 11, "single-hand", 4, "regret-based")
        rendered = experiment.render_config(base, variant, 256, 256, 128, 512, 8, "5m", 0)
        self.assertIn('kind = "single-hand"', rendered)
        self.assertIn("seed = 11", rendered)
        self.assertIn("batch_sweeps = 4", rendered)
        self.assertIn("max_sweeps = 256", rendered)
        self.assertIn('max_time = "5m"', rendered)
        self.assertIn("evaluation_samples = 128", rendered)
        self.assertIn("deviator_traversals = 512", rendered)
        self.assertIn("threads = 8", rendered)
        self.assertIn('[solver.discount]\nkind = "none"', rendered)

    def test_periodic_discount_ends_after_final_sweep(self):
        base = experiment.BASE_CONFIG.read_text(encoding="utf-8")
        variant = experiment.Variant("vector-b4", 0, "range-vector", 4, "none")
        rendered = experiment.render_config(base, variant, 65_536, 65_536, 8192, 100_000, 8, "15m", 10_000)
        self.assertIn(
            '[solver.discount]\nkind = "periodic"\nevery_sweeps = 10000\nuntil_sweeps = 65537',
            rendered,
        )

    def test_pruning_selector_keeps_paired_seeds(self):
        unpruned = experiment.matrix_variants("none")
        pruned = experiment.matrix_variants("regret-based")
        self.assertEqual(len(unpruned), 9)
        self.assertEqual(len(pruned), 3)
        self.assertEqual({variant.seed for variant in unpruned}, {0, 11, 29})
        self.assertTrue(all(variant.pruning == "regret-based" for variant in pruned))

    def test_variant_filter_intersects_pruning(self):
        selected = experiment.parse_variant_names("vector-b4,vector-b4-prune")
        self.assertEqual(
            {variant.run_id for variant in experiment.matrix_variants("none", selected)},
            {f"vector-b4-seed{seed:04d}" for seed in (0, 11, 29)},
        )
        self.assertEqual(experiment.matrix_variants("none", {"vector-b4-prune"}), [])
        with self.assertRaises(ValueError):
            experiment.parse_variant_names("vector-b4,unknown")

    def test_verifier_requires_complete_sweeps_and_six_seat_quality(self):
        run = Path("unused-run")
        result = {"status": "sweep-limit", "sweeps": 256}
        seat = {
            "deviationGainLowerBound": {"mean": 0.1, "stderr": 0.01, "ci95": [0.08, 0.12]}
        }
        row = {"sweeps": 256, "seats": [seat] * 6}
        with (
            mock.patch.object(experiment, "read_json", return_value=result),
            mock.patch.object(Path, "is_file", return_value=True),
            mock.patch.object(Path, "read_text", return_value=json.dumps(row) + "\n"),
        ):
            status, _, _ = experiment.verify_result(run, 256, {256})
            self.assertEqual(status, "ok")

            row["seats"] = [seat] * 5
            with mock.patch.object(Path, "read_text", return_value=json.dumps(row) + "\n"):
                status, _, _ = experiment.verify_result(run, 256, {256})
            self.assertEqual(status, "failed-missing-live-quality")

        with (
            mock.patch.object(
                experiment, "read_json", return_value={"status": "time-limit", "sweeps": 256}
            ),
            mock.patch.object(experiment, "quality_rows", return_value=[]),
        ):
            status, _, _ = experiment.verify_result(run, 256, {256})
            self.assertEqual(status, "incomplete-time-limit")


if __name__ == "__main__":
    unittest.main()
