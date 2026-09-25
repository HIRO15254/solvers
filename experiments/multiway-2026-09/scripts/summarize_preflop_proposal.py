"""Validate retained preflop-proposal audits and compare separate estimators."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import zipfile
from pathlib import Path


def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def sha(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def require(test, message):
    if not test:
        raise ValueError(message)


def strip_timing(value):
    if isinstance(value, dict):
        return {k: strip_timing(v) for k, v in value.items()
                if k not in {"elapsedSecs", "constructionElapsedSecs", "frequencyElapsedSecs"}}
    if isinstance(value, list):
        return [strip_timing(v) for v in value]
    return value


def estimate_checks(estimate, label, *, probability=False):
    require(isinstance(estimate, dict), label + " missing estimate")
    require(math.isfinite(estimate["mean"]) and math.isfinite(estimate["stderr"])
            and estimate["stderr"] >= 0, label + " non-finite estimate")
    if probability:
        require(-1e-12 <= estimate["mean"] <= 1 + 1e-12, label + " probability")


def source_checks(prefix, n):
    require(0 <= prefix["positive_weight_samples"] <= n, "positive count")
    positive = prefix["positive_weight_samples"]
    require(0 <= prefix["effective_sample_size"] <= positive + 1e-8, "prefix ESS")
    require(0 <= prefix["max_normalized_weight"] <= 1 + 1e-12, "maximum weight")
    estimate_checks(prefix["relative_weight_mean"], "relative weight")
    require(prefix["relative_weight_mean"]["mean"] >= 0, "negative relative weight")
    require((prefix["relative_weight_mean"]["mean"] > 0) == (positive > 0), "weight denominator")
    for key in ("prefix_current_fraction", "prefix_regret_fallback_fraction", "prefix_uniform_fallback_fraction"):
        require((prefix[key] is not None) == (positive > 0), "prefix source denominator")
        if prefix[key] is not None:
            estimate_checks(prefix[key], key, probability=True)
    require(len(prefix["coverage_by_street"]) == 4, "street dimensions")
    require(len(prefix["coverage_by_seat"]) == len(prefix["seats"]), "seat dimensions")
    require(all(len(seat) == 4 for seat in prefix["coverage_by_seat"]), "seat street dimensions")
    streets = prefix["coverage_by_street"] + [s for seat in prefix["coverage_by_seat"] for s in seat]
    for street in streets:
        decisions = street["positive_weight_decision_visits"]
        trajectories = street["positive_weight_trajectory_visits"]
        require(0 <= trajectories <= positive and decisions >= trajectories, "street visit counts")
        require((decisions > 0) == (trajectories > 0), "street trajectory denominator")
        require(0 <= street["decision_weight_effective_sample_size"] <= trajectories + 1e-8, "decision ESS")
        for key in ("trajectory_probability", "decisions_per_prefix_trajectory"):
            require((street[key] is not None) == (positive > 0), key + " denominator")
            if street[key] is not None:
                estimate_checks(street[key], key, probability=key == "trajectory_probability")
                require(street[key]["mean"] >= 0, key + " negative mean")
        estimates = [street[k] for k in ("average_fraction", "current_fraction", "regret_fallback_fraction", "uniform_fallback_fraction")]
        require(all((e is not None) == (decisions > 0) for e in estimates), "source denominator")
        if estimates[0] is not None:
            require(math.isclose(sum(e["mean"] for e in estimates), 1, abs_tol=1e-9), "source partition")
            for estimate in estimates:
                estimate_checks(estimate, "source", probability=True)
    for index, street in enumerate(prefix["coverage_by_street"]):
        require(street["positive_weight_decision_visits"] == sum(seat[index]["positive_weight_decision_visits"]
                for seat in prefix["coverage_by_seat"]), "seat decision partition")
    for seat in prefix["seats"]:
        require((seat is not None) == (positive > 0), "utility denominator")
        if seat is not None:
            estimate_checks(seat, "utility")


def argument_values(arguments, flag):
    values = []
    for index, value in enumerate(arguments):
        if value == flag:
            require(index + 1 < len(arguments) and not arguments[index + 1].startswith("--"), flag + " missing value")
            values.append(arguments[index + 1])
    return values


def argument(arguments, flag, default=None):
    values = argument_values(arguments, flag)
    require(len(values) == 1 or (not values and default is not None), flag + " missing/duplicate argument")
    return values[0] if values else default


def validate_evidence(run, experiment):
    for key, file in [("sourceManifestSha256", "source-manifest.json"), ("sourceZipSha256", "source.zip"),
                      ("configSha256", "config.toml"), ("binarySha256", "audit.exe"),
                      ("verificationSha256", "verification/verification.json")]:
        require(sha(run / file) == experiment[key], file + " identity")
    manifest = read(run / "source-manifest.json")
    files = {item["path"]: item["sha256"] for item in manifest["files"]}
    require(files and len(files) == len(manifest["files"]), "duplicate/empty source manifest")
    require(manifest["baseRevision"] == experiment["baseRevision"], "base revision")
    with zipfile.ZipFile(run / "source.zip") as archive:
        require(len(archive.namelist()) == len(files) and set(archive.namelist()) == set(files), "source ZIP entries")
        for name, digest in files.items():
            with archive.open(name) as stream:
                require(hashlib.file_digest(stream, "sha256").hexdigest() == digest, "archived source: " + name)
    verification = read(run / "verification/verification.json")
    require(verification["status"] == "passed" and verification["sourceManifestSha256"] == experiment["sourceManifestSha256"], "verification source/status")
    commands = [check["command"] for check in verification["checks"]]
    required = {"cargo fmt --all --check", "cargo clippy --workspace --all-targets -- -D warnings", "cargo test --workspace",
                "cargo test -p cli --examples --features research-draw-abstraction",
                "cargo test -p multiway --features research-average-sampling --lib average_sampling_research",
                "cargo build --release -p cli --example mw_checkpoint_audit"}
    require(len(commands) == len(set(commands)) and required <= set(commands), "required verification checks")
    for check in verification["checks"]:
        require(check["exitCode"] == 0 and sha(check["log"]) == check["sha256"], "verification log: " + check["command"])
        if check["command"].startswith("cargo test "):
            text = Path(check["log"]).read_text(encoding="utf-8-sig")
            counts = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
            totals = [sum(int(row[index]) for row in counts) for index in range(3)]
            require(counts and "test result: FAILED" not in text and totals[1] == 0, "failed/empty test log")
            require(totals == [check["passed"], check["failed"], check["ignored"]]
                    and len(counts) == check["suites"], "test totals")
    return verification, len(files)


def load_references(experiment):
    references = {}
    anchor = None
    for key in ["rootRegressionReference", "higherBudgetRootReference"]:
        require(sha(experiment[key]) == experiment[key + "Sha256"], "reference identity")
        data = read(experiment[key])
        identity = {k: data[k] for k in ("schemaVersion", "sweeps", "solverStateVersion", "configurationFingerprint", "abstractionFingerprint")}
        identity["checkpointSha256"] = sha(data["checkpoint"])
        require(sha(data["config"]) == experiment["configSha256"], "reference config identity")
        require(anchor is None or anchor == identity, "reference checkpoint/model identity")
        anchor = identity
        require([entry["seed"] for entry in data["conditionalEvaluations"]] == data["evaluationSeeds"], "reference seed schedule")
        for entry in data["conditionalEvaluations"]:
            require(entry["result"]["seed"] == entry["seed"], "reference seed mapping")
            for context, prefix in zip(entry["prefixes"], entry["result"]["prefixes"], strict=True):
                pair = (entry["seed"], context["history"])
                require(pair not in references, "duplicate reference row")
                require(bytes(prefix["history"]).hex() == context["history"] and prefix["action_indices"] == context["actionIndices"], "reference context mapping")
                references[pair] = {"samples": entry["result"]["samples"], "context": context, "result": prefix,
                                    "file": experiment[key], "fileSha256": experiment[key + "Sha256"]}
    return references, anchor


def proposal_rows(data, job, references, prepared):
    arguments = job["arguments"]
    seeds = [int(value) for value in argument(arguments, "--evaluation-seeds").split(",")]
    requested = argument_values(arguments, "--condition-prefix")
    n = int(argument(arguments, "--condition-samples"))
    require(len(seeds) == len(set(seeds)) == 2 and n >= 2, "proposal seed/sample schedule")
    require(len(requested) == len(set(requested)) == 4, "proposal prefix schedule")
    require(data["evaluationSeeds"] == seeds, "output evaluation seeds")
    require("conditionalEvaluations" not in data, "root field in proposal mode")
    expected = []
    for seed in seeds:
        groups = {}
        for path in requested:
            matches = [ref for (ref_seed, _), ref in references.items()
                       if ref_seed == seed and ref["context"]["requested"] == path]
            require(len(matches) == 1, "missing/ambiguous reference for requested prefix")
            context = matches[0]["context"]
            require(context["street"] in {"flop", "turn", "river"}, "non-postflop proposal context")
            actions = context["actionIndices"]
            trunks = [ref["context"] for (ref_seed, _), ref in references.items()
                      if ref_seed == seed and ref["context"]["street"] == "flop"
                      and actions[:len(ref["context"]["actionIndices"])] == ref["context"]["actionIndices"]]
            require(len(trunks) == 1, "missing/ambiguous preflop trunk reference")
            groups.setdefault(trunks[0]["history"], (trunks[0], []))[1].append(context)
        require(len(groups) == 2, "proposal trunk schedule")
        expected.extend((seed, trunk, contexts) for trunk, contexts in groups.values())
    entries = data["preflopConditionalEvaluations"]
    require(len(entries) == len(expected), "missing/duplicate proposal groups")
    rows = []
    for entry, (seed, trunk_context, contexts) in zip(entries, expected, strict=True):
        result = entry["result"]
        require(entry["seed"] == result["seed"] == seed and result["samples"] == n, "proposal seed/sample budget")
        require(entry["prefixes"] == contexts, "proposal context/order/cardinality")
        require(result["total_deal_attempts"] >= n, "rejection count")
        metadata = result["proposal"]
        trunk = bytes(metadata["preflop_history"]).hex()
        require(trunk == trunk_context["history"] and metadata["preflop_actions"] == trunk_context["actionIndices"], "proposal trunk identity")
        require(0 < metadata["pilot_accepted"] <= metadata["pilot_samples"], "pilot acceptance")
        require(trunk not in prepared or prepared[trunk] == metadata, "proposal changed across seeds")
        require(all(item["root_range_fingerprint"] == metadata["root_range_fingerprint"] for item in prepared.values()), "root range identity")
        prepared[trunk] = metadata
        for context, prefix in zip(contexts, result["prefixes"], strict=True):
            require(bytes(prefix["history"]).hex() == context["history"], "history mapping")
            require(prefix["action_indices"] == context["actionIndices"], "action mapping")
            require("reach_probability" not in prefix and "relative_weight_mean" in prefix, "weight labeling")
            source_checks(prefix, n)
            seats = len(prefix["seats"])
            require(seats == len(data["evaluations"][0]["result"]["seats"]), "proposal player count")
            require(all(len(metadata[key]) == seats for key in ("positive_target_combos_by_seat", "floor_adjusted_combos_by_seat", "target_scale_by_seat")), "proposal seat metadata")
            require(all(0 <= floor <= positive <= 1326 and positive > 0 for floor, positive in zip(metadata["floor_adjusted_combos_by_seat"], metadata["positive_target_combos_by_seat"], strict=True)), "proposal support metadata")
            require(all(math.isfinite(scale) and scale > 0 for scale in metadata["target_scale_by_seat"])
                    and math.isfinite(metadata["proposal_floor_fraction"]) and 0 < metadata["proposal_floor_fraction"] <= 1, "proposal scales/floor")
            require(len(bytes(metadata["root_range_fingerprint"])) == len(bytes(metadata["proposal_range_fingerprint"])) == 32, "range fingerprint length")
            rows.append({"seed": seed, "samplesPerTrunk": n,
                         "totalDealAttemptsForTrunk": result["total_deal_attempts"], "proposal": metadata,
                         "context": context, "result": prefix, "rootReference": references[seed, context["history"]]})
    return rows


def summarize_case(run, case, experiment, references, anchor, prepared):
    folder = run / case
    measurement = read(folder / "measurement.json")
    data = read(folder / "stdout.json")
    job_path = run / (case + "-job.json")
    job = read(job_path)
    arguments = job["arguments"]
    require(measurement["exitCode"] == 0 and not measurement["timedOut"], "completed run")
    require(measurement["arguments"] == arguments, "measured/job arguments")
    require(Path(measurement["job"]).resolve() == job_path.resolve(), "measured job path")
    require(measurement["timeoutSeconds"] == job["timeoutSeconds"], "timeout budget")
    for key, path in [("stdoutSha256", folder / "stdout.json"), ("jobSha256", job_path),
                      ("binarySha256", measurement["binary"])]:
        require(sha(path) == measurement[key], key)
    require(measurement["binarySha256"] == experiment["binarySha256"], "experiment binary")
    require(measurement["sourceManifestSha256"] == job["sourceManifestSha256"] == experiment["sourceManifestSha256"], "source identity")
    require(measurement["sourceRevision"] == experiment["baseRevision"], "measured base revision")
    require(measurement["configSha256"] == job["configSha256"] == experiment["configSha256"], "config identity")
    require(Path(data["config"]).resolve() == Path(argument(arguments, "--config")).resolve(), "config argument")
    require(sha(data["config"]) == experiment["configSha256"], "actual config identity")
    require(Path(data["checkpoint"]).resolve() == Path(argument(arguments, "--checkpoint")).resolve(), "checkpoint argument")
    require(sha(data["checkpoint"]) == job["checkpointSha256"] == anchor["checkpointSha256"], "checkpoint identity")
    for key in ("schemaVersion", "sweeps", "solverStateVersion", "configurationFingerprint", "abstractionFingerprint"):
        require(data[key] == anchor[key], "reference model: " + key)
    seeds = [int(value) for value in argument(arguments, "--evaluation-seeds").split(",")]
    samples = int(argument(arguments, "--samples"))
    require(len(seeds) == len(set(seeds)) and data["evaluationSeeds"] == seeds, "seed schedule")
    require(data["evaluationSamplesPerSeed"] == samples, "ordinary sample budget")
    require([entry["seed"] for entry in data["evaluations"]] == seeds, "ordinary evaluation schedule")
    require(all(entry["result"]["samples"] == samples for entry in data["evaluations"]), "ordinary result budget")
    coverage_samples = int(argument(arguments, "--coverage-samples", str(samples)))
    coverage_paths = argument_values(arguments, "--coverage-prefix")
    require([entry["seed"] for entry in data["coverageEvaluations"]] == seeds, "coverage seed schedule")
    for entry in data["coverageEvaluations"]:
        require(entry["result"]["samples"] == coverage_samples, "coverage sample budget")
        require([prefix["requested"] for prefix in entry["prefixes"]] == coverage_paths, "coverage prefix schedule")
        for prefix in entry["prefixes"]:
            require(0 <= prefix["reachedSamples"] <= coverage_samples and prefix["reachedFraction"] == prefix["reachedSamples"] / coverage_samples, "ordinary reach denominator")
    record = {"case": case, "measurement": measurement, "sweeps": data["sweeps"],
              "constructionSeconds": data["constructionElapsedSecs"],
              "configurationFingerprint": data["configurationFingerprint"],
              "abstractionFingerprint": data["abstractionFingerprint"], "rows": []}
    mode = argument(arguments, "--condition-sampler", "root")
    if case == "root-regression":
        require(mode == "root", "root regression sampler")
        reference = read(experiment["rootRegressionReference"])
        require(sha(reference["config"]) == sha(data["config"]) == experiment["configSha256"], "root copied config identity")
        comparison = strip_timing(data)
        original = strip_timing(reference)
        comparison["config"] = original["config"]
        require(comparison == original, "root output changed")
        require(int(argument(arguments, "--condition-samples")) == reference["conditionalEvaluations"][0]["result"]["samples"], "root condition budget")
        require(argument_values(arguments, "--condition-prefix") == [context["requested"] for context in reference["conditionalEvaluations"][0]["prefixes"]], "root condition schedule")
        record["allFieldsExceptTimingAndVerifiedConfigLocationExactlyMatch"] = True
        record["verifiedConfigLocations"] = {"actual": data["config"], "reference": reference["config"],
                                             "sha256": experiment["configSha256"]}
    else:
        require(mode == "preflop-proposal", "proposal sampler")
        record["rows"] = proposal_rows(data, job, references, prepared)
    return record


def summarize(run):
    experiment = read(run / "experiment.json")
    require(len(experiment["cases"]) == len(set(experiment["cases"])) == 3
            and "root-regression" in experiment["cases"], "retained three-case design")
    verification, source_files = validate_evidence(run, experiment)
    references, anchor = load_references(experiment)
    prepared = {}
    cases = [summarize_case(run, case, experiment, references, anchor, prepared)
             for case in experiment["cases"]]
    pairs = [(row["seed"], row["context"]["history"]) for case in cases for row in case["rows"]]
    require(len(pairs) == len(set(pairs)) == 16, "complete unique proposal row set")
    require({seed for seed, _ in pairs} == {seed for seed, _ in references}, "proposal/reference seed coverage")
    require(len(prepared) == 2, "shared two-trunk experiment")
    return {"schemaVersion": "solvers.preflop-proposal-validation/v1", "status": "complete",
            "sourceManifestSha256": experiment["sourceManifestSha256"], "sourceZipSha256": experiment["sourceZipSha256"],
            "verification": verification,
            "interpretation": "Corrected proposal and root estimators target the same frozen conditional profile. Seeds are reported separately; no paired standard error or pooled inference is claimed. Proposal relative weight is not root reach. Suffix decisions below a three-player flop may become HU after folds. ESS measures concentration, not a universal variance-equivalent sample count.",
            "cases": cases,
            "validationChecks": ["source ZIP entries and hashes", "binary/config/checkpoint/reference identity",
                                 "literal measured/job argument equality", "seed/trunk/prefix cardinality and sample budgets",
                                 "full reference contexts", "finite weighted estimates and source denominators",
                                 "verification logs and test totals"],
            "sourceFiles": source_files, "binarySha256": experiment["binarySha256"],
            "verificationSha256": experiment["verificationSha256"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = summarize(args.run)
    output = args.output or args.run / "summary.json"
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({"output": str(output), "cases": len(result["cases"]),
                      "proposalRows": sum(len(c["rows"]) for c in result["cases"])}))


if __name__ == "__main__":
    main()
