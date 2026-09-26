"""Current-only bounded worker endpoints; retained payloads are never executable code."""
from __future__ import annotations
import argparse
import datetime as dt
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import struct
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
EXACT = HERE.parent / "exact-mass/run.py"
spec = importlib.util.spec_from_file_location("trusted_current32_exact", EXACT)
exact = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exact)
core = exact.shared
decode, digest = core.decode, core.digest
quality_values, stopping_check, common_header = exact.quality_values, exact.stopping_check, exact.common_header
require, read, save, identity, content, join, timestamp = (
    getattr(core, k) for k in ("require", "read", "save", "identity", "content", "join", "timestamp"))
ENV = {"RUSTUP_TOOLCHAIN": "1.97.0", "CARGO_BUILD_JOBS": "2", "CARGO_INCREMENTAL": "0",
       "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "CARGO_PROFILE_RELEASE_DEBUG": "0",
       "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "2", "RUSTFLAGS": "-C target-cpu=native"}


def controls():
    return {**{name: HERE / name for name in ("run.py", "protocol.json", "source-pins.json")},
            "shared/exact-run.py": EXACT, "shared/showdown-run.py": core.__file__}


def schedule(protocol):
    result = []
    for case_index, (case, definition) in enumerate(protocol["cases"].items()):
        for block in range(4):
            n = (case_index + block) % 3
            order = protocol["threads"][n:] + protocol["threads"][:n]
            for threads in order:
                result.append({"case": case, "block": block, "warmup": block == 0, "threads": threads,
                               "iterations": definition["iterations"], "label": f"{case}-b{block}-t{threads}"})
    return result


def host_record(machine, protocol):
    require(machine["machine"] == "x86_64" and machine["logical_cpus"] == 32
            and len(set(machine["affinity"])) == 32, "32 logical CPU full affinity required")
    require([row["cpu"] for row in machine["topology"]] == machine["affinity"]
            and machine["boot_id"] and machine["cpu_models"], "host topology missing")
    cg = machine["cgroup"]
    require(cg["memory_max"] != "max" and 0 < int(cg["memory_max"]) <= protocol["limits"]["outer_memory_max_bytes"]
            and cg["swap_max"] == "0" and cg["cpu_weight"] == "100", "memory/swap/CPUWeight bound differs")
    require(cg["cpu_limits"], "CPU controller limits missing")
    for value in cg["cpu_limits"].values():
        quota, period = value.split()
        require(quota == "max" or int(quota) / int(period) >= 32, "CPU quota below32")


def host(protocol):
    require(sys.platform == "linux" and platform.machine() == "x86_64", "Linux x86_64 required")
    affinity = sorted(os.sched_getaffinity(0))
    cg = Path("/sys/fs/cgroup") / Path("/proc/self/cgroup").read_text().strip().split("::", 1)[1].lstrip("/")
    topology = []
    for cpu in affinity:
        root = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology")
        topology.append({"cpu": cpu, "core": (root / "core_id").read_text().strip(),
                         "socket": (root / "physical_package_id").read_text().strip()})
    machine = {"machine": platform.machine(), "logical_cpus": os.cpu_count(), "affinity": affinity, "topology": topology,
        "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(),
        "cpu_models": sorted(set(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name"))),
        "cgroup": {"path": str(cg), "memory_max": (cg / "memory.max").read_text().strip(),
                   "swap_max": (cg / "memory.swap.max").read_text().strip(), "cpu_weight": (cg / "cpu.weight").read_text().strip(),
                   "cpu_limits": {str(p): (p / "cpu.max").read_text().strip() for p in (cg, *cg.parents)
                                  if p.is_relative_to("/sys/fs/cgroup") and (p / "cpu.max").is_file()}}}
    host_record(machine, protocol)
    return machine


def foundation(store, plan, directory):
    directory = Path(directory)
    pin = identity(directory / "plan.json")
    require(content(pin) == plan["protocol"]["foundation"]["plan"], "wrong historical foundation plan")
    previous = read(directory / "plan.json")
    retained = core.Store(directory)
    old_arm = previous["arms"]["new"]
    result = exact.validate_build(retained, old_arm, "new", previous["host"]["boot_id"])
    require(result["tests"]["workspace-tests"] == plan["protocol"]["foundation"]["workspace"], "historical workspace counts differ")
    require(result["tests"]["release-oracle"]["passed"] == 3
            and result["tests"]["release-river-resolve"]["passed"] == 1, "historical release validation differs")
    for field, key in (("manifest", "source_manifest"), ("archive", "source_archive")):
        require(content(plan["source"][field]) == content(old_arm[field]) == plan["protocol"]["foundation"][key], "historical source differs")
    manifest = core.source_files(store, plan["source"])
    pins = store.json(plan["controls"]["source-pins.json"]["path"])
    require(pins["revision"] == plan["protocol"]["revision"], "wrong production revision")
    inventory = {row["path"]: content(row) for row in manifest["files"]}
    require(all(inventory.get(row["path"]) == content(row) for row in pins["files"]), "production closure differs")
    return {"scope": "reused historical source-identical validation; not a workspace rerun", **result}


def command(plan, builds, stage):
    case = plan["protocol"]["cases"][stage["case"]]
    return [builds["binary"]["path"], "--config", plan["inputs"][stage["case"]]["path"], "--threads", str(stage["threads"]),
            "--iterations", str(case["iterations"]), "--layout", "compact", "--target-nash-conv", str(case["target_nash_conv"]),
            "--check-every", str(case["check_every"]), "--out", join(plan["output"], "stages", stage["label"], "bench")]


def build_command(plan, stage):
    if stage["label"] == "toolchain":
        return [plan["tools"]["rustc"]["path"], "-Vv"]
    return [plan["tools"]["cargo"]["path"], "build", "--release", "--locked", "--offline", "-p", "cli", "--example",
            "hu_scaling_bench", "--target-dir", plan["target"]]


def build_schedule():
    return [{"label": "toolchain", "timeout_seconds": 30}, {"label": "release-example", "timeout_seconds": 1200}]


def suffix(state, expected):
    require([row["stage"] for row in state["stages"]] == expected, "fixed schedule differs")
    ended = False
    for entry in state["stages"]:
        status = entry["status"]
        require(status in ("passed", "failed", "skipped", "pending"), "nonterminal stage status")
        if status == "passed":
            require(not ended, "passed stage after failure/skip")
        elif status == "failed":
            require(not ended and entry.get("error"), "invalid failed suffix")
            ended = True
        else:
            require(status != "skipped" or entry.get("reason"), "skip lacks reason")
            ended = True
    if state["status"] == "completed":
        require(all(row["status"] == "passed" for row in state["stages"]), "completed with incomplete stages")
    elif state["status"] == "ready":
        require(all(row["status"] == "pending" for row in state["stages"]), "ready after execution")
    else:
        require(state["status"] == "failed" and state.get("error") and all(row["status"] != "pending" for row in state["stages"]), "failure suffix incomplete")


def summarize(protocol, entries):
    cases = {}
    for case in protocol["cases"]:
        workers = {}
        for threads in protocol["threads"]:
            rows = [entry["sample"] for entry in entries if entry["stage"]["case"] == case
                    and entry["stage"]["threads"] == threads and not entry["stage"]["warmup"]]
            require(len(rows) == 3, "incomplete measured worker group")
            times = [row["run_seconds"] for row in rows]
            require(all(math.isfinite(x) and x > 0 for x in times), "invalid measured duration")
            workers[str(threads)] = {"run_seconds": times, "median_seconds": statistics.median(times),
                "iterations": [row["iterations"] for row in rows], "quality": rows[0]["quality"],
                "full_process_seconds": [row["full_process_seconds"] for row in rows],
                "memory": [row["full_process_memory"] for row in rows],
                "phase_sampled_rss": [row["run_phase_sampled_peak_tree_resident_bytes"] for row in rows]}
        for threads in protocol["threads"]:
            row = workers[str(threads)]
            row["speedup_over_one"] = workers["1"]["median_seconds"] / row["median_seconds"]
            row["efficiency"] = row["speedup_over_one"] / threads
        cases[case] = {"workers": workers, "ratio_32_over_16": workers["32"]["median_seconds"] / workers["16"]["median_seconds"],
            "32_slower_than16": workers["32"]["median_seconds"] > workers["16"]["median_seconds"],
            "fastest_observed_workers": min(protocol["threads"], key=lambda n: workers[str(n)]["median_seconds"])}
    return {"cases": cases, "scope": "three endpoint worker counts; descriptive only, not optimal-worker search or external quality",
            "timing_scope": "solve plus every stopping EV/BR check, excluding initialization and post-stop capture",
            "memory_claim": None, "speedup_required": False, "r1_certification": False}


def pins(plan, store, builds=None):
    result = [*plan["controls"].values(), *plan["tools"].values(), plan["python"], plan["supervisor"],
              plan["source"]["manifest"], plan["source"]["archive"], *plan["inputs"].values(),
              store.pin(join(plan["output"], "plan.json"))]
    if builds is not None:
        result += [builds["binary"], store.pin(join(plan["output"], "build.json"))]
    return result


def live(plan, store, builds=None):
    for pin in pins(plan, store, builds):
        require(identity(pin["path"]) == pin, "live identity changed: " + pin["path"])
    core.live_source(plan["source"])
    require(host(plan["protocol"]) == plan["host"], "live host changed")


def retain_stage(store, directory, tools):
    errors = []
    try:
        if (directory / "supervisor.json").is_file():
            core.retain_record(store, directory / "supervisor.json", tools, tolerate_identity_failure=True)
    except (ValueError, OSError) as error:
        errors.append(repr(error))
    for path in sorted(directory.rglob("*")):
        try:
            require(not path.is_symlink(), "output symlink")
            if path.is_file():
                store.add(path, changed_identity=True)
        except (ValueError, OSError) as error:
            errors.append(repr(error))
    require(not errors, "retention errors: " + "; ".join(errors))


def verify_record(store, plan, builds, entry, *, building, previous_end):
    passed = entry["status"] == "passed"
    record = core.record_bytes(store, entry["record"], [plan["python"], *plan["tools"].values()], success=passed)
    stage = entry["stage"]
    directory = "build-stages" if building else "stages"
    require(entry["record"]["path"] == join(plan["output"], directory, stage["label"], "supervisor.json"), "record location differs")
    expected = build_command(plan, stage) if building else command(plan, builds, stage)
    require(record["argv"] == expected and record["cwd"] == (plan["source"]["source"] if building else plan["output"]), "command/cwd differs")
    seconds = stage["timeout_seconds"] if building else 300
    for key, value in core.LIMIT_KEYS.items():
        require(record["limits"][key] == (seconds if key == "timeout_seconds" else plan["protocol"]["limits"][value]), "process bounds differ")
    if passed:
        require(record["resolved_argv"] == expected and entry["supervisor_exit"] == 0
                and record["runtime"]["logical_cpus"] == 32 and record["runtime"]["machine"] == "x86_64", "process runtime differs")
        require(all(pin in record["identity_before"] for pin in pins(plan, store, None if building else builds)), "process pin missing")
        require(entry["host_before"] == entry["host_after"] == plan["host"] and entry["source_after_verified"] is True, "host/source changes")
        require(entry["environment"] == {**ENV, "RUSTC": plan["tools"]["rustc"]["path"]}, "environment differs")
        start, end = timestamp(record["created_at"]), timestamp(record["ended_at"])
        require(previous_end <= start <= end <= timestamp(plan["deadline_utc"]), "process chronology/deadline differs")
        return record, end
    return record, previous_end


def sample(store, plan, stage, record):
    directory = join(plan["output"], "stages", stage["label"], "bench")
    report = store.json(join(directory, "result.json"))
    require(report["schema"] == "r1.hu-scaling-bench/v1" and report["status"] == "completed"
            and report["storage"] == "f32" and report["layout"] == "compact" and report["threads"] == stage["threads"]
            and 0 < report["iterations"] <= stage["iterations"] and report["config"] == plan["inputs"][stage["case"]]["path"], "benchmark invocation differs")
    require(store.json(record["outputs"]["stdout"]["path"]) == report, "stdout report differs")
    require(content(store.pin(join(directory, "config.original.toml"))) == content(plan["inputs"][stage["case"]]), "config copy differs")
    require(0 < report["timing"]["run_seconds"] <= record["elapsed_seconds"], "invalid solve timer")
    require(isinstance(record["measurement"]["root_os_peak_resident_bytes"], int)
            and record["measurement"]["root_os_peak_resident_bytes"] >= 0
            and record["measurement"]["root_os_peak_source"] == "wait4.ru_maxrss_linux_kib", "native Linux peak RSS missing")
    events = [decode(line) for line in store.data(record["outputs"]["stderr"]["path"]).splitlines() if line.strip()]
    events = [event for event in events if event.get("event") == "phase" and event.get("phase") == "run"]
    require([event["status"] for event in events] == ["started", "completed"], "solve phase events missing")
    start, end = [event["process_elapsed_seconds"] for event in events]
    require(0 <= start <= end <= record["elapsed_seconds"] and report["timing"]["run_seconds"] <= end - start + 1e-8,
            "solve timer is outside recorded run phase")
    require(all(math.isfinite(value) and value >= 0 for value in report["timing"].values() if isinstance(value, (float, int))), "nonfinite timing")
    quality, canonical, counts = report["quality"], report["canonical"], report["counts"]
    gains, nc = quality_values(quality)
    require(quality["deviation_gains"] == gains and quality["exploitability_nash_conv_over_two"] == nc / 2,
            "quality derived fields differ")
    require(all(math.isfinite(v) for key in ("subgame_ev", "subgame_br") for v in quality[key]), "nonfinite reported quality")
    stop = stopping_check(report, plan["protocol"]["cases"][stage["case"]])
    require(stop["target_met"], "NashConv target not reached at iteration cap")
    for values, bits in ((quality["solver_ev"], quality["solver_ev_f64_bits_hex"]),
                         (quality["solver_br"], quality["solver_br_f64_bits_hex"])):
        require(len(values) == len(bits) == 2 and all(math.isfinite(v) and struct.pack(">d", v).hex() == b for v, b in zip(values, bits)), "quality bits differ")
    require(struct.pack(">d", quality["nash_conv"]).hex() == quality["nash_conv_f64_bits_hex"] and math.isfinite(quality["nash_conv"]), "NashConv bits differ")
    combos = canonical["global_combos"]
    require(len(combos) == 2 and all(row == sorted(set(row)) and all(0 <= v < 1326 for v in row) for row in combos), "invalid support IDs")
    require(counts["root_dims"] == counts["retained_support_counts"] == [len(row) for row in combos]
            and canonical["union_global_combos"] == sorted(set(sum(combos, []))), "support counts differ")
    artifacts, headers = {}, []
    for field, name in (("strategy_and_cfv", "canonical.bin"), ("supported_state", "state.bin")):
        pin = store.pin(join(directory, name))
        require(canonical[field]["file"] == name and canonical[field]["bytes"] == pin["bytes"]
                and re.fullmatch(r"[0-9a-f]{64}", canonical[field]["blake3"]), "artifact metadata differs")
        artifacts[name] = pin
        headers.append(common_header(store.data(pin["path"]), b"HUCAN001" if name == "canonical.bin" else b"HUSTA001", report))
    require(headers[0] == headers[1], "canonical/state common header differs")
    rows = [decode(line) for line in store.data(record["outputs"]["samples"]["path"]).splitlines() if line.strip()]
    wall_start, wall_end = [event["unix_ms"] / 1000 for event in events]
    phase_rss = [row["tree_resident_bytes"] for row in rows if wall_start <= timestamp(row["at"]) <= wall_end]
    return {"run_seconds": report["timing"]["run_seconds"], "timing": report["timing"], "counts": counts,
            "iterations": report["iterations"], "stopping": stop, "common_header": digest(headers[0]),
            "run_phase_sampled_peak_tree_resident_bytes": max(phase_rss, default=None),
            "quality": quality, "global_combos": combos, "union_global_combos": canonical["union_global_combos"],
            "artifacts": artifacts, "normalized_config": store.pin(join(directory, "config.normalized.toml")),
            "algorithm": report["algorithm"], "rake": report["rake"], "utility": report["utility"],
            "full_process_seconds": record["elapsed_seconds"], "full_process_memory": record["measurement"]}


def same_solution(store, first, second):
    exact.same_solution(store, first, second)
    # JSON numeric equality treats -0.0 as +0.0; explicitly bind every stopping
    # point's f64 bits, as well as the final native bit fields checked above.
    for a, b in zip(first["stopping"]["checks"], second["stopping"]["checks"]):
        for key in ("solver_ev", "solver_br", "nash_conv"):
            av = a[key] if isinstance(a[key], list) else [a[key]]
            bv = b[key] if isinstance(b[key], list) else [b[key]]
            require([struct.pack("<d", x) for x in av] == [struct.pack("<d", x) for x in bv], "trajectory bits differ")


def validate_plan(store, plan):
    require(plan["schema"] == "r1.current-scaling32-plan/v1" and plan["protocol"] == read(HERE / "protocol.json"), "plan/protocol differs")
    require(plan["schedule"] == schedule(plan["protocol"]), "plan schedule differs")
    require(set(plan["controls"]) == set(controls()), "control set differs")
    for name, path in controls().items():
        store.verify(plan["controls"][name])
        require(content(plan["controls"][name]) == content(identity(path)), "trusted control differs: " + name)
    require(plan["environment"] == {**ENV, "RUSTC": plan["tools"]["rustc"]["path"]}, "fixed environment differs")
    require(0 < timestamp(plan["deadline_utc"]) - timestamp(plan["created_at"]) <= plan["protocol"]["maximum_campaign_seconds"], "deadline beyond one hour")
    host_record(plan["host"], plan["protocol"])
    manifest = core.source_files(store, plan["source"])
    for key, field in (("manifest", "source_manifest"), ("archive", "source_archive")):
        require(content(plan["source"][key]) == plan["protocol"]["foundation"][field], "source differs from frozen validation")
    require(plan["supervisor"] == store.pin(join(plan["source"]["source"], "tools/run_supervised.py")), "supervisor outside pinned source")
    require(set(plan["inputs"]) == set(plan["protocol"]["cases"]), "input set differs")
    for case, definition in plan["protocol"]["cases"].items():
        require(plan["inputs"][case] == store.pin(join(plan["source"]["source"], plan["protocol"]["config_directory"], definition["file"])), "input/source binding differs")
    for pin in [plan["python"], *plan["tools"].values()]:
        require(pin["bytes"] > 0 and re.fullmatch(r"[0-9a-f]{64}", pin["sha256"]), "tool identity invalid")
    require(plan["fresh_target_absent_before_prepare"] is True, "target was not fresh")
    roots = [Path(plan["source"]["source"]), Path(plan["output"]), Path(plan["target"])]
    require(not any(a == b or a.is_relative_to(b) or b.is_relative_to(a) for i, a in enumerate(roots) for b in roots[i+1:]), "source/output/target overlap")
    return manifest


def check(out, foundation_proof):
    out = Path(out)
    if (out / "prepare-failure.json").exists():
        failure = read(out / "prepare-failure.json")
        require(failure["status"] == "failed" and failure["error"], "invalid prepare failure")
        retained = (out / "retention.json").is_file()
        if retained:
            core.Store(out)
        return {"schema": "r1.current-scaling32-verification/v1", "status": "failed", "provenance_complete": False,
                "payload_integrity": "verified" if retained else "unavailable", "scope": "available retained bytes only", "error": failure["error"]}
    store = core.Store(out)
    plan, builds, state = read(out / "plan.json"), read(out / "build.json"), read(out / "result.json")
    require(store.data(join(plan["output"], "plan.json")) == (out / "plan.json").read_bytes(), "plan raw bytes differ")
    validate_plan(store, plan)
    prior = foundation(store, plan, foundation_proof)
    require(builds["schema"] == "r1.current-scaling32-build/v1" and state["schema"] == "r1.current-scaling32-result/v1", "result schema differs")
    suffix(builds, build_schedule())
    suffix(state, plan["schedule"])
    if builds["status"] != "completed":
        require(state["status"] in ("ready", "failed") and not any(x["status"] == "passed" for x in state["stages"]), "measurement before completed build")
    else:
        store.verify(builds["binary"])
        require(builds["binary"]["path"] == join(plan["target"], "release/examples/hu_scaling_bench"), "native build binary path differs")
        require(store.data(join(plan["output"], "build.json")) == (out / "build.json").read_bytes(), "retained build result differs")
    previous, count = timestamp(plan["created_at"]), 0
    for entry in builds["stages"]:
        if "record" not in entry:
            require(entry["status"] != "passed", "passed build record missing")
            continue
        record, previous = verify_record(store, plan, None, entry, building=True, previous_end=previous)
        if entry["status"] == "passed":
            count += 1
            if entry["stage"]["label"] == "toolchain":
                text = store.data(record["outputs"]["stdout"]["path"]).decode()
                require("release: 1.97.0" in text and "host: x86_64-unknown-linux-gnu" in text, "wrong native compiler")
            else:
                require(entry["binary_after"] == builds.get("binary"), "compiled binary binding differs")
    seen, passed = {}, []
    for entry in state["stages"]:
        if "record" not in entry:
            require(entry["status"] != "passed", "passed sample record missing")
            continue
        record, previous = verify_record(store, plan, builds, entry, building=False, previous_end=previous)
        if entry["status"] != "passed":
            continue
        actual = sample(store, plan, entry["stage"], record)
        require(actual == entry["sample"], "saved sample differs")
        case = entry["stage"]["case"]
        if case in seen:
            same_solution(store, seen[case], actual)
        seen.setdefault(case, actual)
        passed.append(entry)
    summary = summarize(plan["protocol"], passed) if state["status"] == "completed" else None
    require(state.get("summary") == summary, "incomplete or altered summary")
    status = "failed" if builds["status"] == "failed" or state["status"] == "failed" else state["status"]
    return {"schema": "r1.current-scaling32-verification/v1", "status": status, "payload_integrity": "verified",
            "provenance_complete": True, "historical_validation": prior, "build_stages_passed": count,
            "samples_passed": len(passed), "warmups_passed": sum(x["stage"]["warmup"] for x in passed),
            "summary": summary, "host": plan["host"], "tool_retention": "compiler/Python identities only; measured binary and all raw outputs retained"}


def prepare(args):
    out = args.out.resolve()
    require(not out.exists(), "new output required")
    out.mkdir(parents=True)
    try:
        protocol = read(HERE / "protocol.json")
        require(0 < timestamp(args.deadline_utc) - time.time() <= protocol["maximum_campaign_seconds"], "deadline must be within one hour")
        root, target = args.root.resolve(strict=True), args.target.resolve()
        require(not target.exists(), "fresh absent build target required")
        store = core.Store(out, create=True)
        source = {"source": str(root / "source"), "manifest": store.add(root / "source-candidate-manifest.json"),
                  "archive": store.add(root / "source-candidate.tar.gz")}
        plan = {"schema": "r1.current-scaling32-plan/v1", "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "output": str(out), "target": str(target), "fresh_target_absent_before_prepare": True,
            "protocol": protocol, "schedule": schedule(protocol), "source": source,
            "controls": {name: store.add(path) for name, path in controls().items()},
            "inputs": {case: store.add(root / "source" / protocol["config_directory"] / definition["file"])
                       for case, definition in protocol["cases"].items()},
            "python": identity(sys.executable), "tools": {"cargo": identity(args.cargo), "rustc": identity(args.rustc)},
            "supervisor": store.add(root / "source/tools/run_supervised.py"), "host": host(protocol), "deadline_utc": args.deadline_utc}
        plan["environment"] = {**ENV, "RUSTC": plan["tools"]["rustc"]["path"]}
        validate_plan(store, plan)
        foundation(store, plan, args.foundation_proof)
        core.live_source(source)
        save(out / "plan.json", plan)
        store.add(out / "plan.json")
        save(out / "build.json", {"schema": "r1.current-scaling32-build/v1", "status": "ready",
             "stages": [{"stage": x, "status": "pending"} for x in build_schedule()]})
        save(out / "result.json", {"schema": "r1.current-scaling32-result/v1", "status": "ready",
             "stages": [{"stage": x, "status": "pending"} for x in plan["schedule"]]})
    except BaseException as error:
        save(out / "prepare-failure.json", {"schema": "r1.current-scaling32-prepare-failure/v1", "status": "failed", "error": repr(error), "resumable": False})
        raise


def execute(args, *, building):
    out = args.out.resolve()
    plan, builds, result = read(out / "plan.json"), read(out / "build.json"), read(out / "result.json")
    require(str(out) == plan["output"], "live output cannot be relocated")
    store = core.Store(out)
    state = builds if building else result
    require(state["status"] == "ready" and all(x["status"] == "pending" for x in state["stages"]), "campaign phase cannot resume")
    if not building:
        require(builds["status"] == "completed", "fresh build incomplete")
    destination = out / ("build.json" if building else "result.json")
    tools = [plan["python"], *plan["tools"].values()]
    seen = {}
    entry = None
    try:
        validate_plan(store, plan)
        if building:
            require(not Path(plan["target"]).exists(), "fresh target was populated before build")
            Path(plan["target"]).mkdir(parents=True)
        else:
            require(store.data(join(plan["output"], "build.json")) == (out / "build.json").read_bytes(), "build changed")
        for key in ("CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "R1_SOL_WRITE_PHASE_OUTPUT"):
            require(key not in os.environ, "unexpected environment override: " + key)
        os.environ.update(plan["environment"])
        state["status"] = "running"
        save(destination, state)
        supervisor_spec = importlib.util.spec_from_file_location("current32_bounded_supervisor", plan["supervisor"]["path"])
        supervisor = importlib.util.module_from_spec(supervisor_spec)
        supervisor_spec.loader.exec_module(supervisor)
        for entry in state["stages"]:
            stage = entry["stage"]
            seconds = stage["timeout_seconds"] if building else 300
            require(time.time() + seconds + 20 < timestamp(plan["deadline_utc"]), "insufficient bounded-stage time")
            live(plan, store, None if building else builds)
            directory = out / ("build-stages" if building else "stages") / stage["label"]
            directory.mkdir(parents=True)
            entry.update(status="running", host_before=host(plan["protocol"]), environment=plan["environment"])
            save(destination, state)
            argv = ["--record", str(directory / "supervisor.json"), "--cwd", plan["source"]["source"] if building else str(out), "--disk-path", str(out)]
            for key, field in core.LIMIT_KEYS.items():
                argv += ["--" + key.replace("_", "-"), str(seconds if key == "timeout_seconds" else plan["protocol"]["limits"][field])]
            for pin in pins(plan, store, None if building else builds):
                argv += ["--identity-file", pin["path"]]
            require(time.time() + seconds + 20 < timestamp(plan["deadline_utc"]), "insufficient time after pin verification")
            code = supervisor.main(argv + ["--", *(build_command(plan, stage) if building else command(plan, builds, stage))])
            entry.update(supervisor_exit=code, record=store.add(directory / "supervisor.json"))
            save(destination, state)
            retain_stage(store, directory, tools)
            require(code == 0, "supervised stage failed")
            record = core.record_bytes(store, entry["record"], tools)
            if building and stage["label"] == "toolchain":
                version = store.data(record["outputs"]["stdout"]["path"]).decode()
                require("release: 1.97.0" in version and "host: x86_64-unknown-linux-gnu" in version, "wrong native compiler")
            elif building and stage["label"] == "release-example":
                builds["binary"] = store.add(Path(plan["target"]) / "release/examples/hu_scaling_bench")
                entry["binary_after"] = builds["binary"]
            elif not building:
                actual = sample(store, plan, stage, record)
                if stage["case"] in seen:
                    same_solution(store, seen[stage["case"]], actual)
                seen.setdefault(stage["case"], actual)
                entry["sample"] = actual
            live(plan, store, None if building else builds)
            entry.update(status="passed", host_after=host(plan["protocol"]), source_after_verified=True)
            save(destination, state)
            print(json.dumps({"stage": stage["label"], "status": "passed"}), flush=True)
        state["status"] = "completed"
        if not building:
            state["summary"] = summarize(plan["protocol"], state["stages"])
        save(destination, state)
        if building:
            store.add(destination)
    except BaseException as error:
        state.update(status="failed", error=repr(error))
        state.pop("summary", None)
        for item in state["stages"]:
            if item["status"] == "running":
                item.update(status="failed", error=repr(error))
                directory = out / ("build-stages" if building else "stages") / item["stage"]["label"]
                try:
                    retain_stage(store, directory, tools)
                    if (directory / "supervisor.json").is_file():
                        item["record"] = store.add(directory / "supervisor.json", changed_identity=True)
                except (OSError, ValueError) as retention_error:
                    item["retention_error"] = repr(retention_error)
            elif item["status"] == "pending":
                item.update(status="skipped", reason=repr(error))
        save(destination, state)
        if building:
            result.update(status="failed", error="build failed: " + repr(error))
            for item in result["stages"]:
                item.update(status="skipped", reason="build failed")
            save(out / "result.json", result)
        raise


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase", choices=("prepare", "build", "measure", "check"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--root", type=Path)
    parser.add_argument("--target", type=Path)
    parser.add_argument("--foundation-proof", type=Path)
    parser.add_argument("--cargo", type=Path)
    parser.add_argument("--rustc", type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args()
    if args.phase == "prepare":
        require(all(getattr(args, key) for key in ("root", "target", "foundation_proof", "cargo", "rustc", "deadline_utc")), "prepare arguments missing")
        prepare(args)
    elif args.phase == "check":
        require(args.foundation_proof, "original retained foundation proof required")
        print(json.dumps(check(args.out, args.foundation_proof), indent=2, allow_nan=False))
    else:
        execute(args, building=args.phase == "build")


if __name__ == "__main__":
    main()
