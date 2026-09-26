"""One-shot finite current-source phase campaign. Linux execution; no cloud lifecycle."""
from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
import importlib.util


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


base = load("current_phase_final", HERE.parent / "final-pipeline/run.py")
core = base.core
prep = load("current_phase_prepare", HERE / "prepare.py")
phase = load("current_phase_validator", HERE / "validate.py")
require, read, save, identity, content, join, timestamp = (
    getattr(core, key) for key in ("require", "read", "save", "identity", "content", "join", "timestamp"))
ARMS = ("plain", "off", "time", "memory")
CASES = ("river", "turn", "flop")
ORDER = (ARMS, ARMS, ("time", "memory", "plain", "off"), ("off", "plain", "memory", "time"))
BINS = {"cli": "release/solvers", "codec": "release/examples/current_phase_codec",
        "audit": "release/examples/hu_saved_profile_audit", "probe": "release/examples/hu_pipeline_probe"}


def schedule():
    rows = [{"kind": kind, "label": kind, "timeout": 30} for kind in
            ("rustc-version", "cargo-version", "cc-version", "compile-native", "compile-memory")]
    rows += [{"kind": "build", "arm": arm, "label": "build-" + arm, "timeout": 900}
             for arm in ("plain", "instrumented")]
    rows += [{"kind": "native-calibration", "label": "native-calibration", "timeout": 70},
             {"kind": "memory-calibration", "label": "memory-calibration", "timeout": 30}]
    solves = []
    for case in CASES:
        for block, arms in enumerate(ORDER):
            for arm in arms:
                solves.append({"case": case, "block": block, "warmup": block == 0, "arm": arm,
                               "kind": "solve", "label": f"{case}-b{block}-{arm}-solve", "timeout": 30})
    rows += solves
    for case in CASES:
        for kind in ("decode-all", "read-root"):
            for block, arms in enumerate(ORDER):
                for arm in arms:
                    rows.append({"case": case, "block": block, "warmup": block == 0, "arm": arm,
                                 "kind": kind, "label": f"{case}-b{block}-{arm}-{kind}", "timeout": 30})
    for row in solves:
        for kind in ("audit", "checkpoint", "stream-write"):
            rows.append({**row, "kind": kind, "label": row["label"].removesuffix("solve") + kind})
    return rows


def deadline_contract(created, work, stop, now):
    created, work, stop = map(timestamp, (created, work, stop))
    require(created <= now < work and stop > work, "deadline chronology invalid")
    require(stop - created <= 3600 and work - created <= 2700 and stop - work >= 900,
            "one-hour lifetime / 2700s work / 900s recovery bounds invalid")
    return {"created": created, "work": work, "stop": stop}


def systemd_seconds(text):
    units = {"us": .000001, "ms": .001, "s": 1, "min": 60, "h": 3600, "d": 86400, "w": 604800}
    pieces = text.split()
    require(bool(pieces), "empty systemd duration")
    result = 0.0
    for piece in pieces:
        match = re.fullmatch(r"([0-9]+(?:\.[0-9]+)?)(us|ms|min|s|h|d|w)", piece)
        require(match is not None, "unsupported systemd duration")
        result += float(match[1]) * units[match[2]]
    require(base.finite(result) and result > 0, "invalid systemd duration")
    return result


def stage_fits(deadline, seconds, now):
    require(now + seconds + 20 < timestamp(deadline), "full stage timeout plus 20s does not fit")


def stage_environment(plan, stage):
    env = dict(plan["environment"])
    if stage["kind"] in ("solve", "decode-all", "read-root") and stage["arm"] in ("time", "memory"):
        env.update(R1_CURRENT_PHASE_MODE=stage["arm"],
                   R1_CURRENT_PHASE_OUTPUT=join(plan["output"], "stages", stage["label"], "phase.json"))
    return env


@contextlib.contextmanager
def installed_environment(env):
    previous = dict(os.environ)
    try:
        # The complete child environment is pinned; inherited Cargo/Rust hooks
        # or profiler variables cannot silently alter a measured process.
        os.environ.clear()
        os.environ.update(env)
        yield
    finally:
        os.environ.clear()
        os.environ.update(previous)


def terminal_failure(state, error):
    state.update(status="failed", error=repr(error))
    state.pop("summary", None)
    for row in state["stages"]:
        if row["status"] == "running":
            row.update(status="failed", error=repr(error))
        elif row["status"] == "pending":
            row.update(status="skipped", reason="first failure; no retry")


def execute_sequence(state, execute, persist):
    """A testable first-failure boundary; semantic validation belongs inside execute."""
    try:
        for entry in state["stages"]:
            require(entry["status"] == "pending", "campaign cannot resume or retry")
            entry["status"] = "running"
            persist()
            execute(entry)
            entry["status"] = "passed"
            persist()
    except BaseException as error:
        terminal_failure(state, error)
        persist()
        raise


def controls():
    paths = {name: HERE / name for name in ("runner.py", "check_run.py", "prepare.py", "runtime.rs.in", "source-pins.json",
             "protocol.json", "codec-input.rs", "validate.py", "memory_probe.c", "freeze.json")}
    paths.update({"final/" + name: HERE.parent / "final-pipeline" / name for name in
                  ("run.py", "protocol.json", "hu_pipeline_probe.rs")})
    paths.update({"memory/" + name: HERE.parent / "focused-memory" / name for name in
                  ("native_rss.c", "calibration.py", "protocol.json")})
    paths.update({"shared/showdown.py": base.SHARED, "shared/exact.py": base.EXACT})
    paths.update({"configs/" + case + ".toml": HERE.parent / "final-pipeline/configs" / (case + ".toml") for case in CASES})
    return paths


def host(plan):
    value = core.host({"limits": {"outer_memory_max_bytes": 12884901888}})
    cg = Path(value["cgroup"]["path"])
    value["cgroup"]["cpu_weight"] = (cg / "cpu.weight").read_text().strip()
    require(value["cgroup"]["cpu_weight"] == "100", "CPUWeight must be 100")
    value["kernel"] = Path("/proc/sys/kernel/osrelease").read_text().strip()
    value["cpu_flags"] = sorted(set(line.split(":", 1)[1].strip() for line in
        Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("flags")))
    unit = plan["launch"]["unit"]
    require(unit and all(c.isalnum() or c in "-_.@" for c in unit), "invalid unit")
    result = subprocess.run(["/usr/bin/systemctl", "show", unit, "--no-pager",
        "--property=ControlGroup,KillMode,SendSIGKILL,CPUWeight,MemoryMax,MemorySwapMax,RuntimeMaxUSec,ActiveState"],
        capture_output=True, timeout=3, check=True)
    fields = dict(line.split("=", 1) for line in result.stdout.decode().splitlines())
    require(fields["ControlGroup"] == str(cg).removeprefix("/sys/fs/cgroup")
            and fields["KillMode"] == "control-group" and fields["SendSIGKILL"] == "yes"
            and fields["CPUWeight"] == "100" and fields["MemoryMax"] == "12884901888"
            and fields["MemorySwapMax"] == "0" and fields["ActiveState"] == "active", "outer unit containment invalid")
    require(fields["RuntimeMaxUSec"] == plan["launch"]["runtime_max_systemd"], "unit runtime differs from launch")
    value["unit"] = fields
    return value


def inventory(plan):
    result = {}
    for arm, info in plan["copies"].items():
        root = Path(info["source"])
        actual = {}
        for p in sorted(root.rglob("*")):
            require(not p.is_symlink(), "source symlink")
            if p.is_file():
                actual[p.relative_to(root).as_posix()] = content(identity(p))
        require(actual == info["files"], "source changed: " + arm)
        result[arm] = base.stable(actual)
    return result


def common_pins(plan, state):
    return [*plan["controls"].values(), *plan["inputs"].values(), plan["supervisor"], plan["python"],
            *plan["tools"].values(), plan["launch_record"], *plan["reference"].values(),
            *[c["manifest"] for c in plan["copies"].values()],
            *[pin for bins in state["binaries"].values() for pin in bins.values()], *state["native_binaries"].values()]


def live_identity(plan, state):
    for pin in common_pins(plan, state):
        require(identity(pin["path"]) == pin, "input/tool/binary changed: " + pin["path"])
    value = {"host": host(plan), "sources": inventory(plan)}
    require(value["host"] == plan["host"], "host/boot/containment changed")
    return value


def solve_dir(plan, stage):
    return join(plan["output"], "stages", f"{stage['case']}-b{stage['block']}-{stage['arm']}-solve", "run")


def input_stage(stage):
    return {**stage, "arm": "plain", "block": 1} if stage["kind"] in ("decode-all", "read-root") else stage


def child_command(plan, state, stage):
    kind = stage["kind"]
    directory = join(plan["output"], "stages", stage["label"])
    if kind.endswith("-version"):
        tool = kind.removesuffix("-version")
        return [plan["tools"][tool]["path"], "-Vv" if tool == "rustc" else "--version"]
    if kind in ("compile-native", "compile-memory"):
        control = "memory/native_rss.c" if kind == "compile-native" else "memory_probe.c"
        name = "native-rss" if kind == "compile-native" else "memory-probe"
        return [plan["tools"]["cc"]["path"], "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                plan["controls"][control]["path"], "-o", join(plan["output"], name)]
    if kind == "build":
        return [plan["tools"]["cargo"]["path"], "build", "--locked", "--offline", "--release", "--jobs", "2",
                "-p", "cli", "--bin", "solvers", "--example", "current_phase_codec", "--example", "hu_saved_profile_audit",
                "--example", "hu_pipeline_probe", "--target-dir", plan["copies"][stage["arm"]]["target"]]
    if kind == "native-calibration":
        return [plan["python"]["path"], "-B", plan["controls"]["memory/calibration.py"]["path"], "--launcher",
                state["native_binaries"]["native-rss"]["path"], "--out", join(directory, "calibration")]
    if kind == "memory-calibration":
        return [state["native_binaries"]["memory-probe"]["path"], join(directory, "calibration.json")]
    bins = state["binaries"]["plain" if stage["arm"] == "plain" else "instrumented"]
    run = solve_dir(plan, input_stage(stage))
    if kind == "solve":
        return [bins["cli"]["path"], "solve", plan["inputs"][stage["case"]]["path"], "--out", run, "--sol-streets", "full"]
    if kind == "audit":
        return [bins["audit"]["path"], "--sol", join(run, "solution.sol"), "--threads", "1"]
    if kind == "checkpoint":
        return [bins["probe"]["path"], "--mode", "checkpoint", "--checkpoint", join(run, "checkpoint.ckpt"),
                "--out", join(directory, "probe")]
    require(kind in ("decode-all", "read-root", "stream-write"), "unknown stage")
    return [bins["codec"]["path"], join(run, "solution.sol"), kind, "1", join(directory, "codec")]


def command(plan, state, stage):
    child = child_command(plan, state, stage)
    if stage["kind"] in ("solve", "decode-all", "read-root"):
        return [state["native_binaries"]["native-rss"]["path"], "--report",
                join(plan["output"], "stages", stage["label"], "native.json"), "--", *child]
    return child


def stage_cwd(plan, stage):
    return plan["copies"][stage["arm"]]["source"] if stage["kind"] == "build" else plan["output"]


def supervisor_limits(stage):
    return {"timeout_seconds": stage["timeout"], "memory_limit_bytes": 10737418240,
            "min_free_memory_bytes": 1073741824, "disk_reserve_bytes": 4294967296,
            "poll_seconds": 0.02, "grace_seconds": 5, "kill_wait_seconds": 5}


def check_memory_calibration(value):
    require(value["schema"] == "r1.current-phases-memory-calibration/v1" and value["passed"] is True
            and value["reset_value"] == 5, "reset calibration failed")
    s = value["snapshots"]
    require(set(s) == {"history128", "after_release", "small_start", "small_end", "large_start", "large_end", "final_reset"}, "reset snapshots incomplete")
    for name, snapshot in s.items():
        phase.snapshot(snapshot, name)
    h = lambda name: s[name]["hwm_kib"]
    require(h("history128") >= 112*1024 and h("history128")-h("small_start") >= 96*1024
            and h("small_end") <= h("small_start")+8*1024 and h("large_end") >= h("large_start")+48*1024
            and h("large_end")-h("small_end") >= 32*1024 and h("large_end")-h("final_reset") >= 32*1024,
            "reset calibration thresholds failed")
    return value


def check_native_calibration(store, directory, native):
    value = store.json(join(directory, "calibration", "result.json"))
    require(value["schema"] == "r1.native-rss-calibration/v1" and value["status"] == "passed"
            and value["launcher"] == native and value["parent_allocation_bytes"] == 268435456
            and 0 < value["elapsed_seconds"] < 60 and value["parent_vmrss_kib"] >= 204800
            and value["parent_vmrss_kib_after"] >= 204800, "native calibration invalid")
    require([r["case"] for r in value["cases"]] == ["small", "large"], "native calibration sequence")
    peaks = []
    for row, size in zip(value["cases"], (1048576, 67108864)):
        argv = [native["path"], "--allocate", str(size)]
        output = join(directory, "calibration", row["case"] + ".native.json")
        require(row["allocation_bytes"] == size and row["command"] == [native["path"], "--report", output, "--", *argv]
                and row["launcher_returncode"] == 0 and row["parent_vmrss_kib_before"] >= 204800, "native calibration command/parent")
        for key in ("native_report", "stdout", "stderr"):
            store.verify(row[key])
        r = store.json(output)
        require(r == row["native"] and r["schema"] == "r1.native-rss/v1" and r["source"] == "wait4.ru_maxrss_linux_kib"
                and type(r["child_pid"]) is int and r["child_pid"] > 0 and type(r["ru_maxrss_kib"]) is int
                and r["argv"] == argv and r["exit_code"] == 0 and r["signaled"] is False
                and r["term_signal"] is None and 0 < r["launcher_before_fork"]["vmrss_kib"] < 16384
                and 0 < r["elapsed_seconds"] <= row["elapsed_seconds"] <= value["elapsed_seconds"], "native calibration child")
        peaks.append(r["ru_maxrss_kib"])
    require(0 < peaks[0] < 32768 and 49152 <= peaks[1] <= 98304 and peaks[1]-peaks[0] > 32768,
            "native small/large counter isolation failed")
    return value


def sample_plan(plan):
    protocol = dict(plan["quality_protocol"])
    protocol["versions"] = {arm: {"sol": 4, "checkpoint": 2} for arm in ARMS}
    return {"output": plan["output"], "protocol": protocol}


def verify_stage(store, plan, state, entry):
    """Portable derivation from retained bytes. No live files, process, or retained imports."""
    stage = entry["stage"]; kind = stage["kind"]
    record = core.record_bytes(store, entry["record"], [plan["python"], *plan["tools"].values()])
    require(entry["supervisor_exit"] == 0 and record["argv"] == record["resolved_argv"] == command(plan, state, stage)
            and record["cwd"] == stage_cwd(plan, stage), "stage invocation differs")
    require(entry["environment"] == stage_environment(plan, stage), "stage environment differs")
    require(all(pin in record["identity_before"] for pin in entry["identity_pins"]), "supervisor input pins absent")
    require(all(record["limits"][k] == v for k, v in supervisor_limits(stage).items()), "supervisor limits differ")
    require(record["measurement"]["max_sample_gap_seconds"] <= 1, "sample polling gap exceeded")
    expected_identity = {"host": plan["host"], "sources": {k: base.stable(v["files"]) for k,v in plan["copies"].items()}}
    require(entry["identity_before"] == entry["identity_after"] == expected_identity, "stage host/source changed")
    require(timestamp(record["created_at"])+stage["timeout"]+20 < timestamp(plan["work_deadline_utc"])
            and timestamp(record["ended_at"]) <= timestamp(plan["work_deadline_utc"]), "stage deadline invalid")
    value = {"seconds": record["elapsed_seconds"]}
    directory = join(plan["output"], "stages", stage["label"])
    if kind.endswith("-version"):
        text = store.data(record["outputs"]["stdout"]["path"]).decode()
        if kind == "rustc-version":
            require("release: 1.97.0\n" in text and "host: x86_64-unknown-linux-gnu\n" in text, "Rust toolchain differs")
        if kind == "cargo-version":
            require(text.startswith("cargo 1.97.0 "), "Cargo toolchain differs")
        return value
    if kind in ("build", "compile-native", "compile-memory"):
        return value
    if kind == "native-calibration":
        return check_native_calibration(store, directory, state["native_binaries"]["native-rss"])
    if kind == "memory-calibration":
        return check_memory_calibration(store.json(join(directory, "calibration.json")))
    adapted = input_stage(stage)
    value = base.sample(store, sample_plan(plan), adapted, record)
    if kind in ("solve", "decode-all", "read-root"):
        native = store.json(join(directory, "native.json"))
        require(native["schema"] == "r1.native-rss/v1" and native["source"] == "wait4.ru_maxrss_linux_kib"
                and native["argv"] == child_command(plan, state, stage) and native["exit_code"] == 0
                and native["signaled"] is False and native["term_signal"] is None and native["child_pid"] > 0
                and native["child_pid"] != record["pid"] and type(native["ru_maxrss_kib"]) is int
                and native["ru_maxrss_kib"] > 0 and 0 < native["elapsed_seconds"] <= record["elapsed_seconds"], "native sample invalid")
        samples = [core.decode(line) for line in store.data(record["outputs"]["samples"]["path"]).splitlines() if line.strip()]
        require(all(set(row["pids"]) <= {native["child_pid"],record["pid"]} for row in samples), "unexpected measured child")
        value["native"] = native
        if stage["arm"] in ("time", "memory"):
            value["phase"] = phase.validate(store.json(join(directory, "phase.json")),
                store.json(plan["copies"]["instrumented"]["manifest"]["path"]), kind)
            require(value["phase"]["mode"] == stage["arm"], "phase mode differs")
        else:
            require(join(directory, "phase.json") not in store.entries, "plain/off published phase")
    return value


def signature(kind, value):
    if kind == "solve":
        return {"live": value["live"], "trajectory": value["trajectory"],
                "checkpoint": content(value["artifacts"]["checkpoint.ckpt"]), "config": content(value["artifacts"]["run.toml"])}
    if kind == "audit":
        return {key: value[key] for key in ("quality", "pre_save", "economics")}
    if kind == "checkpoint":
        return {"arrays": value["arrays"], "state": content(value["state"])}
    if kind == "stream-write":
        return value["canonical"]
    raise ValueError("unknown signature")


def compare_completed(store, plan, state, complete=True):
    passed = {r["stage"]["label"]: r for r in state["stages"] if r["status"] == "passed"}
    reference = store.json(plan["reference"]["result.json"]["path"])
    prior = {r["stage"]["label"]: r["sample"] for r in reference["stages"] if r["status"] == "passed"}
    first = {}
    for row in passed.values():
        st = row["stage"]; kind = st["kind"]
        if kind not in ("solve", "audit", "checkpoint", "stream-write", "decode-all", "read-root"):
            continue
        case = st["case"]; value = row["sample"]
        if kind in ("solve", "audit", "checkpoint", "stream-write"):
            sig = signature(kind,value); key=(case,kind)
            require(key not in first or first[key] == sig, "same-source exactness failed: "+str(key))
            first[key]=sig
            require(sig == signature(kind,prior[f"{case}-b0-new-{kind}"]), "proof02 machine-independent fields differ: "+str(key))
            if kind in ("audit", "stream-write"):
                solve = passed[f"{case}-b{st['block']}-{st['arm']}-solve"]["sample"]
                live = solve["live"]
                meta = value["pre_save"] if kind == "audit" else {k:v for k,v in value["metadata"]["meta"].items() if k != "wall_secs"}
                require(meta["iterations"] == live["iterations"] and meta["nash_conv"] == live["nashConv"]
                        and meta["expl"] == [live["explP0"],live["explP1"]], "saved/live metadata differs")
                if kind == "stream-write":
                    audit = passed[f"{case}-b{st['block']}-{st['arm']}-audit"]["sample"]
                    require(meta == audit["pre_save"], "codec/audit metadata differs")
        else:
            baseline = passed.get(f"{case}-b1-plain-stream-write")
            if baseline:
                other=baseline["sample"]
                require(value["canonical"]["root_canonical"] == other["canonical"]["root_canonical"], "codec root canonical differs")
                if kind == "decode-all":
                    require(value["canonical"]["canonical"] == other["canonical"]["canonical"], "codec full canonical differs")
            else:
                require(not complete,"missing codec quality bridge")
    if complete:
        require(len(passed)==len(schedule()), "all stages must pass before completion")
    return first


def summary(state):
    answer={}
    for case in CASES:
        answer[case]={}
        for kind in ("solve","decode-all","read-root"):
            rows=[r for r in state["stages"] if r["stage"].get("case")==case and r["stage"]["kind"]==kind and not r["stage"]["warmup"]]
            timing=phase.calibration({arm:[r["sample"]["native"]["elapsed_seconds"] for r in rows if r["stage"]["arm"]==arm] for arm in ("plain","off","time")})
            plain=max(r["sample"]["native"]["ru_maxrss_kib"] for r in rows if r["stage"]["arm"]=="plain")
            memory=max(max(leaf["memory_peak_kib"] for leaf in r["sample"]["phase"]["leaves"].values()) for r in rows if r["stage"]["arm"]=="memory")
            answer[case][kind]={"timing":timing,"phase_memory_counter_screen":{"max_plain_native_kib":plain,
                "max_memory_leaf_kib":memory,"eligible_descriptive_only":memory<=plain+max(1024,plain*.05)},
                "memory_claim":"instrumented Linux counter only; not a physical bound"}
    return answer


def prepare(args, out, store):
    launch=read(args.launch_record)
    require(launch["schema"]=="r1.current-phases-launch/v1", "launch schema")
    deadline_contract(launch["instance_created_utc"],args.work_deadline_utc,args.stop_deadline_utc,time.time())
    require(launch["work_deadline_utc"]==args.work_deadline_utc and launch["stop_deadline_utc"]==args.stop_deadline_utc
            and 0 < launch["runtime_max_seconds"] <= timestamp(args.work_deadline_utc)-timestamp(launch["unit_started_utc"]),"launch deadlines/runtime differ")
    require(abs(systemd_seconds(launch["runtime_max_systemd"])-launch["runtime_max_seconds"]) <= .000001,
            "systemd textual/numeric runtime differ")
    frozen=read(HERE/"freeze.json")
    for name,pin in {**frozen["files"],**frozen["external_inputs"]}.items():
        require(content(identity(HERE/name))==pin,"frozen preparation changed: "+name)
    protocol=read(HERE/"protocol.json")
    workspace=args.workspace.resolve()
    require(not workspace.exists(),"new work root required")
    require(not workspace.is_relative_to(out) and not out.is_relative_to(workspace),"work/proof roots overlap")
    require(not workspace.is_relative_to(args.source.resolve()) and not args.source.resolve().is_relative_to(workspace),"work/source roots overlap")
    workspace.mkdir(parents=True)
    copies={}
    for arm in ("plain","instrumented"):
        source=workspace/"sources"/arm
        manifest=prep.create_copy(args.source,source,arm)
        helper=source/"crates/cli/examples/hu_pipeline_probe.rs"
        with helper.open("xb") as stream:stream.write((HERE.parent/"final-pipeline/hu_pipeline_probe.rs").read_bytes())
        files={p.relative_to(source).as_posix():content(identity(p)) for p in source.rglob("*") if p.is_file()}
        expected={**manifest["after"],"crates/cli/examples/hu_pipeline_probe.rs":content(identity(helper)),
                  "source-copy.json":content(identity(source/"source-copy.json")),"instrumentation.patch":content(identity(source/"instrumentation.patch"))}
        require(files==expected,"unexpected source overlay")
        for p in source.rglob("*"):
            if p.is_file():store.add(p)
        copies[arm]={"source":str(source),"target":str(workspace/"targets"/arm),"manifest":store.pin(str(source/"source-copy.json")),"files":files}
    ctrl={name:store.add(p) for name,p in controls().items()}
    reference={}
    expected_refs=read(HERE.parent/"focused-memory/protocol.json")["reference_files"]
    for name,pin in expected_refs.items():
        require(content(identity(args.reference_proof/name))==pin,"proof02 pin differs")
        reference[name]=store.add(args.reference_proof/name)
    environment={**base.FIXED_ENV,"RUSTC":str(args.rustc.resolve(strict=True)),"CARGO_NET_OFFLINE":"true",
        "CARGO_HOME":str(args.cargo_home.resolve(strict=True)),"RUSTUP_HOME":str(args.rustup_home.resolve(strict=True)),
        "PATH":str(args.rustc.resolve().parent)+os.pathsep+"/usr/bin:/bin",
        "HOME":str(Path.home()),"TMPDIR":"/tmp","LANG":"C.UTF-8","LC_ALL":"C.UTF-8","TZ":"UTC"}
    plan={"schema":"r1.current-phases-plan/v1","created_at":dt.datetime.now(dt.timezone.utc).isoformat(),
        "output":str(out),"workspace":str(workspace),"protocol":protocol,"quality_protocol":read(HERE.parent/"final-pipeline/protocol.json"),
        "work_deadline_utc":args.work_deadline_utc,"stop_deadline_utc":args.stop_deadline_utc,
        "launch":launch,"launch_record":store.add(args.launch_record),"copies":copies,"controls":ctrl,"reference":reference,
        "inputs":{case:ctrl["configs/"+case+".toml"] for case in CASES},"supervisor":store.add(args.supervisor),
        "python":identity(sys.executable),"tools":{name:identity(getattr(args,name)) for name in ("rustc","cargo","cc")},"environment":environment}
    for name in ("config","config.toml"):
        path=args.cargo_home/name
        require(not path.exists(),"external Cargo home config not allowed")
    plan["host"]=host(plan)
    require(plan["host"]["boot_id"]==launch["boot_id"],"launch boot differs")
    save(out/"plan.json",plan);store.add(out/"plan.json")
    return plan


def run(args):
    out=args.out.resolve();require(not out.exists(),"new experiment root required")
    source=args.source.resolve(strict=True)
    require(not out.is_relative_to(source) and not source.is_relative_to(out),"experiment/source overlap")
    out.mkdir(parents=True);store=core.Store(out,create=True)
    plan=None
    state={"schema":"r1.current-phases-result/v1","status":"running","binaries":{},"native_binaries":{},
           "stages":[{"stage":s,"status":"pending"} for s in schedule()]}
    persist=lambda:save(out/"result.json",state)
    persist()
    try:
        plan=prepare(args,out,store)
        supervisor=load("current_phase_supervisor",plan["supervisor"]["path"])
        def execute(entry):
            st=entry["stage"];directory=out/"stages"/st["label"]
            stage_fits(plan["work_deadline_utc"],st["timeout"],time.time())
            entry["identity_before"]=live_identity(plan,state)
            entry["environment"]=stage_environment(plan,st)
            if st["kind"]=="build":require(not Path(plan["copies"][st["arm"]]["target"]).exists(),"target must be fresh")
            directory.mkdir(parents=True,exist_ok=False)
            pins=common_pins(plan,state)+[store.pin(str(out/"plan.json"))]
            if st["kind"] in ("audit","checkpoint","stream-write","decode-all","read-root"):
                pins += [store.pin(join(solve_dir(plan,input_stage(st)),n)) for n in ("solution.sol","checkpoint.ckpt","run.toml")]
            entry["identity_pins"]=pins
            argv=["--record",str(directory/"supervisor.json"),"--cwd",stage_cwd(plan,st),"--disk-path",str(out)]
            for k,v in supervisor_limits(st).items():argv += ["--"+k.replace("_","-"),str(v)]
            for pin in pins:argv += ["--identity-file",pin["path"]]
            stage_fits(plan["work_deadline_utc"],st["timeout"],time.time());persist()
            with installed_environment(entry["environment"]):
                entry["supervisor_exit"]=supervisor.main([*argv,"--",*command(plan,state,st)])
            base.retain_stage(store,directory,[plan["python"],*plan["tools"].values()])
            entry["record"]=store.add(directory/"supervisor.json")
            require(entry["supervisor_exit"]==0,"supervised stage failed")
            if st["kind"]=="build":
                state["binaries"][st["arm"]]={key:store.add(Path(plan["copies"][st["arm"]]["target"])/rel) for key,rel in BINS.items()}
            elif st["kind"] in ("compile-native","compile-memory"):
                name="native-rss" if st["kind"]=="compile-native" else "memory-probe"
                state["native_binaries"][name]=store.add(out/name)
            entry["identity_after"]=live_identity(plan,state)
            entry["sample"]=verify_stage(store,plan,state,entry)
            # Include the current successful stage when checking prefix invariants.
            entry["status"]="passed"
            try:compare_completed(store,plan,state,complete=False)
            finally:entry["status"]="running"
            print(core.json.dumps({"stage":st["label"],"status":"passed"}),flush=True)
        execute_sequence(state,execute,persist)
        compare_completed(store,plan,state)
        report=summary(state)
        require(time.time() <= timestamp(plan["work_deadline_utc"]),"global deadline exhausted during final quality checks")
        state.update(status="completed",summary=report,completed_at=dt.datetime.now(dt.timezone.utc).isoformat())
        persist()
    except BaseException as error:
        terminal_failure(state,error)
        for entry in state["stages"]:
            if entry["status"]=="failed":
                directory=out/"stages"/entry["stage"]["label"]
                try:
                    identity_only=[plan["python"],*plan["tools"].values()] if plan else [identity(sys.executable)]
                    base.retain_stage(store,directory,identity_only)
                    if (directory/"supervisor.json").exists():entry["record"]=store.add(directory/"supervisor.json",changed_identity=True)
                except (OSError,ValueError) as retained:entry["retention_error"]=repr(retained)
        persist()
        raise
    finally:
        store.add(out/"result.json")


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ("source","workspace","out","reference-proof","cargo","rustc","cc","supervisor","cargo-home","rustup-home","launch-record"):
        parser.add_argument("--"+name,type=Path,required=True)
    for name in ("work-deadline-utc","stop-deadline-utc"):
        parser.add_argument("--"+name,required=True)
    args=parser.parse_args()
    previous={}
    def interrupted(number, _frame):
        raise InterruptedError("campaign interrupted by signal " + str(number))
    try:
        for number in (signal.SIGINT,signal.SIGTERM):
            previous[number]=signal.signal(number,interrupted)
        run(args)
    finally:
        for number,handler in previous.items():
            signal.signal(number,handler)


if __name__=="__main__":
    main()
