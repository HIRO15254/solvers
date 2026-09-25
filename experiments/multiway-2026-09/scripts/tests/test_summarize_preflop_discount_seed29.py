"""Bounded seed-29 evidence/corruption tests; no solver or retained run needed."""
import copy
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_preflop_discount_seed29 as report
from test_summarize_preflop_discount import fixture, temporary_evidence


def seed_fixture():
    old_exp, raw = fixture()
    exp = copy.deepcopy(old_exp)
    exp.update(trainingSeed=29, broaderGoalComplete=False)
    exp["cases"] = [{**copy.deepcopy(case), "name": name, "trainingSeed": 29}
                    for case, name in zip(old_exp["cases"], report.CASES)]
    old11 = copy.deepcopy(old_exp)
    old11["cases"] = [{**copy.deepcopy(case), "name": name, "trainingSeed": 11}
                      for case, name in zip(reversed(old_exp["cases"]), report.seed11.CASES)]
    prior = {0: {"experiment": old_exp}, 11: {"experiment": old11}}
    previous_raw = {}
    for seed, letters in ((0, ("a", "b")), (11, ("c", "d"))):
        previous_raw[seed] = {}
        for kind, letter in zip(("none", "periodic"), letters):
            previous_raw[seed][kind] = copy.deepcopy(raw)
            previous_raw[seed][kind]["configurationFingerprint"] = letter * 64
    raw["configurationFingerprint"] = "e" * 64
    for endpoint in raw["diagnostics"]["endpoints"]:
        endpoint["configuration_fingerprint"] = [238] * 32
    return exp, raw, prior, previous_raw


def analysis_metadata():
    return dict(summarizerSha256=report.sha(report.__file__), testScriptSha256=report.sha(__file__),
                dependencies={m.__file__: report.sha(m.__file__) for m in report.runtime_modules()},
                testDependencies={str(Path(__file__).with_name(name)): report.sha(Path(__file__).with_name(name))
                                  for name in report.TEST_DEPENDENCIES})


def write_configs(run, exp, previous):
    text = '[solver]\nseed=SEED\nbatch_sweeps=4\n[solver.discount]\nkind="KIND"\n'
    for seed, cases in [(29, exp["cases"]), *[(s, v["experiment"]["cases"]) for s, v in previous.items()]]:
        for case in cases:
            path = run / (case["name"] + ".toml")
            content = text.replace("SEED", str(seed)).replace("KIND", case["discount"]["kind"])
            if case["discount"]["kind"] == "periodic":
                content += 'every_sweeps=10000\nuntil_sweeps=10000000\n'
            path.write_bytes(content.replace("\n", "\r\n").encode() if seed == 0 else content.encode())
            case.update(config=str(path), configSha256=report.sha(path))


class Seed29ValidationTests(unittest.TestCase):
    def setUp(self):
        self.exp, self.raw, self.previous, self.prior_raw = seed_fixture()

    def test_fixed_seed_none_first_and_all_budgets(self):
        report.schedule_checks(self.exp)
        for mutate in (lambda e: e["cases"].reverse(),
                       lambda e: e["cases"][0].update(trainingSeed=11),
                       lambda e: e["cases"][0].update(sweeps=32768),
                       lambda e: e["cases"][1]["discount"].update(every_sweeps=5000)):
            changed = copy.deepcopy(self.exp)
            mutate(changed)
            with self.assertRaises(ValueError):
                report.schedule_checks(changed)

    def test_preexecution_allows_only_known_runtime_metadata(self):
        with temporary_evidence() as run:
            p = run / "experiment-preexecution.json"
            p.write_bytes(report.serialized(self.exp))
            self.exp.update(preexecutionSha256=report.sha(p), status="running", **analysis_metadata())
            report.seed11.preexecution_checks(run, self.exp)
            self.exp["endpointSchedule"]["heldOutSamples"] += 1
            with self.assertRaisesRegex(ValueError, "substantive fields"):
                report.seed11.preexecution_checks(run, self.exp)

    def test_runtime_and_transitive_test_dependencies_are_complete_and_immutable(self):
        exp = analysis_metadata()
        self.assertEqual(len(exp["dependencies"]), 8)
        self.assertEqual(len(exp["testDependencies"]), 2)
        report.analysis_identity_checks(exp)
        del exp["dependencies"][report.seed11.__file__]
        with self.assertRaisesRegex(ValueError, "eight-helper"):
            report.analysis_identity_checks(exp)
        exp = analysis_metadata()
        exp["testDependencies"][next(iter(exp["testDependencies"]))] = "0" * 64
        with self.assertRaisesRegex(ValueError, "test-fixture identity"):
            report.analysis_identity_checks(exp)

    def test_configs_allow_only_seed29_against_both_prior_arms(self):
        with temporary_evidence() as run:
            write_configs(run, self.exp, self.previous)
            for case in self.exp["cases"]:
                report.config_checks(case, self.previous)
            case = self.exp["cases"][0]
            path = Path(case["config"])
            original = path.read_text()
            for replacement in (original.replace("seed=29", "seed=11"), original.replace("batch_sweeps=4", "batch_sweeps=8")):
                path.write_text(replacement)
                case["configSha256"] = report.sha(path)
                with self.assertRaises(ValueError):
                    report.config_checks(case, self.previous)

    def test_seed11_config_is_checked_independently_from_seed_zero(self):
        with temporary_evidence() as run:
            write_configs(run, self.exp, self.previous)
            case = self.exp["cases"][0]
            old = report.corresponding_case(self.previous[11], case, 11)
            path = Path(old["config"])
            path.write_text(path.read_text().replace("batch_sweeps=4", "batch_sweeps=8"))
            old["configSha256"] = report.sha(path)
            with self.assertRaisesRegex(ValueError, "only solver.seed"):
                report.config_checks(case, self.previous)

    def test_prior_arm_mapping_is_by_discount_not_reversed_position(self):
        for seed, summary in self.previous.items():
            for case in self.exp["cases"]:
                self.assertEqual(report.corresponding_case(summary, case, seed)["discount"], case["discount"])
        self.previous[11]["experiment"]["cases"][0]["discount"] = {"kind": "none"}
        with self.assertRaisesRegex(ValueError, "unique corresponding"):
            report.corresponding_case(self.previous[11], self.exp["cases"][0], 11)

    def test_model_identity_differs_from_all_four_old_fingerprints(self):
        for previous in self.prior_raw.values():
            report.seed11.model_checks(self.raw, previous)
            for old in previous.values():
                changed = copy.deepcopy(self.raw)
                changed["configurationFingerprint"] = old["configurationFingerprint"]
                with self.assertRaisesRegex(ValueError, "must change across training seeds"):
                    report.seed11.model_checks(changed, previous)
        self.raw["executableBlake3"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "same executable"):
            report.seed11.model_checks(self.raw, self.prior_raw[11])

    def test_full_endpoint_rows_signed_loss_and_eligibility_are_reused(self):
        self.raw["diagnostics"]["endpoints"][4]["held_out"][0]["gain"]["mean"] = -2.0
        report.discount.learning_checks(self.raw, self.exp["cases"][0])
        support = report.discount.support_history_checks(self.raw, self.exp)
        self.assertEqual(len(support), 16)
        self.assertEqual(len(report.discount.ordinary_checks(self.raw, self.exp)), 16)
        values = report.discount.endpoint_checks(self.raw, self.exp, self.prior_raw[0]["none"])
        self.assertEqual([len(e["result"]["fit"]["rows"]) for e in values], [169] * 5)
        self.assertEqual(values[4]["result"]["held_out"][0]["gain"]["mean"], -2.0)
        self.raw["diagnostics"]["endpoints"][0]["fit"]["rows"][0]["effective_sample_size"] = 63.0
        with self.assertRaisesRegex(ValueError, "fit-only gate"):
            report.discount.endpoint_checks(self.raw, self.exp, self.prior_raw[0]["none"])

    def test_missing_rows_menu_and_source_partition_corruption_are_rejected(self):
        changed = copy.deepcopy(self.raw)
        changed["diagnostics"]["support"][0]["rows"].pop()
        with self.assertRaises(ValueError):
            report.discount.support_history_checks(changed, self.exp)
        changed = copy.deepcopy(self.raw)
        changed["diagnostics"]["support"][0]["action_labels"][0] = "wrong"
        with self.assertRaises(ValueError):
            report.seed11.model_checks(changed, self.prior_raw[0])
        changed = copy.deepcopy(self.raw)
        changed["result"]["coverage_evaluations"][0]["result"]["evaluation"]["candidate_policy_coverage"][1]["average_strategy_visits"] -= 1
        with self.assertRaises(ValueError):
            report.discount.ordinary_checks(changed, self.exp)

    def test_cost_uses_arm_identity_and_serial_order_uses_measurement(self):
        cases = [dict(case=name, discount=case["discount"], solveSeconds=seconds, configurationFingerprint=str(i),
                      measurement=dict(startedUtc=start, wallSeconds=10.0))
                 for i, (name, case, seconds, start) in enumerate(zip(report.CASES, self.exp["cases"], (10.0, 12.0),
                        ("2026-09-10T00:00:00Z", "2026-09-10T00:00:11Z")))]
        self.assertEqual(report.cost_gate(list(reversed(cases)), self.exp)["periodicToNoDiscountDriverRatio"], 1.2)
        report.execution_order_checks(report.pair_checks(cases))
        cases[1]["solveSeconds"] = 20.0
        self.assertTrue(report.cost_gate(cases, self.exp)["passed"])
        cases[1]["solveSeconds"] = 20.01
        self.assertFalse(report.cost_gate(cases, self.exp)["passed"])
        cases[1]["measurement"]["startedUtc"] = "2026-09-10T00:00:05Z"
        with self.assertRaisesRegex(ValueError, "serial execution"):
            report.execution_order_checks(report.pair_checks(cases))

    def test_pair_requires_distinct_configs_and_complete_unique_arms(self):
        cases = [dict(case=name, discount=case["discount"], configurationFingerprint="same")
                 for name, case in zip(report.CASES, self.exp["cases"])]
        with self.assertRaisesRegex(ValueError, "different configuration"):
            report.pair_checks(cases)
        with self.assertRaisesRegex(ValueError, "both complete"):
            report.pair_checks(cases[:1])

    def test_literal_job_budget_rejects_corruption_even_with_rehashed_metadata(self):
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
            for flag, field in (("--node", "nodes"), ("--support-node", "nodes"),
                                ("--coverage-prefix", "coveragePrefixes"), ("--endpoint-prefix", "endpointPaths")):
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
            report.discount.measurement_checks(run, self.exp, case, self.raw)
            arguments[arguments.index("--endpoint-seeds") + 1] = "703,702"
            job_path.write_bytes(report.serialized(job))
            measured["jobSha256"] = case["jobSha256"] = report.sha(job_path)
            measured_path.write_bytes(report.serialized(measured))
            with self.assertRaisesRegex(ValueError, "literal diagnostic/training budget"):
                report.discount.measurement_checks(run, self.exp, case, self.raw)

    def test_both_prior_hashes_and_recursive_seed_zero_link_are_required(self):
        with temporary_evidence() as run:
            run0, run11 = run / "seed0", run / "seed11"
            run0.mkdir()
            run11.mkdir()
            binary = run / "research.exe"
            binary.write_bytes(b"non-executable fixture")
            shared = dict(baseRevision="fixture", sourceManifestSha256="1" * 64, sourceZipSha256="2" * 64,
                          binarySha256=report.sha(binary), verificationSha256="3" * 64, runnerSha256="4" * 64,
                          sourceEvidenceRun=str(run), binary=str(binary))
            exp0 = {**self.previous[0]["experiment"], **shared}
            (run0 / "experiment.json").write_bytes(report.serialized(exp0))
            prior0 = dict(experiment=exp0, experimentSha256=report.sha(run0 / "experiment.json"))
            (run0 / "summary.json").write_bytes(report.serialized(prior0))
            refs = dict(priorPilotRun=str(run0), priorPilotExperimentSha256=report.sha(run0 / "experiment.json"),
                        priorPilotSummarySha256=report.sha(run0 / "summary.json"))
            exp11 = {**self.previous[11]["experiment"], **shared, **refs}
            prior11 = dict(experiment=exp11)
            (run11 / "experiment.json").write_bytes(report.serialized(exp11))
            (run11 / "summary.json").write_bytes(report.serialized(prior11))
            for folder, exp in ((run0, exp0), (run11, exp11)):
                for case in exp["cases"]:
                    (folder / case["name"]).mkdir()
                    (folder / case["name"] / "stdout.json").write_bytes(b"{}")
            exp = {**self.exp, **shared, **refs, **analysis_metadata(), "priorReplicationRun": str(run11),
                   "priorReplicationExperimentSha256": report.sha(run11 / "experiment.json"),
                   "priorReplicationSummarySha256": report.sha(run11 / "summary.json")}
            # Mock only the expensive frozen recursive call, not its module
            # globals or any production validators used by the new wrapper.
            with patch.object(report.seed11, "summarize", return_value=prior11) as regenerate:
                previous, _ = report.evidence_checks(exp)
                self.assertEqual(set(previous), {0, 11})
                self.assertEqual(regenerate.call_count, 1)
                (run11 / "summary.json").write_bytes(report.serialized({**prior11, "corrupt": True}))
                exp["priorReplicationSummarySha256"] = report.sha(run11 / "summary.json")
                with self.assertRaisesRegex(ValueError, "byte regeneration"):
                    report.evidence_checks(exp)
                (run11 / "summary.json").write_bytes(report.serialized(prior11))
                exp["priorReplicationSummarySha256"] = report.sha(run11 / "summary.json")
                exp["priorPilotSummarySha256"] = "0" * 64
                with self.assertRaisesRegex(ValueError, "priorPilot summary identity"):
                    report.evidence_checks(exp)

    def test_case_mode_and_three_seed_endpoint_evidence_are_reproducible(self):
        sample = dict(case=report.CASES[0], discount={"kind": "none"}, solveSeconds=10.0,
                      endpoints=report.discount.endpoint_checks(self.raw, self.exp, self.prior_raw[0]["none"]),
                      rootEvaluations=self.raw["diagnostics"]["root_evaluations"],
                      support=report.discount.support_history_checks(self.raw, self.exp),
                      coverageSummary=report.discount.ordinary_checks(self.raw, self.exp))
        for previous in self.previous.values():
            previous["cases"] = [sample, {**sample, "case": "other", "discount": {"kind": "periodic"}}]
        with temporary_evidence() as run:
            self.exp.update(sourceEvidenceRun="fixture", sourceManifestSha256="1" * 64, binarySha256="2" * 64,
                            verificationSha256="3" * 64, priorPilotRun="seed0", priorPilotExperimentSha256="4" * 64,
                            priorPilotSummarySha256="5" * 64, priorReplicationRun="seed11",
                            priorReplicationExperimentSha256="6" * 64, priorReplicationSummarySha256="7" * 64)
            snapshot = run / "experiment-preexecution.json"
            snapshot.write_bytes(report.serialized(self.exp))
            self.exp["preexecutionSha256"] = report.sha(snapshot)
            (run / "experiment.json").write_bytes(report.serialized(self.exp))
            with patch.object(report, "evidence_checks", return_value=(self.previous, self.prior_raw)), \
                 patch.object(report, "summarize_case", return_value=sample) as validate:
                a = report.summarize(run, report.CASES[0])
                self.assertEqual(validate.call_count, 1)
                b = report.summarize(run, report.CASES[0])
                self.assertEqual(report.serialized(a), report.serialized(b))
                self.assertIsNone(a["costGate"])
                self.assertIsNone(a["periodicVersusNoDiscount"])
                self.assertFalse(a["promotionAllowed"])
                self.assertEqual([v["trainingSeed"] for v in a["trainingSeedEvidence"]], [0, 11, 29])
                self.assertEqual([len(v["cases"]) for v in a["trainingSeedEvidence"]], [2, 2, 1])
                self.assertTrue(all(len(c["endpoints"]) == 5 for v in a["trainingSeedEvidence"] for c in v["cases"]))

    def test_complete_summary_keeps_all_three_seeds_without_averaging(self):
        sample = dict(case=report.CASES[0], discount={"kind": "none"}, solveSeconds=10.0, configurationFingerprint="a",
                      measurement=dict(startedUtc="2026-09-10T00:00:00Z", wallSeconds=10.0),
                      endpoints=report.discount.endpoint_checks(self.raw, self.exp, self.prior_raw[0]["none"]),
                      rootEvaluations=[], support=[], coverageSummary=[])
        other = {**sample, "case": report.CASES[1], "discount": report.discount.DISCOUNTS[1], "solveSeconds": 12.0,
                 "configurationFingerprint": "b", "measurement": dict(startedUtc="2026-09-10T00:00:11Z", wallSeconds=13.0)}
        for previous in self.previous.values():
            previous["cases"] = [sample, other]
        with temporary_evidence() as run:
            self.exp.update(sourceEvidenceRun="fixture", sourceManifestSha256="1" * 64, binarySha256="2" * 64,
                            verificationSha256="3" * 64, priorPilotRun="seed0", priorPilotExperimentSha256="4" * 64,
                            priorPilotSummarySha256="5" * 64, priorReplicationRun="seed11",
                            priorReplicationExperimentSha256="6" * 64, priorReplicationSummarySha256="7" * 64)
            snapshot = run / "experiment-preexecution.json"
            snapshot.write_bytes(report.serialized(self.exp))
            self.exp["preexecutionSha256"] = report.sha(snapshot)
            (run / "experiment.json").write_bytes(report.serialized(self.exp))
            with patch.object(report, "evidence_checks", return_value=(self.previous, self.prior_raw)), \
                 patch.object(report, "summarize_case", side_effect=[sample, other]):
                value = report.summarize(run)
            self.assertEqual(value["status"], "completed-replication")
            self.assertEqual(value["costGate"]["periodicToNoDiscountDriverRatio"], 1.2)
            self.assertEqual(len(value["periodicVersusNoDiscount"]), 5)
            self.assertEqual([len(v["cases"]) for v in value["trainingSeedEvidence"]], [2, 2, 2])
            self.assertFalse(value["promotionAllowed"])


if __name__ == "__main__":
    unittest.main()
