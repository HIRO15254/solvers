"""Small self-contained regressions; no retained solver or binary executes."""
import copy
from contextlib import contextmanager
import json
from pathlib import Path
import shutil
import sys
import unittest
import uuid
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_preflop_counterfactual as report


@contextmanager
def temporary_evidence():
    cache = (Path(__file__).resolve().parents[2] / ".cache/tool-tests").resolve()
    folder = cache / ("preflop-counterfactual-" + uuid.uuid4().hex)
    folder.mkdir(parents=True)
    try:
        yield folder
    finally:
        if folder.resolve().parent != cache:
            raise ValueError("test cleanup escaped cache")
        shutil.rmtree(folder)


def write(path, value):
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    Path(path).write_bytes(report.serialized(value))


def estimate(mean=0.0, stderr=0.0):
    return dict(mean=mean, stderr=stderr)


def sampling(n, seed, replays, actor):
    def street(visited):
        return dict(positive_weight_decision_visits=n if visited else 0,
                    positive_weight_trajectory_visits=n if visited else 0,
                    trajectory_probability=estimate(float(visited)),
                    decisions_per_prefix_trajectory=estimate(float(visited)),
                    decision_weight_effective_sample_size=float(n) if visited else 0.0,
                    **{key + "_fraction": estimate(float(key == "average")) if visited else None
                       for key in ("average", "current", "regret_fallback", "uniform_fallback")})
    return dict(seed=seed, samples=n, total_deal_attempts=n, terminal_replays=replays,
                positive_weight_samples=n, relative_weight_mean=estimate(1.0),
                effective_sample_size=float(n), max_normalized_weight=1 / n,
                prefix_current_fraction=estimate(), prefix_regret_fallback_fraction=estimate(),
                prefix_uniform_fallback_fraction=estimate(), baseline_seats=[estimate()] * 6,
                coverage_by_street=[street(s == 0) for s in range(4)],
                coverage_by_seat=[[street(seat == actor and s == 0) for s in range(4)] for seat in range(6)])


def ordinary():
    rows = []
    for seat in range(6):
        row = {}
        for source in report.baseline.pilot.SOURCES:
            value = 128 if seat == 3 and source in {"decision", "stored_strategy", "average_strategy"} else 0
            row[source + "_visits"] = value
            row[source + "_visits_by_street"] = dict(preflop=value, flop=0, turn=0, river=0)
        rows.append(row)
    return dict(samples=128, total_deal_attempts=128,
                seats=[dict(mean=0.0, stderr=0.0, ci95=[0.0, 0.0]) for _ in range(6)],
                deviation_gain_lower_bound=[dict(mean=0.0, stderr=0.0, ci95=[0.0, 0.0]) for _ in range(6)],
                candidate_policy_coverage=rows)


def support_fixture():
    paths = [[], [0] * 4, [0] * 4 + [2], [0] * 4 + [2] * 2,
             [0] * 4 + [2] * 3, [0] * 4 + [2] * 4]
    labels = [["fold", "raise-to:2000"], ["fold", "call:500", "raise-to:3000"],
              ["fold", "call:2000", "raise-to:10000"], ["fold", "call:7000", "raise-to:21000"],
              ["fold", "call:11000", "raise-to:100000:all-in"], ["fold", "call:79000"]]
    nodes = []
    for i, (path, indices, menu, actor) in enumerate(zip(report.SUPPORTS, paths, labels, [3, 1, 2, 1, 2, 1], strict=True)):
        context = dict(requested=path, history=bytes([i] * 16).hex(), actionIndices=indices,
                       actionLabels=[] if path == "root" else path.split("/"),
                       actor=actor, street="preflop", activeOpponents=5 if i == 0 else 1)
        rows = []
        for bucket in range(169):
            strategy = [0.5, 0.5] if len(menu) == 2 else [0.25, 0.25, 0.5]
            if bucket == 0 and i in (1, 2):
                strategy = [1.0, 0.0, 0.0]
            rows.append(dict(bucket=bucket, status="stored-positive-regrets", regrets=strategy.copy(),
                             strategySum=strategy.copy(), strategyMass=1.0,
                             currentStrategy=strategy.copy(), averageStrategy=strategy.copy()))
        nodes.append(dict(context=context, bucketActiveOpponents=context["activeOpponents"], expectedBuckets=169,
                          storedBuckets=169, nonzeroRegretBuckets=169, positiveRegretBuckets=169,
                          averageBuckets=169, averageAndNonzeroRegretBuckets=169, actionLabels=menu, rows=rows))
    return nodes


def endpoint_fixture(support, cf=False):
    actions = len(support["action_labels"])
    active = range(32) if cf else range(1, 33)
    rows = []
    for bucket in range(169):
        n = 2048 if bucket in active else 0
        gains = [estimate(2.0 if a == 0 else -1.0) for a in range(actions)] if n and bucket % 2 else [estimate(-1.0) for _ in range(actions)]
        rows.append(dict(key=report.baseline.key_for(support, bucket), baseline_source="average",
                         positive_weight_samples=n, relative_weight_sum=float(n), effective_sample_size=float(n),
                         max_normalized_weight=1 / n if n else 0.0,
                         action_gains=gains if n else [None] * actions,
                         selected_action=0 if n and bucket % 2 else None))
    result = {k: copy.deepcopy(v) for k, v in support.items() if k != "rows"}
    result.update(configuration_fingerprint=[1] * 32, abstraction_fingerprint=[2] * 32,
                  config=dict(fit_samples=65536, fit_seed=602, held_out_samples=131072,
                              held_out_seeds=[702, 703], min_fit_ess=64),
                  variant=dict(purify_threshold=0.0, use_current_strategy=False), fit_elapsed_secs=1.0,
                  fit=dict(sampling=sampling(65536, 602, 65536 * (actions + 1), support["actor"]),
                           retained_buckets=16, rows=rows),
                  held_out=[dict(elapsed_secs=1.0, sampling=sampling(131072, seed, 196608, support["actor"]),
                                 gain=estimate(-1.0, 0.2), retained_key_weight_fraction=estimate(0.5, 0.01)) for seed in [702, 703]],
                  proposal=dict(preflop_actions=support["action_indices"].copy(), preflop_history=support["history"].copy(),
                                root_range_fingerprint=[3] * 32, proposal_range_fingerprint=[4] * 32,
                                positive_target_combos_by_seat=[1326] * 6, floor_adjusted_combos_by_seat=[0] * 6,
                                target_scale_by_seat=[1.0] * 6, proposal_floor_fraction=1e-7,
                                pilot_samples=4096, pilot_accepted=4000))
    return result


def raw_fixture():
    nodes = support_fixture()
    normalized = {n["context"]["requested"]: report.normalized_support(n) for n in nodes}
    old = [endpoint_fixture(normalized[path]) for path in report.ENDPOINTS]
    reference = dict(configurationFingerprint="01" * 32, abstractionFingerprint="02" * 32, solverStateVersion=4,
                     supportNodes=[dict(requested=path) for path in report.SUPPORTS if path != report.OPEN],
                     diagnostics=dict(support=[normalized[path] for path in report.SUPPORTS if path != report.OPEN], endpoints=old))
    root = nodes[0]["context"]
    common = dict(schemaVersion="solvers.multiway-checkpoint-audit/v1", sweeps=32768, solverStateVersion=4,
                  configurationFingerprint="01" * 32, abstractionFingerprint="02" * 32, constructionElapsedSecs=1.0,
                  evaluationSamplesPerSeed=128, evaluationSeeds=[101, 202],
                  deviatorTraining=dict(seed=0x62722d6175646974, traversalsPerSeat=1, elapsedSecs=0.1,
                                        coverage=[dict(traversals=1, visited_infosets=n, retained_infosets=0,
                                                       total_visits=n, retained_visits=0) for n in (0, 1, 18, 1, 1, 4)]),
                  evaluations=[dict(seed=seed, elapsedSecs=0.1, result=ordinary()) for seed in [101, 202]],
                  nodes=[dict(requested="root", history=root["history"], actor=root["actor"], activeOpponents=5,
                              actions=nodes[0]["actionLabels"], hands=[], frequency=None)],
                  policySupport=nodes, policyArena=dict(nodes=10, columns=169, slots=507, bytes=4096, pages_committed=True))
    actual = copy.deepcopy(common)
    actual["endpointDeviations"] = [dict(context=report.expected_context(p, normalized[p]), elapsedSecs=4.0,
                                         result=copy.deepcopy(e), interpretation="fixture") for p, e in zip(report.ENDPOINTS, old, strict=True)]
    cf = copy.deepcopy(common)
    cf["endpointCounterfactualDeviations"] = []
    for index, (path, previous) in enumerate(zip(report.ENDPOINTS, old, strict=True)):
        result = copy.deepcopy(previous) if index < 2 else endpoint_fixture(normalized[path], cf=True)
        cf["endpointCounterfactualDeviations"].append(dict(context=report.expected_context(path, normalized[path]),
            elapsedSecs=4.0, interpretation="fixture", result=dict(target="opponents-prefix",
                proposal_kind="preflop-opponents-proposal", excluded_actor=result["actor"],
                validated_class_contexts=len(result["action_indices"]) + 1,
                own_prefix_probability_by_bucket=report.own_probabilities(result, normalized), evaluation=result)))
    return reference, actual, cf


def startup_fixture(run, exp):
    prior = run / "prior-startup"
    prior.mkdir()
    old = copy.deepcopy(exp)
    archived = {"tools/summarize_preflop_counterfactual.py": b"archived br0 validator fixture",
                "tools/tests/test_summarize_preflop_counterfactual.py": b"archived br0 test fixture"}
    old["summarizerSha256"] = report.hashlib.sha256(next(iter(archived.values()))).hexdigest()
    old["testScriptSha256"] = report.hashlib.sha256(list(archived.values())[1]).hexdigest()
    for case in old["cases"]:
        job = report.read(case["job"])
        job["arguments"][job["arguments"].index("--br-traversals") + 1] = "0"
        case["job"] = str(prior / (case["name"] + "-job.json"))
        write(case["job"], job)
        case["jobSha256"] = report.sha(case["job"])
    write(prior / "experiment-preexecution.json", old)
    old["preexecutionSha256"] = report.sha(prior / "experiment-preexecution.json")
    old["status"] = "rejected-before-solver-construction"
    write(prior / "experiment.json", old)
    failed = prior / "actual-prefix"
    failed.mkdir()
    (failed / "stdout.json").write_bytes(b"")
    (failed / "stderr.txt").write_text("Error: --br-traversals must be positive\n", encoding="utf-8")
    case = old["cases"][0]
    measured = dict(schemaVersion="solvers.checkpoint-audit-measurement/v1", exitCode=1, timedOut=False,
                    wallSeconds=0.1, stdoutSha256=report.sha(failed / "stdout.json"), job=case["job"],
                    jobSha256=case["jobSha256"], binary=old["binary"], sourceRevision=old["baseRevision"])
    job = report.read(case["job"])
    for key in ("arguments", "binarySha256", "configSha256", "sourceManifestSha256", "timeoutSeconds", "validationReport"):
        measured[key] = job[key]
    write(failed / "measurement.json", measured)
    archive = prior / "preexecution-validator.zip"
    with zipfile.ZipFile(archive, "w") as zipped:
        for name, contents in archived.items():
            zipped.writestr(name, contents)
    failure = dict(status="startup-rejected", exitCode=1, stdoutBytes=0, requiredChecksPassed=7,
                   sourceAndBinaryUnchanged=True, broaderGoalComplete=False, cloudResourcesStarted=False,
                   replacementRun=str(run), wallSeconds=0.1, finalExperimentSha256=report.sha(prior / "experiment.json"),
                   originalPreexecutionSha256=old["preexecutionSha256"], measurement=str(failed / "measurement.json"),
                   stderr=str(failed / "stderr.txt"), preexecutionValidatorArchive=str(archive))
    for field in ("measurement", "stderr", "preexecutionValidatorArchive"):
        failure[field + "Sha256"] = report.sha(failure[field])
    path = prior / "startup-failure.json"
    write(path, failure)
    exp["priorStartupFailure"] = str(path)
    exp["priorStartupFailureSha256"] = report.sha(path)


def evidence_fixture(run):
    exp = dict(baseRevision="fixture", threads=8, memory="8GiB", timeoutSeconds=900,
               endpointPaths=report.ENDPOINTS.copy(), supportNodes=report.SUPPORTS.copy(),
               endpointSchedule=copy.deepcopy(report.SCHEDULE), promotionAllowed=False, broaderGoalComplete=False,
               cloudResourcesStarted=False, status="planned", cacheDir=str(run / "cache"),
               validationReport="docs/validation/multiway-preflop-counterfactual-2026-09-10.md",
               runner=str(Path(report.__file__).with_name("run_average_sampling_measurement.ps1")),
               summarizerSha256=report.sha(report.__file__), testScriptSha256=report.sha(__file__),
               dependencies={m.__file__: report.sha(m.__file__) for m in report.DEPENDENCIES})
    for field, name, content in [("binary", "audit.exe", b"non-executable fixture"),
                                 ("config", "config.toml", b'schema = "solvers.multiway-preflop/v1"\n[game]\nseat_count = 6\n[solver]\nseed = 0\n'),
                                 ("inputCheckpoint", "input.mwckpt", b"checkpoint fixture")]:
        (run / name).write_bytes(content)
        exp[field] = str(run / name)
    files = []
    exp["sourceZip"] = str(run / "source.zip")
    with zipfile.ZipFile(exp["sourceZip"], "w") as archive:
        for path in sorted(report.SOURCE_PATHS):
            content = path.encode()
            archive.writestr(path, content)
            files.append(dict(path=path, sha256=report.hashlib.sha256(content).hexdigest()))
    exp["sourceManifest"] = str(run / "source-manifest.json")
    write(exp["sourceManifest"], dict(baseRevision="fixture", files=files))
    for field in ("binary", "config", "inputCheckpoint", "sourceManifest", "sourceZip", "runner"):
        exp[field + "Sha256"] = report.sha(exp[field])
    checks = []
    for i, command in enumerate(sorted(report.COMMANDS)):
        log = run / "verification" / (str(i) + ".log")
        log.parent.mkdir(exist_ok=True)
        log.write_text("test result: ok. 1 passed; 0 failed; 0 ignored;\n" if command.startswith("cargo test ") else "", encoding="utf-8")
        entry = dict(command=command, log=str(log), sha256=report.sha(log), exitCode=0, elapsedSecs=0.1)
        if command.startswith("cargo test "):
            entry.update(passed=1, failed=0, ignored=0, suites=1)
        checks.append(entry)
    exp["verification"] = str(run / "verification/verification.json")
    write(exp["verification"], dict(status="passed", sourceFiles=len(files), sourceManifestSha256=exp["sourceManifestSha256"], binarySha256=exp["binarySha256"], checks=checks))
    exp["verificationSha256"] = report.sha(exp["verification"])
    exp["cases"] = []
    for target in report.TARGETS:
        case = dict(name=target, target=target, job=str(run / (target + "-job.json")))
        job = dict(arguments=report.expected_arguments(exp, case), timeoutSeconds=900,
                   validationReport=exp["validationReport"], binarySha256=exp["binarySha256"])
        for field in ("config", "inputCheckpoint", "sourceManifest"):
            job[field], job[field + "Sha256"] = exp[field], exp[field + "Sha256"]
        write(case["job"], job)
        case["jobSha256"] = report.sha(case["job"])
        exp["cases"].append(case)
    startup_fixture(run, exp)
    write(run / "experiment-preexecution.json", exp)
    exp["preexecutionSha256"] = report.sha(run / "experiment-preexecution.json")
    return exp


def completed_fixture(run, exp):
    reference, actual, cf = raw_fixture()
    for index, (raw, case) in enumerate(zip([actual, cf], exp["cases"], strict=True)):
        raw.update(config=exp["config"], checkpoint=exp["inputCheckpoint"])
        path = run / case["name"]
        write(path / "stdout.json", raw)
        job = report.read(case["job"])
        measured = dict(schemaVersion="solvers.checkpoint-audit-measurement/v1", binary=exp["binary"], job=case["job"],
                        binarySha256=exp["binarySha256"], jobSha256=case["jobSha256"], configSha256=exp["configSha256"],
                        sourceManifestSha256=exp["sourceManifestSha256"], sourceRevision=exp["baseRevision"],
                        arguments=job["arguments"], timeoutSeconds=900, validationReport=exp["validationReport"],
                        stdoutSha256=report.sha(path / "stdout.json"), exitCode=0, timedOut=False,
                        wallSeconds=30.0, observedPeakWorkingSetBytes=1000000,
                        startedUtc=f"2026-09-10T00:0{index}:00+00:00", peakMethod="fixture whole-process peak")
        write(path / "measurement.json", measured)
    return reference


def amendment_fixture(run, exp):
    # Original scripts are kept in the historical snapshot/archive. The
    # amendment alone identifies the current helper, without rewriting them.
    old = {"tools/summarize_preflop_counterfactual.py": b"original label-confusing validator fixture",
           "tools/tests/test_summarize_preflop_counterfactual.py": b"original label-confusing tests fixture"}
    exp["summarizerSha256"] = report.hashlib.sha256(list(old.values())[0]).hexdigest()
    exp["testScriptSha256"] = report.hashlib.sha256(list(old.values())[1]).hexdigest()
    snapshot = run / "experiment-preexecution.json"
    write(snapshot, {k: v for k, v in exp.items() if k not in report.RUNTIME_FIELDS})
    exp["preexecutionSha256"] = report.sha(snapshot)
    archive = run / "validator-v1.zip"
    with zipfile.ZipFile(archive, "w") as zipped:
        for path, contents in old.items():
            zipped.writestr(path, contents)
    amendment = dict(schemaVersion="solvers.counterfactual-validator-amendment/v1",
                     reason="prefix-context-labels-versus-endpoint-menu", createdUtc="2026-09-10T00:00:45+00:00",
                     originalExperimentSnapshot=str(snapshot), originalExperimentSnapshotSha256=report.sha(snapshot),
                     originalSummarizerSha256=exp["summarizerSha256"], originalTestScriptSha256=exp["testScriptSha256"],
                     validatorArchive=str(archive), validatorArchiveSha256=report.sha(archive),
                     summarizerSha256=report.sha(report.__file__), testScriptSha256=report.sha(__file__),
                     observedStdout=str(run / "actual-prefix/stdout.json"),
                     observedMeasurement=str(run / "actual-prefix/measurement.json"))
    for field in ("observedStdout", "observedMeasurement"):
        amendment[field + "Sha256"] = report.sha(amendment[field])
    path = run / "validator-amendment.json"
    write(path, amendment)
    exp["validatorAmendment"] = str(path)
    exp["validatorAmendmentSha256"] = report.sha(path)
    return amendment


class CounterfactualValidationTests(unittest.TestCase):
    def setUp(self):
        self.reference, self.actual, self.cf = raw_fixture()
        self.exp = dict(endpointPaths=report.ENDPOINTS, supportNodes=report.SUPPORTS,
                        endpointSchedule=copy.deepcopy(report.SCHEDULE), config="config.toml", inputCheckpoint="input.mwckpt")
        for raw in (self.actual, self.cf):
            raw.update(config=self.exp["config"], checkpoint=self.exp["inputCheckpoint"])

    def check(self, cf=True):
        target = report.TARGETS[int(cf)]
        return report.raw_checks(self.cf if cf else self.actual, self.exp, dict(name=target, target=target), self.reference)

    def test_accepts_signed_gains_full_denominators_and_own_zero_cf_evidence(self):
        self.check(False)
        rows = self.check()
        self.assertEqual(rows[2]["zeroOwnReachBuckets"], [0])
        self.assertEqual(rows[2]["zeroOwnReachFitPositiveBuckets"], [0])
        self.assertEqual(rows[2]["zeroOwnReachFitWeightFraction"], 1 / 32)
        self.assertEqual(rows[2]["result"]["evaluation"]["held_out"][0]["gain"]["mean"], -1.0)

    def test_rejects_swapped_target_and_singular_or_duplicate_output(self):
        self.cf["endpointCounterfactualDeviations"][2]["result"]["target"] = "actual-prefix"
        with self.assertRaisesRegex(ValueError, "explicit counterfactual"):
            self.check()
        self.cf = copy.deepcopy(self.actual)
        with self.assertRaisesRegex(ValueError, "exclusive repeated"):
            self.check()
        self.actual["endpointDeviation"] = self.actual["endpointDeviations"][0]
        with self.assertRaisesRegex(ValueError, "exclusive repeated"):
            self.check(False)

    def test_rejects_missing_fit_bucket_and_wrong_own_information_key(self):
        endpoint = self.cf["endpointCounterfactualDeviations"][2]["result"]["evaluation"]
        row = endpoint["fit"]["rows"].pop()
        with self.assertRaisesRegex(ValueError, "complete fit bucket"):
            self.check()
        endpoint["fit"]["rows"].append(row)
        endpoint["fit"]["rows"][0]["key"]["player"] = 5
        with self.assertRaisesRegex(ValueError, "fit own-information key"):
            self.check()

    def test_rejects_out_of_range_and_false_own_zero_metadata(self):
        own = self.cf["endpointCounterfactualDeviations"][2]["result"]["own_prefix_probability_by_bucket"]
        original = own[1]
        for value in (-1.0, 1.01, float("nan"), True, 0.0):
            own[1] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.check()
        own[1] = original
        own.pop()
        with self.assertRaisesRegex(ValueError, "169 bounded"):
            self.check()

    def test_rejects_wrong_class_context_actor_and_opponent_proposal(self):
        wrapper = self.cf["endpointCounterfactualDeviations"][2]["result"]
        for key, value, label in [("validated_class_contexts", 1, "validated public"), ("excluded_actor", 5, "excluded endpoint")]:
            before = wrapper[key]
            wrapper[key] = value
            with self.assertRaisesRegex(ValueError, label):
                self.check()
            wrapper[key] = before
        wrapper["evaluation"]["proposal"]["target_scale_by_seat"][3] = 0.5
        with self.assertRaisesRegex(ValueError, "only excluded actor"):
            self.check()

    def test_actual_legacy_replay_changes_only_documented_clocks(self):
        result = self.actual["endpointDeviations"][2]["result"]
        result["fit_elapsed_secs"] = 0.5
        self.check(False)
        result["held_out"][0]["gain"]["mean"] -= 0.01
        with self.assertRaisesRegex(ValueError, "actual endpoint exact legacy"):
            self.check(False)

    def test_rejects_gate_denominator_replay_and_coverage_corruption(self):
        original = copy.deepcopy(self.cf)
        mutations = [lambda r: r["fit"]["rows"][1].update(selected_action=1),
                     lambda r: r["fit"]["sampling"]["relative_weight_mean"].update(mean=0.5),
                     lambda r: r["fit"]["sampling"].update(terminal_replays=65536),
                     lambda r: r["held_out"][0]["sampling"]["coverage_by_street"][0]["average_fraction"].update(mean=0.5)]
        for mutate in mutations:
            self.cf = copy.deepcopy(original)
            mutate(self.cf["endpointCounterfactualDeviations"][2]["result"]["evaluation"])
            with self.assertRaises(ValueError):
                self.check()

    def test_rejects_missing_or_changed_raw_support_and_own_prior_context(self):
        self.cf["policySupport"][0]["rows"].pop()
        with self.assertRaisesRegex(ValueError, "complete bucket denominator"):
            self.check()
        self.cf = raw_fixture()[2]
        self.cf.update(config=self.exp["config"], checkpoint=self.exp["inputCheckpoint"])
        self.cf["policySupport"][2]["context"]["actionIndices"][-1] = 1
        with self.assertRaisesRegex(ValueError, "BB prior public path"):
            self.check()

    def test_missing_vs_stored_zero_fallback_and_f32_last_residual(self):
        support = report.normalized_support(self.actual["policySupport"][1])
        endpoint = copy.deepcopy(support)
        endpoint["action_indices"] = support["action_indices"] + [2]
        for row in support["rows"]:
            row.update(regrets=None, average_strategy=None)
        missing = report.own_probabilities(endpoint, {report.FOLD4: support})
        f32 = report.baseline.pilot.support_dependency.f32
        self.assertEqual(missing[0], 1.0 - 2 * f32(1 / 3))
        for row in support["rows"]:
            row.update(regrets=[0.0, 0.0, 0.0])
        self.assertEqual(report.own_probabilities(endpoint, {report.FOLD4: support}), missing)
        for row in support["rows"]:
            row["regrets"] = [1.0, 2.0, 5.0]
        self.assertEqual(report.own_probabilities(endpoint, {report.FOLD4: support})[0], 0.625)

    def test_small_evidence_complete_pair_and_byte_reproduction(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            reference = completed_fixture(run, exp)
            report.schedule_checks(run, exp)
            evidence = report.evidence_checks(exp)
            self.assertEqual(evidence["checks"], 7)
            cases = [report.summarize_case(run, exp, c, reference) for c in exp["cases"]]
            compared = report.comparison(cases)
            self.assertFalse(compared["aggregateGainRankingAllowed"])
            self.assertEqual(compared["endpoints"][2]["newlyEligibleBuckets"], [0])
            self.assertEqual(report.serialized(compared), report.serialized(report.comparison(cases)))
            self.assertNotIn("constructionElapsedSecs", cases[0]["commonResult"])
            self.assertIn("policySupport", cases[0]["commonResult"])

    def test_rejects_stale_source_config_binary_and_archive_identity(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            for field in ("config", "inputCheckpoint", "binary", "sourceManifest"):
                p = Path(exp[field]); original = p.read_bytes(); p.write_bytes(original + b"stale")
                with self.subTest(field=field), self.assertRaisesRegex(ValueError, "evidence identity"):
                    report.evidence_checks(exp)
                p.write_bytes(original)
            with zipfile.ZipFile(exp["sourceZip"], "a") as archive:
                archive.writestr("extra.rs", "unexpected")
            exp["sourceZipSha256"] = report.sha(exp["sourceZip"])
            with self.assertRaisesRegex(ValueError, "exact source ZIP"):
                report.evidence_checks(exp)

    def test_rejects_changed_preexecution_job_arguments_and_failed_logs(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            exp["threads"] = 9
            with self.assertRaisesRegex(ValueError, "fixed resources"):
                report.schedule_checks(run, exp)
            exp["threads"] = 8
            exp["cacheDir"] += "changed"
            with self.assertRaisesRegex(ValueError, "immutable preexecution"):
                report.schedule_checks(run, exp)
            exp["cacheDir"] = exp["cacheDir"][:-7]
            case = exp["cases"][0]; job = report.read(case["job"]); job["arguments"][-1] = "opponents-prefix"
            write(case["job"], job); case["jobSha256"] = report.sha(case["job"])
            with self.assertRaisesRegex(ValueError, "literal job arguments"):
                report.job_checks(exp, case)
            verification = report.read(exp["verification"])
            log = Path(verification["checks"][0]["log"]); log.write_text("error: forced failure\n", encoding="utf-8")
            verification["checks"][0]["sha256"] = report.sha(log)
            write(exp["verification"], verification); exp["verificationSha256"] = report.sha(exp["verification"])
            with self.assertRaisesRegex(ValueError, "failed verification log"):
                report.evidence_checks(exp)

    def test_rejects_temporal_overlap_incomplete_pair_and_common_policy_change(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run); reference = completed_fixture(run, exp)
            cases = [report.summarize_case(run, exp, c, reference) for c in exp["cases"]]
            with self.assertRaisesRegex(ValueError, "both completed"):
                report.comparison(cases[:1])
            before = cases[1]["processStartedUnixMs"]
            cases[1]["processStartedUnixMs"] = cases[0]["processFinishedUnixMs"] - 2
            with self.assertRaisesRegex(ValueError, "overlap"):
                report.comparison(cases)
            cases[1]["processStartedUnixMs"] = before
            cases[1]["commonResult"]["policySupport"][2]["rows"][0]["strategyMass"] = 2
            with self.assertRaisesRegex(ValueError, "all non-endpoint"):
                report.comparison(cases)

    def test_rejects_measurement_source_mismatch_timeout_and_nonfinite_clock(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run); reference = completed_fixture(run, exp)
            case = exp["cases"][0]; path = run / case["name"] / "measurement.json"; original = report.read(path)
            for key, value in [("sourceManifestSha256", "00" * 32), ("timedOut", True), ("wallSeconds", -1.0)]:
                measured = copy.deepcopy(original); measured[key] = value; write(path, measured)
                with self.subTest(key=key), self.assertRaises(ValueError):
                    report.summarize_case(run, exp, case, reference)

    def test_rejects_missing_required_build_and_changed_checkpoint_binding(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            verification = report.read(exp["verification"])
            verification["checks"] = [c for c in verification["checks"] if not c["command"].startswith("cargo build ")]
            write(exp["verification"], verification)
            exp["verificationSha256"] = report.sha(exp["verification"])
            with self.assertRaisesRegex(ValueError, "required verification commands"):
                report.evidence_checks(exp)
        self.actual["checkpoint"] = "different.mwckpt"
        with self.assertRaisesRegex(ValueError, "raw checkpoint location"):
            self.check(False)

    def test_rejects_br0_literal_and_candidate_coverage_budget(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            case = exp["cases"][0]
            job = report.read(case["job"])
            job["arguments"][job["arguments"].index("--br-traversals") + 1] = "0"
            write(case["job"], job)
            case["jobSha256"] = report.sha(case["job"])
            with self.assertRaisesRegex(ValueError, "literal job arguments"):
                report.job_checks(exp, case)
        self.actual["deviatorTraining"]["traversalsPerSeat"] = 0
        with self.assertRaisesRegex(ValueError, "one incidental candidate"):
            self.check(False)
        self.actual["deviatorTraining"]["traversalsPerSeat"] = 1
        self.actual["deviatorTraining"]["coverage"][0]["traversals"] = 0
        with self.assertRaisesRegex(ValueError, "coverage count types/budget"):
            self.check(False)

    def test_candidate_coverage_allows_counts_but_enforces_retention_partition(self):
        dev = copy.deepcopy(self.actual["deviatorTraining"])
        dev["coverage"][0].update(visited_infosets=3, total_visits=12, retained_infosets=1, retained_visits=8)
        report.deviator_checks(dev)
        valid = copy.deepcopy(dev)
        for changed in [dict(visited_infosets=13), dict(retained_visits=7), dict(retained_infosets=0),
                        dict(total_visits=23), dict(total_visits=-1), dict(total_visits=2**64)]:
            dev = copy.deepcopy(valid)
            dev["coverage"][0].update(changed)
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                report.deviator_checks(dev)

    def test_prior_startup_rejection_and_archived_validator_are_hash_bound(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            checked = report.startup_failure_checks(exp)
            self.assertTrue(checked["emptyStdoutVerified"])
            failure = report.read(exp["priorStartupFailure"])
            Path(failure["stderr"]).write_text("different failure\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "startup referenced identity"):
                report.startup_failure_checks(exp)
            failure["stderrSha256"] = report.sha(failure["stderr"])
            write(exp["priorStartupFailure"], failure)
            exp["priorStartupFailureSha256"] = report.sha(exp["priorStartupFailure"])
            with self.assertRaisesRegex(ValueError, "positive-budget CLI error"):
                report.startup_failure_checks(exp)
            Path(failure["stderr"]).write_text("Error: --br-traversals must be positive\n", encoding="utf-8")
            failure["stderrSha256"] = report.sha(failure["stderr"])
            with zipfile.ZipFile(failure["preexecutionValidatorArchive"], "w") as archive:
                archive.writestr("tools/summarize_preflop_counterfactual.py", "changed")
                archive.writestr("tools/tests/test_summarize_preflop_counterfactual.py", "changed")
            failure["preexecutionValidatorArchiveSha256"] = report.sha(failure["preexecutionValidatorArchive"])
            write(exp["priorStartupFailure"], failure)
            exp["priorStartupFailureSha256"] = report.sha(exp["priorStartupFailure"])
            with self.assertRaisesRegex(ValueError, "archived failed validator identity"):
                report.startup_failure_checks(exp)

    def test_cli_prefix_context_labels_are_distinct_from_endpoint_menu(self):
        # Literal contexts mirror resolve_condition_prefixes: labels describe
        # traversed edges, while PolicySupportNode.actionLabels is the menu.
        root = support_fixture()[0]
        self.assertEqual(root["actionLabels"], ["fold", "raise-to:2000"])
        self.assertEqual(report.expected_context("root", report.normalized_support(root)),
                         dict(requested="root", history="00" * 16, actionIndices=[], actionLabels=[],
                              actor=3, street="preflop", activeOpponents=5))
        node = support_fixture()[3]
        literal = dict(requested=report.THREE, history="5ebdea819e7b37aee4f9590419b75ca9",
                       actionIndices=[0, 0, 0, 0, 1, 2],
                       actionLabels=["fold", "fold", "fold", "fold", "raise-to:3000", "raise-to:10000"],
                       actor=1, street="preflop", activeOpponents=1)
        node["context"] = copy.deepcopy(literal)
        self.assertEqual(report.expected_context(report.THREE, report.normalized_support(node)), literal)
        self.assertEqual(node["actionLabels"], ["fold", "call:7000", "raise-to:21000"])
        node["context"]["actionLabels"] = node["actionLabels"].copy()
        with self.assertRaisesRegex(ValueError, "public prefix path labels/indices"):
            report.normalized_support(node)

    def test_validator_amendment_preserves_original_snapshot_and_records_effective_hashes(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            completed_fixture(run, exp)
            amendment = amendment_fixture(run, exp)
            report.schedule_checks(run, exp)
            checked = report.analysis_identity_checks(exp)
            self.assertTrue(checked["amendmentApplied"])
            self.assertEqual(checked["effectiveSummarizerSha256"], report.sha(report.__file__))
            self.assertEqual(checked["originalSummarizerSha256"], exp["summarizerSha256"])
            self.assertEqual(checked["validatorArchiveSha256"], amendment["validatorArchiveSha256"])
            without = {k: v for k, v in exp.items() if k not in {"validatorAmendment", "validatorAmendmentSha256"}}
            with self.assertRaisesRegex(ValueError, "summarizer identity"):
                report.analysis_identity_checks(without)
            del exp["validatorAmendmentSha256"]
            with self.assertRaisesRegex(ValueError, "amendment path/hash pair"):
                report.analysis_identity_checks(exp)

    def test_rejects_amendment_hash_new_script_original_archive_and_observation_mismatch(self):
        with temporary_evidence() as run:
            exp = evidence_fixture(run)
            completed_fixture(run, exp)
            amendment = amendment_fixture(run, exp)
            exp["validatorAmendmentSha256"] = "00" * 32
            with self.assertRaisesRegex(ValueError, "validator amendment identity"):
                report.analysis_identity_checks(exp)
            for field, value, message in [
                ("summarizerSha256", "00" * 32, "amended summarizer identity"),
                ("originalTestScriptSha256", "00" * 32, "original script identities"),
                ("observedStdoutSha256", "00" * 32, "observed evidence identity"),
                ("createdUtc", "2026-09-10T00:00:01+00:00", "follows completed actual"),
            ]:
                changed = copy.deepcopy(amendment)
                changed[field] = value
                write(exp["validatorAmendment"], changed)
                exp["validatorAmendmentSha256"] = report.sha(exp["validatorAmendment"])
                with self.subTest(field=field), self.assertRaisesRegex(ValueError, message):
                    report.analysis_identity_checks(exp)
            with zipfile.ZipFile(amendment["validatorArchive"], "w") as zipped:
                zipped.writestr("tools/summarize_preflop_counterfactual.py", "changed")
                zipped.writestr("tools/tests/test_summarize_preflop_counterfactual.py", "changed")
            amendment["validatorArchiveSha256"] = report.sha(amendment["validatorArchive"])
            write(exp["validatorAmendment"], amendment)
            exp["validatorAmendmentSha256"] = report.sha(exp["validatorAmendment"])
            with self.assertRaisesRegex(ValueError, "original archived script identity"):
                report.analysis_identity_checks(exp)


if __name__ == "__main__":
    unittest.main()
