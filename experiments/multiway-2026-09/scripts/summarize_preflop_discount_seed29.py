"""Validate the fixed seed-29, no-discount-first preflop replication.

The frozen seed-11 validator recursively verifies seed zero. All three
training seeds remain separate descriptive evidence, without pooled worlds,
paired-difference errors, or a production recommendation. No solver executes.
"""
from __future__ import annotations

import argparse
from datetime import datetime
from pathlib import Path
import tomllib

import summarize_preflop_discount_seed as seed11
from summarize_preflop_discount import number, read, require, serialized, sha

discount, pilot = seed11.discount, seed11.pilot
CASES = ("seed29-s131072-no-discount", "seed29-s131072-periodic")
TEST_DEPENDENCIES = seed11.TEST_DEPENDENCIES
SHARED_FIELDS = ("baseRevision", "sourceManifestSha256", "sourceZipSha256", "binarySha256",
                 "verificationSha256", "runnerSha256", "threads", "memory", "nodes", "coveragePrefixes",
                 "endpointPaths", "endpointSchedule", "rootSchedule", "ordinarySchedule", "costGate")


def runtime_modules():
    return [seed11, *seed11.runtime_modules()]


def schedule_checks(exp):
    require(exp["trainingSeed"] == 29 and len(exp["cases"]) == 2, "fixed seed-twenty-nine pair")
    for case, name, definition in zip(exp["cases"], CASES, discount.DISCOUNTS, strict=True):
        require((case["name"], case["variant"], case["trainingSeed"], case["sweeps"], case["timeoutSeconds"])
                == (name, "uniform-one", 29, 131072, 1800) and case["discount"] == definition,
                "predeclared no-discount-first order/budget/discount")
    require(exp["threads"] == 8 and exp["memory"] == "8GiB", "fixed resources")
    require(exp["costGate"] == {"maxPeriodicToNoDiscountDriverRatio": 2.0}
            and exp["promotionAllowed"] is False and exp["cloudResourcesStarted"] is False
            and exp["broaderGoalComplete"] is False, "replication cost/promotion/cloud boundary")


def analysis_identity_checks(exp):
    require(sha(__file__) == exp["summarizerSha256"], "analysis identity")
    require(sha(Path(__file__).parent / "tests/test_summarize_preflop_discount_seed29.py")
            == exp["testScriptSha256"], "analysis test identity")
    require({Path(p).resolve() for p in exp["dependencies"]}
            == {Path(m.__file__).resolve() for m in runtime_modules()}, "complete eight-helper runtime dependency set")
    for path, checksum in exp["dependencies"].items():
        require(sha(path) == checksum, "frozen runtime dependency identity: " + path)
    expected = {Path(__file__).parent / "tests" / name for name in TEST_DEPENDENCIES}
    require({Path(p).resolve() for p in exp["testDependencies"]} == {p.resolve() for p in expected},
            "complete imported test-fixture dependency set")
    for path, checksum in exp["testDependencies"].items():
        require(sha(path) == checksum, "frozen test-fixture identity: " + path)


def evidence_checks(exp):
    analysis_identity_checks(exp)
    run11, run0 = Path(exp["priorReplicationRun"]), Path(exp["priorPilotRun"])
    for run, prefix in ((run11, "priorReplication"), (run0, "priorPilot")):
        require(sha(run / "experiment.json") == exp[prefix + "ExperimentSha256"], prefix + " experiment identity")
        require(sha(run / "summary.json") == exp[prefix + "SummarySha256"], prefix + " summary identity")
    # This frozen call regenerates and byte-checks seed zero as part of its
    # own evidence_checks. Do not rerun the same recursive validation twice.
    previous11 = seed11.summarize(run11)
    require(serialized(previous11) == (run11 / "summary.json").read_bytes(), "seed-eleven summary byte regeneration")
    exp11 = previous11["experiment"]
    require(Path(exp11["priorPilotRun"]).resolve() == run0.resolve()
            and exp11["priorPilotExperimentSha256"] == exp["priorPilotExperimentSha256"]
            and exp11["priorPilotSummarySha256"] == exp["priorPilotSummarySha256"],
            "seed-zero reference is the same recursively regenerated pilot")
    previous0 = read(run0 / "summary.json")
    require(previous0["experiment"] == read(run0 / "experiment.json")
            and previous0["experimentSha256"] == exp["priorPilotExperimentSha256"], "seed-zero embedded experiment identity")
    previous = {0: previous0, 11: previous11}
    raws = {}
    for training_seed, summary in previous.items():
        old_exp = summary["experiment"]
        for field in SHARED_FIELDS:
            require(exp[field] == old_exp[field], "same source/model/diagnostic schedule: " + field)
        require(Path(exp["sourceEvidenceRun"]).resolve() == Path(old_exp["sourceEvidenceRun"]).resolve(),
                "original source evidence location")
        require(Path(exp["binary"]).resolve() == Path(old_exp["binary"]).resolve(), "original binary location")
        require({case["trainingSeed"] for case in old_exp["cases"]} == {training_seed}, "prior training-seed identity")
        folder = run0 if training_seed == 0 else run11
        raws[training_seed] = {case["discount"]["kind"]: read(folder / case["name"] / "stdout.json")
                              for case in old_exp["cases"]}
    require(sha(exp["binary"]) == exp["binarySha256"], "same verified binary hash")
    return previous, raws


def corresponding_case(summary, case, training_seed):
    matches = [old for old in summary["experiment"]["cases"] if old["discount"] == case["discount"]]
    require(len(matches) == 1 and matches[0]["trainingSeed"] == training_seed, "unique corresponding prior arm")
    return matches[0]


def config_checks(case, previous):
    path = Path(case["config"])
    require(sha(path) == case["configSha256"], "declared configuration hash")
    config = tomllib.loads(path.read_text(encoding="utf-8-sig"))
    require(config["solver"]["seed"] == case["trainingSeed"] == 29, "actual new training seed")
    require(config["solver"]["discount"] == case["discount"], "actual new discount table")

    def without_seed(value):
        return {**value, "solver": {key: item for key, item in value["solver"].items() if key != "seed"}}

    for training_seed, summary in previous.items():
        old_case = corresponding_case(summary, case, training_seed)
        old_path = Path(old_case["config"])
        require(sha(old_path) == old_case["configSha256"], "prior configuration hash")
        old = tomllib.loads(old_path.read_text(encoding="utf-8-sig"))
        require(old["solver"]["seed"] == training_seed and old["solver"]["discount"] == case["discount"],
                "actual prior training seed/discount")
        require(without_seed(config) == without_seed(old), "only solver.seed may differ from either corresponding prior config")
    return config


def summarize_case(run, exp, case, previous, prior_raw):
    config_checks(case, previous)
    raw = read(run / case["name"] / "stdout.json")
    measurement = discount.measurement_checks(run, exp, case, raw)
    for values in prior_raw.values():
        seed11.model_checks(raw, values)
    discount.learning_checks(raw, case)
    for value in (raw["constructionElapsedSecs"], raw["elapsedSecs"], raw["result"]["solve_elapsed_secs"],
                  raw["diagnostics"]["elapsed_secs"], measurement["wallSeconds"], measurement["observedPeakWorkingSetBytes"]):
        number(value, "phase clock/memory observation", positive=True)
    require(raw["result"]["solve_elapsed_secs"] + raw["diagnostics"]["elapsed_secs"] <= raw["elapsedSecs"] + .01
            and raw["elapsedSecs"] + raw["constructionElapsedSecs"] <= measurement["wallSeconds"] + .01,
            "clock phase containment")
    support = discount.support_history_checks(raw, exp)
    coverage = discount.ordinary_checks(raw, exp)
    endpoints = discount.endpoint_checks(raw, exp, prior_raw[0][case["discount"]["kind"]])
    # Existing endpoint checks validate every proposal against the seed-zero
    # root ranges; explicitly retain the same population anchor for seed 11.
    for old in prior_raw[11].values():
        require(raw["diagnostics"]["endpoints"][0]["proposal"]["root_range_fingerprint"]
                == old["diagnostics"]["endpoints"][0]["proposal"]["root_range_fingerprint"], "same seed-eleven root ranges")
    comparisons = []
    for training_seed, summary in previous.items():
        old_case = corresponding_case(summary, case, training_seed)
        old = next(item for item in summary["cases"] if item["case"] == old_case["name"])
        comparisons.append(dict(trainingSeed=training_seed, case=old["case"],
                                endpoints=discount.endpoint_comparisons(endpoints, old["endpoints"]),
                                driverRatio=raw["result"]["solve_elapsed_secs"] / old["solveSeconds"]))
    return dict(case=case["name"], variant=case["variant"], trainingSeed=29, sweeps=case["sweeps"], discount=case["discount"],
                measurement=measurement, constructionSeconds=raw["constructionElapsedSecs"],
                solveSeconds=raw["result"]["solve_elapsed_secs"], diagnosticsSeconds=raw["diagnostics"]["elapsed_secs"],
                configurationFingerprint=raw["configurationFingerprint"], currentRegretFingerprint=raw["result"]["current_regret_fingerprint"],
                metrics=raw["result"]["metrics"], support=support, histories=raw["result"]["histories"], endpoints=endpoints,
                ordinaryEvaluations=raw["result"]["evaluations"], coverageEvaluations=raw["result"]["coverage_evaluations"],
                coverageSummary=coverage, rootEvaluations=raw["diagnostics"]["root_evaluations"], priorSeedComparisons=comparisons)


def pair_checks(cases):
    require(len(cases) == 2 and {case["case"] for case in cases} == set(CASES), "both complete replication arms")
    arms = {case["discount"]["kind"]: case for case in cases}
    require(set(arms) == {"none", "periodic"}, "unique discount arm identities")
    require(arms["none"]["configurationFingerprint"] != arms["periodic"]["configurationFingerprint"],
            "discount arms must have different configuration identities")
    return arms


def execution_order_checks(arms):
    first, second = arms["none"]["measurement"], arms["periodic"]["measurement"]
    def timestamp(value):
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        require(parsed.tzinfo is not None, "measured start must include a time zone")
        return parsed.timestamp()
    require(timestamp(first["startedUtc"]) + first["wallSeconds"] <= timestamp(second["startedUtc"]) + .01,
            "measured no-discount-first serial execution")


def cost_gate(cases, exp):
    arms = pair_checks(cases)
    ratio = number(arms["periodic"]["solveSeconds"], "periodic driver time", positive=True) / number(arms["none"]["solveSeconds"], "no-discount driver time", positive=True)
    limit = exp["costGate"]["maxPeriodicToNoDiscountDriverRatio"]
    return dict(periodicToNoDiscountDriverRatio=ratio, maximumRatio=limit, passed=ratio <= limit,
                numeratorCase=arms["periodic"]["case"], denominatorCase=arms["none"]["case"],
                basis="sweep-driver elapsed seconds; periodic/no-discount regardless of list order; equal sweeps, not equal time")


def endpoint_evidence(cases):
    """Keep every endpoint's fit/held-out evidence; raw old key rows stay linked."""
    return [dict(case=case["case"], discount=case["discount"], solveSeconds=case["solveSeconds"],
                 endpoints=[dict(context=e["context"], fitSelection=e["fitSelection"], fitSampling=e["result"]["fit"]["sampling"],
                                 heldOut=e["result"]["held_out"]) for e in case["endpoints"]],
                 rootEvaluations=case["rootEvaluations"],
                 support=[dict(context=s["context"], counts=s["counts"]) for s in case["support"]],
                 coverageSummary=case["coverageSummary"]) for case in cases]


def summarize(run, case_name=None):
    exp = read(run / "experiment.json")
    schedule_checks(exp)
    seed11.preexecution_checks(run, exp)
    require(case_name is None or case_name in CASES, "unknown case")
    previous, prior_raw = evidence_checks(exp)
    cases = [summarize_case(run, exp, case, previous, prior_raw) for case in exp["cases"]
             if case_name is None or case["name"] == case_name]
    arms = pair_checks(cases) if case_name is None else None
    if arms is not None:
        execution_order_checks(arms)
    evidence = [dict(trainingSeed=seed, cases=endpoint_evidence(summary["cases"])) for seed, summary in previous.items()]
    evidence.append(dict(trainingSeed=29, cases=endpoint_evidence(cases)))
    return dict(schemaVersion="solvers.preflop-discount-seed29-replication-summary/v1",
                status="completed-replication" if arms is not None else "completed-case", trainingSeed=29,
                experiment=exp, experimentSha256=sha(run / "experiment.json"), cases=cases,
                sourceEvidence=dict(run=exp["sourceEvidenceRun"], sourceFiles=167, sourceManifestSha256=exp["sourceManifestSha256"],
                                    binarySha256=exp["binarySha256"], verificationSha256=exp["verificationSha256"],
                                    preexecutionSha256=exp["preexecutionSha256"], substantivePreexecutionFieldsUnchanged=True,
                                    priorPilotRun=exp["priorPilotRun"], priorPilotExperimentSha256=exp["priorPilotExperimentSha256"],
                                    priorPilotSummarySha256=exp["priorPilotSummarySha256"], priorPilotSummaryByteRegeneration=True,
                                    priorReplicationRun=exp["priorReplicationRun"], priorReplicationExperimentSha256=exp["priorReplicationExperimentSha256"],
                                    priorReplicationSummarySha256=exp["priorReplicationSummarySha256"], priorReplicationSummaryByteRegeneration=True),
                costGate=cost_gate(cases, exp) if arms is not None else None,
                periodicVersusNoDiscount=discount.endpoint_comparisons(arms["periodic"]["endpoints"], arms["none"]["endpoints"]) if arms is not None else None,
                trainingSeedEvidence=evidence, promotionAllowed=False,
                interpretation="Seed-29 replication, no-discount first. Retain all five endpoints for seeds 0, 11 and 29, including signed held-out gains, fit exclusions, all-prefix denominators, root reach and source coverage. Prior complete key rows remain in the hashed prior summaries. Each profile has its own fitted table, conditional population and corrected proposal: shared evaluation seed labels do not create paired worlds or paired standard errors. No pooling, cross-arm quality ranking from average_positive_regret or support counts, default recommendation, or global-convergence claim. Three training seeds do not establish a promotion; a single-case output leaves the new pair incomplete. Same sweeps are not same compute; cost ratios are descriptive.")


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
