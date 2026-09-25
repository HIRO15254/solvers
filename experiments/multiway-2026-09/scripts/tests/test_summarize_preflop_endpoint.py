"""Small standalone corruption regressions for preflop endpoint evidence."""
import copy
from contextlib import contextmanager
import hashlib
import json
from pathlib import Path
import shutil
import sys
import unittest
import uuid
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_preflop_endpoint as report


@contextmanager
def temporary_evidence():
    # Inherit workspace ACLs: the Windows sandbox cannot reopen Python 3.13's
    # owner-only TemporaryDirectory directories.
    cache = (Path(__file__).resolve().parents[2] / ".cache" / "tool-tests").resolve()
    folder = cache / ("preflop-endpoint-" + uuid.uuid4().hex)
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
                coverage_by_street=[street(s == 0) for s in range(4)],
                coverage_by_seat=[[street(seat == 1 and s == 0) for s in range(4)] for seat in range(6)])


def fixture():
    support = dict(history=[1] * 16, action_indices=[0, 0, 0, 0], actor=1, street="preflop",
                   active_opponents=1, bucket_active_opponents=1, expected_buckets=169,
                   action_labels=["fold", "raise-to:3000", "raise-to:100000:all-in"], rows=[])
    fit_rows = []
    for bucket in range(169):
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
                            for seed in (702, 703)],
                  proposal=dict(preflop_actions=support["action_indices"].copy(), preflop_history=support["history"].copy(),
                                root_range_fingerprint=[2] * 32, proposal_range_fingerprint=[3] * 32,
                                positive_target_combos_by_seat=[1326] * 6, floor_adjusted_combos_by_seat=[0] * 6,
                                target_scale_by_seat=[1.0] * 6, proposal_floor_fraction=1e-7,
                                pilot_samples=4096, pilot_accepted=1000))
    return result, schedule, support


def evidence_fixture(run):
    """Complete tiny evidence package: no solver binary executes."""
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
               testScriptSha256=report.sha(__file__),
               runnerSha256=report.sha(Path(report.__file__).with_name("run_average_sampling_measurement.ps1")),
               dependencies={module.__file__: report.sha(module.__file__) for module in
                   (report.pilot, report.balanced, report.endpoint_dependency,
                    report.pilot.support_dependency, report.pilot.evidence_dependency)})
    for key, name in [("sourceManifestSha256", "source-manifest.json"), ("sourceZipSha256", "source.zip"),
                      ("configSha256", "config-seed0.toml"), ("binarySha256", "research.exe")]:
        exp[key] = report.sha(run / name)
    checks = []
    for i, command in enumerate(sorted(report.pilot.COMMANDS)):
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


class PreflopEndpointValidationTests(unittest.TestCase):
    def setUp(self):
        self.result, self.schedule, self.support = fixture()

    def check(self):
        return report.endpoint_checks(self.result, self.schedule, self.support)

    def test_negative_heldout_gain_keeps_all_169_buckets_and_fit_drop_denominators(self):
        self.result["held_out"][0]["gain"]["mean"] = -1.5
        selection = self.check()
        self.assertEqual(selection["retainedBuckets"], [0])
        self.assertEqual(selection["noPositiveFitGainBuckets"], [1])
        self.assertEqual(selection["insufficientEssBuckets"], list(range(2, 169)))
        self.assertEqual(selection["fitWeightFractions"], dict(retained=128 / 257,
                         insufficientEss=1 / 257, noPositiveFitGain=128 / 257))

    def test_rejects_missing_last_support_or_fitted_bucket(self):
        self.support["rows"].pop()
        with self.assertRaisesRegex(ValueError, "complete support"):
            self.check()
        self.result, self.schedule, self.support = fixture()
        self.result["fit"]["rows"].pop()
        with self.assertRaisesRegex(ValueError, "complete fit"):
            self.check()

    def test_rejects_foreign_or_duplicate_own_information_key(self):
        self.result["fit"]["rows"][0]["key"]["player"] = 3
        with self.assertRaisesRegex(ValueError, "own-information key"):
            self.check()
        self.result, self.schedule, self.support = fixture()
        self.result["fit"]["rows"][1]["key"] = self.result["fit"]["rows"][0]["key"]
        with self.assertRaisesRegex(ValueError, "own-information key"):
            self.check()

    def test_missing_and_stored_zero_have_distinct_source_flags(self):
        self.support["rows"][0].update(regrets=None, average_strategy=None)
        self.result["fit"]["rows"][0]["baseline_source"] = "uniform-fallback"
        self.support["rows"][1]["average_strategy"] = None
        self.result["fit"]["rows"][1]["baseline_source"] = "regret-fallback"
        self.check()
        self.result["fit"]["rows"][1]["baseline_source"] = "uniform-fallback"
        with self.assertRaisesRegex(ValueError, "baseline source/support"):
            self.check()

    def test_rejects_singleton_gain_and_later_tied_argmax(self):
        self.result["fit"]["rows"][2]["action_gains"][0] = estimate(1.0)
        with self.assertRaisesRegex(ValueError, "per-key gain denominator"):
            self.check()
        self.result, self.schedule, self.support = fixture()
        self.result["fit"]["rows"][0]["selected_action"] = 1
        with self.assertRaisesRegex(ValueError, "first argmax"):
            self.check()

    def test_rejects_fit_selected_subset_denominator(self):
        self.result["fit"]["sampling"]["relative_weight_mean"] = estimate(128 / 65536)
        with self.assertRaisesRegex(ValueError, "all-key fit weight partition"):
            self.check()

    def test_rejects_false_zero_ess_and_low_ess_selection(self):
        self.result["fit"]["rows"][0]["effective_sample_size"] = 0.0
        with self.assertRaisesRegex(ValueError, "concentration lower bound"):
            self.check()
        self.result["fit"]["rows"][0]["effective_sample_size"] = 63.9
        with self.assertRaisesRegex(ValueError, "fit-only gate"):
            self.check()

    def test_rejects_nonzero_gain_without_candidate_and_incomplete_fit_replays(self):
        held = self.result["held_out"][0]
        held["sampling"]["terminal_replays"] = 64
        held["retained_key_weight_fraction"] = estimate(0.0)
        with self.assertRaisesRegex(ValueError, "unsupported worlds retain baseline"):
            self.check()
        self.result, self.schedule, self.support = fixture()
        self.result["fit"]["sampling"]["terminal_replays"] -= 1
        with self.assertRaisesRegex(ValueError, "fit action replay enumeration"):
            self.check()

    def test_rejects_source_partition_and_shared_fit_heldout_seed(self):
        self.result["held_out"][0]["sampling"]["coverage_by_street"][0]["average_fraction"] = estimate(0.9)
        with self.assertRaisesRegex(ValueError, "source partition"):
            self.check()
        self.result, self.schedule, self.support = fixture()
        self.result["config"]["held_out_seeds"][0] = self.schedule["heldOutSeeds"][0] = 602
        with self.assertRaisesRegex(ValueError, "independent fit/held-out"):
            self.check()

    def test_proposal_is_root_or_partial_prefix_without_endpoint_action(self):
        for path in ([], [0, 0, 0, 0], [0, 0, 0, 0, 1, 2, 2, 2]):
            with self.subTest(path=path):
                self.result["action_indices"] = path
                proposal = self.result["proposal"]
                proposal["preflop_actions"] = path.copy()
                report.proposal_checks(proposal, self.result, [2] * 32)
                proposal["preflop_actions"].append(1)
                with self.assertRaisesRegex(ValueError, "excluding endpoint action"):
                    report.proposal_checks(proposal, self.result, [2] * 32)

    def test_proposal_retains_folded_seats_and_never_labels_relative_weight_as_reach(self):
        self.result["proposal"]["positive_target_combos_by_seat"].pop()
        with self.assertRaisesRegex(ValueError, "retain blockers"):
            report.proposal_checks(self.result["proposal"], self.result, [2] * 32)
        self.result, self.schedule, self.support = fixture()
        self.result["fit"]["sampling"]["reach_probability"] = estimate(0.5)
        with self.assertRaisesRegex(ValueError, "not absolute reach"):
            report.proposal_checks(self.result["proposal"], self.result, [2] * 32)

    def test_absolute_root_unit_reach_and_empty_source_prefix(self):
        s = sampling(128, 128, 801, 128)
        s["seats"] = s.pop("baseline_seats")
        s["reach_probability"] = s.pop("relative_weight_mean")
        self.support["action_indices"] = []
        s.update(history=self.support["history"], action_indices=[])
        value = dict(samples=128, seed=801, total_deal_attempts=128, prefixes=[s])
        report.root_checks(value, dict(samples=128), [self.support])
        s["reach_probability"] = estimate(0.5)
        with self.assertRaisesRegex(ValueError, "unit probability"):
            report.root_checks(value, dict(samples=128), [self.support])
        s["reach_probability"] = estimate(1.0)
        s["prefix_regret_fallback_fraction"] = estimate(1.0)
        with self.assertRaisesRegex(ValueError, "no policy sources"):
            report.root_checks(value, dict(samples=128), [self.support])

    def test_exact_learning_replay_allows_only_driver_time_change(self):
        raw = {name: "same" for name in ("configurationFingerprint", "abstractionFingerprint", "solverStateVersion",
                "threads", "nodes", "supportNodes", "coveragePrefixes", "effectiveConfigBlake3")}
        raw.update(result=dict(current_regret_fingerprint="same", metrics=dict(sweeps=32768),
                               histories=[dict(strategy=[0.4, 0.6])], solve_elapsed_secs=1.0),
                   diagnostics=dict(support=[self.support]))
        changed = copy.deepcopy(raw)
        changed["result"]["solve_elapsed_secs"] = 2.0
        report.learning_replay_checks(changed, raw)
        changed["result"]["histories"][0]["strategy"][0] = 0.5
        with self.assertRaisesRegex(ValueError, "all learning metrics"):
            report.learning_replay_checks(changed, raw)
        changed = copy.deepcopy(raw)
        changed["diagnostics"]["support"][0]["rows"][168]["regrets"][0] = 1.0
        with self.assertRaisesRegex(ValueError, "all sixteen raw support"):
            report.learning_replay_checks(changed, raw)

    def test_complete_evidence_and_byte_identical_json_regeneration(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            first = report.evidence_checks(run, exp)
            second = report.evidence_checks(run, exp)
            encode = lambda value: (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode()
            self.assertEqual(encode(first), encode(second))

    def test_rejects_changed_archive_despite_updated_outer_hash(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            with zipfile.ZipFile(run / "source.zip", "w") as archive:
                for i in range(167):
                    archive.writestr(f"fixture/{i}.rs", "changed" if i == 2 else str(i))
            exp["sourceZipSha256"] = report.sha(run / "source.zip")
            with self.assertRaisesRegex(ValueError, "archived source"):
                report.evidence_checks(run, exp)

    def test_rejects_missing_command_and_changed_test_log_totals(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            path = run / "verification/verification.json"
            original = report.read(path)
            value = copy.deepcopy(original)
            value["checks"].pop()
            path.write_text(json.dumps(value), encoding="utf-8")
            exp["verificationSha256"] = report.sha(path)
            with self.assertRaisesRegex(ValueError, "seven required"):
                report.evidence_checks(run, exp)
            value = copy.deepcopy(original)
            next(c for c in value["checks"] if c["command"].startswith("cargo test "))["passed"] = 2
            path.write_text(json.dumps(value), encoding="utf-8")
            exp["verificationSha256"] = report.sha(path)
            with self.assertRaisesRegex(ValueError, "test totals"):
                report.evidence_checks(run, exp)

    def test_rejects_unrecorded_test_script_identity(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            exp["testScriptSha256"] = "0" * 64
            with self.assertRaisesRegex(ValueError, "analysis test identity"):
                report.evidence_checks(run, exp)


if __name__ == "__main__":
    unittest.main()
