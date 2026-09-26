"""Finite two-binary kernel comparison and portable original-byte checks.

Builds/cloud lifecycle are external. Only prepare/measure use original VM paths;
check reads retained bytes and never imports or executes retained source files.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import io
import json
import math
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import statistics
import struct
import sys
import tarfile
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
FULL_STAGES = ("toolchain", "fmt", "clippy", "workspace-tests", "docs", "release-example",
               "release-oracle", "release-river-resolve")
BUILD_STAGES = ("toolchain", "release-example")
LIMIT_KEYS = {"timeout_seconds": "sample_timeout_seconds", "memory_limit_bytes": "rss_bytes",
              "min_free_memory_bytes": "min_free_bytes", "disk_reserve_bytes": "disk_reserve_bytes",
              "poll_seconds": "poll_seconds", "grace_seconds": "grace_seconds",
              "kill_wait_seconds": "kill_wait_seconds"}


def require(value, message):
    if not value:
        raise ValueError(message)


def decode(data):
    def unique(pairs):
        obj = {}
        for key, value in pairs:
            require(key not in obj, "duplicate JSON key: " + key)
            obj[key] = value
        return obj
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def read(path):
    return decode(Path(path).read_bytes())


def digest(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def content(pin):
    return {key: pin[key] for key in ("bytes", "sha256")}


def identity(path):
    path = Path(path).resolve(strict=True)
    before = path.stat()
    value = digest(path.read_bytes())
    after = path.stat()
    require((before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns), "file changed while hashing")
    return {"path": str(path), **value}


def save(path, value):
    path = Path(path)
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)


def join(root, *parts):
    return str(PurePosixPath(root).joinpath(*parts))


def timestamp(value):
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(parsed.tzinfo is not None, "UTC offset required")
    return parsed.timestamp()


class Store:
    """Small content-addressed retention index; archive members stay virtual."""
    def __init__(self, out, create=False):
        self.out = Path(out)
        self.virtual = {}
        if create:
            (self.out / "payload").mkdir()
            self.entries = {}
            self.versions = []
            self.flush()
        else:
            index = read(self.out / "retention.json")
            require(index["schema"] == "r1.showdown-kernel-retention/v1", "retention schema")
            self.entries = index["files"]
            self.versions = index.get("identity_versions", [])
            for path, pin in list(self.entries.items()) + [(pin["path"], pin) for pin in self.versions]:
                require(pin["path"] == path and re.fullmatch(r"[0-9a-f]{64}", pin["sha256"]), "invalid retention entry")
                require(digest(self.blob(pin).read_bytes()) == content(pin), "payload integrity: " + path)

    def blob(self, pin):
        return self.out / "payload" / pin["sha256"]

    def flush(self):
        save(self.out / "retention.json", {"schema": "r1.showdown-kernel-retention/v1", "files": self.entries, "identity_versions": self.versions})

    def add(self, path, *, changed_identity=False):
        pin = identity(path)
        changed = pin["path"] in self.entries and self.entries[pin["path"]] != pin
        require(not changed or changed_identity, "retained original changed")
        data = Path(path).read_bytes()
        require(digest(data) == content(pin), "source changed during retain")
        target = self.blob(pin)
        if target.exists():
            require(target.read_bytes() == data, "content-address collision")
        else:
            target.write_bytes(data)
        if changed:
            if pin not in self.versions:
                self.versions.append(pin)
        else:
            self.entries[pin["path"]] = pin
        self.flush()
        return pin

    def data(self, path):
        if path in self.virtual:
            return self.virtual[path]
        require(path in self.entries, "required original bytes missing: " + path)
        return self.blob(self.entries[path]).read_bytes()

    def pin(self, path):
        return {"path": path, **digest(self.data(path))}

    def verify(self, pin):
        if pin in self.versions:
            require(digest(self.blob(pin).read_bytes()) == content(pin), "version payload differs")
        else:
            require(self.pin(pin["path"]) == pin, "retained pin differs: " + pin["path"])

    def json(self, path):
        return decode(self.data(path))


def source_files(store, arm):
    store.verify(arm["manifest"])
    store.verify(arm["archive"])
    manifest = store.json(arm["manifest"]["path"])
    require(content(arm["archive"]) == {"bytes": manifest["archive_bytes"], "sha256": manifest["archive_sha256"]}, "archive binding")
    expected = {entry["path"]: content(entry) for entry in manifest["files"]}
    require(len(expected) == len(manifest["files"]), "duplicate source path")
    actual, directories = {}, []
    with tarfile.open(fileobj=io.BytesIO(store.data(arm["archive"]["path"])), mode="r:gz") as archive:
        for member in archive:
            name = member.name.rstrip("/")
            safe = PurePosixPath(name)
            require(not safe.is_absolute() and ".." not in safe.parts and "\\" not in name, "unsafe archive member")
            if member.isdir():
                directories.append(name)
                continue
            require(member.isfile() and name not in actual, "unexpected or duplicate archive member")
            data = archive.extractfile(member).read()
            actual[name] = digest(data)
            path = join(arm["source"], name)
            require(path not in store.virtual or store.virtual[path] == data, "source alias collision")
            if path in store.entries:
                require(store.data(path) == data, "archive/retained source disagree")
            store.virtual[path] = data
    require(actual == expected, "source archive exact file set/SHA differs")
    require(sorted(directories) == sorted(manifest.get("directory_entries", [])), "archive directory set differs")
    return manifest


def live_source(arm):
    manifest = read(arm["manifest"]["path"])
    root = Path(arm["source"])
    files = list(root.rglob("*"))
    require(not any(path.is_symlink() for path in files), "source symlink")
    actual = {path.relative_to(root).as_posix(): content(identity(path)) for path in files if path.is_file()}
    require(actual == {row["path"]: content(row) for row in manifest["files"]}, "live source changed")


def schedule(protocol):
    result = []
    for case_index, (case, definition) in enumerate(protocol["cases"].items()):
        for block in range(4):
            order = ("old", "new") if (case_index + block) % 2 == 0 else ("new", "old")
            for arm in order:
                result.append({"case": case, "block": block, "warmup": block == 0, "arm": arm,
                               "iterations": definition["iterations"], "label": f"{case}-b{block}-{arm}"})
    return result


def command(plan, stage):
    return [plan["arms"][stage["arm"]]["binary"]["path"], "--config", plan["inputs"][stage["case"]]["path"],
            "--threads", "1", "--iterations", str(stage["iterations"]), "--layout", "compact",
            "--out", join(plan["output"], "stages", stage["label"], "bench")]


def host(protocol):
    require(sys.platform == "linux" and platform.machine() == "x86_64", "Linux x86_64 required")
    require(os.cpu_count() == 4 and len(os.sched_getaffinity(0)) == 4, "four logical CPUs and full affinity required")
    topology = []
    for cpu in sorted(os.sched_getaffinity(0)):
        root = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology")
        topology.append({"cpu": cpu, "core": root.joinpath("core_id").read_text().strip(),
                         "socket": root.joinpath("physical_package_id").read_text().strip()})
    cg = Path("/sys/fs/cgroup") / Path("/proc/self/cgroup").read_text().strip().split("::", 1)[1].lstrip("/")
    memory, swap = (cg / "memory.max").read_text().strip(), (cg / "memory.swap.max").read_text().strip()
    require(memory != "max" and 0 < int(memory) <= protocol["limits"]["outer_memory_max_bytes"] and swap == "0", "outer memory/swap limit")
    cpu_limits = {}
    for parent in (cg, *cg.parents):
        if parent.is_relative_to("/sys/fs/cgroup") and (parent / "cpu.max").is_file():
            value = (parent / "cpu.max").read_text().strip()
            quota, period = value.split()
            require(quota == "max" or int(quota) / int(period) >= 4, "CPU quota below four CPUs")
            cpu_limits[str(parent)] = value
    return {"boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(), "machine": platform.machine(),
            "logical_cpus": os.cpu_count(), "affinity": sorted(os.sched_getaffinity(0)), "topology": topology,
            "cpu_models": sorted(set(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name"))),
            "cgroup": {"path": str(cg), "memory_max": memory, "swap_max": swap, "cpu_limits": cpu_limits}}


def clean_record(record):
    require(record["schema"] == "solvers.supervised-run/v1" and record["shell"] is False, "supervisor schema/shell")
    require(record["state"] == "completed" and record["stop_reason"] == "completed"
            and record["child_exit_code"] == 0 and record["supervisor_exit_code"] == 0
            and record["cleanup_complete"] is True and record["forced"] is False and not record["errors"], "unclean process completion")
    require(record["identity_unchanged"] is True and record["identity_before"] == record["identity_after"], "identity changed")


def host_record(machine, protocol):
    require(machine["machine"] == "x86_64" and machine["logical_cpus"] == 4 and len(set(machine["affinity"])) == 4, "host CPUs differ")
    require([row["cpu"] for row in machine["topology"]] == machine["affinity"] and machine["cpu_models"] and machine["boot_id"], "host topology absent")
    cg = machine["cgroup"]
    require(cg["memory_max"] != "max" and 0 < int(cg["memory_max"]) <= protocol["limits"]["outer_memory_max_bytes"]
            and cg["swap_max"] == "0", "recorded containment differs")
    require(cg["cpu_limits"], "CPU quota records missing")
    for value in cg["cpu_limits"].values():
        quota, period = value.split()
        require(quota == "max" or int(quota) / int(period) >= 4, "recorded CPU quota too low")


def record_bytes(store, pin, identity_only=(), success=True):
    store.verify(pin)
    record = store.json(pin["path"])
    require(record["schema"] == "solvers.supervised-run/v1" and record["shell"] is False, "supervisor schema/shell")
    if success:
        clean_record(record)
        require(set(record["outputs"]) == {"stdout", "stderr", "samples"}
                and all(set(row) == {"path", "bytes", "sha256"} for row in record["outputs"].values()), "completed record lacks raw output SHA pins")
    allowed = {row["path"]: row for row in identity_only}
    for key in ("identity_before", "identity_after"):
        for row in record[key]:
            if row["path"] in allowed:
                if success:
                    require(row == allowed[row["path"]], "tool identity differs")
            else:
                try:
                    store.verify(row)
                except (ValueError, FileNotFoundError):
                    if success:
                        raise
    for row in record["outputs"].values():
        if "sha256" in row:
            store.verify(row)
    samples = []
    sample_pin = record["outputs"]["samples"]
    if "sha256" in sample_pin:
        samples = [decode(line) for line in store.data(sample_pin["path"]).splitlines() if line.strip()]
        measurement = record["measurement"]
        require(len(samples) == measurement["sample_count"], "raw sample count differs")
        require(max((row["tree_resident_bytes"] for row in samples), default=0) == measurement["sampled_peak_tree_resident_bytes"], "raw peak RSS differs")
        if samples:
            require(record["last_sample"] == samples[-1], "raw last sample differs")
            for key in ("root_os_peak_resident_bytes", "root_os_peak_source", "job_os_peak_commit_bytes"):
                value = next((row[key] for row in reversed(samples) if row[key] is not None), None)
                require(value == measurement[key], "native peak metric differs")
            if success:
                require(not samples[-1]["pids"], "nonempty final containment")
    if success:
        require(samples, "completed process has no samples")
    return record


def validate_build(store, arm, role, boot):
    manifest = source_files(store, arm)
    if role == "old":
        require(manifest["base_commit"] == "d8135a12c22c9d5e70216380298bda480e63ad05", "wrong old source revision")
        require(manifest["dirty"] is False and not manifest.get("changed_paths_against_base"), "old source is not a clean baseline")
    store.verify(arm["validation"])
    state = store.json(arm["validation"]["path"])
    expected_mode = "release-build-only" if role == "old" else "full-validation"
    require(state["schema"] == "r1.range-scaling-validation/v1" and state["status"] == "completed"
            and state["mode"] == expected_mode and state["boot_id"] == boot, "build/validation mode, status or boot differs")
    require(state["source_root"] == arm["source"] and state["source_manifest"] == arm["manifest"]
            and state["source_archive"] == arm["archive"] and state["binary"] == arm["binary"], "build source/binary binding")
    require([row["label"] for row in state["stages"]] == list(BUILD_STAGES if role == "old" else FULL_STAGES), "validation stage set")
    for key, expected in {"RUSTUP_TOOLCHAIN": "1.97.0", "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "2",
                          "CARGO_INCREMENTAL": "0", "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0"}.items():
        require(state["environment"][key] == expected, "validation environment differs: " + key)
    require(state["environment"]["CARGO_BUILD_JOBS"] in ("1", "2"), "unbounded build jobs")
    store.verify(state["runner"])
    require(content(state["runner"]) == content(store.pin(join(arm["source"], "experiments/hu-postflop-r1/range-scaling/validate.py"))), "validation runner/source differs")
    tools = list(state["tools"].values()) + [arm["python"]]
    test_counts = {}
    for stage in state["stages"]:
        require(stage["status"] == "passed" and stage["supervisor_exit"] == 0, "validation failed stage")
        record = record_bytes(store, stage["record"], tools)
        executable = arm["python"] if stage["label"] == "docs" else state["tools"]["rustc" if stage["label"] == "toolchain" else "cargo"]
        require(record["argv"] == stage["argv"] and record["resolved_argv"] == [executable["path"], *stage["argv"][1:]]
                and record["cwd"] == arm["source"], "validation invocation/executable differs")
        for pin in (arm["manifest"], arm["archive"], state["runner"], state["tools"]["cargo"], state["tools"]["rustc"],
                    store.pin(join(arm["source"], "tools/run_supervised.py")), arm["python"]):
            require(pin in record["identity_before"], "build record lacks source/tool pin")
        argv = stage["argv"]
        label = stage["label"]
        tails = {"toolchain": ["-Vv"], "fmt": ["fmt", "--all", "--check"], "docs": ["-B", "tools/check_docs.py"],
                 "clippy": ["clippy", "--workspace", "--all-targets", "--target-dir", state["target"], "--", "-D", "warnings"],
                 "workspace-tests": ["test", "--workspace", "--no-fail-fast", "--target-dir", state["target"]],
                 "release-example": ["build", "--release", "-p", "cli", "--example", "hu_scaling_bench", "--target-dir", state["target"]],
                 "release-oracle": ["test", "--release", "-p", "holdem", "--test", "oracle_diff", "--target-dir", state["target"], "--", "--include-ignored"],
                 "release-river-resolve": ["test", "--release", "-p", "cli", "--lib", "--target-dir", state["target"], "sol::tests::river_resolve_accuracy", "--", "--exact", "--ignored"]}
        require(argv[1:] == tails[label], "validation command scope differs")
        require(record["limits"]["timeout_seconds"] == stage["timeout_seconds"], "validation timeout differs")
        text = store.data(record["outputs"]["stdout"]["path"]).decode("utf-8")
        if label == "toolchain":
            require("release: 1.97.0" in text and "host: x86_64-unknown-linux-gnu" in text, "toolchain output differs")
        if label in ("workspace-tests", "release-oracle", "release-river-resolve"):
            rows = [tuple(map(int, row)) for row in re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", text)]
            require(rows and sum(row[0] for row in rows) > 0 and not any(row[1] for row in rows), "test summary missing or failed")
            test_counts[label] = {"summary_count": len(rows), "passed": sum(row[0] for row in rows), "ignored": sum(row[2] for row in rows)}
            if label == "release-river-resolve":
                require(sum(row[0] for row in rows) == 1 and not any(row[2] for row in rows)
                        and "test sol::tests::river_resolve_accuracy ... ok" in text, "ignored river accuracy did not run")
        if label == "docs":
            require(re.search(r"Documentation check passed: \d+ Markdown files\.", text), "documentation check output absent")
    store.verify(arm["binary"])
    require(arm["binary"]["path"] == join(state["target"], "release/examples/hu_scaling_bench"), "build output path differs")
    return {"mode": expected_mode, "stages": len(state["stages"]), "tests": test_counts, "full_workspace_validated": role == "new"}


def sample(store, plan, stage, record):
    directory = join(plan["output"], "stages", stage["label"], "bench")
    report = store.json(join(directory, "result.json"))
    require(report["schema"] == "r1.hu-scaling-bench/v1" and report["status"] == "completed"
            and report["storage"] == "f32" and report["layout"] == "compact" and report["threads"] == 1
            and report["iterations"] == stage["iterations"] and report["config"] == plan["inputs"][stage["case"]]["path"], "benchmark invocation differs")
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
    for values, bits in ((quality["solver_ev"], quality["solver_ev_f64_bits_hex"]),
                         (quality["solver_br"], quality["solver_br_f64_bits_hex"])):
        require(len(values) == len(bits) == 2 and all(math.isfinite(v) and struct.pack(">d", v).hex() == b for v, b in zip(values, bits)), "quality bits differ")
    require(struct.pack(">d", quality["nash_conv"]).hex() == quality["nash_conv_f64_bits_hex"] and math.isfinite(quality["nash_conv"]), "NashConv bits differ")
    combos = canonical["global_combos"]
    require(len(combos) == 2 and all(row == sorted(set(row)) and all(0 <= v < 1326 for v in row) for row in combos), "invalid support IDs")
    require(counts["root_dims"] == counts["retained_support_counts"] == [len(row) for row in combos]
            and canonical["union_global_combos"] == sorted(set(sum(combos, []))), "support counts differ")
    artifacts = {}
    for field, name in (("strategy_and_cfv", "canonical.bin"), ("supported_state", "state.bin")):
        pin = store.pin(join(directory, name))
        require(canonical[field]["file"] == name and canonical[field]["bytes"] == pin["bytes"]
                and re.fullmatch(r"[0-9a-f]{64}", canonical[field]["blake3"]), "artifact metadata differs")
        artifacts[name] = pin
    return {"run_seconds": report["timing"]["run_seconds"], "timing": report["timing"], "counts": counts,
            "quality": quality, "global_combos": combos, "union_global_combos": canonical["union_global_combos"],
            "artifacts": artifacts, "normalized_config": store.pin(join(directory, "config.normalized.toml")),
            "algorithm": report["algorithm"], "rake": report["rake"], "utility": report["utility"],
            "full_process_seconds": record["elapsed_seconds"], "full_process_memory": record["measurement"]}


def same_solution(store, first, second):
    for key in ("counts", "quality", "global_combos", "union_global_combos", "algorithm", "rake", "utility"):
        require(first[key] == second[key], "solution metadata differs: " + key)
    for name in ("canonical.bin", "state.bin"):
        a, b = first["artifacts"][name], second["artifacts"][name]
        require(content(a) == content(b) and store.data(a["path"]) == store.data(b["path"]), "original solution bytes differ: " + name)
    a, b = first["normalized_config"], second["normalized_config"]
    require(content(a) == content(b) and store.data(a["path"]) == store.data(b["path"]), "normalized config differs")


def summarize(protocol, entries):
    cases, ratios = {}, []
    for case in protocol["cases"]:
        groups = {arm: [row["sample"] for row in entries if row["stage"]["case"] == case and row["stage"]["arm"] == arm and not row["stage"]["warmup"]] for arm in ("old", "new")}
        require(all(len(rows) == 3 for rows in groups.values()), "incomplete measured case")
        times = {arm: [row["run_seconds"] for row in rows] for arm, rows in groups.items()}
        medians = {arm: statistics.median(values) for arm, values in times.items()}
        ratio = medians["new"] / medians["old"]
        ratios.append(ratio)
        memory = {arm: {key: [row["full_process_memory"][key] for row in rows] for key in ("root_os_peak_resident_bytes", "sampled_peak_tree_resident_bytes")} for arm, rows in groups.items()}
        cases[case] = {"run_seconds": times, "medians": medians, "new_over_old": ratio,
                       "paired_new_faster": sum(a < b for a, b in zip(times["new"], times["old"])),
                       "memory_bytes": memory, "memory_medians_bytes": {arm: {key: statistics.median(values) for key, values in fields.items()} for arm, fields in memory.items()}}
    geometric_mean = math.exp(sum(math.log(value) for value in ratios) / len(ratios))
    guard = protocol["guard"]
    return {"cases": cases, "geometric_mean_new_over_old": geometric_mean,
            "descriptive_guard_pass": geometric_mean <= guard["geometric_mean_new_over_old_max"] and all(ratio <= guard["every_case_new_over_old_max"] for ratio in ratios),
            "r1_certification": False}


def check(out):
    out = Path(out)
    store = Store(out)
    plan, state = read(out / "plan.json"), read(out / "result.json")
    require(plan["schema"] == "r1.showdown-kernel-plan/v1" and state["schema"] == "r1.showdown-kernel-result/v1", "campaign schema")
    plan_pin = store.pin(join(plan["output"], "plan.json"))
    require(content(plan_pin) == content(identity(out / "plan.json")), "local plan differs")
    protocol = plan["protocol"]
    require(protocol == decode(store.data(plan["controls"]["protocol.json"]["path"])), "protocol bytes differ")
    require(protocol == read(HERE / "protocol.json"), "unexpected frozen protocol")
    require(plan["schedule"] == schedule(protocol) and len(plan["schedule"]) == 32, "schedule differs")
    host_record(plan["host"], protocol)
    require(plan["environment"] == {"RAYON_NUM_THREADS": "1"}, "runner environment differs")
    require(timestamp(plan["deadline_utc"]) > timestamp(plan["created_at"]), "deadline invalid")
    builds = {role: validate_build(store, arm, role, plan["host"]["boot_id"]) for role, arm in plan["arms"].items()}
    require(set(builds) == {"old", "new"}, "arm set differs")
    for name in ("crates/cli/examples/hu_scaling_bench.rs", "tools/run_supervised.py"):
        require(store.data(join(plan["arms"]["old"]["source"], name)) == store.data(join(plan["arms"]["new"]["source"], name)), "old/new measurement definition differs: " + name)
    for pin in plan["controls"].values():
        store.verify(pin)
    for name in ("run.py", "protocol.json", "verify.py"):
        require(content(plan["controls"][name]) == content(identity(HERE / name)), "trusted local campaign code differs: " + name)
    store.verify(plan["supervisor"])
    require(plan["supervisor"] == store.pin(join(plan["arms"]["new"]["source"], "tools/run_supervised.py")), "supervisor/source binding")
    for case, pin in plan["inputs"].items():
        store.verify(pin)
        require(pin["path"] == join(plan["arms"]["new"]["source"], protocol["config_directory"], protocol["cases"][case]["file"]), "input path differs from source fixture")
        for arm in plan["arms"].values():
            other = store.pin(join(arm["source"], protocol["config_directory"], protocol["cases"][case]["file"]))
            require(content(pin) == content(other), "old/new fixture differs")
    require(set(plan["inputs"]) == set(protocol["cases"]), "case input set differs")
    require(len(state["stages"]) == 32, "result schedule length")
    passed, first_by_case, failed, failed_records = [], {}, False, []
    previous_end = timestamp(plan["created_at"])
    required_pins = [plan_pin, *plan["controls"].values(), plan["supervisor"], *plan["inputs"].values()]
    required_pins += [pin for arm in plan["arms"].values() for pin in (arm["manifest"], arm["archive"], arm["validation"], arm["binary"])]
    for expected, entry in zip(plan["schedule"], state["stages"]):
        require(entry["stage"] == expected, "result stage order differs")
        status = entry["status"]
        if status in ("skipped", "pending"):
            require(status != "skipped" or entry.get("reason"), "missing skip reason")
            failed = True
            continue
        require(not failed, "executed sample after failure/skip")
        if status == "failed":
            failed = True
            require(entry.get("error"), "failed stage lacks reason")
        else:
            require(status == "passed", "nonterminal stage")
            require("record" in entry, "passed sample missing record")
        if "record" in entry:
            record = record_bytes(store, entry["record"], [plan["python"]], success=status == "passed")
            require(record["argv"] == command(plan, expected) and record["cwd"] == plan["output"], "sample invocation differs")
            if status == "passed":
                require(record["resolved_argv"] == command(plan, expected), "sample executable differs")
                require(record["runtime"]["logical_cpus"] == 4 and record["runtime"]["machine"] == "x86_64", "sample runtime differs")
            require(entry.get("supervisor_exit", record["supervisor_exit_code"]) == record["supervisor_exit_code"], "saved supervisor exit differs")
            require(all(record["limits"][key] == protocol["limits"][value] for key, value in LIMIT_KEYS.items()), "sample bounds differ")
            if status == "passed":
                require(all(pin in record["identity_before"] for pin in required_pins), "sample pin binding missing")
            else:
                unavailable = []
                for pin in record["identity_before"] + record["identity_after"]:
                    if pin["path"] == plan["python"]["path"]:
                        continue
                    try:
                        store.verify(pin)
                    except (ValueError, FileNotFoundError):
                        unavailable.append(pin)
                failed_records.append({"stage": expected["label"], "state": record["state"], "stop_reason": record["stop_reason"],
                                       "supervisor_exit_code": record["supervisor_exit_code"], "identity_unchanged": record["identity_unchanged"],
                                       "identity_bytes_unavailable": unavailable, "cleanup_complete": record["cleanup_complete"]})
        if status != "passed":
            continue
        require(entry["host_before"] == entry["host_after"] == plan["host"] and entry["source_after_verified"] is True, "sample host/source changed")
        started, ended = timestamp(record["created_at"]), timestamp(record["ended_at"])
        require(previous_end <= started <= ended, "sample chronology overlaps or runs backward")
        previous_end = ended
        require(timestamp(record["ended_at"]) <= timestamp(plan["deadline_utc"]), "sample exceeded deadline")
        actual = sample(store, plan, expected, record)
        require(actual == entry["sample"], "saved sample summary differs")
        if expected["case"] in first_by_case:
            same_solution(store, first_by_case[expected["case"]], actual)
        else:
            first_by_case[expected["case"]] = actual
        passed.append(entry)
    if state["status"] == "completed":
        require(len(passed) == 32 and not failed, "incomplete completed campaign")
        summary = summarize(protocol, passed)
        require(summary == state["summary"], "timing summary differs")
    else:
        require(state["status"] in ("ready", "failed"), "nonterminal campaign")
        require("summary" not in state, "incomplete campaign claims summary")
        require((state["status"] == "ready" and all(row["status"] == "pending" for row in state["stages"]))
                or (state["status"] == "failed" and state.get("error") and all(row["status"] != "pending" for row in state["stages"])), "inconsistent status")
        summary = None
    return {"schema": "r1.showdown-kernel-verification/v1", "status": state["status"], "payload_integrity": "verified",
            "builds": builds, "samples_passed": len(passed), "failed_records": failed_records, "summary": summary,
            "source_after_scope": "Recorded live rehash, not independent post-run filesystem snapshot",
            "tool_retention": "Compiler/Python identities only; measured binaries and raw outputs retained"}


def retain_record(store, path, tools, *, tolerate_identity_failure=False):
    pin = store.add(path)
    record = read(path)
    # Preserve raw outputs before touching identities that may have changed or
    # vanished. Those identity failures are the reason for retaining a failed run.
    for row in record["outputs"].values():
        if "sha256" in row:
            require(store.add(row["path"]) == row, "record output changed")
    allowed = {row["path"]: row for row in tools}
    for key in ("identity_before", "identity_after"):
        for row in record[key]:
            if row["path"] in allowed:
                if not tolerate_identity_failure:
                    require(row == allowed[row["path"]], "tool changed")
            else:
                try:
                    store.verify(row)
                except (ValueError, FileNotFoundError):
                    try:
                        actual = store.add(row["path"], changed_identity=tolerate_identity_failure)
                        require(actual == row, "record pin changed")
                    except (ValueError, FileNotFoundError):
                        if not tolerate_identity_failure:
                            raise
    return pin


def prepare(args):
    protocol = read(HERE / "protocol.json")
    require(0 < timestamp(args.deadline_utc) - time.time() < 6 * 3600, "deadline must be within six hours")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    store = Store(out, create=True)
    machine = host(protocol)
    python = identity(sys.executable)
    arms = {}
    for role, root in (("old", args.old_root), ("new", args.new_root)):
        root = root.resolve()
        validation_path = root / "validation/result.json"
        state = read(validation_path)
        arm = {"source": str(root / "source"), "manifest": store.add(root / "source-candidate-manifest.json"),
               "archive": store.add(root / "source-candidate.tar.gz"), "validation": store.add(validation_path),
               "binary": store.add(state["binary"]["path"]), "python": python}
        store.add(state["runner"]["path"])
        for stage in state["stages"]:
            retain_record(store, stage["record"]["path"], [*state["tools"].values(), python])
        live_source(arm)
        validate_build(store, arm, role, machine["boot_id"])
        arms[role] = arm
    require(arms["old"]["source"] != arms["new"]["source"] and arms["old"]["binary"]["path"] != arms["new"]["binary"]["path"], "separate sources/targets required")
    controls = {name: store.add(HERE / name) for name in ("run.py", "protocol.json", "verify.py")}
    inputs = {case: store.add(Path(arms["new"]["source"]) / protocol["config_directory"] / definition["file"]) for case, definition in protocol["cases"].items()}
    plan = {"schema": "r1.showdown-kernel-plan/v1", "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "output": str(out), "protocol": protocol, "schedule": schedule(protocol), "arms": arms, "inputs": inputs,
            "controls": controls, "python": python, "supervisor": store.add(Path(arms["new"]["source"]) / "tools/run_supervised.py"),
            "host": machine, "deadline_utc": args.deadline_utc, "environment": {"RAYON_NUM_THREADS": "1"}}
    save(out / "plan.json", plan)
    store.add(out / "plan.json")
    save(out / "result.json", {"schema": "r1.showdown-kernel-result/v1", "status": "ready", "stages": [{"stage": stage, "status": "pending"} for stage in plan["schedule"]]})
    check(out)
    print(json.dumps({"status": "ready", "output": str(out)}))


def live_pins(plan):
    pins = [*plan["controls"].values(), plan["supervisor"], plan["python"], *plan["inputs"].values()]
    pins += [pin for arm in plan["arms"].values() for pin in (arm["manifest"], arm["archive"], arm["validation"], arm["binary"])]
    plan_path = str(Path(plan["output"]) / "plan.json")
    pins.append(read(Path(plan["output"]) / "retention.json")["files"][plan_path])
    for pin in pins:
        require(identity(pin["path"]) == pin, "live identity changed")
    for arm in plan["arms"].values():
        live_source(arm)
    require(host(plan["protocol"]) == plan["host"], "live host changed")
    return pins


def measure(args):
    out = args.out.resolve()
    plan, state = read(out / "plan.json"), read(out / "result.json")
    require(check(out)["status"] == "ready", "measurement can start once only")
    require(str(out) == plan["output"], "live output relocated")
    store = Store(out)
    os.environ["RAYON_NUM_THREADS"] = "1"
    os.environ.pop("R1_SOL_WRITE_PHASE_OUTPUT", None)
    state["status"] = "running"
    save(out / "result.json", state)
    previous = {}
    try:
        live_pins(plan)
        spec = importlib.util.spec_from_file_location("bounded_kernel_supervisor", plan["supervisor"]["path"])
        supervisor = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(supervisor)
        for entry in state["stages"]:
            stage = entry["stage"]
            limits = plan["protocol"]["limits"]
            require(time.time() + limits["sample_timeout_seconds"] + 20 < timestamp(plan["deadline_utc"]), "insufficient deadline for fully bounded sample")
            pins = live_pins(plan)
            directory = out / "stages" / stage["label"]
            directory.mkdir(parents=True)
            entry.update(status="running", host_before=host(plan["protocol"]))
            save(out / "result.json", state)
            argv = ["--record", str(directory / "supervisor.json"), "--cwd", str(out), "--disk-path", str(out)]
            for field, key in LIMIT_KEYS.items():
                argv += ["--" + field.replace("_", "-"), str(limits[key])]
            for pin in pins:
                argv += ["--identity-file", pin["path"]]
            code = supervisor.main(argv + ["--", *command(plan, stage)])
            entry["record"] = store.add(directory / "supervisor.json")
            save(out / "result.json", state)
            retain_record(store, directory / "supervisor.json", [plan["python"]], tolerate_identity_failure=code != 0)
            entry["supervisor_exit"] = code
            if (directory / "bench").exists():
                for path in (directory / "bench").rglob("*"):
                    if path.is_file():
                        store.add(path)
            require(code == 0, "supervisor failed: " + stage["label"])
            record = record_bytes(store, entry["record"], [plan["python"]])
            actual = sample(store, plan, stage, record)
            if stage["case"] in previous:
                same_solution(store, previous[stage["case"]], actual)
            else:
                previous[stage["case"]] = actual
            live_pins(plan)
            entry.update(status="passed", sample=actual, host_after=host(plan["protocol"]), source_after_verified=True)
            save(out / "result.json", state)
            print(json.dumps({"stage": stage["label"], "run_seconds": actual["run_seconds"], "status": "passed"}), flush=True)
        state.update(status="completed", summary=summarize(plan["protocol"], state["stages"]))
        save(out / "result.json", state)
        check(out)
    except BaseException as error:
        state.update(status="failed", error=repr(error))
        state.pop("summary", None)
        for entry in state["stages"]:
            if entry["status"] == "running":
                entry.update(status="failed", error=repr(error))
                directory = out / "stages" / entry["stage"]["label"]
                try:
                    if (directory / "supervisor.json").is_file():
                        entry["record"] = store.add(directory / "supervisor.json")
                        retain_record(store, directory / "supervisor.json", [plan["python"]], tolerate_identity_failure=True)
                    for path in directory.rglob("*"):
                        if path.is_file():
                            store.add(path, changed_identity=True)
                except (OSError, ValueError) as retention_error:
                    entry["retention_error"] = repr(retention_error)
            elif entry["status"] == "pending":
                entry.update(status="skipped", reason=repr(error))
        save(out / "result.json", state)
        raise


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase", choices=("prepare", "measure", "check"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--old-root", type=Path)
    parser.add_argument("--new-root", type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args()
    if args.phase == "prepare":
        require(args.old_root is not None and args.new_root is not None and args.deadline_utc, "prepare requires both roots/deadline")
        prepare(args)
    elif args.phase == "measure":
        measure(args)
    else:
        print(json.dumps(check(args.out), indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
