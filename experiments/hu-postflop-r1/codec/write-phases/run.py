#!/usr/bin/env python3
"""Freeze/run the finite writer phase experiment. Linux cgroup v2 only; no builds/cloud."""
from __future__ import annotations
import argparse
import contextlib
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
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
BLOCK = 1024 * 1024
ENV = "R1_SOL_WRITE_PHASE_OUTPUT"
COPY_KEYS = ("baseline-plain", "baseline-instrumented", "candidate-plain", "candidate-instrumented")
BUILD_LIMITS = {"campaign_seconds": 3000, "build_seconds": 600, "copy_seconds": 60, "tool_seconds": 15,
                "memory_bytes": 4 * 1024**3, "min_free_memory_bytes": 2 * 1024**3,
                "disk_reserve_bytes": 4 * 1024**3, "jobs": 2}
TOOL_FILES = {"runner": "run.py", "validator": "validate.py", "applier": "apply.py", "runtime": "runtime.rs.inc", "source_pins": "source-pins.json"}
ASSURANCE = "sampled process inventory plus operator dedicated-window declaration"


def require(ok, why):
    if not ok:
        raise ValueError(why)


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def instant(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "timestamp needs timezone")
    return result


def read(path):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def invalid(value):
        raise ValueError("nonfinite JSON " + value)
    with Path(path).open("rb") as stream:
        raw = stream.read(32 * BLOCK + 1)
    require(len(raw) <= 32 * BLOCK, "oversized JSON")
    result = json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid)
    json.dumps(result, allow_nan=False)
    return result


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def identity(path):
    path = Path(path).resolve(strict=True)
    before = path.stat()
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(BLOCK), b""):
            hasher.update(chunk)
    after = path.stat()
    require((before.st_size, before.st_mtime_ns, before.st_ctime_ns, before.st_ino) ==
            (after.st_size, after.st_mtime_ns, after.st_ctime_ns, after.st_ino), "file changed while hashing")
    return {"path": str(path), "bytes": after.st_size, "sha256": hasher.hexdigest()}


def verify(ref):
    require(isinstance(ref, dict) and set(ref) == {"path", "bytes", "sha256"}
            and isinstance(ref["path"], str) and Path(ref["path"]).is_absolute()
            and type(ref["bytes"]) is int and ref["bytes"] >= 0
            and isinstance(ref["sha256"], str) and re.fullmatch("[0-9a-f]{64}", ref["sha256"]), "invalid FileRef")
    require(identity(ref["path"]) == ref, "changed/missing identity: " + ref["path"])


def same_bytes(a, b):
    with Path(a).open("rb") as left, Path(b).open("rb") as right:
        while True:
            x, y = left.read(BLOCK), right.read(BLOCK)
            require(x == y, "artifact bytes differ")
            if not x:
                return


def write(path, value, *, exclusive=True):
    path = Path(path)
    fd, temporary = tempfile.mkstemp(prefix=".writer-", dir=path.parent)
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


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def host():
    require(sys.platform == "linux", "actual host checks require Linux")
    cpu = Path("/proc/cpuinfo").read_text()
    field = lambda key: next((line.split(":", 1)[1].strip() for line in cpu.splitlines() if line.startswith(key + "\t")), None)
    result = {"system": platform.system(), "machine": platform.machine(), "kernel": platform.release(),
              "logical_cpus": os.cpu_count(), "cpu": field("model name"), "flags": field("flags"),
              "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip()}
    require(result["cpu"] and result["flags"] and result["boot_id"], "CPU/features/boot unsupported")
    return result


def process_inventory():
    result = {}
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            stat = (path / "stat").read_text()
            close = stat.rfind(")")
            fields = stat[close + 2:].split()
            with (path / "cmdline").open("rb") as stream:
                command = stream.read(65537)
            require(len(command) <= 65536, "oversized process command")
            result[int(path.name)] = {"pid": int(path.name), "parent": int(fields[1]),
                                      "start_ticks": int(fields[19]), "comm": stat[stat.index("(") + 1:close],
                                      "cmdline_sha256": hashlib.sha256(command).hexdigest()}
        except FileNotFoundError:
            continue  # A process exited while enumerating. A persistent change still fails below.
    require(os.getpid() in result, "cannot observe own process")
    return result


def process_scan(baseline=None, cgroup=None, *, idle=False):
    processes = process_inventory()
    own = {os.getpid()}
    parent = processes[os.getpid()]["parent"]
    while parent in processes and parent not in own:
        own.add(parent)
        parent = processes[parent]["parent"]
    descendants = {os.getpid()}
    while True:
        added = {pid for pid, row in processes.items() if row["parent"] in descendants}
        if added <= descendants:
            break
        descendants |= added
    own |= descendants
    require(not idle or descendants == {os.getpid()}, "workload descendants remain outside supervised stage")
    foreign = [{k: v for k, v in row.items() if k != "parent"} for pid, row in sorted(processes.items()) if pid not in own]
    banned = {"scp", "sftp-server", "rsync", "cargo", "rustc", "rustfmt", "solvers", "sol_codec_bench", "tar", "gzip", "pigz", "zstd"}
    require(not any(row["comm"] in banned for row in foreign), "concurrent transfer/build/solver/collection process")
    require(baseline is None or foreign == baseline, "foreign process inventory changed during dedicated window")
    members = set(map(int, (cgroup / "cgroup.procs").read_text().split())) if cgroup else {os.getpid()}
    require(os.getpid() in members and members <= own, "unrelated process in experiment cgroup")
    if cgroup:
        expected_group = "/" + cgroup.relative_to(Path("/sys/fs/cgroup")).as_posix()
        for pid in descendants - {os.getpid()}:
            try:
                child_groups = (Path("/proc") / str(pid) / "cgroup").read_text().splitlines()
            except FileNotFoundError:
                continue  # Already exited. cgroup.procs omits zombies; do not require their membership there.
            require("0::" + expected_group in child_groups, "workload descendant escaped outer cgroup")
    return {"status": "clear", "baseline_foreign": foreign, "foreign_same": True,
            "no_unrelated_cgroup_members": True, "assurance": ASSURANCE}


def duration(value):
    require(isinstance(value, str) and value not in ("infinity", "0"), "unbounded systemd duration")
    units = {"us": 1e-6, "ms": 1e-3, "s": 1, "min": 60, "h": 3600, "d": 86400}
    matches = list(re.finditer(r"([0-9]+(?:\.[0-9]+)?)(us|ms|min|s|h|d)", value.replace(" ", "")))
    require(matches and "".join(m.group(0) for m in matches) == value.replace(" ", ""), "unsupported systemd duration")
    return sum(float(m.group(1)) * units[m.group(2)] for m in matches)


def environment(window, output_parent, *, full=True, previous=None):
    require(sys.platform == "linux", "cgroup containment unsupported on this host")
    lines = Path("/proc/self/cgroup").read_text().splitlines()
    groups = [line[3:] for line in lines if line.startswith("0::")]
    require(len(groups) == 1 and groups[0].startswith("/") and ".." not in Path(groups[0]).parts, "cgroup v2 required")
    group = Path("/sys/fs/cgroup") / groups[0].lstrip("/")
    limits = {name: (group / name).read_text().strip() for name in
              ("memory.max", "memory.swap.max", "pids.max", "cpu.max", "cpuset.cpus.effective")}
    require(limits["memory.max"].isdigit() and 0 < int(limits["memory.max"]) <= 4 * 1024**3, "finite outer memory.max <=4GiB required")
    require(limits["memory.swap.max"] == "0" and limits["pids.max"].isdigit()
            and 0 < int(limits["pids.max"]) <= 512, "swap/pids containment unsupported")
    quota, period = limits["cpu.max"].split()
    require(quota.isdigit() and period.isdigit() and 0 < int(quota) <= int(period) * os.cpu_count()
            and int(period) > 0 and limits["cpuset.cpus.effective"], "finite CPU quota required")
    if full:
        properties = ("ControlGroup", "ActiveState", "KillMode", "SendSIGKILL", "RuntimeMaxUSec", "TimeoutStopUSec")
        raw = subprocess.check_output([window["systemctl"]["path"], "show", window["systemd_unit"],
                                       "--property=" + ",".join(properties)], text=True, timeout=5)
        systemd = dict(line.split("=", 1) for line in raw.splitlines() if "=" in line)
    else:
        systemd = previous["systemd"]
    require(systemd["ControlGroup"] == groups[0] and systemd["ActiveState"] == "active"
            and systemd["KillMode"] == "control-group" and systemd["SendSIGKILL"] == "yes"
            and 0 < duration(systemd["RuntimeMaxUSec"]) <= 1500
            and 0 < duration(systemd["TimeoutStopUSec"]) <= 15, "actual systemd containment/deadline differs")
    resolved = str(Path(output_parent).resolve(strict=True))
    mounts = []
    for line in Path("/proc/self/mountinfo").read_text().splitlines():
        fields = line.split()
        mount = fields[4].replace("\\040", " ")
        if resolved == mount or resolved.startswith(mount.rstrip("/") + "/"):
            mounts.append((len(mount), line))
    require(mounts, "output filesystem mount missing")
    return {"cgroup_path": str(group), "limits": limits, "systemd": systemd,
            "mount": max(mounts)[1], "output_device": Path(output_parent).stat().st_dev}


def refs_in(value):
    if isinstance(value, dict):
        if set(value) == {"path", "bytes", "sha256"}:
            yield value
        else:
            for child in value.values():
                yield from refs_in(child)
    elif isinstance(value, list):
        for child in value:
            yield from refs_in(child)


def all_frozen_refs(plan):
    refs = list(refs_in({key: plan[key] for key in ("protocol", "files", "build", "build_body", "inputs", "copies", "window", "window_body")}))
    for entry in plan["copies"].values():
        manifest = read(entry["source_manifest"]["path"])
        refs += [{"path": str(Path(manifest["output_path"]) / name), **pin} for name, pin in manifest["after"].items()]
        for key in ("formatter",):
            if manifest.get(key):
                refs.append({name: manifest[key][name] for name in ("path", "bytes", "sha256")})
        record = read(entry["build_record"]["path"])
        refs += list(refs_in(record))
    unique = {}
    for ref in refs:
        require(ref["path"] not in unique or unique[ref["path"]] == ref, "conflicting frozen file identities")
        unique[ref["path"]] = ref
    return [unique[key] for key in sorted(unique)]


def verify_build_preparation(plan):
    """Check retained producer evidence without invoking tools or reading live /proc."""
    build = plan["build_body"]
    require(build["limits"] == BUILD_LIMITS and all(type(v) is int for v in build["limits"].values()),
            "build limits differ from the finite preparation protocol")
    verify(build["freeze"])
    frozen = read(build["freeze"]["path"])
    require(frozen["schema"] == "r1.write-phase-build-freeze/v1"
            and frozen["limits"] == build["limits"] and frozen["settings"] == build["settings"]
            and frozen["host"] == plan["host"], "build freeze settings/host/limits differ")
    outer = build["outer_before"]
    require(outer == frozen["outer"] == build["outer_after"], "build outer controls or events changed")
    controls, unit = outer["controls"], outer["systemd"]
    require(controls["memory.max"].isdigit() and 0 < int(controls["memory.max"]) <= BUILD_LIMITS["memory_bytes"]
            and controls["memory.swap.max"] == "0" and controls["pids.max"].isdigit()
            and 0 < int(controls["pids.max"]) <= 512, "unbounded build memory/swap/pids")
    cpu = controls["cpu.max"].split()
    require(type(plan["host"]["logical_cpus"]) is int and plan["host"]["logical_cpus"] > 0
            and len(cpu) == 2 and all(x.isdigit() and int(x) > 0 for x in cpu)
            and int(cpu[0]) <= int(cpu[1]) * plan["host"]["logical_cpus"]
            and isinstance(controls["cpuset.cpus.effective"], str) and controls["cpuset.cpus.effective"],
            "unbounded build CPU quota")
    group = unit["ControlGroup"]
    require(isinstance(group, str) and group.startswith("/") and group != "/"
            and all(part not in ("", ".", "..") for part in group[1:].split("/"))
            and outer["cgroup_path"] == "/sys/fs/cgroup" + group
            and unit["ActiveState"] == "active" and unit["KillMode"] == "control-group"
            and unit["SendSIGKILL"] == "yes" and 0 < duration(unit["RuntimeMaxUSec"]) <= 3300
            and 0 < duration(unit["TimeoutStopUSec"]) <= 15, "unbounded or mismatched build systemd containment")
    require(set(outer["events"]) == {"memory.events", "pids.events"}
            and all(isinstance(rows, dict) and rows and all(isinstance(k, str) and isinstance(v, str) and v.isdigit()
                    for k, v in rows.items()) for rows in outer["events"].values()), "build resource event evidence incomplete")
    require(frozen["owner"] == frozen["files"]["owner"], "build owner freeze differs")
    verify(frozen["owner"])
    owner = read(frozen["owner"]["path"])
    require(owner["schema"] == "r1.write-phase-build-owner/v1" and owner["resources_authorized"] is True
            and re.fullmatch(r"[A-Za-z0-9_.@-]+\.service", owner["systemd_unit"])
            and group.rsplit("/", 1)[-1] == owner["systemd_unit"], "build owner/unit evidence differs")
    for key in ("budget_record", "systemctl"):
        verify(owner[key])
    started, ended = instant(build["started_at"]), instant(build["ended_at"])
    require(started <= instant(frozen["issued_at"]) <= ended <= instant(plan["issued_at"])
            and ended < instant(owner["deadline_utc"])
            and 0 < (instant(owner["deadline_utc"]) - started).total_seconds() <= 3300,
            "build freeze/window chronology differs")
    require(frozen["files"]["protocol"] == plan["protocol"], "build frozen protocol differs")
    for key in TOOL_FILES:
        require(frozen["files"][key] == plan["files"][key], "build frozen tool differs: " + key)

    def checked_refs(refs):
        result = {}
        for ref in refs:
            verify(ref)
            require(ref["path"] not in result, "duplicate build frozen identity")
            result[ref["path"]] = ref
        return result

    initial, final = checked_refs(frozen["identities"]), checked_refs(build["frozen_inputs"])
    require(all(final.get(path) == ref for path, ref in initial.items())
            and final.get(build["freeze"]["path"]) == build["freeze"], "build input freeze closure differs")
    required = [*frozen["files"].values(), *frozen["inputs"].values(), build["compiler"], build["cargo"],
                build["formatter"], owner["budget_record"], owner["systemctl"]]
    require(all(initial.get(ref["path"]) == ref for ref in required), "build initial identity closure incomplete")
    environment = frozen["environment"]
    require(environment["RUSTC"] == build["compiler"]["path"] and environment["RUSTFMT"] == build["formatter"]["path"]
            and environment["RUSTFLAGS"] == build["settings"]["rustflags"] and environment["CARGO_INCREMENTAL"] == "0"
            and environment["CARGO_BUILD_JOBS"] == str(BUILD_LIMITS["jobs"])
            and environment["CARGO_NET_OFFLINE"] == "true", "build frozen compiler environment differs")
    labels = ["cargo-version", "rustc-version", "rustfmt-version"] + ["copy-" + k for k in COPY_KEYS] + ["build-" + k for k in COPY_KEYS]
    require([stage["label"] for stage in build["stages"]] == labels, "build preparation stages incomplete or reordered")
    stage_refs = {}
    for stage in build["stages"]:
        verify(stage["record"])
        record = read(stage["record"]["path"])
        label = stage["label"]
        timeout = BUILD_LIMITS["build_seconds" if label.startswith("build-") else "copy_seconds" if label.startswith("copy-") else "tool_seconds"]
        limits = {"timeout_seconds": timeout, "grace_seconds": 5, "kill_wait_seconds": 5, "poll_seconds": 0.05,
                  "memory_limit_bytes": BUILD_LIMITS["memory_bytes"], "min_free_memory_bytes": BUILD_LIMITS["min_free_memory_bytes"],
                  "disk_reserve_bytes": BUILD_LIMITS["disk_reserve_bytes"]}
        require(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
                and record["stop_reason"] == "completed" and record["limits"] == limits
                and all(type(record[k]) is int and record[k] == 0 for k in ("child_exit_code", "supervisor_exit_code"))
                and record["errors"] == [] and record["shell"] is False and record["cleanup_complete"] is True
                and record["identity_unchanged"] is True and record["identity_before"] == record["identity_after"]
                and record["forced"] is False and record["stop_requested_at"] is None, "build preparation stage failed or limits differ")
        require(instant(frozen["issued_at"]) <= instant(record["created_at"]) <= instant(record["started_at"])
                <= instant(record["ended_at"]) <= ended, "build preparation stage chronology differs")
        for ref in refs_in(record):
            verify(ref)
        stage_refs[label] = stage["record"]
    return frozen, stage_refs


def verify_build(plan):
    build = plan["build_body"]
    require(build == read(plan["build"]["path"]) and build["schema"] == "r1.write-phase-build/v1"
            and build["status"] == "completed" and build["host"] == plan["host"], "build identity/status/host differs")
    require(set(build["copies"]) == set(COPY_KEYS) and build["copies"] == plan["copies"], "four build copies required")
    settings = build["settings"]
    require(settings["profile"] == "release" and settings["fresh_targets"] is True
            and isinstance(settings["rustflags"], str) and isinstance(settings["target"], str) and settings["target"], "build settings incomplete")
    for key in ("compiler", "cargo"):
        verify(build[key])
    frozen, stage_refs = verify_build_preparation(plan)
    pins = read(plan["files"]["source_pins"]["path"])["roles"]
    source_roots, targets = set(), set()
    for key, entry in build["copies"].items():
        role, mode = key.split("-", 1)
        manifest = read(entry["source_manifest"]["path"])
        require(manifest["schema"] == "r1.write-phase-source-copy/v1" and manifest["role"] == role
                and manifest["mode"] == mode and manifest["revision"] == pins[role]["revision"], "wrong source copy")
        before = copy.deepcopy(pins[role]["files"])
        example = "crates/formats/examples/sol_codec_bench.rs"
        before[example] = pins["candidate"]["files"][example]
        require(manifest["before"] == before and set(manifest["after"]) == set(before), "source copy original files differ")
        changed = {name for name in before if before[name] != manifest["after"][name]}
        expected = {"crates/formats/src/sol_indexed.rs", "crates/formats/src/lib.rs", example} if mode == "instrumented" else set()
        require(changed == expected, "source patch changed unexpected files")
        for name, file_key in (("apply.py", "applier"), ("runtime.rs.inc", "runtime"), ("source-pins.json", "source_pins"), ("validate.py", "validator")):
            require(manifest["instrumentation"][name] == {k: plan["files"][file_key][k] for k in ("bytes", "sha256")}, "instrumentation differs from source copy")
        require(manifest["instrumentation"]["protocol.json"] == {k: plan["protocol"][k] for k in ("bytes", "sha256")}, "source protocol differs")
        source_roots.add(manifest["output_path"])
        record = read(entry["build_record"]["path"])
        require(entry["build_record"] == stage_refs["build-" + key], "copy/build stage record differs")
        require(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
                and record["stop_reason"] == "completed" and all(type(record[key]) is int and record[key] == 0 for key in ("child_exit_code", "supervisor_exit_code")) and record["cleanup_complete"] is True
                and record["identity_unchanged"] is True and record["identity_before"] == record["identity_after"]
                and record["errors"] == [] and record["shell"] is False, "failed build supervision")
        require(record["cwd"] == manifest["output_path"] and record["argv"] == entry["argv"], "build command/source differs")
        require(all(ref in record["identity_before"] for ref in (build["compiler"], entry["source_manifest"], entry["build_environment"])), "build compiler/source/environment identity missing")
        build_env = read(entry["build_environment"]["path"])
        require(build_env["schema"] == "r1.write-phase-build-environment/v1" and build_env["host"] == plan["host"]
                and build_env["environment"] == frozen["environment"]
                and build_env["environment"]["RUSTFLAGS"] == settings["rustflags"]
                and build_env["environment"]["CARGO_INCREMENTAL"] == "0"
                and build_env["target_exists"] is False and build_env["cwd"] == manifest["output_path"], "build environment evidence differs")
        argv = entry["argv"]
        require(argv[0] == build["cargo"]["path"] and "--release" in argv and "--locked" in argv
                and "--target-dir" in argv and "--example" in argv
                and argv[argv.index("--example") + 1] == "sol_codec_bench", "wrong build command")
        target = argv[argv.index("--target-dir") + 1]
        require(argv == [build["cargo"]["path"], "build", "--release", "--locked", "-p", "formats",
                         "--example", "sol_codec_bench", "--target", settings["target"], "--target-dir", target]
                and record["resolved_argv"] == argv, "build command is not the fixed release example command")
        require(Path(target).is_absolute() and entry["target_absent_before"] is True and build_env["target_dir"] == target
                and Path(entry["binary"]["path"]) == Path(target) / settings["target"] / "release/examples/sol_codec_bench"
                and "--target" in argv and argv[argv.index("--target") + 1] == settings["target"], "fresh target/binary binding missing")
        require(entry["host_before"] == plan["host"] == entry["host_after"], "build CPU/boot changed")
        require(instant(record["ended_at"]) <= instant(plan["issued_at"]), "build completed after freeze")
        require(instant(build_env["observed_at"]) <= instant(record["created_at"]) <= instant(record["started_at"]) <= instant(record["ended_at"]), "build chronology differs")
        targets.add(target)
    require(len(source_roots) == len(targets) == 4, "source/target directories must be separate")
    roots = [Path(value) for value in [*source_roots, *targets]]
    require(all(path.is_absolute() for path in roots) and all(not left.is_relative_to(right)
            for i, left in enumerate(roots) for j, right in enumerate(roots) if i != j), "source/target directories overlap")


def verify_plan(plan, *, live=False):
    body = copy.deepcopy(plan)
    saved = body.pop("plan_sha256")
    require(plan["schema"] == "r1.write-phase-plan/v1" and digest(body) == saved, "plan digest/schema differs")
    require(read(plan["protocol"]["path"]) == plan["protocol_body"], "protocol body changed")
    protocol = plan["protocol_body"]
    require(protocol["schema"] == "r1.sol-write-phase-protocol/v1" and protocol["samples"] == {"warmup": 18, "measured": 108, "total": 126}, "wrong finite protocol")
    require(plan["limits"] == protocol["limits"] and plan["window_body"] == read(plan["window"]["path"]), "limits/window differs")
    require(plan["window_body"]["schema"] == "r1.write-phase-window/v1"
            and plan["window_body"]["dedicated_host"] is True
            and plan["window_body"]["no_transfer_build_or_other_work"] is True, "dedicated window declaration missing")
    selected = read(plan["files"]["input_selection"]["path"])
    require(selected["schema"] == "r1.codec-input-selection/v1", "wrong input selection schema")
    require(set(plan["inputs"]) == set(protocol["cases_in_order"]), "case set changed")
    for case, ref in plan["inputs"].items():
        require(all(ref[k] == selected["cases"][case][k] for k in ("bytes", "sha256")), "input bytes differ")
    for ref in all_frozen_refs(plan):
        verify(ref)
    verify_build(plan)
    if live:
        require(identity(Path(__file__)) == plan["files"]["runner"], "executing runner differs")
        for entry in plan["copies"].values():
            manifest = read(entry["source_manifest"]["path"])
            root = Path(manifest["output_path"])
            expected = {name for name in manifest["after"] if name.startswith("crates/")}
            actual = {path.relative_to(root).as_posix() for path in (root / "crates").rglob("*") if path.is_file()}
            require(actual == expected and not (root / ".cargo").exists(), "source copy has unpinned Cargo/crate files")
        require(host() == plan["host"], "CPU/boot changed")
        require(environment(plan["window_body"], Path(plan["run_root"]).parent) == plan["environment"], "outer containment/filesystem changed")


def schedule(protocol):
    result = []
    for case in protocol["cases_in_order"]:
        for block, indices in enumerate([list(range(6)), *protocol["measured_block_arm_indices"]]):
            for index in indices:
                result.append({"index": len(result), "case": case, "arm": protocol["arms"][index],
                               "block": block, "excluded": block == 0})
    require(len(result) == 126 and len({tuple(row.values()) for row in result}) == 126, "invalid finite schedule")
    return result


def freeze(args):
    require(ENV not in os.environ, "phase environment must be unset before freeze")
    run_root = args.run_root.resolve()
    require(not run_root.exists() and run_root.parent.is_dir(), "run root must be new under an existing parent")
    window_ref = identity(args.window)
    window = read(args.window)
    require(re.fullmatch(r"[A-Za-z0-9_.@-]+\.service", window["systemd_unit"]), "invalid unit name")
    verify(window["systemctl"])
    deadline = instant(window["deadline_utc"])
    require(0 < (deadline - dt.datetime.now(dt.timezone.utc)).total_seconds() <= 1500, "window deadline invalid")
    protocol = read(HERE / "protocol.json")
    files = {key: identity(HERE / name) for key, name in TOOL_FILES.items()}
    files.update(input_selection=identity(HERE.parent / "inputs.json"), supervisor=identity(args.supervisor), python=identity(sys.executable))
    build_ref, build = identity(args.build), read(args.build)
    env = environment(window, run_root.parent)
    process = process_scan(cgroup=Path(env["cgroup_path"]), idle=True)
    events = {name: dict(line.split() for line in (Path(env["cgroup_path"]) / name).read_text().splitlines())
              for name in ("memory.events", "pids.events")}
    require(shutil.disk_usage(run_root.parent).free >= 10 * 1024**3, "need >=10GiB free for outputs plus reserve")
    plan = {"schema": "r1.write-phase-plan/v1", "issued_at": now(), "protocol": identity(HERE / "protocol.json"),
            "protocol_body": protocol, "files": files, "build": build_ref, "build_body": build,
            "copies": build["copies"], "inputs": {case: identity(args.inputs / (case + ".sol")) for case in protocol["cases_in_order"]},
            "host": host(), "environment": env, "process_baseline": process["baseline_foreign"],
            "cgroup_events": events,
            "limits": protocol["limits"], "run_root": str(run_root), "window": window_ref, "window_body": window}
    plan["plan_sha256"] = digest(plan)
    verify_plan(plan, live=True)
    write(args.out, plan)
    return plan


def health(plan, *, idle=False, full=False):
    require(dt.datetime.now(dt.timezone.utc) < instant(plan["window_body"]["deadline_utc"]), "owner window deadline exceeded")
    observed_host = host()
    require(observed_host == plan["host"], "CPU/boot changed")
    observed_environment = environment(plan["window_body"], Path(plan["run_root"]).parent,
                                       full=full, previous=plan["environment"])
    require(observed_environment == plan["environment"], "outer containment/filesystem changed")
    group = Path(observed_environment["cgroup_path"])
    scan = process_scan(plan["process_baseline"], group, idle=idle)
    events = {}
    for name in ("memory.events", "pids.events"):
        events[name] = dict(line.split() for line in (group / name).read_text().splitlines())
    require(events == plan["cgroup_events"], "cgroup resource-limit event changed")
    return observed_host, observed_environment, scan, events


def capture_snapshot(plan, path, expected_env):
    record = {"schema": "r1.write-phase-stage-snapshot/v1", "observed_at": now(), "status": "clear",
              "phase_env": os.environ.get(ENV), "identities": [], "host": None,
              "environment": None, "process_scan": None, "cgroup_events": None, "errors": []}
    try:
        expected = all_frozen_refs(plan)
        record["identities"] = [identity(ref["path"]) for ref in expected]
        require(record["identities"] == expected, "frozen identity changed before/after stage")
        for entry in plan["copies"].values():
            manifest = read(entry["source_manifest"]["path"])
            root = Path(manifest["output_path"])
            expected_names = {name for name in manifest["after"] if name.startswith("crates/")}
            actual_names = {item.relative_to(root).as_posix() for item in (root / "crates").rglob("*") if item.is_file()}
            require(actual_names == expected_names and not (root / ".cargo").exists(), "source copy gained unpinned files")
        require(record["phase_env"] == expected_env, "phase environment differs")
        record["host"], record["environment"], record["process_scan"], record["cgroup_events"] = health(plan, idle=True, full=True)
    except BaseException as error:
        record["status"] = "failed"
        record["errors"].append({"type": type(error).__name__, "message": str(error)})
    write(path, record)
    require(record["status"] == "clear", "snapshot failed: " + json.dumps(record["errors"]))
    return identity(path)


class StageGuard:
    def __init__(self, plan, deadline):
        self.plan, self.deadline = plan, deadline
        self.last = None
        self.record = {"schema": "r1.write-phase-stage-guard/v1", "status": "clear", "checks": 0,
                       "max_gap_seconds": 0.0, "poll_seconds": 0.05, "first_failure": None,
                       "assurance": ASSURANCE}

    def check(self):
        if self.record["first_failure"] is not None:
            return  # Do not prevent the supervisor's finally block from verifying cleanup.
        try:
            current = time.monotonic()
            if self.last is not None:
                self.record["max_gap_seconds"] = max(self.record["max_gap_seconds"], current - self.last)
                require(current - self.last <= self.plan["protocol_body"]["runner_monitor"]["max_gap_seconds"], "dedicated-window monitoring gap exceeded")
            self.last = current
            self.record["checks"] += 1
            require(current < self.deadline, "campaign monotonic deadline exceeded")
            health(self.plan)
        except BaseException as error:
            self.record["status"] = "failed"
            self.record["first_failure"] = {"at": now(), "type": type(error).__name__, "reason": str(error)}
            raise


def expected_supervisor_identities(plan, label):
    role, mode = label["arm"].split("-", 1)
    key = role + ("-plain" if mode == "plain" else "-instrumented")
    refs = [plan["copies"][key]["binary"], plan["files"]["python"], plan["files"]["supervisor"],
            plan["inputs"][label["case"]], plan["files"]["runner"], plan["protocol"], plan["files"]["validator"]]
    return list({ref["path"]: ref for ref in refs}.values())


def supervise(plan, label, directory, deadline):
    refs = expected_supervisor_identities(plan, label)
    role, mode = label["arm"].split("-", 1)
    key = role + ("-plain" if mode == "plain" else "-instrumented")
    manifest = read(plan["copies"][key]["source_manifest"]["path"])
    remaining = min(plan["limits"]["per_process_seconds"], deadline - time.monotonic() - 11)
    require(remaining > 0, "insufficient campaign deadline for bounded cleanup")
    # Reserve cleanup rather than silently shortening the fixed per-process budget.
    require(remaining == plan["limits"]["per_process_seconds"], "not enough time for the next fixed-duration stage")
    supervisor = load(plan["files"]["supervisor"]["path"], "write_phase_supervisor")
    guard = StageGuard(plan, deadline)
    original = supervisor.LinuxProcess

    class CheckedProcess(original):
        def sample(self):
            result = super().sample()
            guard.check()
            return result

    supervisor.LinuxProcess = CheckedProcess
    limits = plan["limits"]
    argv = ["--record", str(directory / "supervisor.json"), "--cwd", manifest["output_path"],
            "--stdout", str(directory / "stdout.log"), "--stderr", str(directory / "stderr.log"),
            "--samples", str(directory / "supervisor.samples.jsonl"), "--disk-path", str(directory),
            "--timeout-seconds", str(limits["per_process_seconds"]), "--memory-limit-bytes", str(limits["memory_bytes"]),
            "--min-free-memory-bytes", str(limits["min_free_memory_bytes"]), "--disk-reserve-bytes", str(limits["disk_reserve_bytes"]),
            "--grace-seconds", "5", "--kill-wait-seconds", "5", "--poll-seconds", "0.05"]
    for ref in refs[3:]:
        argv += ["--identity-file", ref["path"]]
    argv += ["--", refs[0]["path"], plan["inputs"][label["case"]]["path"], "stream-write", "1", str(directory / "output")]
    try:
        return supervisor.main(argv)
    finally:
        write(directory / "stage-guard.json", guard.record)


def phase_check(plan, phase, report, rewritten):
    validator = load(plan["files"]["validator"]["path"], "write_phase_validator")
    with Path(rewritten).open("rb") as stream:
        header = stream.read(106)
        require(len(header) == 106 and header[:10] == b"SLVRSOLV\x03\x00", "wrong rewritten format")
        groups = int.from_bytes(header[98:106], "little")
        directory = 106 + int.from_bytes(header[50:58], "little")
        size = Path(rewritten).stat().st_size
        require(0 < groups <= 1000000 and directory + groups * 64 <= size, "invalid directory extent")
        stream.seek(directory)
        compressed = 0
        for _ in range(groups):
            entry = stream.read(64)
            require(len(entry) == 64, "truncated directory")
            compressed += int.from_bytes(entry[24:28], "little")
    return validator.validate_phase(phase, expected_groups=groups, expected_file_bytes=size,
                                    expected_compressed_bytes=compressed, operation_seconds=report["timing"]["operation_seconds"])


def collect_sample(plan, label, directory, code):
    names = {"record": "supervisor.json", "report": "output/result.json", "stdout": "stdout.log", "stderr": "stderr.log",
             "samples": "supervisor.samples.jsonl", "canonical": "output/canonical.bin", "root_canonical": "output/root-canonical.bin",
             "rewritten": "output/rewritten.sol", "before": "before.json", "after": "after.json", "guard": "stage-guard.json"}
    result = {**label, **{key: identity(directory / name) for key, name in names.items()}}
    record, report, guard = read(result["record"]["path"]), read(result["report"]["path"]), read(result["guard"]["path"])
    require(type(code) is int and code == 0 and record["state"] == "completed" and record["stop_reason"] == "completed"
            and all(type(record[key]) is int and record[key] == 0 for key in ("child_exit_code", "supervisor_exit_code"))
            and record["cleanup_complete"] is True and record["identity_unchanged"] is True
            and record["errors"] == [] and record["shell"] is False, "failed supervisor/resource/cleanup")
    require(record["identity_before"] == record["identity_after"] == expected_supervisor_identities(plan, label), "supervisor identities differ")
    require(guard["status"] == "clear" and type(guard["checks"]) is int and guard["checks"] >= 1
            and guard["checks"] == record["measurement"]["sample_count"] and guard["first_failure"] is None
            and guard["max_gap_seconds"] <= plan["protocol_body"]["runner_monitor"]["max_gap_seconds"], "dedicated-window guard failed")
    limits = plan["limits"]
    require(record["limits"] == {"timeout_seconds": limits["per_process_seconds"], "grace_seconds": 5.0,
            "kill_wait_seconds": 5.0, "poll_seconds": 0.05, "memory_limit_bytes": limits["memory_bytes"],
            "min_free_memory_bytes": limits["min_free_memory_bytes"], "disk_reserve_bytes": limits["disk_reserve_bytes"]}, "supervisor limits differ")
    item = plan["inputs"][label["case"]]
    argv = [expected_supervisor_identities(plan, label)[0]["path"], item["path"], "stream-write", "1", str(directory / "output")]
    require(record["argv"] == record["resolved_argv"] == argv, "sample command differs")
    role, mode = label["arm"].split("-", 1)
    key = role + ("-plain" if mode == "plain" else "-instrumented")
    require(record["cwd"] == read(plan["copies"][key]["source_manifest"]["path"])["output_path"], "sample cwd/source differs")
    require(record["outputs"] == {key: result[key] for key in ("stdout", "stderr", "samples")}, "supervisor output references differ")
    require(read(result["stdout"]["path"]) == report, "benchmark stdout/report differ")
    require(report["schema"] == "r1.sol-codec-sample/v1" and report["status"] == "completed"
            and type(report["format_version"]) is int and report["format_version"] == 3
            and report["operation"] == "stream-write" and type(report["iterations"]) is int and report["iterations"] == 1
            and report["metadata"]["mode"] == "Full" and report["input"]["bytes"] == item["bytes"], "wrong sample format/mode/operation")
    for value in report["timing"].values():
        require(type(value) in (int, float) and math.isfinite(value) and value >= 0, "invalid sample timing")
    require(set(report["timing"]) == {"preparation_load_seconds", "open_seconds", "operation_seconds", "operation_seconds_per_iteration", "validation_output_seconds"}
            and report["timing"]["operation_seconds"] > 0 and report["timing"]["operation_seconds_per_iteration"] == report["timing"]["operation_seconds"], "invalid operation duration")
    count = report["metadata"]["stored_nodes"]
    require(type(count) is int and count > 0 and report["decoded_strategy_blocks"] == report["decoded_value_blocks"] == count
            and len(report["selected_srefs"]) == count and all(type(x) is int and x >= 0 for x in report["selected_srefs"])
            and report["selected_srefs"] == sorted(set(report["selected_srefs"])) and report["selected_srefs"][0] == 0, "decoded policy incomplete")
    require(report["solve_iterations"] == report["metadata"]["meta"]["iterations"], "solve metadata differs")
    for key in ("canonical", "root_canonical"):
        filename = "canonical.bin" if key == "canonical" else "root-canonical.bin"
        require(report[key]["file"] == filename and report[key]["bytes"] == result[key]["bytes"] and re.fullmatch("[0-9a-f]{64}", report[key]["blake3"]), "canonical record differs")
    require(re.fullmatch("[0-9a-f]{64}", report["input"]["blake3"]), "invalid input BLAKE3")
    require(all(result["rewritten"][key] == item[key] for key in ("bytes", "sha256"))
            and report["rewritten"] == {"file": "rewritten.sol", **report["input"]}, "rewritten identity differs")
    same_bytes(result["rewritten"]["path"], item["path"])
    phase_path = directory / "phase.json"
    if label["arm"].endswith("-on"):
        validation = phase_check(plan, read(phase_path), report, result["rewritten"]["path"])
        require(validation["status"] == "phase_record_valid_not_campaign_acceptance", "phase incomplete")
        write(directory / "phase-validation.json", validation)
        result.update(phase=identity(phase_path), phase_validation=identity(directory / "phase-validation.json"))
    else:
        require(not phase_path.exists(), "unexpected OFF/plain phase output")
        result.update(phase=None, phase_validation=None)
    result.update(timing=report["timing"], metadata=report["metadata"], measurement=record["measurement"], input=report["input"])
    return result


def pair_check(reference, result):
    require(reference["metadata"] == result["metadata"] and reference["input"] == result["input"], "cross-arm metadata/input differs")
    for key in ("canonical", "root_canonical", "rewritten"):
        require(all(reference[key][field] == result[key][field] for field in ("bytes", "sha256")), "cross-arm output identity differs")
        same_bytes(reference[key]["path"], result[key]["path"])


def analyze(state, plan):
    require(state["status"] == "completed" and state["first_failure"] is None, "incomplete campaign cannot publish comparison")
    expected = schedule(plan["protocol_body"])
    require([{key: row[key] for key in expected[0]} for row in state["samples"]] == expected, "sample order/count differs")
    rows = []
    low, high = plan["protocol_body"]["calibration"]["ratio_bounds"]
    for case in plan["protocol_body"]["cases_in_order"]:
        samples = [row for row in state["samples"] if row["case"] == case and not row["excluded"]]
        arms = {}
        for arm in plan["protocol_body"]["arms"]:
            values = [row["timing"]["operation_seconds"] for row in samples if row["arm"] == arm]
            require(len(values) == 6, "missing measured arm")
            arms[arm] = {"raw_seconds": values, "median_seconds": statistics.median(values), "min_seconds": min(values), "max_seconds": max(values)}
        calibration = {}
        for role in ("baseline", "candidate"):
            off_plain = arms[role + "-off"]["median_seconds"] / arms[role + "-plain"]["median_seconds"]
            on_off = arms[role + "-on"]["median_seconds"] / arms[role + "-off"]["median_seconds"]
            calibration[role] = {"off_over_plain": off_plain, "on_over_off": on_off,
                                 "attribution": "eligible_descriptive_only" if low <= off_plain <= high and low <= on_off <= high else "not_evaluated"}
        pairs = [[arms["baseline-plain"]["raw_seconds"][i], arms["candidate-plain"]["raw_seconds"][i]] for i in range(6)]
        rows.append({"case": case, "arms": arms, "calibration": calibration, "plain_pairs_seconds": pairs,
                     "plain_candidate_over_baseline": arms["candidate-plain"]["median_seconds"] / arms["baseline-plain"]["median_seconds"],
                     "plain_paired_faster_count": sum(b < a for a, b in pairs)})
    return {"schema": "r1.write-phase-comparison/v1", "status": "completed", "plan_sha256": plan["plan_sha256"],
            "rows": rows, "scope": "Descriptive writer-after-own-preload only; no solver speed/quality/R1 acceptance"}


def stage(plan, label, deadline):
    directory = Path(plan["run_root"]) / (f"{label['index']:03d}-{label['case']}-{label['block']}-{label['arm']}")
    directory.mkdir()
    expected_env = str(directory / "phase.json") if label["arm"].endswith("-on") else None
    previous = os.environ.get(ENV)
    try:
        if expected_env is None:
            os.environ.pop(ENV, None)
        else:
            os.environ[ENV] = expected_env
        capture_snapshot(plan, directory / "before.json", expected_env)
        code = supervise(plan, label, directory, deadline)
        primary = None
        if code != 0:
            try:
                guard = read(directory / "stage-guard.json")
                reason = guard["first_failure"]["reason"] if guard["first_failure"] else read(directory / "supervisor.json")["stop_reason"]
            except BaseException:
                reason = "supervisor failed without a complete record"
            primary = RuntimeError(f"first supervised failure: {reason}; exit={code}")
        try:
            capture_snapshot(plan, directory / "after.json", expected_env)
        except BaseException:
            if primary is not None:
                raise primary
            raise
        if primary is not None:
            raise primary
        return collect_sample(plan, label, directory, code)
    finally:
        if previous is None:
            os.environ.pop(ENV, None)
        else:
            os.environ[ENV] = previous


def execute(plan, plan_ref, *, perform_stage=stage, snapshot=capture_snapshot, verifier=verify_plan, clock=time.monotonic):
    require(ENV not in os.environ, "phase environment must be unset before run")
    root = Path(plan["run_root"])
    root.mkdir()
    state = {"schema": "r1.write-phase-campaign/v1", "status": "running", "started_at": now(), "ended_at": None,
             "run_root": str(root), "plan": plan_ref, "plan_sha256": plan["plan_sha256"], "samples": [],
             "initial_snapshot": None, "final_snapshot": None, "first_failure": None}
    deadline = clock() + plan["limits"]["campaign_seconds"]
    label = None
    try:
        write(root / "result.json", state)
        verifier(plan, live=True)
        state["initial_snapshot"] = snapshot(plan, root / "initial-snapshot.json", None)
        for label in schedule(plan["protocol_body"]):
            require(clock() < deadline, "campaign deadline exceeded")
            row = perform_stage(plan, label, deadline)
            reference = next((old for old in state["samples"] if old["case"] == row["case"]), None)
            if reference is not None:
                pair_check(reference, row)
            state["samples"].append(row)
            write(root / "result.json", state, exclusive=False)
        verifier(plan, live=True)
        state["final_snapshot"] = snapshot(plan, root / "final-snapshot.json", None)
        require(clock() < deadline, "campaign deadline exceeded after final checks")
        state.update(status="completed", ended_at=now())
        comparison = analyze(state, plan)
        write(root / "comparison.json", comparison)
    except BaseException as error:
        state["status"] = "failed"
        state["ended_at"] = now()
        state["first_failure"] = {"at": now(), "type": type(error).__name__, "reason": str(error), "stage": label,
                                  "available_stage_evidence": []}
        if label is not None:
            directory = root / f"{label['index']:03d}-{label['case']}-{label['block']}-{label['arm']}"
            for path in sorted(directory.rglob("*")) if directory.exists() else []:
                if path.is_file():
                    try:
                        state["first_failure"]["available_stage_evidence"].append(identity(path))
                    except BaseException:
                        pass
        if state["final_snapshot"] is None:
            try:
                state["final_snapshot"] = snapshot(plan, root / "final-snapshot.json", None)
            except BaseException:
                if (root / "final-snapshot.json").exists():
                    state["final_snapshot"] = identity(root / "final-snapshot.json")
    finally:
        write(root / "result.json", state, exclusive=False)
    return state


@contextlib.contextmanager
def window_lock(path):
    import fcntl
    path = Path(path)
    require(path.is_absolute() and path.parent.is_dir() and not path.is_symlink(), "invalid window lock path")
    with path.open("a+b") as stream:
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            yield
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    prepare = sub.add_parser("freeze")
    for name in ("build", "window", "supervisor", "inputs", "run-root", "out"):
        prepare.add_argument("--" + name, type=Path, required=True)
    run = sub.add_parser("run")
    run.add_argument("--plan", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "freeze":
        plan = freeze(args)
        print(json.dumps({"status": "frozen_not_run", "plan_sha256": plan["plan_sha256"]}))
        return 0
    plan_ref, plan = identity(args.plan), read(args.plan)
    with window_lock(plan["window_body"]["lock_path"]):
        state = execute(plan, plan_ref)
    print(json.dumps({"status": state["status"], "samples": len(state["samples"]), "first_failure": state["first_failure"]}))
    return 0 if state["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
