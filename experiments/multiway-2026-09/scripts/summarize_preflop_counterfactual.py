"""Validate actual/opponents-prefix diagnostics of one retained checkpoint.

This validator executes no solver. The measured main profile is unchanged;
one incidental read-only candidate traversal per seat satisfies the audit CLI.
Each target keeps its own fitted table,
all-key denominator and signed held-out estimates; aggregate gains across the
two conditional populations are never ranked or combined.
"""
from __future__ import annotations

import argparse
import copy
from datetime import datetime
import hashlib
import math
from pathlib import Path, PurePosixPath
import re
import tomllib
import zipfile

import summarize_checkpoint_write as shared
import summarize_preflop_endpoint as baseline
from summarize_checkpoint_write import (
    checked_file, digest, finite_values, integer, number, read, require,
    same_path, serialized, sha,
)

TARGETS = ("actual-prefix", "opponents-prefix")
CASES = TARGETS
FOLD4 = "fold/fold/fold/fold"
OPEN = FOLD4 + "/raise-to:3000"
THREE = OPEN + "/raise-to:10000"
FOUR = THREE + "/raise-to:21000"
FIVE = FOUR + "/raise-to:100000:all-in"
ENDPOINTS = ["root", FOLD4, THREE, FOUR, FIVE]
SUPPORTS = ["root", FOLD4, OPEN, THREE, FOUR, FIVE]
SCHEDULE = dict(fitSamples=65536, fitSeed=602, heldOutSamples=131072,
                heldOutSeeds=[702, 703], minFitEss=64)
RUNTIME_FIELDS = {"status", "completedUtc", "preexecutionSha256", "validatorAmendment", "validatorAmendmentSha256"}
DEPENDENCIES = (baseline, baseline.pilot, baseline.balanced, baseline.endpoint_dependency,
                baseline.pilot.support_dependency, baseline.pilot.evidence_dependency, shared)
COMMANDS = {
    "cargo fmt --all --check",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo clippy -p cli --examples --features research-draw-abstraction -- -D warnings",
    "cargo test --workspace",
    "cargo test -p cli --examples --features research-draw-abstraction",
    "cargo test -p multiway --features research-average-sampling --lib",
    "cargo build --release -p cli --example mw_checkpoint_audit",
}
SOURCE_PATHS = {
    "Cargo.toml", "Cargo.lock", ".cargo/config.toml", "crates/multiway/src/checkpoint.rs",
    "crates/multiway/src/solver/mod.rs", "crates/multiway/src/solver/eval.rs",
    "crates/multiway/src/solver/conditioned.rs", "crates/multiway/src/solver/preflop_proposal.rs",
    "crates/multiway/src/solver/counterfactual_proposal_tests.rs",
    "crates/multiway/src/solver/endpoint_deviation.rs",
    "crates/multiway/src/solver/endpoint_counterfactual_tests.rs",
    "crates/cli/examples/mw_checkpoint_audit.rs",
}


def schedule_checks(run, exp):
    require((exp["threads"], exp["memory"], exp["timeoutSeconds"]) == (8, "8GiB", 900), "fixed resources")
    require(exp["endpointPaths"] == ENDPOINTS and exp["supportNodes"] == SUPPORTS
            and exp["endpointSchedule"] == SCHEDULE, "fixed endpoint/support schedules")
    require(exp["promotionAllowed"] is False and exp["broaderGoalComplete"] is False
            and exp["cloudResourcesStarted"] is False, "diagnostic-only scope")
    require(len(exp["cases"]) == 2, "two case definitions")
    for case, target in zip(exp["cases"], TARGETS, strict=True):
        require(case["name"] == case["target"] == target, "fixed target/case order")
        same_path(case["job"], run / (target + "-job.json"), "case job location")
    checked_file(run / "experiment-preexecution.json", exp["preexecutionSha256"], "preexecution identity")
    before = read(run / "experiment-preexecution.json")
    require({k: v for k, v in exp.items() if k not in RUNTIME_FIELDS}
            == {k: v for k, v in before.items() if k not in RUNTIME_FIELDS}, "immutable preexecution conditions")


def evidence_checks(exp):
    same_path(exp["runner"], Path(__file__).with_name("run_average_sampling_measurement.ps1"), "frozen runner location")
    for field in ("binary", "config", "inputCheckpoint", "sourceManifest", "sourceZip", "verification", "runner"):
        checked_file(exp[field], exp[field + "Sha256"], "evidence identity: " + field)
    analysis = analysis_identity_checks(exp)
    require({Path(p).resolve() for p in exp["dependencies"]}
            == {Path(m.__file__).resolve() for m in DEPENDENCIES}, "complete frozen helper dependencies")
    for path, checksum in exp["dependencies"].items():
        checked_file(path, checksum, "helper identity")
    config = tomllib.loads(Path(exp["config"]).read_text(encoding="utf-8-sig"))
    require(config["schema"] == "solvers.multiway-preflop/v1"
            and config["game"]["seat_count"] == 6 and config["solver"]["seed"] == 0, "fixed checkpoint model")
    manifest = read(exp["sourceManifest"])
    files = {entry["path"]: digest(entry["sha256"]) for entry in manifest["files"]}
    require(len(files) == len(manifest["files"]) and SOURCE_PATHS <= set(files), "complete unique source manifest")
    require(manifest["baseRevision"] == exp["baseRevision"], "source base revision")
    for path in files:
        require(not PurePosixPath(path).is_absolute() and ".." not in PurePosixPath(path).parts
                and "\\" not in path, "canonical archive path")
    with zipfile.ZipFile(exp["sourceZip"]) as archive:
        require(len(archive.namelist()) == len(files) and set(archive.namelist()) == set(files), "exact source ZIP entries")
        for path, checksum in files.items():
            with archive.open(path) as source:
                require(hashlib.file_digest(source, "sha256").hexdigest() == checksum, "archived source identity")
    verification = read(exp["verification"])
    require(verification["status"] == "passed" and verification["sourceFiles"] == len(files)
            and verification["sourceManifestSha256"] == exp["sourceManifestSha256"]
            and verification["binarySha256"] == exp["binarySha256"], "verification source/binary/status")
    checks = verification["checks"]
    require(len({c["command"] for c in checks}) == len(checks)
            and COMMANDS <= {c["command"] for c in checks}, "required verification commands")
    for check in checks:
        require(check["exitCode"] == 0, "failed verification command")
        checked_file(check["log"], check["sha256"], "verification log identity")
        number(check["elapsedSecs"], "verification duration")
        text = Path(check["log"]).read_text(encoding="utf-8-sig")
        require("test result: FAILED" not in text and not re.search(r"^error(?:\[|:)", text, re.M), "failed verification log")
        if check["command"].startswith("cargo test "):
            rows = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
            totals = [sum(int(row[i]) for row in rows) for i in range(3)]
            require(rows and totals[0] > 0 and totals[1] == 0, "successful nonempty test log")
            require(totals == [check["passed"], check["failed"], check["ignored"]]
                    and len(rows) == check["suites"], "verification test totals")
    startup = startup_failure_checks(exp)
    return dict(sourceFiles=len(files), checks=len(checks), sourceManifestSha256=exp["sourceManifestSha256"],
                sourceZipSha256=exp["sourceZipSha256"], binarySha256=exp["binarySha256"],
                verificationSha256=exp["verificationSha256"], archivedSourcesVerified=True,
                inputCheckpointSha256=exp["inputCheckpointSha256"], priorStartupFailure=startup,
                analysis=analysis)


def analysis_identity_checks(exp):
    test = Path(__file__).parent / "tests/test_summarize_preflop_counterfactual.py"
    paired = [field in exp for field in ("validatorAmendment", "validatorAmendmentSha256")]
    require(paired[0] == paired[1], "validator amendment path/hash pair")
    if not paired[0]:
        checked_file(__file__, exp["summarizerSha256"], "summarizer identity")
        checked_file(test, exp["testScriptSha256"], "test identity")
        return dict(amendmentApplied=False, effectiveSummarizerSha256=exp["summarizerSha256"],
                    effectiveTestScriptSha256=exp["testScriptSha256"])
    checked_file(exp["validatorAmendment"], exp["validatorAmendmentSha256"], "validator amendment identity")
    amendment = read(exp["validatorAmendment"])
    require(amendment["schemaVersion"] == "solvers.counterfactual-validator-amendment/v1"
            and amendment["reason"] == "prefix-context-labels-versus-endpoint-menu", "validator amendment schema/reason")
    run = Path(exp["cases"][0]["job"]).parent
    same_path(amendment["originalExperimentSnapshot"], run / "experiment-preexecution.json", "amendment original snapshot location")
    checked_file(amendment["originalExperimentSnapshot"], amendment["originalExperimentSnapshotSha256"], "amendment original snapshot identity")
    require(amendment["originalExperimentSnapshotSha256"] == exp["preexecutionSha256"], "amendment frozen snapshot binding")
    original = read(amendment["originalExperimentSnapshot"])
    require(amendment["originalSummarizerSha256"] == original["summarizerSha256"] == exp["summarizerSha256"]
            and amendment["originalTestScriptSha256"] == original["testScriptSha256"] == exp["testScriptSha256"], "amendment original script identities")
    checked_file(amendment["validatorArchive"], amendment["validatorArchiveSha256"], "validator original archive identity")
    same_path(amendment["validatorArchive"], run / "validator-v1.zip", "validator original archive location")
    names = {"tools/summarize_preflop_counterfactual.py": exp["summarizerSha256"],
             "tools/tests/test_summarize_preflop_counterfactual.py": exp["testScriptSha256"]}
    with zipfile.ZipFile(amendment["validatorArchive"]) as archive:
        require(len(archive.namelist()) == 2 and set(archive.namelist()) == set(names), "original validator archive entries")
        for path, checksum in names.items():
            require(hashlib.sha256(archive.read(path)).hexdigest() == digest(checksum), "original archived script identity")
    checked_file(__file__, amendment["summarizerSha256"], "amended summarizer identity")
    checked_file(test, amendment["testScriptSha256"], "amended test identity")
    for field, name in (("observedStdout", "stdout.json"), ("observedMeasurement", "measurement.json")):
        same_path(amendment[field], run / "actual-prefix" / name, "amendment observed evidence location")
        checked_file(amendment[field], amendment[field + "Sha256"], "amendment observed evidence identity")
    measured = read(amendment["observedMeasurement"])
    require(measured["schemaVersion"] == "solvers.checkpoint-audit-measurement/v1"
            and measured["exitCode"] == 0 and measured["timedOut"] is False
            and measured["stdoutSha256"] == amendment["observedStdoutSha256"], "amendment completed actual observation")
    observed = read(amendment["observedStdout"])
    require(observed["policySupport"][0]["context"]["actionLabels"] == []
            and observed["policySupport"][0]["actionLabels"], "observed prefix labels differ from endpoint menu")
    created = datetime.fromisoformat(amendment["createdUtc"].replace("Z", "+00:00"))
    started = datetime.fromisoformat(measured["startedUtc"].replace("Z", "+00:00"))
    require(created.tzinfo is not None and started.tzinfo is not None
            and created.timestamp() >= started.timestamp() + number(measured["wallSeconds"], "observed process duration", True) - .001,
            "amendment follows completed actual observation")
    return dict(amendmentApplied=True, path=exp["validatorAmendment"], sha256=exp["validatorAmendmentSha256"],
                reason=amendment["reason"], createdUtc=amendment["createdUtc"], createdUnixMs=created.timestamp() * 1000,
                originalExperimentSnapshotSha256=amendment["originalExperimentSnapshotSha256"],
                originalSummarizerSha256=exp["summarizerSha256"], originalTestScriptSha256=exp["testScriptSha256"],
                validatorArchiveSha256=amendment["validatorArchiveSha256"],
                observedStdoutSha256=amendment["observedStdoutSha256"], observedMeasurementSha256=amendment["observedMeasurementSha256"],
                effectiveSummarizerSha256=amendment["summarizerSha256"], effectiveTestScriptSha256=amendment["testScriptSha256"])


def startup_failure_checks(exp):
    """Retain the rejected br0 plan and its original validator, without running it."""
    checked_file(exp["priorStartupFailure"], exp["priorStartupFailureSha256"], "startup failure identity")
    failure = read(exp["priorStartupFailure"])
    require(failure["status"] == "startup-rejected" and failure["exitCode"] == 1
            and failure["stdoutBytes"] == 0 and failure["requiredChecksPassed"] == 7
            and failure["sourceAndBinaryUnchanged"] is True
            and failure["broaderGoalComplete"] is False and failure["cloudResourcesStarted"] is False,
            "retained startup rejection status/scope")
    same_path(failure["replacementRun"], Path(exp["cases"][0]["job"]).parent, "startup replacement run")
    prior_run = Path(exp["priorStartupFailure"]).parent
    checked_file(prior_run / "experiment.json", failure["finalExperimentSha256"], "failed experiment identity")
    checked_file(prior_run / "experiment-preexecution.json", failure["originalPreexecutionSha256"], "failed preexecution identity")
    old, before = read(prior_run / "experiment.json"), read(prior_run / "experiment-preexecution.json")
    require({k: v for k, v in old.items() if k not in RUNTIME_FIELDS}
            == {k: v for k, v in before.items() if k not in RUNTIME_FIELDS}, "failed plan immutable conditions")
    require(old["preexecutionSha256"] == failure["originalPreexecutionSha256"], "failed preexecution binding")
    for field in ("binary", "config", "inputCheckpoint", "sourceManifest", "sourceZip", "verification", "runner"):
        same_path(old[field], exp[field], "unchanged startup/replacement input: " + field)
        require(old[field + "Sha256"] == exp[field + "Sha256"], "unchanged startup/replacement identity: " + field)
    for field in ("endpointPaths", "supportNodes", "endpointSchedule", "threads", "memory", "timeoutSeconds", "baseRevision"):
        require(old[field] == exp[field], "unchanged startup/replacement schedule: " + field)
    for field in ("measurement", "stderr", "preexecutionValidatorArchive"):
        checked_file(failure[field], failure[field + "Sha256"], "startup referenced identity: " + field)
    measured = read(failure["measurement"])
    require(measured["schemaVersion"] == "solvers.checkpoint-audit-measurement/v1"
            and measured["exitCode"] == 1 and measured["timedOut"] is False
            and measured["wallSeconds"] == failure["wallSeconds"], "failed measurement result")
    number(failure["wallSeconds"], "startup failure duration", True)
    same_path(failure["measurement"], prior_run / "actual-prefix/measurement.json", "failed measurement location")
    same_path(failure["stderr"], prior_run / "actual-prefix/stderr.txt", "failed stderr location")
    stdout = Path(failure["measurement"]).parent / "stdout.json"
    checked_file(stdout, measured["stdoutSha256"], "failed stdout identity")
    require(stdout.stat().st_size == 0, "rejected startup produced no solver output")
    require(Path(failure["stderr"]).read_text(encoding="utf-8-sig").strip()
            == "Error: --br-traversals must be positive", "retained positive-budget CLI error")
    require(len(old["cases"]) == 2 and old["cases"][0]["name"] == "actual-prefix", "failed case schedule")
    case = old["cases"][0]
    checked_file(case["job"], case["jobSha256"], "failed job identity")
    same_path(case["job"], prior_run / "actual-prefix-job.json", "failed job location")
    same_path(measured["job"], case["job"], "failed measured job location")
    same_path(measured["binary"], old["binary"], "failed measured binary location")
    job = read(case["job"])
    args = expected_arguments(exp, case)
    args[args.index("--br-traversals") + 1] = "0"
    require(job["arguments"] == measured["arguments"] == args and measured["jobSha256"] == case["jobSha256"], "failed literal br0 plan")
    for field in ("binarySha256", "configSha256", "sourceManifestSha256", "timeoutSeconds"):
        require(job[field] == measured[field] == old[field], "failed measured input identity: " + field)
    require(measured["sourceRevision"] == old["baseRevision"]
            and measured["validationReport"] == job["validationReport"] == old["validationReport"], "failed measurement source/report")
    scripts = {"tools/summarize_preflop_counterfactual.py": before["summarizerSha256"],
               "tools/tests/test_summarize_preflop_counterfactual.py": before["testScriptSha256"]}
    with zipfile.ZipFile(failure["preexecutionValidatorArchive"]) as archive:
        require(len(archive.namelist()) == 2 and set(archive.namelist()) == set(scripts), "failed validator archive entries")
        for name, checksum in scripts.items():
            require(hashlib.sha256(archive.read(name)).hexdigest() == digest(checksum), "archived failed validator identity")
    return dict(path=exp["priorStartupFailure"], sha256=exp["priorStartupFailureSha256"],
                status=failure["status"], measurementSha256=failure["measurementSha256"],
                preexecutionSha256=failure["originalPreexecutionSha256"],
                validatorArchiveSha256=failure["preexecutionValidatorArchiveSha256"],
                emptyStdoutVerified=True, sourceAndBinaryUnchanged=True)


def baseline_checks(exp):
    run = Path(exp["baselineRun"])
    checked_file(run / "experiment.json", exp["baselineExperimentSha256"], "baseline experiment identity")
    checked_file(run / "summary.json", exp["baselineSummarySha256"], "baseline summary identity")
    checked_file(exp["baselineRaw"], exp["baselineRawSha256"], "baseline raw identity")
    same_path(exp["baselineRaw"], run / baseline.CASE / "stdout.json", "baseline raw location")
    reproduced = baseline.summarize(run)
    require(serialized(reproduced) == (run / "summary.json").read_bytes(), "baseline summary byte reproduction")
    prior = read(run / "experiment.json")
    require(prior["configSha256"] == exp["configSha256"] and prior["endpointPaths"] == exp["endpointPaths"]
            and prior["endpointSchedule"] == exp["endpointSchedule"], "same frozen baseline config/schedule")
    raw = read(exp["baselineRaw"])
    require(len(raw["diagnostics"]["support"]) == 16 and len(raw["diagnostics"]["endpoints"]) == 5,
            "complete frozen baseline support/endpoints")
    return raw, dict(run=str(run), experimentSha256=exp["baselineExperimentSha256"],
                     summarySha256=exp["baselineSummarySha256"], rawSha256=exp["baselineRawSha256"],
                     sourceManifestSha256=prior["sourceManifestSha256"], sourceFiles=167,
                     binarySha256=prior["binarySha256"], summaryRegeneratedByteIdentically=True,
                     rootReachEvaluations=reproduced["rootEvaluations"])


def expected_arguments(exp, case):
    args = ["--config", str(Path(exp["config"]).resolve()), "--checkpoint", str(Path(exp["inputCheckpoint"]).resolve()),
            "--threads", "8", "--memory", "8GiB", "--cache-dir", str(Path(exp["cacheDir"]).resolve()),
            "--samples", "128", "--evaluation-seeds", "101,202", "--br-traversals", "1",
            "--node", "root", "--node-frequency-samples", "0"]
    for flag, values in [("--support-node", exp["supportNodes"]), ("--endpoint-prefix", exp["endpointPaths"])]:
        for value in values:
            args.extend([flag, value])
    return args + ["--endpoint-fit-samples", "65536", "--endpoint-fit-seed", "602", "--endpoint-samples", "131072",
                   "--endpoint-seeds", "702,703", "--endpoint-min-fit-ess", "64", "--endpoint-target", case["target"]]


def job_checks(exp, case):
    checked_file(case["job"], case["jobSha256"], "job identity")
    job = read(case["job"])
    require(job["arguments"] == expected_arguments(exp, case), "literal job arguments")
    require(job["timeoutSeconds"] == exp["timeoutSeconds"] and job["validationReport"] == exp["validationReport"], "job timeout/report")
    require(job["binarySha256"] == exp["binarySha256"], "job binary identity")
    for field in ("config", "inputCheckpoint", "sourceManifest"):
        same_path(job[field], exp[field], "job input location: " + field)
        require(job[field + "Sha256"] == exp[field + "Sha256"], "job input identity: " + field)
    return job


def normalized_support(node):
    """Match research's normalized support, retaining missing versus stored zero."""
    baseline.pilot.support_dependency.support_checks(node)
    context = node["context"]
    require(set(context) == {"requested", "history", "actionIndices", "actionLabels", "actor", "street", "activeOpponents"}, "complete public support context")
    require(context["street"] == "preflop" and node["expectedBuckets"] == 169, "preflop support dimensions")
    prefix_labels = [] if context["requested"] == "root" else context["requested"].split("/")
    require(context["actionLabels"] == prefix_labels
            and len(context["actionIndices"]) == len(prefix_labels), "public prefix path labels/indices")
    require(re.fullmatch(r"[0-9a-f]{32}", context["history"]) and type(context["actor"]) is int
            and 0 <= context["actor"] < 6 and type(context["activeOpponents"]) is int
            and 1 <= context["activeOpponents"] <= 5
            and node["bucketActiveOpponents"] == context["activeOpponents"], "preflop public counts/history")
    require(all(type(a) is int and 0 <= a < 8 for a in context["actionIndices"]), "legal path indices")
    support = dict(history=list(bytes.fromhex(context["history"])), action_indices=context["actionIndices"],
                   actor=context["actor"], street="preflop", active_opponents=context["activeOpponents"],
                   bucket_active_opponents=node["bucketActiveOpponents"], expected_buckets=169,
                   action_labels=node["actionLabels"], rows=[])
    for bucket, row in enumerate(node["rows"]):
        require(set(row) == {"bucket", "status", "regrets", "strategySum", "strategyMass", "currentStrategy", "averageStrategy"}, "complete raw support fields")
        support["rows"].append(dict(key=baseline.key_for(support, bucket), regrets=row["regrets"], average_strategy=row["averageStrategy"]))
    baseline.support_checks(support)
    return support


def support_checks(raw, exp, reference):
    require(len(raw["policySupport"]) == 6
            and [s["context"]["requested"] for s in raw["policySupport"]] == exp["supportNodes"], "six support contexts/order")
    old = dict(zip([c["requested"] for c in reference["supportNodes"]], reference["diagnostics"]["support"], strict=True))
    supports = {}
    for node in raw["policySupport"]:
        path = node["context"]["requested"]
        support = normalized_support(node)
        if path in old:
            require(support == old[path], "all legacy normalized support fields exactly match")
        supports[path] = support
    require(len({tuple(s["history"]) for s in supports.values()}) == 6, "unique support histories")
    extra, next_node = supports[OPEN], supports[THREE]
    require(extra["action_indices"] == next_node["action_indices"][:-1]
            and extra["actor"] == supports[FOUR]["actor"]
            and extra["active_opponents"] == extra["bucket_active_opponents"] == 1
            and extra["action_labels"][next_node["action_indices"][-1]] == "raise-to:10000", "additional BB prior public path/context")
    for path in ENDPOINTS[1:]:
        child = supports[path]
        for parent in supports.values():
            depth = len(parent["action_indices"])
            if depth < len(child["action_indices"]) and child["action_indices"][:depth] == parent["action_indices"]:
                expected_label = path.split("/")[depth]
                a = child["action_indices"][depth]
                require(a < len(parent["action_labels"]) and parent["action_labels"][a] == expected_label, "ancestor action label/index binding")
    return supports


def own_probabilities(endpoint, supports):
    f32 = baseline.pilot.support_dependency.f32
    path, actor = endpoint["action_indices"], endpoint["actor"]
    values = [1.0] * 169
    for depth, action in enumerate(path):
        prior = next((s for s in supports.values() if s["action_indices"] == path[:depth]), None)
        # All earlier own decisions for these five endpoints are exported;
        # omitted prefix nodes are initial folds by other seats.
        if prior is None:
            require(depth < 4 and actor in (1, 2), "missing prior own-context evidence")
            continue
        if prior["actor"] != actor:
            continue
        require(action < len(prior["action_labels"]), "own prefix action bound")
        for bucket, row in enumerate(prior["rows"]):
            strategy = row["average_strategy"]
            if strategy is None:
                if row["regrets"] is None:
                    strategy = [f32(1 / len(prior["action_labels"]))] * len(prior["action_labels"])
                else:
                    strategy = baseline.pilot.support_dependency.normalized_f32([max(f32(v), 0.0) for v in row["regrets"]])
            strategy = [f32(v) for v in strategy]
            before = 0.0
            for p in strategy[:action]:
                before += p
            before = min(before, 1.0)
            after = 1.0 if action + 1 == len(strategy) else min(before + strategy[action], 1.0)
            values[bucket] *= after - before
    return values


def expected_context(path, support):
    return dict(requested=path, history=baseline.hex_bytes(support["history"]),
                actionIndices=support["action_indices"], actionLabels=[] if path == "root" else path.split("/"),
                actor=support["actor"], street=support["street"], activeOpponents=support["active_opponents"])


def without_endpoint_clocks(result):
    return baseline.pilot.without_endpoint_clocks(result)


def endpoint_rows(raw, exp, case, reference, supports):
    target = case["target"]
    field = "endpointDeviations" if target == "actual-prefix" else "endpointCounterfactualDeviations"
    other = "endpointCounterfactualDeviations" if target == "actual-prefix" else "endpointDeviations"
    require(other not in raw and "endpointDeviation" not in raw and "endpointCounterfactualDeviation" not in raw,
            "exclusive repeated endpoint target output")
    entries = raw[field]
    require(len(entries) == 5, "five repeated endpoint results")
    roots = reference["diagnostics"]["endpoints"][0]["proposal"]
    rows = []
    for index, (entry, path, old) in enumerate(zip(entries, exp["endpointPaths"], reference["diagnostics"]["endpoints"], strict=True)):
        support = supports[path]
        require(entry["context"] == expected_context(path, support), "endpoint public context/order")
        wrapped = entry["result"]
        result = wrapped if target == "actual-prefix" else wrapped["evaluation"]
        finite_values(result)
        selection = baseline.endpoint_checks(result, exp["endpointSchedule"], support)
        baseline.proposal_checks(result["proposal"], result, roots["root_range_fingerprint"])
        require(baseline.hex_bytes(result["configuration_fingerprint"], 32) == raw["configurationFingerprint"]
                and baseline.hex_bytes(result["abstraction_fingerprint"], 32) == raw["abstractionFingerprint"], "endpoint model identity")
        expected_own = own_probabilities(result, supports)
        if target == "actual-prefix":
            require(without_endpoint_clocks(result) == without_endpoint_clocks(old), "actual endpoint exact legacy replay")
        else:
            require(set(wrapped) == {"target", "proposal_kind", "excluded_actor", "validated_class_contexts", "own_prefix_probability_by_bucket", "evaluation"}, "complete CF wrapper")
            require(wrapped["target"] == "opponents-prefix" and wrapped["proposal_kind"] == "preflop-opponents-proposal", "explicit counterfactual target/proposal")
            require(type(wrapped["excluded_actor"]) is int and wrapped["excluded_actor"] == result["actor"], "excluded endpoint actor")
            require(type(wrapped["validated_class_contexts"]) is int
                    and wrapped["validated_class_contexts"] == len(result["action_indices"]) + 1, "all validated public class contexts")
            own = wrapped["own_prefix_probability_by_bucket"]
            require(isinstance(own, list) and len(own) == 169
                    and all(type(p) in (int, float) and math.isfinite(p) and 0 <= p <= 1 for p in own), "169 bounded own-prefix factors")
            require(own == expected_own, "own-prefix factors reconstructed from frozen policies")
            for bucket, p in enumerate(own):
                if p == 0:
                    require(old["fit"]["rows"][bucket]["positive_weight_samples"] == 0, "own-zero has no actual-prefix fit weight")
            for seat in range(6):
                expected_proposal = roots if seat == result["actor"] else old["proposal"]
                for key in ("positive_target_combos_by_seat", "floor_adjusted_combos_by_seat", "target_scale_by_seat"):
                    require(result["proposal"][key][seat] == expected_proposal[key][seat], "CF changes only excluded actor proposal factors")
            if index < 2:
                require(own == [1.0] * 169 and without_endpoint_clocks(result) == without_endpoint_clocks(old), "root/unopened counterfactual equals actual exactly")
        for sample in [result["fit"]["sampling"]] + [h["sampling"] for h in result["held_out"]]:
            require(sample["positive_weight_samples"] == sample["samples"], "preflop proposal accepted worlds have positive target weight")
        elapsed = number(entry["elapsedSecs"], "endpoint total duration", True)
        require(result["fit_elapsed_secs"] + sum(h["elapsed_secs"] for h in result["held_out"]) <= elapsed + .001, "endpoint timing containment")
        zero = [i for i, p in enumerate(expected_own) if p == 0]
        weights = result["fit"]["rows"]
        total = sum(row["relative_weight_sum"] for row in weights)
        rows.append(dict(context=entry["context"], target=target, elapsedSecs=elapsed,
                         fitSelection=selection, ownPrefixProbabilityByBucket=expected_own,
                         zeroOwnReachBuckets=zero,
                         zeroOwnReachFitPositiveBuckets=[i for i in zero if weights[i]["positive_weight_samples"] > 0],
                         zeroOwnReachFitWeightFraction=sum(weights[i]["relative_weight_sum"] for i in zero) / total,
                         result=wrapped))
    return rows


def deviator_checks(dev):
    require(type(dev["traversalsPerSeat"]) is int and dev["traversalsPerSeat"] == 1 and dev["seed"] == 0x62722d6175646974
            and len(dev["coverage"]) == 6, "one incidental candidate traversal per seat")
    fields = {"traversals", "visited_infosets", "retained_infosets", "total_visits", "retained_visits"}
    for c in dev["coverage"]:
        require(set(c) == fields and all(type(v) is int and 0 <= v <= 2**64 - 1 for v in c.values())
                and c["traversals"] == 1, "candidate coverage count types/budget")
        visited, retained, total, kept = (c[k] for k in ("visited_infosets", "retained_infosets", "total_visits", "retained_visits"))
        require(retained <= visited <= total and kept <= total and kept >= 8 * retained
                and (retained > 0) == (kept > 0)
                and visited - retained <= total - kept <= 7 * (visited - retained),
                "candidate coverage partition/retention threshold")
    number(dev["elapsedSecs"], "incidental candidate duration")


def raw_checks(raw, exp, case, reference):
    finite_values(raw)
    require(raw["schemaVersion"] == "solvers.multiway-checkpoint-audit/v1" and raw["sweeps"] == 32768
            and raw["solverStateVersion"] == reference["solverStateVersion"] == 4, "restored audit schema/progress")
    for key in ("freshTraining", "coverageEvaluations", "conditionalEvaluations", "preflopConditionalEvaluations"):
        require(key not in raw, "no undeclared training/diagnostics")
    same_path(raw["config"], exp["config"], "raw config location")
    same_path(raw["checkpoint"], exp["inputCheckpoint"], "raw checkpoint location")
    for key in ("configurationFingerprint", "abstractionFingerprint"):
        require(digest(raw[key]) == reference[key], "same retained model identity")
    require(raw["evaluationSamplesPerSeed"] == 128 and raw["evaluationSeeds"] == [101, 202]
            and [e["seed"] for e in raw["evaluations"]] == [101, 202], "incidental evaluation schedule")
    for entry in raw["evaluations"]:
        number(entry["elapsedSecs"], "ordinary duration", True)
        baseline.pilot.profile_checks(entry["result"], 128, False)
    dev = raw["deviatorTraining"]
    deviator_checks(dev)
    require(len(raw["nodes"]) == 1 and raw["nodes"][0]["requested"] == "root"
            and raw["nodes"][0]["frequency"] is None and "frequencyElapsedSecs" not in raw["nodes"][0], "root export without frequency sampling")
    root = reference["diagnostics"]["support"][0]
    node = raw["nodes"][0]
    require(node["history"] == baseline.hex_bytes(root["history"]) and node["actor"] == root["actor"]
            and node["activeOpponents"] == root["active_opponents"] and node["actions"] == root["action_labels"], "root export public identity")
    arena = raw["policyArena"]
    require(arena["pages_committed"] is True and all(type(arena[k]) is int and arena[k] > 0 for k in ("nodes", "columns", "slots", "bytes"))
            and arena["bytes"] <= 8 * 1024**3 and arena["slots"] >= arena["columns"], "admitted dense arena")
    supports = support_checks(raw, exp, reference)
    return endpoint_rows(raw, exp, case, reference, supports)


def common_result(raw):
    result = copy.deepcopy(raw)
    result.pop("constructionElapsedSecs")
    result.pop("endpointDeviations", None)
    result.pop("endpointCounterfactualDeviations", None)
    result["deviatorTraining"].pop("elapsedSecs")
    for entry in result["evaluations"]:
        entry.pop("elapsedSecs")
    return result


def summarize_case(run, exp, case, reference):
    job = job_checks(exp, case)
    folder = run / case["name"]
    raw, measured = read(folder / "stdout.json"), read(folder / "measurement.json")
    endpoints = raw_checks(raw, exp, case, reference)
    finite_values(measured)
    require(measured["schemaVersion"] == "solvers.checkpoint-audit-measurement/v1"
            and measured["exitCode"] == 0 and measured["timedOut"] is False, "successful measurement")
    same_path(measured["binary"], exp["binary"], "measurement binary path")
    same_path(measured["job"], case["job"], "measurement job path")
    require(measured["arguments"] == job["arguments"] and measured["timeoutSeconds"] == 900
            and measured["validationReport"] == exp["validationReport"], "measurement arguments/timeout/report")
    for field, checksum in dict(binary=exp["binarySha256"], job=case["jobSha256"], config=exp["configSha256"], sourceManifest=exp["sourceManifestSha256"]).items():
        require(measured[field + "Sha256"] == checksum, "measurement identity: " + field)
    require(measured["sourceRevision"] == exp["baseRevision"], "measured base revision")
    checked_file(folder / "stdout.json", measured["stdoutSha256"], "raw output identity")
    wall = number(measured["wallSeconds"], "whole-process duration", True)
    construct = number(raw["constructionElapsedSecs"], "constructor duration", True)
    diagnostic = sum(e["elapsedSecs"] for e in endpoints)
    require(construct + diagnostic + raw["deviatorTraining"]["elapsedSecs"] + sum(e["elapsedSecs"] for e in raw["evaluations"]) <= wall + .01, "whole-process phase containment")
    start = datetime.fromisoformat(measured["startedUtc"].replace("Z", "+00:00"))
    require(start.tzinfo is not None, "timestamp timezone")
    return dict(case=case["name"], target=case["target"], endpoints=endpoints,
                constructionSeconds=construct, endpointDiagnosticsSeconds=diagnostic,
                wallSeconds=wall, observedPeakWorkingSetBytes=integer(measured["observedPeakWorkingSetBytes"], "lifetime peak", True),
                peakMethod=measured["peakMethod"], processStartedUnixMs=start.timestamp() * 1000,
                processFinishedUnixMs=start.timestamp() * 1000 + wall * 1000,
                measurementSha256=sha(folder / "measurement.json"), stdoutSha256=measured["stdoutSha256"],
                commonResult=common_result(raw))


def result_of(endpoint):
    return endpoint["result"] if endpoint["target"] == "actual-prefix" else endpoint["result"]["evaluation"]


def comparison(cases):
    require(len(cases) == 2 and tuple(c["case"] for c in cases) == CASES
            and tuple(c["target"] for c in cases) == TARGETS, "both completed target cases in fixed order")
    actual, cf = cases
    require(actual["processFinishedUnixMs"] <= cf["processStartedUnixMs"] + 1, "serialized process order/overlap")
    require(actual["commonResult"] == cf["commonResult"], "all non-endpoint non-clock raw fields exactly match")
    require(len(actual["endpoints"]) == len(cf["endpoints"]) == 5, "complete endpoint comparison")
    endpoints = []
    for index, (a, c) in enumerate(zip(actual["endpoints"], cf["endpoints"], strict=True)):
        require(a["context"] == c["context"], "cross-target public context")
        left, right = result_of(a), result_of(c)
        if index < 2:
            require(without_endpoint_clocks(left) == without_endpoint_clocks(right), "root/unopened target equality")
        keys = []
        for bucket, (arow, crow) in enumerate(zip(left["fit"]["rows"], right["fit"]["rows"], strict=True)):
            require(arow["key"] == crow["key"], "cross-target own key")
            keys.append(dict(bucket=bucket, key=arow["key"], ownPrefixProbability=c["ownPrefixProbabilityByBucket"][bucket],
                             actualPrefix=arow, opponentsPrefix=crow))
        eligible_a = {i for i, r in enumerate(left["fit"]["rows"]) if r["effective_sample_size"] >= SCHEDULE["minFitEss"]}
        eligible_c = {i for i, r in enumerate(right["fit"]["rows"]) if r["effective_sample_size"] >= SCHEDULE["minFitEss"]}
        endpoints.append(dict(context=a["context"], perKeyFit=keys,
                              actualFitSelection=a["fitSelection"], opponentsFitSelection=c["fitSelection"],
                              newlyEligibleBuckets=sorted(eligible_c - eligible_a), noLongerEligibleBuckets=sorted(eligible_a - eligible_c),
                              actualHeldOut=left["held_out"], opponentsHeldOut=right["held_out"],
                              aggregateGainRankingAllowed=False, ownZeroBuckets=c["zeroOwnReachBuckets"]))
    return dict(allNonEndpointNonClockRawFieldsExactlyMatch=True, rootAndUnopenedTargetsExactlyMatch=True,
                endpoints=endpoints, opponentsToActualEndpointTimeRatio=cf["endpointDiagnosticsSeconds"] / actual["endpointDiagnosticsSeconds"],
                opponentsToActualWallTimeRatio=cf["wallSeconds"] / actual["wallSeconds"],
                aggregateGainRankingAllowed=False, pooledSeeds=False, pairedErrorBars=False)


def summarize(run, case_name=None):
    run = Path(run)
    exp = read(run / "experiment.json")
    finite_values(exp)
    schedule_checks(run, exp)
    require(case_name is None or case_name in CASES, "unknown case")
    evidence = evidence_checks(exp)
    reference, baseline_evidence = baseline_checks(exp)
    cases = [summarize_case(run, exp, c, reference) for c in exp["cases"] if case_name is None or c["name"] == case_name]
    if evidence["analysis"]["amendmentApplied"]:
        for case in cases:
            if case["target"] == "opponents-prefix":
                require(evidence["analysis"]["createdUnixMs"] <= case["processStartedUnixMs"] + 1,
                        "validator amendment precedes counterfactual measurement")
    return dict(schemaVersion="solvers.preflop-counterfactual-summary/v1",
                status="completed-target-comparison" if case_name is None else "completed-case",
                experiment=exp, experimentSha256=sha(run / "experiment.json"), evidence=evidence,
                baselineEvidence=baseline_evidence, cases=cases,
                comparison=comparison(cases) if case_name is None else None,
                promotionAllowed=False, broaderGoalComplete=False, cloudResourcesStarted=False,
                interpretation="One retained 32768-sweep production profile, no additional main-profile training; one incidental read-only candidate traversal per seat satisfies the existing audit CLI. Actual-prefix endpoint output must exactly reproduce the frozen baseline except documented clocks. Opponents-prefix excludes only the endpoint actor's earlier action probabilities, keeps all folded-card blockers and all unsupported-key weight, and fits its own independent table. Root/unopened targets coincide; deep aggregate gains and relative-weight means concern different populations and are not ranked. All 169 per-key fit estimates, eligibility partitions, held-out signed gains and coverage are retained. Evaluation seeds do not create independent learning seeds, no cross-target paired SE or pooled estimate is claimed, and ESS/source coverage alone does not establish strategy quality. Root reach is separately retained from the historical baseline, never inferred from relative proposal weights. Working-set observations cover the whole process; no phase RSS is inferred. The frozen runner records declared input hashes; actual retained inputs are checked by this validator, without a before/after time-of-use guarantee.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--case", choices=CASES)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = serialized(summarize(args.run, args.case))
    if args.output:
        args.output.write_bytes(output)
    else:
        print(output.decode("utf-8"), end="")


if __name__ == "__main__":
    main()
