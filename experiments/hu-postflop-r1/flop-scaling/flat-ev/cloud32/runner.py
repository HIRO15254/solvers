"""Finite Linux EV/flat comparison; source preparation, build, then smoke/pilot/matrix."""
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import sys
import tarfile
import time
import traceback

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
COMMON = HERE.parent / "timing/run.py"
COMMON_SHA = "c4f3fbd1375f465333e2327465a098b96fb35922e8dbdcce48a3e181290b7892"
if hashlib.sha256(COMMON.read_bytes()).hexdigest() != COMMON_SHA:
    raise ValueError("Pinned stream/quality helper changed")
spec = importlib.util.spec_from_file_location("cloud32_timing_helpers", COMMON)
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)
need, read, save, pin, pair, located = (getattr(common, n) for n in ("need", "read", "save", "pin", "pair", "located"))
SUPERVISOR = ROOT / "tools/run_supervised.py"
SUPERVISOR_SHA = "5bd46e106bbc48e971c43c1ea080ed16e22356e83747ec6fb57157013fd029b8"
SOLVER = "crates/engine/src/solver.rs"
SOLVER_PINS = {"baseline": "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a",
               "flat": "ec9e5e6b5230684fce6f2315370ceda0e05fb4346eb7e4ff44ea05e6028aabcd"}
EXAMPLE = "crates/holdem/examples/flop_cloud32_probe.rs"
ADAPTER_SHA = "63cba37a5b0f79dfed2ca0ddf879daada3f0fee094ee5f287f86bc1f37414d46"
WORKERS = (1, 2, 4, 8, 16, 32)
CASES = ("narrow", "expanded")
LIMITS = {"grace_seconds": 0.2, "kill_wait_seconds": 5, "poll_seconds": 0.1,
          "memory_limit_bytes": 8 * 1024**3, "min_free_memory_bytes": 2 * 1024**3,
          "disk_reserve_bytes": 2 * 1024**3}
MAX_RETAINED_BEFORE_STAGE = 400 * 1024**2
MAX_ARCHIVE = 1024**3
GENERIC = HERE.parent / "generic-checks/execution-checks01/verification.json"


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def inventory(root, deadline=None):
    result = {}
    for path in sorted(root.rglob("*")):
        need(not path.is_symlink(), "source symlinks are not accepted")
        if path.is_file():
            result[path.relative_to(root).as_posix()] = pin(path, deadline)
        else:
            need(path.is_dir(), "source entry is not regular")
    return result


def package_bindings(manifest, installation, manifest_pin, sources, adapter, package_pins, destination):
    """Bind both copied workspaces to the installer-verified package, not each other alone."""
    need(manifest["schema"] == "r1-flat-ev-cloud32-package/v1", "package schema differs")
    need(package_pins == manifest["files"], "installed package bytes differ")
    originals = manifest["source_pins"]
    need({name.removeprefix("source/"): value for name, value in package_pins.items()
          if name.startswith("source/")} == originals, "original source package binding differs")
    need(all(name in originals for name in (SOLVER, "Cargo.toml", "Cargo.lock", ".cargo/config.toml"))
         and EXAMPLE not in originals, "original workspace boundary differs")
    need(originals[SOLVER]["sha256"] == SOLVER_PINS["baseline"]
         and manifest["candidate"]["sha256"] == SOLVER_PINS["flat"]
         and package_pins["candidate/solver.rs"] == manifest["candidate"], "packaged solver pin differs")
    adapter_name = "experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32/solve.rs"
    need(adapter["sha256"] == ADAPTER_SHA and package_pins[adapter_name] == adapter,
         "packaged adapter pin differs")
    for arm in ("baseline", "flat"):
        expected = {**originals, EXAMPLE: adapter}
        if arm == "flat":
            expected[SOLVER] = manifest["candidate"]
        need(sources[arm] == expected, f"{arm} source differs from installed package")
    need(installation["manifest"] == manifest_pin
         and installation["source_revision"] == manifest["source_revision"]
         and installation["source_files"] == len(originals)
         and installation["destination"] == str(destination)
         and installation["builds_or_solves_started"] == 0, "installation receipt binding differs")
    archive_sha = installation["archive_sha256"]
    need(isinstance(archive_sha, str) and len(archive_sha) == 64
         and all(c in "0123456789abcdef" for c in archive_sha), "installation archive identity missing")
    return {"manifest": manifest_pin, "archive_sha256": archive_sha,
            "source_revision": manifest["source_revision"], "source_files": len(originals),
            "archive_identity_scope": "Installer recorded caller-pinned archive SHA; runner rechecks installed bytes"}


def matrix_rows():
    rows = []
    for case in CASES:
        for round_index in range(4):
            workers = WORKERS if round_index % 2 == 0 else WORKERS[::-1]
            for index, worker in enumerate(workers):
                arms = ("baseline", "flat") if (round_index + index) % 2 == 0 else ("flat", "baseline")
                for arm in arms:
                    rows.append({"name": f"{case}-r{round_index}-w{worker}-{arm}", "case": case,
                                 "round": round_index, "warmup": round_index == 0,
                                 "workers": worker, "arm": arm, "status": "pending"})
    return rows


def host():
    need(sys.platform == "linux" and platform.machine() == "x86_64", "Linux x86_64 required")
    affinity = sorted(os.sched_getaffinity(0))
    need(os.cpu_count() == 32 and len(affinity) == 32, "32 CPUs and full affinity required")
    cg = Path("/sys/fs/cgroup") / Path("/proc/self/cgroup").read_text().strip().split("::", 1)[1].lstrip("/")
    topology = []
    for cpu in affinity:
        path = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology")
        topology.append({"cpu": cpu, "core": (path / "core_id").read_text().strip(),
                         "socket": (path / "physical_package_id").read_text().strip()})
    quota = {str(p): (p / "cpu.max").read_text().strip() for p in (cg, *cg.parents)
             if p.is_relative_to("/sys/fs/cgroup") and (p / "cpu.max").is_file()}
    cgroup = {"path": str(cg), "memory_max": (cg / "memory.max").read_text().strip(),
              "swap_max": (cg / "memory.swap.max").read_text().strip(),
              "cpu_weight": (cg / "cpu.weight").read_text().strip(), "cpu_limits": quota}
    need(cgroup["memory_max"] == str(12 * 1024**3) and cgroup["swap_max"] == "0"
         and cgroup["cpu_weight"] == "100" and quota, "outer service limits differ")
    for text in quota.values():
        limit, period = text.split()
        need(limit == "max" or int(limit) / int(period) >= 32, "CPU quota below 32")
    return {"machine": platform.machine(), "logical_cpus": os.cpu_count(), "affinity": affinity,
            "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(), "topology": topology,
            "physical_cores": len({(x["core"], x["socket"]) for x in topology}), "cgroup": cgroup,
            "cpu_models": sorted(set(x.split(":", 1)[1].strip() for x in Path("/proc/cpuinfo").read_text().splitlines()
                                     if x.startswith("model name")))}


def background_window():
    def snapshot():
        fields = Path("/proc/stat").read_text().splitlines()[0].split()
        need(fields[0] == "cpu", "aggregate CPU counter missing")
        return {"at": now(), "monotonic": time.monotonic(), "ticks": list(map(int, fields[1:9]))}
    a = snapshot()
    time.sleep(1)
    b = snapshot()
    delta = [y - x for x, y in zip(a["ticks"], b["ticks"])]
    total = sum(delta)
    # /proc/stat iowait may decrease. Keep raw data and avoid treating the
    # diagnostic counter as an invariant or a reason to discard a sample.
    valid = total > 0 and all(x >= 0 for x in delta)
    return {"before": a, "after": b, "counter_valid": valid,
            "busy_percent": 100 * (total - delta[3] - delta[4]) / total if valid else None,
            "scope": "whole-host background before/after workload; no isolation claim"}


def source_archive(root, pins, destination):
    with destination.open("xb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w|") as archive:
            for name, expected in pins.items():
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = expected["bytes"], 0o644, 0
                with (root / name).open("rb") as stream:
                    archive.addfile(info, stream)
    need(destination.stat().st_size < 1024**2, "source archive exceeds 1 MiB design bound")


def prepare(args):
    machine = host()
    deadline_utc = dt.datetime.fromisoformat(args.deadline_utc.replace("Z", "+00:00"))
    need(deadline_utc.utcoffset() == dt.timedelta(0), "explicit UTC deadline required")
    remaining = deadline_utc.timestamp() - time.time()
    need(0 < remaining <= 2400, "experiment must fit maximum 40-minute absolute window")
    out, workspace = args.out.resolve(), args.workspace.resolve()
    sources = {"baseline": args.baseline_source.resolve(strict=True), "flat": args.flat_source.resolve(strict=True)}
    paths = [out, workspace, *sources.values()]
    need(len(set(paths)) == 4 and all(not a.is_relative_to(b) for a in paths for b in paths if a != b), "source/output/workspace paths overlap")
    need(not out.exists() and not workspace.exists(), "fresh proof and workspace required")
    provenance = read(HERE / "provenance.json")
    need(provenance["outside_bounds_and_usage_byte_identical"], "adapter inverse check missing")
    for name, expected in provenance["generated"].items():
        need(pin(HERE / name) == expected, "adapter differs from generated provenance")
    need(pin(HERE / "prepare.py") == provenance["preparer"], "adapter preparer differs")
    generic = read(GENERIC)
    need(generic["status"] == "passed" and generic["source_unchanged"] and generic["candidate_solver"]["sha256"] == SOLVER_PINS["flat"], "Windows generic prerequisite differs")
    pins = {arm: inventory(path) for arm, path in sources.items()}
    need(pins["baseline"].keys() == pins["flat"].keys(), "source file sets differ")
    need([name for name in pins["baseline"] if pins["baseline"][name] != pins["flat"][name]] == [SOLVER], "unexpected arm source difference")
    for arm in sources:
        need(pins[arm][SOLVER]["sha256"] == SOLVER_PINS[arm], "solver source pin differs")
        need(pins[arm][EXAMPLE] == pin(HERE / "solve.rs"), "both examples must use the same adapter")
    manifest_path, installation_path = ROOT / "manifest.json", ROOT / "installation.json"
    manifest, installation = read(manifest_path), read(installation_path)
    package_pins = {}
    for name in manifest["files"]:
        relative = PurePosixPath(name)
        need(not relative.is_absolute() and ".." not in relative.parts and "\\" not in name
             and ":" not in name and str(relative) == name, "unsafe package path")
        path = ROOT / name
        need(path.is_file() and not path.is_symlink() and path.resolve().is_relative_to(ROOT),
             "package member is not a contained regular file")
        package_pins[name] = pin(path)
    package = package_bindings(manifest, installation, pin(manifest_path), pins,
                               pin(HERE / "solve.rs"), package_pins, ROOT)
    need(pin(SUPERVISOR)["sha256"] == SUPERVISOR_SHA, "Linux supervisor differs")
    controls = [Path(__file__), COMMON, common.SHARED, SUPERVISOR, HERE / "protocol.md",
                HERE / "prepare.py", HERE / "provenance.json", HERE / "adapter.patch", HERE / "solve.rs", GENERIC,
                HERE / "install.py", manifest_path, installation_path]
    for path in controls:
        if path not in (manifest_path, installation_path):
            need(package_pins.get(path.relative_to(ROOT).as_posix()) == pin(path), "control is not package-pinned")
    tools = {"cargo": located(args.cargo.resolve(strict=True)), "rustc": located(args.rustc.resolve(strict=True))}
    out.mkdir(parents=True)
    workspace.mkdir(parents=True)
    (out / "inputs").mkdir()
    (out / "canonical").mkdir()
    (workspace / "canonical").mkdir()
    captured = {}
    for path in controls:
        value = pin(path)
        destination = out / "inputs" / value["sha256"]
        if not destination.exists():
            shutil.copyfile(path, destination)
        captured[str(path)] = {**value, "retained": destination.relative_to(out).as_posix()}
    source_records = {}
    for arm, source in sources.items():
        archive = out / f"source-{arm}.tar.gz"
        source_archive(source, pins[arm], archive)
        source_records[arm] = {"path": str(source), "files": pins[arm], "archive": located(archive)}
    plan = {"schema": "r1.flat-ev-cloud32/v1", "output": str(out), "workspace": str(workspace),
            "host": machine, "deadline_utc": args.deadline_utc,
            "deadline_monotonic": time.monotonic() + max(0, deadline_utc.timestamp() - time.time()),
            "sources": source_records, "controls": captured, "package": package, "tools": tools, "limits": LIMITS,
            "matrix": matrix_rows(), "pilot_iterations": list(common.PILOT_ITERATIONS),
            "environment": {"RUSTC": tools["rustc"]["path"], "RUSTFLAGS": "-C target-cpu=native",
                            "CARGO_BUILD_JOBS": "2", "CARGO_INCREMENTAL": "0", "RAYON_NUM_THREADS": "1"},
            "same_boot_build_required": True, "retained_before_stage_limit": MAX_RETAINED_BEFORE_STAGE}
    live(plan)
    save(out / "plan.json", plan)
    print(json.dumps({"status": "prepared", "sources": {a: len(pins[a]) for a in pins}, "matrix_processes": 96}), flush=True)


def live(plan, reserve=0):
    common.deadline_check(plan["deadline_monotonic"], reserve)
    need(host() == plan["host"], "boot, topology, affinity or service limits changed")
    need(plan["limits"] == LIMITS and plan["matrix"] == matrix_rows()
         and plan["pilot_iterations"] == list(common.PILOT_ITERATIONS), "fixed protocol changed")
    for path, value in plan["controls"].items():
        need(pin(path, plan["deadline_monotonic"]) == pair(value), "control changed")
    for arm, source in plan["sources"].items():
        need(inventory(Path(source["path"]), plan["deadline_monotonic"]) == source["files"], f"{arm} source changed")
    for value in plan["tools"].values():
        need(pin(value["path"], plan["deadline_monotonic"]) == pair(value), "tool changed")


def retained_bytes(out):
    return sum(p.stat().st_size for p in out.rglob("*") if p.is_file())


def load_supervisor():
    need(pin(SUPERVISOR)["sha256"] == SUPERVISOR_SHA, "supervisor source changed")
    spec = importlib.util.spec_from_file_location("cloud32_linux_supervisor", SUPERVISOR)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def stage(out, plan, item, receipt, receipt_path, seconds, cwd):
    live(plan, seconds + 20)
    need(pin(out / "plan.json") == receipt["plan"], "plan changed")
    need(retained_bytes(out) <= MAX_RETAINED_BEFORE_STAGE, "retained proof headroom exhausted")
    directory = out / item["name"]
    directory.mkdir()
    item.update(status="running", started_at=now(), host_before=host())
    item["background_before"] = background_window()
    argv = ["--record", str(directory / "supervisor.json"), "--cwd", str(cwd),
            "--disk-path", str(out), "--timeout-seconds", str(seconds)]
    for key, value in LIMITS.items():
        argv += ["--" + key.replace("_", "-"), str(value)]
    identities = [out / "plan.json", Path(__file__), SUPERVISOR, Path(item["command"][0])]
    for path in dict.fromkeys(identities):
        argv += ["--identity-file", str(path)]
    item["supervisor_argv"] = argv + ["--", *item["command"]]
    save(receipt_path, receipt)
    live(plan, seconds + 15)
    try:
        item["supervisor_exit"] = load_supervisor().main(item["supervisor_argv"])
    finally:
        item["ended_at"] = now()
        if (directory / "supervisor.json").exists():
            item["record"] = located(directory / "supervisor.json")
        save(receipt_path, receipt)
    need(item["supervisor_exit"] == 0, "bounded stage failed")
    record = read(directory / "supervisor.json")
    need(record["state"] == "completed" and record["child_exit_code"] == record["supervisor_exit_code"] == 0, "supervisor incomplete")
    need(record["identity_unchanged"] and record["identity_before"] == record["identity_after"], "stage identity changed")
    need(record["cleanup_complete"] and not record["forced"] and record["last_sample"]["pids"] == [], "stage cleanup incomplete")
    for payload in record["outputs"].values():
        need(pin(payload["path"]) == pair(payload), "supervisor raw output changed")
    item["process_seconds"] = record["elapsed_seconds"]
    item["root_os_peak_resident_bytes"] = record["last_sample"]["root_os_peak_resident_bytes"]
    item["root_os_peak_source"] = record["last_sample"]["root_os_peak_source"]
    item["background_after"] = background_window()
    item["host_after"] = host()
    live(plan)
    return directory


def gzip_verified(source, destination, expected):
    with Path(source).open("rb") as original, destination.open("xb") as raw:
        with gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as stream:
            shutil.copyfileobj(original, stream, 1024 * 1024)
    digest, size = hashlib.sha256(), 0
    with gzip.open(destination, "rb") as stream, Path(source).open("rb") as original:
        while block := stream.read(1024 * 1024):
            need(block == original.read(len(block)), "retained gzip does not reproduce source bytes")
            digest.update(block)
            size += len(block)
        need(original.read(1) == b"", "retained gzip is truncated")
    need({"bytes": size, "sha256": digest.hexdigest()} == pair(expected), "retained content pin differs")
    return located(destination)


def retain_state(out, plan, item, canonical, receipt, receipt_path):
    original = item["outputs"]["state.bin"]
    state = Path(original["path"])
    if canonical is None:
        compressed = out / "canonical" / (original["sha256"] + ".bin.gz")
        raw = Path(plan["workspace"]) / "canonical" / (original["sha256"] + ".bin")
        need(not compressed.exists() and not raw.exists(), "new canonical path already exists")
        archive = gzip_verified(state, compressed, original)
        canonical = {"state": {**original, "path": str(raw)}, "state_gzip": archive,
                     "quality": item["outputs"]["quality.json"]}
        item["state_retention"] = {"kind": "new_canonical", "original": original,
                                   "canonical": canonical, "gzip_fullbyte_verified": True, "raw_removed": False}
        save(receipt_path, receipt)  # durable proof before the raw file leaves proof/
        need(state.resolve().is_relative_to(out.resolve()) and raw.resolve().is_relative_to(Path(plan["workspace"]).resolve()), "canonical move outside owned roots")
        os.replace(state, raw)
    else:
        need(item["fullstate_and_quality_bytes_equal_canonical"], "duplicate has not been byte-compared")
        need(pin(canonical["state_gzip"]["path"]) == pair(canonical["state_gzip"]), "canonical gzip changed")
        item["state_retention"] = {"kind": "alias", "original": original, "canonical": canonical,
                                   "fullbyte_equal": True, "raw_removed": False}
        save(receipt_path, receipt)
        need(state.resolve().is_relative_to(out.resolve()), "duplicate removal outside proof")
        state.unlink()
    item["state_retention"]["raw_removed"] = True
    save(receipt_path, receipt)
    return canonical


def complete(item, receipt, path):
    item.update(status="completed", verified_at=now())
    save(path, receipt)
    print(json.dumps({"stage": item["name"], "status": "completed", "case": item.get("case"),
                      "arm": item.get("arm"), "workers": item.get("workers"),
                      "iterations": item.get("iterations"), "result": item.get("result")}), flush=True)


def solve(out, plan, binaries, item, receipt, path, canonical=None):
    for binary in binaries.values():
        need(pin(binary["path"]) == pair(binary), "built binary changed")
    item["command"] = [binaries[item["arm"]]["path"], item["case"], str(item["workers"]),
                       str(item["iterations"]), str(out / item["name"] / "artifacts")]
    directory = stage(out, plan, item, receipt, path, 120, out)
    common.output_checks(directory, item, canonical, plan["deadline_monotonic"])
    retained = retain_state(out, plan, item, canonical, receipt, path)
    live(plan)
    complete(item, receipt, path)
    return retained


def finish_manifest(out):
    # Retain failed/partial files too. The manifest is data, never imported code.
    files = {p.relative_to(out).as_posix(): pin(p) for p in sorted(out.rglob("*"))
             if p.is_file() and p.name != "retained.json"}
    save(out / "retained.json", {"schema": "r1.flat-ev-cloud32-retained/v1", "files": files,
                                "stored_bytes": sum(v["bytes"] for v in files.values())})


def execute(args):
    out = args.out.resolve(strict=True)
    plan = read(out / "plan.json")
    need(str(out) == plan["output"], "proof directory differs from prepared plan")
    filename = "build.json" if args.phase == "build" else "execution.json"
    destination = out / filename
    need(not destination.exists(), "phase was already attempted; no retry/resume")
    rows = ([{"name": "toolchain", "kind": "toolchain", "status": "pending"}]
            + [{"name": f"build-{arm}", "arm": arm, "kind": "build", "status": "pending"} for arm in ("baseline", "flat")]
            + [{"name": f"smoke-narrow-{arm}-{workers}", "kind": "smoke", "arm": arm, "case": "narrow",
                "workers": workers, "iterations": 2, "warmup": False, "round": None, "status": "pending"}
               for arm, workers in (("baseline", 1), ("flat", 1), ("baseline", 32), ("flat", 32))]
            if args.phase == "build" else matrix_rows())
    pilots = [] if args.phase == "build" else [
        {"name": f"pilot-{case}-{n}", "case": case, "arm": "baseline", "workers": 1,
         "iterations": n, "warmup": False, "round": None, "status": "pending"}
        for case in CASES for n in common.PILOT_ITERATIONS]
    receipt = {"schema": "r1.flat-ev-cloud32-phase/v1", "phase": args.phase, "status": "running",
               "plan": pin(out / "plan.json"), "started_at": now(), "stages": rows,
               "pilot_stages": pilots, "selected": {}, "binaries": {}}
    save(destination, receipt)
    current = None
    try:
        live(plan)
        for key in list(os.environ):
            if key.startswith("CARGO_PROFILE_") or key in {"CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTUP_TOOLCHAIN"}:
                os.environ.pop(key)
        os.environ.update(plan["environment"])
        if args.phase == "build":
            smoke_canonical = None
            for current in rows:
                kind = current["kind"]
                if kind == "smoke":
                    smoke_canonical = solve(out, plan, receipt["binaries"], current, receipt, destination, smoke_canonical)
                    continue
                if kind == "toolchain":
                    current["command"] = [plan["tools"]["rustc"]["path"], "-Vv"]
                    directory = stage(out, plan, current, receipt, destination, 10, out)
                    record = read(directory / "supervisor.json")
                    version = Path(record["outputs"]["stdout"]["path"]).read_text()
                    need("release: 1.97.0" in version and "host: x86_64-unknown-linux-gnu" in version, "native compiler version differs")
                    receipt["rustc_version"] = version
                else:
                    arm = current["arm"]
                    target = Path(plan["workspace"]) / f"{arm}-target"
                    need(not target.exists(), "build target must be absent and fresh")
                    current["command"] = [plan["tools"]["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2",
                                          "--target-dir", str(target), "-p", "holdem", "--example", "flop_cloud32_probe", "--message-format=json"]
                    stage(out, plan, current, receipt, destination, 300, Path(plan["sources"][arm]["path"]))
                    binary = located(target / "release/examples/flop_cloud32_probe")
                    current["artifact"] = binary
                    receipt["binaries"][arm] = binary
                    current["retained_binary"] = gzip_verified(binary["path"], out / f"binary-{arm}.gz", binary)
                complete(current, receipt, destination)
            receipt["smoke_canonical"] = smoke_canonical
            need(all(s["status"] == "completed" for s in rows), "build/smoke prerequisite incomplete")
        else:
            built = read(out / "build.json")
            need(built["status"] == "completed" and built["plan"] == receipt["plan"] and len(built["stages"]) == 7,
                 "both native builds and four smoke checks must precede measurements")
            need(all(s["status"] == "completed" for s in built["stages"]), "smoke checks incomplete")
            receipt["build"] = pin(out / "build.json")
            receipt["binaries"] = binaries = built["binaries"]
            for current in pilots:
                case = current["case"]
                if case in receipt["selected"]:
                    current.update(status="skipped", reason="baseline already selected smaller N")
                    continue
                canonical = solve(out, plan, binaries, current, receipt, destination)
                if common.pilot_done(current["iterations"], current["result"]["cfr_seconds"]):
                    receipt["selected"][case] = {"iterations": current["iterations"], "cfr_seconds": current["result"]["cfr_seconds"],
                                                "short_timing": current["result"]["cfr_seconds"] < 4,
                                                "canonical": canonical, "stage": current["name"]}
                    save(destination, receipt)
            need(set(receipt["selected"]) == set(CASES), "baseline-only pilot incomplete")
            for current in rows:
                need(pin(out / "build.json") == receipt["build"], "build receipt changed")
                selected = receipt["selected"][current["case"]]
                current["iterations"] = selected["iterations"]
                solve(out, plan, binaries, current, receipt, destination, selected["canonical"])
            need(len(rows) == 96 and all(s["status"] == "completed" for s in rows), "matrix incomplete")
        live(plan)
        receipt["status"] = "completed"
    except BaseException as error:
        receipt.update(status="failed", error=repr(error), traceback=traceback.format_exc())
        if current is not None and current["status"] not in {"completed", "skipped"}:
            current.update(status="failed", error=repr(error))
        for row in [*pilots, *rows]:
            if row["status"] == "pending":
                row.update(status="skipped", reason="stopped on first failure")
        raise
    finally:
        receipt["ended_at"] = now()
        receipt["counts"] = {state: sum(s["status"] == state for s in rows) for state in ("completed", "failed", "skipped")}
        receipt["pilot_counts"] = {state: sum(s["status"] == state for s in pilots) for state in ("completed", "failed", "skipped")}
        save(destination, receipt)
        try:
            finish_manifest(out)
        except BaseException as error:
            receipt.update(status="failed", retention_error=repr(error))
            save(destination, receipt)
            raise
        print(json.dumps({"phase": args.phase, "status": receipt["status"], "counts": receipt["counts"], "pilot_counts": receipt["pilot_counts"]}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "build", "matrix"))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--workspace", type=Path)
    parser.add_argument("--baseline-source", type=Path)
    parser.add_argument("--flat-source", type=Path)
    parser.add_argument("--cargo", type=Path)
    parser.add_argument("--rustc", type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args()
    if args.phase == "prepare":
        need(all(getattr(args, k) for k in ("workspace", "baseline_source", "flat_source", "cargo", "rustc", "deadline_utc")), "prepare arguments missing")
        prepare(args)
    else:
        execute(args)


if __name__ == "__main__":
    main()
