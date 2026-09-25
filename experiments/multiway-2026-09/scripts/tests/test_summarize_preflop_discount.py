"""Study-specific corruption tests; no solver run or retained local run required."""
import copy
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_preflop_discount as report
from test_summarize_preflop_endpoint import estimate, fixture as endpoint_fixture, sampling, temporary_evidence


def fixture():
    # The fixture is a validated-output shape, not a claim that these public
    # paths describe this small synthetic game. Real tree identity is checked
    # against the recursively validated baseline and complete parsed config.
    paths = ["root"] + ["/".join(["fold"] * i) for i in range(1, 16)]
    exp = dict(threads=8, memory="8GiB", nodes=paths, coveragePrefixes=[paths[i] for i in range(8)],
               endpointPaths=[paths[i] for i in (0, 4, 5, 6, 7)],
               endpointSchedule=dict(fitSamples=65536, fitSeed=602, heldOutSamples=131072, heldOutSeeds=[702, 703], minFitEss=64),
               rootSchedule=dict(samples=262144, seeds=[801, 802]),
               ordinarySchedule=dict(samples=128, coverageSamples=131072, seeds=[101, 202]),
               costGate=dict(maxPeriodicToNoDiscountDriverRatio=2.0), promotionAllowed=False, cloudResourcesStarted=False,
               cases=[dict(name=name, variant="uniform-one", trainingSeed=0, sweeps=131072, timeoutSeconds=1800,
                           discount=discount.copy()) for name, discount in zip(report.CASES, report.DISCOUNTS)])
    raw = {key: "a" * 64 for key in ("executableBlake3", "effectiveConfigBlake3", "configurationFingerprint", "abstractionFingerprint")}
    raw.update(schemaVersion="solvers.multiway-average-sampling-research/v1", sourceRevision="b" * 64,
               solverStateVersion=2, threads=8, nodes=[], supportNodes=[], coveragePrefixes=[], endpointPrefixes=[],
               result=dict(variant="uniform-one", threads=8, current_regret_fingerprint="c" * 64,
                           metrics=dict(sweeps=131072, traversals=786432, infosets=2704, memory_bytes=1000000,
                                        total_deal_attempts=786432, mean_deal_attempts=1.0, hand_updates=900000,
                                        average_positive_regret=[1.0] * 6), histories=[]),
               diagnostics=dict(support=[], endpoints=[], root_evaluations=[]))
    endpoints = []
    for i, path in enumerate(paths):
        endpoint, _, support = endpoint_fixture()
        context = dict(requested=path, history=bytes([i] * 16).hex(), actor=1, street="preflop", activeOpponents=1)
        support.update(history=[i] * 16, action_indices=[0] * i)
        endpoint.update(history=[i] * 16, action_indices=[0] * i,
                        configuration_fingerprint=[170] * 32, abstraction_fingerprint=[170] * 32)
        endpoint["proposal"].update(preflop_history=[i] * 16, preflop_actions=[0] * i)
        for bucket, row in enumerate(support["rows"]):
            row["key"] = report.pilot.key_for(support, bucket)
            endpoint["fit"]["rows"][bucket]["key"] = copy.deepcopy(row["key"])
        raw["nodes"].append(context)
        raw["diagnostics"]["support"].append(support)
        raw["result"]["histories"].append(dict(history=support["history"], strategies=[
            dict(key=row["key"], status="average-observed", actions=[dict(action=label, probability=value)
                 for label, value in zip(support["action_labels"], row["average_strategy"], strict=True)])
            for row in support["rows"]]))
        endpoints.append(endpoint)
    raw["supportNodes"] = copy.deepcopy(raw["nodes"])
    raw["coveragePrefixes"] = copy.deepcopy(raw["nodes"][:8])
    raw["endpointPrefixes"] = [copy.deepcopy(raw["nodes"][i]) for i in (0, 4, 5, 6, 7)]
    raw["diagnostics"]["endpoints"] = [endpoints[i] for i in (0, 4, 5, 6, 7)]
    ds = exp["endpointSchedule"]
    raw["diagnostics"]["config"] = dict(support_paths=[s["action_indices"] for s in raw["diagnostics"]["support"]],
        endpoint_paths=[e["action_indices"] for e in raw["diagnostics"]["endpoints"]],
        endpoint=dict(fit_samples=ds["fitSamples"], fit_seed=ds["fitSeed"], held_out_samples=ds["heldOutSamples"],
                      held_out_seeds=ds["heldOutSeeds"], min_fit_ess=ds["minFitEss"]),
        root_samples=exp["rootSchedule"]["samples"], root_seeds=exp["rootSchedule"]["seeds"])
    for seed in exp["rootSchedule"]["seeds"]:
        n = exp["rootSchedule"]["samples"]
        prefixes = []
        for endpoint in raw["diagnostics"]["endpoints"]:
            prefix = sampling(n, n, seed, n)
            prefix["seats"] = prefix.pop("baseline_seats")
            prefix["reach_probability"] = prefix.pop("relative_weight_mean")
            prefix.update(history=endpoint["history"], action_indices=endpoint["action_indices"])
            prefixes.append(prefix)
        raw["diagnostics"]["root_evaluations"].append(dict(samples=n, seed=seed, total_deal_attempts=n, prefixes=prefixes))
    def coverage(n):
        result = []
        for seat in range(6):
            row = {}
            for source in report.pilot.SOURCES:
                amount = n if seat == 1 and source in {"decision", "stored_strategy", "average_strategy"} else 0
                row[source + "_visits"] = amount
                row[source + "_visits_by_street"] = {s: amount if s == "preflop" else 0 for s in report.pilot.STREETS}
            result.append(row)
        return result
    def profile(n, baseline):
        seats = [dict(mean=0.0, stderr=0.0, ci95=[0.0, 0.0])] * 6
        return dict(samples=n, total_deal_attempts=n, seats=seats,
                    deviation_gain_lower_bound=None if baseline else copy.deepcopy(seats), candidate_policy_coverage=coverage(n))
    raw["result"]["evaluations"] = [dict(seed=seed, result=profile(128, False)) for seed in (101, 202)]
    raw["result"]["coverage_evaluations"] = [dict(seed=seed, result=dict(evaluation=profile(131072, True),
        prefixes=[dict(history=list(bytes.fromhex(context["history"])),
                       reached_samples=131072, trajectory_visits_by_street={s: 131072 if s == "preflop" else 0 for s in report.pilot.STREETS},
                       candidate_policy_coverage=coverage(131072)) for context in raw["coveragePrefixes"]])) for seed in (101, 202)]
    return exp, raw


class PreflopDiscountValidationTests(unittest.TestCase):
    def setUp(self):
        self.exp, self.raw = fixture()

    def test_schedule_accepts_only_two_fixed_learning_arms(self):
        report.schedule_checks(self.exp)
        self.exp["cases"][1]["sweeps"] = 32768
        with self.assertRaisesRegex(ValueError, "predeclared case"):
            report.schedule_checks(self.exp)

    def test_configuration_allows_lf_rewrite_and_only_discount_table_difference(self):
        with temporary_evidence() as run:
            baseline = run / "baseline.toml"
            baseline.write_bytes(b'[solver]\r\nseed=0\r\nbatch_sweeps=4\r\n[solver.discount]\r\nkind="none"\r\n')
            config = run / "periodic.toml"
            config.write_text('[solver]\nseed=0\nbatch_sweeps=4\n[solver.discount]\nkind="periodic"\nevery_sweeps=10000\nuntil_sweeps=10000000\n', encoding="utf-8")
            case = {**self.exp["cases"][1], "config": str(config), "configSha256": report.sha(config)}
            report.config_checks(case, baseline)
            config.write_text(config.read_text().replace("batch_sweeps=4", "batch_sweeps=8"), encoding="utf-8")
            case["configSha256"] = report.sha(config)
            with self.assertRaisesRegex(ValueError, "only discount table"):
                report.config_checks(case, baseline)

    def test_configuration_rejects_unrecorded_discount_cadence(self):
        with temporary_evidence() as run:
            config = run / "config.toml"
            config.write_text('[solver]\nseed=0\n[solver.discount]\nkind="periodic"\nevery_sweeps=9999\nuntil_sweeps=10000000\n', encoding="utf-8")
            case = {**self.exp["cases"][1], "config": str(config), "configSha256": report.sha(config)}
            with self.assertRaisesRegex(ValueError, "declared discount"):
                report.config_checks(case, config)

    def test_model_comparison_allows_learning_changes_but_requires_correct_fingerprint_scope(self):
        other = copy.deepcopy(self.raw)
        other["result"]["current_regret_fingerprint"] = "d" * 64
        other["diagnostics"]["support"][0]["rows"][0]["regrets"][0] = 12.0
        other["effectiveConfigBlake3"] = "e" * 64
        report.model_checks(other, self.raw, self.exp["cases"][0])
        other["configurationFingerprint"] = "f" * 64
        report.model_checks(other, self.raw, self.exp["cases"][1])
        with self.assertRaisesRegex(ValueError, "no-discount model fingerprint"):
            report.model_checks(other, self.raw, self.exp["cases"][0])

    def test_model_comparison_rejects_changed_menu_or_source(self):
        changed = copy.deepcopy(self.raw)
        changed["diagnostics"]["support"][0]["action_labels"][1] = "raise-to:4000"
        with self.assertRaisesRegex(ValueError, "public menu/layout"):
            report.model_checks(changed, self.raw, self.exp["cases"][0])
        changed = copy.deepcopy(self.raw)
        changed["sourceRevision"] = "changed"
        with self.assertRaisesRegex(ValueError, "same executable"):
            report.model_checks(changed, self.raw, self.exp["cases"][0])

    def test_learning_budget_and_deal_accounting_are_validated_without_regret_ranking(self):
        report.learning_checks(self.raw, self.exp["cases"][0])
        self.raw["result"]["metrics"]["average_positive_regret"] = [0.001] * 6
        report.learning_checks(self.raw, self.exp["cases"][0])
        self.raw["result"]["metrics"]["mean_deal_attempts"] = 2.0
        with self.assertRaisesRegex(ValueError, "deal accounting"):
            report.learning_checks(self.raw, self.exp["cases"][0])

    def test_full_support_and_normalized_history_exports_agree(self):
        self.assertEqual(len(report.support_history_checks(self.raw, self.exp)), 16)
        self.raw["result"]["histories"][0]["strategies"][168]["actions"][0]["probability"] += .01
        with self.assertRaisesRegex(ValueError, "normalized policy agreement"):
            report.support_history_checks(self.raw, self.exp)

    def test_missing_bucket_cannot_be_hidden_by_matching_history_cardinality(self):
        self.raw["diagnostics"]["support"][0]["rows"].pop()
        self.raw["result"]["histories"][0]["strategies"].pop()
        with self.assertRaisesRegex(ValueError, "complete support"):
            report.support_history_checks(self.raw, self.exp)

    def test_ordinary_and_coverage_partitions_and_root_equality(self):
        self.assertEqual(len(report.ordinary_checks(self.raw, self.exp)), 16)
        coverage = self.raw["result"]["coverage_evaluations"][0]["result"]
        coverage["prefixes"][0]["reached_samples"] -= 1
        with self.assertRaisesRegex(ValueError, "trajectory count"):
            report.ordinary_checks(self.raw, self.exp)

    def test_coverage_cannot_swap_seeds_or_drop_requested_prefix(self):
        self.raw["result"]["coverage_evaluations"][1]["seed"] = 101
        with self.assertRaisesRegex(ValueError, "seed/cardinality"):
            report.ordinary_checks(self.raw, self.exp)
        self.exp, self.raw = fixture()
        self.raw["result"]["coverage_evaluations"][0]["result"]["prefixes"].pop()
        with self.assertRaisesRegex(ValueError, "prefix cardinality"):
            report.ordinary_checks(self.raw, self.exp)

    def test_all_five_endpoint_results_accept_signed_losses(self):
        baseline = copy.deepcopy(self.raw)
        self.raw["diagnostics"]["endpoints"][0]["held_out"][0]["gain"]["mean"] = -3.5
        results = report.endpoint_checks(self.raw, self.exp, baseline)
        self.assertEqual(len(results), 5)
        self.assertEqual(results[0]["result"]["held_out"][0]["gain"]["mean"], -3.5)
        previous = report.endpoint_checks(baseline, self.exp, baseline)
        comparison = report.endpoint_comparisons(results, previous)
        self.assertEqual(comparison[0]["heldOut"][0]["descriptiveGainMeanChange"], -4.5)
        self.assertFalse(comparison[0]["heldOut"][0]["pairedEstimate"])

    def test_endpoint_proposal_cannot_include_target_action(self):
        self.raw["diagnostics"]["endpoints"][0]["proposal"]["preflop_actions"].append(1)
        with self.assertRaisesRegex(ValueError, "excluding endpoint action"):
            report.endpoint_checks(self.raw, self.exp, self.raw)

    def test_endpoint_frozen_fit_gate_and_full_denominator_are_checked(self):
        self.raw["diagnostics"]["endpoints"][0]["fit"]["rows"][0]["effective_sample_size"] = 63.9
        with self.assertRaisesRegex(ValueError, "fit-only gate"):
            report.endpoint_checks(self.raw, self.exp, self.raw)
        self.exp, self.raw = fixture()
        self.raw["diagnostics"]["endpoints"][1]["fit"]["sampling"]["relative_weight_mean"] = estimate(128 / 65536)
        with self.assertRaisesRegex(ValueError, "all-key fit weight partition"):
            report.endpoint_checks(self.raw, self.exp, self.raw)

    def test_cost_gate_uses_driver_clock_inclusive_bound_and_requires_both_arms(self):
        cases = [dict(case=name, solveSeconds=seconds, diagnosticsSeconds=10000.0)
                 for name, seconds in zip(report.CASES, (10.0, 20.0))]
        self.assertTrue(report.cost_gate(cases, self.exp)["passed"])
        cases[1]["solveSeconds"] = 20.01
        self.assertFalse(report.cost_gate(cases, self.exp)["passed"])
        with self.assertRaisesRegex(ValueError, "both complete"):
            report.cost_gate(cases[:1], self.exp)

    def test_single_case_does_not_require_unfinished_arm_or_report_cost_pass(self):
        with temporary_evidence() as run:
            (run / "experiment.json").write_bytes(report.serialized(self.exp))
            with patch.object(report, "evidence_checks", return_value=({"rootEvaluations": []}, {})), \
                 patch.object(report, "summarize_case", side_effect=lambda run, exp, case, *unused: {"case": case["name"]}) as validate:
                # The source-provenance fields are already verified by the
                # separately tested evidence validator in a real invocation.
                self.exp.update(sourceEvidenceRun="fixture", sourceManifestSha256="a" * 64,
                    binarySha256="b" * 64, verificationSha256="c" * 64, baselineSummarySha256="d" * 64)
                (run / "experiment.json").write_bytes(report.serialized(self.exp))
                result = report.summarize(run, report.CASES[0])
                self.assertEqual(validate.call_count, 1)
                self.assertIsNone(result["costGate"])
                self.assertIsNone(result["periodicVersusNoDiscount"])
                self.assertEqual(result["status"], "completed-case")

    def test_recursive_baseline_requires_canonical_bytes_not_only_updated_summary_hash(self):
        with temporary_evidence() as run:
            source = run / "source"
            source.mkdir()
            (source / "research.exe").write_bytes(b"non-executable")
            (source / "experiment.json").write_bytes(b"{}")
            baseline_exp = {**self.exp, "baseRevision": "fixture", "sourceManifestSha256": "1" * 64,
                            "sourceZipSha256": "2" * 64, "binarySha256": report.sha(source / "research.exe"),
                            "verificationSha256": "3" * 64, "runnerSha256": "4" * 64}
            baseline = dict(experiment=baseline_exp, endpoints=[], rootEvaluations=[])
            summary_path = source / "summary.json"
            summary_path.write_bytes(report.serialized(baseline))
            raw_folder = source / report.preflop.CASE
            raw_folder.mkdir()
            (raw_folder / "stdout.json").write_bytes(b"{}")
            exp = {**baseline_exp, "sourceEvidenceRun": str(source), "baselineSummary": str(summary_path),
                   "baselineSummarySha256": report.sha(summary_path), "baselineExperimentSha256": report.sha(source / "experiment.json"),
                   "binary": str(source / "research.exe"), "summarizerSha256": report.sha(report.__file__),
                   "testScriptSha256": report.sha(__file__), "dependencies": {m.__file__: report.sha(m.__file__) for m in
                       [report.preflop, report.pilot, report.preflop.balanced, report.preflop.endpoint_dependency,
                        report.pilot.support_dependency, report.pilot.evidence_dependency]}}
            with patch.object(report.preflop, "summarize", return_value=baseline) as regenerate:
                report.evidence_checks(exp)
                self.assertEqual(regenerate.call_count, 1)
                summary_path.write_text(json.dumps(baseline), encoding="utf-8")
                exp["baselineSummarySha256"] = report.sha(summary_path)
                with self.assertRaisesRegex(ValueError, "byte regeneration"):
                    report.evidence_checks(exp)

    def test_measured_job_rejects_changed_budget_even_with_matching_updated_hashes(self):
        with temporary_evidence() as run:
            case = self.exp["cases"][0]
            config = run / "config.toml"
            config.write_bytes(b"fixture")
            case.update(config=str(config), configSha256=report.sha(config))
            self.exp.update(binary=str(run / "shared.exe"), binarySha256="1" * 64,
                            sourceManifestSha256="2" * 64, baseRevision="fixture", validationReport="fixture.md")
            opts = {"--config": str(config), "--cache-dir": str(run / "cache"), "--variant": "uniform-one",
                    "--sweeps": "131072", "--threads": "8", "--memory": "8GiB", "--source-revision": "2" * 64,
                    "--evaluation-seeds": "101,202", "--evaluation-samples": "128", "--coverage-samples": "131072",
                    "--endpoint-fit-samples": "65536", "--endpoint-fit-seed": "602", "--endpoint-samples": "131072",
                    "--endpoint-seeds": "702,703", "--endpoint-min-fit-ess": "64", "--root-samples": "262144", "--root-seeds": "801,802"}
            arguments = [item for pair in opts.items() for item in pair]
            for flag, field in [("--node", "nodes"), ("--support-node", "nodes"),
                                ("--coverage-prefix", "coveragePrefixes"), ("--endpoint-prefix", "endpointPaths")]:
                for path in self.exp[field]:
                    arguments.extend([flag, path])
            job = dict(arguments=arguments, sourceManifestSha256="2" * 64, validationReport="fixture.md",
                       configSha256=case["configSha256"], timeoutSeconds=1800)
            job_path = run / (case["name"] + "-job.json")
            job_path.write_bytes(report.serialized(job))
            case["jobSha256"] = report.sha(job_path)
            folder = run / case["name"]
            folder.mkdir()
            self.raw["config"] = str(config)
            (folder / "stdout.json").write_bytes(report.serialized(self.raw))
            measured = {**job, "schemaVersion": "solvers.average-sampling-measurement/v1", "exitCode": 0, "timedOut": False,
                        "binary": self.exp["binary"], "binarySha256": self.exp["binarySha256"], "job": str(job_path),
                        "jobSha256": case["jobSha256"], "stdoutSha256": report.sha(folder / "stdout.json"), "sourceRevision": "fixture"}
            measured_path = folder / "measurement.json"
            measured_path.write_bytes(report.serialized(measured))
            report.measurement_checks(run, self.exp, case, self.raw)
            arguments[arguments.index("--sweeps") + 1] = "131073"
            job_path.write_bytes(report.serialized(job))
            measured["jobSha256"] = case["jobSha256"] = report.sha(job_path)
            measured_path.write_bytes(report.serialized(measured))
            with self.assertRaisesRegex(ValueError, "literal diagnostic/training budget"):
                report.measurement_checks(run, self.exp, case, self.raw)


if __name__ == "__main__":
    unittest.main()
