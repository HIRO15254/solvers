"""Summarize retained Simple K32 average-sampling research measurements.

This read-only calculation uses explicit per-node bucket denominators. Dispersion
uses only buckets observed in all compared runs and reports that intersection;
it is not an estimate of isolated averaging variance or equilibrium error.
The default fixed phase requires identical sweep/progress/regret state. Explicit
--control-phase and --research-phase selectors permit calibrated sweep budgets
while retaining every other model, source, context and evaluation check.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import statistics
import tomllib
from pathlib import Path

from gcp_average_sampling_pilot import identity, validate_output, validate_pair

STREETS = ("preflop", "flop", "turn", "river")
VARIANTS = ("uniform-one", "enumerate-first-opponent")
SOURCES = ("decision", "stored_strategy", "average_strategy", "regret_fallback", "uniform_fallback", "current_strategy")


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def nonnegative_int(value, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"invalid {label}")
    return value


def finite_number(value, label: str, *, positive=False):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0 or (positive and value == 0):
        raise ValueError(f"invalid {label}")
    return value


def history_hex(value) -> str:
    if not isinstance(value, list) or len(value) != 16 or any(type(x) is not int or not 0 <= x <= 255 for x in value):
        raise ValueError("invalid history key")
    return bytes(value).hex()


def job_options(arguments: list) -> dict:
    if len(arguments) % 2:
        raise ValueError("expected literal key/value research arguments")
    options = {}
    for name, value in zip(arguments[::2], arguments[1::2]):
        if not isinstance(name, str) or not name.startswith("--") or not isinstance(value, str):
            raise ValueError("invalid research argument")
        if name in options and name not in ("--node", "--coverage-prefix"):
            raise ValueError(f"duplicate scalar argument {name}")
        options.setdefault(name, []).append(value)
    return options


def validate_execution(directory: Path, output: dict, execution: dict) -> tuple[dict, dict]:
    if execution.get("schemaVersion") != "solvers.average-sampling-measurement/v1" or execution.get("exitCode") != 0 or execution.get("timedOut") is not False:
        raise ValueError(f"incomplete measurement: {directory}")
    if output.get("schemaVersion") != "solvers.multiway-average-sampling-research/v1":
        raise ValueError("unexpected research schema")
    for path, expected, label in [
        (directory / "stdout.json", execution["stdoutSha256"], "output"),
        (Path(execution["binary"]), execution["binarySha256"], "binary"),
        (Path(execution["job"]), execution["jobSha256"], "job"),
        (directory.parent / "source-manifest.json", execution["sourceManifestSha256"], "source manifest"),
    ]:
        if sha(path) != expected:
            raise ValueError(f"{label} hash mismatch")
    job = read(Path(execution["job"]))
    for name in ("arguments", "configSha256", "sourceManifestSha256", "timeoutSeconds"):
        if job[name] != execution[name]:
            raise ValueError(f"measurement/job {name} mismatch")
    options = job_options(execution["arguments"])
    config_path = Path(options["--config"][0])
    if sha(config_path) != execution["configSha256"]:
        raise ValueError("config hash mismatch")
    config = tomllib.loads(config_path.read_text(encoding="utf-8-sig"))
    abstraction = config["game"]["abstraction"]
    if (config["game"]["information"]["recall"] != "current-street"
            or abstraction["kind"] != "ehs2-percentile"
            or abstraction["buckets"] != {street: 32 for street in STREETS[1:]}
            or set(abstraction) != {"kind", "buckets"}):
        raise ValueError("config does not match fixed Simple K32 bucket denominator")
    if output["sourceRevision"] != execution["sourceManifestSha256"] or options["--source-revision"] != [output["sourceRevision"]]:
        raise ValueError("source manifest identity mismatch")
    validate_output(output, variant=options["--variant"][0], seed=config["solver"]["seed"],
                    config=config_path, source_revision=output["sourceRevision"],
                    threads=int(options["--threads"][0]), sweeps=int(options["--sweeps"][0]))
    if output["result"]["variant"] not in VARIANTS:
        raise ValueError("unknown research variant")
    for value, label in [(output["elapsedSecs"], "research time"),
                         (output["constructionElapsedSecs"], "construction time"),
                         (output["result"]["solve_elapsed_secs"], "solve time"),
                         (execution["wallSeconds"], "wall time")]:
        finite_number(value, label, positive=True)
    if output["result"]["solve_elapsed_secs"] > output["elapsedSecs"] or output["elapsedSecs"] + output["constructionElapsedSecs"] > execution["wallSeconds"] + 0.01:
        raise ValueError("inconsistent timing phases")
    nonnegative_int(execution["observedPeakWorkingSetBytes"], "peak working set")
    return config, options


def validate_context(ctx: dict, players: int) -> None:
    if ctx["street"] not in STREETS or not 0 <= nonnegative_int(ctx["actor"], "actor") < players:
        raise ValueError("invalid node street/actor")
    if not 1 <= nonnegative_int(ctx["activeOpponents"], "active opponents") < players:
        raise ValueError("invalid node active opponents")
    value = ctx["history"]
    if not isinstance(value, str) or len(value) != 32 or any(c not in "0123456789abcdef" for c in value):
        raise ValueError("invalid context history")


def rows_by_bucket(history: dict, ctx: dict) -> dict:
    rows = {}
    menu = None
    street = STREETS.index(ctx["street"])
    count = 169 if street == 0 else 32
    if history_hex(history["history"]) != ctx["history"]:
        raise ValueError("node history mismatch")
    for row in history["strategies"]:
        key = row["key"]
        if (history_hex(key["history"]) != ctx["history"] or key["player"] != ctx["actor"]
                or key["street"] != street or key["active_opponents"] != ctx["activeOpponents"]):
            raise ValueError("row InfoKey differs from node context")
        path = key["bucket_path"]
        if len(path) != 4 or any(value != 2**32 - 1 for i, value in enumerate(path) if i != street):
            raise ValueError("row is not a current-street bucket")
        bucket = nonnegative_int(path[street], "bucket")
        if bucket >= count:
            raise ValueError("export does not match Simple K32 bucket denominator")
        if bucket in rows:
            raise ValueError("duplicate bucket in current-street export")
        rows[bucket] = row
        actions = row["actions"]
        if row["status"] == "average-observed":
            if not actions:
                raise ValueError("invalid observed average row")
            for action in actions:
                finite_number(action["probability"], "action probability")
            labels = [action["action"] for action in actions]
            if any(not isinstance(label, str) or not label for label in labels) or len(set(labels)) != len(labels):
                raise ValueError("invalid or duplicate action labels")
            if menu is not None and menu != labels:
                raise ValueError("action menu differs across node buckets")
            menu = labels
            if not math.isclose(sum(a["probability"] for a in actions), 1.0, abs_tol=2e-6):
                raise ValueError("average row does not normalize")
        elif row["status"] != "zero-average-mass-omitted" or actions is not None:
            raise ValueError("unknown row status or mislabeled fallback")
    return rows


def validate_seat_coverage(seats: list, players: int) -> dict:
    if len(seats) != players:
        raise ValueError("coverage seat count mismatch")
    totals = {street: {name: 0 for name in SOURCES} for street in STREETS}
    for seat in seats:
        for name in SOURCES:
            total = nonnegative_int(seat[name + "_visits"], "visit count")
            values = seat[name + "_visits_by_street"]
            if set(values) != set(STREETS):
                raise ValueError("coverage street fields mismatch")
            if sum(nonnegative_int(values[s], "street visit count") for s in STREETS) != total:
                raise ValueError("total coverage differs from street sum")
            for street in STREETS:
                totals[street][name] += values[street]
        for street in STREETS:
            counts = {name: seat[name + "_visits_by_street"][street] for name in SOURCES}
            if (counts["stored_strategy"] != counts["average_strategy"] + counts["regret_fallback"] + counts["current_strategy"]
                    or counts["decision"] != counts["stored_strategy"] + counts["uniform_fallback"]):
                raise ValueError("policy sources do not partition decisions")
            if counts["current_strategy"]:
                raise ValueError("average research profile unexpectedly requests current policy")
    return totals


def validate_profile(profile: dict, samples: int, players: int, *, baseline: bool) -> None:
    if profile["samples"] != samples or samples < 2 or nonnegative_int(profile["total_deal_attempts"], "deal attempts") < samples:
        raise ValueError("profile sample budget mismatch")
    validate_seat_coverage(profile["candidate_policy_coverage"], players)
    estimates = [profile["seats"]]
    gain = profile["deviation_gain_lower_bound"]
    if baseline != (gain is None):
        raise ValueError("wrong deviation-gain availability")
    if gain is not None:
        estimates.append(gain)
    for values in estimates:
        if len(values) != players:
            raise ValueError("profile seat count mismatch")
        for value in values:
            if not math.isfinite(value["mean"]) or len(value["ci95"]) != 2 or not all(math.isfinite(x) for x in value["ci95"]):
                raise ValueError("invalid profile estimate")
            finite_number(value["stderr"], "profile standard error")
            if not value["ci95"][0] <= value["mean"] <= value["ci95"][1]:
                raise ValueError("profile confidence interval is unordered")


def summarize_run(directory: Path) -> tuple[dict, dict]:
    output = read(directory / "stdout.json")
    execution = read(directory / "measurement.json")
    config, options = validate_execution(directory, output, execution)
    players = config["game"]["seat_count"]
    result = output["result"]
    contexts = output["nodes"]
    histories = result["histories"]
    if len(contexts) != len(histories):
        raise ValueError("node context count mismatch")
    if [ctx["requested"] for ctx in contexts] != options.get("--node", ["root"]):
        raise ValueError("node request differs from job")
    coverage_contexts = output.get("coveragePrefixes", [])
    if [ctx["requested"] for ctx in coverage_contexts] != options.get("--coverage-prefix", []):
        raise ValueError("prefix request differs from job")
    if len({ctx["history"] for ctx in coverage_contexts}) != len(coverage_contexts):
        raise ValueError("duplicate prefix histories")
    for ctx in contexts + coverage_contexts:
        validate_context(ctx, players)
    node_rows = []
    for ctx, history in zip(contexts, histories):
        rows = rows_by_bucket(history, ctx)
        count = 169 if ctx["street"] == "preflop" else 32
        if any(bucket < 0 or bucket >= count for bucket in rows):
            raise ValueError("export does not match Simple K32 bucket denominator")
        positive = sum(row["status"] == "average-observed" for row in rows.values())
        node_rows.append({**ctx, "bucketDenominator": count, "averageObserved": positive,
                          "touchedZeroAverage": len(rows) - positive, "untouched": count - len(rows)})
    coverage = []
    seeds = [int(seed) for seed in options["--evaluation-seeds"][0].split(",")]
    if not seeds or len(set(seeds)) != len(seeds) or any(seed < 0 for seed in seeds):
        raise ValueError("invalid evaluation seed schedule")
    if [value["seed"] for value in result["evaluations"]] != seeds:
        raise ValueError("ordinary evaluation seed schedule mismatch")
    for evaluation in result["evaluations"]:
        validate_profile(evaluation["result"], int(options["--evaluation-samples"][0]), players, baseline=False)
    if [value["seed"] for value in result["coverage_evaluations"]] != (seeds if coverage_contexts else []):
        raise ValueError("coverage evaluation seed schedule mismatch")
    for evaluation in result["coverage_evaluations"]:
        nested = evaluation["result"]
        samples = int(options.get("--coverage-samples", options["--evaluation-samples"])[0])
        validate_profile(nested["evaluation"], samples, players, baseline=True)
        if len(coverage_contexts) != len(nested["prefixes"]):
            raise ValueError("prefix context count mismatch")
        for ctx, prefix in zip(coverage_contexts, nested["prefixes"]):
            if history_hex(prefix["history"]) != ctx["history"]:
                raise ValueError("prefix history mismatch")
            reached = nonnegative_int(prefix["reached_samples"], "reached samples")
            if reached > samples:
                raise ValueError("prefix reach exceeds evaluation samples")
            if ctx["history"] == "00" * 16:
                if prefix["candidate_policy_coverage"] != nested["evaluation"]["candidate_policy_coverage"]:
                    raise ValueError("root coverage differs from unconditional coverage")
                if reached != samples:
                    raise ValueError("root did not reach every sample")
            totals = validate_seat_coverage(prefix["candidate_policy_coverage"], players)
            by_street = {}
            for index, street in enumerate(STREETS):
                counts = totals[street]
                trajectories = nonnegative_int(prefix["trajectory_visits_by_street"][street], "decision trajectories")
                if trajectories > reached or trajectories > counts["decision"] or bool(trajectories) != bool(counts["decision"]):
                    raise ValueError("invalid decision trajectory denominator")
                if index < STREETS.index(ctx["street"]) and trajectories:
                    raise ValueError("prefix includes decisions before its street")
                by_street[street] = {**counts, "averageFraction": counts["average_strategy"] / counts["decision"] if counts["decision"] else None,
                                    "trajectories": trajectories}
            coverage.append({"evaluationSeed": evaluation["seed"], **ctx,
                             "reached": prefix["reached_samples"], "byStreet": by_street})
    return {"directory": str(directory), "outputSha256": execution["stdoutSha256"],
            "binarySha256": execution["binarySha256"], "solveSeconds": result["solve_elapsed_secs"],
            "constructionSeconds": output["constructionElapsedSecs"], "researchSeconds": output["elapsedSecs"],
            "wallSeconds": execution["wallSeconds"], "peakWorkingSetBytes": execution["observedPeakWorkingSetBytes"],
            "sweeps": result["metrics"]["sweeps"], "currentRegretFingerprint": result["current_regret_fingerprint"],
            "trainingSeed": config["solver"]["seed"], "variant": result["variant"],
            "nodes": node_rows, "coverage": coverage, "evaluations": result["evaluations"]}, output


def comparison_phases(phase: str | None, control_phase: str | None, research_phase: str | None) -> tuple[dict, bool]:
    calibrated = control_phase is not None or research_phase is not None
    if calibrated:
        if phase is not None or not control_phase or not research_phase:
            raise ValueError("calibrated comparison requires both phase selectors and excludes --phase")
        return dict(zip(VARIANTS, (control_phase, research_phase))), True
    phase = "fixed" if phase is None else phase
    if not phase:
        raise ValueError("phase must not be empty")
    return dict.fromkeys(VARIANTS, phase), False


def summarize(root: Path, phase: str | None, seeds: list[int], *,
              control_phase: str | None = None, research_phase: str | None = None) -> dict:
    phases, calibrated = comparison_phases(phase, control_phase, research_phase)
    if not seeds or len(set(seeds)) != len(seeds) or any(type(seed) is not int or seed < 0 for seed in seeds):
        raise ValueError("training seeds must be nonempty, unique nonnegative integers")
    runs, outputs, pairs = {}, {}, []
    reference = None
    model_config = None
    node_menus = None
    for seed in seeds:
        for variant in VARIANTS:
            key = f"{phases[variant]}-{seed}-{variant}"
            runs[key], outputs[key] = summarize_run(root / key)
            if runs[key]["trainingSeed"] != seed or runs[key]["variant"] != variant:
                raise ValueError("run directory seed/variant mismatch")
            output = outputs[key]
            comparison = {name: output[name] for name in ("sourceRevision", "executableBlake3", "solverStateVersion", "threads", "abstractionFingerprint", "nodes")}
            comparison["coveragePrefixes"] = output.get("coveragePrefixes", [])
            comparison["binarySha256"] = runs[key]["binarySha256"]
            if not calibrated:
                comparison["sweeps"] = runs[key]["sweeps"]
            comparison["evaluationSchedule"] = [(e["seed"], e["result"]["samples"]) for e in output["result"]["evaluations"]]
            comparison["coverageSchedule"] = [(e["seed"], e["result"]["evaluation"]["samples"]) for e in output["result"]["coverage_evaluations"]]
            config = tomllib.loads(Path(output["config"]).read_text(encoding="utf-8-sig"))
            config["solver"].pop("seed")
            if reference is None:
                reference, model_config = comparison, config
                node_menus = [None] * len(output["nodes"])
            elif comparison != reference or config != model_config:
                raise ValueError("cross-run context, model or measurement schedule mismatch")
            for index, history in enumerate(output["result"]["histories"]):
                observed = next((row for row in history["strategies"] if row["status"] == "average-observed"), None)
                if observed is not None:
                    menu = [action["action"] for action in observed["actions"]]
                    if node_menus[index] is not None and menu != node_menus[index]:
                        raise ValueError("cross-run action menu mismatch")
                    node_menus[index] = menu
        first, second = (f"{phases[v]}-{seed}-{v}" for v in VARIANTS)
        if calibrated:
            control_identity, research_identity = identity(outputs[first]), identity(outputs[second])
            # Only an explicit calibrated comparison relaxes progress/regret
            # equality. Source, executable, game/config/abstraction and threads
            # still bind each same-seed pair, including effective config bytes.
            for name in ("sweeps", "progress"):
                control_identity.pop(name)
                research_identity.pop(name)
            if control_identity != research_identity:
                raise ValueError("calibrated paired identity mismatch")
        else:
            validate_pair(outputs[first], outputs[second])
        pairs.append({"seed": seed,
                      "controlRun": first, "researchRun": second,
                      "controlSweeps": runs[first]["sweeps"], "researchSweeps": runs[second]["sweeps"],
                      "controlSolveSeconds": runs[first]["solveSeconds"], "researchSolveSeconds": runs[second]["solveSeconds"],
                      "controlRegretFingerprint": runs[first]["currentRegretFingerprint"],
                      "researchRegretFingerprint": runs[second]["currentRegretFingerprint"],
                      "regretFingerprintEqual": runs[first]["currentRegretFingerprint"] == runs[second]["currentRegretFingerprint"],
                      "solveTimeRatio": runs[second]["solveSeconds"] / runs[first]["solveSeconds"],
                      "wallTimeRatio": runs[second]["wallSeconds"] / runs[first]["wallSeconds"]})
    dispersion = []
    if len(seeds) >= 2:
        values = list(outputs.values())
        for index, ctx in enumerate(values[0]["nodes"]):
            mappings = {key: rows_by_bucket(value["result"]["histories"][index], ctx) for key, value in outputs.items()}
            common = set.intersection(*(set(b for b, row in rows.items() if row["status"] == "average-observed") for rows in mappings.values()))
            for bucket in common:
                menus = [[a["action"] for a in rows[bucket]["actions"]] for rows in mappings.values()]
                if any(menu != menus[0] for menu in menus):
                    raise ValueError("cross-variant action menu mismatch")
            sigmas = {}
            for variant in VARIANTS:
                cells = []
                for bucket in sorted(common):
                    rows = [mappings[f"{phases[variant]}-{seed}-{variant}"][bucket]["actions"] for seed in seeds]
                    labels = [a["action"] for a in rows[0]]
                    if any([a["action"] for a in row] != labels for row in rows):
                        raise ValueError("action menu mismatch")
                    for action in range(len(labels)):
                        cells.append(statistics.stdev(row[action]["probability"] for row in rows))
                sigmas[variant] = statistics.mean(cells) if cells else None
            dispersion.append({**ctx, "commonObservedBuckets": len(common), "trainingSeeds": seeds,
                               "meanActionCellSampleStdDev": sigmas})
    interpretation = "Coverage uses each variant's own baseline reach. Common-bucket dispersion includes regret-learning variation and excludes missing/zero-mass buckets; fixed-denominator node counts report that missingness. No convergence or equilibrium certificate."
    if calibrated:
        interpretation += " Calibrated controls may have different sweep/progress/regret states; actual solve-time ratios report the remaining compute mismatch rather than asserting exact equal time."
    return {"schema": "solvers.average-depth-summary/v1", "phase": None if calibrated else phases[VARIANTS[0]], "seeds": seeds,
            "comparisonMode": "calibrated-compute" if calibrated else "fixed-sweeps",
            "controlPhase": phases[VARIANTS[0]], "researchPhase": phases[VARIANTS[1]],
            "runs": runs, "pairedComparisons": pairs, "dispersion": dispersion,
            "interpretation": interpretation}


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    selectors = parser.add_mutually_exclusive_group()
    selectors.add_argument("--phase", help="same-sweep paired phase (default: fixed)")
    selectors.add_argument("--control-phase", help="explicit calibrated UniformOne phase; requires --research-phase")
    parser.add_argument("--research-phase", help="EnumerateFirst phase for the calibrated comparison")
    parser.add_argument("--seeds", default="0,11,29")
    parser.add_argument("--output", type=Path, required=True)
    return parser


if __name__ == "__main__":
    parser = argument_parser()
    args = parser.parse_args()
    try:
        comparison_phases(args.phase, args.control_phase, args.research_phase)
    except ValueError as error:
        parser.error(str(error))
    summary = summarize(args.root, args.phase, [int(x) for x in args.seeds.split(",")],
                        control_phase=args.control_phase, research_phase=args.research_phase)
    args.output.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "pairs": summary["pairedComparisons"]}))
