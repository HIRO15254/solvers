"""Validate six serialized, restored-state checkpoint-writing measurements.

Only standard-library code runs. SHA256 checks actual files; the executable's
reported BLAKE3 identities are cross-checked across cases, not recomputed here.
Timing and sampled working sets are descriptive storage measurements.
"""
from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import re
import statistics
import tomllib
import zipfile

ORDER = (("owned", 1), ("borrowed", 1), ("borrowed", 2),
         ("owned", 2), ("owned", 3), ("borrowed", 3))
CASES = tuple(f"restored-{mode}-{pair}" for mode, pair in ORDER)
RUNTIME_FIELDS = {"status", "completedUtc", "preexecutionSha256"}
COMMANDS = {
    "cargo fmt --all --check",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo clippy -p cli --examples --features research-draw-abstraction -- -D warnings",
    "cargo test --workspace",
    "cargo test -p cli --examples --features research-draw-abstraction",
    "cargo test -p multiway --features research-average-sampling --lib",
    "cargo build --release -p cli --example mw_checkpoint_write_bench",
}
SOURCE_PATHS = {
    "Cargo.toml", "Cargo.lock", ".cargo/config.toml",
    "crates/multiway/src/checkpoint.rs", "crates/multiway/src/solver/mod.rs",
    "crates/multiway/src/solver/snapshot.rs", "crates/multiway/src/solver/snapshot_tests.rs",
    "crates/cli/src/multiway_solve.rs", "crates/cli/examples/mw_checkpoint_write_bench.rs",
}
NONCOMMON_RAW = {"mode", "output", "constructionSeconds", "writeSeconds",
                 "writeStartedUnixMs", "writeFinishedUnixMs"}


def require(condition, label):
    if not condition:
        raise ValueError(label)


def read(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON field: " + key)
            result[key] = value
        return result
    return json.loads(Path(path).read_text(encoding="utf-8-sig"), object_pairs_hook=unique)


def serialized(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True, allow_nan=False) + "\n").encode("utf-8")


def sha(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def number(value, label, positive=False):
    require(type(value) in (int, float) and math.isfinite(value) and value >= 0
            and (not positive or value > 0), label)
    return value


def integer(value, label, positive=False):
    require(type(value) is int, label)
    return number(value, label, positive)


def finite_values(value):
    if isinstance(value, dict):
        for child in value.values():
            finite_values(child)
    elif isinstance(value, list):
        for child in value:
            finite_values(child)
    elif isinstance(value, float):
        require(math.isfinite(value), "nonfinite recorded value")


def digest(value):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value), "digest format")
    return value


def same_path(actual, expected, label):
    require(isinstance(actual, str) and Path(actual).resolve() == Path(expected).resolve(), label)


def checked_file(path, expected, label):
    require(sha(path) == digest(expected), label)


def schedule_checks(run, exp):
    require((exp["threads"], exp["memory"], exp["timeoutSeconds"]) == (8, "8GiB", 300), "fixed resources")
    require(exp["promotionAllowed"] is False and exp["broaderGoalComplete"] is False
            and exp["cloudResourcesStarted"] is False, "storage-only scope")
    require(len(exp["cases"]) == 6, "six complete case definitions")
    for case, name, (mode, pair) in zip(exp["cases"], CASES, ORDER, strict=True):
        require((case["name"], case["mode"], case["pair"]) == (name, mode, pair), "fixed case order/mode/pair")
        same_path(case["job"], run / f"{name}-job.json", "case job location")
        same_path(case["output"], run / name / "checkpoint.mwckpt", "case output location")
    checked_file(run / "experiment-preexecution.json", exp["preexecutionSha256"], "preexecution identity")
    before = read(run / "experiment-preexecution.json")
    require({k: v for k, v in exp.items() if k not in RUNTIME_FIELDS}
            == {k: v for k, v in before.items() if k not in RUNTIME_FIELDS}, "immutable preexecution conditions")


def evidence_checks(exp):
    for field in ("binary", "config", "inputCheckpoint", "sourceManifest", "sourceZip", "verification", "runner"):
        checked_file(exp[field], exp[field + "Sha256"], "evidence identity: " + field)
    checked_file(__file__, exp["summarizerSha256"], "summarizer identity")
    checked_file(Path(__file__).parent / "tests/test_summarize_checkpoint_write.py",
                 exp["testScriptSha256"], "test identity")
    config = tomllib.loads(Path(exp["config"]).read_text(encoding="utf-8-sig"))
    require(config["schema"] == "solvers.multiway-preflop/v1"
            and config["game"]["seat_count"] == 6 and config["solver"]["seed"] == 0, "fixed production model/seed")
    manifest = read(exp["sourceManifest"])
    files = {entry["path"]: digest(entry["sha256"]) for entry in manifest["files"]}
    require(len(files) == len(manifest["files"]) and SOURCE_PATHS <= set(files), "unique complete source manifest")
    require(manifest["baseRevision"] == exp["baseRevision"], "source base revision")
    for path in files:
        require(not PurePosixPath(path).is_absolute() and ".." not in PurePosixPath(path).parts
                and "\\" not in path, "canonical source archive path")
    with zipfile.ZipFile(exp["sourceZip"]) as archive:
        require(len(archive.namelist()) == len(files) and set(archive.namelist()) == set(files), "exact source ZIP entries")
        for path, checksum in files.items():
            with archive.open(path) as source:
                require(hashlib.file_digest(source, "sha256").hexdigest() == checksum, "archived source identity: " + path)
    verification = read(exp["verification"])
    require(verification["status"] == "passed" and verification["sourceFiles"] == len(files)
            and verification["sourceManifestSha256"] == exp["sourceManifestSha256"]
            and verification["binarySha256"] == exp["binarySha256"], "verification source/binary/status")
    checks = verification["checks"]
    require(len({check["command"] for check in checks}) == len(checks)
            and COMMANDS <= {check["command"] for check in checks}, "required verification command set")
    for check in checks:
        require(check["exitCode"] == 0, "verification command failed")
        checked_file(check["log"], check["sha256"], "verification log identity")
        number(check["elapsedSecs"], "verification clock")
        text = Path(check["log"]).read_text(encoding="utf-8-sig")
        require("test result: FAILED" not in text and not re.search(r"^error(?:\[|:)", text, re.M), "failed verification log")
        if check["command"].startswith("cargo test "):
            rows = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
            totals = [sum(int(row[i]) for row in rows) for i in range(3)]
            require(rows and totals[0] > 0 and totals[1] == 0, "successful nonempty test log")
            require(totals == [check["passed"], check["failed"], check["ignored"]]
                    and len(rows) == check["suites"], "verification test totals")
    return dict(sourceFiles=len(files), checks=len(checks), sourceManifestSha256=exp["sourceManifestSha256"],
                sourceZipSha256=exp["sourceZipSha256"], binarySha256=exp["binarySha256"],
                verificationSha256=exp["verificationSha256"], archivedSourcesVerified=True)


def expected_arguments(exp, case):
    return ["--config", str(Path(exp["config"]).resolve()),
            "--checkpoint", str(Path(exp["inputCheckpoint"]).resolve()),
            "--output", str(Path(case["output"]).resolve()), "--mode", case["mode"],
            "--threads", "8", "--memory", "8GiB", "--cache-dir", str(Path(exp["cacheDir"]).resolve()),
            "--source-revision", exp["baseRevision"]]


def job_checks(exp, case):
    checked_file(case["job"], case["jobSha256"], "job identity")
    job = read(case["job"])
    require(job["arguments"] == expected_arguments(exp, case), "literal job arguments")
    require(job["timeoutSeconds"] == 300 and job["validationReport"] == exp["validationReport"], "job timeout/report")
    require(job["binarySha256"] == exp["binarySha256"], "job binary identity")
    for field in ("config", "inputCheckpoint", "sourceManifest"):
        same_path(job[field], exp[field], "job input location: " + field)
        require(job[field + "Sha256"] == exp[field + "Sha256"], "job input identity: " + field)
    return job


def phase_checks(raw, measurement):
    finite_values(raw)
    finite_values(measurement)
    construction = number(raw["constructionSeconds"], "construction seconds", True)
    write = number(raw["writeSeconds"], "write seconds", True)
    wall = number(measurement["wallSeconds"], "wall seconds", True)
    require(construction + write <= wall + .001, "phase seconds exceed process wall time")
    start = integer(raw["writeStartedUnixMs"], "write start timestamp", True)
    end = integer(raw["writeFinishedUnixMs"], "write finish timestamp", True)
    require(end >= start and abs((end - start) / 1000 - write) <= .0011, "write clock interval mismatch")
    started = datetime.fromisoformat(measurement["startedUtc"].replace("Z", "+00:00"))
    require(started.tzinfo is not None, "measurement timestamp timezone")
    process_start = started.timestamp() * 1000
    require(start >= process_start - 1 and end <= process_start + wall * 1000 + 1,
            "write interval outside process interval")
    peak = integer(measurement["observedPeakWorkingSetBytes"], "lifetime peak", True)
    observations = measurement["workingSetObservations"]
    require(isinstance(observations, list), "working-set observation list")
    previous = process_start - 1
    phase = []
    for observation in observations:
        timestamp = integer(observation["unixMs"], "working-set timestamp", True)
        working = integer(observation["workingSetBytes"], "working-set bytes")
        require(previous <= timestamp <= process_start + wall * 1000 + 1, "working-set timestamp order/range")
        require(working <= peak, "working set exceeds observed lifetime peak")
        previous = timestamp
        if start <= timestamp <= end:
            require(working > 0, "write-phase active process working set")
            phase.append(working)
    return dict(constructionSeconds=construction, writeSeconds=write, wallSeconds=wall,
                processStartedUnixMs=process_start, processFinishedUnixMs=process_start + wall * 1000,
                writeStartedUnixMs=start, writeFinishedUnixMs=end,
                observedPeakWorkingSetBytes=peak, writePhaseSamples=len(phase),
                observedWriteWorkingSetBytes=max(phase) if phase else None)


def raw_checks(raw, exp, case):
    require(raw["schemaVersion"] == "solvers.multiway-checkpoint-write-bench/v1", "raw schema")
    require(raw["sourceRevision"] == exp["baseRevision"] and raw["mode"] == case["mode"]
            and raw["threads"] == 8, "raw mode/source/threads")
    for field in ("config", "inputCheckpoint"):
        same_path(raw[field], exp[field], "raw input location: " + field)
    same_path(raw["output"], case["output"], "raw output location")
    require(raw["freshSweeps"] is None and raw["trainingSeconds"] is None, "restored-only measurement")
    for field in ("outputBlake3", "executableBlake3", "effectiveConfigBlake3"):
        digest(raw[field])
    for field in ("configurationFingerprint", "abstractionFingerprint"):
        value = raw[field]
        require(isinstance(value, list) and len(value) == 32
                and all(type(byte) is int and 0 <= byte <= 255 for byte in value), "raw fingerprint format")
    runtime = raw["runtime"]
    require(set(runtime) == {"confirmations_met", "next_evaluation_sweep", "evaluation_samples",
                             "evaluation_sequence", "cumulative_solve_millis"}, "runtime fields")
    for value in runtime.values():
        integer(value, "runtime counter")
    metrics = raw["metrics"]
    require(set(metrics) == {"sweeps", "traversals", "infosets", "memory_bytes", "total_deal_attempts",
                             "mean_deal_attempts", "hand_updates", "average_positive_regret"}, "metrics fields")
    for field in ("sweeps", "traversals", "infosets", "memory_bytes", "total_deal_attempts", "hand_updates"):
        integer(metrics[field], "metric counter: " + field, True)
    require(metrics["sweeps"] == 32768 and metrics["traversals"] == 32768 * 6, "fixed restored sweep/traversal count")
    require(metrics["memory_bytes"] <= 8 * 1024**3 and metrics["hand_updates"] >= metrics["traversals"], "payload/hand-update bounds")
    number(metrics["mean_deal_attempts"], "mean deal attempts", True)
    require(len(metrics["average_positive_regret"]) == 6, "six-seat metric vector")
    for value in metrics["average_positive_regret"]:
        number(value, "positive regret diagnostic")
    integer(raw["outputBytes"], "output byte length", True)
    require(Path(case["output"]).stat().st_size == raw["outputBytes"], "actual checkpoint output byte length")


def summarize_case(run, exp, case):
    job = job_checks(exp, case)
    raw_path = run / case["name"] / "stdout.json"
    measurement_path = run / case["name"] / "measurement.json"
    raw, measurement = read(raw_path), read(measurement_path)
    raw_checks(raw, exp, case)
    require(measurement["schemaVersion"] == "solvers.checkpoint-write-measurement/v1"
            and measurement["exitCode"] == 0 and measurement["timedOut"] is False
            and measurement["inputsUnchanged"] is True, "successful stable-input measurement")
    same_path(measurement["binary"], exp["binary"], "measurement binary path")
    same_path(measurement["job"], case["job"], "measurement job path")
    require(measurement["arguments"] == job["arguments"] and measurement["timeoutSeconds"] == 300
            and measurement["validationReport"] == exp["validationReport"], "measurement argv/timeout/report")
    hashes = dict(binary=exp["binarySha256"], job=case["jobSha256"], config=exp["configSha256"],
                  inputCheckpoint=exp["inputCheckpointSha256"], sourceManifest=exp["sourceManifestSha256"])
    require(measurement["inputHashesBefore"] == hashes and measurement["inputHashesAfter"] == hashes,
            "measurement before/after input hashes")
    for field, checksum in hashes.items():
        require(measurement[field + "Sha256"] == checksum, "measurement identity: " + field)
    require(measurement["sourceRevision"] == exp["baseRevision"], "measurement base revision")
    checked_file(raw_path, measurement["stdoutSha256"], "raw output identity")
    phase = phase_checks(raw, measurement)
    return dict(case=case["name"], mode=case["mode"], pair=case["pair"], **phase,
                outputSha256=sha(case["output"]), outputBytes=raw["outputBytes"], outputBlake3=raw["outputBlake3"],
                measurementSha256=sha(measurement_path), stdoutSha256=measurement["stdoutSha256"],
                commonResult={k: v for k, v in raw.items() if k not in NONCOMMON_RAW},
                peakMethod=measurement["peakMethod"], phasePeakMethod=measurement["phasePeakMethod"])


def comparison(cases):
    require(len(cases) == 6 and tuple(case["case"] for case in cases) == CASES, "all six completed cases in fixed order")
    first = cases[0]
    for case in cases[1:]:
        require(case["commonResult"] == first["commonResult"], "all non-timing results exactly equal")
        require((case["outputSha256"], case["outputBytes"], case["outputBlake3"])
                == (first["outputSha256"], first["outputBytes"], first["outputBlake3"]), "all checkpoint output identities equal")
    for previous, following in zip(cases, cases[1:]):
        require(previous["processFinishedUnixMs"] <= following["processStartedUnixMs"] + 1,
                "serialized process intervals overlap or run out of order")
        require(previous["writeFinishedUnixMs"] <= following["writeStartedUnixMs"], "serialized execution order")
    fields = ("writeSeconds", "observedWriteWorkingSetBytes", "observedPeakWorkingSetBytes")
    summaries = {}
    for field in fields:
        modes = {}
        for mode in ("owned", "borrowed"):
            values = [case[field] for case in cases if case["mode"] == mode]
            available = [value for value in values if value is not None]
            # Do not silently turn a missing repeat into a two-observation
            # comparison. Keep all three values and mark the median unavailable.
            modes[mode] = dict(values=values, available=len(available),
                               median=statistics.median(available) if len(available) == 3 else None)
        owned, borrowed = modes["owned"]["median"], modes["borrowed"]["median"]
        summaries[field] = dict(**modes, borrowedToOwnedRatio=borrowed / owned if owned is not None and borrowed is not None else None,
                                borrowedMinusOwned=borrowed - owned if owned is not None and borrowed is not None else None)
    pairs = []
    for pair in (1, 2, 3):
        arms = {case["mode"]: case for case in cases if case["pair"] == pair}
        require(set(arms) == {"owned", "borrowed"}, "one case per mode in each pair")
        pairs.append(dict(pair=pair, owned=arms["owned"]["case"], borrowed=arms["borrowed"]["case"],
                          borrowedToOwnedWriteRatio=arms["borrowed"]["writeSeconds"] / arms["owned"]["writeSeconds"]))
    return dict(allNonTimingResultsExactlyMatch=True, allOutputFileSha256ExactlyMatch=True,
                outputSha256=first["outputSha256"], medians=summaries, pairs=pairs,
                statisticalScope="Three serialized repeat pairs; medians, ratios and differences are descriptive. No paired statistical error or confidence interval is estimated.")


def summarize(run, case_name=None):
    run = Path(run)
    exp = read(run / "experiment.json")
    finite_values(exp)
    schedule_checks(run, exp)
    require(case_name is None or case_name in CASES, "unknown case")
    evidence = evidence_checks(exp)
    cases = [summarize_case(run, exp, case) for case in exp["cases"] if case_name is None or case["name"] == case_name]
    return dict(schemaVersion="solvers.checkpoint-write-summary/v1",
                status="completed-six-case-comparison" if case_name is None else "completed-case",
                experiment=exp, experimentSha256=sha(run / "experiment.json"), evidence=evidence,
                cases=cases, comparison=comparison(cases) if case_name is None else None,
                interpretation="Storage evidence from one restored 32768-sweep state, with no training. Actual output SHA256 values must match across all six cases. Reported BLAKE3 values are checked for format and cross-case equality; SHA256 is recomputed from files. Write time includes preparation, compression, fsync, atomic persist and temporary snapshot/index disposal; hashing and final solver disposal are outside. Write-phase current working-set samples can omit peaks and include already resident construction allocations. Empty phase samples are unavailable, never zero; the separate lifetime peak may be dominated by restore. No hard memory-cap, convergence, learning-speed or promotion claim.")


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
