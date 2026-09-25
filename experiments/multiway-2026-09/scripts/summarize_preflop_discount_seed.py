"""Validate the fixed seed-11 replication of the preflop discount pilot.

Periodic runs first, reversing seed-zero execution order. Source evidence and
the complete prior pilot are verified recursively; no solver binary executes.
Cross-profile estimates remain descriptive and do not authorize promotion.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import tomllib

import summarize_preflop_discount as discount
from summarize_preflop_discount import number, read, require, serialized, sha

preflop, pilot = discount.preflop, discount.pilot
CASES = ("seed11-s131072-periodic", "seed11-s131072-no-discount")
TEST_DEPENDENCIES = ("test_summarize_preflop_discount.py", "test_summarize_preflop_endpoint.py")
RUNTIME_METADATA = {"status", "completedUtc", "summarizerSha256", "testScriptSha256",
                    "dependencies", "testDependencies", "preexecutionSha256"}


def runtime_modules():
    return [discount, preflop, pilot, preflop.balanced, preflop.endpoint_dependency,
            pilot.support_dependency, pilot.evidence_dependency]


def schedule_checks(exp):
    require(exp["trainingSeed"] == 11 and len(exp["cases"]) == 2, "fixed seed-eleven pair")
    for case, name, definition in zip(exp["cases"], CASES, reversed(discount.DISCOUNTS), strict=True):
        require((case["name"], case["variant"], case["trainingSeed"], case["sweeps"], case["timeoutSeconds"])
                == (name, "uniform-one", 11, 131072, 1800) and case["discount"] == definition,
                "predeclared reversed execution order/budget/discount")
    require(exp["threads"] == 8 and exp["memory"] == "8GiB", "fixed resources")
    require(exp["costGate"] == {"maxPeriodicToNoDiscountDriverRatio": 2.0}
            and exp["promotionAllowed"] is False and exp["cloudResourcesStarted"] is False,
            "replication cost/promotion/cloud boundary")


def analysis_identity_checks(exp):
    require(sha(__file__) == exp["summarizerSha256"], "analysis identity")
    require(sha(Path(__file__).parent / "tests/test_summarize_preflop_discount_seed.py")
            == exp["testScriptSha256"], "analysis test identity")
    require({Path(path).resolve() for path in exp["dependencies"]}
            == {Path(module.__file__).resolve() for module in runtime_modules()}, "complete seven-helper runtime dependency set")
    for path, checksum in exp["dependencies"].items():
        require(sha(path) == checksum, "frozen runtime dependency identity: " + path)
    expected_tests = {Path(__file__).parent / "tests" / name for name in TEST_DEPENDENCIES}
    require({Path(path).resolve() for path in exp["testDependencies"]}
            == {path.resolve() for path in expected_tests}, "complete imported test-fixture dependency set")
    for path, checksum in exp["testDependencies"].items():
        require(sha(path) == checksum, "frozen test-fixture identity: " + path)


def preexecution_checks(run, exp):
    path = run / "experiment-preexecution.json"
    require(sha(path) == exp["preexecutionSha256"], "immutable preexecution snapshot hash")
    before = read(path)
    require({key: value for key, value in exp.items() if key not in RUNTIME_METADATA}
            == {key: value for key, value in before.items() if key not in RUNTIME_METADATA},
            "all substantive fields must match preexecution snapshot")


def evidence_checks(exp):
    analysis_identity_checks(exp)
    prior_run = Path(exp["priorPilotRun"])
    require(sha(prior_run / "experiment.json") == exp["priorPilotExperimentSha256"], "prior pilot experiment identity")
    require(sha(prior_run / "summary.json") == exp["priorPilotSummarySha256"], "prior pilot summary identity")
    prior = discount.summarize(prior_run)
    require(serialized(prior) == (prior_run / "summary.json").read_bytes(), "prior pilot summary byte regeneration")
    previous_exp = prior["experiment"]
    for field in ("baseRevision", "sourceManifestSha256", "sourceZipSha256", "binarySha256", "verificationSha256",
                  "runnerSha256", "threads", "memory", "nodes", "coveragePrefixes", "endpointPaths", "endpointSchedule",
                  "rootSchedule", "ordinarySchedule", "costGate"):
        require(exp[field] == previous_exp[field], "same source/model/diagnostic schedule: " + field)
    require(Path(exp["sourceEvidenceRun"]).resolve() == Path(previous_exp["sourceEvidenceRun"]).resolve(), "original source evidence location")
    require(Path(exp["binary"]).resolve() == Path(previous_exp["binary"]).resolve()
            and sha(exp["binary"]) == exp["binarySha256"], "same verified binary location/hash")
    raw = {case["discount"]["kind"]: read(prior_run / case["name"] / "stdout.json") for case in previous_exp["cases"]}
    return prior, raw


def corresponding_prior_case(prior, case):
    matches = [old for old in prior["experiment"]["cases"] if old["discount"] == case["discount"]]
    require(len(matches) == 1 and matches[0]["trainingSeed"] == 0, "unique corresponding seed-zero arm")
    return matches[0]


def config_checks(case, prior_case):
    path, old_path = Path(case["config"]), Path(prior_case["config"])
    require(sha(path) == case["configSha256"] and sha(old_path) == prior_case["configSha256"], "declared configuration hashes")
    config = tomllib.loads(path.read_text(encoding="utf-8-sig"))
    old = tomllib.loads(old_path.read_text(encoding="utf-8-sig"))
    require(config["solver"]["seed"] == case["trainingSeed"] == 11
            and old["solver"]["seed"] == prior_case["trainingSeed"] == 0, "actual new/prior training seed")
    require(config["solver"]["discount"] == old["solver"]["discount"] == case["discount"] == prior_case["discount"],
            "corresponding discount table")
    def without_seed(value):
        return {**value, "solver": {key: item for key, item in value["solver"].items() if key != "seed"}}
    require(without_seed(config) == without_seed(old), "only solver.seed may differ from corresponding seed-zero config")
    return config


def model_checks(raw, prior_raw):
    # Seed and discount both belong to configurationFingerprint. Structural
    # configuration plus shared source fixes the tree; exported menus add a
    # direct check, but the output carries no separate full-tree digest.
    require(len(prior_raw) == 2, "both prior model identities")
    for old in prior_raw.values():
        for field in ("schemaVersion", "sourceRevision", "executableBlake3", "abstractionFingerprint", "solverStateVersion",
                      "threads", "nodes", "supportNodes", "coveragePrefixes", "endpointPrefixes"):
            require(raw[field] == old[field], "same executable/abstraction/public context: " + field)
        require(raw["configurationFingerprint"] != old["configurationFingerprint"], "configuration identity must change across training seeds")
        supports, previous = raw["diagnostics"]["support"], old["diagnostics"]["support"]
        require(len(supports) == len(previous) == 16, "complete public support layouts")
        for current, reference in zip(supports, previous, strict=True):
            require(pilot.public_layout(current) == pilot.public_layout(reference), "unchanged public menu/layout")


def summarize_case(run, exp, case, prior, prior_raw):
    old_case = corresponding_prior_case(prior, case)
    config_checks(case, old_case)
    raw = read(run / case["name"] / "stdout.json")
    measurement = discount.measurement_checks(run, exp, case, raw)
    model_checks(raw, prior_raw)
    discount.learning_checks(raw, case)
    for value in (raw["constructionElapsedSecs"], raw["elapsedSecs"], raw["result"]["solve_elapsed_secs"],
                  raw["diagnostics"]["elapsed_secs"], measurement["wallSeconds"], measurement["observedPeakWorkingSetBytes"]):
        number(value, "phase clock/memory observation", positive=True)
    require(raw["result"]["solve_elapsed_secs"] + raw["diagnostics"]["elapsed_secs"] <= raw["elapsedSecs"] + .01
            and raw["elapsedSecs"] + raw["constructionElapsedSecs"] <= measurement["wallSeconds"] + .01, "clock phase containment")
    support = discount.support_history_checks(raw, exp)
    coverage = discount.ordinary_checks(raw, exp)
    endpoints = discount.endpoint_checks(raw, exp, prior_raw[case["discount"]["kind"]])
    previous = next(old for old in prior["cases"] if old["case"] == old_case["name"])
    return dict(case=case["name"], variant=case["variant"], trainingSeed=11, sweeps=case["sweeps"], discount=case["discount"],
                measurement=measurement, constructionSeconds=raw["constructionElapsedSecs"],
                solveSeconds=raw["result"]["solve_elapsed_secs"], diagnosticsSeconds=raw["diagnostics"]["elapsed_secs"],
                configurationFingerprint=raw["configurationFingerprint"], currentRegretFingerprint=raw["result"]["current_regret_fingerprint"],
                metrics=raw["result"]["metrics"], support=support, histories=raw["result"]["histories"], endpoints=endpoints,
                ordinaryEvaluations=raw["result"]["evaluations"], coverageEvaluations=raw["result"]["coverage_evaluations"],
                coverageSummary=coverage, rootEvaluations=raw["diagnostics"]["root_evaluations"],
                correspondingSeedZeroCase=old_case["name"], seedZeroComparisons=discount.endpoint_comparisons(endpoints, previous["endpoints"]),
                seedZeroDriverRatio=raw["result"]["solve_elapsed_secs"] / previous["solveSeconds"])


def pair_checks(cases):
    require(len(cases) == 2 and {case["case"] for case in cases} == set(CASES), "both complete replication arms")
    arms = {case["discount"]["kind"]: case for case in cases}
    require(set(arms) == {"none", "periodic"}, "unique discount arm identities")
    require(arms["none"]["configurationFingerprint"] != arms["periodic"]["configurationFingerprint"],
            "discount arms must have different configuration identities")
    return arms


def cost_gate(cases, exp):
    arms = pair_checks(cases)
    ratio = number(arms["periodic"]["solveSeconds"], "periodic driver time", positive=True) / number(arms["none"]["solveSeconds"], "no-discount driver time", positive=True)
    limit = exp["costGate"]["maxPeriodicToNoDiscountDriverRatio"]
    return dict(periodicToNoDiscountDriverRatio=ratio, maximumRatio=limit, passed=ratio <= limit,
                numeratorCase=arms["periodic"]["case"], denominatorCase=arms["none"]["case"],
                basis="sweep-driver elapsed seconds; periodic/no-discount regardless of execution order; equal sweeps, not equal time")


def summarize(run, case_name=None):
    exp = read(run / "experiment.json")
    schedule_checks(exp)
    preexecution_checks(run, exp)
    require(case_name is None or case_name in CASES, "unknown case")
    prior, prior_raw = evidence_checks(exp)
    cases = [summarize_case(run, exp, case, prior, prior_raw) for case in exp["cases"]
             if case_name is None or case["name"] == case_name]
    arms = pair_checks(cases) if case_name is None else None
    return dict(schemaVersion="solvers.preflop-discount-seed-replication-summary/v1",
                status="completed-replication" if case_name is None else "completed-case", trainingSeed=11,
                experiment=exp, experimentSha256=sha(run / "experiment.json"), cases=cases,
                sourceEvidence=dict(run=exp["sourceEvidenceRun"], sourceFiles=167, sourceManifestSha256=exp["sourceManifestSha256"],
                                    binarySha256=exp["binarySha256"], verificationSha256=exp["verificationSha256"],
                                    preexecutionSha256=exp["preexecutionSha256"], substantivePreexecutionFieldsUnchanged=True,
                                    priorPilotRun=exp["priorPilotRun"], priorPilotExperimentSha256=exp["priorPilotExperimentSha256"],
                                    priorPilotSummarySha256=exp["priorPilotSummarySha256"], priorPilotSummaryByteRegeneration=True),
                costGate=cost_gate(cases, exp) if arms is not None else None,
                periodicVersusNoDiscount=discount.endpoint_comparisons(arms["periodic"]["endpoints"], arms["none"]["endpoints"]) if arms is not None else None,
                seedZeroPeriodicVersusNoDiscount=prior["periodicVersusNoDiscount"],
                seedZeroRootEvaluations=[dict(case=case["case"], result=case["rootEvaluations"]) for case in prior["cases"]],
                promotionAllowed=False,
                interpretation="One additional training seed at the fixed budget. Periodic-first execution reverses seed-zero order; clock ratios are descriptive. Configuration fingerprints differ with both seed and discount, while source, executable, structural game, abstraction and public menus stay fixed. Each profile has its own fitted table, conditional population and proposal: cross-profile and cross-training-seed mean changes have no paired standard error, and the held-out seeds are evaluation repetitions, not additional training seeds. Preserve signed gains, all-prefix denominators, per-key eligibility, ESS and separate root reach. Support counts and discounted average_positive_regret are not cross-arm quality scores. No pooling, default promotion or global-convergence claim follows from two training seeds.")


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
