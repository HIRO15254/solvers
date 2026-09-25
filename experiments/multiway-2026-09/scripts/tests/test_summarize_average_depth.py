from copy import deepcopy
import json
import math
from pathlib import Path
import shutil
import sys
import unittest
import uuid


sys.path.insert(0, str(Path(__file__).parents[1]))
import summarize_average_depth as summary


class AverageDepthSummaryTests(unittest.TestCase):
    def setUp(self):
        self.cache = (Path(__file__).parents[2] / ".cache" / "tool-tests").resolve()
        self.root = self.cache / f"average-depth-{uuid.uuid4().hex}"
        self.root.mkdir(parents=True)
        self.addCleanup(self.cleanup)
        (self.root / "source-manifest.json").write_text("{}", encoding="utf-8")
        self.binary = self.root / "research.exe"
        self.binary.write_text("immutable fixture; never executed", encoding="utf-8")

    def cleanup(self):
        # Restrict recursive cleanup to this test's checked workspace cache.
        if self.root.resolve().parent != self.cache:
            raise ValueError("test cleanup escaped its cache directory")
        shutil.rmtree(self.root)

    @staticmethod
    def coverage(preflop=False, river=False):
        values = {name + "_visits_by_street": dict.fromkeys(summary.STREETS, 0)
                  for name in summary.SOURCES}
        for street, counts in (("preflop", (10, 9, 6, 3, 1, 0) if preflop else (0,) * 6),
                               ("river", (2, 2, 1, 1, 0, 0) if river else (0,) * 6)):
            for name, count in zip(summary.SOURCES, counts):
                values[name + "_visits_by_street"][street] = count
        values.update({name + "_visits": sum(values[name + "_visits_by_street"].values())
                       for name in summary.SOURCES})
        return values

    @staticmethod
    def row(ctx, bucket, probability=0.4, zero=False):
        street = summary.STREETS.index(ctx["street"])
        path = [2**32 - 1] * 4
        path[street] = bucket
        return {"key": {"history": list(bytes.fromhex(ctx["history"])), "player": ctx["actor"],
                        "street": street, "active_opponents": ctx["activeOpponents"], "bucket_path": path},
                "status": "zero-average-mass-omitted" if zero else "average-observed",
                "actions": None if zero else [{"action": "check", "probability": probability},
                                              {"action": "bet", "probability": 1 - probability}]}

    def run_fixture(self, seed=0, variant="uniform-one", probability=0.4, *,
                    phase="fixed", sweeps=4096, fingerprint="c" * 64, solve_seconds=0.5):
        name = f"{phase}-{seed}-{variant}"
        directory = self.root / name
        directory.mkdir()
        config = self.root / f"config-seed{seed}.toml"
        config.write_text(f'''[game]
seat_count = 3
[game.information]
recall = "current-street"
[game.abstraction]
kind = "ehs2-percentile"
[game.abstraction.buckets]
flop = 32
turn = 32
river = 32
[solver]
seed = {seed}
''', encoding="utf-8")
        source_hash = summary.sha(self.root / "source-manifest.json")
        contexts = [{"requested": "root", "history": "00" * 16, "actor": 0, "street": "preflop", "activeOpponents": 2},
                    {"requested": "river-path", "history": "11" * 16, "actor": 0, "street": "river", "activeOpponents": 2}]
        seat_coverage = [self.coverage(True, True), self.coverage(), self.coverage()]
        profile = {"samples": 16, "total_deal_attempts": 16,
                   "seats": [{"mean": 0.0, "stderr": 0.1, "ci95": [-0.2, 0.2]} for _ in range(3)],
                   "deviation_gain_lower_bound": [{"mean": 0.0, "stderr": 0.1, "ci95": [0.0, 0.2]} for _ in range(3)],
                   "candidate_policy_coverage": seat_coverage}
        baseline = deepcopy(profile)
        baseline["deviation_gain_lower_bound"] = None
        prefixes = [{"history": [0] * 16, "reached_samples": 16,
                     "trajectory_visits_by_street": dict(zip(summary.STREETS, (10, 0, 0, 2))),
                     "candidate_policy_coverage": deepcopy(seat_coverage)},
                    {"history": [17] * 16, "reached_samples": 2,
                     "trajectory_visits_by_street": dict(zip(summary.STREETS, (0, 0, 0, 2))),
                     "candidate_policy_coverage": [self.coverage(river=True), self.coverage(), self.coverage()]}]
        output = {"schemaVersion": "solvers.multiway-average-sampling-research/v1",
                  "sourceRevision": source_hash, "executableBlake3": "a" * 64,
                  "effectiveConfigBlake3": summary.sha(config), "configurationFingerprint": summary.sha(config),
                  "abstractionFingerprint": "b" * 64, "solverStateVersion": 4, "threads": 2,
                  "config": str(config), "elapsedSecs": 1.0, "constructionElapsedSecs": 0.3,
                  "nodes": deepcopy(contexts), "coveragePrefixes": deepcopy(contexts),
                  "result": {"variant": variant, "threads": 2, "solve_elapsed_secs": solve_seconds,
                             "current_regret_fingerprint": fingerprint,
                             "metrics": {"sweeps": sweeps, "traversals": sweeps * 3, "total_deal_attempts": sweeps * 3, "hand_updates": sweeps},
                             "histories": [{"history": list(bytes.fromhex(ctx["history"])),
                                            "strategies": [self.row(ctx, 0, probability), self.row(ctx, 1, zero=True)]}
                                           for ctx in contexts],
                             "evaluations": [{"seed": seed, "result": deepcopy(profile)} for seed in (101, 202)],
                             "coverage_evaluations": [{"seed": seed, "result": {"evaluation": deepcopy(baseline), "prefixes": deepcopy(prefixes)}}
                                                      for seed in (101, 202)]}}
        arguments = ["--config", str(config), "--variant", variant, "--sweeps", str(sweeps), "--threads", "2",
                     "--source-revision", source_hash, "--evaluation-seeds", "101,202", "--evaluation-samples", "16",
                     "--coverage-samples", "16", "--node", "root", "--node", "river-path",
                     "--coverage-prefix", "root", "--coverage-prefix", "river-path"]
        job = {"arguments": arguments, "configSha256": summary.sha(config), "sourceManifestSha256": source_hash, "timeoutSeconds": 60}
        job_path = self.root / f"{name}-job.json"
        job_path.write_text(json.dumps(job), encoding="utf-8")
        measurement = {**job, "schemaVersion": "solvers.average-sampling-measurement/v1", "exitCode": 0, "timedOut": False,
                       "binary": str(self.binary), "binarySha256": summary.sha(self.binary), "job": str(job_path),
                       "jobSha256": summary.sha(job_path), "wallSeconds": 2.0, "observedPeakWorkingSetBytes": 16384}
        self.save(directory, output, measurement)
        return directory, output, measurement

    @staticmethod
    def save(directory, output, measurement):
        path = directory / "stdout.json"
        path.write_text(json.dumps(output), encoding="utf-8")
        measurement["stdoutSha256"] = summary.sha(path)
        (directory / "measurement.json").write_text(json.dumps(measurement), encoding="utf-8")

    def test_fixed_denominator_preserves_absent_zero_and_unmeasured(self):
        directory, output, measurement = self.run_fixture()
        result, _ = summary.summarize_run(directory)
        self.assertEqual((result["nodes"][1]["averageObserved"], result["nodes"][1]["touchedZeroAverage"], result["nodes"][1]["untouched"]), (1, 1, 30))
        self.assertIsNone(result["coverage"][1]["byStreet"]["flop"]["averageFraction"])
        output["result"]["histories"][1]["strategies"] = []
        self.save(directory, output, measurement)
        result, _ = summary.summarize_run(directory)
        self.assertEqual(result["nodes"][1]["untouched"], 32)

    def test_incomplete_or_corrupt_measurements_fail(self):
        directory, output, measurement = self.run_fixture()
        for name, value in (("timedOut", True), ("exitCode", 1), ("stdoutSha256", "0" * 64), ("binarySha256", "0" * 64), ("jobSha256", "0" * 64), ("configSha256", "0" * 64)):
            with self.subTest(name=name):
                invalid = {**measurement, name: value}
                (directory / "measurement.json").write_text(json.dumps(invalid), encoding="utf-8")
                with self.assertRaises(ValueError):
                    summary.summarize_run(directory)

    def test_context_and_row_corruption_fail(self):
        directory, output, measurement = self.run_fixture()
        changes = [lambda x: x["result"]["histories"][1]["strategies"][0]["key"].update(street=1),
                   lambda x: x["result"]["histories"][1]["strategies"][0]["key"].update(player=1),
                   lambda x: x["result"]["histories"][1]["strategies"][0]["key"].update(history=[9] * 16),
                   lambda x: x["result"]["histories"][1]["strategies"][0]["key"]["bucket_path"].__setitem__(3, 32),
                   lambda x: x["result"]["histories"][1]["strategies"].append(deepcopy(x["result"]["histories"][1]["strategies"][0])),
                   lambda x: x["result"]["histories"][1]["strategies"][0]["actions"][0].update(probability=math.nan),
                   lambda x: x["result"]["histories"][1]["strategies"][1].update(actions=[{"action": "check", "probability": 1.0}])]
        for change in changes:
            invalid = deepcopy(output)
            change(invalid)
            self.save(directory, invalid, measurement)
            with self.assertRaises(ValueError):
                summary.summarize_run(directory)

    def test_missing_prefix_seed_partition_and_trajectory_fail(self):
        directory, output, measurement = self.run_fixture()
        changes = [lambda x: x["result"]["coverage_evaluations"][0]["result"]["prefixes"].pop(),
                   lambda x: x["result"]["coverage_evaluations"].pop(),
                   lambda x: x["result"]["coverage_evaluations"][1].update(seed=101),
                   lambda x: x["result"]["coverage_evaluations"][0]["result"]["prefixes"][1]["trajectory_visits_by_street"].update(river=3),
                   lambda x: x["result"]["coverage_evaluations"][0]["result"]["prefixes"][1]["candidate_policy_coverage"][0].update(average_strategy_visits=2)]
        for change in changes:
            invalid = deepcopy(output)
            change(invalid)
            self.save(directory, invalid, measurement)
            with self.assertRaises(ValueError):
                summary.summarize_run(directory)

    def test_dispersion_intersection_is_explicit_and_unobserved_is_not_zero(self):
        for seed, probability in ((0, 0.2), (11, 0.4), (29, 0.6)):
            for variant in summary.VARIANTS:
                self.run_fixture(seed, variant, probability if variant == "uniform-one" else 0.4)
        result = summary.summarize(self.root, "fixed", [0, 11, 29])
        for node in result["dispersion"]:
            self.assertEqual(node["commonObservedBuckets"], 1)
            self.assertAlmostEqual(node["meanActionCellSampleStdDev"]["uniform-one"], 0.2)
            self.assertEqual(node["meanActionCellSampleStdDev"]["enumerate-first-opponent"], 0.0)

    def test_cross_variant_action_menu_and_cross_seed_context_fail(self):
        fixtures = [self.run_fixture(seed, variant) for seed in (0, 11) for variant in summary.VARIANTS]
        directory, output, measurement = fixtures[-1]
        output["result"]["histories"][0]["strategies"][0]["actions"][1]["action"] = "raise"
        self.save(directory, output, measurement)
        with self.assertRaisesRegex(ValueError, "action menu"):
            summary.summarize(self.root, "fixed", [0, 11])
        output["result"]["histories"][0]["strategies"][0]["actions"][1]["action"] = "bet"
        output["nodes"][1]["actor"] = 1
        for row in output["result"]["histories"][1]["strategies"]:
            row["key"]["player"] = 1
        self.save(directory, output, measurement)
        with self.assertRaisesRegex(ValueError, "cross-run context"):
            summary.summarize(self.root, "fixed", [0, 11])

    def test_duplicate_training_seeds_and_wrong_run_variant_fail(self):
        with self.assertRaises(ValueError):
            summary.summarize(self.root, "fixed", [0, 0])
        directory, output, measurement = self.run_fixture()
        output["result"]["variant"] = "enumerate-first-opponent"
        self.save(directory, output, measurement)
        with self.assertRaisesRegex(ValueError, "variant mismatch"):
            summary.summarize_run(directory)

    def test_calibrated_comparison_explicitly_allows_different_budgets(self):
        self.run_fixture(sweeps=6000, fingerprint="d" * 64)
        self.run_fixture(variant="enumerate-first-opponent")
        # The ordinary/default mode still rejects an accidental unequal budget.
        with self.assertRaises(ValueError):
            summary.summarize(self.root, None, [0])
        self.run_fixture(phase="calibrated", sweeps=6000, fingerprint="d" * 64, solve_seconds=0.49)
        result = summary.summarize(self.root, None, [0], control_phase="calibrated", research_phase="fixed")
        self.assertEqual(result["comparisonMode"], "calibrated-compute")
        pair = result["pairedComparisons"][0]
        self.assertEqual((pair["controlSweeps"], pair["researchSweeps"]), (6000, 4096))
        self.assertEqual(pair["controlRun"], "calibrated-0-uniform-one")
        self.assertFalse(pair["regretFingerprintEqual"])
        self.assertAlmostEqual(pair["solveTimeRatio"], 0.5 / 0.49)

    def test_fixed_sweeps_still_require_equal_regret(self):
        self.run_fixture()
        self.run_fixture(variant="enumerate-first-opponent", fingerprint="d" * 64)
        with self.assertRaisesRegex(ValueError, "currentRegretFingerprint mismatch"):
            summary.summarize(self.root, "fixed", [0])

    def test_calibrated_mode_does_not_relax_configuration_or_schedule_identity(self):
        self.run_fixture(phase="calibrated", sweeps=6000)
        directory, output, measurement = self.run_fixture(variant="enumerate-first-opponent")
        output["configurationFingerprint"] = "f" * 64
        self.save(directory, output, measurement)
        with self.assertRaisesRegex(ValueError, "calibrated paired identity"):
            summary.summarize(self.root, None, [0], control_phase="calibrated", research_phase="fixed")
        output["configurationFingerprint"] = summary.sha(Path(output["config"]))
        output["result"]["evaluations"][0]["result"]["samples"] = 32
        self.save(directory, output, measurement)
        with self.assertRaisesRegex(ValueError, "sample budget"):
            summary.summarize(self.root, None, [0], control_phase="calibrated", research_phase="fixed")

    def test_calibrated_dispersion_uses_all_six_selected_runs(self):
        for index, (seed, probability) in enumerate(((0, 0.2), (11, 0.4), (29, 0.6))):
            self.run_fixture(seed, probability=probability, phase="calibrated", sweeps=6000 + 1000 * index,
                             fingerprint="d" * 64)
            directory, output, measurement = self.run_fixture(seed, "enumerate-first-opponent")
            # A bucket observed in only five selected runs must be excluded.
            if seed == 29:
                output["result"]["histories"][1]["strategies"][0] = self.row(output["nodes"][1], 0, zero=True)
                self.save(directory, output, measurement)
        result = summary.summarize(self.root, None, [0, 11, 29], control_phase="calibrated", research_phase="fixed")
        self.assertEqual(len(result["runs"]), 6)
        self.assertEqual(result["dispersion"][0]["commonObservedBuckets"], 1)
        self.assertAlmostEqual(result["dispersion"][0]["meanActionCellSampleStdDev"]["uniform-one"], 0.2)
        self.assertEqual(result["dispersion"][1]["commonObservedBuckets"], 0)
        self.assertIsNone(result["dispersion"][1]["meanActionCellSampleStdDev"]["uniform-one"])

    def test_phase_selectors_require_explicit_complete_calibration_request(self):
        for phase, control, research in (("fixed", "calibrated", "fixed"), (None, "calibrated", None), (None, None, "fixed")):
            with self.assertRaises(ValueError):
                summary.comparison_phases(phase, control, research)
        self.assertEqual(summary.comparison_phases(None, None, None), (dict.fromkeys(summary.VARIANTS, "fixed"), False))
        args = summary.argument_parser().parse_args(["--root", "unused", "--output", "unused",
                                                    "--control-phase", "calibrated", "--research-phase", "fixed"])
        self.assertIsNone(args.phase)
        self.assertEqual(summary.comparison_phases(args.phase, args.control_phase, args.research_phase),
                         ({"uniform-one": "calibrated", "enumerate-first-opponent": "fixed"}, True))


if __name__ == "__main__":
    unittest.main()
