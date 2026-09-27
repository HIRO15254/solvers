"""Finite same-boot baseline-only Linux CPU occupancy diagnostic; no resume."""
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import struct
import sys
import tarfile
import time
import traceback

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
FLOP = HERE.parent
COMMON = FLOP / "flat-ev/timing/run.py"
DURABLE = FLOP / "worker-scratch/durable.py"
SUPERVISOR = ROOT / "tools/run_supervised.py"
PINS = {COMMON: "c4f3fbd1375f465333e2327465a098b96fb35922e8dbdcce48a3e181290b7892",
        DURABLE: "3217fe32daf623ed6547a4a073b597a6ce008670314459dfc24f4061f364a497",
        SUPERVISOR: "5bd46e106bbc48e971c43c1ea080ed16e22356e83747ec6fb57157013fd029b8"}


def load(name, path, sha=None):
    if sha is not None and hashlib.sha256(path.read_bytes()).hexdigest() != sha:
        raise ValueError("Trusted helper pin differs: " + str(path))
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


common = load("occupancy_timing", COMMON, PINS[COMMON])
durable = load("occupancy_durable", DURABLE, PINS[DURABLE])
need, read, pin, pair, located = (getattr(common, n) for n in ("need", "read", "pin", "pair", "located"))
SOLVER = "crates/engine/src/solver.rs"
SOLVER_SHA = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"
ORIGINAL = FLOP / "flat-ev/cloud32/solve.rs"
ORIGINAL_SHA = "63cba37a5b0f79dfed2ca0ddf879daada3f0fee094ee5f287f86bc1f37414d46"
EXAMPLES = {"original": "flop_cloud32_probe", "cpu": "flop_cpu_occupancy_probe"}
CPU_ADAPTER = HERE / "adapter/solve.rs"
CPU_ADAPTER_SHA = "a6746b4316216231f3a4bf02120968d7b78ddb876d6d04cd611bf1efed7ba2a5"
CASES = ("narrow", "expanded")
CONFIGS = ("16-full", "32-full", "16-onecore")
ITERATIONS = 16
LIMITS = {"grace_seconds": 0.2, "kill_wait_seconds": 5, "poll_seconds": 0.1,
          "memory_limit_bytes": 8 * 1024**3, "min_free_memory_bytes": 2 * 1024**3,
          "disk_reserve_bytes": 2 * 1024**3}
MAX_RETAINED = 240 * 1024**2


def loads(text):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            need(key not in result, "duplicate JSON key")
            result[key] = value
        return result

    def invalid(value):
        raise ValueError("non-finite JSON literal: " + value)

    def number(value):
        parsed = float(value)
        need(math.isfinite(parsed), "non-finite JSON number")
        return parsed

    return json.loads(text, object_pairs_hook=unique, parse_constant=invalid, parse_float=number)


def read(path):
    return loads(Path(path).read_text(encoding="utf-8"))


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def relative(name):
    p = PurePosixPath(name)
    need(p.parts and not p.is_absolute() and ".." not in p.parts and "\\" not in name
         and ":" not in name and str(p) == name, "unsafe relative path")
    return p


def inventory(root, deadline=None):
    result = {}
    for p in sorted(root.rglob("*")):
        need(not p.is_symlink(), "symlink is forbidden")
        if p.is_file():
            result[p.relative_to(root).as_posix()] = pin(p, deadline)
        else:
            need(p.is_dir(), "nonregular member")
    return result


def one_per_core(topology, affinity):
    need(len(affinity) == len(set(affinity)) == 32 and len(topology) == 32, "32 logical CPUs required")
    groups = {}
    need({x["cpu"] for x in topology} == set(affinity), "topology/affinity mismatch")
    for item in topology:
        groups.setdefault((str(item["socket"]), str(item["core"])), []).append(item["cpu"])
    need(len(groups) == 16 and all(len(v) == 2 for v in groups.values()), "16 physical cores with two siblings required")
    return sorted(min(v) for v in groups.values())


def host():
    need(sys.platform == "linux" and platform.machine() == "x86_64", "Linux x86_64 required")
    affinity = sorted(os.sched_getaffinity(0))
    need(os.cpu_count() == 32, "guest logical CPU count differs")
    topology = []
    for cpu in affinity:
        base = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology")
        topology.append({"cpu": cpu, "core": (base / "core_id").read_text().strip(),
                         "socket": (base / "physical_package_id").read_text().strip()})
    selected = one_per_core(topology, affinity)
    cg = Path("/sys/fs/cgroup") / Path("/proc/self/cgroup").read_text().strip().split("::", 1)[1].lstrip("/")
    quota = {str(p): (p / "cpu.max").read_text().strip() for p in (cg, *cg.parents)
             if p.is_relative_to("/sys/fs/cgroup") and (p / "cpu.max").is_file()}
    limits = {"memory_max": (cg / "memory.max").read_text().strip(), "swap_max": (cg / "memory.swap.max").read_text().strip(),
              "cpu_weight": (cg / "cpu.weight").read_text().strip(), "cpu_limits": quota}
    need(limits["memory_max"] == str(12 * 1024**3) and limits["swap_max"] == "0"
         and limits["cpu_weight"] == "100" and quota, "outer service bounds differ")
    for value in quota.values():
        maximum, period = value.split()
        need(maximum == "max" or int(maximum) / int(period) >= 32, "CPU quota below 32")
    return {"machine": "x86_64", "logical_cpus": 32, "physical_cores": 16, "affinity": affinity,
            "topology": topology, "one_per_core": selected, "cgroup": limits,
            "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(),
            "cpu_models": sorted(set(x.split(":", 1)[1].strip() for x in Path("/proc/cpuinfo").read_text().splitlines()
                                     if x.startswith("model name")))}


def schedule():
    rows = []
    for kind, adapter in (("canonical", "original"), ("calibration", "cpu")):
        for case in CASES:
            rows.append({"name": f"{kind}-{case}", "kind": kind, "case": case, "adapter": adapter,
                         "workers": 1, "configuration": "1-full", "iterations": 16, "round": None, "warmup": False})
    for case in CASES:
        for r in range(4):
            for config in (CONFIGS if r % 2 == 0 else CONFIGS[::-1]):
                rows.append({"name": f"{case}-r{r}-{config}", "kind": "diagnostic", "case": case, "adapter": "cpu",
                             "workers": 32 if config == "32-full" else 16, "configuration": config,
                             "iterations": 16, "round": r, "warmup": r == 0})
    return rows


def environment(tools):
    return {"RUSTC": tools["rustc"]["path"], "RUSTFLAGS": "-C target-cpu=native", "CARGO_BUILD_JOBS": "2",
            "CARGO_INCREMENTAL": "0", "RAYON_NUM_THREADS": "1"}


def source_bindings(originals, source, original_adapter, cpu_adapter):
    need(originals[SOLVER]["sha256"] == SOLVER_SHA and original_adapter["sha256"] == ORIGINAL_SHA
         and cpu_adapter["sha256"] == CPU_ADAPTER_SHA,
         "baseline source/adapter identity differs")
    added = {f"crates/holdem/examples/{name}.rs": value
             for name, value in ((EXAMPLES["original"], original_adapter), (EXAMPLES["cpu"], cpu_adapter))}
    need(not set(originals).intersection(added), "example overwrites original source")
    need(source == originals | added, "source must be original snapshot plus exactly two examples")
    need(all(n in originals for n in ("Cargo.toml", "Cargo.lock", ".cargo/config.toml")), "workspace metadata missing")


def archive_source(source, files, destination):
    with destination.open("xb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w|") as tar:
            for name, value in files.items():
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = value["bytes"], 0o644, 0
                with (source / name).open("rb") as stream:
                    tar.addfile(info, stream)
    need(destination.stat().st_size < 1024**2, "source archive bound exceeded")


def prepare(args):
    machine = host()
    end = dt.datetime.fromisoformat(args.deadline_utc.replace("Z", "+00:00"))
    need(end.utcoffset() == dt.timedelta(0) and 0 < end.timestamp() - time.time() <= 960, "maximum 16-minute UTC window required")
    out, workspace, source = args.out.resolve(), args.workspace.resolve(), args.source.resolve(strict=True)
    paths = [out, workspace, source]
    need(len(set(paths)) == 3 and all(not a.is_relative_to(b) for a in paths for b in paths if a != b), "owned paths overlap")
    need(not out.exists() and not workspace.exists(), "fresh output/workspace required")
    manifest_path, installation_path = ROOT / "manifest.json", ROOT / "installation.json"
    manifest, installation = read(manifest_path), read(installation_path)
    need(manifest["schema"] == "r1-cpu-occupancy-package/v1", "package schema differs")
    for name, value in manifest["files"].items():
        p = ROOT / relative(name)
        need(p.is_file() and not p.is_symlink() and p.resolve().is_relative_to(ROOT) and pin(p) == value, "installed package changed")
    need(installation["manifest"] == pin(manifest_path) and installation["destination"] == str(ROOT)
         and installation["source_revision"] == manifest["source_revision"]
         and installation["source_files"] == len(manifest["source_pins"])
         and installation["builds_or_solves_started"] == 0, "installation binding differs")
    need(len(installation["archive_sha256"]) == 64 and all(c in "0123456789abcdef" for c in installation["archive_sha256"]), "package archive identity missing")
    files = inventory(source)
    source_bindings(manifest["source_pins"], files, pin(ORIGINAL), pin(CPU_ADAPTER))
    need({n.removeprefix("source/"): v for n, v in manifest["files"].items() if n.startswith("source/")}
         == manifest["source_pins"], "packaged original source binding differs")
    controls = [Path(__file__), HERE / "analyze.py", HERE / "protocol.jp.md", COMMON, common.SHARED, DURABLE, SUPERVISOR, ORIGINAL]
    controls += [p for p in sorted((HERE / "adapter").rglob("*")) if p.is_file()]
    for p in controls:
        need(manifest["files"].get(p.relative_to(ROOT).as_posix()) == pin(p), "control not package-pinned")
    for p, sha in PINS.items():
        need(pin(p)["sha256"] == sha, "frozen helper changed")
    tools = {n: located(getattr(args, n).resolve(strict=True)) for n in ("cargo", "rustc", "taskset")}
    out.mkdir(parents=True)
    workspace.mkdir(parents=True)
    for p in (out / "inputs", out / "canonical", workspace / "canonical"):
        p.mkdir()
    captured = {}
    for p in [*controls, manifest_path, installation_path]:
        value = pin(p)
        retained = out / "inputs" / value["sha256"]
        if not retained.exists():
            shutil.copyfile(p, retained)
        captured[str(p)] = {**value, "retained": retained.relative_to(out).as_posix()}
    archive_source(source, files, out / "source.tar.gz")
    plan = {"schema": "r1.cpu-occupancy/v1", "output": str(out), "workspace": str(workspace), "created_at": now(),
            "deadline_utc": args.deadline_utc, "deadline_monotonic": time.monotonic() + end.timestamp() - time.time(),
            "host": machine, "source": {"path": str(source), "files": files, "archive": located(out / "source.tar.gz")},
            "controls": captured, "tools": tools, "environment": environment(tools), "limits": LIMITS,
            "schedule": schedule(), "package": {"manifest": pin(manifest_path), "archive_sha256": installation["archive_sha256"]}}
    durable.sync_files([located(p) for p in out.rglob("*") if p.is_file()])
    durable.atomic_json(out / "plan.json", plan, once=True)
    live(plan)
    print(json.dumps({"status": "prepared", "solves": 28, "diagnostic_rows": 24}), flush=True)


def live(plan, reserve=0):
    common.deadline_check(plan["deadline_monotonic"], reserve)
    need(time.time() + reserve < dt.datetime.fromisoformat(plan["deadline_utc"]).timestamp(), "UTC deadline reached")
    need(host() == plan["host"] and plan["schedule"] == schedule() and plan["limits"] == LIMITS, "host or protocol changed")
    need(plan["environment"] == environment(plan["tools"]), "environment changed")
    for path, value in plan["controls"].items():
        need(pin(path, plan["deadline_monotonic"]) == pair(value), "control changed")
    need(inventory(Path(plan["source"]["path"]), plan["deadline_monotonic"]) == plan["source"]["files"], "source changed")
    for value in plan["tools"].values():
        need(pin(value["path"], plan["deadline_monotonic"]) == pair(value), "tool changed")


def terminal(record):
    need(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
         and record["stop_reason"] == "completed" and not record["errors"]
         and record["child_exit_code"] == record["supervisor_exit_code"] == 0
         and record["cleanup_complete"] is True and not record["forced"] and record["last_sample"]["pids"] == [],
         "supervisor terminal/cleanup differs")
    need(record["identity_unchanged"] and record["identity_before"] == record["identity_after"], "stage identity differs")
    identities = record["identity_before"]
    need(len({x["path"] for x in identities}) == len(identities), "duplicate supervisor identity path")


def raw_outputs(record, directory):
    suffixes = {"stdout": "stdout.log", "stderr": "stderr.log", "samples": "samples.jsonl"}
    need(set(record["outputs"]) == set(suffixes), "supervisor raw output set differs")
    for key, suffix in suffixes.items():
        need(record["outputs"][key]["path"] == str(directory) + "/supervisor." + suffix, "supervisor raw output path differs")
    return record["outputs"].values()


def stage(out, plan, row, receipt, seconds, cwd, binaries=()):
    live(plan, seconds + 10)
    need(pin(out / "plan.json") == receipt["plan"], "prepared plan changed")
    need(sum(p.stat().st_size for p in out.rglob("*") if p.is_file()) <= MAX_RETAINED, "proof retention bound exceeded")
    directory = out / row["name"]
    directory.mkdir()
    row.update(status="running", started_at=now(), host_before=host())
    args = ["--record", str(directory / "supervisor.json"), "--cwd", str(cwd), "--disk-path", str(out), "--timeout-seconds", str(seconds)]
    for key, value in LIMITS.items():
        args += ["--" + key.replace("_", "-"), str(value)]
    identities = [out / "plan.json", Path(__file__), SUPERVISOR, Path(row["command"][0]), *map(Path, binaries)]
    for p in dict.fromkeys(identities):
        args += ["--identity-file", str(p)]
    row["supervisor_argv"] = args + ["--", *row["command"]]
    durable.atomic_json(out / "execution.json", receipt)
    try:
        row["supervisor_exit"] = load("occupancy_supervisor", SUPERVISOR, PINS[SUPERVISOR]).main(row["supervisor_argv"])
    finally:
        row["ended_at"] = now()
        if (directory / "supervisor.json").exists():
            row["record"] = located(directory / "supervisor.json")
        durable.atomic_json(out / "execution.json", receipt)
    need(row["supervisor_exit"] == 0, "bounded process failed")
    record = read(directory / "supervisor.json")
    terminal(record)
    for value in raw_outputs(record, directory):
        need(pin(value["path"]) == pair(value), "supervisor output changed")
    row.update(process_seconds=record["elapsed_seconds"], root_os_peak_resident_bytes=record["last_sample"]["root_os_peak_resident_bytes"],
               root_os_peak_source=record["last_sample"]["root_os_peak_source"], host_after=host())
    live(plan)
    return directory


def cpulist(text):
    cpus = []
    for group in text.split(","):
        bounds = group.split("-")
        need(1 <= len(bounds) <= 2, "invalid CPU list")
        a, b = int(bounds[0]), int(bounds[-1])
        need(0 <= a <= b < 4096, "CPU list bounds differ")
        cpus.extend(range(a, b + 1))
    need(cpus == sorted(set(cpus)), "CPU list is not unique/sorted")
    return cpus


def validate_values(row, result, invocation, quality):
    for value in (result, invocation):
        need(value["schema"] == "r1.flop-native-solve/v1" and value["case"] == row["case"]
             and value["threads"] == row["workers"] and value["iterations"] == 16, "condition differs")
    need(result["performance_claim"] is False and result["cfv_capture"] is False
         and result["state_file"] == "state.bin" and result["quality_file"] == "quality.json", "result scope differs")
    need(result["status"] == "completed" and invocation["planned_iterations"] == 16
         and invocation["storage"] == "f32" and invocation["schedule"] == "dcfr"
         and invocation["chance_depth"] == 2 and invocation["min_children"] == 12
         and invocation["alpha"] == 1.5 and invocation["beta"] == 0 and invocation["gamma"] == 3
         and invocation["pow4_reset"] is True and invocation["quality_target"] is None and invocation["cfv_capture"] is False, "solver semantics differ")
    need(quality["schema"] == "r1.flop-native-quality/v1" and quality["cfv_capture"] is False
         and quality["case"] == row["case"] and quality["iterations"] == 16
         and quality["root_support"] == list(common.HEADERS[row["case"]][2:4]) and quality["quality_target"] is None, "quality condition differs")
    need(quality["normalizer_bits"] == struct.pack(">d", 870.0 if row["case"] == "narrow" else 8700.0).hex(), "normalizer differs")
    for metric in ("ev", "br", "exploitability"):
        need(len(quality[metric]) == len(quality[metric + "_bits"]) == 2, "quality dimensions differ")
        for value, bits in zip(quality[metric], quality[metric + "_bits"]):
            need(math.isfinite(value) and struct.pack(">d", value).hex() == bits, "quality value/bits differ")
    for key in ("build_seconds", "cfr_seconds", "state_write_seconds", "quality_seconds"):
        need(math.isfinite(result[key]) and result[key] > 0, "nonpositive phase timing")


def validate_cpu(row, cpu, result, expected_affinity):
    need(cpu["schema"] == "r1.flop-cpu-occupancy/v1" and cpu["case"] == row["case"]
         and cpu["threads"] == row["workers"] and cpu["iterations"] == 16
         and cpu["clock"] == "CLOCK_PROCESS_CPUTIME_ID" and cpu["clock_id"] == 2
         and cpu["scope"] == "all threads in this process" and cpu["performance_claim"] is False, "CPU schema/condition differs")
    need(cpu["cfr_wall_seconds"] == result["cfr_seconds"] and cpu["quality_wall_seconds"] == result["quality_seconds"], "CPU wall copies differ")
    need(cpulist(cpu["cpu_allowed_list"]) == expected_affinity, "observed child affinity differs")
    for key in ("cfr_cpu_seconds", "quality_cpu_seconds", "exploitability_cpu_seconds", "exploitability_wall_seconds"):
        need(math.isfinite(cpu[key]) and cpu[key] >= 0, "invalid CPU observation")
    for key in ("ev_cpu_seconds", "br_cpu_seconds", "ev_wall_seconds", "br_wall_seconds"):
        need(len(cpu[key]) == 2 and all(math.isfinite(x) and x >= 0 for x in cpu[key]), "invalid per-call timing")
    need(all(x > 0 for x in cpu["ev_wall_seconds"] + cpu["br_wall_seconds"] + [cpu["exploitability_wall_seconds"]]), "zero call wall interval")


def complete(out, row, receipt):
    live(receipt["prepared_plan"])
    row.update(status="completed", verified_at=now())
    directory = out / row["name"]
    durable.sync_files([located(p) for p in directory.rglob("*") if p.is_file()])
    row["completion"] = durable.atomic_json(directory / "completed.json", {k: v for k, v in row.items() if k != "completion"}, once=True)
    durable.atomic_json(out / "execution.json", {k: v for k, v in receipt.items() if k != "prepared_plan"})
    print(json.dumps({"stage": row["name"], "status": "completed", "case": row.get("case"), "result": row.get("result")}), flush=True)


def solve(out, plan, row, receipt, canonical):
    binary = receipt["binaries"][row["adapter"]]
    need(pin(binary["path"]) == pair(binary), "binary changed")
    affinity = plan["host"]["one_per_core"] if row["configuration"] == "16-onecore" else plan["host"]["affinity"]
    command = [binary["path"], row["case"], str(row["workers"]), "16", str(out / row["name"] / "artifacts")]
    if row["configuration"] == "16-onecore":
        command = [plan["tools"]["taskset"]["path"], "--cpu-list", ",".join(map(str, affinity)), *command]
    row.update(command=command, expected_child_affinity=affinity)
    directory = stage(out, plan, row, receipt, 90, out, [binary["path"]])
    artifacts = directory / "artifacts"
    names = {"invocation.json", "result.json", "quality.json", "state.bin"} | ({"cpu.json"} if row["adapter"] == "cpu" else set())
    need({p.name for p in artifacts.iterdir()} == names, "artifact membership differs")
    result, invocation, quality = (read(artifacts / n) for n in ("result.json", "invocation.json", "quality.json"))
    validate_values(row, result, invocation, quality)
    row["result"] = result
    if row["kind"] == "canonical":
        need(result["cfr_seconds"] >= 4, "fixed N16 baseline CFR below four seconds; no adaptive iterations")
    if row["adapter"] == "cpu":
        cpu = read(artifacts / "cpu.json")
        validate_cpu(row, cpu, result, affinity)
        row["cpu"] = cpu
    state = common.compare_state(artifacts / "state.bin", (16, 16, *common.HEADERS[row["case"]]),
                                 canonical["state"] if canonical else None, plan["deadline_monotonic"])
    need(state["bytes"] == result["state_bytes"], "state byte count differs")
    row["outputs"] = {name: located(artifacts / name) for name in sorted(names - {"state.bin"})} | {"state.bin": state}
    durable.sync_files(list(row["outputs"].values()))
    if canonical is None:
        raw = Path(plan["workspace"]) / "canonical" / (row["case"] + ".bin")
        archive = durable.gzip_verified(state["path"], out / "canonical" / (row["case"] + ".bin.gz"), state)
        canonical = {"state": {**state, "path": str(raw)}, "state_gzip": archive, "quality": row["outputs"]["quality.json"]}
        kind = "new_canonical"
    else:
        need((artifacts / "quality.json").read_bytes() == Path(canonical["quality"]["path"]).read_bytes(), "canonical quality bytes differ")
        need(pin(canonical["state_gzip"]["path"]) == pair(canonical["state_gzip"]), "canonical archive changed")
        kind = "alias"
    retention = {"kind": kind, "original": state, "canonical": canonical, "fullbyte_verified": True, "raw_removed": False}
    durable.atomic_json(directory / "retention.json", retention, once=True)
    live(plan)
    if kind == "new_canonical":
        os.replace(state["path"], canonical["state"]["path"])
        durable.sync_directory(Path(canonical["state"]["path"]).parent)
    else:
        Path(state["path"]).unlink()
    durable.sync_directory(artifacts)
    retention["raw_removed"] = True
    durable.atomic_json(directory / "retention.json", retention)
    row["state_retention"] = retention
    row["retention_receipt"] = located(directory / "retention.json")
    complete(out, row, receipt)
    return canonical


def finish_manifest(out):
    files = {n: p for n, p in inventory(out).items() if n != "retained.json"}
    need(sum(v["bytes"] for v in files.values()) <= MAX_RETAINED, "retained bound exceeded")
    durable.sync_files([{ "path": str(out / n), **p} for n, p in files.items()])
    durable.atomic_json(out / "retained.json", {"schema": "r1.cpu-occupancy-retained/v1", "files": files}, once=True)


def execute(args):
    out = args.out.resolve(strict=True)
    plan = read(out / "plan.json")
    need(str(out) == plan["output"] and not (out / "execution.json").exists(), "no retry or resume")
    rows = [{"name": "toolchain", "kind": "toolchain", "status": "pending"}, {"name": "build", "kind": "build", "status": "pending"}]
    rows += [{**r, "status": "pending"} for r in schedule()]
    receipt = {"schema": "r1.cpu-occupancy-execution/v1", "status": "running", "plan": pin(out / "plan.json"),
               "started_at": now(), "stages": rows, "binaries": {}, "canonical": {}, "prepared_plan": plan}
    current = None
    try:
        live(plan, 250)
        for key in list(os.environ):
            if key.startswith("CARGO_PROFILE_") or key in {"CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTUP_TOOLCHAIN"}:
                os.environ.pop(key)
        os.environ.update(plan["environment"])
        for current in rows:
            if current["kind"] == "toolchain":
                current["command"] = [plan["tools"]["rustc"]["path"], "-Vv"]
                directory = stage(out, plan, current, receipt, 10, out)
                record = read(directory / "supervisor.json")
                version = Path(record["outputs"]["stdout"]["path"]).read_text()
                need("release: 1.97.0" in version and "host: x86_64-unknown-linux-gnu" in version, "compiler version differs")
                receipt["rustc_version"] = version
                complete(out, current, receipt)
            elif current["kind"] == "build":
                target = Path(plan["workspace"]) / "target"
                need(not target.exists(), "fresh native Cargo target required")
                current["command"] = [plan["tools"]["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2", "--target-dir", str(target),
                                      "-p", "holdem", "--example", EXAMPLES["original"], "--example", EXAMPLES["cpu"], "--message-format=json"]
                stage(out, plan, current, receipt, 240, Path(plan["source"]["path"]))
                for kind, name in EXAMPLES.items():
                    binary = located(target / "release/examples" / name)
                    receipt["binaries"][kind] = binary
                    current.setdefault("retained_binaries", {})[kind] = durable.gzip_verified(binary["path"], out / f"binary-{kind}.gz", binary)
                complete(out, current, receipt)
                durable.atomic_json(out / "build.json", {"status": "completed", "plan": receipt["plan"], "stages": rows[:2], "binaries": receipt["binaries"]}, once=True)
            else:
                canonical = receipt["canonical"].get(current["case"])
                receipt["canonical"][current["case"]] = solve(out, plan, current, receipt, canonical)
        live(plan)
        need(all(r["status"] == "completed" for r in rows), "incomplete fixed schedule")
        receipt["status"] = "completed"
    except BaseException as error:
        receipt.update(status="failed", error=repr(error), traceback=traceback.format_exc())
        if current is not None and current["status"] != "completed":
            current.update(status="failed", error=repr(error))
        for row in rows:
            if row["status"] == "pending":
                row.update(status="skipped", reason="stopped on first failure")
        raise
    finally:
        receipt.pop("prepared_plan", None)
        receipt["ended_at"] = now()
        receipt["counts"] = {s: sum(r["status"] == s for r in rows) for s in ("completed", "failed", "skipped")}
        durable.atomic_json(out / "execution.json", receipt)
        finish_manifest(out)
        print(json.dumps({"status": receipt["status"], "counts": receipt["counts"]}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "execute"))
    parser.add_argument("--out", type=Path, required=True)
    for name in ("source", "workspace", "cargo", "rustc", "taskset"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args()
    if args.phase == "prepare":
        need(all(getattr(args, n) for n in ("source", "workspace", "cargo", "rustc", "taskset", "deadline_utc")), "prepare arguments missing")
        prepare(args)
    else:
        execute(args)


if __name__ == "__main__":
    main()
