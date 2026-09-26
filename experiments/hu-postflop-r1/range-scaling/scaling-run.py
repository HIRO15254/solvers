"""Bounded F32 layout/thread experiment; builds and cloud lifecycle are external.

pilot creates a new plan, runs compact-1 once per case and freezes iterations.
measure runs the predeclared warmup/three-block sequence. check is read-only,
but requires the original source/binary paths (not an offline retention tool).
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import struct
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
PROTOCOL = HERE / "protocol.json"
SOURCE_PART = Path("experiments/hu-postflop-r1/range-scaling")
SKIP = {".git", "target", "runs", ".cache", "__pycache__"}
PHASES = ("build", "solver_init", "run", "cfv_capture_p0", "cfv_capture_p1")


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


def content(pin):
    return {key: pin[key] for key in ("bytes", "sha256")}


def verify(pin):
    require(identity(pin["path"]) == pin, f"file identity changed: {pin['path']}")


def save(path, value, *, initial=False):
    require(not initial or not path.exists(), f"output already exists: {path}")
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)


def inventory(root):
    result = {}
    for directory, subdirs, files in os.walk(root):
        subdirs[:] = sorted(name for name in subdirs if name not in SKIP)
        for name in subdirs + files:
            require(not (Path(directory) / name).is_symlink(), "source symlinks unsupported")
        for name in sorted(files):
            path = Path(directory) / name
            result[path.relative_to(root).as_posix()] = content(identity(path))
    return result


def host(protocol):
    require(sys.platform == "linux" and platform.machine() == "x86_64", "Linux x86_64 required")
    expected = protocol["logical_cpus"]
    require(os.cpu_count() == expected and len(os.sched_getaffinity(0)) == expected,
            f"exactly {expected} visible and available logical CPUs required")
    cpuinfo = Path("/proc/cpuinfo").read_text()
    fields = ("processor", "model name", "vendor_id", "physical id", "core id", "siblings", "cpu cores", "flags")
    cpu = {field: [line.split(":", 1)[1].strip() for line in cpuinfo.splitlines()
                   if line.split(":", 1)[0].strip() == field] for field in fields}
    return {"boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(),
            "machine": platform.machine(), "kernel": platform.release(), "logical_cpus": os.cpu_count(),
            "affinity": sorted(os.sched_getaffinity(0)), "cpu": cpu}


def containment(protocol):
    relative = next((line[3:] for line in Path("/proc/self/cgroup").read_text().splitlines()
                     if line.startswith("0::")), None)
    require(relative is not None, "cgroup v2 required")
    root = Path("/sys/fs/cgroup")
    group = root / relative.lstrip("/")
    memory, quota, ancestors = [], [], []
    current = group
    while current.is_relative_to(root):
        entry = {"path": str(current)}
        for field in ("memory.max", "memory.swap.max", "cpu.max", "cpuset.cpus.effective"):
            if (current / field).is_file():
                entry[field] = (current / field).read_text().strip()
        if entry.get("memory.max", "max") != "max":
            memory.append(int(entry["memory.max"]))
        if "cpu.max" in entry and not entry["cpu.max"].startswith("max"):
            amount, period = map(int, entry["cpu.max"].split())
            quota.append(amount / period)
        ancestors.append(entry)
        if current == root:
            break
        current = current.parent
    require(memory and min(memory) <= protocol["limits"]["outer_memory_max_bytes"],
            "outer MemoryMax must be finite and <= 12 GiB")
    require(any(item.get("memory.swap.max") == "0" for item in ancestors), "outer swap must be disabled")
    require(not quota or min(quota) >= protocol["logical_cpus"], "CPU quota is below requested logical CPUs")
    return {"ancestors": ancestors, "effective_memory_max_bytes": min(memory),
            "effective_cpu_quota": min(quota) if quota else None,
            "cpu_stat": (group / "cpu.stat").read_text()}


def runlist(protocol, iterations):
    arms = list(protocol["arms"])
    result = []
    for case_index, case in enumerate(protocol["cases"]):
        for block in range(4):
            start = (3 * case_index + block) % len(arms)
            for arm in arms[start:] + arms[:start]:
                result.append({"label": f"{case}-b{block}-{arm}", "case": case, "arm": arm,
                               "block": block, "warmup": block == 0, "iterations": iterations[case]})
    return result


def pilot_stages(protocol):
    return [{"label": "pilot-" + case, "case": case, "arm": "compact-1", "block": -1, "warmup": True,
             "pilot": True, "iterations": item["iteration_cap"]} for case, item in protocol["cases"].items()]


def final_iterations(cap, seconds, target):
    require(math.isfinite(seconds) and seconds > 0, "invalid pilot solve duration")
    return max(1, min(cap, math.floor(cap * min(1, target / seconds))))


def clean_record(record):
    require(record["state"] == "completed" and record["supervisor_exit_code"] == 0
            and record["child_exit_code"] == 0 and record["cleanup_complete"]
            and record["identity_unchanged"] and not record["errors"]
            and not record["forced"] and record["stop_reason"] == "completed",
            "supervised process did not complete cleanly")
    for pin in record["outputs"].values():
        verify(pin)


def pins(plan, live):
    for pin in plan["pins"].values():
        verify(pin)
    for pin in plan["inputs"].values():
        verify(pin)
    require(read(plan["pins"]["protocol"]["path"]) == plan["protocol"], "embedded protocol differs")
    require(inventory(Path(plan["source"])) == plan["source_files"], "source inventory changed")
    if live:
        require(host(plan["protocol"]) == plan["host"], "boot or CPU changed")
        containment(plan["protocol"])


def equal_files(a, b):
    require(content(a) == content(b), f"canonical bytes differ: {a['path']} vs {b['path']}")
    with Path(a["path"]).open("rb") as left, Path(b["path"]).open("rb") as right:
        while True:
            x, y = left.read(1024 * 1024), right.read(1024 * 1024)
            require(x == y, "canonical byte comparison failed")
            if not x:
                return


def phase_memory(record):
    # serde_json object key ordering is not part of this contract.
    events = []
    for line in Path(record["outputs"]["stderr"]["path"]).read_text().splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict) and event.get("event") == "phase":
            events.append(event)
    samples = [json.loads(line) for line in Path(record["outputs"]["samples"]["path"]).read_text().splitlines()]
    result = {}
    for phase in PHASES:
        selected = [event for event in events if event["phase"] == phase]
        require(len(selected) == 2 and [event["status"] for event in selected] == ["started", "completed"],
                f"missing phase bounds: {phase}")
        start, end = [event["unix_ms"] for event in selected]
        require(isinstance(start, int) and end >= start, "invalid phase clock bounds")
        inside = [sample["tree_resident_bytes"] for sample in samples
                  if start <= dt.datetime.fromisoformat(sample["at"]).timestamp() * 1000 <= end]
        result[phase] = {"start_unix_ms": start, "end_unix_ms": end, "sample_count": len(inside),
                         "sampled_tree_peak_bytes": max(inside) if inside else None}
    return result


def command_for(plan, stage):
    arm = plan["protocol"]["arms"][stage["arm"]]
    directory = Path(plan["output"]) / "stages" / stage["label"]
    return [plan["pins"]["binary"]["path"], "--config", plan["inputs"][stage["case"]]["path"],
            "--threads", str(arm["threads"]), "--iterations", str(stage["iterations"]),
            "--layout", arm["layout"], "--out", str(directory / "bench")]


def verify_record(plan, stage, record):
    clean_record(record)
    require(record["argv"] == command_for(plan, stage) and record["cwd"] == plan["output"], "record invocation differs")
    expected_limits = {"timeout_seconds": "sample_timeout_seconds", "memory_limit_bytes": "rss_bytes",
                       "min_free_memory_bytes": "min_free_bytes", "disk_reserve_bytes": "disk_reserve_bytes",
                       "poll_seconds": "poll_seconds", "grace_seconds": "grace_seconds", "kill_wait_seconds": "kill_wait_seconds"}
    require(all(record["limits"][key] == plan["protocol"]["limits"][value] for key, value in expected_limits.items()),
            "supervisor limits differ")
    expected_pins = [*plan["pins"].values(), *plan["inputs"].values(), identity(Path(plan["output"]) / "plan.json")]
    if not stage.get("pilot"):
        expected_pins.append(identity(Path(plan["output"]) / "frozen.json"))
    for field in ("identity_before", "identity_after"):
        require(all(pin in record[field] for pin in expected_pins), "record identity binding differs")


def sample_report(plan, stage, directory, record):
    bench = directory / "bench"
    report = read(bench / "result.json")
    arm = plan["protocol"]["arms"][stage["arm"]]
    require(report["schema"] == "r1.hu-scaling-bench/v1" and report["status"] == "completed"
            and report["storage"] == "f32" and report["layout"] == arm["layout"]
            and report["threads"] == arm["threads"] and report["iterations"] == stage["iterations"],
            "benchmark invocation/result mismatch")
    require(read(record["outputs"]["stdout"]["path"]) == report, "stdout report differs")
    require(content(identity(bench / "config.original.toml")) == content(plan["inputs"][stage["case"]]),
            "original config differs")
    timing = report["timing"]
    require(all(math.isfinite(timing[key]) and timing[key] >= 0 for key in
                ("build_seconds", "solver_init_seconds", "run_seconds")), "invalid phase durations")
    require(timing["run_seconds"] > 0, "zero solve duration")
    canonical = report["canonical"]
    counts = report["counts"]
    require([len(combos) for combos in canonical["global_combos"]] == counts["retained_support_counts"],
            "support count/IDs differ")
    for combos in canonical["global_combos"]:
        require(combos == sorted(set(combos)) and all(0 <= combo < 1326 for combo in combos), "invalid combo IDs")
    require(canonical["union_global_combos"] == sorted(set(sum(canonical["global_combos"], []))),
            "union support IDs differ")
    quality = report["quality"]
    for value_field, bits_field in (("solver_ev", "solver_ev_f64_bits_hex"), ("solver_br", "solver_br_f64_bits_hex")):
        require(len(quality[value_field]) == 2 and len(quality[bits_field]) == 2, "missing per-seat quality")
        require(all(math.isfinite(value) and struct.pack(">d", value).hex() == bits
                    for value, bits in zip(quality[value_field], quality[bits_field])), "quality raw bits differ")
    require(math.isfinite(quality["nash_conv"]) and struct.pack(">d", quality["nash_conv"]).hex() == quality["nash_conv_f64_bits_hex"],
            "NashConv raw bits differ")
    if arm["layout"] == "compact":
        require(counts["root_dims"] == counts["retained_support_counts"], "compact dims omit stored hands")
    else:
        require(counts["root_dims"] == [1326, 1326], "dense control is not full combo layout")
    artifacts = {}
    for field, name in (("strategy_and_cfv", "canonical.bin"), ("supported_state", "state.bin")):
        pin = identity(bench / name)
        require(canonical[field]["file"] == name and canonical[field]["bytes"] == pin["bytes"]
                and re.fullmatch(r"[0-9a-f]{64}", canonical[field]["blake3"]), "canonical metadata differs")
        artifacts[name] = pin
    return {"report": identity(bench / "result.json"), "artifacts": artifacts,
            "timing": timing, "counts": counts, "quality": report["quality"],
            "global_combos": canonical["global_combos"], "union_global_combos": canonical["union_global_combos"],
            "full_process_seconds": record["elapsed_seconds"],
            "full_process_memory": record["measurement"], "phase_sampled_memory": phase_memory(record)}


def same_solution(a, b):
    for name in ("canonical.bin", "state.bin"):
        equal_files(a["artifacts"][name], b["artifacts"][name])
    for name in ("global_combos", "union_global_combos", "quality"):
        require(a[name] == b[name], f"solution metadata differs: {name}")
    for name in ("retained_support_counts", "nodes", "action_nodes", "deals", "normalizer", "root_subtree_has_chance"):
        require(a["counts"][name] == b["counts"][name], f"topology/support metadata differs: {name}")


def execute(plan, state, stage):
    protocol = plan["protocol"]
    limits = protocol["limits"]
    deadline = dt.datetime.fromisoformat(plan["deadline_utc"]).timestamp()
    require(time.time() + limits["sample_timeout_seconds"] + 20 < deadline,
            "deadline: insufficient time for another fully bounded sample")
    pins(plan, live=True)
    directory = Path(plan["output"]) / "stages" / stage["label"]
    directory.mkdir()
    arm = protocol["arms"][stage["arm"]]
    command = command_for(plan, stage)
    entry = {"stage": stage, "status": "running", "host_before": host(protocol), "containment_before": containment(protocol)}
    state["stages"].append(entry)
    save(Path(plan["output"]) / "result.json", state)
    spec = importlib.util.spec_from_file_location("scaling_supervisor", plan["pins"]["supervisor"]["path"])
    supervisor = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(supervisor)
    args = ["--record", str(directory / "supervisor.json"), "--cwd", plan["output"],
            "--timeout-seconds", str(limits["sample_timeout_seconds"]), "--memory-limit-bytes", str(limits["rss_bytes"]),
            "--min-free-memory-bytes", str(limits["min_free_bytes"]), "--disk-reserve-bytes", str(limits["disk_reserve_bytes"]),
            "--disk-path", plan["output"], "--poll-seconds", str(limits["poll_seconds"]),
            "--grace-seconds", str(limits["grace_seconds"]), "--kill-wait-seconds", str(limits["kill_wait_seconds"])]
    for pin in [*plan["pins"].values(), *plan["inputs"].values()]:
        args += ["--identity-file", pin["path"]]
    for filename in ("plan.json", "frozen.json"):
        if (Path(plan["output"]) / filename).exists():
            args += ["--identity-file", str(Path(plan["output"]) / filename)]
    os.environ["RAYON_NUM_THREADS"] = str(arm["threads"])
    os.environ.pop("R1_SOL_WRITE_PHASE_OUTPUT", None)
    code = supervisor.main(args + ["--", *command])
    entry.update(status="checking" if code == 0 else "failed", supervisor_exit=code,
                 record=identity(directory / "supervisor.json"), host_after=host(protocol), containment_after=containment(protocol))
    save(Path(plan["output"]) / "result.json", state)
    record = read(entry["record"]["path"])
    require(code == 0, f"stage failed: {stage['label']}; supervisor state={record['state']}")
    verify_record(plan, stage, record)
    entry["sample"] = sample_report(plan, stage, directory, record)
    pins(plan, live=True)
    require(entry["host_after"] == plan["host"], "host changed during sample")
    if not stage.get("pilot"):
        previous = next((item for item in state["stages"][:-1] if item["stage"]["case"] == stage["case"]
                         and not item["stage"].get("pilot") and item["status"] == "passed"), None)
        if previous:
            same_solution(previous["sample"], entry["sample"])
    entry["status"] = "passed"
    save(Path(plan["output"]) / "result.json", state)
    print(json.dumps({"stage": stage["label"], "status": "passed", "run_seconds": entry["sample"]["timing"]["run_seconds"]}), flush=True)
    return entry["sample"]


def summarize(protocol, entries):
    result = {}
    screen = protocol["screen"]
    for case, definition in protocol["cases"].items():
        groups = {arm: [item["sample"] for item in entries if item["stage"]["case"] == case
                       and item["stage"]["arm"] == arm and not item["stage"]["warmup"]
                       and not item["stage"].get("pilot") and item["status"] == "passed"]
                  for arm in protocol["arms"]}
        require(all(len(items) == 3 for items in groups.values()), "incomplete measured blocks")
        times = {arm: [sample["timing"]["run_seconds"] for sample in samples] for arm, samples in groups.items()}
        medians = {arm: statistics.median(values) for arm, values in times.items()}
        ratios = {"compact2_over_compact1": medians["compact-2"] / medians["compact-1"],
                  "compact4_over_compact1": medians["compact-4"] / medians["compact-1"],
                  "compact4_over_compact2": medians["compact-4"] / medians["compact-2"]}
        wins = {arm: sum(a < b for a, b in zip(times[arm], times["compact-1"])) for arm in ("compact-2", "compact-4")}
        eligible = definition["speed_screen"] and medians["compact-1"] >= screen["minimum_compact1_median_run_seconds"]
        passed = all(value <= screen[name + "_max"] for name, value in ratios.items()) and all(
            count >= screen["strictly_faster_blocks_min"] for count in wins.values())
        compact_arms = sorted((arm for arm in groups if protocol["arms"][arm]["layout"] == "compact"),
                              key=lambda arm: protocol["arms"][arm]["threads"])
        scaling = []
        previous = "compact-1"
        for arm in compact_arms:
            threads = protocol["arms"][arm]["threads"]
            speedup = medians["compact-1"] / medians[arm]
            scaling.append({"threads": threads, "speedup_over_one": speedup, "parallel_efficiency": speedup / threads,
                            "speedup_over_previous": medians[previous] / medians[arm],
                            "slower_than_previous": medians[arm] > medians[previous]})
            previous = arm
        result[case] = {"run_seconds": times, "run_medians": medians, "ratios_of_medians": ratios,
                        "scaling": scaling,
                        "fastest_observed_threads": protocol["arms"][min(compact_arms, key=lambda arm: medians[arm])]["threads"],
                        "first_adjacent_slowdown_threads": next((row["threads"] for row in scaling if row["slower_than_previous"]), None),
                        "paired_faster_blocks": wins, "speed_screen": ("pass" if passed else "miss") if eligible else "not_evaluated",
                        "screen_reason": "eligible" if eligible else "predeclared overhead control or median below one second",
                        "phase_medians": {arm: {name: statistics.median(sample["timing"][name] for sample in samples)
                                                for name in ("build_seconds", "solver_init_seconds", "run_seconds")}
                                          for arm, samples in groups.items()},
                        "full_process_seconds": {arm: [sample["full_process_seconds"] for sample in samples]
                                                 for arm, samples in groups.items()},
                        "memory": {arm: [{"full_process": sample["full_process_memory"], "phases": sample["phase_sampled_memory"],
                                          "storage_payload_bytes": sample["counts"]["f32_storage_payload_bytes"]}
                                         for sample in samples] for arm, samples in groups.items()}}
    return result


def verify_frozen(output, plan, frozen, state):
    require(frozen["plan"] == identity(output / "plan.json"), "frozen plan identity differs")
    pilots = [entry for entry in state["stages"] if entry["stage"].get("pilot")]
    require([entry["stage"] for entry in pilots] == pilot_stages(plan["protocol"]), "pilot stages differ")
    require(frozen["pilot_records"] == [entry["record"] for entry in pilots], "pilot record binding differs")
    derived = {entry["stage"]["case"]: final_iterations(plan["protocol"]["cases"][entry["stage"]["case"]]["iteration_cap"],
               entry["sample"]["timing"]["run_seconds"], plan["protocol"]["pilot"]["solve_target_seconds"]) for entry in pilots}
    require(frozen["iterations"] == derived, "iterations differ from the predeclared pilot rule")
    for pin in frozen["pilot_records"]:
        verify(pin)


def check(output, live=False, *, completing=False):
    plan, frozen, state = read(output / "plan.json"), read(output / "frozen.json"), read(output / "result.json")
    require(state["status"] == ("measuring" if completing else "completed"), "campaign is not eligible for final verification")
    pins(plan, live)
    verify_frozen(output, plan, frozen, state)
    expected = runlist(plan["protocol"], frozen["iterations"])
    actual = [item for item in state["stages"] if not item["stage"].get("pilot")]
    require([item["stage"] for item in state["stages"]] == pilot_stages(plan["protocol"]) + expected,
            "missing, extra or reordered pilot/measured invocation")
    reference = {}
    for entry in state["stages"]:
        require(entry["status"] == "passed", "stage not passed")
        verify(entry["record"])
        record = read(entry["record"]["path"])
        verify_record(plan, entry["stage"], record)
        require(entry["host_before"] == entry["host_after"] == plan["host"], "stage host differs")
        sample = sample_report(plan, entry["stage"], Path(entry["record"]["path"]).parent, record)
        require(sample == entry["sample"], "saved sample report differs")
        if not entry["stage"].get("pilot"):
            case = entry["stage"]["case"]
            if case in reference:
                same_solution(reference[case], sample)
            reference[case] = sample
    summary = summarize(plan["protocol"], actual)
    if not completing:
        require(state["summary"] == summary, "saved summary differs")
    return summary


def pilot(args):
    protocol = read(PROTOCOL)
    source = args.source.resolve(strict=True)
    output = args.out.resolve()
    require(not output.exists() and not output.is_relative_to(source), "output must be new and outside source")
    deadline = dt.datetime.fromisoformat(args.deadline_utc)
    require(deadline.tzinfo and deadline <= dt.datetime.fromisoformat(protocol["deadline_utc_latest"]), "deadline must be timezone-aware and <= predeclared latest")
    manifest = read(args.source_manifest)
    source_files = {item["path"]: content(item) for item in manifest["files"]}
    require(len(source_files) == len(manifest["files"]) and inventory(source) == source_files, "source/manifest exact set mismatch")
    require(re.fullmatch(r"[0-9a-f]{40}", manifest["base_commit"]), "full base revision required")
    build_record = read(args.build_record)
    clean_record(build_record)
    require(all(identity(args.source_manifest) in build_record[key] for key in ("identity_before", "identity_after")),
            "build record does not bind the source manifest")
    require(build_record["cwd"] == str(source) and all(token in build_record["argv"] for token in
            ("build", "--release", "cli", "--example", "hu_scaling_bench")), "expected release example build provenance missing")
    binary = args.binary.resolve(strict=True)
    require(binary.name == "hu_scaling_bench" and binary.parent.name == "examples" and binary.parent.parent.name == "release", "expected Cargo release example path")
    output.mkdir(parents=True)
    (output / "stages").mkdir()
    shutil.copy2(binary, output / "hu_scaling_bench")
    plan = {"schema": "r1.hu-range-scaling-plan/v1", "source": str(source), "source_revision": manifest["base_commit"],
            "source_files": source_files, "output": str(output), "deadline_utc": deadline.isoformat(),
            "protocol": protocol, "host": host(protocol), "initial_containment": containment(protocol),
            "pins": {"binary": identity(output / "hu_scaling_bench"), "compiled_binary": identity(binary),
                     "manifest": identity(args.source_manifest), "build_record": identity(args.build_record),
                     "supervisor": identity(source / "tools/run_supervised.py"), "runner": identity(__file__),
                     "protocol": identity(PROTOCOL), "python": identity(sys.executable)},
            "inputs": {case: identity(source / SOURCE_PART / "configs" / item["file"]) for case, item in protocol["cases"].items()}}
    require(content(plan["pins"]["binary"]) == content(plan["pins"]["compiled_binary"]), "binary copy differs")
    save(output / "plan.json", plan, initial=True)
    state = {"schema": "r1.hu-range-scaling-result/v1", "status": "pilot_running", "stages": [], "skipped": []}
    save(output / "result.json", state, initial=True)
    pending = pilot_stages(protocol)
    run_pending(plan, state, pending)
    iterations = {entry["stage"]["case"]: final_iterations(entry["stage"]["iterations"], entry["sample"]["timing"]["run_seconds"],
                  protocol["pilot"]["solve_target_seconds"]) for entry in state["stages"]}
    save(output / "frozen.json", {"plan": identity(output / "plan.json"), "iterations": iterations,
                                  "pilot_records": [entry["record"] for entry in state["stages"]]}, initial=True)
    state["status"] = "ready_for_measurement"
    save(output / "result.json", state)


def run_pending(plan, state, pending):
    for index, stage in enumerate(pending):
        try:
            execute(plan, state, stage)
        except BaseException as error:
            state["status"] = "failed"
            state["error"] = {"type": type(error).__name__, "message": str(error), "stage": stage["label"]}
            started = bool(state["stages"] and state["stages"][-1]["stage"] == stage)
            if started:
                state["stages"][-1]["status"] = "failed"
            state["skipped"] = [{"stage": item, "reason": "campaign stopped: " + str(error)} for item in pending[index + int(started):]]
            save(Path(plan["output"]) / "result.json", state)
            raise


def measure(output):
    plan, frozen, state = read(output / "plan.json"), read(output / "frozen.json"), read(output / "result.json")
    require(state["status"] == "ready_for_measurement", "measurement requires successful untouched pilot")
    verify_frozen(output, plan, frozen, state)
    require([entry["stage"] for entry in state["stages"]] == pilot_stages(plan["protocol"]), "measurement already started")
    state["status"] = "measuring"
    save(output / "result.json", state)
    run_pending(plan, state, runlist(plan["protocol"], frozen["iterations"]))
    try:
        state["summary"] = check(output, live=True, completing=True)
        state["status"] = "completed"
    except BaseException as error:
        state["status"] = "failed"
        state["error"] = {"type": type(error).__name__, "message": str(error), "stage": "final_check"}
        raise
    finally:
        save(output / "result.json", state)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", choices=("pilot", "measure", "check"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    for name in ("source", "source-manifest", "binary", "build-record"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args(argv)
    try:
        if args.phase == "pilot":
            require(all((args.source, args.source_manifest, args.binary, args.build_record, args.deadline_utc)), "pilot arguments missing")
            pilot(args)
        elif args.phase == "measure":
            measure(args.out.resolve(strict=True))
        else:
            print(json.dumps({"status": "verified", "summary": check(args.out.resolve(strict=True))}, allow_nan=False))
        return 0
    except Exception as error:
        print(f"{type(error).__name__}: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
