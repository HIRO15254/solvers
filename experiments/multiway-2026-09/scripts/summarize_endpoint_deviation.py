"""Validate fixed-fit, held-out one-step endpoint-deviation evidence."""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

import summarize_preflop_proposal as dependency
from summarize_preflop_proposal import (
    argument, estimate_checks, read, require, sha, source_checks, validate_evidence,
)


def sampling_checks(sampling, samples, seed, players):
    require(sampling["samples"] == samples and sampling["seed"] == seed, "sampling schedule")
    require(sampling["total_deal_attempts"] >= samples, "joint deal attempts")
    require(len(sampling["baseline_seats"]) == players, "baseline seat count")
    source_checks({**sampling, "seats": sampling["baseline_seats"]}, samples)
    require(sampling["terminal_replays"] >= sampling["positive_weight_samples"], "terminal replay count")


def endpoint_checks(result, schedule, context):
    config = result["config"]
    require(config == dict(fit_samples=schedule["fitSamples"], fit_seed=schedule["fitSeed"],
                           held_out_samples=schedule["heldOutSamples"],
                           held_out_seeds=schedule["heldOutSeeds"], min_fit_ess=64.0), "fixed fit configuration")
    require(config["fit_seed"] not in config["held_out_seeds"]
            and len(set(config["held_out_seeds"])) == len(config["held_out_seeds"]), "independent seed schedule")
    require(result["variant"] == dict(purify_threshold=0.0, use_current_strategy=False), "frozen average profile")
    require(bytes(result["history"]).hex() == context["history"]
            and result["action_indices"] == context["actionIndices"]
            and result["actor"] == context["actor"]
            and result["street"] == context["street"]
            and result["active_opponents"] == context["activeOpponents"], "endpoint identity")
    require(context["street"] == "river" and context["actor"] == 3
            and context["activeOpponents"] == result["bucket_active_opponents"] == 2, "actual three-player endpoint")
    require(context["history"] == "86599afe4e294e3e6fcd0d7b5a993b0a", "selected public endpoint")
    require(result["expected_buckets"] == 32 and result["action_labels"] ==
            ["check", "bet-to:10500", "bet-to:93500:all-in"], "complete endpoint menu/denominator")
    actions = len(result["action_labels"])
    fit = result["fit"]
    sampling_checks(fit["sampling"], config["fit_samples"], config["fit_seed"], 6)
    require(fit["sampling"]["terminal_replays"] == (actions + 1) * fit["sampling"]["positive_weight_samples"], "fit enumerates every legal endpoint action")
    rows = fit["rows"]
    require(len(rows) == result["expected_buckets"], "complete fitted bucket rows")
    sums, positives, retained = [], 0, 0
    for bucket, row in enumerate(rows):
        require(row["baseline_source"] == "uniform-fallback", "frozen missing-column endpoint source")
        key = row["key"]
        require(key == dict(history=result["history"], player=result["actor"], street=3,
                            active_opponents=2, bucket_path=[4294967295] * 3 + [bucket]), "own-information key")
        count, weight = row["positive_weight_samples"], row["relative_weight_sum"]
        require(isinstance(count, int) and 0 <= count <= config["fit_samples"], "fit row count")
        require(math.isfinite(weight) and weight >= 0 and (weight > 0) == (count > 0), "fit row weight")
        require(0 <= row["effective_sample_size"] <= count + 1e-8
                and 0 <= row["max_normalized_weight"] <= 1 + 1e-12, "fit row concentration")
        estimates = row["action_gains"]
        require(len(estimates) == actions and all((e is not None) == (count >= 2) for e in estimates), "fit action dimensions/denominators")
        for estimate in estimates:
            if estimate is not None:
                estimate_checks(estimate, "fit gain")
        selected = row["selected_action"]
        expected = None
        if count and row["effective_sample_size"] >= config["min_fit_ess"]:
            best = max(range(actions), key=lambda action: estimates[action]["mean"])
            if estimates[best]["mean"] > 0:
                expected = best
        require(selected == expected, "fit-only retention and deterministic first maximum")
        retained += selected is not None
        positives += count
        sums.append(weight)
    require(positives == fit["sampling"]["positive_weight_samples"], "fit key count partition")
    require(math.isclose(sum(sums), config["fit_samples"] * fit["sampling"]["relative_weight_mean"]["mean"], rel_tol=1e-11), "fit key weight partition")
    require(retained == fit["retained_buckets"], "retained key denominator")
    require(len(result["held_out"]) == len(config["held_out_seeds"]), "held-out cardinality")
    for held_out, seed in zip(result["held_out"], config["held_out_seeds"], strict=True):
        sampling = held_out["sampling"]
        sampling_checks(sampling, config["held_out_samples"], seed, 6)
        positive = sampling["positive_weight_samples"]
        require(positive <= sampling["terminal_replays"] <= positive * 2, "held-out baseline and supported candidate replay counts")
        for key in ("gain", "retained_key_weight_fraction"):
            require((held_out[key] is not None) == (positive > 0), "all-prefix denominator")
            if held_out[key] is not None:
                estimate_checks(held_out[key], key, probability=key != "gain")
        candidate_replays = sampling["terminal_replays"] - positive
        if not retained:
            require(candidate_replays == 0, "no candidate outside frozen fit support")
        if candidate_replays == 0 and positive:
            require(held_out["gain"] == dict(mean=0.0, stderr=0.0)
                    and held_out["retained_key_weight_fraction"] == dict(mean=0.0, stderr=0.0), "unsupported baseline retained")
        elif positive:
            fraction = held_out["retained_key_weight_fraction"]["mean"]
            require(fraction > 0, "candidate replay has positive prefix weight")
            if candidate_replays == positive:
                require(math.isclose(fraction, 1.0, abs_tol=1e-12), "full candidate replay weight coverage")
        require(math.isfinite(held_out["elapsed_secs"]) and held_out["elapsed_secs"] > 0, "held-out clock")
    require(math.isfinite(result["fit_elapsed_secs"]) and result["fit_elapsed_secs"] > 0, "fit clock")


def summarize(run):
    exp = read(run / "experiment.json")
    verification, source_files = validate_evidence(run, exp)
    for key, file in [("summarizerSha256", Path(__file__)),
                      ("summaryDependencySha256", Path(dependency.__file__)),
                      ("runnerSha256", Path(__file__).with_name("run_average_sampling_measurement.ps1"))]:
        require(sha(file) == exp[key], "analysis/runner identity: " + key)
    require(sha(exp["checkpoint"]) == exp["checkpointSha256"], "checkpoint identity")
    require(sha(exp["rootReachReference"]) == exp["rootReachReferenceSha256"], "root-reach reference identity")
    reference = read(exp["rootReachReference"])
    require(sha(reference["config"]) == exp["configSha256"]
            and Path(reference["checkpoint"]).resolve() == Path(exp["checkpoint"]).resolve(), "root reference model/checkpoint")
    results = []
    shared_identity = shared_context = shared_proposal = None
    for case in ("pilot", "main"):
        schedule = exp[case]
        require(schedule["timeoutSeconds"] == 900, "external timeout")
        folder = run / case
        job_path = run / (case + "-job.json")
        job, measure, data = read(job_path), read(folder / "measurement.json"), read(folder / "stdout.json")
        args = job["arguments"]
        require(measure["schemaVersion"] == "solvers.checkpoint-audit-measurement/v1"
                and measure["exitCode"] == 0 and not measure["timedOut"], "completed measurement")
        require(sha(job_path) == measure["jobSha256"] and measure["arguments"] == args, "literal job")
        require(Path(measure["job"]).resolve() == job_path.resolve(), "measured job path")
        require(sha(folder / "stdout.json") == measure["stdoutSha256"], "raw output identity")
        require(sha(measure["binary"]) == measure["binarySha256"] == exp["binarySha256"], "measured binary")
        require(measure["sourceManifestSha256"] == job["sourceManifestSha256"] == exp["sourceManifestSha256"], "measured source")
        require(measure["timeoutSeconds"] == job["timeoutSeconds"] == schedule["timeoutSeconds"], "literal timeout")
        require(measure["sourceRevision"] == exp["baseRevision"], "measured revision")
        require(measure["validationReport"] == job["validationReport"] == exp["validationReport"], "report mapping")
        require(sha(argument(args, "--config")) == sha(data["config"]) == job["configSha256"]
                == measure["configSha256"] == exp["configSha256"], "measured config")
        require(Path(argument(args, "--checkpoint")).resolve() == Path(data["checkpoint"]).resolve()
                == Path(exp["checkpoint"]).resolve(), "checkpoint path")
        require("freshTraining" not in data and "--fresh-sweeps" not in args, "read-only frozen state")
        require(data["schemaVersion"] == "solvers.multiway-checkpoint-audit/v1", "audit schema")
        require(data["sweeps"] == 32768 and data["solverStateVersion"] == 4, "frozen progress/state")
        require(argument(args, "--samples") == "128" and data["evaluationSamplesPerSeed"] == 128
                and argument(args, "--br-traversals") == "1" and data["deviatorTraining"]["traversalsPerSeat"] == 1,
                "incidental candidate budgets")
        require(argument(args, "--evaluation-seeds") == "101,202" and data["evaluationSeeds"] == [101, 202]
                and [row["seed"] for row in data["evaluations"]] == [101, 202], "incidental candidate seeds")
        require(argument(args, "--node-frequency-samples") == "0"
                and all(node["frequency"] is None for node in data["nodes"]), "no incidental frequency sampling")
        identity = {key: data[key] for key in ("configurationFingerprint", "abstractionFingerprint", "policyArena")}
        require(shared_identity is None or shared_identity == identity, "same frozen model")
        shared_identity = identity
        require(all(data[key] == reference[key] for key in ("sweeps", "solverStateVersion", "configurationFingerprint", "abstractionFingerprint")), "root reference profile identity")
        for flag, value in [("--endpoint-prefix", exp["endpoint"]), ("--endpoint-fit-samples", str(schedule["fitSamples"])),
                            ("--endpoint-fit-seed", str(schedule["fitSeed"])), ("--endpoint-samples", str(schedule["heldOutSamples"])),
                            ("--endpoint-seeds", ",".join(map(str, schedule["heldOutSeeds"]))),
                            ("--endpoint-min-fit-ess", "64"), ("--threads", "8"), ("--memory", "8GiB")]:
            require(argument(args, flag) == value, "literal endpoint schedule: " + flag)
        endpoint = data["endpointDeviation"]
        require(endpoint["context"]["requested"] == exp["endpoint"], "requested endpoint")
        require(shared_context is None or shared_context == endpoint["context"], "stable public context")
        shared_context = endpoint["context"]
        result = endpoint["result"]
        require(argument(args, "--support-node") == exp["endpoint"] and len(data["policySupport"]) == 1, "raw endpoint support schedule")
        support = data["policySupport"][0]
        require(support["context"] == endpoint["context"] and support["expectedBuckets"] == result["expected_buckets"]
                and support["actionLabels"] == result["action_labels"] and support["storedBuckets"] == 0,
                "raw endpoint support identity")
        require(len(support["rows"]) == support["expectedBuckets"] and all(row["status"] == "missing" for row in support["rows"]), "missing source evidence")
        require(bytes(result["configuration_fingerprint"]).hex() == data["configurationFingerprint"]
                and bytes(result["abstraction_fingerprint"]).hex() == data["abstractionFingerprint"], "endpoint baseline identity")
        require(shared_proposal is None or shared_proposal == result["proposal"], "frozen proposal identity")
        shared_proposal = result["proposal"]
        proposal = result["proposal"]
        require(proposal["preflop_actions"] == result["action_indices"][:7]
                and bytes(proposal["preflop_history"]).hex() == "4151089d6b7f0776d6f34d48604f54da", "three-player preflop proposal trunk")
        require(len(proposal["root_range_fingerprint"]) == len(proposal["proposal_range_fingerprint"]) == 32
                and proposal["proposal_floor_fraction"] == 1e-7, "corrected proposal identity")
        require(0 < proposal["pilot_accepted"] <= proposal["pilot_samples"], "proposal admission")
        for name in ("positive_target_combos_by_seat", "floor_adjusted_combos_by_seat", "target_scale_by_seat"):
            require(len(proposal[name]) == 6, "all seats retain blockers")
        endpoint_checks(result, schedule, endpoint["context"])
        require(endpoint["elapsedSecs"] >= result["fit_elapsed_secs"] + sum(row["elapsed_secs"] for row in result["held_out"]), "phase clocks")
        require(measure["wallSeconds"] >= data["constructionElapsedSecs"] + endpoint["elapsedSecs"], "whole-process clock")
        results.append(dict(case=case, measurement=measure, constructionElapsedSecs=data["constructionElapsedSecs"], endpoint=endpoint))
    root_reach = []
    for entry in reference["conditionalEvaluations"]:
        require(entry["seed"] == entry["result"]["seed"], "root reference seed")
        for context, prefix in zip(entry["prefixes"], entry["result"]["prefixes"], strict=True):
            if context["history"] != shared_context["history"]:
                continue
            require(context == shared_context, "root reference public context")
            source_checks({**prefix, "relative_weight_mean": prefix["reach_probability"]}, entry["result"]["samples"])
            ordinary = next(c for c in reference["coverageEvaluations"] if c["seed"] == entry["seed"])
            observed = next(p for p in ordinary["prefixes"] if p["history"] == context["history"])
            root_reach.append(dict(seed=entry["seed"], samples=entry["result"]["samples"],
                                   reachProbability=prefix["reach_probability"], effectiveSampleSize=prefix["effective_sample_size"],
                                   maxNormalizedWeight=prefix["max_normalized_weight"],
                                   ordinarySamples=ordinary["result"]["samples"], ordinaryReachedSamples=observed["reachedSamples"]))
    require([row["seed"] for row in root_reach] == [303, 404]
            and all(row["samples"] == row["ordinarySamples"] == 1048576 for row in root_reach), "separate root reach schedule")
    return dict(schemaVersion="solvers.endpoint-deviation-evidence/v1", status="complete",
                sourceManifestSha256=exp["sourceManifestSha256"], sourceFiles=source_files,
                binarySha256=exp["binarySha256"], checkpointSha256=exp["checkpointSha256"],
                summarizerSha256=exp["summarizerSha256"], summaryDependencySha256=exp["summaryDependencySha256"],
                verification=verification, cases=results, rootReachReference=exp["rootReachReference"],
                rootReachReferenceSha256=exp["rootReachReferenceSha256"], rootReachContext=root_reach,
                interpretation="One frozen baseline, one endpoint-only deviation table fitted independently of held-out worlds. Signed conditional gain uses all prefix weight. Low retained coverage cannot establish a strong baseline. No global exploitability or balanced strategy improvement is claimed.")


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
