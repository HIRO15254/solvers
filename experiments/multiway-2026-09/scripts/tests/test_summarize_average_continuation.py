"""Small corruption regressions independent of measurements and solver binaries."""
import copy
from contextlib import contextmanager
import hashlib
import json
from pathlib import Path
import sys
import shutil
import unittest
import uuid
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_average_continuation as report


@contextmanager
def temporary_evidence():
    # Windows sandbox tokens cannot reopen Python 3.13's owner-only temporary
    # directories. Use the existing test-cache pattern with inherited ACLs.
    cache = (Path(__file__).resolve().parents[2] / ".cache" / "tool-tests").resolve()
    folder = cache / ("average-continuation-" + uuid.uuid4().hex)
    folder.mkdir(parents=True)
    try:
        yield folder
    finally:
        if folder.resolve().parent != cache:
            raise ValueError("test cleanup escaped its cache directory")
        shutil.rmtree(folder)


def estimate(mean, stderr=0.0):
    return dict(mean=mean, stderr=stderr)


def sampling(n, positive, seed, replays):
    def street(visited):
        return dict(positive_weight_decision_visits=positive if visited else 0,
                    positive_weight_trajectory_visits=positive if visited else 0,
                    trajectory_probability=estimate(float(visited)),
                    decisions_per_prefix_trajectory=estimate(float(visited)),
                    decision_weight_effective_sample_size=float(positive) if visited else 0.0,
                    **{key + "_fraction": estimate(float(key == "average")) if visited else None
                       for key in ("average", "current", "regret_fallback", "uniform_fallback")})
    return dict(seed=seed, samples=n, total_deal_attempts=n, terminal_replays=replays,
                positive_weight_samples=positive, relative_weight_mean=estimate(positive / n),
                effective_sample_size=float(positive), max_normalized_weight=1 / positive,
                prefix_current_fraction=estimate(0.0), prefix_regret_fallback_fraction=estimate(0.0),
                prefix_uniform_fallback_fraction=estimate(0.0), baseline_seats=[estimate(0.0)] * 6,
                coverage_by_street=[street(s == 3) for s in range(4)],
                coverage_by_seat=[[street(seat == 1 and s == 3) for s in range(4)] for seat in range(6)])


def fixture():
    support = dict(history=[1] * 16, action_indices=[0, 0], actor=1, street="river",
                   active_opponents=2, bucket_active_opponents=2, expected_buckets=32,
                   action_labels=["check", "bet-to:10", "bet-to:20:all-in"], rows=[])
    fit_rows = []
    for bucket in range(32):
        key = report.key_for(support, bucket)
        support["rows"].append(dict(key=key, regrets=[0.0] * 3, average_strategy=[0.25, 0.25, 0.5]))
        n = 128 if bucket < 2 else 1 if bucket == 2 else 0
        fit_rows.append(dict(key=key, baseline_source="average", positive_weight_samples=n,
                             relative_weight_sum=float(n), effective_sample_size=float(n),
                             max_normalized_weight=1 / n if n else 0.0,
                             action_gains=([estimate(v) for v in ([2.0, 2.0, -1.0] if bucket == 0
                                                                 else [-1.0, -2.0, -3.0])]
                                           if n >= 2 else [None] * 3), selected_action=0 if bucket == 0 else None))
    schedule = dict(fitSamples=65536, fitSeed=602, heldOutSamples=131072, heldOutSeeds=[702, 703], minFitEss=64)
    result = {k: copy.deepcopy(v) for k, v in support.items() if k != "rows"}
    result.update(config=dict(fit_samples=65536, fit_seed=602, held_out_samples=131072,
                              held_out_seeds=[702, 703], min_fit_ess=64),
                  variant=dict(purify_threshold=0.0, use_current_strategy=False), fit_elapsed_secs=1.0,
                  fit=dict(sampling=sampling(65536, 257, 602, 1028), retained_buckets=1, rows=fit_rows),
                  held_out=[dict(elapsed_secs=1.0, sampling=sampling(131072, 64, seed, 88),
                                 gain=estimate(1.0, 0.2), retained_key_weight_fraction=estimate(24 / 64, 0.01))
                            for seed in (702, 703)])
    return result, schedule, support


def evidence_fixture(run):
    """Small complete evidence package; no binary executes or solver files copy."""
    def write(path, value):
        path.write_text(json.dumps(value), encoding="utf-8")
    (run / "verification").mkdir()
    files = [{"path": f"fixture/{i}.rs", "sha256": hashlib.sha256(str(i).encode()).hexdigest()}
             for i in range(167)]
    write(run / "source-manifest.json", dict(baseRevision="fixture", files=files))
    with zipfile.ZipFile(run / "source.zip", "w") as archive:
        for i, row in enumerate(files):
            archive.writestr(row["path"], str(i))
    (run / "config-seed0.toml").write_text("fixture = true\n", encoding="utf-8")
    (run / "research.exe").write_bytes(b"non-executable fixture")
    exp = dict(baseRevision="fixture", summarizerSha256=report.sha(report.__file__),
               runnerSha256=report.sha(Path(report.__file__).with_name("run_average_sampling_measurement.ps1")),
               dependencies={module.__file__: report.sha(module.__file__) for module in
                   (report.balanced, report.endpoint_dependency, report.support_dependency, report.evidence_dependency)})
    for key, name in [("sourceManifestSha256", "source-manifest.json"), ("sourceZipSha256", "source.zip"),
                      ("configSha256", "config-seed0.toml"), ("binarySha256", "research.exe")]:
        exp[key] = report.sha(run / name)
    checks = []
    for i, command in enumerate(sorted(report.COMMANDS)):
        log = run / "verification" / f"{i}.log"
        is_test = command.startswith("cargo test ")
        log.write_text("test result: ok. 1 passed; 0 failed; 0 ignored;\n" if is_test else "", encoding="utf-8")
        check = dict(command=command, log=str(log), sha256=report.sha(log), exitCode=0, elapsedSecs=0.1)
        if is_test:
            check.update(passed=1, failed=0, ignored=0, suites=1)
        checks.append(check)
    write(run / "verification/verification.json", dict(status="passed", sourceManifestSha256=exp["sourceManifestSha256"], checks=checks))
    exp["verificationSha256"] = report.sha(run / "verification/verification.json")
    return exp


class AverageContinuationValidationTests(unittest.TestCase):
    def setUp(self):
        self.result, self.schedule, self.support = fixture()

    def check(self):
        return report.endpoint_checks(self.result, self.schedule, self.support)

    def test_accepts_negative_heldout_gain_and_keeps_fit_drop_denominators(self):
        self.result["held_out"][0]["gain"]["mean"] = -1.5
        selection = self.check()
        self.assertEqual(selection["retainedBuckets"], [0])
        self.assertEqual(selection["noPositiveFitGainBuckets"], [1])
        self.assertEqual(selection["insufficientEssBuckets"], list(range(2, 32)))
        self.assertEqual(selection["fitWeightFractions"], dict(retained=128 / 257,
                         insufficientEss=1 / 257, noPositiveFitGain=128 / 257))

    def test_distinguishes_missing_zero_and_nonzero_but_numeric_zero_matches(self):
        other = copy.deepcopy(self.support)
        self.support["rows"][0].update(regrets=None, average_strategy=None)
        self.support["rows"][1].update(regrets=[-1.0, 0.0, 0.0], average_strategy=None)
        other["rows"][1] = copy.deepcopy(self.support["rows"][1])
        counts = report.support_checks(self.support)
        self.assertEqual(counts["missingBuckets"], 1)
        self.assertEqual(counts["storedZeroRegretBuckets"], 30)
        self.assertEqual(counts["nonzeroRegretBuckets"], 1)
        self.assertEqual(counts["positiveRegretBuckets"], 0)
        self.assertEqual(report.numeric_regrets(self.support), report.numeric_regrets(other))

    def test_rejects_missing_support_bucket(self):
        self.support["rows"].pop()
        with self.assertRaisesRegex(ValueError, "complete support"):
            self.check()

    def test_rejects_foreign_private_information(self):
        self.result["fit"]["rows"][0]["key"]["player"] = 3
        with self.assertRaisesRegex(ValueError, "own-information key"):
            self.check()

    def test_rejects_exposed_raw_average_mass(self):
        self.support["rows"][0]["strategy_sum"] = [1.0, 1.0, 2.0]
        with self.assertRaisesRegex(ValueError, "no raw average mass"):
            self.check()
        with self.assertRaisesRegex(ValueError, "must not expose state"):
            report.normalized_only({"unexpected": [{"strategy_sum": [1.0]}]})

    def test_rejects_average_source_for_omitted_average(self):
        self.support["rows"][0]["average_strategy"] = None
        with self.assertRaisesRegex(ValueError, "baseline source/support"):
            self.check()

    def test_rejects_singleton_gain_estimate(self):
        self.result["fit"]["rows"][2]["action_gains"][0] = estimate(1.0)
        with self.assertRaisesRegex(ValueError, "per-key gain denominator"):
            self.check()

    def test_rejects_later_argmax_tie(self):
        self.result["fit"]["rows"][0]["selected_action"] = 1
        with self.assertRaisesRegex(ValueError, "first argmax"):
            self.check()

    def test_rejects_fit_weight_from_selected_subset(self):
        self.result["fit"]["sampling"]["relative_weight_mean"] = estimate(128 / 65536)
        with self.assertRaisesRegex(ValueError, "all-key fit weight partition"):
            self.check()

    def test_rejects_nonzero_gain_without_candidate_replay(self):
        held = self.result["held_out"][0]
        held["sampling"]["terminal_replays"] = 64
        held["retained_key_weight_fraction"] = estimate(0.0)
        with self.assertRaisesRegex(ValueError, "unsupported worlds retain baseline"):
            self.check()

    def test_rejects_fit_replay_not_all_actions(self):
        self.result["fit"]["sampling"]["terminal_replays"] -= 1
        with self.assertRaisesRegex(ValueError, "fit action replay enumeration"):
            self.check()

    def test_rejects_source_fraction_partition_corruption(self):
        self.result["held_out"][0]["sampling"]["coverage_by_street"][3]["average_fraction"] = estimate(0.9)
        with self.assertRaisesRegex(ValueError, "source partition"):
            self.check()

    def test_rejects_changed_heldout_budget(self):
        self.result["held_out"][0]["sampling"]["samples"] += 1
        with self.assertRaisesRegex(ValueError, "sampling schedule"):
            self.check()

    def test_literal_options_reject_duplicate_budget_but_keep_repeatable_paths(self):
        with self.assertRaisesRegex(ValueError, "duplicate scalar"):
            report.options(["--sweeps", "32768", "--sweeps", "32769"])
        self.assertEqual(report.options(["--node", "root", "--node", "fold"]),
                         {"--node": ["root", "fold"]})

    def test_paired_raw_numeric_change_rejected_even_when_fingerprint_claims_equal(self):
        common = {name: "same" for name in ("executableBlake3", "effectiveConfigBlake3",
            "configurationFingerprint", "abstractionFingerprint", "solverStateVersion", "sourceRevision",
            "nodes", "coveragePrefixes", "supportNodes", "endpointPrefixes")}
        metrics = {name: 1 for name in ("sweeps", "traversals", "total_deal_attempts", "mean_deal_attempts",
                                       "hand_updates", "memory_bytes", "average_positive_regret")}
        common.update(result=dict(current_regret_fingerprint="same", metrics=metrics),
                      diagnostics=dict(support=[self.support]))
        other = copy.deepcopy(common)
        other["diagnostics"]["support"][0]["rows"][0]["regrets"][0] = 1.0
        with self.assertRaisesRegex(ValueError, "paired numeric regret"):
            report.pair_checks(common, other)

    def test_complete_source_and_seven_command_evidence_passes(self):
        with temporary_evidence() as folder:
            run = Path(folder)
            exp = evidence_fixture(run)
            self.assertEqual(report.evidence_checks(run, exp)[1], 167)

    def test_rejects_rehashed_archive_with_changed_source_entry(self):
        with temporary_evidence() as folder:
            run = Path(folder)
            exp = evidence_fixture(run)
            with zipfile.ZipFile(run / "source.zip", "w") as archive:
                for i in range(167):
                    archive.writestr(f"fixture/{i}.rs", "changed" if i == 2 else str(i))
            exp["sourceZipSha256"] = report.sha(run / "source.zip")
            with self.assertRaisesRegex(ValueError, "archived source"):
                report.evidence_checks(run, exp)

    def test_rejects_missing_required_command_even_if_verification_rehashed(self):
        with temporary_evidence() as folder:
            run = Path(folder)
            exp = evidence_fixture(run)
            path = run / "verification/verification.json"
            value = report.read(path)
            value["checks"].pop()
            path.write_text(json.dumps(value), encoding="utf-8")
            exp["verificationSha256"] = report.sha(path)
            with self.assertRaisesRegex(ValueError, "seven required"):
                report.evidence_checks(run, exp)

    def test_rejects_test_total_disagreement_even_if_verification_rehashed(self):
        with temporary_evidence() as folder:
            run = Path(folder)
            exp = evidence_fixture(run)
            path = run / "verification/verification.json"
            value = report.read(path)
            next(c for c in value["checks"] if c["command"].startswith("cargo test "))["passed"] = 2
            path.write_text(json.dumps(value), encoding="utf-8")
            exp["verificationSha256"] = report.sha(path)
            with self.assertRaisesRegex(ValueError, "test totals"):
                report.evidence_checks(run, exp)

    def test_explicit_seed_config_allows_only_hashed_declared_seed_change(self):
        with temporary_evidence() as run:
            anchor = run / "config-seed0.toml"
            anchor.write_text("[solver]\nseed = 0\nopponent_exploration = 0.0\n", encoding="utf-8")
            explicit = run / "config-seed11.toml"
            explicit.write_text("[solver]\nseed = 11\nopponent_exploration = 0.0\n", encoding="utf-8")
            exp = dict(configSha256=report.sha(anchor))
            case = dict(trainingSeed=11, config=str(explicit), configSha256=report.sha(explicit))
            self.assertEqual(report.case_config(run, exp, case)[2]["solver"]["seed"], 11)
            self.assertEqual(report.case_config(run, exp, dict(trainingSeed=0))[0], anchor)
            with self.assertRaisesRegex(ValueError, "path and hash"):
                report.case_config(run, exp, dict(trainingSeed=11, config=str(explicit)))
            with self.assertRaisesRegex(ValueError, "case training seed"):
                report.case_config(run, exp, {**case, "trainingSeed": 29})
            explicit.write_text("[solver]\nseed = 11\nopponent_exploration = 0.25\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "file hash"):
                report.case_config(run, exp, case)
            with self.assertRaisesRegex(ValueError, "only declared training seed"):
                report.case_config(run, exp, {**case, "configSha256": report.sha(explicit)})

    def test_seed_bound_fingerprint_is_strict_for_pilot_and_separate_from_abstraction(self):
        with temporary_evidence() as run:
            config = run / "config.toml"
            config.write_text("[solver]\nseed = 0\n", encoding="utf-8")
            raw = dict(configurationFingerprint="seed11", abstractionFingerprint="same", solverStateVersion=2)
            reference = {**raw, "config": str(config), "configurationFingerprint": "seed0"}
            report.reference_model_checks(raw, dict(trainingSeed=11), [("fixture", reference)])
            with self.assertRaisesRegex(ValueError, "same-seed reference"):
                report.reference_model_checks(raw, dict(trainingSeed=0), [("fixture", reference)])
            raw["abstractionFingerprint"] = "changed"
            with self.assertRaisesRegex(ValueError, "abstraction/state identity"):
                report.reference_model_checks(raw, dict(trainingSeed=11), [("fixture", reference)])


if __name__ == "__main__":
    unittest.main()
