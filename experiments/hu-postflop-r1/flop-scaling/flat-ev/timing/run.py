"""Bounded Windows timing comparison: prepare, build, pilot, matrix (never chained)."""
from __future__ import annotations

import argparse
import ctypes
import datetime
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import time
import traceback

HERE = Path(__file__).resolve().parent
FLOP = HERE.parents[1]
ROOT = HERE.parents[4]
SHARED = FLOP / "ev-scratch/run.py"
SHARED_SHA = "14f6fd5e95777aa1c7758b7ae99045b2f33452e5381b62a3a43efd6368b3da5d"
if hashlib.sha256(SHARED.read_bytes()).hexdigest() != SHARED_SHA:
    raise ValueError("Frozen native diagnostic helper differs")
spec = importlib.util.spec_from_file_location("timing_native_helpers", SHARED)
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)
need, read = shared.need, shared.read
WRAPPER = FLOP / "native-preflight/run_bounded.py"
BASE = FLOP / "ev-scratch/proof01"
FLAT = ROOT / "runs/flop-flat-ev01"
GENERIC = HERE.parent / "generic-checks/execution-checks01/verification.json"
CASES = ("narrow", "expanded")
WORKERS = (1, 2, 4, 8, 16)
PILOT_ITERATIONS = (16, 32, 64, 128)
LIMITS = {"timeout_seconds": 120, "grace_seconds": 0.2, "kill_wait_seconds": 5,
          "poll_seconds": 0.1, "memory_limit_bytes": 469762048,
          "min_free_memory_bytes": 1610612736, "disk_reserve_bytes": 2147483648}
JOB = {"limit_flags": 8704, "job_memory_limit_bytes": 536870912,
       "root_priority_class": 16384, "verified_before_resume": True}
HEADERS = {"narrow": (367662, 147104, 34, 30, 10176768, 10176768),
           "expanded": (367662, 147104, 63, 160, 35459676, 35459676)}


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def deadline_check(deadline, reserve=0):
    need(deadline is None or time.monotonic() + reserve < deadline, "absolute phase deadline reached")


def pin(path, deadline=None):
    digest, size = hashlib.sha256(), 0
    with Path(path).open("rb") as stream:
        while block := stream.read(1024 * 1024):
            deadline_check(deadline)
            digest.update(block)
            size += len(block)
    return {"bytes": size, "sha256": digest.hexdigest()}


def located(path, deadline=None):
    return {"path": str(path), **pin(path, deadline)}


def pair(value):
    return {k: value[k] for k in ("bytes", "sha256")}


def save(path, value):
    path = Path(path)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")
    os.replace(temporary, path)


def unchanged(pins, deadline=None):
    for path, expected in pins.items():
        need(pin(path, deadline) == expected, f"input changed: {path}")


def inventory(directory, deadline=None):
    return {str(p): pin(p, deadline) for p in sorted(Path(directory).iterdir())
            if p.suffix in {".rlib", ".rmeta", ".dll"}}


def matrix_rows():
    rows = []
    for round_index in range(4):
        for case in CASES:
            workers = WORKERS if round_index % 2 == 0 else WORKERS[::-1]
            for index, worker in enumerate(workers):
                arms = ("baseline", "flat") if (round_index + index) % 2 == 0 else ("flat", "baseline")
                for arm in arms:
                    rows.append({"name": f"r{round_index}-{case}-{worker}-{arm}", "case": case,
                                 "arm": arm, "workers": worker, "round": round_index,
                                 "warmup": round_index == 0, "status": "pending"})
    return rows


def pilot_done(iterations, cfr_seconds):
    need(iterations in PILOT_ITERATIONS and math.isfinite(cfr_seconds) and cfr_seconds >= 0,
         "invalid baseline pilot observation")
    return cfr_seconds >= 4 or iterations == 128


def verify_record(record):
    need(record["state"] == "completed" and record["child_exit_code"] == record["supervisor_exit_code"] == 0,
         "bounded process did not complete successfully")
    need(record["identity_unchanged"] and record["identity_before"] == record["identity_after"], "stage identity changed")
    need(record["cleanup_complete"] and not record["forced"] and record["last_sample"]["pids"] == [], "stage cleanup incomplete")
    need(record["bounded_job_settings"] == JOB, "queried Job settings differ")


def native_inputs(directory, arm):
    plan, built, execution = (read(directory / name) for name in ("plan.json", "build.json", "execution.json"))
    need(built["status"] == execution["status"] == "completed", f"{arm} native prerequisite incomplete")
    need(built["plan"] == execution["plan"] == pin(directory / "plan.json"), "native plan binding differs")
    need(execution["build"] == pin(directory / "build.json"), "native execution/build binding differs")
    need(len(execution["stages"]) == 8 and all(s["fullstate_and_quality_bytes_equal_baseline"] for s in execution["stages"]),
         "short native full-state comparison prerequisite incomplete")
    expected_target = ROOT / "target" / ("flop-ev-scratch01" if arm == "baseline" else "flop-flat-ev01")
    need(Path(plan["target"]) == expected_target, "unexpected native target")
    libraries = {}
    for name in ("engine", "game", "holdem"):
        stages = [s for s in built["stages"] if s["name"].endswith("-" + name)]
        need(len(stages) == 1, "ambiguous native library receipt")
        artifact = stages[0]["artifact"]
        path = expected_target / f"lib{name}.rlib"
        need(artifact["path"] == str(path) and pin(path) == pair(artifact), "native library pin differs")
        libraries[str(path)] = pair(artifact)
    need(pin(directory / "source.tar.gz") == plan["source_archive"], "native source archive differs")
    return plan, libraries


def prepare(args):
    out, target = shared.child(args.out, ROOT / "runs"), shared.child(args.target, ROOT / "target")
    need(not out.exists() and not target.exists(), "prepare requires fresh output and target")
    base_manifest = read(BASE / "manifest.json")
    for name in ("plan.json", "build.json", "execution.json", "source.tar.gz"):
        need(pin(BASE / name) == base_manifest["files"][name], "retained baseline file differs")
    base, base_libs = native_inputs(BASE, "baseline")
    flat, flat_libs = native_inputs(FLAT, "flat")
    need(base["compiler"] == flat["compiler"] and base["compiler_version"] == flat["compiler_version"], "compiler mismatch")
    need(base["dependency_pins"] == flat["dependency_pins"] and base["reused_externs"] == flat["reused_externs"], "release dependencies differ")
    compiler = base["compiler"]
    need(pin(compiler["path"]) == pair(compiler), "compiler binary changed")
    generic = read(GENERIC)
    need(generic["status"] == "passed" and generic["source_unchanged"], "generic tests incomplete")
    for key, path in (("native_plan", FLAT / "plan.json"), ("native_build", FLAT / "build.json"),
                      ("native_engine", ROOT / "target/flop-flat-ev01/libengine.rlib")):
        need(pair(generic[key]) == pin(path), "generic test source/library binding differs")
    names = [name for s in generic["stages"] for name in s["tests_passed"]]
    need(len(names) == 8 and len(set(names)) == 8, "generic test coverage receipt differs")
    provenance = read(HERE / "provenance.json")
    need(provenance["outside_bounds_and_usage_byte_identical"], "adapter has unrelated differences")
    for name, expected in provenance["generated"].items():
        need(pin(HERE / name) == expected, "timing adapter provenance differs")
    need(pin(HERE / "prepare.py") == provenance["preparer"], "adapter preparer differs")
    controls = [Path(__file__), SHARED, WRAPPER, ROOT / "tools/run_supervised.py", GENERIC,
                HERE / "protocol.md", HERE / "prepare.py", HERE / "provenance.json", HERE / "adapter.patch",
                HERE / "solve.rs", Path(compiler["path"]), Path(sys.executable), BASE / "manifest.json"]
    for directory in (BASE, FLAT):
        controls += [directory / name for name in ("plan.json", "build.json", "execution.json", "source.tar.gz")]
    controls += [FLOP / f"fixtures/{case}.toml" for case in CASES]
    dependency_inventories = {}
    for directory in (base["dependency_directory"], base["target"], flat["target"]):
        dependency_inventories[directory] = inventory(directory)
    need(dependency_inventories[base["dependency_directory"]] == base["dependency_pins"], "release dependency set changed")
    unchanged(base_libs | flat_libs)
    out.mkdir(parents=True)
    target.mkdir(parents=True)
    source = out / "source"
    source.mkdir()
    shutil.copyfile(HERE / "solve.rs", source / "solve.rs")
    controls.append(source / "solve.rs")
    jobs = []
    for arm, native in (("baseline", base), ("flat", flat)):
        job = next(j for j in native["build_jobs"] if j["name"] == "ordinary")
        argv = list(job["argv"])
        argv[argv.index("--crate-name") + 1] = f"flat_ev_timing_{arm}"
        old_adapter = str(Path(native["source"]) / "adapters/solve.rs")
        need(argv.count(old_adapter) == 1, "native adapter argv differs")
        argv[argv.index(old_adapter)] = str(source / "solve.rs")
        artifact = target / f"{arm}.exe"
        argv[argv.index("-o") + 1] = str(artifact)
        argv[1:1] = ["-C", f"metadata=r1_flat_ev_timing_{arm}_v1"]
        jobs.append({"name": f"build-{arm}", "arm": arm, "argv": argv,
                     "artifact": str(artifact), "status": "pending"})
    plan = {"schema": "r1-flat-ev-timing/v1", "target": str(target), "source": str(source),
            "compiler": compiler, "compiler_version": base["compiler_version"],
            "controls": {str(p): pin(p) for p in controls}, "native_libraries": base_libs | flat_libs,
            "dependency_inventories": dependency_inventories, "build_jobs": jobs,
            "limits": LIMITS, "matrix_seconds": 1200, "matrix": matrix_rows(),
            "pilot_iterations": list(PILOT_ITERATIONS), "pilot_minimum_cfr_seconds": 4,
            "scope": "Fixed-profile local 8-core/16-logical comparison; no external or 32-vCPU certification"}
    validate_inputs(plan)
    save(out / "plan.json", plan)
    print(json.dumps({"status": "prepared", "out": str(out), "build_jobs": 2, "matrix_processes": 80}), flush=True)


def validate_inputs(plan, deadline=None):
    need(plan["limits"] == LIMITS and plan["matrix_seconds"] == 1200 and plan["matrix"] == matrix_rows(), "fixed protocol differs")
    need(plan["pilot_iterations"] == list(PILOT_ITERATIONS) and plan["pilot_minimum_cfr_seconds"] == 4, "pilot rule differs")
    unchanged(plan["controls"], deadline)
    unchanged(plan["native_libraries"], deadline)
    for directory, expected in plan["dependency_inventories"].items():
        need(inventory(directory, deadline) == expected, "searched dependency inventory changed")


def host_window(seconds=1.0):
    # Read only. Before/after windows measure background activity while no
    # workload belonging to this driver is running, not its CPU utilization.
    from ctypes import wintypes as w
    spec = importlib.util.spec_from_file_location("timing_supervisor_memory", ROOT / "tools/run_supervised.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    api = module.WindowsAPI()
    api.k.GetSystemTimes.argtypes = [ctypes.POINTER(w.FILETIME)] * 3
    api.k.GetSystemTimes.restype = w.BOOL
    def point():
        values = [w.FILETIME() for _ in range(3)]
        api.check(api.k.GetSystemTimes(*(ctypes.byref(v) for v in values)), "GetSystemTimes")
        return {"at": now(), "clock": time.monotonic(), "ticks": [(v.dwHighDateTime << 32) | v.dwLowDateTime for v in values],
                "memory": api.memory()}
    start = point()
    time.sleep(seconds)
    end = point()
    idle, kernel, user = [b - a for a, b in zip(start["ticks"], end["ticks"])]
    need(kernel + user > 0 and 0 <= idle <= kernel + user, "host CPU counters invalid")
    return {"before": start, "after": end, "logical_cpus": os.cpu_count(),
            "busy_percent": 100 * (kernel + user - idle) / (kernel + user),
            "elapsed_seconds": end["clock"] - start["clock"]}


def stage(out, plan, item, receipt, receipt_path, deadline):
    validate_inputs(plan, deadline)
    need(pin(out / "plan.json", deadline) == receipt["plan"], "plan changed during phase")
    deadline_check(deadline, 140)  # full fixed stage + cleanup, windows, and bounded driver margin
    directory = out / item["name"]
    directory.mkdir()
    item["status"], item["started_at"] = "running", now()
    item["background_before"] = host_window()
    memory = item["background_before"]["after"]["memory"]
    need(memory["available_bytes"] >= 1610612736 and memory["commit_available_bytes"] >= 1610612736, "fresh host memory gate failed")
    need(shutil.disk_usage(directory).free >= LIMITS["disk_reserve_bytes"], "fresh disk gate failed")
    cmd = [sys.executable, "-B", str(WRAPPER), "--record", str(directory / "record.json"), "--cwd", str(ROOT)]
    for name, value in LIMITS.items():
        cmd += ["--" + name.replace("_", "-"), str(value)]
    cmd += ["--disk-path", str(directory), "--identity-file", str(out / "plan.json"),
            "--identity-file", str(HERE / "run.py"), "--", *item["workload_argv"]]
    item["argv"] = cmd
    save(receipt_path, receipt)
    stdout = stderr = b""
    try:
        result = subprocess.run(cmd, capture_output=True, timeout=135, check=False)
        stdout, stderr = result.stdout, result.stderr
        item["wrapper_exit_code"] = result.returncode
    except subprocess.TimeoutExpired as error:
        # subprocess.run kills only this driver's wrapper; its kill-on-close
        # Job owns the workload. Preserve uncertainty and stop without retry.
        stdout, stderr = error.stdout or b"", error.stderr or b""
        item["wrapper_timeout"] = True
        raise
    finally:
        (directory / "wrapper.stdout.log").write_bytes(stdout)
        (directory / "wrapper.stderr.log").write_bytes(stderr)
        if (directory / "record.json").exists():
            item["record"] = located(directory / "record.json")
        item["ended_at"] = now()
        save(receipt_path, receipt)
    need(item["wrapper_exit_code"] == 0, f"bounded stage failed: {item['name']}")
    record = read(directory / "record.json")
    verify_record(record)
    for payload in record["outputs"].values():
        need(pin(payload["path"], deadline) == pair(payload), "supervisor raw output changed")
    item["root_os_peak_resident_bytes"] = record["last_sample"]["root_os_peak_resident_bytes"]
    item["job_os_peak_commit_bytes"] = record["last_sample"]["job_os_peak_commit_bytes"]
    item["process_seconds"] = record["elapsed_seconds"]
    item["background_after"] = host_window()
    validate_inputs(plan, deadline)
    deadline_check(deadline)
    return directory


def compare_state(path, expected_header, canonical=None, deadline=None):
    digest, size = hashlib.sha256(), 0
    reference = Path(canonical["path"]).open("rb") if canonical else None
    try:
        with Path(path).open("rb") as stream:
            header = stream.read(72)
            need(header == b"R1F32S01" + struct.pack("<8Q", *expected_header), "state header differs")
            stream.seek(0)
            while block := stream.read(1024 * 1024):
                deadline_check(deadline)
                if reference:
                    need(block == reference.read(len(block)), "full state bytes differ from pilot canonical")
                digest.update(block)
                size += len(block)
            need(reference is None or reference.read(1) == b"", "canonical state has extra bytes")
        expected_size = 72 + 2 * sum(expected_header[4:6]) + 4 * sum(expected_header[6:8])
        need(size == expected_size, "state length differs")
        result = {"path": str(path), "bytes": size, "sha256": digest.hexdigest()}
        if canonical:
            need(pair(result) == pair(canonical), "canonical state identity differs")
        return result
    finally:
        if reference:
            reference.close()


def output_checks(directory, item, canonical=None, deadline=None):
    output = directory / "artifacts"
    need({p.name for p in output.iterdir()} == {"invocation.json", "result.json", "state.bin", "quality.json"}, "unexpected solver artifact set")
    result, invocation, quality = (read(output / name) for name in ("result.json", "invocation.json", "quality.json"))
    for value in (result, invocation):
        need(value["case"] == item["case"] and value["threads"] == item["workers"] and value["iterations"] == item["iterations"], "result condition differs")
    need(result["status"] == "completed" and invocation["planned_iterations"] == item["iterations"], "solver incomplete")
    need(invocation["storage"] == "f32" and invocation["schedule"] == "dcfr" and invocation["chance_depth"] == 2 and invocation["min_children"] == 12, "solver protocol differs")
    need(quality["case"] == item["case"] and quality["iterations"] == item["iterations"], "quality condition differs")
    for metric in ("ev", "br", "exploitability"):
        need(len(quality[metric]) == len(quality[metric + "_bits"]) == 2, "quality vector size differs")
        for value, bits in zip(quality[metric], quality[metric + "_bits"]):
            need(math.isfinite(value) and struct.pack(">d", value).hex() == bits, "quality value/bits differ")
    for key in ("build_seconds", "cfr_seconds", "state_write_seconds", "quality_seconds"):
        need(math.isfinite(result[key]) and result[key] >= 0, "invalid phase timer")
    item["result"] = result
    state = compare_state(output / "state.bin", (item["iterations"], item["iterations"], *HEADERS[item["case"]]),
                          canonical["state"] if canonical else None, deadline)
    need(result["state_bytes"] == state["bytes"], "result state length differs")
    quality_pin = located(output / "quality.json", deadline)
    if canonical:
        need(pin(canonical["quality"]["path"], deadline) == pair(canonical["quality"]), "canonical quality changed")
        need((output / "quality.json").read_bytes() == Path(canonical["quality"]["path"]).read_bytes(), "quality bytes differ from pilot canonical")
    item["outputs"] = {name: located(output / name, deadline) for name in ("result.json", "invocation.json", "quality.json")}
    item["outputs"]["state.bin"] = state
    item["canonical"] = canonical
    item["fullstate_and_quality_bytes_equal_canonical"] = canonical is not None
    return {"state": state, "quality": quality_pin}


def complete(item, receipt, path):
    item["status"] = "completed"
    item["verified_at"] = now()
    save(path, receipt)
    print(json.dumps({"stage": item["name"], "status": "completed", "case": item.get("case"),
                      "workers": item.get("workers"), "iterations": item.get("iterations"),
                      "result": item.get("result")}), flush=True)


def execute(args):
    out = shared.child(args.out, ROOT / "runs")
    plan = read(out / "plan.json")
    path = out / {"build": "build.json", "pilot": "pilot.json", "matrix": "execution.json"}[args.phase]
    need(not path.exists(), "phase already attempted; no retry or resume")
    if args.phase == "build":
        rows = [dict(job) for job in plan["build_jobs"]]
    elif args.phase == "pilot":
        rows = [{"name": f"pilot-{case}-{n}", "case": case, "arm": "baseline", "workers": 1,
                 "round": None, "warmup": False, "iterations": n, "status": "pending"}
                for case in CASES for n in PILOT_ITERATIONS]
    else:
        rows = matrix_rows()
    receipt = {"schema": "r1-flat-ev-timing-phase/v1", "phase": args.phase, "status": "running",
               "started_at": now(), "plan": pin(out / "plan.json"), "stages": rows, "selected": {}}
    save(path, receipt)
    deadline = time.monotonic() + 1200 if args.phase == "matrix" else None
    if deadline:
        receipt["absolute_deadline_utc"] = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=1200)).isoformat()
    current = None
    try:
        validate_inputs(plan, deadline)
        if args.phase == "build":
            need(not any(Path(plan["target"]).iterdir()), "timing target is not empty")
            for current in rows:
                current["workload_argv"] = current.pop("argv")
                stage(out, plan, current, receipt, path, deadline)
                current["artifact"] = located(current["artifact"], deadline)
                complete(current, receipt, path)
        else:
            built = read(out / "build.json")
            need(built["status"] == "completed" and built["plan"] == receipt["plan"] and len(built["stages"]) == 2, "timing build incomplete")
            receipt["build"] = pin(out / "build.json")
            binaries = {s["arm"]: s["artifact"] for s in built["stages"]}
            pins = {v["path"]: pair(v) for v in binaries.values()}
            if args.phase == "matrix":
                pilot = read(out / "pilot.json")
                need(pilot["status"] == "completed" and pilot["plan"] == receipt["plan"] and pilot["build"] == receipt["build"], "pilot prerequisite differs")
                need(set(pilot["selected"]) == set(CASES), "pilot selections incomplete")
                receipt["pilot"] = pin(out / "pilot.json")
                receipt["selected"] = pilot["selected"]
            for current in rows:
                case = current["case"]
                if args.phase == "pilot" and case in receipt["selected"]:
                    current.update(status="skipped", reason="baseline pilot already selected smaller N")
                    continue
                if args.phase == "matrix":
                    current["iterations"] = receipt["selected"][case]["iterations"]
                unchanged(pins, deadline)
                need(pin(out / "build.json", deadline) == receipt["build"], "build receipt changed")
                current["workload_argv"] = [binaries[current["arm"]]["path"], case, str(current["workers"]),
                                            str(current["iterations"]), str(out / current["name"] / "artifacts")]
                directory = stage(out, plan, current, receipt, path, deadline)
                canonical = receipt["selected"][case]["canonical"] if args.phase == "matrix" else None
                own_canonical = output_checks(directory, current, canonical, deadline)
                unchanged(pins, deadline)
                if args.phase == "pilot" and pilot_done(current["iterations"], current["result"]["cfr_seconds"]):
                    receipt["selected"][case] = {"iterations": current["iterations"], "cfr_seconds": current["result"]["cfr_seconds"],
                                                "short_timing": current["result"]["cfr_seconds"] < 4,
                                                "canonical": own_canonical, "stage": current["name"]}
                complete(current, receipt, path)
            if args.phase == "pilot":
                need(set(receipt["selected"]) == set(CASES), "pilot incomplete")
            else:
                need(len(rows) == 80 and all(s["status"] == "completed" for s in rows), "matrix incomplete")
        deadline_check(deadline)
        receipt["status"] = "completed"
    except BaseException as error:
        receipt["status"], receipt["error"], receipt["traceback"] = "failed", repr(error), traceback.format_exc()
        if current is not None and current["status"] not in {"completed", "skipped"}:
            current["status"], current["error"] = "failed", repr(error)
        for row in rows:
            if row["status"] == "pending":
                row.update(status="skipped", reason="phase stopped on first failure")
        raise
    finally:
        receipt["ended_at"] = now()
        receipt["counts"] = {state: sum(s["status"] == state for s in rows) for state in ("completed", "failed", "skipped")}
        save(path, receipt)
        print(json.dumps({"phase": args.phase, "status": receipt["status"], "counts": receipt["counts"]}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "build", "pilot", "matrix"))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--target", type=Path)
    args = parser.parse_args()
    if args.phase == "prepare":
        need(args.target is not None, "prepare requires --target")
        prepare(args)
    else:
        need(os.name == "nt", "Windows bounded stage wrapper required")
        execute(args)


if __name__ == "__main__":
    main()
