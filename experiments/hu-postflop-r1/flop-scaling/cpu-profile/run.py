"""Finite baseline-only software CPU sampling; no optimization/performance screen."""
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
import time
import traceback

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
BASE = HERE.parent / "chance-grain/run.py"
BASE_SHA = "143a89ace93b169787b34f0c26f4683b1fde643d9fbe5553e6120cd92f8e2b79"
assert hashlib.sha256(BASE.read_bytes()).hexdigest() == BASE_SHA, "trusted base changed"
spec = importlib.util.spec_from_file_location("cpu_profile_base", BASE)
base = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = base
spec.loader.exec_module(base)
need, read, pin, pair, located, relative = (getattr(base, n) for n in ("need", "read", "pin", "pair", "located", "relative"))
common, durable, SUPERVISOR, PINS = base.common, base.durable, base.SUPERVISOR, base.PINS
CASES, ARMS, WORKERS = ("narrow", "expanded"), ("baseline",), (16, 32)
ITERATIONS, BUILD_SECONDS, WINDOW_SECONDS = 64, 480, 1200
MEASURE_SECONDS, STOP_SECONDS, RECOVERY_SECONDS = 900, 2700, 900
MAX_RETAINED, PERF_MAX = 240 * 1024**2, 16 * 1024**2
EXAMPLE = "flop_cpu_profile_probe"
CPU_ADAPTER = HERE / "adapter/solve.rs"
CPU_ADAPTER_SHA = "b1ffa4f46a79a6c167f3118cd32602fa19d8222c302f3c48c682705ad0e14301"
SOLVER, SOLVER_SHA = base.SOLVER, base.SOLVER_SHA
TEST_NAMES = base.TEST_NAMES
PHASES = ("cfr", "state_write", "quality", "ev_p0", "ev_p1", "br_p0", "br_p1", "exploitability")
PERF_FIELDS = "pid,tid,time,period,event,ip,sym,dso"


def utc(text):
    value = dt.datetime.fromisoformat(text.replace("Z", "+00:00"))
    need(value.utcoffset() == dt.timedelta(0), "UTC timestamp required")
    return value


def bounds(launch, stop, deadline, phase, current):
    launch, stop, deadline, current = map(utc, (launch, stop, deadline, current))
    need(STOP_SECONDS - 1 < (stop - launch).total_seconds() <= STOP_SECONDS, "STOP must be launch request +45 minutes, floored to second")
    need(launch <= current < deadline, "invalid phase clock")
    if phase == "build":
        need(deadline <= launch + dt.timedelta(seconds=WINDOW_SECONDS), "build exceeds launch +20 minutes")
    else:
        need(current <= launch + dt.timedelta(seconds=900), "measurement dispatch after launch +15 minutes")
        need(600 < (deadline - current).total_seconds() <= MEASURE_SECONDS, "measurement requires >10..15 minutes")
        need(deadline <= stop - dt.timedelta(seconds=RECOVERY_SECONDS), "recovery margin reduced")
    return deadline


def schedule():
    rows = [{"name": f"canonical-{c}", "kind": "canonical", "case": c, "arm": "baseline", "depth": 2,
             "workers": 1, "iterations": ITERATIONS, "round": None, "warmup": False} for c in CASES]
    for case in CASES:
        for r in range(2):
            for workers in (WORKERS if r == 0 else WORKERS[::-1]):
                rows.append({"name": f"{case}-r{r}-w{workers}", "kind": "profile", "case": case, "arm": "baseline",
                             "depth": 2, "workers": workers, "iterations": ITERATIONS, "round": r, "warmup": False})
    return rows


def controls():
    return [Path(__file__), HERE / "analyze.py", HERE / "protocol.jp.md", BASE,
            HERE.parent / "chance-grain/analyze.py", base.COMMON, common.SHARED, base.DURABLE, SUPERVISOR,
            CPU_ADAPTER, HERE / "adapter/prepare.py", HERE / "adapter/provenance.json"]


def environment(tools):
    return {"RUSTC": tools["rustc"]["path"],
            "RUSTFLAGS": "-C target-cpu=x86-64-v3 -C force-frame-pointers=yes -C debuginfo=line-tables-only",
            "CARGO_BUILD_JOBS": "2", "CARGO_INCREMENTAL": "0", "RAYON_NUM_THREADS": "1", "LC_ALL": "C"}


def inventory(root, deadline=None):
    result = {}
    for p in sorted(root.rglob("*"), key=lambda p: p.relative_to(root).parts):
        need(not p.is_symlink(), "symlink forbidden")
        if p.is_file():
            result[p.relative_to(root).as_posix()] = pin(p, deadline)
        else:
            need(p.is_dir(), "nonregular member")
    return result


def archive_source(source, files, destination):
    ordered = sorted(files, key=lambda n: PurePosixPath(n).parts)
    with destination.open("xb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w|") as tar:
            for name in ordered:
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = files[name]["bytes"], 0o644, 0
                with (source / name).open("rb") as stream:
                    tar.addfile(info, stream)
    need(destination.stat().st_size < 1024**2, "source archive bound exceeded")


def prepare(args):
    machine = base.host("build")
    created = base.now()
    end = bounds(args.launch_attempted_at, args.stop_deadline_utc, args.deadline_utc, "build", created)
    out, workspace, source = args.out.resolve(), args.workspace.resolve(), args.baseline_source.resolve(strict=True)
    paths = [out, workspace, source]
    need(len(set(paths)) == 3 and all(not a.is_relative_to(b) for a in paths for b in paths if a != b), "owned paths overlap")
    need(not out.exists() and not workspace.exists(), "fresh output/workspace required")
    mp, ip = ROOT / "manifest.json", ROOT / "installation.json"
    manifest, installed = read(mp), read(ip)
    need(manifest["schema"] == "r1-cpu-profile-package/v1", "package schema differs")
    for name, value in manifest["files"].items():
        p = ROOT / relative(name)
        need(p.is_file() and not p.is_symlink() and p.resolve().is_relative_to(ROOT) and pin(p) == value, "installed package changed")
    need(installed["manifest"] == pin(mp) and installed["destination"] == str(ROOT)
         and installed["source_revision"] == manifest["source_revision"]
         and installed["source_files"] == len(manifest["source_pins"])
         and installed["builds_or_solves_started"] == 0, "installation differs")
    need(re.fullmatch("[0-9a-f]{64}", installed["archive_sha256"]) is not None, "archive SHA invalid")
    files = inventory(source)
    base.source_bindings(manifest["source_pins"], {"baseline": files}, pin(CPU_ADAPTER))
    need({n.removeprefix("source/"): v for n, v in manifest["files"].items() if n.startswith("source/")}
         == manifest["source_pins"], "packaged source differs")
    for p in controls():
        need(manifest["files"].get(p.relative_to(ROOT).as_posix()) == pin(p), "control not package-pinned")
    tools = {n: located(getattr(args, n).resolve(strict=True)) for n in ("cargo", "rustc", "perf")}
    tools["python"] = located(Path(sys.executable).resolve(strict=True))
    out.mkdir(parents=True)
    workspace.mkdir(parents=True)
    for p in (out / "inputs", out / "canonical", workspace / "canonical"):
        p.mkdir()
    captured = {}
    for p in [*controls(), mp, ip]:
        value = pin(p)
        retained = out / "inputs" / value["sha256"]
        if not retained.exists():
            shutil.copyfile(p, retained)
        captured[str(p)] = {**value, "retained": retained.relative_to(out).as_posix()}
    archive = out / "source-baseline.tar.gz"
    archive_source(source, files, archive)
    plan = {"schema": "r1.cpu-profile/v1", "output": str(out), "workspace": str(workspace), "created_at": created,
            "deadline_utc": args.deadline_utc, "deadline_monotonic": time.monotonic() + end.timestamp() - time.time(),
            "launch_attempted_at": args.launch_attempted_at, "stop_deadline_utc": args.stop_deadline_utc,
            "host": machine, "phase": "build", "plan_file": "plan.json", "sources": {"baseline": {"path": str(source), "files": files, "archive": located(archive)}},
            "controls": captured, "tools": tools, "environment": environment(tools), "limits": base.phase_limits("build"),
            "schedule": schedule(), "package": {"manifest": pin(mp), "archive_sha256": installed["archive_sha256"]}}
    durable.sync_files([located(p) for p in out.rglob("*") if p.is_file()])
    durable.atomic_json(out / "plan.json", plan, once=True)
    base.live(plan)
    print(json.dumps({"status": "prepared", "solves": 10, "profiles": 8}), flush=True)


def perf_command(perf, data, child):
    return [perf, "record", "-e", "cpu-clock", "-F", "97", "--strict-freq", "--clockid", "mono",
            "--call-graph", "fp,32", "--no-buildid-cache", "--max-size", "16M", "-o", str(data), "--", *child]


def script_command(perf, data):
    return [perf, "script", "--ns", "--show-lost-events", "-F", PERF_FIELDS, "-i", str(data)]


def capture(command, directory, name, seconds=15, max_bytes=8 * 1024**2, allowed=(0,)):
    """No new process group: the outer supervisor/cgroup owns every subprocess."""
    stdout, stderr = directory / (name + ".stdout.log"), directory / (name + ".stderr.log")
    with stdout.open("xb") as so, stderr.open("xb") as se:
        process = subprocess.Popen(command, stdout=so, stderr=se)
        end = time.monotonic() + seconds
        while process.poll() is None:
            if time.monotonic() >= end or stdout.stat().st_size + stderr.stat().st_size > max_bytes:
                process.kill()
                process.wait(timeout=5)
                raise ValueError("bounded perf helper timed out or exceeded output cap")
            time.sleep(.02)
        code = process.wait()
    row = {"argv": command, "returncode": code, "stdout": located(stdout), "stderr": located(stderr)}
    durable.atomic_json(directory / (name + ".json"), row, once=True)
    need(code in allowed and stdout.stat().st_size + stderr.stat().st_size <= max_bytes, "perf helper failed")
    return row


# Deliberately narrow text grammar. The actual installed perf must demonstrate
# this grammar in the software preflight before Cargo build/tests or solves.
HEADER = re.compile(r"^\s*(\d+)\s*/\s*(\d+)\s+(\d+)\.(\d{9}):\s+(\d+)\s+cpu-clock:\s*(?:([0-9a-fA-F]+)\s+(.+)\s+\((.*)\))?\s*$")
FRAME = re.compile(r"^\s+([0-9a-fA-F]+)\s+(.+)\s+\((.*)\)\s*$")


def parse_samples(text, phases=None):
    samples, current = [], None
    for line in text.splitlines():
        if not line.strip():
            current = None
            continue
        m = HEADER.fullmatch(line)
        if m:
            pid, tid, sec, ns, period, ip, sym, dso = m.groups()
            current = {"pid": int(pid), "tid": int(tid), "ns": int(sec) * 10**9 + int(ns), "period": int(period),
                       "frames": [] if ip is None else [{"ip": ip, "symbol": sym.strip(), "dso": dso}]}
            need(current["period"] > 0, "zero sample period")
            samples.append(current)
        else:
            m = FRAME.fullmatch(line)
            need(current is not None and m is not None, "unrecognized perf sample/LOST structure")
            ip, sym, dso = m.groups()
            current["frames"].append({"ip": ip, "symbol": sym.strip(), "dso": dso})
    need(samples, "no software samples")
    need(all(1 <= len(s["frames"]) <= 33 for s in samples), "empty or unexpected callchain length")
    selected = samples if phases is None else [s for s in samples if s["pid"] == phases["pid"]
                  and phases["phases"]["cfr"]["start_ns"] <= s["ns"] < phases["phases"]["cfr"]["end_ns"]]
    need(selected, "no samples within CFR interval")
    leaves, leaf_periods = {}, {}
    for s in selected:
        frame = s["frames"][0]
        key = frame["symbol"] + " (" + frame["dso"] + ")"
        leaves[key] = leaves.get(key, 0) + 1
        leaf_periods[key] = leaf_periods.get(key, 0) + s["period"]
    unknown = lambda f: f["symbol"] in {"[unknown]", "unknown", "0x0"} or f["dso"] in {"[unknown]", "unknown"}
    if phases is not None:
        own = [s for s in samples if s["pid"] == phases["pid"]]
        need(min(s["ns"] for s in own) < phases["phases"]["cfr"]["start_ns"]
             and max(s["ns"] for s in own) >= phases["phases"]["cfr"]["end_ns"], "sample stream does not bracket CFR")
    return {"all_samples": len(samples), "cfr_samples": len(selected), "cfr_period_sum": sum(s["period"] for s in selected),
            "unknown_leaf_samples": sum(unknown(s["frames"][0]) for s in selected),
            "unknown_any_frame_samples": sum(any(unknown(f) for f in s["frames"]) for s in selected),
            "at_callchain_limit_samples": sum(len(s["frames"]) >= 32 for s in selected),
            "tid_count": len({s["tid"] for s in selected}), "leaf_sample_counts": dict(sorted(leaves.items())),
            "leaf_period_sums": dict(sorted(leaf_periods.items())),
            "truncation_interpretation": "at limit is possible truncation; fp termination before limit can also be incomplete",
            "lost_samples": None, "throttle_events": None, "lost_throttle_note": "read raw perf dump record census separately"}


def record_census(text):
    # session.c dump_event prints offset [size]: RECORD; a timestamp may
    # precede the offset. Bare names in symbols/body/footer are not records.
    kinds = re.findall(r"\b(?:0x[0-9a-fA-F]+|0)\s+\[(?:0x[0-9a-fA-F]+|0)\]:\s+(PERF_RECORD_[A-Z0-9_]+)\b", text)
    need(kinds and "PERF_RECORD_SAMPLE" in kinds, "unrecognized perf dump record grammar")
    counts = {k: kinds.count(k) for k in sorted(set(kinds))}
    return {"record_counts": counts,
            "loss_or_throttle_records_present": any(counts.get(k, 0) for k in ("PERF_RECORD_LOST", "PERF_RECORD_LOST_SAMPLES", "PERF_RECORD_THROTTLE", "PERF_RECORD_UNTHROTTLE")),
            "counts_are_record_occurrences_not_lost_sample_cardinality": True}


def perf_read(perf, data, directory, phases=None):
    need(0 < data.stat().st_size <= PERF_MAX, "perf file bound exceeded")
    scripted = capture(script_command(perf, data), directory, "script", seconds=20, max_bytes=64 * 1024**2)
    dumped = capture([perf, "script", "-D", "-i", str(data)], directory, "dump", seconds=20, max_bytes=64 * 1024**2)
    summary = parse_samples(Path(scripted["stdout"]["path"]).read_text(), phases)
    census = record_census(Path(dumped["stdout"]["path"]).read_text())
    need(census["record_counts"]["PERF_RECORD_SAMPLE"] == summary["all_samples"], "sample text/dump census differs")
    attributes = capture([perf, "evlist", "-v", "-i", str(data)], directory, "evlist")
    validate_attributes(Path(attributes["stdout"]["path"]).read_text())
    need(not Path(attributes["stderr"]["path"]).read_text().strip(), "perf evlist diagnostics require manual review")
    need(not Path(scripted["stderr"]["path"]).read_text().strip(), "perf script diagnostics require manual review")
    need(not census["loss_or_throttle_records_present"], "lost/throttled sampling is not evaluable")
    # Only after successful parsing, preserve exact human-readable outputs in
    # gzip. A parser failure leaves both original plaintexts for recovery.
    compressed = {}
    for name, record in (("script", scripted), ("dump", dumped)):
        original = record["stdout"]
        compressed[name] = durable.gzip_verified(original["path"], directory / (name + ".stdout.log.gz"), original)
        Path(original["path"]).unlink()
    result = {"schema": "r1.cpu-profile-perf/v1", "data": located(data), "script": scripted, "dump": dumped,
              "script_stdout_gzip": compressed["script"], "dump_stdout_gzip": compressed["dump"],
              "attributes": attributes, "summary": summary, "census": census}
    durable.atomic_json(directory / "perf.json", result, once=True)
    return result


def validate_attributes(text):
    for field, value in (("use_clockid", 1), ("clockid", 1), ("freq", 1)):
        need(re.search(r"\b" + field + r"\s*:\s*" + str(value) + r"\b", text) is not None, "perf attr missing/different: " + field)
    need(re.search(r"(?:\bsample_freq|\{\s*sample_period,\s*sample_freq\s*\})\s*:\s*97\b", text) is not None,
         "perf sample frequency/union value differs")
    need("cpu-clock" in text and "CALLCHAIN" in text, "software event/callchain attr missing")


def perf_preflight(args):
    out = args.out.resolve(strict=True)
    perf = str(args.perf.resolve(strict=True))
    version = capture([perf, "--version"], out, "version")
    help_record = capture([perf, "record", "-h"], out, "record-help", allowed=(0, 129))
    help_script = capture([perf, "script", "-h"], out, "script-help", allowed=(0, 129))
    help_text = "\n".join(Path(v[k]["path"]).read_text() for v in (help_record, help_script) for k in ("stdout", "stderr"))
    for name in ("--strict-freq", "--clockid", "--call-graph", "--no-buildid-cache", "--max-size", "--ns", "--show-lost-events"):
        need(name in help_text, "required installed perf option missing: " + name)
    data = out / "preflight.data"
    command = perf_command(perf, data, [sys.executable, str(Path(__file__).resolve()), "perf-child", "--out", str(out)])
    recorded = capture(command, out, "record", seconds=8)
    observed = perf_read(perf, data, out)
    need(observed["summary"]["all_samples"] >= 10, "software preflight too few samples")
    clock = read(out / "clock.json")
    need(clock["clock"] == "CLOCK_MONOTONIC" and clock["start_ns"] < clock["end_ns"], "preflight clock missing")
    selected = 0
    with gzip.open(observed["script_stdout_gzip"]["path"], "rt") as stream:
        for line in stream:
            m = HEADER.fullmatch(line.rstrip("\n"))
            if m and int(m[1]) == clock["pid"] and clock["start_ns"] <= int(m[3]) * 10**9 + int(m[4]) < clock["end_ns"]:
                selected += 1
    need(selected >= 10, "perf timestamps do not match native monotonic window")
    durable.atomic_json(out / "preflight.json", {"schema": "r1.cpu-profile-preflight/v1", "status": "passed",
        "version": version, "record_help": help_record, "script_help": help_script, "record": recorded,
        "perf": observed, "perf_identity": located(perf), "clock": located(out / "clock.json"), "clock_samples": selected}, once=True)


def validate_phases(row, phases, result):
    need(phases["schema"] == "r1.flop-cpu-profile-phases/v1" and phases["case"] == row["case"]
         and phases["threads"] == row["workers"] and phases["iterations"] == ITERATIONS
         and type(phases["pid"]) is int and phases["pid"] > 0 and phases["clock"] == "CLOCK_MONOTONIC"
         and phases["clock_id"] == 1 and phases["unit"] == "nanoseconds" and phases["interval"] == "[start_ns,end_ns)"
         and phases["status"] == "completed" and phases["performance_claim"] is False, "phase clock/schema differs")
    need(set(phases["phases"]) == set(PHASES), "phase set differs")
    p = phases["phases"]
    for span in p.values():
        need(set(span) == {"start_ns", "end_ns"} and all(type(v) is int and 0 <= v < 2**64 for v in span.values())
             and span["start_ns"] < span["end_ns"], "invalid phase interval")
    need(p["cfr"]["end_ns"] <= p["state_write"]["start_ns"] < p["state_write"]["end_ns"] <= p["quality"]["start_ns"], "phase order differs")
    end = p["quality"]["start_ns"]
    for name in PHASES[3:]:
        need(end <= p[name]["start_ns"] < p[name]["end_ns"] <= p["quality"]["end_ns"], "quality interval order differs")
        end = p[name]["end_ns"]
    # Instant and CLOCK_MONOTONIC brackets differ by two clock reads; not equality.
    for name in PHASES[:3]:
        need(abs((p[name]["end_ns"] - p[name]["start_ns"]) / 1e9 - result[name + "_seconds"]) < .01, "phase clock/timer disagreement")


def solve(out, plan, row, receipt, canonical):
    binary = receipt["binaries"]["baseline"]
    need(pin(binary["path"]) == pair(binary), "binary changed")
    directory = out / row["name"]
    child = [binary["path"], row["case"], str(row["workers"]), str(ITERATIONS), str(directory / "artifacts")]
    profiled = row["kind"] == "profile"
    command = perf_command(plan["tools"]["perf"]["path"], directory / "perf.data", child) if profiled else child
    row.update(command=command, child_command=child, expected_child_affinity=plan["host"]["affinity"])
    directory = base.stage(out, plan, row, receipt, 90 if profiled else 120, out,
                           [binary["path"], plan["tools"]["perf"]["path"]])
    artifacts = directory / "artifacts"
    names = {"invocation.json", "result.json", "quality.json", "state.bin", "cpu.json", "phases.json"}
    need({p.name for p in artifacts.iterdir()} == names, "artifact membership differs")
    result, invocation, quality = (read(artifacts / n) for n in ("result.json", "invocation.json", "quality.json"))
    # CPU-profile adapter retains the original fixed depth2 invocation contract.
    base.validate_values(row, result, {**invocation, "quality_chance_depth": 2}, quality)
    base.validate_cpu(row, read(artifacts / "cpu.json"), result, row["expected_child_affinity"])
    phases = read(artifacts / "phases.json")
    validate_phases(row, phases, result)
    row.update(result=result, cpu=read(artifacts / "cpu.json"), phases=phases)
    state = common.compare_state(artifacts / "state.bin", (ITERATIONS, ITERATIONS, *common.HEADERS[row["case"]]),
                                 canonical["state"] if canonical else None, plan["deadline_monotonic"])
    need(state["bytes"] == result["state_bytes"], "state bytes differ")
    row["outputs"] = {n: located(artifacts / n) for n in sorted(names - {"state.bin"})} | {"state.bin": state}
    durable.sync_files(list(row["outputs"].values()))
    if profiled:
        need(canonical is not None, "profile lacks prior canonical")
        base.live(plan, 60)
        row["perf"] = perf_read(plan["tools"]["perf"]["path"], directory / "perf.data", directory, phases)
        need(sum(p.stat().st_size for p in out.glob("*/perf.data")) <= 128 * 1024**2, "total perf cap exceeded")
    if canonical is None:
        raw = Path(plan["workspace"]) / "canonical" / (row["case"] + ".bin")
        archive = durable.gzip_verified(state["path"], out / "canonical" / (row["case"] + ".bin.gz"), state)
        canonical = {"state": {**state, "path": str(raw)}, "state_gzip": archive, "quality": row["outputs"]["quality.json"]}
        kind = "new_canonical"
    else:
        need((artifacts / "quality.json").read_bytes() == Path(canonical["quality"]["path"]).read_bytes(), "canonical quality differs")
        need(pin(canonical["state_gzip"]["path"]) == pair(canonical["state_gzip"]), "canonical archive changed")
        kind = "alias"
    retention = {"kind": kind, "original": state, "canonical": canonical, "fullbyte_verified": True, "raw_removed": False}
    durable.atomic_json(directory / "retention.json", retention, once=True)
    base.live(plan)
    if kind == "new_canonical":
        os.replace(state["path"], canonical["state"]["path"])
        durable.sync_directory(Path(canonical["state"]["path"]).parent)
    else:
        Path(state["path"]).unlink()
    durable.sync_directory(artifacts)
    retention["raw_removed"] = True
    durable.atomic_json(directory / "retention.json", retention)
    row.update(state_retention=retention, retention_receipt=located(directory / "retention.json"))
    base.complete(out, row, receipt)
    return canonical


def build_rows():
    return [{"name": "perf-preflight-build", "kind": "perf-preflight"}, *base_build_rows()]


def finish_manifest(out):
    files = {n: p for n, p in inventory(out).items() if n != "retained.json"}
    need(sum(v["bytes"] for v in files.values()) <= MAX_RETAINED, "retained bound exceeded")
    durable.sync_files([{"path": str(out / n), **p} for n, p in files.items()])
    durable.atomic_json(out / "retained.json", {"schema": "r1.cpu-profile-retained/v1", "files": files}, once=True)


def measure_prepare(args):
    out = args.out.resolve(strict=True)
    original, built, execution = (read(out / n) for n in ("plan.json", "build.json", "build-execution.json"))
    need(built["status"] == execution["status"] == "completed" and built["plan"] == pin(out / "plan.json")
         and built["execution"] == pin(out / "build-execution.json") and built["stages"] == execution["stages"]
         and built["binaries"] == execution["binaries"] and base.inventory_subset(out, built["files"]) == built["files"], "successful immutable build required")
    machine, created = base.host("measure"), base.now()
    need(machine["boot_id"] != original["host"]["boot_id"] and machine["instance_id"] == original["host"]["instance_id"], "same-instance resize boundary differs")
    end = bounds(original["launch_attempted_at"], original["stop_deadline_utc"], args.deadline_utc, "measure", created)
    plan = {**original, "phase": "measure", "plan_file": "measurement.json", "host": machine, "limits": base.phase_limits("measure"),
            "created_at": created, "deadline_utc": args.deadline_utc, "deadline_monotonic": time.monotonic() + end.timestamp() - time.time(),
            "build_plan": pin(out / "plan.json"), "build_receipt": pin(out / "build.json"), "build_execution": pin(out / "build-execution.json")}
    for binary in built["binaries"].values():
        need(pin(binary["path"]) == pair(binary), "prebuilt binary changed")
    need(not (out / "measurement.json").exists() and not (out / "execution.json").exists(), "no retry/resume")
    durable.atomic_json(out / "measurement.json", plan, once=True)
    base.live(plan)


def execute(args):
    out = args.out.resolve(strict=True)
    building = args.phase == "build"
    plan = read(out / ("plan.json" if building else "measurement.json"))
    receipt_file = "build-execution.json" if building else "execution.json"
    need(plan["phase"] == ("build" if building else "measure") and str(out) == plan["output"] and not (out / receipt_file).exists(), "no retry/resume")
    schedule_rows = build_rows() if building else [{"name": "perf-preflight-measure", "kind": "perf-preflight"}, *schedule()]
    rows = [{**r, "status": "pending"} for r in schedule_rows]
    built = None if building else read(out / "build.json")
    if built:
        need(pin(out / "build.json") == plan["build_receipt"] and pin(out / "build-execution.json") == plan["build_execution"]
             and base.inventory_subset(out, built["files"]) == built["files"], "immutable build changed")
    receipt = {"schema": "r1.cpu-profile-execution/v1", "phase": plan["phase"], "receipt_file": receipt_file,
               "status": "running", "plan": pin(out / plan["plan_file"]), "started_at": base.now(), "stages": rows,
               "binaries": {} if building else built["binaries"], "canonical": {}, "prepared_plan": plan}
    current = None
    try:
        base.clean_environment(plan)
        for current in rows:
            if current["kind"] == "perf-preflight":
                current["command"] = [plan["tools"]["python"]["path"], str(Path(__file__)), "perf-check", "--perf", plan["tools"]["perf"]["path"], "--out", str(out / current["name"])]
                directory = base.stage(out, plan, current, receipt, 60, out, [plan["tools"]["perf"]["path"]])
                current["preflight"] = located(directory / "preflight.json")
                need(read(directory / "preflight.json")["status"] == "passed", "perf preflight failed")
                base.complete(out, current, receipt)
            elif current["kind"] == "toolchain":
                current["command"] = [plan["tools"]["rustc"]["path"], "-Vv"]
                directory = base.stage(out, plan, current, receipt, 10, out)
                record = read(directory / "supervisor.json")
                version = Path(record["outputs"]["stdout"]["path"]).read_text()
                need("release: 1.97.0" in version and "host: x86_64-unknown-linux-gnu" in version, "compiler differs")
                receipt["rustc_version"] = version
                base.complete(out, current, receipt)
            elif current["kind"] in {"build", "tests"}:
                target = Path(plan["workspace"]) / "baseline-target"
                if current["kind"] == "build":
                    need(not target.exists(), "fresh Cargo target required")
                    current["command"] = [plan["tools"]["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2", "--target-dir", str(target), "-p", "holdem", "--example", EXAMPLE, "--message-format=json"]
                else:
                    current["command"] = base.test_command(plan)
                directory = base.stage(out, plan, current, receipt, BUILD_SECONDS, Path(plan["sources"]["baseline"]["path"]))
                if current["kind"] == "build":
                    binary = located(target / "release/examples" / EXAMPLE)
                    receipt["binaries"]["baseline"] = binary
                    current["retained_binary"] = durable.gzip_verified(binary["path"], out / "binary-baseline.gz", binary)
                else:
                    record = read(directory / "supervisor.json")
                    base.validate_tests(Path(record["outputs"]["stdout"]["path"]).read_text())
                base.complete(out, current, receipt)
            else:
                receipt["canonical"][current["case"]] = solve(out, plan, current, receipt, receipt["canonical"].get(current["case"]))
        base.live(plan)
        need(all(r["status"] == "completed" for r in rows), "incomplete schedule")
        receipt["status"] = "completed"
    except BaseException as error:
        receipt.update(status="failed", diagnostic="not_evaluated", error=repr(error), traceback=traceback.format_exc())
        if current is not None and current["status"] != "completed":
            current.update(status="failed", error=repr(error))
        for row in rows:
            if row["status"] == "pending":
                row.update(status="skipped", reason="stopped on first failure")
        base.preserve_failed_state(out, plan, current, receipt)
        raise
    finally:
        receipt.pop("prepared_plan", None)
        receipt["ended_at"] = base.now()
        receipt["counts"] = {s: sum(r["status"] == s for r in rows) for s in ("completed", "failed", "skipped")}
        base.persist(out, receipt)
        if building and receipt["status"] == "completed":
            files = inventory(out)
            durable.sync_files([{"path": str(out / n), **p} for n, p in files.items()])
            durable.atomic_json(out / "build.json", {"schema": "r1.cpu-profile-build/v1", "status": "completed", "plan": receipt["plan"],
                "execution": pin(out / receipt_file), "stages": rows, "binaries": receipt["binaries"], "files": files}, once=True)
        else:
            finish_manifest(out)
        print(json.dumps({"status": receipt["status"], "phase": plan["phase"], "counts": receipt["counts"]}), flush=True)


# Reuse frozen stage supervision, host/features, finite input identity checks,
# test selection and full-state comparator. These functions resolve their module
# globals; bind only explicit campaign parameters/hooks before calling them.
base_build_rows = base.build_rows
for name in ("controls", "environment", "schedule", "inventory", "MAX_RETAINED", "EXAMPLE", "CPU_ADAPTER", "CPU_ADAPTER_SHA"):
    setattr(base, name, globals()[name])
base.__file__ = __file__
base.canonical_key = lambda row: row["case"]
phase_limits, host, validate_features, one_per_core, terminal, raw_outputs = (getattr(base, n) for n in ("phase_limits", "host", "validate_features", "one_per_core", "terminal", "raw_outputs"))
test_command, validate_tests, source_bindings = base.test_command, base.validate_tests, base.source_bindings
validate_cpu = base.validate_cpu


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "build", "measure-prepare", "measure", "perf-check", "perf-child"))
    parser.add_argument("--out", type=Path)
    parser.add_argument("--source", "--baseline-source", dest="baseline_source", type=Path)
    for name in ("workspace", "cargo", "rustc", "perf"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--deadline-utc", "--build-deadline-utc", "--measurement-deadline-utc", dest="deadline_utc")
    parser.add_argument("--launch-attempted-at")
    parser.add_argument("--stop-deadline-utc")
    args = parser.parse_args()
    if args.phase == "perf-child":
        need(args.out is not None, "preflight clock output required")
        start = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
        end = time.monotonic() + 1.0
        while time.monotonic() < end:
            pass
        durable.atomic_json(args.out / "clock.json", {"clock": "CLOCK_MONOTONIC", "pid": os.getpid(), "start_ns": start,
            "end_ns": time.clock_gettime_ns(time.CLOCK_MONOTONIC)}, once=True)
        return
    need(args.out is not None, "output required")
    if args.phase == "perf-check":
        need(args.perf is not None, "perf path required")
        perf_preflight(args)
    elif args.phase == "prepare":
        need(all(getattr(args, n) for n in ("baseline_source", "workspace", "cargo", "rustc", "perf", "deadline_utc", "launch_attempted_at", "stop_deadline_utc")), "prepare arguments missing")
        prepare(args)
    elif args.phase == "measure-prepare":
        need(args.deadline_utc, "measurement deadline required")
        measure_prepare(args)
    else:
        execute(args)


if __name__ == "__main__":
    main()
