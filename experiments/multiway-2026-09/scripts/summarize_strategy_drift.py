"""Validate six restored-state legacy/compact strategy-drift measurements.

No solver executes. Refresh times and retained payload are separate from
whole-process lifetime peak, which includes restore and checkpoint writing.
"""
from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
from pathlib import Path, PurePosixPath
import re
import statistics
import tomllib
import zipfile

import summarize_checkpoint_write as shared
from summarize_checkpoint_write import (checked_file, digest, finite_values, integer,
                                       number, read, require, same_path, serialized, sha)

ORDER = (("legacy", 1), ("compact", 1), ("compact", 2), ("legacy", 2), ("legacy", 3), ("compact", 3))
CASES = tuple(f"restored-{mode}-{pair}" for mode, pair in ORDER)
RUNTIME_FIELDS = {"status", "completedUtc", "preexecutionSha256"}
COMMANDS = {
    "cargo fmt --all --check",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo clippy -p cli --examples --features research-draw-abstraction -- -D warnings",
    "cargo test --workspace",
    "cargo test -p cli --examples --features research-draw-abstraction",
    "cargo test -p multiway --features research-average-sampling --lib",
    "cargo build --release -p cli --example mw_strategy_drift_bench",
}
SOURCE_PATHS = {
    "Cargo.toml", "Cargo.lock", ".cargo/config.toml", "crates/multiway/src/checkpoint.rs",
    "crates/multiway/src/solver/mod.rs", "crates/multiway/src/solver/drift.rs",
    "crates/multiway/src/solver/drift_tests.rs", "crates/multiway/src/solver/snapshot.rs",
    "crates/cli/src/multiway_solve.rs", "crates/cli/examples/mw_strategy_drift_bench.rs",
}
NONCOMMON_RAW = {"mode", "output", "constructionElapsedSecs", "firstRefreshSeconds", "stableRefreshSeconds", "retainedPayloadBytes"}


def schedule_checks(run, exp):
    require((exp["threads"], exp["memory"], exp["timeoutSeconds"]) == (8, "8GiB", 300), "fixed resources")
    require(exp["promotionAllowed"] is False and exp["broaderGoalComplete"] is False
            and exp["cloudResourcesStarted"] is False, "storage-only scope")
    require(len(exp["cases"]) == 6, "six case definitions")
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
    checked_file(Path(__file__).parent / "tests/test_summarize_strategy_drift.py", exp["testScriptSha256"], "test identity")
    require({Path(p).resolve() for p in exp["dependencies"]} == {Path(shared.__file__).resolve()}, "complete frozen helper dependency")
    for path, checksum in exp["dependencies"].items():
        checked_file(path, checksum, "helper identity")
    config = tomllib.loads(Path(exp["config"]).read_text(encoding="utf-8-sig"))
    require(config["schema"] == "solvers.multiway-preflop/v1"
            and config["game"]["seat_count"] == 6 and config["solver"]["seed"] == 0, "fixed model/seed")
    manifest = read(exp["sourceManifest"])
    files = {entry["path"]: digest(entry["sha256"]) for entry in manifest["files"]}
    require(len(files) == len(manifest["files"]) and SOURCE_PATHS <= set(files), "unique complete source manifest")
    require(manifest["baseRevision"] == exp["baseRevision"], "source base revision")
    for path in files:
        require(not PurePosixPath(path).is_absolute() and ".." not in PurePosixPath(path).parts and "\\" not in path,
                "canonical source archive path")
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
    require(len({c["command"] for c in checks}) == len(checks) and COMMANDS <= {c["command"] for c in checks},
            "required verification command set")
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
            require(totals == [check["passed"], check["failed"], check["ignored"]] and len(rows) == check["suites"], "verification test totals")
    return dict(sourceFiles=len(files), checks=len(checks), sourceManifestSha256=exp["sourceManifestSha256"],
                sourceZipSha256=exp["sourceZipSha256"], binarySha256=exp["binarySha256"],
                verificationSha256=exp["verificationSha256"], archivedSourcesVerified=True,
                inputCheckpointSha256=exp["inputCheckpointSha256"], reportedBlake3Recomputed=False)


def expected_arguments(exp, case):
    return ["--config", str(Path(exp["config"]).resolve()), "--checkpoint", str(Path(exp["inputCheckpoint"]).resolve()),
            "--output", str(Path(case["output"]).resolve()), "--mode", case["mode"], "--threads", "8", "--memory", "8GiB",
            "--cache-dir", str(Path(exp["cacheDir"]).resolve()), "--source-revision", exp["sourceManifestSha256"]]


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


def raw_checks(raw, exp, case):
    finite_values(raw)
    require(raw["schemaVersion"] == "solvers.strategy-drift-bench/v1", "raw schema")
    require(raw["sourceRevision"] == exp["sourceManifestSha256"] and raw["mode"] == case["mode"] and raw["threads"] == 8,
            "raw mode/source/threads")
    for field in ("config", "inputCheckpoint"):
        same_path(raw[field], exp[field], "raw input location: " + field)
    same_path(raw["output"], case["output"], "raw output location")
    for field in ("outputBlake3", "executableBlake3", "effectiveConfigBlake3"):
        digest(raw[field])
    for field in ("configurationFingerprint", "abstractionFingerprint"):
        value = raw[field]
        require(isinstance(value, list) and len(value) == 32 and all(type(b) is int and 0 <= b <= 255 for b in value), "raw fingerprint format")
    for field in ("firstDriftBits", "stableDriftBits"):
        require(isinstance(raw[field], list) and len(raw[field]) == 6 and all(type(b) is int and b == 0 for b in raw[field]),
                "static restored-state zero drift bits: " + field)
    runtime = raw["runtime"]
    require(set(runtime) == {"confirmations_met", "next_evaluation_sweep", "evaluation_samples", "evaluation_sequence", "cumulative_solve_millis"}, "runtime fields")
    for value in runtime.values():
        integer(value, "runtime counter")
    metrics = raw["metrics"]
    require(set(metrics) == {"sweeps", "traversals", "infosets", "memory_bytes", "total_deal_attempts", "mean_deal_attempts", "hand_updates", "average_positive_regret"}, "metrics fields")
    for field in ("sweeps", "traversals", "infosets", "memory_bytes", "total_deal_attempts", "hand_updates"):
        integer(metrics[field], "metric counter: " + field, True)
    require(raw["sweeps"] == metrics["sweeps"] == 32768 and metrics["traversals"] == 32768 * 6, "fixed restored sweep/traversal count")
    require(metrics["total_deal_attempts"] >= metrics["traversals"] and metrics["hand_updates"] >= metrics["traversals"]
            and metrics["memory_bytes"] <= 8 * 1024**3, "payload/deal/update bounds")
    number(metrics["mean_deal_attempts"], "mean deal attempts", True)
    require(abs(metrics["mean_deal_attempts"] - metrics["total_deal_attempts"] / metrics["traversals"]) <= 1e-12, "mean deal accounting")
    require(len(metrics["average_positive_regret"]) == 6, "six-seat regret vector")
    for value in metrics["average_positive_regret"]:
        number(value, "positive regret diagnostic")
    columns = integer(raw["observedColumns"], "observed columns", True)
    slots = integer(raw["observedActionSlots"], "observed action slots", True)
    payload = integer(raw["retainedPayloadBytes"], "retained payload", True)
    require(columns == metrics["infosets"] and slots >= columns, "complete observed layout counts")
    require(payload >= 4 * slots + (8 * columns if case["mode"] == "compact" else 0), "retained payload lower bound")
    for field in ("constructionElapsedSecs", "firstRefreshSeconds", "stableRefreshSeconds"):
        number(raw[field], "positive phase clock: " + field, True)
    integer(raw["outputBytes"], "output bytes", True)
    require(Path(case["output"]).stat().st_size == raw["outputBytes"], "actual output byte length")


def summarize_case(run, exp, case):
    job = job_checks(exp, case)
    path = run / case["name"]
    raw, measured = read(path / "stdout.json"), read(path / "measurement.json")
    raw_checks(raw, exp, case)
    finite_values(measured)
    require(measured["schemaVersion"] == "solvers.checkpoint-audit-measurement/v1"
            and measured["exitCode"] == 0 and measured["timedOut"] is False, "successful measurement")
    same_path(measured["binary"], exp["binary"], "measurement binary path")
    same_path(measured["job"], case["job"], "measurement job path")
    require(measured["arguments"] == job["arguments"] and measured["timeoutSeconds"] == 300
            and measured["validationReport"] == exp["validationReport"], "measurement arguments/timeout/report")
    for field, checksum in dict(binary=exp["binarySha256"], job=case["jobSha256"], config=exp["configSha256"], sourceManifest=exp["sourceManifestSha256"]).items():
        require(measured[field + "Sha256"] == checksum, "measurement identity: " + field)
    require(measured["sourceRevision"] == exp["baseRevision"], "measured base revision")
    checked_file(path / "stdout.json", measured["stdoutSha256"], "raw output identity")
    wall = number(measured["wallSeconds"], "process wall seconds", True)
    peak = integer(measured["observedPeakWorkingSetBytes"], "lifetime peak", True)
    require(raw["constructionElapsedSecs"] + raw["firstRefreshSeconds"] + raw["stableRefreshSeconds"] <= wall + .01, "phase clock containment")
    started = datetime.fromisoformat(measured["startedUtc"].replace("Z", "+00:00"))
    require(started.tzinfo is not None, "measurement timestamp timezone")
    start = started.timestamp() * 1000
    return dict(case=case["name"], mode=case["mode"], pair=case["pair"],
                constructionSeconds=raw["constructionElapsedSecs"], firstRefreshSeconds=raw["firstRefreshSeconds"],
                stableRefreshSeconds=raw["stableRefreshSeconds"], retainedPayloadBytes=raw["retainedPayloadBytes"],
                wallSeconds=wall, observedPeakWorkingSetBytes=peak, processStartedUnixMs=start, processFinishedUnixMs=start + wall * 1000,
                observedColumns=raw["observedColumns"], observedActionSlots=raw["observedActionSlots"],
                outputSha256=sha(case["output"]), outputBytes=raw["outputBytes"], outputBlake3=raw["outputBlake3"],
                stdoutSha256=measured["stdoutSha256"], measurementSha256=sha(path / "measurement.json"),
                commonResult={key: value for key, value in raw.items() if key not in NONCOMMON_RAW}, peakMethod=measured["peakMethod"])


def comparison(cases):
    require(len(cases) == 6 and tuple(c["case"] for c in cases) == CASES, "all six completed cases in fixed order")
    first = cases[0]
    for case in cases[1:]:
        require(case["commonResult"] == first["commonResult"], "all logical results exactly equal")
        require((case["outputSha256"], case["outputBytes"], case["outputBlake3"])
                == (first["outputSha256"], first["outputBytes"], first["outputBlake3"]), "all output checkpoint identities equal")
    for previous, following in zip(cases, cases[1:]):
        require(previous["processFinishedUnixMs"] <= following["processStartedUnixMs"] + 1, "serialized process order/overlap")
    medians = {}
    for field in ("firstRefreshSeconds", "stableRefreshSeconds", "retainedPayloadBytes", "observedPeakWorkingSetBytes", "constructionSeconds", "wallSeconds"):
        modes = {mode: [c[field] for c in cases if c["mode"] == mode] for mode in ("legacy", "compact")}
        require(all(len(v) == 3 for v in modes.values()), "three repeats per mode")
        left, right = (statistics.median(modes[mode]) for mode in ("legacy", "compact"))
        medians[field] = dict(legacy=dict(values=modes["legacy"], median=left), compact=dict(values=modes["compact"], median=right),
                              compactToLegacyRatio=right / left, compactMinusLegacy=right - left)
    pairs = []
    for pair in (1, 2, 3):
        arms = {c["mode"]: c for c in cases if c["pair"] == pair}
        require(set(arms) == {"legacy", "compact"}, "one case per mode per pair")
        pairs.append(dict(pair=pair, legacy=arms["legacy"]["case"], compact=arms["compact"]["case"],
                          firstRefreshRatio=arms["compact"]["firstRefreshSeconds"] / arms["legacy"]["firstRefreshSeconds"],
                          stableRefreshRatio=arms["compact"]["stableRefreshSeconds"] / arms["legacy"]["stableRefreshSeconds"]))
    return dict(allLogicalResultsExactlyMatch=True, allOutputFileSha256ExactlyMatch=True, outputSha256=first["outputSha256"],
                medians=medians, pairs=pairs,
                statisticalScope="Three serialized static-state repeat pairs; ratios and medians are descriptive, without paired statistical errors or confidence intervals. Retained payload is representation-dependent and excluded from logical equality.")


def summarize(run, case_name=None):
    run = Path(run)
    exp = read(run / "experiment.json")
    finite_values(exp)
    schedule_checks(run, exp)
    require(case_name is None or case_name in CASES, "unknown case")
    evidence = evidence_checks(exp)
    cases = [summarize_case(run, exp, case) for case in exp["cases"] if case_name is None or case["name"] == case_name]
    return dict(schemaVersion="solvers.strategy-drift-summary/v1", status="completed-six-case-comparison" if case_name is None else "completed-case",
                experiment=exp, experimentSha256=sha(run / "experiment.json"), evidence=evidence, cases=cases,
                comparison=comparison(cases) if case_name is None else None,
                interpretation="One restored 32768-sweep dense K32 state, without additional learning. First capture and stable refresh are timed separately; zero per-seat f64 bits and complete observed counts must match. All output checkpoint SHA256 values are recomputed and identical, while reported BLAKE3 values are checked for format and agreement. Checkpoints are written by the common borrowed writer after tracker disposal. Retained payload includes policy capacities but omits allocator overhead; it is not working-set memory. The runner records whole-process lifetime peak only, which includes restore, refresh, checkpoint writing and disposal; no phase RSS or allocation peak is inferred. Input hashes are checked against current retained files and literal jobs, not measured before/after by this frozen runner. No learning-speed, changing-profile growth, hard memory-cap, convergence or default-promotion claim follows.")


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
