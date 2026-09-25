"""Seed-replication corruption tests, using two explicitly hashed fixtures."""
import copy
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_preflop_discount_seed as report
from test_summarize_preflop_discount import fixture, temporary_evidence


def seed_fixture():
    previous_exp, old = fixture()
    exp = copy.deepcopy(previous_exp)
    exp["trainingSeed"] = 11
    exp["cases"] = [{**copy.deepcopy(case), "name": name, "trainingSeed": 11}
                    for case, name in zip(reversed(previous_exp["cases"]), report.CASES)]
    periodic = copy.deepcopy(old)
    periodic["configurationFingerprint"] = "d" * 64
    current = copy.deepcopy(old)
    current["configurationFingerprint"] = "e" * 64
    for endpoint in current["diagnostics"]["endpoints"]:
        endpoint["configuration_fingerprint"] = [238] * 32
    return exp, current, previous_exp, {"none": old, "periodic": periodic}


def analysis_metadata():
    return dict(summarizerSha256=report.sha(report.__file__), testScriptSha256=report.sha(__file__),
                dependencies={module.__file__: report.sha(module.__file__) for module in report.runtime_modules()},
                testDependencies={str(Path(__file__).with_name(name)): report.sha(Path(__file__).with_name(name))
                                  for name in report.TEST_DEPENDENCIES})


class PreflopDiscountSeedValidationTests(unittest.TestCase):
    def setUp(self):
        self.exp, self.raw, self.previous_exp, self.prior_raw = seed_fixture()

    def test_fixed_seed_and_reversed_order_are_required(self):
        report.schedule_checks(self.exp)
        self.exp["cases"].reverse()
        with self.assertRaisesRegex(ValueError, "reversed execution order"):
            report.schedule_checks(self.exp)
        self.exp["cases"].reverse()
        self.exp["cases"][0]["trainingSeed"] = 0
        with self.assertRaisesRegex(ValueError, "reversed execution order"):
            report.schedule_checks(self.exp)

    def test_preexecution_preserves_all_substantive_fields(self):
        with temporary_evidence() as run:
            path = run / "experiment-preexecution.json"
            path.write_bytes(report.serialized(self.exp))
            self.exp.update(preexecutionSha256=report.sha(path), status="periodic-running", completedUtc="fixture", **analysis_metadata())
            report.preexecution_checks(run, self.exp)
            self.exp["cases"][0]["jobSha256"] = "changed"
            with self.assertRaisesRegex(ValueError, "substantive fields"):
                report.preexecution_checks(run, self.exp)

    def test_preexecution_rejects_unknown_added_configuration_metadata(self):
        with temporary_evidence() as run:
            path = run / "experiment-preexecution.json"
            path.write_bytes(report.serialized(self.exp))
            self.exp.update(preexecutionSha256=report.sha(path), revisedFitBudget=42)
            with self.assertRaisesRegex(ValueError, "substantive fields"):
                report.preexecution_checks(run, self.exp)
            path.write_bytes(b"{}")
            with self.assertRaisesRegex(ValueError, "snapshot hash"):
                report.preexecution_checks(run, self.exp)

    def test_runtime_and_transitive_fixture_hashes_are_explicit(self):
        exp = analysis_metadata()
        report.analysis_identity_checks(exp)
        exp["testDependencies"].pop(next(iter(exp["testDependencies"])))
        with self.assertRaisesRegex(ValueError, "test-fixture dependency set"):
            report.analysis_identity_checks(exp)
        exp = analysis_metadata()
        exp["dependencies"].pop(next(iter(exp["dependencies"])))
        with self.assertRaisesRegex(ValueError, "seven-helper"):
            report.analysis_identity_checks(exp)

    def test_changed_fixture_hash_is_rejected(self):
        exp = analysis_metadata()
        exp["testDependencies"][next(iter(exp["testDependencies"]))] = "0" * 64
        with self.assertRaisesRegex(ValueError, "test-fixture identity"):
            report.analysis_identity_checks(exp)

    def test_seed_eleven_config_changes_only_seed_not_discount_or_batch(self):
        with temporary_evidence() as run:
            text = '[solver]\nseed=0\nbatch_sweeps=4\n[solver.discount]\nkind="periodic"\nevery_sweeps=10000\nuntil_sweeps=10000000\n'
            old, new = run / "seed0.toml", run / "seed11.toml"
            old.write_bytes(text.replace("\n", "\r\n").encode())
            new.write_bytes(text.replace("seed=0", "seed=11").encode())
            prior = {**self.previous_exp["cases"][1], "config": str(old), "configSha256": report.sha(old)}
            case = {**self.exp["cases"][0], "config": str(new), "configSha256": report.sha(new)}
            report.config_checks(case, prior)
            new.write_bytes(text.replace("seed=0", "seed=11").replace("batch_sweeps=4", "batch_sweeps=8").encode())
            case["configSha256"] = report.sha(new)
            with self.assertRaisesRegex(ValueError, "only solver.seed"):
                report.config_checks(case, prior)

    def test_wrong_actual_seed_rejected_even_with_new_hash(self):
        with temporary_evidence() as run:
            old, new = run / "seed0.toml", run / "seed11.toml"
            text = '[solver]\nseed=0\n[solver.discount]\nkind="none"\n'
            old.write_bytes(text.encode())
            new.write_bytes(text.replace("seed=0", "seed=29").encode())
            prior = {**self.previous_exp["cases"][0], "config": str(old), "configSha256": report.sha(old)}
            case = {**self.exp["cases"][1], "config": str(new), "configSha256": report.sha(new)}
            with self.assertRaisesRegex(ValueError, "training seed"):
                report.config_checks(case, prior)

    def test_corresponding_prior_arm_is_mapped_by_discount_not_position(self):
        prior = {"experiment": self.previous_exp}
        self.assertEqual(report.corresponding_prior_case(prior, self.exp["cases"][0])["name"], report.discount.CASES[1])
        self.assertEqual(report.corresponding_prior_case(prior, self.exp["cases"][1])["name"], report.discount.CASES[0])
        self.previous_exp["cases"][0]["discount"] = self.previous_exp["cases"][1]["discount"]
        with self.assertRaisesRegex(ValueError, "unique corresponding"):
            report.corresponding_prior_case(prior, self.exp["cases"][0])

    def test_cross_seed_fingerprints_must_change_while_regret_changes_are_allowed(self):
        self.raw["result"]["current_regret_fingerprint"] = "f" * 64
        self.raw["diagnostics"]["support"][0]["rows"][0]["regrets"][0] = 12.0
        report.model_checks(self.raw, self.prior_raw)
        self.raw["configurationFingerprint"] = self.prior_raw["periodic"]["configurationFingerprint"]
        with self.assertRaisesRegex(ValueError, "must change across training seeds"):
            report.model_checks(self.raw, self.prior_raw)

    def test_cross_seed_public_menu_or_executable_change_is_rejected(self):
        self.raw["diagnostics"]["support"][0]["action_labels"][1] = "raise-to:4000"
        with self.assertRaisesRegex(ValueError, "public menu/layout"):
            report.model_checks(self.raw, self.prior_raw)
        self.exp, self.raw, self.previous_exp, self.prior_raw = seed_fixture()
        self.raw["executableBlake3"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "same executable"):
            report.model_checks(self.raw, self.prior_raw)

    def test_cost_ratio_uses_arm_identity_under_reversed_order(self):
        cases = [dict(case=name, discount=case["discount"], solveSeconds=seconds, configurationFingerprint=str(i))
                 for i, (name, case, seconds) in enumerate(zip(report.CASES, self.exp["cases"], (12.0, 10.0)))]
        self.assertEqual(report.cost_gate(cases, self.exp)["periodicToNoDiscountDriverRatio"], 1.2)
        self.assertEqual(report.cost_gate(list(reversed(cases)), self.exp)["periodicToNoDiscountDriverRatio"], 1.2)
        cases[0]["solveSeconds"] = 20.0
        self.assertTrue(report.cost_gate(cases, self.exp)["passed"])
        cases[0]["solveSeconds"] = 20.01
        self.assertFalse(report.cost_gate(cases, self.exp)["passed"])
        with self.assertRaisesRegex(ValueError, "both complete"):
            report.cost_gate(cases[:1], self.exp)

    def test_discount_arms_cannot_share_configuration_identity(self):
        cases = [dict(case=name, discount=case["discount"], configurationFingerprint="same")
                 for name, case in zip(report.CASES, self.exp["cases"])]
        with self.assertRaisesRegex(ValueError, "different configuration"):
            report.pair_checks(cases)

    def test_reused_leaf_validators_preserve_full_rows_negative_gain_and_root_reach(self):
        self.raw["diagnostics"]["endpoints"][0]["held_out"][0]["gain"]["mean"] = -4.0
        report.discount.learning_checks(self.raw, self.exp["cases"][0])
        report.discount.support_history_checks(self.raw, self.exp)
        self.assertEqual(len(report.discount.ordinary_checks(self.raw, self.exp)), 16)
        endpoints = report.discount.endpoint_checks(self.raw, self.exp, self.prior_raw["periodic"])
        self.assertEqual(len(endpoints), 5)
        self.assertEqual(len(endpoints[0]["result"]["fit"]["rows"]), 169)
        self.assertEqual(endpoints[0]["result"]["held_out"][0]["gain"]["mean"], -4.0)
        self.raw["diagnostics"]["endpoints"][0]["fit"]["rows"][0]["effective_sample_size"] = 63.0
        with self.assertRaisesRegex(ValueError, "fit-only gate"):
            report.discount.endpoint_checks(self.raw, self.exp, self.prior_raw["periodic"])

    def test_recursive_prior_validation_requires_matching_regenerated_bytes(self):
        with temporary_evidence() as run:
            prior_run, source = run / "prior", run / "source"
            prior_run.mkdir()
            source.mkdir()
            (source / "research.exe").write_bytes(b"non-executable fixture")
            previous_exp = {**self.previous_exp, "baseRevision": "fixture", "sourceManifestSha256": "1" * 64,
                "sourceZipSha256": "2" * 64, "binarySha256": report.sha(source / "research.exe"),
                "verificationSha256": "3" * 64, "runnerSha256": "4" * 64,
                "sourceEvidenceRun": str(source), "binary": str(source / "research.exe")}
            prior = dict(experiment=previous_exp)
            for case in previous_exp["cases"]:
                folder = prior_run / case["name"]
                folder.mkdir()
                (folder / "stdout.json").write_bytes(b"{}")
            (prior_run / "experiment.json").write_bytes(report.serialized(previous_exp))
            summary_path = prior_run / "summary.json"
            summary_path.write_bytes(report.serialized(prior))
            exp = {**previous_exp, **analysis_metadata(), "priorPilotRun": str(prior_run),
                "priorPilotExperimentSha256": report.sha(prior_run / "experiment.json"), "priorPilotSummarySha256": report.sha(summary_path)}
            with patch.object(report.discount, "summarize", return_value=prior) as validate:
                report.evidence_checks(exp)
                self.assertEqual(validate.call_count, 1)
                summary_path.write_bytes(report.serialized({**prior, "changed": True}))
                exp["priorPilotSummarySha256"] = report.sha(summary_path)
                with self.assertRaisesRegex(ValueError, "byte regeneration"):
                    report.evidence_checks(exp)

    def test_case_mode_leaves_cost_and_current_pair_comparison_unavailable(self):
        with temporary_evidence() as run:
            self.exp.update(sourceEvidenceRun="fixture", sourceManifestSha256="1" * 64, binarySha256="2" * 64,
                verificationSha256="3" * 64, priorPilotRun="prior", priorPilotExperimentSha256="4" * 64,
                priorPilotSummarySha256="5" * 64)
            snapshot = run / "experiment-preexecution.json"
            snapshot.write_bytes(report.serialized(self.exp))
            self.exp["preexecutionSha256"] = report.sha(snapshot)
            (run / "experiment.json").write_bytes(report.serialized(self.exp))
            prior = dict(periodicVersusNoDiscount=[], cases=[dict(case=name, rootEvaluations=[]) for name in report.discount.CASES])
            with patch.object(report, "evidence_checks", return_value=(prior, {})), \
                 patch.object(report, "summarize_case", side_effect=lambda run, exp, case, *args: {"case": case["name"]}) as validate:
                value = report.summarize(run, report.CASES[0])
                self.assertEqual(validate.call_count, 1)
                self.assertEqual(value["status"], "completed-case")
                self.assertIsNone(value["costGate"])
                self.assertIsNone(value["periodicVersusNoDiscount"])
                self.assertFalse(value["promotionAllowed"])


if __name__ == "__main__":
    unittest.main()
