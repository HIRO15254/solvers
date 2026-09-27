"""Portable two-CPU build, then one-boot 32-CPU CFR chance-grain screen."""
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
import subprocess
import sys
import tarfile
import time
import traceback
import urllib.request

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
CPU_ADAPTER = HERE / "adapter/solve.rs"
CPU_ADAPTER_SHA = "69d09d308e2d5d12160235dc9fc3fe981acc8f2103809a1fcccaaeed8b97c195"
CASES = ("narrow", "expanded")
ARMS = ("baseline",)
DEPTHS = (2, 1)
WORKERS = (16, 32)
EXAMPLE = "flop_chance_grain_probe"
BUILD_SECONDS = 480
WINDOW_SECONDS = 1200
TEST_NAMES = ("mapped_chance_and_action_siblings_preserve_f32_state", "mapped_chance_and_action_siblings_preserve_i16_state",
              "untrained_empty_and_variable_value_spaces_f32", "untrained_empty_and_variable_value_spaces_i16",
              "parallel_chance_fanout_is_bitwise_deterministic", "selected_value_recording_preserves_ancestor_values_and_storage")
V3_FLAGS = set("cx16 lahf_lm popcnt pni ssse3 sse4_1 sse4_2 avx avx2 bmi1 bmi2 f16c fma abm movbe xsave".split())
ITERATIONS = 16
LIMITS = {"grace_seconds": 0.2, "kill_wait_seconds": 5, "poll_seconds": 0.1,
          "memory_limit_bytes": 8 * 1024**3, "min_free_memory_bytes": 2 * 1024**3,
          "disk_reserve_bytes": 2 * 1024**3}
MAX_RETAINED = 240 * 1024**2


def phase_limits(phase):
    return {**LIMITS, "memory_limit_bytes": (4 if phase == "build" else 8) * 1024**3}


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


def feature_evidence():
    # glibc uses CPU_FEATURE_USABLE (including OS extended-state support), not
    # hardware CPUID alone. The independent kernel flag check is conservative.
    loader = Path("/lib64/ld-linux-x86-64.so.2").resolve(strict=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith("LD_") and k != "GLIBC_TUNABLES"}
    env["LC_ALL"] = "C"
    command = [str(loader), "--help"]
    result = subprocess.run(command, capture_output=True, text=True, env=env, timeout=5, check=True)
    text = Path("/proc/cpuinfo").read_text()
    flags = {}
    for block in text.strip().split("\n\n"):
        entries = dict(line.split(":", 1) for line in block.splitlines() if ":" in line)
        entries = {k.strip(): v.strip() for k, v in entries.items()}
        if "processor" in entries:
            flags[int(entries["processor"])] = sorted(entries.get("flags", "").split())
    affinity = sorted(os.sched_getaffinity(0))
    evidence = {"loader": located(loader), "argv": command, "stdout": result.stdout, "stderr": result.stderr,
                "returncode": result.returncode, "per_cpu_flags": {str(c): flags[c] for c in affinity}}
    validate_features(evidence, affinity)
    return evidence


def validate_features(evidence, affinity):
    need(evidence["returncode"] == 0 and not evidence["stderr"]
         and "x86-64-v3 (supported, searched)" in [x.strip() for x in evidence["stdout"].splitlines()], "glibc usable v3 not established")
    need(set(evidence["per_cpu_flags"]) == set(map(str, affinity)), "per-CPU feature membership differs")
    for flags in evidence["per_cpu_flags"].values():
        need(len(flags) == len(set(flags)) and V3_FLAGS <= set(flags), "required v3 CPU/OS flags missing")


def host(phase):
    need(phase in ("build", "measure") and sys.platform == "linux" and platform.machine() == "x86_64", "Linux x86_64 required")
    count = 2 if phase == "build" else 32
    affinity = sorted(os.sched_getaffinity(0))
    need(os.cpu_count() == count and len(affinity) == count, "phase CPU/affinity count differs")
    topology = []
    for cpu in affinity:
        base = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology")
        topology.append({"cpu": cpu, "core": (base / "core_id").read_text().strip(),
                         "socket": (base / "physical_package_id").read_text().strip()})
    selected = one_per_core(topology, affinity) if phase == "measure" else []
    cg = Path("/sys/fs/cgroup") / Path("/proc/self/cgroup").read_text().strip().split("::", 1)[1].lstrip("/")
    quota = {str(p): (p / "cpu.max").read_text().strip() for p in (cg, *cg.parents)
             if p.is_relative_to("/sys/fs/cgroup") and (p / "cpu.max").is_file()}
    limits = {"memory_max": (cg / "memory.max").read_text().strip(), "swap_max": (cg / "memory.swap.max").read_text().strip(),
              "cpu_weight": (cg / "cpu.weight").read_text().strip(), "cpu_limits": quota}
    need(limits["memory_max"] == str((6 if phase == "build" else 12) * 1024**3) and limits["swap_max"] == "0"
         and limits["cpu_weight"] == "100" and quota, "outer service bounds differ")
    for value in quota.values():
        maximum, period = value.split()
        need(maximum == "max" or int(maximum) / int(period) >= count, "CPU quota below phase count")
    request = urllib.request.Request("http://metadata.google.internal/computeMetadata/v1/instance/id", headers={"Metadata-Flavor": "Google"})
    with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=3) as response:
        instance_id = response.read(64).decode("ascii").strip()
        need(response.headers.get("Metadata-Flavor") == "Google" and instance_id.isdecimal(), "instance identity missing")
    return {"machine": "x86_64", "logical_cpus": count, "instance_id": instance_id,
            "physical_cores": len({(t["socket"], t["core"]) for t in topology}), "affinity": affinity,
            "topology": topology, "one_per_core": selected, "cgroup": limits,
            "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(), "v3": feature_evidence(),
            "cpu_models": sorted(set(x.split(":", 1)[1].strip() for x in Path("/proc/cpuinfo").read_text().splitlines()
                                     if x.startswith("model name")))}


def schedule():
    rows = []
    for workers in (1, 32):
        for depth in DEPTHS:
            rows.append({"name": f"smoke-narrow-w{workers}-d{depth}", "kind": "smoke", "case": "narrow", "arm": "baseline", "depth": depth,
                         "workers": workers, "iterations": 2, "round": None, "warmup": False})
    for case in CASES:
        rows.append({"name": f"canonical-{case}", "kind": "canonical", "case": case, "arm": "baseline", "depth": 2,
                     "workers": 1, "iterations": 16, "round": None, "warmup": False})
    for case in CASES:
        for r in range(4):
            for workers in (WORKERS if r % 2 == 0 else WORKERS[::-1]):
                for depth in (DEPTHS if r % 2 == 0 else DEPTHS[::-1]):
                    rows.append({"name": f"{case}-r{r}-w{workers}-d{depth}", "kind": "matrix", "case": case, "arm": "baseline", "depth": depth,
                                 "workers": workers, "iterations": 16, "round": r, "warmup": r == 0})
    return rows


def canonical_key(row):
    return "smoke-narrow" if row["kind"] == "smoke" else row["case"]


def controls():
    return [Path(__file__), HERE / "analyze.py", HERE / "protocol.jp.md", COMMON, common.SHARED, DURABLE, SUPERVISOR,
            CPU_ADAPTER, HERE / "adapter/prepare.py", HERE / "adapter/provenance.json"]


def environment(tools):
    return {"RUSTC": tools["rustc"]["path"], "RUSTFLAGS": "-C target-cpu=x86-64-v3", "CARGO_BUILD_JOBS": "2",
            "CARGO_INCREMENTAL": "0", "RAYON_NUM_THREADS": "1"}


def source_bindings(originals, sources, adapter):
    need(originals[SOLVER]["sha256"] == SOLVER_SHA and adapter["sha256"] == CPU_ADAPTER_SHA, "baseline/adapter differs")
    example = f"crates/holdem/examples/{EXAMPLE}.rs"
    need(example not in originals and set(sources) == {"baseline"}, "source arms differ")
    need(sources["baseline"] == originals | {example: adapter}, "baseline source differs")
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
    machine = host("build")
    end = dt.datetime.fromisoformat(args.deadline_utc.replace("Z", "+00:00"))
    need(end.utcoffset() == dt.timedelta(0) and 0 < end.timestamp() - time.time() <= WINDOW_SECONDS, "maximum20-minute UTC window required")
    out, workspace = args.out.resolve(), args.workspace.resolve()
    sources = {arm: getattr(args, arm + "_source").resolve(strict=True) for arm in ARMS}
    paths = [out, workspace, *sources.values()]
    need(len(set(paths)) == 3 and all(not a.is_relative_to(b) for a in paths for b in paths if a != b), "owned paths overlap")
    need(not out.exists() and not workspace.exists(), "fresh output/workspace required")
    manifest_path, installation_path = ROOT / "manifest.json", ROOT / "installation.json"
    manifest, installation = read(manifest_path), read(installation_path)
    need(manifest["schema"] == "r1-chance-grain-package/v1", "package schema differs")
    for name, value in manifest["files"].items():
        p = ROOT / relative(name)
        need(p.is_file() and not p.is_symlink() and p.resolve().is_relative_to(ROOT) and pin(p) == value, "installed package changed")
    need(installation["manifest"] == pin(manifest_path) and installation["destination"] == str(ROOT)
         and installation["source_revision"] == manifest["source_revision"]
         and installation["source_files"] == len(manifest["source_pins"])
         and installation["builds_or_solves_started"] == 0, "installation binding differs")
    need(len(installation["archive_sha256"]) == 64 and all(c in "0123456789abcdef" for c in installation["archive_sha256"]), "package archive identity missing")
    files = {arm: inventory(source) for arm, source in sources.items()}
    source_bindings(manifest["source_pins"], files, pin(CPU_ADAPTER))
    need({n.removeprefix("source/"): v for n, v in manifest["files"].items() if n.startswith("source/")}
         == manifest["source_pins"], "packaged original source binding differs")
    for p in controls():
        need(manifest["files"].get(p.relative_to(ROOT).as_posix()) == pin(p), "control not package-pinned")
    for p, sha in PINS.items():
        need(pin(p)["sha256"] == sha, "frozen helper changed")
    tools = {n: located(getattr(args, n).resolve(strict=True)) for n in ("cargo", "rustc")}
    out.mkdir(parents=True)
    workspace.mkdir(parents=True)
    for p in (out / "inputs", out / "canonical", workspace / "canonical"):
        p.mkdir()
    captured = {}
    for p in [*controls(), manifest_path, installation_path]:
        value = pin(p)
        retained = out / "inputs" / value["sha256"]
        if not retained.exists():
            shutil.copyfile(p, retained)
        captured[str(p)] = {**value, "retained": retained.relative_to(out).as_posix()}
    source_records = {}
    for arm, source in sources.items():
        archive = out / ("source-" + arm + ".tar.gz")
        archive_source(source, files[arm], archive)
        source_records[arm] = {"path": str(source), "files": files[arm], "archive": located(archive)}
    plan = {"schema": "r1.chance-grain/v1", "output": str(out), "workspace": str(workspace), "created_at": now(),
            "deadline_utc": args.deadline_utc, "deadline_monotonic": time.monotonic() + end.timestamp() - time.time(),
            "host": machine, "phase": "build", "plan_file": "plan.json", "sources": source_records,
            "controls": captured, "tools": tools, "environment": environment(tools), "limits": phase_limits("build"),
            "schedule": schedule(), "package": {"manifest": pin(manifest_path), "archive_sha256": installation["archive_sha256"]}}
    durable.sync_files([located(p) for p in out.rglob("*") if p.is_file()])
    durable.atomic_json(out / "plan.json", plan, once=True)
    live(plan)
    print(json.dumps({"status": "prepared", "solves": 38, "matrix_rows": 32}), flush=True)

def live(plan, reserve=0):
    common.deadline_check(plan["deadline_monotonic"], reserve)
    need(time.time() + reserve < dt.datetime.fromisoformat(plan["deadline_utc"]).timestamp(), "UTC deadline reached")
    need(host(plan["phase"]) == plan["host"] and plan["schedule"] == schedule() and plan["limits"] == phase_limits(plan["phase"]), "host or protocol changed")
    need(plan["environment"] == environment(plan["tools"]), "environment changed")
    for path, value in plan["controls"].items():
        need(pin(path, plan["deadline_monotonic"]) == pair(value), "control changed")
    for source in plan["sources"].values():
        need(inventory(Path(source["path"]), plan["deadline_monotonic"]) == source["files"], "source changed")
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
    need(pin(out / plan["plan_file"]) == receipt["plan"], "prepared plan changed")
    need(sum(p.stat().st_size for p in out.rglob("*") if p.is_file()) <= MAX_RETAINED, "proof retention bound exceeded")
    directory = out / row["name"]
    directory.mkdir()
    row.update(status="running", started_at=now(), host_before=host(plan["phase"]))
    row["environment"] = {k: os.environ.get(k) for k in plan["environment"]}
    row["forbidden_environment"] = sorted(k for k in os.environ if forbidden_environment(k))
    need(row["environment"] == plan["environment"] and not row["forbidden_environment"], "actual stage build environment differs")
    args = ["--record", str(directory / "supervisor.json"), "--cwd", str(cwd), "--disk-path", str(out), "--timeout-seconds", str(seconds)]
    for key, value in plan["limits"].items():
        args += ["--" + key.replace("_", "-"), str(value)]
    identities = [out / plan["plan_file"], Path(__file__), SUPERVISOR, Path(row["command"][0]), *map(Path, binaries)]
    for p in dict.fromkeys(identities):
        args += ["--identity-file", str(p)]
    row["supervisor_argv"] = args + ["--", *row["command"]]
    persist(out, receipt)
    try:
        row["supervisor_exit"] = load("occupancy_supervisor", SUPERVISOR, PINS[SUPERVISOR]).main(row["supervisor_argv"])
    finally:
        row["ended_at"] = now()
        if (directory / "supervisor.json").exists():
            row["record"] = located(directory / "supervisor.json")
        persist(out, receipt)
    need(row["supervisor_exit"] == 0, "bounded process failed")
    record = read(directory / "supervisor.json")
    terminal(record)
    for value in raw_outputs(record, directory):
        need(pin(value["path"]) == pair(value), "supervisor output changed")
    row.update(process_seconds=record["elapsed_seconds"], root_os_peak_resident_bytes=record["last_sample"]["root_os_peak_resident_bytes"],
               root_os_peak_source=record["last_sample"]["root_os_peak_source"], host_after=host(plan["phase"]))
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
             and value["threads"] == row["workers"] and value["iterations"] == row["iterations"], "condition differs")
    need(result["performance_claim"] is False and result["cfv_capture"] is False
         and result["state_file"] == "state.bin" and result["quality_file"] == "quality.json", "result scope differs")
    need(result["status"] == "completed" and invocation["planned_iterations"] == row["iterations"]
         and invocation["storage"] == "f32" and invocation["schedule"] == "dcfr"
         and invocation["chance_depth"] == row["depth"] and invocation["quality_chance_depth"] == 2 and invocation["min_children"] == 12
         and invocation["alpha"] == 1.5 and invocation["beta"] == 0 and invocation["gamma"] == 3
         and invocation["pow4_reset"] is True and invocation["quality_target"] is None and invocation["cfv_capture"] is False, "solver semantics differ")
    need(quality["schema"] == "r1.flop-native-quality/v1" and quality["cfv_capture"] is False
         and quality["case"] == row["case"] and quality["iterations"] == row["iterations"]
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
         and cpu["threads"] == row["workers"] and cpu["iterations"] == row["iterations"]
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
    persist(out, receipt)
    print(json.dumps({"stage": row["name"], "status": "completed", "case": row.get("case"), "result": row.get("result")}), flush=True)


def solve(out, plan, row, receipt, canonical):
    binary = receipt["binaries"][row["arm"]]
    need(pin(binary["path"]) == pair(binary), "binary changed")
    affinity = plan["host"]["affinity"]
    command = [binary["path"], row["case"], str(row["workers"]), str(row["iterations"]), str(row["depth"]), str(out / row["name"] / "artifacts")]
    row.update(command=command, expected_child_affinity=affinity)
    directory = stage(out, plan, row, receipt, 90, out, [binary["path"]])
    artifacts = directory / "artifacts"
    names = {"invocation.json", "result.json", "quality.json", "state.bin"} | {"cpu.json", "grain.json"}
    need({p.name for p in artifacts.iterdir()} == names, "artifact membership differs")
    result, invocation, quality = (read(artifacts / n) for n in ("result.json", "invocation.json", "quality.json"))
    validate_values(row, result, invocation, quality)
    row["result"] = result
    if row["kind"] == "canonical":
        need(result["cfr_seconds"] >= 4, "fixed N16 baseline CFR below four seconds; no adaptive iterations")
    cpu = read(artifacts / "cpu.json")
    validate_cpu(row, cpu, result, affinity)
    row["cpu"] = cpu
    row["grain"] = read(artifacts / "grain.json")
    validate_grain(row, row["grain"])
    state = common.compare_state(artifacts / "state.bin", (row["iterations"], row["iterations"], *common.HEADERS[row["case"]]),
                                 canonical["state"] if canonical else None, plan["deadline_monotonic"])
    need(state["bytes"] == result["state_bytes"], "state byte count differs")
    row["outputs"] = {name: located(artifacts / name) for name in sorted(names - {"state.bin"})} | {"state.bin": state}
    durable.sync_files(list(row["outputs"].values()))
    if canonical is None:
        raw = Path(plan["workspace"]) / "canonical" / (canonical_key(row) + ".bin")
        archive = durable.gzip_verified(state["path"], out / "canonical" / (canonical_key(row) + ".bin.gz"), state)
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
    durable.atomic_json(out / "retained.json", {"schema": "r1.chance-grain-retained/v1", "files": files}, once=True)


def preserve_failed_state(out, plan, row, receipt):
    """Optional lossless compression, never state equality or a successful row."""
    if row is None:
        return
    state = out / row["name"] / "artifacts/state.bin"
    if not state.exists():
        return
    try:
        common.deadline_check(plan["deadline_monotonic"], 30)
        need(time.time() + 30 < dt.datetime.fromisoformat(plan["deadline_utc"]).timestamp(), "no failed-state compression time")
        need(state.is_file() and not state.is_symlink(), "failed state is not a regular file")
        original = located(state, plan["deadline_monotonic"])
        compressed = durable.gzip_verified(state, state.with_suffix(".failed.gz"), original)
        record = {"schema": "r1.chance-grain-failed-state-capture/v1", "original": original, "gzip": compressed,
                  "fullbyte_compression_verified": True, "canonical_equality_verified": False,
                  "raw_removed": False}
        path = state.parent.parent / "failed-state-capture.json"
        durable.atomic_json(path, record, once=True)
        common.deadline_check(plan["deadline_monotonic"])
        state.unlink()
        durable.sync_directory(state.parent)
        record["raw_removed"] = True
        durable.atomic_json(path, record)
        receipt["failed_state_capture"] = located(path)
    except BaseException as error:
        # Never retry or remove raw bytes after a failed gzip/publication. If
        # killed here, outer recovery retains every visible original/gzip file.
        receipt["failed_state_capture_error"] = repr(error)


def persist(out, receipt):
    durable.atomic_json(out / receipt["receipt_file"], {k: v for k, v in receipt.items() if k != "prepared_plan"})


def validate_grain(row, grain):
    need(grain["schema"] == "r1.flop-chance-grain-observation/v1" and grain["case"] == row["case"]
         and grain["cfr_depth"] == row["depth"] and grain["quality_depth"] == 2 and grain["min_children"] == 12,
         "grain observation condition differs")
    need(grain["counts_scope"] == "one structural root traversal; not tasks, iterations or seat passes", "frontier count scope differs")
    for key in ("eligible_chance_nodes_by_depth", "eligible_child_edges_by_depth"):
        v = grain[key]
        need(len(v) == 2 and all(type(x) is int and x > 0 for x in v) and v[0] < v[1], "frontier shape differs")
    need(grain["eligible_chance_nodes_by_depth"][1] <= 1034, "frontier exceeds all chance nodes")


def clean_environment(plan):
    for key in list(os.environ):
        if forbidden_environment(key):
            os.environ.pop(key)
    os.environ.update(plan["environment"])


def forbidden_environment(key):
    return key.startswith("CARGO_PROFILE_") or key in {"CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTUP_TOOLCHAIN", "CARGO_BUILD_TARGET"}


def measure_prepare(args):
    out = args.out.resolve(strict=True)
    original = read(out / "plan.json")
    built, execution = read(out / "build.json"), read(out / "build-execution.json")
    need(built["status"] == execution["status"] == "completed" and built["plan"] == pin(out / "plan.json")
         and built["execution"] == pin(out / "build-execution.json"), "immutable successful build required")
    need(built["stages"] == execution["stages"] and built["binaries"] == execution["binaries"], "build rows differ")
    # Immutable build outputs are rehashed before crossing the intentional reboot.
    need(inventory_subset(out, built["files"]) == built["files"], "build evidence changed across reboot")
    machine = host("measure")
    need(machine["boot_id"] != original["host"]["boot_id"], "separate resized measurement boot required")
    need(machine["instance_id"] == original["host"]["instance_id"], "instance changed across resize")
    end = dt.datetime.fromisoformat(args.deadline_utc.replace("Z", "+00:00"))
    need(end.utcoffset() == dt.timedelta(0) and 600 < end.timestamp() - time.time() <= WINDOW_SECONDS, "10..20-minute measurement window required")
    plan = {**original, "phase": "measure", "plan_file": "measurement.json", "host": machine, "limits": phase_limits("measure"),
            "created_at": now(), "deadline_utc": args.deadline_utc,
            "deadline_monotonic": time.monotonic() + end.timestamp() - time.time(),
            "build_plan": pin(out / "plan.json"), "build_receipt": pin(out / "build.json"),
            "build_execution": pin(out / "build-execution.json")}
    for binary in built["binaries"].values():
        need(pin(binary["path"]) == pair(binary), "prebuilt portable binary differs")
    need(not (out / "measurement.json").exists() and not (out / "execution.json").exists(), "no measurement retry/resume")
    durable.atomic_json(out / "measurement.json", plan, once=True)
    live(plan)
    print(json.dumps({"status": "measurement_prepared", "build_boot": original["host"]["boot_id"], "measurement_boot": machine["boot_id"]}), flush=True)


def inventory_subset(out, files):
    return {name: pin(out / relative(name)) for name in files}


def build_rows():
    return [{"name": "toolchain", "kind": "toolchain"},
            {"name": "build-baseline", "kind": "build", "arm": "baseline"},
            {"name": "tests-baseline", "kind": "tests", "arm": "baseline"}]


def execute(args):
    out = args.out.resolve(strict=True)
    building = args.phase == "build"
    plan = read(out / ("plan.json" if building else "measurement.json"))
    receipt_file = "build-execution.json" if building else "execution.json"
    need(plan["phase"] == ("build" if building else "measure") and str(out) == plan["output"] and not (out / receipt_file).exists(), "no retry or resume")
    rows = [{**r, "status": "pending"} for r in (build_rows() if building else schedule())]
    built = None if building else read(out / "build.json")
    if built is not None:
        need(pin(out / "build.json") == plan["build_receipt"] and pin(out / "build-execution.json") == plan["build_execution"]
             and inventory_subset(out, built["files"]) == built["files"], "immutable build changed")
    receipt = {"schema": "r1.chance-grain-execution/v1", "phase": plan["phase"], "receipt_file": receipt_file,
               "status": "running", "plan": pin(out / plan["plan_file"]), "started_at": now(), "stages": rows,
               "binaries": {} if building else built["binaries"], "canonical": {}, "prepared_plan": plan}
    current = None
    try:
        live(plan, BUILD_SECONDS + 10 if building else 100)
        clean_environment(plan)
        for current in rows:
            if current["kind"] == "toolchain":
                current["command"] = [plan["tools"]["rustc"]["path"], "-Vv"]
                directory = stage(out, plan, current, receipt, 10, out)
                record = read(directory / "supervisor.json")
                version = Path(record["outputs"]["stdout"]["path"]).read_text()
                need("release: 1.97.0" in version and "host: x86_64-unknown-linux-gnu" in version, "compiler version differs")
                receipt["rustc_version"] = version
                complete(out, current, receipt)
            elif current["kind"] in {"build", "tests"}:
                target = Path(plan["workspace"]) / "baseline-target"
                if current["kind"] == "build":
                    need(not target.exists(), "fresh portable Cargo target required")
                    current["command"] = [plan["tools"]["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2", "--target-dir", str(target),
                                          "-p", "holdem", "--example", EXAMPLE, "--message-format=json"]
                else:
                    current["command"] = test_command(plan)
                directory = stage(out, plan, current, receipt, BUILD_SECONDS, Path(plan["sources"]["baseline"]["path"]))
                if current["kind"] == "build":
                    binary = located(target / "release/examples" / EXAMPLE)
                    receipt["binaries"]["baseline"] = binary
                    current["retained_binary"] = durable.gzip_verified(binary["path"], out / "binary-baseline.gz", binary)
                else:
                    record = read(directory / "supervisor.json")
                    validate_tests(Path(record["outputs"]["stdout"]["path"]).read_text())
                complete(out, current, receipt)
            else:
                key = canonical_key(current)
                receipt["canonical"][key] = solve(out, plan, current, receipt, receipt["canonical"].get(key))
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
        preserve_failed_state(out, plan, current, receipt)
        raise
    finally:
        receipt.pop("prepared_plan", None)
        receipt["ended_at"] = now()
        receipt["counts"] = {s: sum(r["status"] == s for r in rows) for s in ("completed", "failed", "skipped")}
        persist(out, receipt)
        if building and receipt["status"] == "completed":
            files = inventory(out)
            durable.sync_files([{ "path": str(out / n), **p} for n, p in files.items()])
            durable.atomic_json(out / "build.json", {"schema": "r1.chance-grain-build/v1", "status": "completed",
                "plan": receipt["plan"], "execution": pin(out / receipt_file), "stages": rows, "binaries": receipt["binaries"], "files": files}, once=True)
        elif not building or receipt["status"] == "failed":
            try:
                finish_manifest(out)
            except BaseException as error:
                receipt.update(status="failed", retention_manifest_error=repr(error))
                persist(out, receipt)
                if "error" not in receipt:
                    raise
        print(json.dumps({"status": receipt["status"], "phase": plan["phase"], "counts": receipt["counts"]}), flush=True)


def test_command(plan):
    return [plan["tools"]["cargo"]["path"], "test", "--locked", "--offline", "--release", "-j2",
            "--target-dir", str(PurePosixPath(plan["workspace"]) / "baseline-target"),
            "-p", "engine", "-p", "holdem", "-p", "cfr-ref", "--tests", "--", "--test-threads=1"]


def validate_tests(stdout):
    # These existing regressions bind mapped chance budgets and read-only value
    # storage behavior. Nonzero process exit already rejects every other failure.
    for name in TEST_NAMES:
        need(stdout.splitlines().count("test " + name + " ... ok") == 1, "required core regression missing: " + name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "build", "measure-prepare", "measure"))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--source", "--baseline-source", dest="baseline_source", type=Path)
    for name in ("workspace", "cargo", "rustc"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--deadline-utc", "--build-deadline-utc", "--measurement-deadline-utc", dest="deadline_utc")
    args = parser.parse_args()
    if args.phase == "prepare":
        need(all(getattr(args, n) for n in ("baseline_source", "workspace", "cargo", "rustc", "deadline_utc")), "prepare arguments missing")
        prepare(args)
    elif args.phase == "measure-prepare":
        need(args.deadline_utc, "measurement deadline missing")
        measure_prepare(args)
    else:
        execute(args)

if __name__ == "__main__":
    main()
