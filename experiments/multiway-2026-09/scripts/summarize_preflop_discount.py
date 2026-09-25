"""Validate the two predeclared seed-zero preflop learning pilot arms.

The 32,768-sweep baseline is rebuilt and checked recursively. More sweeps and
periodic discount may change every regret and policy; their signed endpoint
estimates are descriptive comparisons, not paired estimates or promotion.
"""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import tomllib

import summarize_preflop_endpoint as preflop
from summarize_preflop_endpoint import count, digest, hex_bytes, number, options, read, require, sha

pilot = preflop.pilot
CASES = ("seed0-s131072-no-discount", "seed0-s131072-periodic")
DISCOUNTS = ({"kind": "none"}, {"kind": "periodic", "every_sweeps": 10000, "until_sweeps": 10000000})


def serialized(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True, allow_nan=False) + "\n").encode("utf-8")


def schedule_checks(exp):
    require(len(exp["cases"]) == 2, "two predeclared pilot arms")
    for case, name, discount in zip(exp["cases"], CASES, DISCOUNTS, strict=True):
        require((case["name"], case["variant"], case["trainingSeed"], case["sweeps"], case["timeoutSeconds"])
                == (name, "uniform-one", 0, 131072, 1800) and case["discount"] == discount,
                "predeclared case budget/discount")
    require(exp["threads"] == 8 and exp["memory"] == "8GiB", "fixed resources")
    require(exp["costGate"] == {"maxPeriodicToNoDiscountDriverRatio": 2.0}
            and exp["promotionAllowed"] is False and exp["cloudResourcesStarted"] is False,
            "pilot cost/promotion/cloud boundary")
    # Reuse the exact five-endpoint diagnostic schedule while leaving the
    # distinct training budget and configuration checks to this module.
    preflop.schedule_checks({**exp, "trainingSeed": 0, "sweeps": 32768,
                             "variant": "uniform-one", "timeoutSeconds": 1200})


def evidence_checks(exp):
    require(sha(__file__) == exp["summarizerSha256"], "analysis identity")
    require(sha(Path(__file__).parent / "tests/test_summarize_preflop_discount.py")
            == exp["testScriptSha256"], "analysis test identity")
    modules = [preflop, pilot, preflop.balanced, preflop.endpoint_dependency,
               pilot.support_dependency, pilot.evidence_dependency]
    require({Path(path).resolve() for path in exp["dependencies"]}
            == {Path(module.__file__).resolve() for module in modules}, "complete six-helper dependency set")
    for path, checksum in exp["dependencies"].items():
        require(sha(path) == checksum, "frozen dependency identity: " + path)
    source = Path(exp["sourceEvidenceRun"])
    summary_path = Path(exp["baselineSummary"])
    require(summary_path.resolve() == (source / "summary.json").resolve(), "baseline/source evidence mapping")
    require(sha(source / "experiment.json") == exp["baselineExperimentSha256"], "baseline experiment identity")
    require(sha(summary_path) == exp["baselineSummarySha256"], "baseline summary identity")
    baseline = preflop.summarize(source)
    require(serialized(baseline) == summary_path.read_bytes(), "baseline summary byte regeneration")
    baseline_exp = baseline["experiment"]
    for field in ("baseRevision", "sourceManifestSha256", "sourceZipSha256", "binarySha256",
                  "verificationSha256", "runnerSha256", "threads", "memory", "nodes", "coveragePrefixes",
                  "endpointPaths", "endpointSchedule", "rootSchedule", "ordinarySchedule"):
        require(exp[field] == baseline_exp[field], "same verified source/diagnostic schedule: " + field)
    require(Path(exp["binary"]).resolve() == (source / "research.exe").resolve()
            and sha(exp["binary"]) == exp["binarySha256"], "reused verified binary")
    raw = read(source / preflop.CASE / "stdout.json")
    return baseline, raw


def config_checks(case, baseline_config_path):
    path = Path(case["config"])
    require(sha(path) == case["configSha256"], "case configuration hash")
    config = tomllib.loads(path.read_text(encoding="utf-8-sig"))
    baseline = tomllib.loads(Path(baseline_config_path).read_text(encoding="utf-8-sig"))
    require(config["solver"]["discount"] == case["discount"], "declared discount table")
    def without_discount(value):
        return {**value, "solver": {k: v for k, v in value["solver"].items() if k != "discount"}}
    require(without_discount(config) == without_discount(baseline), "only discount table may differ from baseline")
    require(config["solver"]["seed"] == case["trainingSeed"] == 0, "fixed training seed")
    return config


def model_checks(raw, baseline, case):
    # Full configured game/tree identity follows from identical parsed game,
    # abstraction and source; the output additionally checks all 16 public
    # node layouts. It does not contain an independent full-tree digest.
    for field in ("schemaVersion", "sourceRevision", "executableBlake3", "abstractionFingerprint",
                  "solverStateVersion", "threads", "nodes", "supportNodes", "coveragePrefixes", "endpointPrefixes"):
        require(raw[field] == baseline[field], "same executable/abstraction/public context: " + field)
    if case["discount"] == DISCOUNTS[0]:
        require(raw["configurationFingerprint"] == baseline["configurationFingerprint"], "no-discount model fingerprint")
    else:
        require(raw["configurationFingerprint"] != baseline["configurationFingerprint"], "periodic discount changes configuration identity")
    require(len(raw["diagnostics"]["support"]) == len(baseline["diagnostics"]["support"]) == 16,
            "complete public support layouts")
    for current, old in zip(raw["diagnostics"]["support"], baseline["diagnostics"]["support"], strict=True):
        require(pilot.public_layout(current) == pilot.public_layout(old), "unchanged public menu/layout")


def measurement_checks(run, exp, case, raw):
    folder, job_path = run / case["name"], run / (case["name"] + "-job.json")
    measurement, job = read(folder / "measurement.json"), read(job_path)
    require(measurement["schemaVersion"] == "solvers.average-sampling-measurement/v1"
            and measurement["exitCode"] == 0 and measurement["timedOut"] is False, "successful AverageSampling measurement")
    require(sha(job_path) == case["jobSha256"] == measurement["jobSha256"]
            and Path(measurement["job"]).resolve() == job_path.resolve()
            and measurement["arguments"] == job["arguments"], "literal measured job identity")
    require(sha(folder / "stdout.json") == measurement["stdoutSha256"], "raw output hash")
    require(Path(measurement["binary"]).resolve() == Path(exp["binary"]).resolve()
            and measurement["binarySha256"] == exp["binarySha256"], "measured reused binary")
    for field in ("sourceManifestSha256", "validationReport"):
        require(measurement[field] == job[field] == exp[field], "measurement identity: " + field)
    require(measurement["configSha256"] == job["configSha256"] == case["configSha256"], "measured configuration hash")
    require(measurement["sourceRevision"] == exp["baseRevision"]
            and measurement["timeoutSeconds"] == job["timeoutSeconds"] == case["timeoutSeconds"], "source revision/timeout")
    opts = options(job["arguments"])
    expected = {"--variant": "uniform-one", "--sweeps": "131072", "--threads": "8", "--memory": "8GiB",
                "--source-revision": exp["sourceManifestSha256"], "--evaluation-seeds": "101,202",
                "--evaluation-samples": "128", "--coverage-samples": "131072", "--endpoint-fit-samples": "65536",
                "--endpoint-fit-seed": "602", "--endpoint-samples": "131072", "--endpoint-seeds": "702,703",
                "--endpoint-min-fit-ess": "64", "--root-samples": "262144", "--root-seeds": "801,802"}
    for flag, value in expected.items():
        require(opts.get(flag) == [value], "literal diagnostic/training budget: " + flag)
    for flag, field in (("--node", "nodes"), ("--support-node", "nodes"),
                        ("--coverage-prefix", "coveragePrefixes"), ("--endpoint-prefix", "endpointPaths")):
        require(opts.get(flag) == exp[field], "literal public paths: " + flag)
    require(set(opts) == set(expected) | {"--config", "--cache-dir", "--node", "--support-node", "--coverage-prefix", "--endpoint-prefix"},
            "no undeclared job flags/artifacts")
    require(Path(opts["--config"][0]).resolve() == Path(case["config"]).resolve() == Path(raw["config"]).resolve(),
            "actual configuration location")
    return measurement


def learning_checks(raw, case):
    pilot.normalized_only(raw)
    result = raw["result"]
    require(result["variant"] == case["variant"] and result["threads"] == 8, "reported algorithm/threads")
    for field in ("executableBlake3", "effectiveConfigBlake3", "configurationFingerprint", "abstractionFingerprint"):
        digest(raw[field])
    digest(result["current_regret_fingerprint"])
    metrics = result["metrics"]
    require(metrics["sweeps"] == case["sweeps"] and metrics["traversals"] == case["sweeps"] * 6, "completed training budget")
    for key in ("infosets", "memory_bytes", "total_deal_attempts", "hand_updates"):
        count(metrics[key], 2**64 - 1, "training metric: " + key)
    require(metrics["total_deal_attempts"] >= metrics["traversals"]
            and math.isclose(metrics["mean_deal_attempts"], metrics["total_deal_attempts"] / metrics["traversals"], rel_tol=1e-12),
            "training deal accounting")
    require(len(metrics["average_positive_regret"]) == 6
            and all(math.isfinite(v) and v >= 0 for v in metrics["average_positive_regret"]), "diagnostic regret seats")


def support_history_checks(raw, exp):
    supports = raw["diagnostics"]["support"]
    require(raw["supportNodes"] == raw["nodes"] and len(supports) == len(raw["nodes"]) == 16, "sixteen support nodes")
    require([c["requested"] for c in raw["nodes"]] == exp["nodes"]
            and len({c["history"] for c in raw["nodes"]}) == 16, "ordered unique node contexts")
    output = []
    for context, support in zip(raw["nodes"], supports, strict=True):
        pilot.context_checks(context, support)
        output.append(dict(context=context, counts=pilot.support_checks(support), result=support))
    histories = raw["result"]["histories"]
    require(len(histories) == len(supports), "history export cardinality")
    for history, support in zip(histories, supports, strict=True):
        require(history["history"] == support["history"], "history export identity")
        stored = [row for row in support["rows"] if row["regrets"] is not None]
        require(len(history["strategies"]) == len(stored), "complete stored history rows")
        for row, ref in zip(history["strategies"], stored, strict=True):
            require(row["key"] == ref["key"], "history own-information key order")
            average = ref["average_strategy"]
            require(row["status"] == ("average-observed" if average is not None else "zero-average-mass-omitted"),
                    "history average source")
            require(row["actions"] == (None if average is None else [dict(action=a, probability=p)
                    for a, p in zip(support["action_labels"], average, strict=True)]), "history normalized policy agreement")
    return output


def ordinary_checks(raw, exp):
    result, schedule = raw["result"], exp["ordinarySchedule"]
    require([e["seed"] for e in result["evaluations"]] == schedule["seeds"]
            and [e["seed"] for e in result["coverage_evaluations"]] == schedule["seeds"], "ordinary seed/cardinality schedule")
    contexts = raw["coveragePrefixes"]
    require([c["requested"] for c in contexts] == exp["coveragePrefixes"], "coverage context order")
    for context in contexts:
        require(context == raw["nodes"][exp["nodes"].index(context["requested"])], "coverage/support context")
    for entry in result["evaluations"]:
        pilot.profile_checks(entry["result"], schedule["samples"], False)
    output = []
    for entry in result["coverage_evaluations"]:
        value, n = entry["result"], schedule["coverageSamples"]
        pilot.profile_checks(value["evaluation"], n, True)
        require(len(value["prefixes"]) == len(contexts), "coverage prefix cardinality")
        for prefix, context in zip(value["prefixes"], contexts, strict=True):
            require(hex_bytes(prefix["history"]) == context["history"], "coverage history")
            reached = count(prefix["reached_samples"], n, "reached worlds")
            require(set(prefix["trajectory_visits_by_street"]) == set(pilot.STREETS), "coverage street schema")
            totals = pilot.seat_coverage(prefix["candidate_policy_coverage"])
            for street, visits in prefix["trajectory_visits_by_street"].items():
                count(visits, reached, "trajectory count")
                decisions = totals[street]["decision"]
                require(decisions >= visits and (decisions > 0) == (visits > 0), "trajectory/decision consistency")
                if pilot.STREETS.index(street) < pilot.STREETS.index(context["street"]):
                    require(decisions == visits == 0, "coverage cannot precede prefix")
            require(prefix["trajectory_visits_by_street"][context["street"]] == reached, "endpoint street reach count")
            if reached == 0:
                require(all(v["decision"] == 0 for v in totals.values()), "unreached coverage has no visits")
            if context["requested"] == "root":
                require(reached == n and prefix["candidate_policy_coverage"] == value["evaluation"]["candidate_policy_coverage"],
                        "root baseline coverage equality")
            output.append(dict(seed=entry["seed"], context=context, samples=n, reachedSamples=reached,
                trajectoryVisitsByStreet=prefix["trajectory_visits_by_street"], sourceVisitsByStreet=totals,
                averageFractionByStreet={s: totals[s]["average_strategy"] / totals[s]["decision"]
                                         if totals[s]["decision"] else None for s in pilot.STREETS}))
    return output


def endpoint_checks(raw, exp, baseline):
    diagnostic, ds = raw["diagnostics"], exp["endpointSchedule"]
    supports = dict(zip(exp["nodes"], diagnostic["support"], strict=True))
    selected = [supports[path] for path in exp["endpointPaths"]]
    contexts = [raw["nodes"][exp["nodes"].index(path)] for path in exp["endpointPaths"]]
    require(raw["endpointPrefixes"] == contexts and len(diagnostic["endpoints"]) == 5
            and all(c["street"] == "preflop" for c in contexts), "five preflop endpoint contexts")
    require(diagnostic["config"] == dict(support_paths=[s["action_indices"] for s in diagnostic["support"]],
        endpoint_paths=[s["action_indices"] for s in selected],
        endpoint=dict(fit_samples=ds["fitSamples"], fit_seed=ds["fitSeed"], held_out_samples=ds["heldOutSamples"],
                      held_out_seeds=ds["heldOutSeeds"], min_fit_ess=ds["minFitEss"]),
        root_samples=exp["rootSchedule"]["samples"], root_seeds=exp["rootSchedule"]["seeds"]), "actual endpoint/root budgets")
    root_range = baseline["diagnostics"]["endpoints"][0]["proposal"]["root_range_fingerprint"]
    output = []
    for result, support, context in zip(diagnostic["endpoints"], selected, contexts, strict=True):
        selection = preflop.endpoint_checks(result, ds, support)
        preflop.proposal_checks(result["proposal"], result, root_range)
        require(hex_bytes(result["configuration_fingerprint"], 32) == raw["configurationFingerprint"]
                and hex_bytes(result["abstraction_fingerprint"], 32) == raw["abstractionFingerprint"], "endpoint model identity")
        output.append(dict(context=context, fitSelection=selection, result=result))
    require([r["seed"] for r in diagnostic["root_evaluations"]] == exp["rootSchedule"]["seeds"], "root seed/cardinality schedule")
    for root in diagnostic["root_evaluations"]:
        preflop.root_checks(root, exp["rootSchedule"], selected)
    return output


def endpoint_comparisons(current, baseline):
    require(len(current) == len(baseline) == 5, "comparison endpoint cardinality")
    output = []
    for new, old in zip(current, baseline, strict=True):
        require(new["context"] == old["context"], "comparison endpoint context")
        rows = []
        for held, reference in zip(new["result"]["held_out"], old["result"]["held_out"], strict=True):
            require(held["sampling"]["seed"] == reference["sampling"]["seed"], "comparison held-out seed schedule")
            rows.append(dict(seed=held["sampling"]["seed"], baselineGain=reference["gain"], gain=held["gain"],
                descriptiveGainMeanChange=(held["gain"]["mean"] - reference["gain"]["mean"]
                                           if held["gain"] is not None and reference["gain"] is not None else None),
                baselineRetainedWeight=reference["retained_key_weight_fraction"], retainedWeight=held["retained_key_weight_fraction"],
                baselineEss=reference["sampling"]["effective_sample_size"], ess=held["sampling"]["effective_sample_size"],
                baselineMaxWeight=reference["sampling"]["max_normalized_weight"], maxWeight=held["sampling"]["max_normalized_weight"],
                pairedEstimate=False))
        output.append(dict(context=new["context"], baselineRetainedBuckets=old["result"]["fit"]["retained_buckets"],
                           retainedBuckets=new["result"]["fit"]["retained_buckets"], heldOut=rows))
    return output


def summarize_case(run, exp, case, baseline_summary, baseline_raw):
    folder = run / case["name"]
    config_checks(case, baseline_raw["config"])
    raw = read(folder / "stdout.json")
    measurement = measurement_checks(run, exp, case, raw)
    model_checks(raw, baseline_raw, case)
    learning_checks(raw, case)
    for value in (raw["constructionElapsedSecs"], raw["elapsedSecs"], raw["result"]["solve_elapsed_secs"],
                  raw["diagnostics"]["elapsed_secs"], measurement["wallSeconds"], measurement["observedPeakWorkingSetBytes"]):
        number(value, "phase clock/memory observation", positive=True)
    require(raw["result"]["solve_elapsed_secs"] + raw["diagnostics"]["elapsed_secs"] <= raw["elapsedSecs"] + .01
            and raw["elapsedSecs"] + raw["constructionElapsedSecs"] <= measurement["wallSeconds"] + .01, "clock phase containment")
    support = support_history_checks(raw, exp)
    coverage = ordinary_checks(raw, exp)
    endpoints = endpoint_checks(raw, exp, baseline_raw)
    return dict(case=case["name"], variant=case["variant"], trainingSeed=case["trainingSeed"], sweeps=case["sweeps"],
                discount=case["discount"], measurement=measurement, constructionSeconds=raw["constructionElapsedSecs"],
                solveSeconds=raw["result"]["solve_elapsed_secs"], diagnosticsSeconds=raw["diagnostics"]["elapsed_secs"],
                configurationFingerprint=raw["configurationFingerprint"], currentRegretFingerprint=raw["result"]["current_regret_fingerprint"],
                metrics=raw["result"]["metrics"], support=support, histories=raw["result"]["histories"], endpoints=endpoints,
                ordinaryEvaluations=raw["result"]["evaluations"], coverageEvaluations=raw["result"]["coverage_evaluations"],
                coverageSummary=coverage, rootEvaluations=raw["diagnostics"]["root_evaluations"],
                baselineComparisons=endpoint_comparisons(endpoints, baseline_summary["endpoints"]),
                baselineDriverRatio=raw["result"]["solve_elapsed_secs"] / baseline_summary["solveSeconds"])


def cost_gate(cases, exp):
    require([case["case"] for case in cases] == list(CASES), "cost gate needs both complete arms")
    ratio = number(cases[1]["solveSeconds"], "periodic driver time", positive=True) / number(cases[0]["solveSeconds"], "control driver time", positive=True)
    limit = exp["costGate"]["maxPeriodicToNoDiscountDriverRatio"]
    return dict(periodicToNoDiscountDriverRatio=ratio, maximumRatio=limit, passed=ratio <= limit,
                basis="sweep-driver elapsed seconds, including discount scans; equal sweeps, not equal time")


def summarize(run, case_name=None):
    exp = read(run / "experiment.json")
    schedule_checks(exp)
    require(case_name is None or case_name in CASES, "unknown case")
    baseline_summary, baseline_raw = evidence_checks(exp)
    cases = [summarize_case(run, exp, case, baseline_summary, baseline_raw) for case in exp["cases"]
             if case_name is None or case["name"] == case_name]
    return dict(schemaVersion="solvers.preflop-discount-summary/v1", status="completed-pilot" if case_name is None else "completed-case",
                experiment=exp, experimentSha256=sha(run / "experiment.json"), cases=cases,
                sourceEvidence=dict(run=exp["sourceEvidenceRun"], sourceFiles=167, sourceManifestSha256=exp["sourceManifestSha256"],
                                    binarySha256=exp["binarySha256"], verificationSha256=exp["verificationSha256"],
                                    baselineSummarySha256=exp["baselineSummarySha256"], baselineSummaryByteRegeneration=True),
                baselineRootEvaluations=baseline_summary["rootEvaluations"],
                costGate=cost_gate(cases, exp) if case_name is None else None,
                periodicVersusNoDiscount=endpoint_comparisons(cases[1]["endpoints"], cases[0]["endpoints"]) if case_name is None else None,
                promotionAllowed=False,
                interpretation="Seed-zero learning pilot only. Each endpoint uses its own frozen fit table and all prefix weights, including unsupported keys. Profiles change conditional populations and proposal distributions: matching seed numbers do not establish paired worlds or a paired standard error. Mean differences are descriptive; retain ESS, candidate coverage, signed gains and separate absolute root reach. Discounted average_positive_regret is mechanically rescaled and is not a cross-arm quality score. Source policy coverage is not strategic quality. No default promotion or equilibrium claim follows from this pilot.")


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
