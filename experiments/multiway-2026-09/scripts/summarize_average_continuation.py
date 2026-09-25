"""Validate the predeclared, two-case average-continuation pilot.

This is a fixed-sweep learning-identity check and conditional diagnostic screen,
not a promotion decision. Missing and stored-zero columns remain distinct in
support counts, but both contain numerically zero regrets for pair comparison.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timedelta
import hashlib
import json
import math
import re
import tomllib
import zipfile
from pathlib import Path

import summarize_balanced_endpoint as balanced
import summarize_endpoint_deviation as endpoint_dependency
import summarize_opponent_exploration as support_dependency
import summarize_preflop_proposal as evidence_dependency
from summarize_preflop_proposal import estimate_checks, read, require, sha, source_checks

STREETS = ("preflop", "flop", "turn", "river")
VARIANTS = ("uniform-one", "postflop-continuation")
SOURCES = ("decision", "stored_strategy", "average_strategy", "current_strategy",
           "regret_fallback", "uniform_fallback")
COMMANDS = {
    "cargo fmt --all --check",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo clippy -p cli --examples --features research-draw-abstraction -- -D warnings",
    "cargo test --workspace",
    "cargo test -p cli --examples --features research-draw-abstraction",
    "cargo test -p multiway --features research-average-sampling --lib",
    "cargo build --release -p cli --features research-average-sampling --example mw_average_sampling_research",
}


def number(value, label, *, positive=False):
    require(type(value) in (int, float) and math.isfinite(value)
            and value >= 0 and (not positive or value > 0), label)
    return value


def count(value, maximum, label):
    require(type(value) is int and 0 <= value <= maximum, label)
    return value


def hex_bytes(value, size=16):
    require(isinstance(value, list) and len(value) == size
            and all(type(v) is int and 0 <= v < 256 for v in value), "byte identity")
    return bytes(value).hex()


def digest(value):
    require(isinstance(value, str) and re.fullmatch("[0-9a-f]{64}", value), "digest format")
    return value


def normalized_only(value):
    if isinstance(value, dict):
        require(not ({"strategy_sum", "strategySum", "strategyMass", "checkpoint", "solver_state", "snapshot_state"}
                     & set(value)), "research output must not expose state or raw average mass")
        for nested in value.values():
            normalized_only(nested)
    elif isinstance(value, list):
        for nested in value:
            normalized_only(nested)


def without_endpoint_clocks(value):
    """Only the evaluator's documented clocks are excluded from exact replay."""
    result = dict(value)
    result.pop("fit_elapsed_secs")
    result["held_out"] = [{k: v for k, v in held.items() if k != "elapsed_secs"}
                          for held in value["held_out"]]
    return result


def options(arguments):
    require(len(arguments) % 2 == 0, "literal key/value arguments")
    output = {}
    repeated = {"--node", "--support-node", "--coverage-prefix", "--endpoint-prefix"}
    for key, value in zip(arguments[::2], arguments[1::2], strict=True):
        require(isinstance(key, str) and key.startswith("--") and isinstance(value, str), "literal argument")
        require(key not in output or key in repeated, "duplicate scalar argument")
        output.setdefault(key, []).append(value)
    return output


def evidence_checks(run, exp):
    for key, name in [("sourceManifestSha256", "source-manifest.json"),
                      ("sourceZipSha256", "source.zip"), ("configSha256", "config-seed0.toml"),
                      ("binarySha256", "research.exe"), ("verificationSha256", "verification/verification.json")]:
        require(sha(run / name) == exp[key], "evidence identity: " + name)
    require(sha(__file__) == exp["summarizerSha256"], "summarizer identity")
    require(sha(Path(__file__).with_name("run_average_sampling_measurement.ps1")) == exp["runnerSha256"], "runner identity")
    dependencies = [balanced, endpoint_dependency, support_dependency, evidence_dependency]
    expected = {Path(module.__file__).resolve() for module in dependencies}
    require({Path(path).resolve() for path in exp["dependencies"]} == expected, "complete analysis dependency set")
    for path, checksum in exp["dependencies"].items():
        require(sha(path) == checksum, "analysis dependency: " + path)
    manifest = read(run / "source-manifest.json")
    files = {item["path"]: item["sha256"] for item in manifest["files"]}
    require(len(files) == len(manifest["files"]) == 167, "complete unique source manifest")
    require(manifest["baseRevision"] == exp["baseRevision"], "source base revision")
    with zipfile.ZipFile(run / "source.zip") as archive:
        require(len(archive.namelist()) == len(files) and set(archive.namelist()) == set(files), "source ZIP entries")
        for path, checksum in files.items():
            with archive.open(path) as source:
                require(hashlib.file_digest(source, "sha256").hexdigest() == checksum, "archived source: " + path)
    verification = read(run / "verification/verification.json")
    require(verification["status"] == "passed"
            and verification["sourceManifestSha256"] == exp["sourceManifestSha256"], "verification status/source")
    checks = verification["checks"]
    require(len(checks) == 7 and {c["command"] for c in checks} == COMMANDS, "seven required verification commands")
    for check in checks:
        require(check["exitCode"] == 0 and sha(check["log"]) == check["sha256"], "verification log identity/pass")
        number(check["elapsedSecs"], "verification clock")
        if check["command"].startswith("cargo test "):
            text = Path(check["log"]).read_text(encoding="utf-8-sig")
            rows = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
            totals = [sum(int(row[i]) for row in rows) for i in range(3)]
            require(rows and totals[0] > 0 and totals[1] == 0 and "test result: FAILED" not in text,
                    "successful nonempty test log")
            require(totals == [check["passed"], check["failed"], check["ignored"]]
                    and len(rows) == check["suites"], "test totals")
    return verification, len(files)


def context_checks(context, support):
    street = support["street"]
    require(street in STREETS and context["street"] == street
            and context["history"] == hex_bytes(support["history"])
            and context["actor"] == support["actor"]
            and context["activeOpponents"] == support["active_opponents"], "support public context")
    count(support["actor"], 5, "actor range")
    require(1 <= support["active_opponents"] <= support["bucket_active_opponents"] <= 5,
            "current/bucket opponent contexts")
    require(support["expected_buckets"] == (169 if street == "preflop" else 32), "configured bucket denominator")
    require(len(support["action_indices"]) == (0 if context["requested"] == "root"
                                              else len(context["requested"].split("/"))), "public path depth")
    require(all(type(i) is int and i >= 0 for i in support["action_indices"]), "public path indices")


def key_for(support, bucket):
    street = STREETS.index(support["street"])
    path = [2**32 - 1] * 4
    path[street] = bucket
    return dict(history=support["history"], player=support["actor"], street=street,
                active_opponents=support["active_opponents"], bucket_path=path)


def support_checks(support):
    labels, rows = support["action_labels"], support["rows"]
    require(labels and all(isinstance(v, str) and v for v in labels) and len(set(labels)) == len(labels), "legal support menu")
    require(len(rows) == support["expected_buckets"], "complete support bucket rows")
    counts = dict(missingBuckets=0, storedBuckets=0, storedZeroRegretBuckets=0,
                  nonzeroRegretBuckets=0, positiveRegretBuckets=0, averageBuckets=0,
                  averageAndNonzeroRegretBuckets=0)
    for bucket, row in enumerate(rows):
        require(set(row) == {"key", "regrets", "average_strategy"}, "normalized support only; no raw average mass")
        require(row["key"] == key_for(support, bucket), "support own-information key")
        regrets, average = row["regrets"], row["average_strategy"]
        if regrets is None:
            require(average is None, "missing column cannot have average")
            counts["missingBuckets"] += 1
            continue
        require(len(regrets) == len(labels) and all(type(v) in (int, float) and math.isfinite(v) for v in regrets), "finite regret vector")
        nonzero, positive = any(v != 0 for v in regrets), any(v > 0 for v in regrets)
        counts["storedBuckets"] += 1
        counts["storedZeroRegretBuckets"] += not nonzero
        counts["nonzeroRegretBuckets"] += nonzero
        counts["positiveRegretBuckets"] += positive
        if average is not None:
            require(len(average) == len(labels)
                    and all(type(v) in (int, float) and math.isfinite(v) and v >= 0 for v in average)
                    and math.isclose(sum(average), 1.0, abs_tol=2e-6), "normalized average vector")
            counts["averageBuckets"] += 1
            counts["averageAndNonzeroRegretBuckets"] += nonzero
    return counts


def numeric_regrets(support):
    return [row["regrets"] if row["regrets"] is not None else [0.0] * len(support["action_labels"])
            for row in support["rows"]]


def endpoint_checks(result, schedule, support):
    config = dict(fit_samples=schedule["fitSamples"], fit_seed=schedule["fitSeed"],
                  held_out_samples=schedule["heldOutSamples"], held_out_seeds=schedule["heldOutSeeds"],
                  min_fit_ess=schedule["minFitEss"])
    require(result["config"] == config, "endpoint schedule")
    require(result["variant"] == dict(purify_threshold=0.0, use_current_strategy=False), "baseline average profile")
    for name in ("history", "action_indices", "actor", "street", "active_opponents",
                 "bucket_active_opponents", "expected_buckets", "action_labels"):
        require(result[name] == support[name], "endpoint/support identity: " + name)
    support_checks(support)
    actions = len(result["action_labels"])
    require(result["street"] == "river" and 1 <= actions <= 8, "river endpoint action bound")
    fit, rows = result["fit"], result["fit"]["rows"]
    endpoint_dependency.sampling_checks(fit["sampling"], config["fit_samples"], config["fit_seed"], 6)
    require(fit["sampling"]["terminal_replays"] == (actions + 1) * fit["sampling"]["positive_weight_samples"], "fit action replay enumeration")
    require(len(rows) == support["expected_buckets"], "complete fit bucket rows")
    weights, positives, retained = [], 0, 0
    for bucket, (row, raw) in enumerate(zip(rows, support["rows"], strict=True)):
        require(row["key"] == key_for(support, bucket), "fit own-information key")
        source = "uniform-fallback" if raw["regrets"] is None else "average" if raw["average_strategy"] is not None else "regret-fallback"
        require(row["baseline_source"] == source, "baseline source/support mapping")
        n = count(row["positive_weight_samples"], config["fit_samples"], "fit sample count")
        weight = number(row["relative_weight_sum"], "fit weight")
        require((weight > 0) == (n > 0), "fit weight denominator")
        ess = number(row["effective_sample_size"], "fit ESS")
        maximum = number(row["max_normalized_weight"], "fit maximum weight")
        require(ess <= n + 1e-8 and maximum <= 1 + 1e-12, "fit concentration bound")
        if n < 2:
            require(ess == maximum == n, "zero/singleton concentration")
        gains = row["action_gains"]
        require(len(gains) == actions and all((g is not None) == (n >= 2) for g in gains), "per-key gain denominator")
        for gain in gains:
            if gain is not None:
                estimate_checks(gain, "signed fit gain")
        expected = None
        if n >= 2 and ess >= config["min_fit_ess"]:
            best = max(range(actions), key=lambda a: gains[a]["mean"])
            if gains[best]["mean"] > 0:
                expected = best
        require(row["selected_action"] is None or type(row["selected_action"]) is int, "selected action type")
        require(row["selected_action"] == expected, "fit-only gate/first argmax")
        retained += expected is not None
        positives += n
        weights.append(weight)
    require(retained == fit["retained_buckets"] and positives == fit["sampling"]["positive_weight_samples"], "fit count partition")
    require(math.isclose(sum(weights), config["fit_samples"] * fit["sampling"]["relative_weight_mean"]["mean"], rel_tol=1e-11), "all-key fit weight partition")
    require(len(result["held_out"]) == len(config["held_out_seeds"]), "held-out cardinality")
    for held, seed in zip(result["held_out"], config["held_out_seeds"], strict=True):
        sampling = held["sampling"]
        endpoint_dependency.sampling_checks(sampling, config["held_out_samples"], seed, 6)
        n = sampling["positive_weight_samples"]
        replays = count(sampling["terminal_replays"], 2 * n, "held-out replay budget") - n
        require(replays >= 0, "baseline replay count")
        for field in ("gain", "retained_key_weight_fraction"):
            require((held[field] is not None) == (n > 0), "all-prefix held-out denominator")
            if held[field] is not None:
                estimate_checks(held[field], field, probability=field != "gain")
        if not retained:
            require(replays == 0, "no candidate outside frozen fit table")
        if n and replays == 0:
            require(held["gain"] == held["retained_key_weight_fraction"] == dict(mean=0.0, stderr=0.0), "unsupported worlds retain baseline")
        elif n:
            fraction = held["retained_key_weight_fraction"]["mean"]
            require(fraction > 0, "candidate weight positive")
            if replays == n:
                require(math.isclose(fraction, 1.0, abs_tol=1e-12), "all candidates weight fraction")
        number(held["elapsed_secs"], "held-out clock", positive=True)
    number(result["fit_elapsed_secs"], "fit clock", positive=True)
    return balanced.fit_selection(result, config["min_fit_ess"])


def seat_coverage(rows):
    require(len(rows) == 6, "coverage seat count")
    totals = {s: {c: 0 for c in SOURCES} for s in STREETS}
    for row in rows:
        for source in SOURCES:
            values = row[source + "_visits_by_street"]
            require(set(values) == set(STREETS), "coverage streets")
            require(sum(count(values[s], 2**64 - 1, "coverage count") for s in STREETS)
                    == row[source + "_visits"], "coverage street partition")
            for street in STREETS:
                totals[street][source] += values[street]
        for street in STREETS:
            at = {source: row[source + "_visits_by_street"][street] for source in SOURCES}
            require(at["decision"] == at["stored_strategy"] + at["uniform_fallback"]
                    and at["stored_strategy"] == at["average_strategy"] + at["current_strategy"] + at["regret_fallback"]
                    and at["current_strategy"] == 0, "coverage source partition")
    return totals


def profile_checks(profile, n, baseline):
    require(profile["samples"] == n and profile["total_deal_attempts"] >= n, "profile sample budget")
    seat_coverage(profile["candidate_policy_coverage"])
    gain = profile["deviation_gain_lower_bound"]
    require((gain is None) == baseline, "ordinary/baseline deviation availability")
    for estimates in [profile["seats"]] + ([] if baseline else [gain]):
        require(len(estimates) == 6, "profile seat cardinality")
        for estimate in estimates:
            estimate_checks(estimate, "profile estimate")
            require(len(estimate["ci95"]) == 2 and all(math.isfinite(x) for x in estimate["ci95"])
                    and estimate["ci95"][0] <= estimate["mean"] <= estimate["ci95"][1], "profile interval")


def public_layout(support):
    return {k: v for k, v in support.items() if k != "rows"}


def references_checks(exp):
    output = []
    for path, checksum in exp["references"].items():
        require(sha(path) == checksum, "reference raw hash")
        raw = read(path)
        require(sha(raw["config"]) == exp["configSha256"] and raw["sweeps"] == exp["pilotSweeps"], "reference config/sweeps")
        output.append((path, raw))
    require(len(output) == 4, "four independent retained reference files")
    return output


def schedule_checks(exp):
    require([(c["name"], c["variant"], c["trainingSeed"], c["sweeps"], c["timeoutSeconds"]) for c in exp["cases"]]
            == [("seed0-" + v, v, 0, 32768, 900) for v in VARIANTS], "two predeclared seed-zero pilot cases")
    require(exp["pilotSweeps"] == 32768 and exp["trainingSeeds"] == [0, 11, 29]
            and exp["threads"] == 8 and exp["memory"] == "8GiB", "pilot resource schedule")
    require(exp["endpointSchedule"] == dict(fitSamples=65536, fitSeed=602, heldOutSamples=131072,
                                            heldOutSeeds=[702, 703], minFitEss=64), "endpoint fixed budget")
    require(exp["rootSchedule"] == dict(samples=262144, seeds=[801, 802]), "root independent budget")
    require(exp["ordinarySchedule"] == dict(samples=128, coverageSamples=131072, seeds=[101, 202]), "ordinary schedule")
    for name, size in [("nodes", 16), ("coveragePrefixes", 8), ("endpointPaths", 3)]:
        require(len(exp[name]) == len(set(exp[name])) == size, "requested path cardinality")
    require(set(exp["endpointPaths"]) <= set(exp["nodes"]), "endpoint raw support coverage")


def case_config(run, exp, case):
    """An explicit future seed config must differ from the anchor only by seed.

    This helper does not admit a new cohort: schedule_checks still restricts
    the top-level summarizer to its two predeclared pilot cases.
    """
    require(("config" in case) == ("configSha256" in case), "explicit config requires path and hash")
    path = Path(case["config"]) if "config" in case else run / "config-seed0.toml"
    checksum = case.get("configSha256", exp["configSha256"])
    require(sha(path) == checksum, "case config file hash")
    config = tomllib.loads(path.read_text(encoding="utf-8-sig"))
    seed = count(case["trainingSeed"], 2**64 - 1, "training seed")
    require(config["solver"]["seed"] == seed, "case training seed")
    anchor = tomllib.loads((run / "config-seed0.toml").read_text(encoding="utf-8-sig"))
    config_comparable = {**config, "solver": {k: v for k, v in config["solver"].items() if k != "seed"}}
    anchor_comparable = {**anchor, "solver": {k: v for k, v in anchor["solver"].items() if k != "seed"}}
    require(config_comparable == anchor_comparable, "only declared training seed may change in config")
    return path, checksum, config


def reference_model_checks(raw, case, references):
    for _, reference in references:
        require(raw["abstractionFingerprint"] == reference["abstractionFingerprint"]
                and raw["solverStateVersion"] == reference["solverStateVersion"], "retained abstraction/state identity")
        reference_config = tomllib.loads(Path(reference["config"]).read_text(encoding="utf-8-sig"))
        # The configuration fingerprint includes the training seed. A future
        # cohort wrapper checks exact fingerprints within its same-seed pairs;
        # the seed-zero pilot still compares exactly with all four references.
        if case["trainingSeed"] == reference_config["solver"]["seed"]:
            require(raw["configurationFingerprint"] == reference["configurationFingerprint"], "same-seed reference model fingerprint")


def summarize_case(run, exp, case, references):
    folder, job_path = run / case["name"], run / (case["name"] + "-job.json")
    config_path, config_hash, config = case_config(run, exp, case)
    raw, measurement, job = read(folder / "stdout.json"), read(folder / "measurement.json"), read(job_path)
    normalized_only(raw)
    require(measurement["schemaVersion"] == "solvers.average-sampling-measurement/v1"
            and measurement["exitCode"] == 0 and measurement["timedOut"] is False, "successful AverageSampling measurement")
    require(measurement["arguments"] == job["arguments"] and sha(job_path) == measurement["jobSha256"]
            and Path(measurement["job"]).resolve() == job_path.resolve(), "literal measured job")
    require(sha(folder / "stdout.json") == measurement["stdoutSha256"], "raw output hash")
    require(Path(measurement["binary"]).resolve() == (run / "research.exe").resolve()
            and measurement["binarySha256"] == exp["binarySha256"], "measured frozen binary")
    for field in ("sourceManifestSha256", "validationReport"):
        require(measurement[field] == job[field] == exp[field], "measurement identity: " + field)
    require(measurement["configSha256"] == job["configSha256"] == config_hash, "measured case config hash")
    require(measurement["sourceRevision"] == exp["baseRevision"]
            and measurement["timeoutSeconds"] == job["timeoutSeconds"] == case["timeoutSeconds"], "source revision/timeout")
    opts = options(job["arguments"])
    expected = {"--variant": case["variant"], "--sweeps": str(case["sweeps"]), "--threads": "8",
                "--memory": "8GiB", "--source-revision": exp["sourceManifestSha256"],
                "--evaluation-seeds": "101,202", "--evaluation-samples": "128", "--coverage-samples": "131072",
                "--endpoint-fit-samples": "65536", "--endpoint-fit-seed": "602", "--endpoint-samples": "131072",
                "--endpoint-seeds": "702,703", "--endpoint-min-fit-ess": "64", "--root-samples": "262144", "--root-seeds": "801,802"}
    for flag, value in expected.items():
        require(opts.get(flag) == [value], "literal schedule: " + flag)
    for flag, field in [("--node", "nodes"), ("--support-node", "nodes"),
                        ("--coverage-prefix", "coveragePrefixes"), ("--endpoint-prefix", "endpointPaths")]:
        require(opts.get(flag) == exp[field], "literal path schedule: " + flag)
    require(set(opts) == set(expected) | {"--config", "--cache-dir", "--node", "--support-node", "--coverage-prefix", "--endpoint-prefix"}, "no undeclared flags/artifacts")
    require(config_path.resolve() == Path(opts["--config"][0]).resolve() == Path(raw["config"]).resolve(), "actual config location")
    require(config["solver"]["seed"] == case["trainingSeed"]
            and config["solver"]["opponent_exploration"] == 0, "unchanged regret parameters")
    require(config["game"]["information"]["recall"] == "current-street"
            and config["game"]["abstraction"]["kind"] == "ehs2-percentile"
            and config["game"]["abstraction"]["buckets"] == {s: 32 for s in STREETS[1:]}, "fixed current-street K32 abstraction")
    require(raw["schemaVersion"] == "solvers.multiway-average-sampling-research/v1"
            and raw["sourceRevision"] == exp["sourceManifestSha256"] and raw["threads"] == 8, "output schema/source/threads")
    for field in ("executableBlake3", "effectiveConfigBlake3", "configurationFingerprint", "abstractionFingerprint"):
        digest(raw[field])
    reference_model_checks(raw, case, references)
    result, diagnostic = raw["result"], raw["diagnostics"]
    require(result["variant"] == case["variant"] and result["threads"] == 8, "reported variant/threads")
    digest(result["current_regret_fingerprint"])
    metrics = result["metrics"]
    require(metrics["sweeps"] == case["sweeps"] and metrics["traversals"] == case["sweeps"] * 6,
            "complete fixed sweeps")
    for key in ("infosets", "memory_bytes", "total_deal_attempts", "hand_updates"):
        count(metrics[key], 2**64 - 1, "training metric")
    require(metrics["total_deal_attempts"] >= metrics["traversals"], "training deals")
    require(math.isclose(metrics["mean_deal_attempts"], metrics["total_deal_attempts"] / metrics["traversals"], rel_tol=1e-12), "deal mean")
    require(len(metrics["average_positive_regret"]) == 6 and all(math.isfinite(v) and v >= 0 for v in metrics["average_positive_regret"]), "regret diagnostic seats")
    for value in (result["solve_elapsed_secs"], raw["constructionElapsedSecs"], raw["elapsedSecs"], diagnostic["elapsed_secs"], measurement["wallSeconds"]):
        number(value, "phase clock", positive=True)
    require(result["solve_elapsed_secs"] + diagnostic["elapsed_secs"] <= raw["elapsedSecs"] + 0.01
            and raw["elapsedSecs"] + raw["constructionElapsedSecs"] <= measurement["wallSeconds"] + 0.01, "clock phase containment")
    number(measurement["observedPeakWorkingSetBytes"], "peak memory", positive=True)
    supports = diagnostic["support"]
    require(raw["supportNodes"] == raw["nodes"] and len(supports) == len(raw["nodes"]) == 16, "all requested support nodes")
    require([c["requested"] for c in raw["nodes"]] == exp["nodes"], "node request order")
    require(len({c["history"] for c in raw["nodes"]}) == 16, "unique public histories")
    support_summary, by_path = [], {}
    for context, support in zip(raw["nodes"], supports, strict=True):
        context_checks(context, support)
        by_path[context["requested"]] = support
        support_summary.append(dict(context=context, **public_layout(support), **support_checks(support)))
    require(len(result["histories"]) == len(supports), "history export cardinality")
    for history, support in zip(result["histories"], supports, strict=True):
        require(history["history"] == support["history"], "history export identity")
        stored = [row for row in support["rows"] if row["regrets"] is not None]
        require(len(history["strategies"]) == len(stored), "stored history row cardinality")
        for row, reference in zip(history["strategies"], stored, strict=True):
            require(row["key"] == reference["key"], "history/support key order")
            average = reference["average_strategy"]
            require(row["status"] == ("average-observed" if average is not None else "zero-average-mass-omitted"), "history average status")
            require(row["actions"] == (None if average is None else [dict(action=a, probability=p)
                    for a, p in zip(support["action_labels"], average, strict=True)]), "history normalized average agreement")
    endpoint_supports = [by_path[path] for path in exp["endpointPaths"]]
    endpoint_contexts = [raw["nodes"][exp["nodes"].index(path)] for path in exp["endpointPaths"]]
    require(raw["endpointPrefixes"] == endpoint_contexts and len(diagnostic["endpoints"]) == 3, "three endpoint contexts")
    ds = exp["endpointSchedule"]
    require(diagnostic["config"] == dict(support_paths=[s["action_indices"] for s in supports],
            endpoint_paths=[s["action_indices"] for s in endpoint_supports],
            endpoint=dict(fit_samples=ds["fitSamples"], fit_seed=ds["fitSeed"], held_out_samples=ds["heldOutSamples"],
                          held_out_seeds=ds["heldOutSeeds"], min_fit_ess=ds["minFitEss"]),
            root_samples=exp["rootSchedule"]["samples"], root_seeds=exp["rootSchedule"]["seeds"]), "actual diagnostic budgets/paths")
    endpoint_summary = []
    for value, support, context in zip(diagnostic["endpoints"], endpoint_supports, endpoint_contexts, strict=True):
        selection = endpoint_checks(value, ds, support)
        require(hex_bytes(value["configuration_fingerprint"], 32) == raw["configurationFingerprint"]
                and hex_bytes(value["abstraction_fingerprint"], 32) == raw["abstractionFingerprint"], "endpoint model identity")
        anchors = [ref["endpointDeviation"]["result"] for _, ref in references
                   if "endpointDeviation" in ref and ref["endpointDeviation"]["result"]["history"] == value["history"]]
        require(len(anchors) == 1, "unique endpoint proposal reference")
        anchor = anchors[0]["proposal"]
        balanced.proposal_checks(value["proposal"], value,
            dict(preflopActionCount=len(anchor["preflop_actions"]), actionIndices=support["action_indices"],
                 preflopHistory=hex_bytes(anchor["preflop_history"])), anchor["root_range_fingerprint"])
        endpoint_summary.append(dict(context=context, fitSelection=selection, result=value))
    require([r["seed"] for r in diagnostic["root_evaluations"]] == exp["rootSchedule"]["seeds"], "root seeds/cardinality")
    for root in diagnostic["root_evaluations"]:
        n = exp["rootSchedule"]["samples"]
        require(root["samples"] == n and root["total_deal_attempts"] >= n and len(root["prefixes"]) == 3, "root budget/cardinality")
        for prefix, support in zip(root["prefixes"], endpoint_supports, strict=True):
            require(prefix["history"] == support["history"] and prefix["action_indices"] == support["action_indices"], "root prefix identity")
            require("relative_weight_mean" not in prefix and len(prefix["seats"]) == 6, "absolute root reach field")
            source_checks({**prefix, "relative_weight_mean": prefix["reach_probability"]}, n)
            estimate_checks(prefix["reach_probability"], "root reach", probability=True)
    ordinary = exp["ordinarySchedule"]
    require([e["seed"] for e in result["evaluations"]] == ordinary["seeds"]
            and [e["seed"] for e in result["coverage_evaluations"]] == ordinary["seeds"], "ordinary seed/cardinality")
    coverage_contexts = raw["coveragePrefixes"]
    require([c["requested"] for c in coverage_contexts] == exp["coveragePrefixes"], "coverage context order")
    for context in coverage_contexts:
        require(context == raw["nodes"][exp["nodes"].index(context["requested"])] , "coverage/support context")
    for entry in result["evaluations"]:
        profile_checks(entry["result"], ordinary["samples"], False)
    coverage_summary = []
    for entry in result["coverage_evaluations"]:
        value = entry["result"]
        profile_checks(value["evaluation"], ordinary["coverageSamples"], True)
        require(len(value["prefixes"]) == len(coverage_contexts), "coverage prefix cardinality")
        for prefix, context in zip(value["prefixes"], coverage_contexts, strict=True):
            require(hex_bytes(prefix["history"]) == context["history"], "coverage history")
            reached = count(prefix["reached_samples"], ordinary["coverageSamples"], "reached worlds")
            require(set(prefix["trajectory_visits_by_street"]) == set(STREETS), "trajectory street schema")
            for visits in prefix["trajectory_visits_by_street"].values():
                count(visits, reached, "trajectory count")
            totals = seat_coverage(prefix["candidate_policy_coverage"])
            for street, visits in prefix["trajectory_visits_by_street"].items():
                decisions = totals[street]["decision"]
                require(decisions >= visits and (decisions > 0) == (visits > 0), "trajectory/decision consistency")
                if STREETS.index(street) < STREETS.index(context["street"]):
                    require(decisions == visits == 0, "coverage cannot precede prefix street")
            require(prefix["trajectory_visits_by_street"][context["street"]] == reached, "endpoint street reached trajectory count")
            if reached == 0:
                require(all(v["decision"] == 0 for v in totals.values()), "unreached coverage has no visits")
            if context["requested"] == "root":
                require(reached == ordinary["coverageSamples"]
                        and prefix["candidate_policy_coverage"] == value["evaluation"]["candidate_policy_coverage"], "root ordinary coverage equality")
            coverage_summary.append(dict(seed=entry["seed"], context=context, samples=ordinary["coverageSamples"],
                reachedSamples=reached, trajectoryVisitsByStreet=prefix["trajectory_visits_by_street"],
                sourceVisitsByStreet=totals,
                averageFractionByStreet={s: totals[s]["average_strategy"] / totals[s]["decision"]
                                         if totals[s]["decision"] else None for s in STREETS}))
    return dict(case=case["name"], variant=case["variant"], trainingSeed=case["trainingSeed"], sweeps=case["sweeps"],
                measurement=measurement, constructionSeconds=raw["constructionElapsedSecs"],
                solveSeconds=result["solve_elapsed_secs"], diagnosticsSeconds=diagnostic["elapsed_secs"],
                currentRegretFingerprint=result["current_regret_fingerprint"], support=support_summary,
                endpoints=endpoint_summary, rootEvaluations=diagnostic["root_evaluations"],
                ordinaryEvaluations=result["evaluations"], coverageEvaluations=result["coverage_evaluations"],
                coverageSummary=coverage_summary), raw


def pair_checks(left, right):
    for name in ("executableBlake3", "effectiveConfigBlake3", "configurationFingerprint", "abstractionFingerprint",
                 "solverStateVersion", "sourceRevision", "nodes", "coveragePrefixes", "supportNodes", "endpointPrefixes"):
        require(left[name] == right[name], "paired identity: " + name)
    a, b = left["result"], right["result"]
    require(a["current_regret_fingerprint"] == b["current_regret_fingerprint"], "paired regret fingerprint")
    # touched/infosets can change when average-only walks expose zero columns.
    # Average-positive-regret uses traversal count, not touched count, as its
    # denominator and must therefore remain exactly equal as well.
    for field in ("sweeps", "traversals", "total_deal_attempts", "mean_deal_attempts", "hand_updates",
                  "memory_bytes", "average_positive_regret"):
        require(a["metrics"][field] == b["metrics"][field], "paired learning counter: " + field)
    require(len(left["diagnostics"]["support"]) == len(right["diagnostics"]["support"]), "paired support cardinality")
    for x, y in zip(left["diagnostics"]["support"], right["diagnostics"]["support"], strict=True):
        require(public_layout(x) == public_layout(y), "paired public support layout")
        require(numeric_regrets(x) == numeric_regrets(y), "paired numeric regret vectors")


def baseline_replay_checks(raw, references):
    endpoint_matches, root_matches, support_matches = [], [], []
    actual_endpoints = {hex_bytes(r["history"]): r for r in raw["diagnostics"]["endpoints"]}
    actual_support = {hex_bytes(r["history"]): r for r in raw["diagnostics"]["support"]}
    actual_roots = {(r["seed"], r["samples"], hex_bytes(p["history"])): (r, p)
                    for r in raw["diagnostics"]["root_evaluations"] for p in r["prefixes"]}
    for path, reference in references:
        endpoint = reference.get("endpointDeviation")
        if endpoint:
            value = endpoint["result"]
            history = hex_bytes(value["history"])
            require(without_endpoint_clocks(actual_endpoints[history]) == without_endpoint_clocks(value), "UniformOne endpoint numerical replay")
            endpoint_matches.append(dict(reference=path, history=history))
        for entry in reference.get("conditionalEvaluations", []):
            for prefix in entry["result"]["prefixes"]:
                key = entry["seed"], entry["result"]["samples"], hex_bytes(prefix["history"])
                if key in actual_roots:
                    result, actual = actual_roots[key]
                    require(actual == prefix and result["total_deal_attempts"] == entry["result"]["total_deal_attempts"], "UniformOne root prefix numerical replay")
                    root_matches.append(dict(reference=path, seed=key[0], history=key[2]))
        for support in reference.get("policySupport", []):
            history = support["context"]["history"]
            if history in actual_support:
                actual = actual_support[history]
                context = support["context"]
                require(actual["actor"] == context["actor"] and actual["street"] == context["street"]
                        and actual["active_opponents"] == context["activeOpponents"]
                        and actual["action_indices"] == context["actionIndices"]
                        and actual["bucket_active_opponents"] == support["bucketActiveOpponents"]
                        and actual["expected_buckets"] == support["expectedBuckets"], "reference support full context")
                require(actual["action_labels"] == support["actionLabels"] and len(actual["rows"]) == len(support["rows"]), "reference support menu/denominator")
                for bucket, (row, old) in enumerate(zip(actual["rows"], support["rows"], strict=True)):
                    require(old["bucket"] == bucket and row["regrets"] == old["regrets"]
                            and row["average_strategy"] == old["averageStrategy"], "UniformOne raw support replay")
                support_matches.append(dict(reference=path, history=history))
    require(len(endpoint_matches) == 3 and len({m["history"] for m in endpoint_matches}) == 3, "three exact endpoint replays")
    require(len(root_matches) == 4, "four exact HU root prefix replays")
    require(len({m["history"] for m in support_matches}) == 13, "thirteen retained raw support nodes")
    return dict(endpointMatches=endpoint_matches, rootPrefixMatches=root_matches, supportMatches=support_matches,
                endpointNonTimingFieldsExactlyMatch=True, rootPrefixFieldsExactlyMatch=True)


def summarize(run):
    exp = read(run / "experiment.json")
    schedule_checks(exp)
    verification, source_count = evidence_checks(run, exp)
    references = references_checks(exp)
    outputs = [summarize_case(run, exp, case, references) for case in exp["cases"]]
    summaries, raw = zip(*outputs, strict=True)
    pair_checks(*raw)
    arguments = [options(case["measurement"]["arguments"]) for case in summaries]
    require({k: v for k, v in arguments[0].items() if k != "--variant"}
            == {k: v for k, v in arguments[1].items() if k != "--variant"}, "only paired proposal argument differs")
    first = summaries[0]["measurement"]
    second = summaries[1]["measurement"]
    require(datetime.fromisoformat(first["startedUtc"]) + timedelta(seconds=first["wallSeconds"])
            <= datetime.fromisoformat(second["startedUtc"]), "serial non-overlapping pilot measurements")
    replay = baseline_replay_checks(raw[0], references)
    ratio = summaries[1]["solveSeconds"] / summaries[0]["solveSeconds"]
    return dict(schemaVersion="solvers.average-continuation-summary/v1", status="completed-pilot",
                experiment=exp, experimentSha256=sha(run / "experiment.json"), verification=verification,
                sourceFiles=source_count, cases=list(summaries), baselineReplay=replay,
                fixedSweepLearningIdentity=dict(regretFingerprintEqual=True, numericSupportRegretsEqual=True,
                    missingVersusStoredZeroMayDiffer=True),
                pilotGate=dict(driverCostRatio=ratio, maximumDriverCostRatio=2.0, passed=ratio <= 2.0,
                               policyPromotion=False, completedTrainingSeeds=[0],
                               remainingFixedSeeds=[11, 29], calibratedControlsCompleted=False,
                               cohortStatus="deferred-to-prioritize-preflop-quality"),
                interpretation="Pilot only. More average support is not regret learning or equilibrium quality. Endpoint gains retain signed held-out estimates and all prefix weights; different profiles induce different conditional populations, so matching seeds do not pair their proposals. The predeclared postflop cohort and calibrated controls have not run and are deferred because the user prioritized preflop solution quality, especially deep 3bet/4bet/5bet decisions.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    text = json.dumps(summarize(args.run), ensure_ascii=False, indent=2, sort_keys=True, allow_nan=False) + "\n"
    if args.output:
        args.output.write_text(text, encoding="utf-8", newline="\n")
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
