import json
import shutil
import unittest
import uuid
from pathlib import Path
from unittest import mock

from tools import multiway_convergence_bench as bench


SOURCE = '''schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 3
button = 0

[game.defaults]
stack_bb = 2.0
range = "random"

[solver]
kind = "range-vector"
seed = 7
batch_sweeps = 1

[solver.discount]
kind = "none"

[solver.pruning]
kind = "none"

[run]
max_sweeps = 20

[run.stop]
target = 1000000.0
check_every_sweeps = 20

[run.resources]
threads = 1
memory = "64MiB"
'''


class MultiwayConvergenceBenchTests(unittest.TestCase):
    def setUp(self):
        self.temp_directories = []

    def tearDown(self):
        for path in reversed(self.temp_directories):
            shutil.rmtree(path, ignore_errors=True)

    def workspace_temp_dir(self):
        # The managed Windows test host denies writes inside tempfile's
        # mode-700 directories.  Keep fixture material under target/, which
        # is already the repository's ignored scratch area.
        path = Path("target") / f"multiway-convergence-test-{uuid.uuid4().hex}"
        path.mkdir(parents=True)
        self.temp_directories.append(path)
        return path

    def test_fixed_config_rewrites_only_benchmark_controls(self):
        result = bench.fixed_budget_config(
            SOURCE,
            seed=11,
            solver_kind="range-vector",
            pruning="regret-based",
            batch=4,
            discount="1000",
            sweeps=100,
            max_time=None,
        )
        self.assertIn('kind = "range-vector"', result)
        self.assertIn("seed = 11", result)
        self.assertIn("batch_sweeps = 4", result)
        self.assertIn('kind = "periodic"', result)
        self.assertIn("every_sweeps = 1000", result)
        self.assertIn("max_sweeps = 100", result)
        # The cadence is deliberately outside the fixed sweep budget.
        self.assertIn("check_every_sweeps = 101", result)
        self.assertIn('kind = "regret-based"', result)

    def test_none_discount_removes_periodic_only_keys(self):
        periodic = SOURCE.replace(
            '[solver.discount]\nkind = "none"',
            '[solver.discount]\nkind = "periodic"\nevery_sweeps = 100\nuntil_sweeps = 1000',
        )
        result = bench.fixed_budget_config(
            periodic,
            seed=0,
            solver_kind="range-vector",
            pruning="none",
            batch=1,
            discount="none",
            sweeps=10,
            max_time=None,
        )
        discount = result.split("[solver.discount]", 1)[1].split("[solver.pruning]", 1)[0]
        self.assertIn('kind = "none"', discount)
        self.assertNotIn("every_sweeps", discount)
        self.assertNotIn("until_sweeps", discount)

    def test_sweep_u64_max_is_rejected(self):
        with self.assertRaises(ValueError):
            bench.fixed_budget_config(
                SOURCE,
                seed=0,
                solver_kind="range-vector",
                pruning="none",
                batch=1,
                discount="config",
                sweeps=bench.MAX_U64,
                max_time=None,
            )

    def test_missing_sections_are_added(self):
        result = bench.fixed_budget_config(
            SOURCE.replace("[solver.discount]\nkind = \"none\"\n\n", ""),
            seed=0,
            solver_kind="single-hand",
            pruning="none",
            batch=1,
            discount="none",
            sweeps=10,
            max_time="30s",
        )
        self.assertIn('[solver.discount]\nkind = "none"', result)
        self.assertIn('max_time = "30s"', result)

    def test_profile_equivalence_is_semantic_and_unknown_for_general_fixture(self):
        self.assertEqual(
            bench.evaluation_profile_equivalence(SOURCE),
            bench.PROFILE_EQUIVALENCE_UNKNOWN,
        )
        fixture = Path("examples/bench_multiway/3max_2bb.toml").read_text(encoding="utf-8")
        self.assertEqual(
            bench.evaluation_profile_equivalence(fixture),
            bench.PROFILE_EQUIVALENCE_VERIFIED,
        )
        fixture_6max = Path("examples/bench_multiway/6max_2bb.toml").read_text(encoding="utf-8")
        self.assertEqual(
            bench.evaluation_profile_equivalence(fixture_6max),
            bench.PROFILE_EQUIVALENCE_VERIFIED,
        )

    def test_relative_tree_source_is_copied_with_its_config_relative_layout(self):
        directory = self.workspace_temp_dir()
        source_config = directory / "source.toml"
        source_config.write_text(SOURCE + '\n[game.tree]\nkind = "script"\nsource = "trees/main.mwtree"\n', encoding="utf-8")
        tree = directory / "trees" / "main.mwtree"
        tree.parent.mkdir()
        tree.write_text("checkdown\n", encoding="utf-8")
        variant_root = directory / "variant"
        copied = bench.copy_external_tree_source(
            source_config, source_config.read_text(encoding="utf-8"), variant_root
        )
        self.assertEqual(copied[0]["reference"], "trees/main.mwtree")
        self.assertEqual(
            (variant_root / "trees" / "main.mwtree").read_text(encoding="utf-8"),
            "checkdown\n",
        )

    def test_matrix_skips_invalid_single_hand_pruning(self):
        variants, invalid = bench.variants_from_args(
            [0], ["range-vector", "single-hand"], ["none", "regret-based"], [1], ["config"]
        )
        self.assertEqual(len(variants), 3)
        self.assertEqual(len(invalid), 1)
        self.assertIn("single-hand", invalid[0]["reason"])

    def test_unknown_pruning_fails_before_writing_output(self):
        directory = self.workspace_temp_dir()
        config = directory / "config.toml"
        output = directory / "results"
        config.write_text(SOURCE, encoding="utf-8")
        with mock.patch("sys.stderr") as stderr:
            code = bench.main(
                [
                    str(config),
                    "--output-root",
                    str(output),
                    "--pruning",
                    "unknown",
                    "--dry-run",
                ]
            )
        self.assertEqual(code, 2)
        self.assertFalse(output.exists())
        self.assertIn("unsupported pruning", "".join(call.args[0] for call in stderr.write.call_args_list if call.args))

    def test_dry_run_prints_plan_without_subprocess(self):
        directory = self.workspace_temp_dir()
        config = directory / "config.toml"
        config.write_text(SOURCE, encoding="utf-8")
        with mock.patch.object(bench.subprocess, "run") as run, mock.patch("sys.stdout") as stdout:
            code = bench.main(
                [
                    str(config),
                    "--seeds",
                    "0",
                    "--solver-kinds",
                    "range-vector",
                    "--pruning",
                    "none",
                    "--batches",
                    "1",
                    "--discounts",
                    "config",
                    "--evaluation-solver",
                    "evaluator-bin",
                    "--dry-run",
                ]
            )
        self.assertEqual(code, 0)
        run.assert_not_called()
        printed = "".join(call.args[0] for call in stdout.write.call_args_list if call.args)
        self.assertIn(bench.TOOL_SCHEMA, printed)
        self.assertIn('"variant_id"', printed)
        self.assertIn('"evaluation_solver": "evaluator-bin"', printed)

    def test_run_one_keeps_raw_artifacts_and_separates_timing(self):
        directory = self.workspace_temp_dir()
        root = directory / "results"
        source = directory / "source.toml"
        source.write_text(SOURCE, encoding="utf-8")
        variant = bench.Variant(0, "range-vector", "none", 1, "config")

        def fake_run(command, stdout, stderr, check, timeout):
            if "evaluate" in command:
                stdout.write(
                    "ehs2 tables: loaded in 0.64s\n"
                    + json.dumps(
                        {
                            "samples": 8,
                            "total_deal_attempts": 8,
                            "seats": [
                                {"mean": 0.0, "stderr": 0.1, "ci95": [-0.2, 0.2]},
                                {"mean": 0.0, "stderr": 0.1, "ci95": [-0.2, 0.2]},
                            ],
                            "deviation_gain_lower_bound": [
                                {"mean": 0.0, "stderr": 0.1, "ci95": [0.0, 0.2]},
                                {"mean": 0.0, "stderr": 0.1, "ci95": [0.0, 0.2]},
                            ],
                        }
                    )
                    + "\n"
                )
            else:
                stdout.write("solver output\n")
            stderr.write("")
            run_dir = root / "variants" / variant.id / "run"
            run_dir.mkdir(parents=True, exist_ok=True)
            (run_dir / "run.json").write_text(
                json.dumps(
                    {
                        "schemaVersion": 3,
                        "kind": "preflop-multiway",
                        "status": "sweep-limit",
                        "sweeps": 4,
                        "elapsedSecs": 0.25,
                    }
                ),
                encoding="utf-8",
            )
            (run_dir / "events.jsonl").write_text(
                json.dumps(
                    {
                        "seq": 1,
                        "unixMs": 1,
                        "level": "info",
                        "kind": "notice",
                        "message": "ehs2 tables built in 1.25s",
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            (run_dir / "solution.mwsol").write_bytes(b"fixture")
            return mock.Mock(returncode=0)

        with mock.patch.object(bench.subprocess, "run", side_effect=fake_run):
            summary = bench._run_one(
                "solver",
                None,
                source,
                root,
                variant,
                sweeps=4,
                max_time=None,
                eval_samples=8,
                eval_seed=2,
                br_traversals=3,
                timeout=10,
                inspect=True,
            )
        variant_root = root / "variants" / variant.id
        self.assertTrue((variant_root / "config.toml").is_file())
        self.assertTrue((variant_root / "solve.stdout.log").is_file())
        self.assertTrue((variant_root / "evaluation.stdout.log").is_file())
        parsed_evaluation = json.loads(
            (variant_root / "evaluation.json").read_text(encoding="utf-8")
        )
        self.assertEqual(parsed_evaluation["samples"], 8)
        self.assertTrue((variant_root / "run" / "events.jsonl").is_file())
        self.assertEqual(summary["events"]["abstraction_build_seconds"], 1.25)
        self.assertEqual(
            summary["evaluation_profile_equivalence"],
            bench.PROFILE_EQUIVALENCE_UNKNOWN,
        )
        self.assertEqual(summary["timing"]["solver_reported_elapsed_seconds"], 0.25)
        self.assertIsNone(summary["timing"]["training_seconds"])
        self.assertEqual(summary["evaluation"]["returncode"], 0)

    def test_evaluation_notice_and_unusable_output_are_distinguished(self):
        valid = "ehs2 tables loaded in 0.5s\n" + json.dumps(
            {
                "samples": 2,
                "total_deal_attempts": 2,
                "seats": [
                    {"mean": 0.0, "stderr": 0.1, "ci95": [-0.2, 0.2]},
                    {"mean": 0.0, "stderr": 0.1, "ci95": [-0.2, 0.2]},
                ],
                "deviation_gain_lower_bound": [
                    {"mean": 0.0, "stderr": 0.1, "ci95": [0.0, 0.2]},
                    {"mean": 0.0, "stderr": 0.1, "ci95": [0.0, 0.2]},
                ],
            }
        )
        self.assertEqual(bench.parse_evaluation_stdout(valid)["samples"], 2)
        with self.assertRaises(ValueError):
            bench.parse_evaluation_stdout("unexpected banner\n{" + '"samples": 2}')
        with self.assertRaises(ValueError):
            bench.parse_evaluation_stdout(json.dumps({"samples": 2}))
        with self.assertRaises(ValueError):
            bench.parse_evaluation_stdout(
                json.dumps(
                    {
                        "samples": 2,
                        "total_deal_attempts": 2,
                        "seats": [
                            {"mean": 0.0, "stderr": -0.1, "ci95": [-0.2, 0.2]},
                            {"mean": 0.0, "stderr": 0.1, "ci95": [0.2, -0.2]},
                        ],
                        "deviation_gain_lower_bound": [
                            {"mean": 0.0, "stderr": 0.1, "ci95": [0.0, 0.2]},
                            {"mean": -0.1, "stderr": 0.1, "ci95": [0.0, 0.2]},
                        ],
                    }
                )
            )

        directory = self.workspace_temp_dir()
        root = directory / "bad-results"
        source = directory / "source.toml"
        source.write_text(SOURCE, encoding="utf-8")
        variant = bench.Variant(0, "range-vector", "none", 1, "config")

        def malformed_evaluation(command, stdout, stderr, check, timeout):
            if "evaluate" in command:
                stdout.write("ehs2 tables: loaded in 0.5s\nnot json\n")
            else:
                stdout.write("")
            stderr.write("")
            run_dir = root / "variants" / variant.id / "run"
            run_dir.mkdir(parents=True, exist_ok=True)
            (run_dir / "run.json").write_text(
                json.dumps({"status": "sweep-limit", "elapsedSecs": 0.1}), encoding="utf-8"
            )
            (run_dir / "solution.mwsol").write_bytes(b"fixture")
            return mock.Mock(returncode=0)

        with mock.patch.object(bench.subprocess, "run", side_effect=malformed_evaluation):
            summary = bench._run_one(
                "solver",
                None,
                source,
                root,
                variant,
                sweeps=4,
                max_time=None,
                eval_samples=2,
                eval_seed=2,
                br_traversals=3,
                timeout=10,
                inspect=False,
            )
        self.assertEqual(summary["status"], "evaluation-failed")
        self.assertFalse(summary["evaluation"]["usable"])
        self.assertTrue((root / "variants" / variant.id / "evaluation.stdout.log").is_file())

    def test_existing_variant_is_refused_before_config_write(self):
        directory = self.workspace_temp_dir()
        root = directory / "results"
        source = directory / "source.toml"
        source.write_text(SOURCE, encoding="utf-8")
        variant = bench.Variant(0, "range-vector", "none", 1, "config")
        variant_root = root / "variants" / variant.id
        variant_root.mkdir(parents=True)
        marker = variant_root / "marker.txt"
        marker.write_text("keep", encoding="utf-8")
        with self.assertRaises(FileExistsError):
            bench._run_one(
                "solver",
                None,
                source,
                root,
                variant,
                sweeps=4,
                max_time=None,
                eval_samples=8,
                eval_seed=2,
                br_traversals=3,
                timeout=10,
                inspect=True,
            )
        self.assertTrue(marker.is_file())
        self.assertFalse((variant_root / "config.toml").exists())

    def test_failed_solver_returns_nonzero_from_main(self):
        directory = self.workspace_temp_dir()
        config = directory / "config.toml"
        output = directory / "results"
        config.write_text(SOURCE, encoding="utf-8")

        def failed_run(command, stdout, stderr, check, timeout):
            stdout.write("")
            stderr.write("fixture failure\n")
            return mock.Mock(returncode=7)

        with mock.patch.object(bench.subprocess, "run", side_effect=failed_run):
            code = bench.main(
                [
                    str(config),
                    "--output-root",
                    str(output),
                    "--seeds",
                    "0",
                    "--solver-kinds",
                    "range-vector",
                    "--pruning",
                    "none",
                    "--batches",
                    "1",
                    "--discounts",
                    "config",
                ]
            )
        self.assertEqual(code, 1)
        summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["summaries"][0]["status"], "solve-failed")


if __name__ == "__main__":
    unittest.main()
