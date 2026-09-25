"""Validate the fresh-run opponent-exploration screen and raw policy support."""
from __future__ import annotations

import argparse
import json
import math
import struct
import tomllib
from pathlib import Path

import summarize_preflop_proposal as dependency
from summarize_preflop_proposal import (
    argument, argument_values, read, require, sha, source_checks, validate_evidence,
)


def f32(value):
    """Recover the binary f32 behind serde's shortest round-trip decimal."""
    return struct.unpack("<f", struct.pack("<f", value))[0]


def normalized_f32(values):
    total = 0.0
    for value in values:
        total = f32(total + value)
    require(math.isfinite(total), "f32 normalization overflow")
    return [f32(value / total) for value in values] if total > 0 else [f32(1 / len(values))] * len(values)


def support_checks(node):
    rows = node["rows"]
    require(isinstance(node["expectedBuckets"], int) and node["expectedBuckets"] > 0, "positive bucket denominator")
    require([r["bucket"] for r in rows] == list(range(node["expectedBuckets"])), "complete bucket denominator")
    labels = node["actionLabels"]
    require(labels and len(set(labels)) == len(labels), "support labels")
    counts = dict(storedBuckets=0, nonzeroRegretBuckets=0, positiveRegretBuckets=0,
                  averageBuckets=0, averageAndNonzeroRegretBuckets=0)
    for row in rows:
        fields = [row[k] for k in ("regrets", "strategySum", "strategyMass", "currentStrategy", "averageStrategy")]
        if row["status"] == "missing":
            require(all(v is None for v in fields), "missing support has values")
            continue
        regrets, sums, mass, current, average = fields
        require(len(regrets) == len(sums) == len(current) == len(labels), "support width")
        require(all(math.isfinite(v) for v in regrets + sums + current), "nonfinite support")
        require(all(v >= 0 for v in sums + current), "negative mass/probability")
        regrets, sums, current = ([f32(v) for v in values] for values in (regrets, sums, current))
        # Rust adds f64::from(f32) in action order. Python's parsed decimals
        # are different f64 values, and newer Python sum() may compensate.
        expected_mass = 0.0
        for value in sums:
            expected_mass += value
        require(math.isfinite(mass) and mass == expected_mass, "raw strategy mass")
        require(math.isclose(sum(current), 1, abs_tol=1e-6), "current normalization")
        require(current == normalized_f32([max(v, 0.0) for v in regrets]), "current regret mapping")
        positive = any(v > 0 for v in regrets)
        nonzero = any(v != 0 for v in regrets)
        expected = "stored-positive-regrets" if positive else "stored-nonpositive-regrets" if nonzero else "stored-zero-regrets"
        require(row["status"] == expected, "support source class")
        require((average is not None) == (mass > 0), "average fallback substitution")
        if average is not None:
            require(len(average) == len(labels) and all(math.isfinite(v) and v >= 0 for v in average)
                    and math.isclose(sum(average), 1, abs_tol=1e-6), "average normalization")
            require([f32(v) for v in average] == normalized_f32(sums), "average mass mapping")
        counts["storedBuckets"] += 1
        counts["nonzeroRegretBuckets"] += int(nonzero)
        counts["positiveRegretBuckets"] += int(positive)
        counts["averageBuckets"] += int(mass > 0)
        counts["averageAndNonzeroRegretBuckets"] += int(mass > 0 and nonzero)
    require(all(node[k] == v for k, v in counts.items()), "support counts")
    return counts


def coverage_checks(rows, players):
    require(len(rows) == players, "coverage seat cardinality")
    categories = ("decision_visits", "stored_strategy_visits", "uniform_fallback_visits",
                  "average_strategy_visits", "current_strategy_visits", "regret_fallback_visits")
    for row in rows:
        for category in categories:
            streets = row[category + "_by_street"]
            require(set(streets) == {"preflop", "flop", "turn", "river"}
                    and all(isinstance(v, int) and v >= 0 for v in streets.values()), "coverage street counts")
            require(sum(streets.values()) == row[category], "coverage street/total partition")
        for street in [None, "preflop", "flop", "turn", "river"]:
            values = [row[c] if street is None else row[c + "_by_street"][street] for c in categories]
            decisions, stored, uniform, average, current, regret = values
            require(decisions == stored + uniform and stored == average + current + regret, "coverage source partition")


def proposal_schedule(data, args, seeds, support_contexts):
    """Resolve requested groups from the independently exported public contexts."""
    paths = argument_values(args, "--condition-prefix")
    require(len(paths) == len(set(paths)) == 5, "five unique conditional paths")
    contexts = {context["requested"]: context for context in support_contexts}
    require(all(path in contexts for path in paths), "conditional path missing from support contexts")
    selected = [contexts[path] for path in paths]
    require(all(context["street"] in {"flop", "turn", "river"} for context in selected), "postflop conditional endpoints")
    trunks = [context for context in selected if context["street"] == "flop"]
    require(len(trunks) == 3, "three preflop trunks")
    groups = {}
    for context in selected:
        matches = [trunk for trunk in trunks
                   if context["actionIndices"][:len(trunk["actionIndices"])] == trunk["actionIndices"]]
        require(len(matches) == 1, "conditional trunk membership")
        trunk = matches[0]
        groups.setdefault(trunk["history"], (trunk, []))[1].append(context)
    expected = [(seed, trunk, members) for seed in seeds for trunk, members in groups.values()]
    require(len(data["preflopConditionalEvaluations"]) == len(expected), "conditional group cardinality")
    return expected


def summarize(run):
    exp = read(run / "experiment.json")
    verification, source_files = validate_evidence(run, exp)
    scripts = {"summarizerSha256": Path(__file__), "summaryDependencySha256": Path(dependency.__file__),
               "runnerSha256": Path(__file__).with_name("run_average_sampling_measurement.ps1")}
    for key, path in scripts.items():
        require(sha(path) == exp[key], "analysis/runner identity: " + key)
    require(exp["cases"] == ["pilot-seed0-eps0", "pilot-seed0-eps006", "pilot-seed0-eps025"], "pilot case schedule")
    require(exp["trainingSeed"] == 0 and exp["sweeps"] == 32768, "predeclared training budget")
    baseline_config = tomllib.loads((run / "config.toml").read_text(encoding="utf-8-sig"))
    require(baseline_config["solver"].pop("opponent_exploration") == 0, "anchor exploration")
    shared_arguments = None
    shared_solver_config = None
    shared_contexts = None
    shared_support_layout = None
    shared_arena = None
    shared_abstraction = None
    all_cases = []
    reference = read(exp["baselineConditionalReference"])
    require(sha(exp["baselineConditionalReference"]) == exp["baselineConditionalReferenceSha256"], "baseline reference hash")
    require(sha(reference["config"]) == exp["configSha256"], "baseline reference config")
    require(reference["sweeps"] == exp["sweeps"], "baseline reference sweeps")
    reference_rows = {}
    root_range_fingerprint = reference["preflopConditionalEvaluations"][0]["result"]["proposal"]["root_range_fingerprint"]
    for entry in reference["preflopConditionalEvaluations"]:
        require(entry["seed"] == entry["result"]["seed"], "reference seed identity")
        require(entry["result"]["samples"] == exp["conditionalSamplesPerSeedPerTrunk"], "reference conditional budget")
        require(entry["result"]["proposal"]["root_range_fingerprint"] == root_range_fingerprint, "reference root ranges")
        for context, prefix in zip(entry["prefixes"], entry["result"]["prefixes"], strict=True):
            key = entry["seed"], context["requested"]
            require(key not in reference_rows, "duplicate baseline reference row")
            reference_rows[key] = (context, prefix, entry["result"]["proposal"])
    for case, expected_epsilon in zip(exp["cases"], [0.0, 0.06, 0.25], strict=True):
        folder = run / case
        job_path = run / (case + "-job.json")
        job, measure, data = read(job_path), read(folder / "measurement.json"), read(folder / "stdout.json")
        args = job["arguments"]
        comparable_args = args.copy()
        comparable_args[comparable_args.index("--config") + 1] = "<case-config>"
        require(shared_arguments is None or comparable_args == shared_arguments, "same non-config arguments")
        shared_arguments = comparable_args
        require(measure["schemaVersion"] == "solvers.checkpoint-audit-measurement/v1", "measurement kind")
        require(measure["exitCode"] == 0 and not measure["timedOut"], "successful case")
        require(Path(measure["job"]).resolve() == job_path.resolve(), "measured job path")
        require(measure["timeoutSeconds"] == job["timeoutSeconds"] == 900, "timeout budget")
        require(measure["sourceRevision"] == exp["baseRevision"], "measured base revision")
        require(measure["validationReport"] == job["validationReport"] == exp["validationReport"], "report mapping")
        require(measure["arguments"] == args and sha(job_path) == measure["jobSha256"], "literal job")
        require(sha(folder / "stdout.json") == measure["stdoutSha256"], "stdout hash")
        require(sha(measure["binary"]) == measure["binarySha256"] == exp["binarySha256"], "binary hash")
        require(measure["sourceManifestSha256"] == job["sourceManifestSha256"] == exp["sourceManifestSha256"], "source hash")
        config_path = argument(args, "--config")
        require(Path(data["config"]).resolve() == Path(config_path).resolve(), "config path")
        require(sha(config_path) == job["configSha256"] == measure["configSha256"], "config hash")
        config = tomllib.loads(Path(config_path).read_text(encoding="utf-8-sig"))
        epsilon = config["solver"].pop("opponent_exploration")
        require(epsilon == job["opponentExploration"] == expected_epsilon, "exploration parameter/case mapping")
        require(config == baseline_config, "only exploration changes")
        require(config["solver"]["seed"] == job["trainingSeed"] == exp["trainingSeed"], "training seed identity")
        require(data["schemaVersion"] == reference["schemaVersion"] and data["solverStateVersion"] == reference["solverStateVersion"], "solver/output versions")
        require(data["abstractionFingerprint"] == reference["abstractionFingerprint"], "reference abstraction identity")
        train = data["freshTraining"]
        require("checkpoint" not in data and "--checkpoint" not in args, "fresh-only boundary")
        n = int(argument(args, "--fresh-sweeps"))
        require(n == exp["sweeps"] == train["requestedSweeps"] == data["sweeps"] == train["metrics"]["sweeps"], "actual sweep budget")
        require(train["solverConfig"]["exploration_epsilon"] == epsilon and train["solverConfig"]["seed"] == exp["trainingSeed"], "actual solver parameters")
        solver_config = {key: value for key, value in train["solverConfig"].items() if key != "exploration_epsilon"}
        require(shared_solver_config is None or solver_config == shared_solver_config, "same effective solver settings")
        shared_solver_config = solver_config
        require(train["solverConfig"]["traverser_vector"] and not train["solverConfig"]["prune"]
                and train["solverConfig"]["discount_until"] == 0 and train["solverConfig"]["sweep_batch"] == 4, "pilot algorithm settings")
        players = len(reference["evaluations"][0]["result"]["seats"])
        require(train["metrics"]["traversals"] == n * players, "complete seat sweep budget")
        require(all(math.isfinite(v) and v > 0 for v in (train["solveElapsedSecs"], measure["wallSeconds"], data["constructionElapsedSecs"]))
                and measure["wallSeconds"] >= train["solveElapsedSecs"] + data["constructionElapsedSecs"], "driver/whole-process clocks")
        contexts = [node["context"] for node in data["policySupport"]]
        require([c["requested"] for c in contexts] == argument_values(args, "--support-node"), "support schedule")
        require(len({c["history"] for c in contexts}) == len(contexts) == 14, "unique support histories")
        require(shared_contexts is None or shared_contexts == contexts, "public model contexts")
        shared_contexts = contexts
        support_layout = [{key: node[key] for key in ("context", "bucketActiveOpponents", "expectedBuckets", "actionLabels")}
                          for node in data["policySupport"]]
        require(shared_support_layout is None or shared_support_layout == support_layout, "fixed support layout/denominators")
        shared_support_layout = support_layout
        require(data["policyArena"]["pages_committed"] and (shared_arena is None or data["policyArena"] == shared_arena), "same committed arena")
        shared_arena = data["policyArena"]
        require(shared_abstraction is None or shared_abstraction == data["abstractionFingerprint"], "same abstraction")
        shared_abstraction = data["abstractionFingerprint"]
        support = []
        for node in data["policySupport"]:
            support.append({"context": node["context"], "expectedBuckets": node["expectedBuckets"], **support_checks(node)})
        seeds = exp["evaluationSeeds"]
        require(seeds == [101, 202] == [int(v) for v in argument(args, "--evaluation-seeds").split(",")], "literal evaluation seed schedule")
        require(data["evaluationSeeds"] == seeds, "evaluation seeds")
        require(data["evaluationSamplesPerSeed"] == exp["candidateSamplesPerSeed"], "candidate budget")
        require(data["deviatorTraining"]["traversalsPerSeat"] == exp["candidateTrainingTraversalsPerSeat"], "candidate fit budget")
        require(data["deviatorTraining"]["seed"] == int(argument(args, "--br-seed", str(0x6272_2d61_7564_6974))), "candidate fit seed")
        require(int(argument(args, "--samples")) == exp["candidateSamplesPerSeed"] == 128
                and int(argument(args, "--br-traversals")) == exp["candidateTrainingTraversalsPerSeat"] == 1, "literal incidental evaluation budget")
        require([entry["seed"] for entry in data["evaluations"]] == seeds, "candidate evaluation cardinality")
        require(all(entry["result"]["samples"] == exp["candidateSamplesPerSeed"]
                    and len(entry["result"]["seats"]) == players for entry in data["evaluations"]), "actual candidate evaluation budget")
        for entry in data["evaluations"]:
            coverage_checks(entry["result"]["candidate_policy_coverage"], players)
        require(int(argument(args, "--coverage-samples")) == exp["ordinaryCoverageSamplesPerSeed"] == 131072, "literal ordinary coverage budget")
        require([e["seed"] for e in data["coverageEvaluations"]] == seeds, "ordinary coverage schedule")
        for entry in data["coverageEvaluations"]:
            require(entry["result"]["samples"] == exp["ordinaryCoverageSamplesPerSeed"], "ordinary coverage budget")
            require([p["requested"] for p in entry["prefixes"]] == argument_values(args, "--coverage-prefix"), "ordinary prefix schedule")
            require(entry["result"]["deviation_gain_lower_bound"] is None, "baseline-only coverage")
            coverage_checks(entry["result"]["candidate_policy_coverage"], players)
            for prefix in entry["prefixes"]:
                context = next(c for c in contexts if c["requested"] == prefix["requested"])
                require(prefix["history"] == context["history"], "ordinary prefix identity")
                require(0 <= prefix["reachedSamples"] <= exp["ordinaryCoverageSamplesPerSeed"]
                        and prefix["reachedFraction"] == prefix["reachedSamples"] / exp["ordinaryCoverageSamplesPerSeed"], "ordinary reach denominator")
                coverage_checks(prefix["candidatePolicyCoverage"], players)
                trajectories = prefix["trajectoryVisitsByStreet"]
                require(set(trajectories) == {"preflop", "flop", "turn", "river"}
                        and all(0 <= count <= prefix["reachedSamples"] for count in trajectories.values())
                        and trajectories[context["street"]] == prefix["reachedSamples"], "ordinary trajectory denominator")
        require(argument(args, "--condition-sampler") == "preflop-proposal" and "conditionalEvaluations" not in data, "proposal mode boundary")
        require(int(argument(args, "--condition-samples")) == exp["conditionalSamplesPerSeedPerTrunk"] == 131072, "literal conditional budget")
        expected_paths = argument_values(args, "--condition-prefix")
        by_seed = {seed: [] for seed in seeds}
        conditional = []
        baseline_equal = 0
        prepared = {}
        schedule = proposal_schedule(data, args, seeds, contexts)
        for entry, (expected_seed, trunk, members) in zip(data["preflopConditionalEvaluations"], schedule, strict=True):
            require(entry["seed"] == expected_seed and entry["prefixes"] == members, "exact conditional group/context schedule")
            require(entry["seed"] in seeds and entry["result"]["seed"] == entry["seed"], "conditional seed")
            require(entry["result"]["samples"] == exp["conditionalSamplesPerSeedPerTrunk"], "conditional budget")
            require(entry["result"]["total_deal_attempts"] >= entry["result"]["samples"], "proposal deal attempts")
            metadata = entry["result"]["proposal"]
            require(metadata["root_range_fingerprint"] == root_range_fingerprint
                    and len(bytes(metadata["proposal_range_fingerprint"])) == 32, "proposal/root range identity")
            require(bytes(metadata["preflop_history"]).hex() == trunk["history"]
                    and metadata["preflop_actions"] == trunk["actionIndices"], "proposal trunk identity")
            require(trunk["history"] not in prepared or prepared[trunk["history"]] == metadata, "proposal stable across seeds")
            require(all(other["root_range_fingerprint"] == metadata["root_range_fingerprint"] for other in prepared.values()), "shared root ranges")
            prepared[trunk["history"]] = metadata
            require(0 < metadata["pilot_accepted"] <= metadata["pilot_samples"], "proposal pilot acceptance")
            require(all(len(metadata[key]) == players for key in ("positive_target_combos_by_seat", "floor_adjusted_combos_by_seat", "target_scale_by_seat")), "proposal seat metadata")
            require(all(0 <= floor <= positive <= 1326 and positive > 0 for floor, positive in
                        zip(metadata["floor_adjusted_combos_by_seat"], metadata["positive_target_combos_by_seat"], strict=True)), "proposal support metadata")
            require(all(math.isfinite(scale) and scale > 0 for scale in metadata["target_scale_by_seat"])
                    and metadata["proposal_floor_fraction"] == 1e-7, "proposal scales/floor")
            for context, prefix in zip(entry["prefixes"], entry["result"]["prefixes"], strict=True):
                by_seed[entry["seed"]].append(context["requested"])
                require(bytes(prefix["history"]).hex() == context["history"] and prefix["action_indices"] == context["actionIndices"], "conditional identity")
                require(len(prefix["seats"]) == players and "reach_probability" not in prefix, "conditional population/weight labeling")
                source_checks(prefix, entry["result"]["samples"])
                conditional.append({"seed": entry["seed"], "context": context, "result": prefix})
                key = entry["seed"], context["requested"]
                if epsilon == 0 and key in reference_rows:
                    require((context, prefix, metadata) == reference_rows[key], "fresh baseline differs from retained frozen checkpoint/context/proposal")
                    baseline_equal += 1
        require(len(data["preflopConditionalEvaluations"]) == len(seeds)*3, "conditional trunk count")
        require(all(paths == expected_paths for paths in by_seed.values()), "complete conditional schedule")
        if epsilon == 0:
            require(sha(config_path) == exp["configSha256"] and data["configurationFingerprint"] == reference["configurationFingerprint"], "baseline checkpoint/config identity")
            require(baseline_equal == len(reference_rows) == 8, "complete baseline regression")
        require([node["requested"] for node in data["nodes"]] == argument_values(args, "--node"), "preflop node cardinality")
        frequency_samples = int(argument(args, "--node-frequency-samples"))
        require(frequency_samples == 131072, "node frequency budget")
        for node in data["nodes"]:
            context = next(c for c in contexts if c["requested"] == node["requested"])
            require(node["history"] == context["history"] and node["actor"] == context["actor"]
                    and node["activeOpponents"] == context["activeOpponents"], "preflop node identity")
            require(node["frequency"]["sampleCount"] == frequency_samples
                    and node["frequency"]["totalDealAttempts"] >= frequency_samples, "actual node frequency budget")
            require(node["frequency"]["seed"] == int(argument(args, "--node-frequency-seed", str(0x6e6f_6465_2d66_7265))), "node frequency seed")
        all_cases.append({"case": case, "epsilon": epsilon, "measurement": measure,
                          "freshTraining": train, "configurationFingerprint": data["configurationFingerprint"],
                          "abstractionFingerprint": data["abstractionFingerprint"],
                          "supportSummary": support, "policySupport": data["policySupport"],
                          "conditional": conditional, "nodes": data["nodes"],
                          "ordinaryCoverage": data["coverageEvaluations"],
                          "baselineConditionalRowsExactlyMatch": baseline_equal})
    baseline_seconds = all_cases[0]["freshTraining"]["solveElapsedSecs"]
    for case in all_cases:
        case["driverCostRatioToBaseline"] = case["freshTraining"]["solveElapsedSecs"] / baseline_seconds
        case["passesPilotCostGate"] = case["driverCostRatioToBaseline"] <= 2
    return {"schemaVersion": "solvers.opponent-exploration-screen/v1", "status": "pilot-complete",
            "sourceManifestSha256": exp["sourceManifestSha256"], "sourceFiles": source_files,
            "binarySha256": exp["binarySha256"], "verification": verification,
            "summarizerSha256": exp["summarizerSha256"], "summaryDependencySha256": exp["summaryDependencySha256"],
            "runnerSha256": exp["runnerSha256"], "baselineConditionalReferenceSha256": exp["baselineConditionalReferenceSha256"],
            "interpretation": "Fresh same-seed fixed-sweep parameter screen. Support and within-profile conditional estimates are not strategy-quality or exploitability proofs. Each learned profile changes the conditioned population. Additional training seeds, calibrated compute and held-out deviation evidence remain required before promotion.",
            "cases": all_cases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = summarize(args.run)
    output = args.output or args.run / "summary.json"
    output.write_text(json.dumps(result, indent=2)+"\n", encoding="utf-8", newline="\n")
    print(json.dumps({"output": str(output), "cases": len(result["cases"])}))


if __name__ == "__main__":
    main()
