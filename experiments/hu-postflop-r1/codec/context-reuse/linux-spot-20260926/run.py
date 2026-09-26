"""Finite VM08 validation/build and three-arm release writer measurement.

The caller owns VM cost/lifetime, source extraction, outer cgroup and transfers.
Build stops before measurement, allowing completed evidence to be collected.
Python 3.11+ standard library; reuses the candidate's existing supervisor.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import itertools
import json
import math
import os
from pathlib import Path
import platform
import random
import re
import shutil
import statistics
import sys
import time

HERE = Path(__file__).resolve().parent
PROTOCOL = HERE / "protocol.json"
EXAMPLE = "crates/formats/examples/sol_codec_bench.rs"
SKIP = {".git", "target", "runs", ".cache", "__pycache__"}
SIGINT_TEST = "a_canceled_heads_up_solve_closes_as_canceled_and_resumes"


def require(value, message):
    if not value:
        raise ValueError(message)


def read(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(Path(path).read_text(encoding="utf-8-sig"), object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def save(path, value, *, initial=False):
    path = Path(path)
    require(not initial or not path.exists(), f"output already exists: {path}")
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("x", encoding="utf-8", newline="\n") as output:
        json.dump(value, output, indent=2, allow_nan=False)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(path)


def identity(path):
    path = Path(path).resolve(strict=True)
    before = path.stat()
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    after = path.stat()
    require((before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns),
            f"file changed while hashing: {path}")
    return {"path": str(path), "bytes": after.st_size, "sha256": digest.hexdigest()}


def content(value):
    return {key: value[key] for key in ("bytes", "sha256")}


def verify(value):
    require(identity(value["path"]) == value, f"identity changed: {value['path']}")


def inventory(root):
    root = Path(root).resolve(strict=True)
    result = {}
    for directory, subdirs, files in os.walk(root):
        subdirs[:] = sorted(name for name in subdirs if name not in SKIP)
        for name in subdirs + files:
            require(not (Path(directory) / name).is_symlink(), "source symlink is unsupported")
        for name in sorted(files):
            path = Path(directory) / name
            result[path.relative_to(root).as_posix()] = content(identity(path))
    return result


def host():
    require(sys.platform == "linux" and platform.machine() == "x86_64", "Linux x86_64 required")
    text = Path("/proc/cpuinfo").read_text()
    values = lambda field: sorted({line.split(":", 1)[1].strip() for line in text.splitlines()
                                  if line.split(":", 1)[0].strip() == field})
    return {"boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(),
            "machine": platform.machine(), "kernel": platform.release(),
            "logical_cpus": os.cpu_count(), "model": values("model name"),
            "vendor": values("vendor_id"), "flags": values("flags")}


def cgroup_limit():
    lines = Path("/proc/self/cgroup").read_text().splitlines()
    relative = next((line[3:] for line in lines if line.startswith("0::")), None)
    require(relative is not None, "cgroup v2 containment required")
    group = Path("/sys/fs/cgroup") / relative.lstrip("/")
    limits = []
    for parent in [group, *group.parents]:
        if parent == Path("/sys"):
            break
        file = parent / "memory.max"
        if file.is_file() and (value := file.read_text().strip()) != "max":
            limits.append(int(value))
    require(limits and min(limits) <= 6 * 1024**3, "finite outer MemoryMax <= 6 GiB required")
    return {"path": str(group), "effective_memory_max_bytes": min(limits)}


def order(protocol):
    arms = protocol["arms"]
    rng = random.Random(protocol["seed"])
    rows = []
    for case_index, case in enumerate(protocol["inputs"]):
        warm = arms[case_index:] + arms[:case_index]
        rows.extend({"case": case, "block": 0, "arm": arm, "warmup": True} for arm in warm)
        blocks = list(itertools.permutations(arms))
        blocks.append(tuple(warm))
        rng.shuffle(blocks)
        for block, permutation in enumerate(blocks, 1):
            rows.extend({"case": case, "block": block, "arm": arm, "warmup": False}
                        for arm in permutation)
    return rows


def source_inputs(configuration, protocol):
    require(set(configuration) == set(protocol["arms"]), "exactly three source arms required")
    sources = {}
    for arm in protocol["arms"]:
        entry = configuration[arm]
        root = Path(entry["root"]).resolve(strict=True)
        require(re.fullmatch(r"[0-9a-f]{40}", entry["revision"]), "full source revision required")
        if arm in protocol["revisions"]:
            require(entry["revision"] == protocol["revisions"][arm], f"wrong {arm} revision")
        manifest = identity(entry["manifest"])
        archive = identity(entry["archive"])
        require(not Path(manifest["path"]).is_relative_to(root)
                and not Path(archive["path"]).is_relative_to(root), "source evidence must be outside source")
        document = read(manifest["path"])
        require(document["base_commit"] == entry["revision"], "manifest revision differs")
        require(archive["sha256"] == document["archive_sha256"]
                and archive["bytes"] == document["archive_bytes"], "source archive differs")
        pins = {item["path"]: content(item) for item in document["files"]}
        require(len(pins) == len(document["files"]), "duplicate source file")
        require(inventory(root) == pins, f"extracted {arm} source differs from manifest")
        require(pins[EXAMPLE] == protocol["example"], "all arms need the identical benchmark example")
        if arm == "candidate":
            require(pins["crates/formats/src/sol_indexed.rs"] == protocol["candidate_writer"],
                    "candidate is not the adopted context reuse implementation")
        sources[arm] = {"root": str(root), "revision": entry["revision"],
                        "manifest": manifest, "archive": archive, "files": pins}
    roots = [Path(item["root"]) for item in sources.values()]
    require(all(not a.is_relative_to(b) for a in roots for b in roots if a != b)
            and len(set(roots)) == 3, "source roots must be separate")
    return sources


def validation_stages(plan):
    cargo = plan["tools"]["cargo"]["path"]
    python = plan["tools"]["python"]["path"]
    root = plan["sources"]["candidate"]["root"]
    commands = [
        ("toolchain", [plan["tools"]["rustc"]["path"], "-Vv"], 30),
        ("fmt", [cargo, "fmt", "--all", "--check"], 60),
        ("clippy", [cargo, "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"], 1800),
        ("workspace-tests", [cargo, "test", "--locked", "--workspace", "--", "--test-threads=2"], 2400),
        ("docs", [python, "tools/check_docs.py"], 60),
        ("python-tools", [python, "-m", "unittest", "discover", "-s", "tools/tests", "-v"], 300),
        ("sigint-build", [cargo, "test", "--locked", "--release", "-p", "cli", "--test", "cli_integration", SIGINT_TEST, "--no-run"], 1800),
        ("sigint-test", [cargo, "test", "--locked", "--release", "-p", "cli", "--test", "cli_integration", SIGINT_TEST, "--", "--ignored", "--exact", "--test-threads=1"], 180),
    ]
    stages = [{"label": name, "arm": "candidate", "kind": "validation", "argv": argv,
               "cwd": root, "timeout": timeout} for name, argv, timeout in commands]
    for arm in plan["protocol"]["arms"]:
        stages.append({"label": "release-" + arm, "arm": arm, "kind": "build",
                       "cwd": plan["sources"][arm]["root"], "timeout": 1800,
                       "argv": [cargo, "build", "--locked", "--release", "-p", "formats", "--example", "sol_codec_bench"]})
    return stages


def sample_stages(plan):
    root = Path(plan["output"])
    result = []
    for item in plan["order"]:
        label = f"{item['case']}-{item['block']}-{item['arm']}"
        result.append({**item, "label": label, "kind": "sample", "cwd": str(root),
                       "timeout": plan["protocol"]["limits"]["sample_seconds"],
                       "argv": [str(root / "binaries" / item["arm"]), plan["inputs"][item["case"]]["path"],
                                "stream-write", "1", str(root / "stages" / label / "output")]})
    return result


def pin_check(plan, *, live):
    for item in plan["tools"].values():
        verify(item)
    for item in plan["inputs"].values():
        verify(item)
    for source in plan["sources"].values():
        verify(source["archive"])
        verify(source["manifest"])
        require(inventory(source["root"]) == source["files"], "source changed")
    if live:
        require(host() == plan["host"], "CPU/boot changed; new builds and a new plan required")
        cgroup_limit()


def remaining(plan, timeout):
    deadline = dt.datetime.fromisoformat(plan["deadline_utc"]).timestamp()
    require(time.time() + timeout + 20 < deadline, "insufficient time for full bounded stage")


def environment(plan, arm):
    forbidden = {"RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
                 "CARGO_BUILD_RUSTFLAGS"}
    forbidden.update(key for key in os.environ if key.startswith("CARGO_TARGET_")
                     and (key.endswith("_RUSTFLAGS") or key.endswith("_LINKER")))
    require(not forbidden.intersection(os.environ), "unexpected compiler/flags/linker override")
    os.environ.update(plan["environment"])
    os.environ["CARGO_TARGET_DIR"] = plan["targets"][arm]
    os.environ.pop("R1_SOL_WRITE_PHASE_OUTPUT", None)
    os.environ["PATH"] = str(Path(plan["tools"]["cargo"]["path"]).parent) + ":/opt/r1/cargo/bin:" + os.environ["PATH"]


def supervisor(plan):
    spec = importlib.util.spec_from_file_location("vm08_supervisor", plan["tools"]["supervisor"]["path"])
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def stage_record_ok(record, stage):
    require(record["state"] == "completed" and record["supervisor_exit_code"] == 0
            and record["child_exit_code"] == 0 and record["cleanup_complete"]
            and record["identity_unchanged"] and not record["errors"]
            and not record.get("forced", False), f"stage did not complete cleanly: {stage['label']}")
    require(record["argv"] == stage["argv"] and record["cwd"] == stage["cwd"], "record command differs")
    limits = record["limits"]
    require(limits["timeout_seconds"] == stage["timeout"] and limits["memory_limit_bytes"] == 6 * 1024**3
            and limits["min_free_memory_bytes"] == 768 * 1024**2 and limits["disk_reserve_bytes"] == 4 * 1024**3
            and limits["grace_seconds"] == 5 and limits["kill_wait_seconds"] == 5, "record limits differ")
    for value in record["outputs"].values():
        verify(value)


def validation_report(stage, record):
    text = Path(record["outputs"]["stdout"]["path"]).read_text()
    if stage["label"] == "toolchain":
        require("release: 1.97.0" in text and "host: x86_64-unknown-linux-gnu" in text, "wrong Rust toolchain")
    summaries = [list(map(int, row)) for row in re.findall(
        r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", text)]
    if stage["label"] in ("workspace-tests", "sigint-test"):
        require(summaries and sum(row[0] for row in summaries) > 0 and all(row[1] == 0 for row in summaries),
                "requested Rust tests did not execute successfully")
    if stage["label"] == "workspace-tests":
        require(len(re.findall(r"test sol_indexed::context_reuse_tests::\w+ \.\.\. ok", text)) == 4,
                "all four new context tests must run in workspace validation")
    if stage["label"] == "sigint-test":
        require(f"test {SIGINT_TEST} ... ok" in text and len(summaries) == 1
                and summaries[0][:3] == [1, 0, 0], "requested Unix SIGINT test did not execute")
    return {"test_summaries": summaries,
            "totals": [sum(row[index] for row in summaries) for index in range(5)]}


def execute(plan, state, stage):
    remaining(plan, stage["timeout"])
    pin_check(plan, live=True)
    environment(plan, stage["arm"])
    if stage["kind"] == "sample":
        verify(state["binaries"][stage["arm"]])
    root = Path(plan["output"])
    directory = root / "stages" / stage["label"]
    directory.mkdir()
    limits = plan["protocol"]["limits"]
    fixed = [root / "plan.json", Path(plan["tools"]["runner"]["path"]), PROTOCOL,
             Path(plan["tools"]["rustc"]["path"])]
    fixed.extend(Path(value["path"]) for value in plan["inputs"].values())
    args = ["--record", str(directory / "supervisor.json"), "--cwd", stage["cwd"],
            "--timeout-seconds", str(stage["timeout"]), "--memory-limit-bytes", str(limits["rss_bytes"]),
            "--min-free-memory-bytes", str(limits["min_free_bytes"]), "--disk-reserve-bytes", str(limits["disk_reserve_bytes"]),
            "--disk-path", str(root), "--grace-seconds", "5", "--kill-wait-seconds", "5", "--poll-seconds", "0.1"]
    for path in fixed:
        args += ["--identity-file", str(path)]
    entry = {"label": stage["label"], "stage": stage, "status": "running", "host_before": host()}
    state["stages"].append(entry)
    save(root / "result.json", state)
    remaining(plan, stage["timeout"])
    code = supervisor(plan).main(args + ["--", *stage["argv"]])
    entry.update(supervisor_exit=code, host_after=host(), record=identity(directory / "supervisor.json"))
    record = read(directory / "supervisor.json")
    entry["status"] = "passed" if code == 0 else "failed"
    save(root / "result.json", state)
    require(code == 0, f"supervisor failed: {stage['label']}")
    stage_record_ok(record, stage)
    pin_check(plan, live=True)
    require(entry["host_before"] == entry["host_after"] == plan["host"], "host changed during stage")
    if stage["kind"] == "validation":
        entry["validation"] = validation_report(stage, record)
    if stage["kind"] == "build":
        compiled = Path(plan["targets"][stage["arm"]]) / "release/examples/sol_codec_bench"
        destination = root / "binaries" / stage["arm"]
        original = identity(compiled)
        shutil.copy2(compiled, destination)
        binary = identity(destination)
        require(content(binary) == content(original), "binary copy differs")
        state["binaries"][stage["arm"]] = binary
        entry["compiled_binary"] = original
    if stage["kind"] == "sample":
        entry["sample"] = inspect_sample(plan, state, stage, record)
    save(root / "result.json", state)
    print(json.dumps({"stage": stage["label"], "status": entry["status"]}), flush=True)


def equal_files(first, second):
    with Path(first).open("rb") as left, Path(second).open("rb") as right:
        while True:
            a, b = left.read(1024 * 1024), right.read(1024 * 1024)
            require(a == b, f"different bytes: {first}, {second}")
            if not a:
                return


def inspect_sample(plan, state, stage, record):
    binary = state["binaries"][stage["arm"]]
    input_pin = plan["inputs"][stage["case"]]
    for key in ("identity_before", "identity_after"):
        require(binary in record[key] and input_pin in record[key], "sample binary/input binding differs")
    output = Path(stage["argv"][-1])
    report = read(output / "result.json")
    require(report["schema"] == "r1.sol-codec-sample/v1" and report["status"] == "completed"
            and report["operation"] == "stream-write" and report["iterations"] == 1
            and report["format_version"] == 3 and report["metadata"]["mode"] == "Full",
            "incorrect sample report")
    require(report["input"]["bytes"] == input_pin["bytes"] and report["rewritten"]["bytes"] == input_pin["bytes"],
            "incorrect input/output length")
    duration = report["timing"]["operation_seconds"]
    require(isinstance(duration, (float, int)) and math.isfinite(duration) and duration > 0, "invalid writer timing")
    files = {name: identity(output / name) for name in ("result.json", "rewritten.sol", "canonical.bin", "root-canonical.bin")}
    require(content(files["rewritten.sol"]) == content(input_pin), "rewritten SHA differs")
    equal_files(files["rewritten.sol"]["path"], input_pin["path"])
    for name, field in (("canonical.bin", "canonical"), ("root-canonical.bin", "root_canonical")):
        require(report[field]["file"] == name and report[field]["bytes"] == files[name]["bytes"], "canonical report differs")
    return {"operation_seconds": duration, "files": files}


def summarize(plan, state):
    metrics = {}
    for case in plan["protocol"]["inputs"]:
        timings = {arm: [] for arm in plan["protocol"]["arms"]}
        reference = None
        for entry in state["stages"]:
            stage = entry["stage"]
            if stage["kind"] != "sample" or stage["case"] != case:
                continue
            files = entry["sample"]["files"]
            if reference is None:
                reference = files
            for name in ("canonical.bin", "root-canonical.bin", "rewritten.sol"):
                require(content(files[name]) == content(reference[name]), "cross-arm/iteration SHA differs")
                equal_files(files[name]["path"], reference[name]["path"])
            if not stage["warmup"]:
                timings[stage["arm"]].append((stage["block"], entry["sample"]["operation_seconds"]))
        raw = {arm: [value for _, value in sorted(values)] for arm, values in timings.items()}
        require(all(len(values) == 7 for values in raw.values()), "seven blocks required")
        medians = {arm: statistics.median(values) for arm, values in raw.items()}
        comparisons = {}
        for a, b in (("candidate", "bulk"), ("candidate", "legacy"), ("bulk", "legacy")):
            comparisons[f"{a}_over_{b}"] = {"median_ratio": medians[a] / medians[b],
                                           "paired_ratios": [x / y for x, y in zip(raw[a], raw[b])],
                                           "strictly_faster_blocks": sum(x < y for x, y in zip(raw[a], raw[b]))}
        metrics[case] = {"seconds": raw, "medians": medians, "comparisons": comparisons}
    screen = plan["protocol"]["screen"]
    supports = all(case["comparisons"]["candidate_over_bulk"]["median_ratio"] <= screen["all_cases_candidate_over_bulk_max"]
                   and case["comparisons"]["candidate_over_legacy"]["median_ratio"] <= screen["all_cases_candidate_over_legacy_max"]
                   for case in metrics.values())
    flop = metrics["flop"]["comparisons"]["candidate_over_bulk"]
    supports = supports and flop["median_ratio"] <= screen["flop_candidate_over_bulk_max"] and flop["strictly_faster_blocks"] >= screen["flop_strictly_faster_blocks_min"]
    return {"cases": metrics, "descriptive_adoption_screen": supports, "r1_acceptance": None,
            "canonical_and_original_sol_byte_equality": True}


def check(root, *, ready=False):
    root = Path(root).resolve(strict=True)
    plan, state = read(root / "plan.json"), read(root / "result.json")
    require(state["plan"] == identity(root / "plan.json"), "plan changed")
    require(plan["protocol"] == read(PROTOCOL), "protocol differs")
    pin_check(plan, live=False)
    require(plan["order"] == order(plan["protocol"]), "sample order changed")
    expected = validation_stages(plan) + ([] if ready else sample_stages(plan))
    require(len(state["stages"]) == len(expected), "missing/extra stage")
    require(state["status"] == ("ready_for_measurement" if ready else "completed"), "run incomplete")
    for entry, stage in zip(state["stages"], expected):
        require(entry["stage"] == stage and entry["status"] == "passed", "stage differs")
        verify(entry["record"])
        record = read(entry["record"]["path"])
        stage_record_ok(record, stage)
        require(entry["host_before"] == entry["host_after"] == plan["host"], "recorded host changed")
        if stage["kind"] == "build":
            verify(entry["compiled_binary"])
            require(content(entry["compiled_binary"]) == content(state["binaries"][stage["arm"]]), "build binary differs")
        elif stage["kind"] == "sample":
            require(inspect_sample(plan, state, stage, record) == entry["sample"], "sample changed")
        else:
            require(validation_report(stage, record) == entry["validation"], "validation report changed")
    for binary in state["binaries"].values():
        verify(binary)
    require(set(state["binaries"]) == set(plan["protocol"]["arms"]), "missing binary")
    if not ready:
        require(summarize(plan, state) == state["comparison"], "reported comparison differs")
    return plan, state


def build(args):
    protocol = read(PROTOCOL)
    host_identity = host()
    require(host_identity["logical_cpus"] == 2, "VM08 requires two logical CPUs")
    containment = cgroup_limit()
    sources = source_inputs(read(args.sources), protocol)
    output, targets_root = args.out.resolve(), args.targets_root.resolve()
    require(not output.exists() and not targets_root.exists(), "fresh output and target root required")
    for path in (output, targets_root):
        require(not any(path.is_relative_to(Path(s["root"])) for s in sources.values()), "output/target cannot be within source")
    output.mkdir(parents=True)
    targets_root.mkdir(parents=True)
    (output / "stages").mkdir()
    (output / "binaries").mkdir()
    targets = {arm: str(targets_root / arm) for arm in protocol["arms"]}
    for path in targets.values():
        Path(path).mkdir()
    toolchain = args.toolchain.resolve(strict=True)
    tools = {name: identity(toolchain / name) for name in ("cargo", "rustc", "rustdoc", "clippy-driver", "rustfmt")}
    tools.update(python=identity(sys.executable), runner=identity(__file__), protocol=identity(PROTOCOL),
                 sources_config=identity(args.sources), supervisor=identity(Path(sources["candidate"]["root"]) / "tools/run_supervised.py"))
    inputs = {case: identity(args.inputs / (case + ".sol")) for case in protocol["inputs"]}
    require(all(content(inputs[case]) == expected for case, expected in protocol["inputs"].items()), "wrong fixed input")
    deadline = dt.datetime.fromisoformat(args.deadline_utc.replace("Z", "+00:00"))
    require(deadline.tzinfo is not None and 0 < deadline.timestamp() - time.time() <= 6 * 3600, "deadline must be within six hours")
    plan = {"schema": "r1.context-linux-plan/v1", "protocol": protocol, "output": str(output), "host": host_identity,
            "outer_cgroup_at_build": containment, "sources": sources, "targets": targets, "tools": tools, "inputs": inputs,
            "deadline_utc": deadline.isoformat(), "order": order(protocol),
            "environment": {"CARGO_HOME": "/opt/r1/cargo", "RUSTUP_HOME": "/opt/r1/rustup", "RUSTUP_TOOLCHAIN": "1.97.0",
                            "CARGO_BUILD_JOBS": "1", "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "2",
                            "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "CARGO_INCREMENTAL": "0",
                            "RUSTC": tools["rustc"]["path"], "RUSTDOC": tools["rustdoc"]["path"]}}
    save(output / "plan.json", plan, initial=True)
    state = {"schema": "r1.context-linux-result/v1", "plan": identity(output / "plan.json"),
             "status": "building", "stages": [], "binaries": {}}
    save(output / "result.json", state, initial=True)
    for stage in validation_stages(plan):
        execute(plan, state, stage)
    state["status"] = "ready_for_measurement"
    save(output / "result.json", state)
    check(output, ready=True)
    print(json.dumps({"status": state["status"], "out": str(output)}), flush=True)


def measure(args):
    plan, state = check(args.run, ready=True)
    pin_check(plan, live=True)
    state["status"] = "measuring"
    save(Path(plan["output"]) / "result.json", state)
    for stage in sample_stages(plan):
        execute(plan, state, stage)
    state["comparison"] = summarize(plan, state)
    state["status"] = "completed"
    save(Path(plan["output"]) / "result.json", state)
    check(plan["output"])
    print(json.dumps({"status": "completed", "comparison": state["comparison"]}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--phase", choices=("build", "measure"), required=True)
    for name in ("sources", "inputs", "out", "targets-root", "run"):
        run.add_argument("--" + name, type=Path)
    run.add_argument("--deadline-utc")
    run.add_argument("--toolchain", type=Path, default=Path("/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin"))
    check_parser = commands.add_parser("check")
    check_parser.add_argument("--run", type=Path, required=True)
    check_parser.add_argument("--ready", action="store_true")
    args = parser.parse_args()
    if args.command == "check":
        check(args.run, ready=args.ready)
        print(json.dumps({"status": "verified", "ready_only": args.ready}))
        return
    root = args.out if args.phase == "build" else args.run
    try:
        if args.phase == "build":
            require(all(getattr(args, key) is not None for key in ("sources", "inputs", "out", "targets_root", "deadline_utc")), "build arguments missing")
            build(args)
        else:
            require(args.run is not None, "measure requires --run")
            measure(args)
    except BaseException as error:
        if root is not None and (root / "result.json").is_file():
            state = read(root / "result.json")
            state.update(status="failed", error=f"{type(error).__name__}: {error}")
            save(root / "result.json", state)
        raise


if __name__ == "__main__":
    main()
