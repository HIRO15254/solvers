"""Validate one fixed-profile preflop endpoint baseline, not a tuning cohort.

Every endpoint keeps 169 own-information rows. Conditional signed improvement,
absolute root reach, policy-source coverage and learning identity stay separate.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import zipfile
from pathlib import Path

import summarize_average_continuation as pilot
from summarize_average_continuation import (
    balanced, count, digest, endpoint_dependency, estimate_checks, hex_bytes,
    key_for, normalized_only, number, options, read, require, sha, source_checks,
    support_checks,
)

CASE = "seed0-s32768-no-discount"


def endpoint_checks(result, schedule, support):
    config = dict(fit_samples=schedule["fitSamples"], fit_seed=schedule["fitSeed"],
                  held_out_samples=schedule["heldOutSamples"], held_out_seeds=schedule["heldOutSeeds"],
                  min_fit_ess=schedule["minFitEss"])
    require(result["config"] == config, "endpoint schedule")
    require(config["fit_samples"] >= 2 and config["held_out_samples"] >= 2
            and len(config["held_out_seeds"]) == len(set(config["held_out_seeds"]))
            and config["fit_seed"] not in config["held_out_seeds"]
            and math.isfinite(config["min_fit_ess"]) and config["min_fit_ess"] >= 2,
            "independent fit/held-out schedule")
    require(result["variant"] == dict(purify_threshold=0.0, use_current_strategy=False), "baseline average profile")
    for name in ("history", "action_indices", "actor", "street", "active_opponents",
                 "bucket_active_opponents", "expected_buckets", "action_labels"):
        require(result[name] == support[name], "endpoint/support identity: " + name)
    support_checks(support)
    actions = len(result["action_labels"])
    require(result["street"] == "preflop" and result["expected_buckets"] == 169
            and 1 <= actions <= 8, "preflop endpoint action/bucket bound")
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
        if n:
            require(ess >= 1 - 1e-8 and maximum >= 1 / n - 1e-12,
                    "positive fit concentration lower bound")
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



def proposal_checks(proposal, endpoint, root_range):
    require(proposal["preflop_actions"] == endpoint["action_indices"]
            and proposal["preflop_history"] == endpoint["history"],
            "proposal is precisely the root/partial preflop prefix, excluding endpoint action")
    require(proposal["root_range_fingerprint"] == root_range, "unchanged root ranges")
    hex_bytes(proposal["proposal_range_fingerprint"], 32)
    require(proposal["proposal_floor_fraction"] == 1e-7
            and type(proposal["pilot_samples"]) is int
            and 0 < proposal["pilot_accepted"] <= proposal["pilot_samples"], "proposal admission")
    for field in ("positive_target_combos_by_seat", "floor_adjusted_combos_by_seat", "target_scale_by_seat"):
        require(len(proposal[field]) == 6, "all folded and active seats retain blockers")
    for positive, floor, scale in zip(proposal["positive_target_combos_by_seat"],
                                    proposal["floor_adjusted_combos_by_seat"],
                                    proposal["target_scale_by_seat"], strict=True):
        count(positive, 1326, "target combo count")
        require(positive > 0, "nonempty target range")
        count(floor, positive, "proposal floor count")
        number(scale, "target scale", positive=True)
    require("reach_probability" not in endpoint["fit"]["sampling"], "proposal weights are not absolute reach")


def evidence_checks(run, exp):
    for key, name in [("sourceManifestSha256", "source-manifest.json"), ("sourceZipSha256", "source.zip"),
                      ("configSha256", "config-seed0.toml"), ("binarySha256", "research.exe"),
                      ("verificationSha256", "verification/verification.json")]:
        require(sha(run / name) == exp[key], "evidence identity: " + name)
    require(sha(__file__) == exp["summarizerSha256"], "analysis identity")
    require(sha(Path(__file__).parent / "tests/test_summarize_preflop_endpoint.py")
            == exp["testScriptSha256"], "analysis test identity")
    require(sha(Path(__file__).with_name("run_average_sampling_measurement.ps1")) == exp["runnerSha256"], "measurement runner identity")
    dependencies = [pilot, balanced, endpoint_dependency, pilot.support_dependency, pilot.evidence_dependency]
    require({Path(path).resolve() for path in exp["dependencies"]}
            == {Path(module.__file__).resolve() for module in dependencies}, "complete analysis dependency set")
    for path, checksum in exp["dependencies"].items():
        require(sha(path) == checksum, "frozen helper identity: " + path)
    manifest = read(run / "source-manifest.json")
    files = {row["path"]: row["sha256"] for row in manifest["files"]}
    require(len(files) == len(manifest["files"]) == 167 and manifest["baseRevision"] == exp["baseRevision"], "source manifest completeness")
    with zipfile.ZipFile(run / "source.zip") as archive:
        require(len(archive.namelist()) == len(files) and set(archive.namelist()) == set(files), "source ZIP entries")
        for path, checksum in files.items():
            with archive.open(path) as source:
                require(hashlib.file_digest(source, "sha256").hexdigest() == checksum, "archived source: " + path)
    verification = read(run / "verification/verification.json")
    require(verification["status"] == "passed" and verification["sourceManifestSha256"] == exp["sourceManifestSha256"], "verification status/source")
    checks = verification["checks"]
    require(len(checks) == 7 and {c["command"] for c in checks} == pilot.COMMANDS, "seven required verification commands")
    for check in checks:
        require(check["exitCode"] == 0 and sha(check["log"]) == check["sha256"], "verification log hash/pass")
        number(check["elapsedSecs"], "verification duration")
        if check["command"].startswith("cargo test "):
            text = Path(check["log"]).read_text(encoding="utf-8-sig")
            rows = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
            totals = [sum(int(row[i]) for row in rows) for i in range(3)]
            require(rows and totals[0] > 0 and totals[1] == 0 and "test result: FAILED" not in text, "successful nonempty test logs")
            require(totals == [check["passed"], check["failed"], check["ignored"]]
                    and len(rows) == check["suites"], "verification test totals")
    return verification


def reference_checks(exp):
    require(len(exp["references"]) == 1, "one exact prior UniformOne reference")
    path, checksum = next(iter(exp["references"].items()))
    require(sha(path) == checksum, "prior raw identity")
    parent = Path(path).parent.parent
    parent_exp = read(parent / "experiment.json")
    pilot.evidence_checks(parent, parent_exp)
    references = pilot.references_checks(parent_exp)
    _, raw = pilot.summarize_case(parent, parent_exp, parent_exp["cases"][0], references)
    require(Path(path).resolve() == (parent / parent_exp["cases"][0]["name"] / "stdout.json").resolve(), "UniformOne reference case mapping")
    require(parent_exp["configSha256"] == exp["configSha256"], "same baseline config")
    return raw, dict(rawPath=path, rawSha256=checksum, sourceManifestSha256=parent_exp["sourceManifestSha256"],
                    binarySha256=parent_exp["binarySha256"], verificationSha256=parent_exp["verificationSha256"])


def schedule_checks(exp):
    require(exp["trainingSeed"] == 0 and exp["sweeps"] == 32768 and exp["variant"] == "uniform-one"
            and exp["threads"] == 8 and exp["memory"] == "8GiB" and exp["timeoutSeconds"] == 1200,
            "single predeclared training/resource schedule")
    require(exp["endpointSchedule"] == dict(fitSamples=65536, fitSeed=602, heldOutSamples=131072,
                                            heldOutSeeds=[702, 703], minFitEss=64), "fixed endpoint sample schedule")
    require(exp["rootSchedule"] == dict(samples=262144, seeds=[801, 802]), "independent root schedule")
    require(exp["ordinarySchedule"] == dict(samples=128, coverageSamples=131072, seeds=[101, 202]), "ordinary replay schedule")
    require(len(exp["nodes"]) == len(set(exp["nodes"])) == 16
            and len(exp["coveragePrefixes"]) == len(set(exp["coveragePrefixes"])) == 8, "support/coverage cardinality")
    selected = [exp["nodes"][i] for i in (0, 4, 5, 6, 7)]
    require(exp["endpointPaths"] == selected and selected[0] == "root", "root/SB/3bet/4bet/5bet endpoint order")


def learning_replay_checks(raw, reference):
    normalized_only(raw)
    for name in ("configurationFingerprint", "abstractionFingerprint", "solverStateVersion", "threads",
                 "nodes", "supportNodes", "coveragePrefixes", "effectiveConfigBlake3"):
        require(raw[name] == reference[name], "unchanged baseline identity: " + name)
    require(raw["diagnostics"]["support"] == reference["diagnostics"]["support"], "all sixteen raw support outputs exactly reproduce baseline")
    require({k: v for k, v in raw["result"].items() if k != "solve_elapsed_secs"}
            == {k: v for k, v in reference["result"].items() if k != "solve_elapsed_secs"},
            "all learning metrics/regrets/histories/ordinary and coverage evaluations exactly reproduce baseline")


def root_checks(result, schedule, supports):
    require(result["samples"] == schedule["samples"] and result["total_deal_attempts"] >= result["samples"]
            and len(result["prefixes"]) == len(supports), "root world budget/prefix cardinality")
    for prefix, support in zip(result["prefixes"], supports, strict=True):
        require(prefix["history"] == support["history"] and prefix["action_indices"] == support["action_indices"], "root prefix identity")
        require("relative_weight_mean" not in prefix and len(prefix["seats"]) == 6, "absolute root reach label/seat count")
        source_checks({**prefix, "relative_weight_mean": prefix["reach_probability"]}, result["samples"])
        estimate_checks(prefix["reach_probability"], "absolute reach", probability=True)
        if not support["action_indices"]:
            require(prefix["reach_probability"] == dict(mean=1.0, stderr=0.0)
                    and prefix["positive_weight_samples"] == result["samples"], "root reaches with unit probability")
            require(all(prefix[field] == dict(mean=0.0, stderr=0.0) for field in
                        ("prefix_current_fraction", "prefix_regret_fallback_fraction", "prefix_uniform_fallback_fraction")),
                    "empty root prefix has no policy sources")


def summarize(run):
    exp = read(run / "experiment.json")
    schedule_checks(exp)
    verification = evidence_checks(run, exp)
    reference, reference_metadata = reference_checks(exp)
    folder, job_path = run / CASE, run / (CASE + "-job.json")
    raw, measurement, job = read(folder / "stdout.json"), read(folder / "measurement.json"), read(job_path)
    require(measurement["schemaVersion"] == "solvers.average-sampling-measurement/v1"
            and measurement["exitCode"] == 0 and measurement["timedOut"] is False, "successful measurement")
    require(Path(measurement["binary"]).resolve() == (run / "research.exe").resolve()
            and measurement["binarySha256"] == exp["binarySha256"], "frozen measured binary")
    require(measurement["arguments"] == job["arguments"] and sha(job_path) == measurement["jobSha256"]
            and Path(measurement["job"]).resolve() == job_path.resolve(), "literal measured job")
    require(sha(folder / "stdout.json") == measurement["stdoutSha256"], "measurement raw hash")
    for field in ("sourceManifestSha256", "configSha256", "validationReport", "timeoutSeconds"):
        require(measurement[field] == job[field] == exp[field], "measured identity: " + field)
    require(measurement["sourceRevision"] == exp["baseRevision"], "base revision")
    opts = options(job["arguments"])
    expected = {"--variant": "uniform-one", "--sweeps": "32768", "--threads": "8", "--memory": "8GiB",
                "--source-revision": exp["sourceManifestSha256"], "--evaluation-seeds": "101,202",
                "--evaluation-samples": "128", "--coverage-samples": "131072", "--endpoint-fit-samples": "65536",
                "--endpoint-fit-seed": "602", "--endpoint-samples": "131072", "--endpoint-seeds": "702,703",
                "--endpoint-min-fit-ess": "64", "--root-samples": "262144", "--root-seeds": "801,802"}
    for flag, value in expected.items():
        require(opts.get(flag) == [value], "literal budget: " + flag)
    for flag, field in [("--node", "nodes"), ("--support-node", "nodes"), ("--coverage-prefix", "coveragePrefixes"), ("--endpoint-prefix", "endpointPaths")]:
        require(opts.get(flag) == exp[field], "literal node schedule: " + flag)
    require(set(opts) == set(expected) | {"--config", "--cache-dir", "--node", "--support-node", "--coverage-prefix", "--endpoint-prefix"}, "no undeclared job flags")
    require(Path(opts["--config"][0]).resolve() == Path(raw["config"]).resolve() == (run / "config-seed0.toml").resolve(), "actual config location")
    require(raw["schemaVersion"] == "solvers.multiway-average-sampling-research/v1"
            and raw["sourceRevision"] == exp["sourceManifestSha256"], "new output schema/source")
    for field, expected_paths in [("nodes", exp["nodes"]), ("supportNodes", exp["nodes"]),
                                  ("coveragePrefixes", exp["coveragePrefixes"]),
                                  ("endpointPrefixes", exp["endpointPaths"])]:
        require([context["requested"] for context in raw[field]] == expected_paths,
                "actual requested public paths: " + field)
    digest(raw["executableBlake3"])
    learning_replay_checks(raw, reference)
    diagnostic = raw["diagnostics"]
    for value in (raw["constructionElapsedSecs"], raw["elapsedSecs"], raw["result"]["solve_elapsed_secs"],
                  diagnostic["elapsed_secs"], measurement["wallSeconds"], measurement["observedPeakWorkingSetBytes"]):
        number(value, "time/memory observation", positive=True)
    require(raw["result"]["solve_elapsed_secs"] + diagnostic["elapsed_secs"] <= raw["elapsedSecs"] + .01
            and raw["elapsedSecs"] + raw["constructionElapsedSecs"] <= measurement["wallSeconds"] + .01, "time phase containment")
    ds = exp["endpointSchedule"]
    supports = dict(zip(exp["nodes"], diagnostic["support"], strict=True))
    selected = [supports[path] for path in exp["endpointPaths"]]
    contexts = [raw["nodes"][exp["nodes"].index(path)] for path in exp["endpointPaths"]]
    require(raw["endpointPrefixes"] == contexts and all(c["street"] == "preflop" for c in contexts), "five preflop contexts")
    require(diagnostic["config"] == dict(support_paths=[s["action_indices"] for s in diagnostic["support"]],
        endpoint_paths=[s["action_indices"] for s in selected],
        endpoint=dict(fit_samples=ds["fitSamples"], fit_seed=ds["fitSeed"], held_out_samples=ds["heldOutSamples"],
                      held_out_seeds=ds["heldOutSeeds"], min_fit_ess=ds["minFitEss"]),
        root_samples=exp["rootSchedule"]["samples"], root_seeds=exp["rootSchedule"]["seeds"]), "actual endpoint/root budgets")
    require(len(diagnostic["endpoints"]) == 5, "five endpoint results")
    root_range = reference["diagnostics"]["endpoints"][0]["proposal"]["root_range_fingerprint"]
    rows = []
    for endpoint, support, context in zip(diagnostic["endpoints"], selected, contexts, strict=True):
        selection = endpoint_checks(endpoint, ds, support)
        proposal_checks(endpoint["proposal"], endpoint, root_range)
        require(hex_bytes(endpoint["configuration_fingerprint"], 32) == raw["configurationFingerprint"]
                and hex_bytes(endpoint["abstraction_fingerprint"], 32) == raw["abstractionFingerprint"], "endpoint identity")
        rows.append(dict(context=context, support=pilot.public_layout(support), supportCounts=support_checks(support),
                         fitSelection=selection, result=endpoint))
    roots = diagnostic["root_evaluations"]
    require([r["seed"] for r in roots] == exp["rootSchedule"]["seeds"], "root seed/cardinality schedule")
    for root in roots:
        root_checks(root, exp["rootSchedule"], selected)
    return dict(schemaVersion="solvers.preflop-endpoint-summary/v1", status="completed-baseline",
                experiment=exp, experimentSha256=sha(run / "experiment.json"), verification=verification,
                sourceFiles=167, measurement=measurement, constructionSeconds=raw["constructionElapsedSecs"],
                solveSeconds=raw["result"]["solve_elapsed_secs"], diagnosticsSeconds=diagnostic["elapsed_secs"],
                baselineReference=reference_metadata,
                learningReplay=dict(allMetricsRegretsHistoriesOrdinaryCoverageExactlyMatch=True,
                                    allSixteenSupportOutputsExactlyMatch=True,
                                    excludedResultFields=["solve_elapsed_secs"]),
                endpoints=rows, rootEvaluations=roots,
                interpretation="Fixed seed-zero UniformOne profile baseline, not a tuning comparison or equilibrium certificate. Endpoint tables use only own information, independent fit/held-out worlds, and all prefix weight including unsupported keys. Signed conditional gains and absolute root reach remain separate; multiplying point estimates is not an unconditional improvement claim. No default or training algorithm changed.")


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
