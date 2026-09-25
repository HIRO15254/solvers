"""Freeze and run a small supervised, byte-exact codec comparison. No builds/cloud.

Python 3.11+ stdlib. Commands: prepare, freeze, run, check. See README.md.
"""
from __future__ import annotations

import argparse
import copy
import datetime as dt
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import sys
import tarfile
import tempfile
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
BASELINE = "88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0"
SOURCE_ARCHIVES = {
    "baseline": "3de4afef13082dca9e17c38c9cc5f5d1734855f017d812ecb04e9cdbe0f7da27",
    "candidate": "a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a",
}
BUILD_STAGES = ("toolchain", "fmt", "clippy", "workspace-tests", "python-tools", "release-cli",
                "release-codec-current", "release-codec-baseline")
CASES = ("river", "turn", "flop")
OPERATIONS = ("decode-all", "read-root", "read-repeat-chunk", "stream-write")
ITERATIONS = {op: 64 if op == "read-repeat-chunk" else 1 for op in OPERATIONS}
THRESHOLDS = {
    "correctness": "exact canonical metadata/float bits/all raw arrays; exact rewritten input and paired SOL bytes",
    "repetitions": 3,
    "warmups_per_case_operation_side": 1,
    "per_case_operation_improvement": {"median_candidate_over_baseline_max": 0.95,
                                       "paired_faster_count_min": 2},
    "scope": "Descriptive codec microbenchmark only; no overall R1, solver quality or end-to-end speed acceptance",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    def unique(pairs):
        out = {}
        for key, value in pairs:
            require(key not in out, f"duplicate JSON key: {key}")
            out[key] = value
        return out
    return json.loads(Path(path).read_text(encoding="utf-8"), object_pairs_hook=unique,
                      parse_constant=lambda x: (_ for _ in ()).throw(ValueError(x)))


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(encode(value)).hexdigest()


def write(path, value, *, exclusive=False):
    path = Path(path)
    fd, temporary = tempfile.mkstemp(prefix=".codec-", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as output:
            json.dump(value, output, indent=2, sort_keys=True, allow_nan=False)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        if exclusive:
            os.link(temporary, path)
        else:
            os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def identity(path):
    path = Path(path).resolve(strict=True)
    before = path.stat()
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    after = path.stat()
    stamp = lambda s: (s.st_size, s.st_mtime_ns, s.st_ctime_ns, s.st_ino)
    require(stamp(before) == stamp(after), f"changed during hash: {path}")
    return {"path": str(path), "bytes": after.st_size, "sha256": hasher.hexdigest()}


def verify(item):
    require(isinstance(item, dict) and set(item) == {"path", "bytes", "sha256"}, "invalid FileRef fields")
    require(isinstance(item["path"], str) and bool(item["path"]), "invalid FileRef path")
    hash_field(item["sha256"])
    require(type(item["bytes"]) is int and item["bytes"] >= 0, "invalid FileRef bytes")
    require(identity(item["path"]) == item, f"identity changed: {item['path']}")


def same_bytes(left, right):
    with Path(left).open("rb") as a, Path(right).open("rb") as b:
        while True:
            aa, bb = a.read(1024 * 1024), b.read(1024 * 1024)
            require(aa == bb, f"byte mismatch: {left} vs {right}")
            if not aa:
                return


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def host():
    result = {"system": platform.system(), "machine": platform.machine(),
              "cpu": platform.processor(), "logical_cpus": os.cpu_count()}
    if Path("/proc/cpuinfo").is_file():
        cpu = Path("/proc/cpuinfo").read_text()
        result["cpu"] = next((x.split(":", 1)[1].strip() for x in cpu.splitlines()
                              if x.startswith("model name")), "unknown")
        result["flags"] = next((x.split(":", 1)[1].strip() for x in cpu.splitlines()
                                if x.startswith("flags")), "unknown")
        result["boot_id"] = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    return result


def hash_field(value):
    require(isinstance(value, str) and re.fullmatch("[0-9a-f]{64}", value) is not None, "expected 64 lowercase SHA-256 hex digits")


def selection():
    result = read(HERE / "inputs.json")
    require(result["schema"] == "r1.codec-input-selection/v1" and set(result["cases"]) == set(CASES),
            "input selection schema/cases differ")
    hash_field(result["source06_archive_sha256"])
    for item in (result["bundle"], result["sidecar"], *result["cases"].values()):
        hash_field(item["sha256"])
        require(type(item["bytes"]) is int and item["bytes"] > 0, "invalid input size")
    return result


def prepare(args):
    selected = selection()
    for path, expected in ((args.bundle, selected["bundle"]), (args.sidecar, selected["sidecar"])):
        got = identity(path)
        require(all(got[k] == expected[k] for k in ("bytes", "sha256")), "retained bundle/sidecar mismatch")
    manifest = read(args.sidecar)
    rows = {row["archive_member"]: row for row in manifest["files"] if row.get("included")}
    args.out.mkdir(parents=True, exist_ok=False)
    with tarfile.open(args.bundle, "r:gz") as archive:
        members = archive.getmembers()
        require(len({x.name for x in members}) == len(members), "duplicate archive member")
        for case, expected in selected["cases"].items():
            member = archive.getmember(expected["archive_member"])
            row = rows[member.name]
            require(member.isfile() and member.size == expected["bytes"], "invalid selected member")
            require(all(row[k] == expected[k] for k in expected), "retained selection changed")
            stream = archive.extractfile(member)
            require(stream is not None, "unreadable selected member")
            data = stream.read(expected["bytes"] + 1)
            require(len(data) == expected["bytes"] and hashlib.sha256(data).hexdigest() == expected["sha256"],
                    "selected SOL hash mismatch")
            require(data[:10] == b"SLVRSOLV\x03\x00", "expected SOL v3")
            with (args.out / f"{case}.sol").open("xb") as output:
                output.write(data)
    write(args.out / "selection.json", selected, exclusive=True)
    print(json.dumps({"status": "prepared", "cases": list(CASES), "scope": "three pinned members only"}))


def inspect_build(path, source_root, *, live_host=True):
    build = read(path)
    require(build["schema"] == "r1.codec-build-attestation/v1" and build["status"] == "completed", "build attestation not completed")
    require(build["settings"]["profile"] == "release", "release binaries required")
    require(isinstance(build["settings"]["rustflags"], str)
            and isinstance(build["settings"]["target"], str) and bool(build["settings"]["target"]),
            "build flags/target missing")
    if live_host:
        require(build["host"] == host(), "build CPU/boot differs from measurement host")
    require(set(build["sides"]) == {"baseline", "candidate"}, "build sides differ")
    require(build["sides"]["baseline"]["revision"] == BASELINE, "baseline revision differs")
    verify(build["compiler"])
    verify(build["example"])
    require(build["example"]["sha256"] == identity(source_root / "crates/formats/examples/sol_codec_bench.rs")["sha256"],
            "build used a different example")
    for role, side in build["sides"].items():
        require(isinstance(side["revision"], str) and re.fullmatch("[0-9a-f]{40}", side["revision"]), "source revision missing")
        verify(side["source"])
        require(side["source"]["sha256"] == SOURCE_ARCHIVES[role], "wrong frozen build source archive")
        verify(side["binary"])
    verify(build["validation_file"])
    validation = read(build["validation_file"]["path"])
    require(validation["schema"] == "r1.codec-build/v1" and validation["status"] == "passed",
            "required validation/build did not pass")
    require(validation["boot_id"] == build["host"].get("boot_id"), "validation/build boot mismatch")
    require(validation["planned_stages"] == list(BUILD_STAGES)
            and [x["name"] for x in validation["stages"]] == list(BUILD_STAGES), "required build stages differ")
    require(len(build["validation_stages"]) == len(BUILD_STAGES), "build supervisor identity count differs")
    validation_root = Path(build["validation_file"]["path"]).parent
    for index, stage in enumerate(validation["stages"]):
        require(stage["status"] == "passed" and stage["exit_code"] == 0, "failed build stage")
        stage_dir = validation_root / f"{index:02d}-{stage['name']}"
        stage_identity = build["validation_stages"][index]
        require(stage_identity["path"] == str(stage_dir / "supervisor.json"), "build supervisor identity path differs")
        verify(stage_identity)
        record = read(stage_dir / "supervisor.json")
        require(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
                and record["child_exit_code"] == 0 and record["supervisor_exit_code"] == 0
                and record["cleanup_complete"] is True and record["identity_unchanged"] is True
                and record["argv"] == stage["argv"] and record["cwd"] == stage["cwd"],
                "build stage/supervisor identity or completion mismatch")
        for item in record["outputs"].values():
            verify(item)
        for item in record["identity_before"]:
            verify(item)
        require(record["identity_before"] == record["identity_after"], "changed build stage input")
    for name in ("driver", "supervisor"):
        verify(validation["identities"][name])
    for role, key, stage_index in (("baseline", "baseline", 7), ("candidate", "current", 6)):
        files = validation["identities"][key]
        require(files and len({f["path"] for f in files}) == len(files), "empty/duplicate build source identities")
        for item in files:
            verify(item)
        root = Path(validation["stages"][stage_index]["cwd"])
        require(role != "candidate" or root == source_root, "candidate source root differs")
        expected_example = identity(root / "crates/formats/examples/sol_codec_bench.rs")
        require(expected_example in files and expected_example["sha256"] == build["example"]["sha256"],
                "baseline/current source manifest did not build identical example")
        binary = build["sides"][role]["binary"]
        expected_binary = Path(validation["stages"][stage_index]["target"]) / "release/examples/sol_codec_bench"
        require(Path(binary["path"]) == expected_binary and binary in validation["binaries"],
                "selected codec binary not bound to corresponding build stage")
    return build


def freeze(args):
    source_root = args.source_root.resolve(strict=True)
    build = inspect_build(args.build_record, source_root)
    require(args.timeout_seconds > 0 and math.isfinite(args.timeout_seconds), "invalid timeout")
    require(args.memory_limit_bytes > 0 and args.min_free_memory_bytes >= 0
            and args.disk_reserve_bytes > 0, "invalid resource bounds")
    inputs = {}
    for case, expected in selection()["cases"].items():
        inputs[case] = identity(args.inputs / f"{case}.sol")
        require(all(inputs[case][k] == expected[k] for k in ("bytes", "sha256")), "wrong source06 input")
    plan = {"schema": "r1.codec-plan/v1", "issued_at": now(), "host": host(),
            "source_root": str(source_root),
            "thresholds": THRESHOLDS, "iterations": ITERATIONS,
            "order": "one warmup per side then 3 paired repeats; B/C, C/B, B/C; cases River/Turn/Flop; operations fixed",
            "cache": "warm input bytes: SHA-256 before each invocation and BLAKE3 before each timed phase; no cache eviction",
            "runner": identity(Path(__file__)), "python": identity(Path(sys.executable)),
            "supervisor": identity(source_root / "tools/run_supervised.py"),
            "example": identity(source_root / "crates/formats/examples/sol_codec_bench.rs"),
            "input_selection": identity(HERE / "inputs.json"), "inputs": inputs,
            "build_record": identity(args.build_record), "build": build,
            "limits": {"timeout_seconds": args.timeout_seconds, "memory_limit_bytes": args.memory_limit_bytes,
                       "min_free_memory_bytes": args.min_free_memory_bytes,
                       "disk_reserve_bytes": args.disk_reserve_bytes, "campaign_seconds": 1800}}
    plan["plan_sha256"] = digest(plan)
    write(args.out, plan, exclusive=True)
    print(json.dumps({"status": "frozen", "plan_sha256": plan["plan_sha256"]}))


def verify_plan(plan, *, live_host=True):
    body = copy.deepcopy(plan)
    saved = body.pop("plan_sha256")
    require(digest(body) == saved and plan["schema"] == "r1.codec-plan/v1", "plan tampered")
    require(plan["thresholds"] == THRESHOLDS and plan["iterations"] == ITERATIONS, "protocol changed")
    require(set(plan["inputs"]) == set(CASES), "case set differs")
    for key in ("runner", "python", "supervisor", "example", "input_selection", "build_record"):
        verify(plan[key])
    selected = read(plan["input_selection"]["path"])
    for case, item in plan["inputs"].items():
        verify(item)
        require(all(item[k] == selected["cases"][case][k] for k in ("bytes", "sha256")), "plan input selection differs")
    require(read(plan["build_record"]["path"]) == plan["build"], "build record differs")
    require(plan["host"] == plan["build"]["host"], "plan/build CPU or boot differs")
    if live_host:
        require(plan["host"] == host(), "CPU/boot changed")
    inspect_build(plan["build_record"]["path"], Path(plan["source_root"]), live_host=live_host)


def sample(plan, case, operation, side, directory):
    directory.mkdir()
    item = plan["inputs"][case]
    binary = plan["build"]["sides"][side]["binary"]
    verify(item)  # Identical warm-file cache policy, outside the child timer.
    verify(binary)
    spec = importlib.util.spec_from_file_location("codec_supervisor", plan["supervisor"]["path"])
    supervisor = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(supervisor)
    command = ["--record", str(directory / "supervisor.json"), "--cwd", plan["source_root"],
               "--stdout", str(directory / "stdout.log"), "--stderr", str(directory / "stderr.log"),
               "--disk-path", str(directory), "--grace-seconds", "5", "--kill-wait-seconds", "5",
               "--poll-seconds", "0.05"]
    for key, value in plan["limits"].items():
        if key != "campaign_seconds":
            command += ["--" + key.replace("_", "-"), str(value)]
    for path in (item["path"], plan["runner"]["path"], plan["example"]["path"]):
        command += ["--identity-file", path]
    command += ["--", binary["path"], item["path"], operation, str(ITERATIONS[operation]),
                str(directory / "output")]
    exit_code = supervisor.main(command)
    record = read(directory / "supervisor.json")
    require(exit_code == 0 and record["state"] == "completed" and record["cleanup_complete"] is True
            and record["identity_unchanged"] is True and record["child_exit_code"] == 0,
            f"supervised sample failed: {directory}")
    verify(item)
    require(plan["host"] == host(), "CPU/boot changed during sample")
    report = read(directory / "output/result.json")
    require(report["schema"] == "r1.sol-codec-sample/v1" and report["status"] == "completed"
            and report["format_version"] == 3 and report["operation"] == operation
            and report["iterations"] == ITERATIONS[operation], "wrong sample identity")
    require(report["input"]["bytes"] == item["bytes"] and report["metadata"]["mode"] == "Full",
            "wrong sample input/mode")
    for value in report["timing"].values():
        require(type(value) in (int, float) and math.isfinite(value) and value >= 0, "invalid timing")
    require(report["timing"]["operation_seconds"] > 0, "zero-duration sample")
    result = {"case": case, "operation": operation, "side": side,
              "record": identity(directory / "supervisor.json"), "report": identity(directory / "output/result.json"),
              "canonical": identity(directory / "output/canonical.bin"),
              "root_canonical": identity(directory / "output/root-canonical.bin"),
              "stdout": identity(directory / "stdout.log"), "stderr": identity(directory / "stderr.log"),
              "samples": identity(record["outputs"]["samples"]["path"]),
              "timing": report["timing"], "measurement": record["measurement"], "metadata": report["metadata"]}
    require(result["canonical"]["bytes"] == report["canonical"]["bytes"]
            and result["root_canonical"]["bytes"] == report["root_canonical"]["bytes"], "canonical length differs")
    if operation == "stream-write":
        result["rewritten"] = identity(directory / "output/rewritten.sol")
        require(all(result["rewritten"][k] == item[k] for k in ("bytes", "sha256")), "rewritten SHA/size differs")
        same_bytes(result["rewritten"]["path"], item["path"])
    return result


def validate_pair(a, b):
    require((a["case"], a["operation"]) == (b["case"], b["operation"]), "pair case/operation differs")
    require(a["metadata"] == b["metadata"], "metadata differs")
    for key in ("canonical", "root_canonical"):
        require(all(a[key][k] == b[key][k] for k in ("bytes", "sha256")), f"{key} identity differs")
        same_bytes(a[key]["path"], b[key]["path"])
    if a["operation"] == "stream-write":
        same_bytes(a["rewritten"]["path"], b["rewritten"]["path"])


def metric(sample_record):
    timing = sample_record["timing"]
    # Fresh root query includes the metadata/directory open; repeated chunk batch excludes it.
    return timing["operation_seconds"] + (timing["open_seconds"] if sample_record["operation"] == "read-root" else 0)


def validate_sample(row, plan, run_root):
    case, operation, side = row["case"], row["operation"], row["side"]
    require(case in CASES and operation in OPERATIONS and side in ("baseline", "candidate")
            and type(row["repetition"]) is int and 0 <= row["repetition"] <= 3, "invalid sample label")
    directory = run_root / f"{case}-{operation}-{row['repetition']}-{side}"
    paths = {"record": directory / "supervisor.json", "report": directory / "output/result.json",
             "canonical": directory / "output/canonical.bin", "root_canonical": directory / "output/root-canonical.bin",
             "stdout": directory / "stdout.log", "stderr": directory / "stderr.log",
             "samples": directory / "supervisor.samples.jsonl"}
    for key, path in paths.items():
        require(row[key]["path"] == str(path), "sample role/path binding differs")
        verify(row[key])
    record, report = read(row["record"]["path"]), read(row["report"]["path"])
    require(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
            and type(record["child_exit_code"]) is int and record["child_exit_code"] == 0
            and type(record["supervisor_exit_code"]) is int and record["supervisor_exit_code"] == 0
            and record["cleanup_complete"] is True and record["identity_unchanged"] is True,
            "retained supervisor failure")
    item, binary = plan["inputs"][case], plan["build"]["sides"][side]["binary"]
    argv = [binary["path"], item["path"], operation, str(ITERATIONS[operation]), str(directory / "output")]
    require(record["argv"] == argv and record["resolved_argv"] == argv
            and record["cwd"] == plan["source_root"], "supervisor command/input/role binding differs")
    identities = []
    for value in (binary, plan["python"], plan["supervisor"], item, plan["runner"], plan["example"]):
        if value not in identities:
            identities.append(value)
    require(record["identity_before"] == identities and record["identity_after"] == identities,
            "supervisor frozen identities differ")
    require(record["outputs"] == {key: row[key] for key in ("stdout", "stderr", "samples")},
            "supervisor output identities differ")
    require(report["schema"] == "r1.sol-codec-sample/v1" and report["status"] == "completed"
            and type(report["format_version"]) is int and report["format_version"] == 3
            and report["metadata"]["mode"] == "Full" and report["input"]["bytes"] == item["bytes"],
            "retained sample schema/status/format/input/mode differs")
    hash_field(report["input"]["blake3"])
    require(record["measurement"] == row["measurement"] and report["timing"] == row["timing"]
            and report["metadata"] == row["metadata"] and report["operation"] == operation
            and type(report["iterations"]) is int and report["iterations"] == ITERATIONS[operation],
            "retained sample fields differ from original report")
    timing = row["timing"]
    require(set(timing) == {"preparation_load_seconds", "open_seconds", "operation_seconds",
                            "operation_seconds_per_iteration", "validation_output_seconds"}, "timing fields differ")
    for value in timing.values():
        require(type(value) in (int, float) and math.isfinite(value) and value >= 0, "invalid retained timing")
    require(timing["operation_seconds"] > 0 and timing["operation_seconds_per_iteration"]
            == timing["operation_seconds"] / ITERATIONS[operation], "zero/inconsistent retained timing")
    partial = operation in ("read-root", "read-repeat-chunk")
    count = 1 if partial else report["metadata"]["stored_nodes"]
    require(type(count) is int and count > 0 and report["decoded_strategy_blocks"] == count
            and report["decoded_value_blocks"] == count and len(report["selected_srefs"]) == count
            and all(type(x) is int and x >= 0 for x in report["selected_srefs"])
            and report["selected_srefs"] == sorted(set(report["selected_srefs"]))
            and report["selected_srefs"][0] == 0, "decoded selection/count differs")
    for key, name in (("canonical", "canonical.bin"), ("root_canonical", "root-canonical.bin")):
        require(report[key]["file"] == name and report[key]["bytes"] == row[key]["bytes"], "canonical length/name differs")
        hash_field(report[key]["blake3"])
    require(report["solve_iterations"] == report["metadata"]["meta"]["iterations"], "solve iteration metadata differs")
    if operation == "stream-write":
        require(row["rewritten"]["path"] == str(directory / "output/rewritten.sol"), "rewrite path differs")
        verify(row["rewritten"])
        require(all(row["rewritten"][k] == item[k] for k in ("bytes", "sha256")), "rewritten input SHA/size differs")
        same_bytes(row["rewritten"]["path"], item["path"])
        require(report["rewritten"] == {"file": "rewritten.sol", **report["input"]}, "rewritten report differs")
    else:
        require("rewritten" not in row and report["rewritten"] is None, "unexpected rewritten output")
    return report


def analyze(state, plan):
    require(state["status"] == "completed", "incomplete campaign cannot publish comparison")
    require(state["schema"] == "r1.codec-campaign/v1" and state["plan_sha256"] == plan["plan_sha256"], "campaign plan differs")
    run_root = Path(state["run_root"])
    require(run_root.is_absolute() and str(run_root.resolve()) == state["run_root"], "invalid run root")
    rows = state["samples"]
    require(len(rows) == len(CASES) * len(OPERATIONS) * 8, "missing/extra samples")
    require(len({(r["case"], r["operation"], r["side"], r["repetition"]) for r in rows}) == len(rows),
            "duplicate samples")
    out = []
    for case in CASES:
        root_reference = None
        full_reference = None
        input_reference = None
        for operation in OPERATIONS:
            pairs = []
            for repetition in range(4):
                matched = [r for r in rows if (r["case"], r["operation"], r["repetition"]) == (case, operation, repetition)]
                require(len(matched) == 2 and {r["side"] for r in matched} == {"baseline", "candidate"}, "incomplete pair")
                a, b = sorted(matched, key=lambda r: r["side"])
                for row in matched:
                    report = validate_sample(row, plan, run_root)
                    if input_reference is None:
                        input_reference = report["input"]
                    require(input_reference == report["input"], "sample input BLAKE3 identity differs")
                    if root_reference is None:
                        root_reference = row["root_canonical"]["path"]
                    same_bytes(root_reference, row["root_canonical"]["path"])
                    if operation in ("decode-all", "stream-write"):
                        if full_reference is None:
                            full_reference = row["canonical"]["path"]
                        same_bytes(full_reference, row["canonical"]["path"])
                validate_pair(a, b)
                if repetition:
                    pairs.append((metric(a), metric(b)))
            baseline = statistics.median(a for a, _ in pairs)
            candidate = statistics.median(b for _, b in pairs)
            ratio = candidate / baseline
            faster = sum(b < a for a, b in pairs)
            out.append({"case": case, "operation": operation, "baseline_median_seconds": baseline,
                        "candidate_median_seconds": candidate, "candidate_over_baseline": ratio,
                        "paired_faster_count": faster, "raw_pairs_seconds": [list(pair) for pair in pairs],
                        "improvement_gate_met": ratio <= 0.95 and faster >= 2})
    return {"schema": "r1.codec-comparison/v1", "plan_sha256": state["plan_sha256"],
            "correctness": "pass", "rows": out, "scope": THRESHOLDS["scope"]}


def run(args):
    args.out = args.out.resolve()
    plan = read(args.plan)
    verify_plan(plan)
    require(dt.datetime.fromisoformat(plan["issued_at"]) <= dt.datetime.now(dt.timezone.utc), "future plan")
    args.out.mkdir(parents=True, exist_ok=False)
    state = {"schema": "r1.codec-campaign/v1", "status": "running", "started_at": now(),
             "run_root": str(args.out),
             "plan": identity(args.plan), "plan_sha256": plan["plan_sha256"], "samples": []}
    write(args.out / "result.json", state, exclusive=True)
    deadline = time.monotonic() + plan["limits"]["campaign_seconds"]
    try:
        for case in CASES:
            for operation in OPERATIONS:
                for repetition in range(4):
                    sides = ("candidate", "baseline") if repetition == 2 else ("baseline", "candidate")
                    pair = []
                    for side in sides:
                        require(time.monotonic() + plan["limits"]["timeout_seconds"] + 10 < deadline,
                                "campaign deadline insufficient for another full stage")
                        name = f"{case}-{operation}-{repetition}-{side}"
                        print(now(), name, flush=True)
                        row = sample(plan, case, operation, side, args.out / name)
                        row["repetition"] = repetition  # 0 is excluded warmup, not another measured sample.
                        pair.append(row)
                        state["samples"].append(row)
                        write(args.out / "result.json", state)
                    validate_pair(*pair)
        verify_plan(plan)
        state.update(status="completed", ended_at=now())
        comparison = analyze(state, plan)
        write(args.out / "comparison.json", comparison, exclusive=True)
        write(args.out / "result.json", state)
    except BaseException as exc:
        state.update(status="failed", ended_at=now(), error=f"{type(exc).__name__}: {exc}")
        write(args.out / "result.json", state)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    prepare_parser = sub.add_parser("prepare")
    for name in ("bundle", "sidecar", "out"):
        prepare_parser.add_argument("--" + name, type=Path, required=True)
    freeze_parser = sub.add_parser("freeze")
    for name in ("inputs", "build-record", "out"):
        freeze_parser.add_argument("--" + name, type=Path, required=True)
    freeze_parser.add_argument("--source-root", type=Path, default=REPO,
                               help="frozen candidate source root; runner package may live elsewhere")
    freeze_parser.add_argument("--timeout-seconds", type=float, default=120)
    freeze_parser.add_argument("--memory-limit-bytes", type=int, default=4 * 1024**3)
    freeze_parser.add_argument("--min-free-memory-bytes", type=int, default=2 * 1024**3)
    freeze_parser.add_argument("--disk-reserve-bytes", type=int, default=2 * 1024**3)
    run_parser = sub.add_parser("run")
    run_parser.add_argument("--plan", type=Path, required=True)
    run_parser.add_argument("--out", type=Path, required=True)
    check_parser = sub.add_parser("check")
    check_parser.add_argument("--run", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args)
    elif args.command == "freeze":
        freeze(args)
    elif args.command == "run":
        run(args)
    else:
        state = read(args.run / "result.json")
        verify(state["plan"])
        plan = read(state["plan"]["path"])
        verify_plan(plan, live_host=False)
        require(plan["plan_sha256"] == state["plan_sha256"], "plan identity differs")
        require(str(args.run.resolve()) == state["run_root"], "check run root differs")
        expected = analyze(state, plan)
        require(expected == read(args.run / "comparison.json"), "comparison differs")
        print(json.dumps({"status": "verified", "rows": len(expected["rows"]), "scope": expected["scope"]}))


if __name__ == "__main__":
    main()
