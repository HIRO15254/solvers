"""Validate frozen-profile HU endpoint diagnostics beside the three-player reference.

Conditional gain, absolute root reach and policy-source coverage remain separate.
Matching numeric seeds across different endpoint proposals do not imply pairing.
"""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

import summarize_endpoint_deviation as endpoint_dependency
import summarize_opponent_exploration as support_dependency
import summarize_preflop_proposal as evidence_dependency
from summarize_endpoint_deviation import sampling_checks
from summarize_opponent_exploration import support_checks
from summarize_preflop_proposal import (
    argument, argument_values, estimate_checks, read, require, sha, source_checks,
    validate_evidence,
)


def finite_count(value, maximum, label):
    require(type(value) is int and 0 <= value <= maximum, label)


def public_context(case):
    return dict(requested=case["endpoint"], history=case["history"],
                actionIndices=case["actionIndices"],
                actionLabels=case["endpoint"].split("/"), actor=case["actor"],
                street="river", activeOpponents=case["activeOpponents"])


def endpoint_checks(result, schedule, case, support, min_ess):
    """Check the complete frozen fit table, not a positive-gain-only subset."""
    config = result["config"]
    require(config == dict(fit_samples=schedule["fitSamples"], fit_seed=schedule["fitSeed"],
                           held_out_samples=schedule["heldOutSamples"],
                           held_out_seeds=schedule["heldOutSeeds"], min_fit_ess=min_ess),
            "fixed fit configuration")
    require(result["variant"] == dict(purify_threshold=0.0, use_current_strategy=False),
            "frozen average profile")
    require(bytes(result["history"]).hex() == case["history"]
            and result["action_indices"] == case["actionIndices"]
            and result["actor"] == case["actor"] and result["street"] == "river"
            and result["active_opponents"] == result["bucket_active_opponents"]
            == case["activeOpponents"] == 1, "HU endpoint identity")
    require(result["expected_buckets"] == support["expectedBuckets"] == 32
            and result["action_labels"] == support["actionLabels"] == case["actionLabels"],
            "complete endpoint menu/denominator")
    require(support["context"] == public_context(case)
            and support["bucketActiveOpponents"] == result["bucket_active_opponents"],
            "raw support context")
    counts = support_checks(support)
    actions = len(case["actionLabels"])
    require(1 <= actions <= 8, "bounded legal action menu")
    fit = result["fit"]
    sampling_checks(fit["sampling"], config["fit_samples"], config["fit_seed"], 6)
    require(fit["sampling"]["terminal_replays"] == (actions + 1) * fit["sampling"]["positive_weight_samples"],
            "fit enumerates every legal endpoint action")
    rows = fit["rows"]
    require(len(rows) == result["expected_buckets"], "complete fitted rows")
    positives, weights, retained = 0, [], 0
    for bucket, (row, raw) in enumerate(zip(rows, support["rows"], strict=True)):
        expected_source = ("uniform-fallback" if raw["status"] == "missing"
                           else "average" if raw["strategyMass"] > 0 else "regret-fallback")
        require(row["baseline_source"] == expected_source, "baseline source/raw support mapping")
        require(row["key"] == dict(history=result["history"], player=case["actor"], street=3,
                                  active_opponents=case["activeOpponents"],
                                  bucket_path=[4294967295] * 3 + [bucket]), "own-information key")
        count, weight = row["positive_weight_samples"], row["relative_weight_sum"]
        finite_count(count, config["fit_samples"], "fit positive world count")
        require(math.isfinite(weight) and weight >= 0 and (weight > 0) == (count > 0),
                "fit positive weight")
        ess, maximum = row["effective_sample_size"], row["max_normalized_weight"]
        require(math.isfinite(ess) and 0 <= ess <= count + 1e-8
                and math.isfinite(maximum) and 0 <= maximum <= 1 + 1e-12,
                "fit weight concentration")
        if count == 0:
            require(ess == maximum == 0, "unobserved fit concentration")
        if count == 1:
            require(ess == maximum == 1, "singleton fit concentration")
        gains = row["action_gains"]
        require(len(gains) == actions and all((g is not None) == (count >= 2) for g in gains),
                "fit action gains use per-key positive worlds")
        for gain in gains:
            if gain is not None:
                estimate_checks(gain, "signed fit gain")
        expected_action = None
        if count >= 2 and ess >= min_ess:
            best = max(range(actions), key=lambda action: gains[action]["mean"])
            if gains[best]["mean"] > 0:
                expected_action = best
        selected = row["selected_action"]
        require(selected is None or type(selected) is int, "selected action type")
        require(selected == expected_action, "fit-only gate and first argmax")
        retained += selected is not None
        positives += count
        weights.append(weight)
    require(retained == fit["retained_buckets"], "retained key count")
    require(positives == fit["sampling"]["positive_weight_samples"], "fit count partition")
    require(math.isclose(sum(weights), config["fit_samples"] * fit["sampling"]["relative_weight_mean"]["mean"],
                         rel_tol=1e-11), "fit weight partition")
    require(len(result["held_out"]) == len(config["held_out_seeds"]), "held-out cardinality")
    for held, seed in zip(result["held_out"], config["held_out_seeds"], strict=True):
        sampling = held["sampling"]
        sampling_checks(sampling, config["held_out_samples"], seed, 6)
        positive = sampling["positive_weight_samples"]
        finite_count(sampling["terminal_replays"], 2 * positive, "held-out replay count")
        candidate_replays = sampling["terminal_replays"] - positive
        require(candidate_replays >= 0, "held-out baseline replay count")
        for field in ("gain", "retained_key_weight_fraction"):
            require((held[field] is not None) == (positive > 0), "all-prefix held-out denominator")
            if held[field] is not None:
                estimate_checks(held[field], field, probability=field != "gain")
        if not retained:
            require(candidate_replays == 0, "no candidate outside frozen fit support")
        if positive and candidate_replays == 0:
            require(held["gain"] == dict(mean=0.0, stderr=0.0)
                    and held["retained_key_weight_fraction"] == dict(mean=0.0, stderr=0.0),
                    "unsupported held-out worlds keep baseline")
        elif positive:
            fraction = held["retained_key_weight_fraction"]["mean"]
            require(fraction > 0, "candidate replay has positive weight")
            if candidate_replays == positive:
                require(math.isclose(fraction, 1.0, abs_tol=1e-12), "full candidate weight")
        require(math.isfinite(held["elapsed_secs"]) and held["elapsed_secs"] > 0, "held-out clock")
    require(math.isfinite(result["fit_elapsed_secs"]) and result["fit_elapsed_secs"] > 0, "fit clock")
    return counts


def root_checks(data, schedule, case):
    entries = data["conditionalEvaluations"]
    seeds, n = schedule["rootSeeds"], schedule["rootSamples"]
    require([entry["seed"] for entry in entries] == seeds, "root seed/cardinality schedule")
    rows = []
    for entry, seed in zip(entries, seeds, strict=True):
        result = entry["result"]
        require(result["seed"] == seed and result["samples"] == n, "root world budget")
        require(result["total_deal_attempts"] >= n, "root deal attempts")
        require(entry["prefixes"] == [public_context(case)] and len(result["prefixes"]) == 1,
                "root context/cardinality")
        prefix = result["prefixes"][0]
        require(bytes(prefix["history"]).hex() == case["history"]
                and prefix["action_indices"] == case["actionIndices"], "root prefix identity")
        require("relative_weight_mean" not in prefix, "absolute root reach labeling")
        require(len(prefix["seats"]) == 6, "root seat count")
        source_checks({**prefix, "relative_weight_mean": prefix["reach_probability"]}, n)
        estimate_checks(prefix["reach_probability"], "root reach", probability=True)
        rows.append(dict(seed=seed, samples=n, totalDealAttempts=result["total_deal_attempts"],
                         positiveWeightSamples=prefix["positive_weight_samples"],
                         reachProbability=prefix["reach_probability"],
                         effectiveSampleSize=prefix["effective_sample_size"],
                         maxNormalizedWeight=prefix["max_normalized_weight"]))
    return rows


def proposal_checks(proposal, result, case, root_range):
    count = case["preflopActionCount"]
    require(type(count) is int and 0 < count < len(case["actionIndices"]), "preflop trunk length")
    require(proposal["preflop_actions"] == case["actionIndices"][:count], "proposal preflop trunk")
    require(len(bytes(proposal["preflop_history"])) == 16, "proposal trunk history length")
    require(bytes(proposal["preflop_history"]).hex() == case["preflopHistory"], "proposal trunk history")
    require(proposal["root_range_fingerprint"] == root_range
            and len(bytes(proposal["proposal_range_fingerprint"])) == 32, "proposal range identity")
    require(proposal["proposal_floor_fraction"] == 1e-7
            and 0 < proposal["pilot_accepted"] <= proposal["pilot_samples"], "corrected proposal admission")
    for field in ("positive_target_combos_by_seat", "floor_adjusted_combos_by_seat", "target_scale_by_seat"):
        require(len(proposal[field]) == 6, "all seats retain blockers")
    require(all(0 <= floor <= positive <= 1326 and positive > 0 for floor, positive in zip(
        proposal["floor_adjusted_combos_by_seat"], proposal["positive_target_combos_by_seat"], strict=True)),
        "proposal support/floor counts")
    require(all(math.isfinite(scale) and scale > 0 for scale in proposal["target_scale_by_seat"]),
            "proposal target scales")
    require("reach_probability" not in result["fit"]["sampling"], "proposal weight is not absolute root reach")


def fit_selection(result, min_ess):
    """First three lists partition buckets; weights are fit-only, not held-out."""
    rows = result["fit"]["rows"]
    groups = dict(retained=[i for i, row in enumerate(rows) if row["selected_action"] is not None],
                  insufficientEss=[i for i, row in enumerate(rows) if row["effective_sample_size"] < min_ess],
                  noPositiveFitGain=[i for i, row in enumerate(rows)
                                     if row["effective_sample_size"] >= min_ess and row["selected_action"] is None])
    weight = sum(row["relative_weight_sum"] for row in rows)
    return {**{name + "Buckets": buckets for name, buckets in groups.items()},
            "fitWeightFractions": {name: sum(rows[i]["relative_weight_sum"] for i in buckets) / weight
                                   if weight > 0 else None for name, buckets in groups.items()},
            # Unobserved/singleton lists overlap the insufficient-ESS group.
            "unobservedBuckets": [i for i, row in enumerate(rows) if row["positive_weight_samples"] == 0],
            "singletonBuckets": [i for i, row in enumerate(rows) if row["positive_weight_samples"] == 1],
            "selectedActionCounts": {label: sum(row["selected_action"] == action for row in rows)
                                     for action, label in enumerate(result["action_labels"])}}


def schedule_checks(exp):
    require([case["name"] for case in exp["cases"]] == ["hu3-river", "hu4-river"], "two fixed HU cases")
    require(exp["schedule"] == dict(fitSamples=65536, fitSeed=602, heldOutSamples=131072,
                                    heldOutSeeds=[702, 703], rootSamples=262144,
                                    rootSeeds=[801, 802], timeoutSeconds=900), "predeclared sample schedule")
    require(exp["minFitEss"] == 64 and exp["threads"] == 8 and exp["memory"] == "8GiB",
            "predeclared gate/resources")
    for case in exp["cases"]:
        require(len(case["actionIndices"]) == len(case["endpoint"].split("/"))
                and math.isfinite(case["potBb"]) and case["potBb"] > 0, "public metadata")


def summarize_case(run, exp, case, reference_data, root_range):
    name, schedule = case["name"], exp["schedule"]
    job_path, folder = run / (name + "-job.json"), run / name
    job, measurement, data = read(job_path), read(folder / "measurement.json"), read(folder / "stdout.json")
    args = job["arguments"]
    require(measurement["schemaVersion"] == "solvers.checkpoint-audit-measurement/v1"
            and measurement["exitCode"] == 0 and not measurement["timedOut"], "completed measurement")
    require(measurement["arguments"] == args and sha(job_path) == measurement["jobSha256"]
            and Path(measurement["job"]).resolve() == job_path.resolve(), "literal measured job")
    require(sha(folder / "stdout.json") == measurement["stdoutSha256"], "raw output hash")
    require(Path(measurement["binary"]).resolve() == Path(exp["binary"]).resolve()
            and measurement["binarySha256"] == exp["binarySha256"], "reused measured binary")
    require(measurement["sourceManifestSha256"] == job["sourceManifestSha256"] == exp["sourceManifestSha256"]
            and measurement["sourceRevision"] == exp["baseRevision"], "measured source")
    require(measurement["timeoutSeconds"] == job["timeoutSeconds"] == schedule["timeoutSeconds"], "timeout budget")
    require(measurement["validationReport"] == job["validationReport"] == exp["validationReport"], "report mapping")
    require(Path(argument(args, "--config")).resolve() == Path(data["config"]).resolve() == Path(exp["config"]).resolve()
            and job["configSha256"] == measurement["configSha256"] == exp["configSha256"], "config identity")
    require(Path(argument(args, "--checkpoint")).resolve() == Path(data["checkpoint"]).resolve()
            == Path(exp["checkpoint"]).resolve() and job["checkpointSha256"] == exp["checkpointSha256"], "checkpoint identity")
    for flag, value in [("--endpoint-prefix", case["endpoint"]), ("--support-node", case["endpoint"]),
                        ("--condition-prefix", case["endpoint"]), ("--condition-sampler", "root"),
                        ("--endpoint-fit-samples", str(schedule["fitSamples"])),
                        ("--endpoint-fit-seed", str(schedule["fitSeed"])),
                        ("--endpoint-samples", str(schedule["heldOutSamples"])),
                        ("--endpoint-seeds", ",".join(map(str, schedule["heldOutSeeds"]))),
                        ("--endpoint-min-fit-ess", "64"), ("--condition-samples", str(schedule["rootSamples"])),
                        ("--evaluation-seeds", ",".join(map(str, schedule["rootSeeds"]))),
                        ("--threads", "8"), ("--memory", "8GiB"), ("--samples", "128"),
                        ("--br-traversals", "1"), ("--node-frequency-samples", "0")]:
        require(argument(args, flag) == value, "literal schedule: " + flag)
    require(argument_values(args, "--node") == ["root"] and len(data["nodes"]) == 1
            and data["nodes"][0]["requested"] == "root" and data["nodes"][0]["frequency"] is None,
            "incidental root node only")
    require(not any(flag in args for flag in ("--fresh-sweeps", "--coverage-prefix", "--coverage-samples"))
            and not any(field in data for field in ("freshTraining", "coverageEvaluations", "preflopConditionalEvaluations")),
            "frozen state; no extra coverage/proposal pass")
    require(data["schemaVersion"] == "solvers.multiway-checkpoint-audit/v1"
            and data["sweeps"] == 32768 and data["solverStateVersion"] == 4, "frozen audit state")
    require(data["evaluationSamplesPerSeed"] == 128 and data["deviatorTraining"]["traversalsPerSeat"] == 1
            and data["evaluationSeeds"] == schedule["rootSeeds"]
            and [row["seed"] for row in data["evaluations"]] == schedule["rootSeeds"]
            and all(row["result"]["samples"] == 128 for row in data["evaluations"]), "incidental evaluation budget")
    require(all(data[field] == reference_data[field] for field in (
        "sweeps", "solverStateVersion", "configurationFingerprint", "abstractionFingerprint", "policyArena")),
        "same reference model and policy arena")
    endpoint = data["endpointDeviation"]
    require(endpoint["context"] == public_context(case), "full endpoint public context")
    result = endpoint["result"]
    require(bytes(result["configuration_fingerprint"]).hex() == data["configurationFingerprint"]
            and bytes(result["abstraction_fingerprint"]).hex() == data["abstractionFingerprint"], "endpoint fingerprint mapping")
    require(len(data["policySupport"]) == 1, "single raw support endpoint")
    counts = endpoint_checks(result, schedule, case, data["policySupport"][0], exp["minFitEss"])
    proposal_checks(result["proposal"], result, case, root_range)
    root_reach = root_checks(data, schedule, case)
    clocks = [measurement["wallSeconds"], data["constructionElapsedSecs"], endpoint["elapsedSecs"]]
    require(all(math.isfinite(value) and value > 0 for value in clocks), "finite phase clocks")
    require(endpoint["elapsedSecs"] >= result["fit_elapsed_secs"] + sum(row["elapsed_secs"] for row in result["held_out"]),
            "endpoint timer scope")
    require(measurement["wallSeconds"] >= data["constructionElapsedSecs"] + endpoint["elapsedSecs"], "whole process scope")
    return dict(name=name, metadata=case, measurement=measurement,
                constructionElapsedSecs=data["constructionElapsedSecs"], endpoint=endpoint,
                supportCounts=counts, policySupport=data["policySupport"][0], rootReachContext=root_reach,
                fitSelection=fit_selection(result, exp["minFitEss"]))


def summarize(run):
    run = Path(run)
    exp = read(run / "experiment.json")
    schedule_checks(exp)
    parent = Path(exp["parentRun"])
    require(sha(parent / "experiment.json") == exp["parentExperimentSha256"], "parent experiment identity")
    parent_exp = read(parent / "experiment.json")
    verification, source_files = validate_evidence(parent, parent_exp)
    require(source_files == 165, "reused compiled source count")
    for field in ("baseRevision", "sourceManifestSha256", "sourceZipSha256", "verificationSha256",
                  "binarySha256", "configSha256", "checkpointSha256", "runnerSha256"):
        require(exp[field] == parent_exp[field], "parent identity: " + field)
    for field, path in (("binarySha256", exp["binary"]), ("configSha256", exp["config"]),
                        ("checkpointSha256", exp["checkpoint"]),
                        ("summarizerSha256", Path(__file__)),
                        ("runnerSha256", Path(__file__).with_name("run_average_sampling_measurement.ps1"))):
        require(sha(path) == exp[field], "current artifact identity: " + field)
    expected_dependencies = {"tools/" + Path(module.__file__).name: sha(module.__file__)
                             for module in (endpoint_dependency, support_dependency, evidence_dependency)}
    require(exp["dependencies"] == expected_dependencies, "all summary dependency identities")
    require(sha(exp["referenceSummary"]) == exp["referenceSummarySha256"], "reference summary identity")
    reference = read(exp["referenceSummary"])
    require(reference["schemaVersion"] == "solvers.endpoint-deviation-evidence/v1"
            and reference["status"] == "complete", "completed three-player reference")
    require(reference == read(parent / "summary.json"), "retained reference summary equality")
    for field in ("sourceManifestSha256", "binarySha256", "checkpointSha256"):
        require(reference[field] == exp[field], "reference identity: " + field)
    main = [case for case in reference["cases"] if case["case"] == "main"]
    require(len(main) == 1, "one three-player main reference")
    reference_data = read(parent / "main" / "stdout.json")
    require(sha(parent / "main" / "stdout.json") == main[0]["measurement"]["stdoutSha256"]
            and reference_data["endpointDeviation"] == main[0]["endpoint"], "reference raw result mapping")
    require(sha(reference_data["config"]) == exp["configSha256"]
            and Path(reference_data["checkpoint"]).resolve() == Path(exp["checkpoint"]).resolve(),
            "reference config/checkpoint")
    require(sha(reference["rootReachReference"]) == reference["rootReachReferenceSha256"], "reference root-reach evidence")
    root_range = main[0]["endpoint"]["result"]["proposal"]["root_range_fingerprint"]
    require(sha(exp["trunkIdentityReference"]) == exp["trunkIdentityReferenceSha256"], "trunk reference hash")
    trunk_reference = read(exp["trunkIdentityReference"])
    require(sha(trunk_reference["config"]) == exp["configSha256"]
            and all(trunk_reference[field] == reference_data[field] for field in (
                "sweeps", "solverStateVersion", "configurationFingerprint", "abstractionFingerprint", "policyArena")),
            "trunk reference model identity")
    for case in exp["cases"]:
        matches = [node["context"] for node in trunk_reference["policySupport"]
                   if node["context"]["history"] == case["preflopHistory"]]
        count = case["preflopActionCount"]
        require(len(matches) == 1 and matches[0]["street"] == "flop"
                and matches[0]["actionIndices"] == case["actionIndices"][:count]
                and matches[0]["requested"] == "/".join(case["endpoint"].split("/")[:count])
                and matches[0]["activeOpponents"] == case["activeOpponents"], "independent trunk context")
    cases = [summarize_case(run, exp, case, reference_data, root_range) for case in exp["cases"]]
    return dict(schemaVersion="solvers.balanced-endpoint-evidence/v1", status="complete",
                parentRun=exp["parentRun"], parentExperimentSha256=exp["parentExperimentSha256"],
                sourceManifestSha256=exp["sourceManifestSha256"], sourceFiles=source_files,
                binarySha256=exp["binarySha256"], checkpointSha256=exp["checkpointSha256"],
                configSha256=exp["configSha256"], summarizerSha256=exp["summarizerSha256"],
                dependencies=exp["dependencies"], verification=verification, cases=cases,
                trunkIdentityReference=exp["trunkIdentityReference"],
                trunkIdentityReferenceSha256=exp["trunkIdentityReferenceSha256"],
                referenceSummary=exp["referenceSummary"], referenceSummarySha256=exp["referenceSummarySha256"],
                referenceCase=main[0], referenceRootReachContext=reference["rootReachContext"],
                referenceFitSelection=fit_selection(main[0]["endpoint"]["result"], exp["minFitEss"]),
                interpretation="Each frozen endpoint table is fitted before held-out evaluation. Signed conditional gain includes every prefix weight. Root reach and source coverage are separate diagnostics. Different trunks/populations are not paired by equal seed numbers; conditional gains do not rank global strategy quality. The three-player root reference has a different sampling budget and low ESS. No policy or default is changed.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = summarize(args.run)
    output = args.output or args.run / "summary.json"
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(dict(output=str(output), cases=len(result["cases"]))))


if __name__ == "__main__":
    main()
