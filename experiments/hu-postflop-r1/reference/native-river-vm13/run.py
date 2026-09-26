"""Finite native diagnostics after the separate VM13 phase campaign is quiescent.

The caller owns cloud/unit lifecycle and must verify the prior unit has no live
processes. This script never builds, retries, changes input budgets, or executes
retained scripts. --control names the trusted deployment/checkout controls.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import signal
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
REVISION = "11e4062ba1735e58b60d12999cb23ed10fd1a163"
CASES = ("006", "022")
KINDS = ("validate", "solve", "tree", "audit")
LIMITS = {"timeout_seconds": 45, "memory_limit_bytes": 10737418240,
          "min_free_memory_bytes": 1073741824, "disk_reserve_bytes": 4294967296,
          "poll_seconds": 0.02, "grace_seconds": 5, "kill_wait_seconds": 5}


def require(ok, message):
    if not ok:
        raise ValueError(message)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def trusted(control):
    return load("native_river_trusted_phase", control / "current-phases/runner.py")


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def schedule():
    return [{"case": case, "kind": kind, "label": case + "-" + kind} for case in CASES for kind in KINDS]


def budget_end(started, work_deadline, now):
    require(started <= now < work_deadline, "invalid or expired work deadline")
    return min(started + 240, work_deadline)


def stage_fits(end, now):
    require(now + LIMITS["timeout_seconds"] + 20 < end, "full 45-second stage plus cleanup does not fit")


def command(plan, stage):
    case, kind = stage["case"], stage["kind"]
    root = PurePosixPath(plan["output"])
    run = root / "stages" / (case + "-solve") / "run"
    if kind == "validate":
        return [plan["cli"]["path"], "validate", plan["inputs"][case]["path"], "--format", "json",
                "--show-effective", "--write-effective", str(root / "stages" / stage["label"] / "effective.toml")]
    if kind == "solve":
        return [plan["cli"]["path"], "solve", plan["inputs"][case]["path"], "--out", str(run), "--sol-streets", "full"]
    if kind == "tree":
        return [plan["cli"]["path"], "export", str(run / "solution.sol"), "tree", "--node", "all", "--format", "json"]
    require(kind == "audit", "unknown native stage")
    return [plan["audit"]["path"], "--sol", str(run / "solution.sol"), "--threads", "1"]


def source_identity(core, source, expected):
    files = {}
    for path in sorted(Path(source).rglob("*")):
        require(not path.is_symlink(), "source symlink")
        if path.is_file():
            files[path.relative_to(source).as_posix()] = core.content(core.identity(path))
    require(files == expected, "plain source inventory changed")
    return files


def immutable_pins(plan):
    return [plan[key] for key in ("cli", "audit", "supervisor", "expectations", "prior_plan", "prior_result")] + [
        *plan["inputs"].values(), *plan["controls"].values()]


def live_identity(core, plan):
    for pin in immutable_pins(plan):
        require(core.identity(pin["path"]) == pin, "immutable input changed: " + pin["path"])
    machine = core.host({"limits": {"outer_memory_max_bytes": 12884901888}})
    require(machine == plan["host"], "host/boot/containment changed")
    source_identity(core, plan["source"], plan["source_files"])
    return {"host": machine, "source_inventory_sha256": plan["source_inventory_sha256"]}


def sample(core, checks, store, plan, stage, record):
    case, kind = stage["case"], stage["kind"]
    expected = store.json(plan["expectations"]["path"])["cases"][case]
    base = PurePosixPath(plan["output"]) / "stages"
    run = base / (case + "-solve") / "run"
    effective = store.data(str(base / (case + "-validate") / "effective.toml"))
    if kind == "validate":
        return checks.check_validate(store.json(record["outputs"]["stdout"]["path"]), effective,
                                     store.data(plan["inputs"][case]["path"]))
    live = store.json(str(run / "run.json"))
    if kind == "solve":
        progress = [core.decode(line) for line in store.data(str(run / "progress.jsonl")).splitlines() if line.strip()]
        result = checks.check_solve(live, store.json(str(run / "manifest.json")), progress,
                                    effective, store.data(str(run / "run.toml")))
        sol, ckpt = store.data(str(run / "solution.sol")), store.data(str(run / "checkpoint.ckpt"))
        for raw, magic, version in ((sol, b"SLVRSOLV", 4), (ckpt, b"SLVRCKPT", 2)):
            require(len(raw) >= 50 and raw[:8] == magic and int.from_bytes(raw[8:10], "little") == version,
                    "artifact format differs")
            require(int.from_bytes(raw[42:50], "little") == live["iterations"], "artifact iteration differs")
        require(sol[10:42] == ckpt[10:42] and sol[10:42].hex() == store.json(str(run / "manifest.json"))["configHash"],
                "artifact config hashes differ")
        result["artifacts"] = {name: store.pin(str(run / name)) for name in ("solution.sol", "checkpoint.ckpt", "run.toml")}
        return result
    report = store.json(record["outputs"]["stdout"]["path"])
    if kind == "tree":
        return checks.check_tree(report, expected)
    solpin = store.pin(str(run / "solution.sol"))
    solpin["config_blake3"] = store.data(solpin["path"])[10:42].hex()
    return checks.check_audit(report, live, expected, solpin)


def retain(stage_dir, store, phase, tools):
    # Retain both record-linked bytes and unmatched failure outputs.
    return phase.base.retain_stage(store, stage_dir, tools)


def run(args):
    started = time.time()
    monotonic_end = time.monotonic() + 240
    phase = trusted(args.control.resolve(strict=True))
    core = phase.core
    checks = load("native_river_checks", HERE / "check.py")
    out = args.out.resolve()
    require(not out.exists(), "new proof directory required")
    out.mkdir(parents=True)
    store = core.Store(out, create=True)
    state = {"schema": "r1.native-river-result/v1", "status": "running",
             "stages": [{"stage": st, "status": "pending"} for st in schedule()]}
    plan = None
    persist = lambda: core.save(out / "result.json", state)
    persist()
    try:
        prior_plan = core.read(args.phase_proof / "plan.json")
        prior_state = core.read(args.phase_proof / "result.json")
        require(prior_state["status"] in ("completed", "failed") and
                all(row["status"] in ("passed", "failed", "skipped") for row in prior_state["stages"]), "prior campaign is not terminal")
        require(args.work_deadline_utc == prior_plan["work_deadline_utc"], "original work deadline changed")
        end = budget_end(started, core.timestamp(args.work_deadline_utc), time.time())
        stage_fits(end, time.time())
        build = next(row for row in prior_state["stages"] if row["stage"]["label"] == "build-plain")
        require(build["status"] == "passed", "plain native build did not pass")
        for name, path in phase.controls().items():
            require(core.content(core.identity(path)) == core.content(prior_plan["controls"][name]), "trusted phase control changed")
        source = prior_plan["copies"]["plain"]
        source_identity(core, source["source"], source["files"])
        manifest = core.read(source["manifest"]["path"])
        require(manifest["source_revision"] == REVISION and manifest["mode"] == "plain", "wrong production source")
        require(out != Path(source["source"]) and not out.is_relative_to(Path(source["source"])), "proof inside source")
        for name in ("cli", "audit"):
            require(core.identity(prior_state["binaries"]["plain"][name]["path"]) == prior_state["binaries"]["plain"][name], "binary changed")
        tools = [prior_plan["python"], *prior_plan["tools"].values()]
        core.retain_record(store, build["record"]["path"], tools)
        core.record_bytes(store, build["record"], tools)
        for name in source["files"]:
            store.add(Path(source["source"]) / name)
        expected = core.read(HERE / "expectations.json")
        require(expected["schema"] == "r1.native-river-expectations/v1" and expected["source_revision"] == REVISION
                and set(expected["cases"]) == set(CASES), "unexpected fixtures")
        inputs = {}
        for case in CASES:
            inputs[case] = store.add(HERE / expected["cases"][case]["config_file"])
            require(core.content(inputs[case]) == core.content(expected["cases"][case]["config_pin"]), "diagnostic config changed")
        machine = core.host({"limits": {"outer_memory_max_bytes": 12884901888}})
        require(machine["boot_id"] == prior_plan["host"]["boot_id"], "boot changed after native build")
        controls = {name: store.add(HERE / name) for name in ("run.py", "check.py")}
        controls.update({"shared/" + name: store.add(path) for name, path in phase.controls().items()})
        plan = {"schema": "r1.native-river-plan/v1", "created_at": utc(), "started_epoch": started,
                "output": str(out), "deadline_epoch": end, "work_deadline_utc": args.work_deadline_utc,
                "scope": "native diagnostic inputs only; external conditions unverified; no external quality threshold",
                "prior_plan": store.add(args.phase_proof / "plan.json"), "prior_result": store.add(args.phase_proof / "result.json"),
                "prior_status": prior_state["status"], "plain_build_record": build["record"], "prior_tools": tools,
                "source": source["source"], "source_revision": REVISION, "source_files": source["files"],
                "source_inventory_sha256": hashlib.sha256(core.json.dumps(source["files"], sort_keys=True).encode()).hexdigest(),
                "cli": store.add(prior_state["binaries"]["plain"]["cli"]["path"]),
                "audit": store.add(prior_state["binaries"]["plain"]["audit"]["path"]),
                "inputs": inputs, "expectations": store.add(HERE / "expectations.json"), "controls": controls,
                "supervisor": store.add(prior_plan["supervisor"]["path"]), "python": core.identity(sys.executable),
                "environment": prior_plan["environment"], "host": machine, "limits": LIMITS,
                "caller_contract": "prior unit inactive, MainPID0 and empty cgroup; new bounded12GiB/swap0/CPUWeight100 unit; no concurrent experiment"}
        require(plan["python"] == prior_plan["python"], "Python differs from phase campaign")
        core.save(out / "plan.json", plan)
        store.add(out / "plan.json")
        supervisor = load("native_river_supervisor", Path(plan["supervisor"]["path"]))
        for entry in state["stages"]:
            stage = entry["stage"]
            stage_fits(end, time.time())
            require(time.monotonic() + 65 < monotonic_end, "240-second monotonic budget exhausted")
            entry.update(status="running", identity_before=live_identity(core, plan))
            persist()
            directory = out / "stages" / stage["label"]
            directory.mkdir(parents=True)
            pins = immutable_pins(plan) + [store.pin(str(out / "plan.json"))]
            if stage["kind"] in ("tree", "audit"):
                pins += [store.pin(str(out / "stages" / (stage["case"] + "-solve") / "run" / name))
                         for name in ("solution.sol", "checkpoint.ckpt", "run.toml")]
            entry["identity_pins"] = pins
            argv = ["--record", str(directory / "supervisor.json"), "--cwd", str(out), "--disk-path", str(out)]
            for key, value in LIMITS.items():
                argv += ["--" + key.replace("_", "-"), str(value)]
            for pin in pins:
                argv += ["--identity-file", pin["path"]]
            stage_fits(end, time.time())
            with phase.installed_environment(plan["environment"]):
                entry["supervisor_exit"] = supervisor.main([*argv, "--", *command(plan, stage)])
            retain(directory, store, phase, [plan["python"]])
            entry["record"] = store.add(directory / "supervisor.json")
            require(entry["supervisor_exit"] == 0, "native supervised stage failed")
            record = core.record_bytes(store, entry["record"], [plan["python"]])
            entry["identity_after"] = live_identity(core, plan)
            require(entry["identity_before"] == entry["identity_after"], "identity differs")
            entry["sample"] = sample(core, checks, store, plan, stage, record)
            entry["status"] = "passed"
            persist()
        require(time.time() <= end and time.monotonic() <= monotonic_end, "global diagnostic deadline exceeded")
        state.update(status="completed", completed_at=utc(), external_quality="not_evaluated")
        persist()
    except BaseException as error:
        phase.terminal_failure(state, error)
        for entry in state["stages"]:
            if entry["status"] == "failed":
                directory = out / "stages" / entry["stage"]["label"]
                try:
                    retain(directory, store, phase, [core.identity(sys.executable)])
                    if (directory / "supervisor.json").exists():
                        entry["record"] = store.add(directory / "supervisor.json", changed_identity=True)
                except (OSError, ValueError) as secondary:
                    entry["retention_error"] = repr(secondary)
        persist()
        raise
    finally:
        store.add(out / "result.json")


def check(args):
    phase = trusted(args.control.resolve(strict=True))
    core = phase.core
    checks = load("native_river_checks", HERE / "check.py")
    store = core.Store(args.out)
    state = core.read(args.out / "result.json")
    require(state["schema"] == "r1.native-river-result/v1" and state["status"] in ("completed", "failed"), "nonterminal result")
    require([row["stage"] for row in state["stages"]] == schedule(), "schedule changed")
    if not (args.out / "plan.json").exists():
        require(state["status"] == "failed", "missing successful plan")
        return {"schema": "r1.native-river-verification/v1", "status": "failed", "payload_integrity": "verified",
                "provenance_complete": False, "error": state["error"]}
    plan = core.read(args.out / "plan.json")
    for name in ("plan.json", "result.json"):
        require(store.data(str(PurePosixPath(plan["output"]) / name)) == (args.out / name).read_bytes(), "metadata bytes differ")
    require(plan["source_revision"] == REVISION and plan["limits"] == LIMITS, "source/limits differ")
    for pin in immutable_pins(plan):
        store.verify(pin)
    for name in ("run.py", "check.py"):
        require(core.content(plan["controls"][name]) == core.content(core.identity(HERE / name)), "trusted checker changed")
    require(core.content(plan["expectations"]) == core.content(core.identity(HERE / "expectations.json")), "trusted expectations differ")
    expected = store.json(plan["expectations"]["path"])
    for case in CASES:
        require(core.content(plan["inputs"][case]) == core.content(expected["cases"][case]["config_pin"]), "retained input changed")
    prior = store.json(plan["prior_plan"]["path"])
    prior_result = store.json(plan["prior_result"]["path"])
    require(prior_result["status"] == plan["prior_status"] in ("completed", "failed")
            and plan["work_deadline_utc"] == prior["work_deadline_utc"]
            and plan["source_files"] == prior["copies"]["plain"]["files"]
            and plan["source"] == prior["copies"]["plain"]["source"]
            and plan["cli"] == prior_result["binaries"]["plain"]["cli"]
            and plan["audit"] == prior_result["binaries"]["plain"]["audit"], "native build/source binding differs")
    core.host_record(plan["host"], {"limits": {"outer_memory_max_bytes": 12884901888}})
    require(plan["host"]["boot_id"] == prior["host"]["boot_id"], "build/diagnostic boot differs")
    core.record_bytes(store, plan["plain_build_record"], plan["prior_tools"])
    for name, pin in plan["source_files"].items():
        require(core.content(store.pin(str(PurePosixPath(plan["source"]) / name))) == pin, "retained source differs")
    require(plan["deadline_epoch"] == min(plan["started_epoch"] + 240, core.timestamp(plan["work_deadline_utc"])), "deadline extended")
    previous, failed = core.timestamp(plan["created_at"]), False
    for row in state["stages"]:
        if row["status"] != "passed":
            require(row["status"] in ("failed", "skipped") and (not failed or row["status"] == "skipped"), "invalid failure suffix")
            failed = True
            if "record" in row:
                core.record_bytes(store, row["record"], [plan["python"]], success=False)
            continue
        require(not failed and row["supervisor_exit"] == 0, "execution after failure")
        record = core.record_bytes(store, row["record"], [plan["python"]])
        require(record["argv"] == record["resolved_argv"] == command(plan, row["stage"])
                and record["cwd"] == plan["output"] and record["limits"] == LIMITS, "stage command/limits differ")
        created, ended = core.timestamp(record["created_at"]), core.timestamp(record["ended_at"])
        require(previous <= created and created + 65 < plan["deadline_epoch"] and ended <= plan["deadline_epoch"], "stage deadline/order differs")
        previous = ended
        require(row["identity_before"] == row["identity_after"] == {"host": plan["host"], "source_inventory_sha256": plan["source_inventory_sha256"]}, "source/host differs")
        require(row["sample"] == sample(core, checks, store, plan, row["stage"], record), "sample rederivation differs")
    require(state["status"] != "completed" or not failed, "incomplete success")
    if state["status"] == "completed":
        require(previous <= core.timestamp(state["completed_at"]) <= plan["deadline_epoch"], "completion deadline differs")
    return {"schema": "r1.native-river-verification/v1", "status": state["status"], "payload_integrity": "verified",
            "provenance_complete": True, "passed": sum(row["status"] == "passed" for row in state["stages"]),
            "external_quality": "not_evaluated", "acceptance": None, "error": state.get("error")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", choices=("run", "check"), required=True)
    parser.add_argument("--control", type=Path, default=HERE.parents[1])
    parser.add_argument("--phase-proof", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--work-deadline-utc")
    args = parser.parse_args()
    if args.phase == "check":
        print(json.dumps(check(args), indent=2, allow_nan=False))
    else:
        require(args.phase_proof and args.work_deadline_utc, "prior proof and original work deadline required")
        def interrupted(number, _frame):
            raise InterruptedError("diagnostic signal " + str(number))
        for number in (signal.SIGINT, signal.SIGTERM):
            signal.signal(number, interrupted)
        run(args)


if __name__ == "__main__":
    main()
