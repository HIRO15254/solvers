#!/usr/bin/env python3
"""Bounded Linux preparation of four pinned writer binaries; no cloud actions.

An owner resource/budget record and an already active finite systemd/cgroup v2
unit are prerequisites. This script grants no spending authority. Dependency
caches must be prepared beforehand: Cargo runs offline, with four fresh targets.
"""
from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("write_phase_build_runner", HERE / "run.py")
R = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(R)
LIMITS = {"campaign_seconds": 3000, "build_seconds": 600, "copy_seconds": 60, "tool_seconds": 15,
          "memory_bytes": 4 * 1024**3, "min_free_memory_bytes": 2 * 1024**3,
          "disk_reserve_bytes": 4 * 1024**3, "jobs": 2}
KEYS = ("baseline-plain", "baseline-instrumented", "candidate-plain", "candidate-instrumented")
EXAMPLE = "crates/formats/examples/sol_codec_bench.rs"


def require(ok, why):
    if not ok:
        raise ValueError(why)


def tool(path, name):
    path = path.resolve(strict=True)
    require(path.name == name and path.is_file() and os.access(path, os.X_OK), "actual toolchain executable required: " + name)
    with path.open("rb") as stream:
        require(stream.read(4) == b"\x7fELF", "ELF toolchain binary required; proxies/wrappers unsupported")
    result = R.identity(path)
    proxy = path.parent / "rustup"
    require(not proxy.is_file() or R.identity(proxy)["sha256"] != result["sha256"], "rustup hard-link proxy unsupported")
    return result


def events(group):
    return {name: dict(line.split() for line in (group / name).read_text().splitlines())
            for name in ("memory.events", "pids.events")}


def outer(owner):
    require(sys.platform == "linux", "build execution requires Linux cgroup v2")
    groups = [line[3:] for line in Path("/proc/self/cgroup").read_text().splitlines() if line.startswith("0::")]
    require(len(groups) == 1 and groups[0].startswith("/") and ".." not in Path(groups[0]).parts, "cgroup v2 required")
    group = Path("/sys/fs/cgroup") / groups[0].lstrip("/")
    controls = {key: (group / key).read_text().strip() for key in
                ("memory.max", "memory.swap.max", "pids.max", "cpu.max", "cpuset.cpus.effective")}
    require(controls["memory.max"].isdigit() and 0 < int(controls["memory.max"]) <= LIMITS["memory_bytes"]
            and controls["memory.swap.max"] == "0" and controls["pids.max"].isdigit()
            and 0 < int(controls["pids.max"]) <= 512, "finite build cgroup memory/swap/pids required")
    quota = controls["cpu.max"].split()
    require(len(quota) == 2 and all(x.isdigit() and int(x) > 0 for x in quota)
            and int(quota[0]) <= int(quota[1]) * os.cpu_count() and controls["cpuset.cpus.effective"], "finite build CPU quota required")
    require(re.fullmatch(r"[A-Za-z0-9_.@-]+\.service", owner["systemd_unit"]), "invalid build unit")
    props = "ControlGroup,ActiveState,KillMode,SendSIGKILL,RuntimeMaxUSec,TimeoutStopUSec"
    output = subprocess.check_output([owner["systemctl"]["path"], "show", owner["systemd_unit"], "--property=" + props], text=True, timeout=5)
    unit = dict(line.split("=", 1) for line in output.splitlines() if "=" in line)
    require(unit["ControlGroup"] == groups[0] and unit["ActiveState"] == "active"
            and unit["KillMode"] == "control-group" and unit["SendSIGKILL"] == "yes"
            and 0 < R.duration(unit["RuntimeMaxUSec"]) <= 3300
            and 0 < R.duration(unit["TimeoutStopUSec"]) <= 15, "actual bounded build unit missing")
    return {"cgroup_path": str(group), "controls": controls, "systemd": unit, "events": events(group)}


def unique(refs):
    result = {}
    for ref in refs:
        require(ref["path"] not in result or result[ref["path"]] == ref, "conflicting build input identities")
        result[ref["path"]] = ref
    return [result[key] for key in sorted(result)]


def verify_refs(refs):
    for ref in refs:
        R.verify(ref)


def source_refs(source, expected):
    actual = {p.relative_to(source).as_posix() for p in (source / "crates").rglob("*") if p.is_file()}
    wanted = {name for name in expected if name.startswith("crates/")}
    require(actual in (wanted, wanted | {EXAMPLE}), "source contains missing/extra crate inputs")
    require(not (source / ".cargo").exists(), "source Cargo config unsupported")
    result = []
    for name, pin in expected.items():
        path = source / name
        require(not path.is_symlink(), "source symlink unsupported")
        ref = R.identity(path)
        require(all(ref[key] == pin[key] for key in ("bytes", "sha256")), "source pin differs: " + name)
        result.append(ref)
    return result


def copy_refs(source, manifest):
    expected = set(manifest["after"]) | {"r1-write-phase-source.json", "instrumentation.patch"}
    actual = {p.relative_to(source).as_posix() for p in source.rglob("*") if p.is_file()}
    require(actual == expected, "generated source acquired unpinned files")
    refs = source_refs(source, manifest["after"])
    return refs + [R.identity(source / name) for name in ("r1-write-phase-source.json", "instrumentation.patch")]


def cargo_config_paths(config):
    cache = config["cargo_home"]
    paths = [cache / "config", cache / "config.toml"]
    for parent in (config["out"], *config["out"].parents):
        paths += [parent / ".cargo/config", parent / ".cargo/config.toml"]
    return paths


def controlled_environment(config):
    # Inherited shell flags/wrappers must not silently change one of four builds.
    forbidden = [key for key in os.environ if key == "R1_SOL_WRITE_PHASE_OUTPUT"
                 or (key.startswith("CARGO_") and key != "CARGO_HOME")
                 or (key.startswith("RUST") and key != "RUSTUP_HOME")]
    require(not forbidden, "clear inherited compiler/build overrides: " + ",".join(sorted(forbidden)))
    cache = config["cargo_home"]
    require(cache.is_dir(), "preseeded Cargo home required")
    require(not any(path.exists() for path in cargo_config_paths(config)), "unfrozen Cargo configuration unsupported")
    return {"RUSTC": config["rustc"]["path"], "RUSTFMT": config["rustfmt"]["path"],
            "RUSTFLAGS": config["rustflags"], "CARGO_INCREMENTAL": "0", "CARGO_BUILD_JOBS": str(LIMITS["jobs"]),
            "CARGO_HOME": str(cache), "CARGO_NET_OFFLINE": "true"}


@contextlib.contextmanager
def environment(values):
    previous = {key: os.environ.get(key) for key in values}
    os.environ.update(values)
    try:
        yield
    finally:
        for key, value in previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def supervise(config, stage, argv, cwd, refs, timeout):
    module = R.load(config["supervisor"]["path"], "write_phase_build_supervisor")
    args = ["--record", str(stage / "supervisor.json"), "--cwd", str(cwd),
            "--stdout", str(stage / "stdout.log"), "--stderr", str(stage / "stderr.log"),
            "--samples", str(stage / "supervisor.samples.jsonl"), "--disk-path", str(config["targets"]),
            "--timeout-seconds", str(timeout), "--grace-seconds", "5", "--kill-wait-seconds", "5", "--poll-seconds", "0.05",
            "--memory-limit-bytes", str(LIMITS["memory_bytes"]), "--min-free-memory-bytes", str(LIMITS["min_free_memory_bytes"]),
            "--disk-reserve-bytes", str(LIMITS["disk_reserve_bytes"])]
    for ref in refs:
        args += ["--identity-file", ref["path"]]
    return module.main([*args, "--", *argv])


def successful_record(stage, argv, cwd, required, timeout):
    ref = R.identity(stage / "supervisor.json")
    record = R.read(ref["path"])
    require(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
            and record["stop_reason"] == "completed" and record["errors"] == [] and record["shell"] is False
            and all(type(record[k]) is int and record[k] == 0 for k in ("child_exit_code", "supervisor_exit_code"))
            and record["cleanup_complete"] is True and record["identity_unchanged"] is True
            and record["forced"] is False and record["stop_requested_at"] is None
            and record["identity_before"] == record["identity_after"], "build stage failed or cleanup/input integrity unproved")
    require(record["argv"] == record["resolved_argv"] == argv and record["cwd"] == str(cwd), "build stage command/cwd differs")
    require(all(item in record["identity_before"] for item in required), "build stage missing frozen identities")
    require(record["limits"] == {"timeout_seconds": timeout, "grace_seconds": 5, "kill_wait_seconds": 5, "poll_seconds": 0.05,
                                 "memory_limit_bytes": LIMITS["memory_bytes"], "min_free_memory_bytes": LIMITS["min_free_memory_bytes"],
                                 "disk_reserve_bytes": LIMITS["disk_reserve_bytes"]}, "supervised build limits differ")
    for item in R.refs_in(record):
        R.verify(item)
    return ref


def execute(config, *, invoke=supervise, host=R.host, containment=outer, clock=time.monotonic):
    """Dependency seams are for small fake tests; CLI always uses real defaults."""
    out, targets = config["out"], config["targets"]
    require(not out.exists() and not targets.exists(), "build reports and target roots must both be fresh")
    require(not out.is_relative_to(targets) and not targets.is_relative_to(out), "durable reports and Cargo outputs overlap")
    for source in config["sources"].values():
        require(all(not path.is_relative_to(source) and not source.is_relative_to(path) for path in (out, targets)), "source/output overlap")
    out.mkdir(parents=True)
    targets.mkdir(parents=True)
    state = {"schema": "r1.write-phase-build/v1", "status": "running", "started_at": R.now(), "ended_at": None,
             "purpose": "prepare four writer binaries; no measurement or spending authority", "limits": LIMITS,
             "compiler": config["rustc"], "cargo": config["cargo"], "formatter": config["rustfmt"],
             "settings": {"profile": "release", "fresh_targets": True, "rustflags": config["rustflags"], "target": config["target"]},
             "copies": {}, "stages": [], "first_failure": None}
    deadline = clock() + LIMITS["campaign_seconds"]
    stage = None
    frozen = []
    try:
        R.write(out / "result.json", state)
        owner = R.read(config["owner"]["path"])
        require(owner["schema"] == "r1.write-phase-build-owner/v1" and owner["resources_authorized"] is True,
                "owner resource/budget preflight required")
        for key in ("budget_record", "systemctl"):
            R.verify(owner[key])
        require(0 < (R.instant(owner["deadline_utc"]) - dt.datetime.now(dt.timezone.utc)).total_seconds() <= 3300,
                "owner build window deadline must be finite and current")
        state["host"] = host()
        state["outer_before"] = containment(owner)
        env = controlled_environment(config)
        pins = R.read(HERE / "source-pins.json")["roles"]
        files = {key: R.identity(HERE / name) for key, name in R.TOOL_FILES.items()}
        files.update(driver=R.identity(__file__), protocol=R.identity(HERE / "protocol.json"),
                     selection=R.identity(HERE.parent / "inputs.json"), supervisor=config["supervisor"],
                     python=R.identity(sys.executable), owner=config["owner"], example=config["example"])
        frozen = list(files.values()) + [config[k] for k in ("rustc", "rustfmt", "cargo")] + [owner[k] for k in ("budget_record", "systemctl")]
        for role, source in config["sources"].items():
            expected = dict(pins[role]["files"])
            if (source / EXAMPLE).exists():
                expected[EXAMPLE] = pins["candidate"]["files"][EXAMPLE]
            frozen += source_refs(source, expected)
        selection = R.read(files["selection"]["path"])
        inputs = {case: R.identity(config["inputs"] / (case + ".sol")) for case in ("river", "turn", "flop")}
        for case, item in inputs.items():
            require(all(item[k] == selection["cases"][case][k] for k in ("bytes", "sha256")), "selected SOL input differs")
        frozen = unique(frozen + list(inputs.values()))
        freeze = {"schema": "r1.write-phase-build-freeze/v1", "issued_at": R.now(), "files": files, "inputs": inputs,
                  "identities": frozen, "host": state["host"], "environment": env, "settings": state["settings"],
                  "outer": state["outer_before"], "owner": config["owner"], "limits": LIMITS}
        R.write(out / "freeze.json", freeze)
        state["freeze"] = R.identity(out / "freeze.json")
        frozen = unique(frozen + [state["freeze"]])
        state["frozen_inputs"] = frozen

        def healthy():
            require(clock() < deadline and dt.datetime.now(dt.timezone.utc) < R.instant(owner["deadline_utc"]), "build campaign deadline expired")
            require(host() == state["host"] and containment(owner) == state["outer_before"], "CPU/boot/outer controls or resource events changed")
            verify_refs(frozen)

        def run_stage(label, argv, cwd, timeout, extra=()):
            nonlocal stage
            healthy()
            require(clock() + timeout + 11 < deadline
                    and dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=timeout + 11) < R.instant(owner["deadline_utc"]),
                    "not enough remaining time for fixed stage plus cleanup")
            stage = out / "stages" / (f'{len(state["stages"]):02d}-' + label)
            stage.mkdir(parents=True)
            refs = unique([*frozen, *extra])
            with environment(env):
                code = invoke(config, stage, argv, cwd, refs, timeout)
            require(type(code) is int and code == 0, "supervised stage returned failure: " + label)
            record = successful_record(stage, argv, cwd, refs, timeout)
            healthy()
            state["stages"].append({"label": label, "record": record})
            R.write(out / "result.json", state, exclusive=False)
            return stage, record

        for key, prefix, args in (("cargo", "cargo ", ["--version"]), ("rustc", "rustc ", ["--version", "--verbose"]),
                                  ("rustfmt", "rustfmt ", ["--version"])):
            directory, _ = run_stage(key + "-version", [config[key]["path"], *args], out, LIMITS["tool_seconds"])
            version = (directory / "stdout.log").read_text()
            require(version.startswith(prefix), "tool executable identity/version differs: " + key)
            if key == "rustc":
                require("host: " + config["target"] in version.splitlines(), "target must match the actual compiler host")
        for key in KEYS:
            role, mode = key.split("-", 1)
            source = out / "sources" / key
            argv = [files["python"]["path"], files["applier"]["path"], "--source", str(config["sources"][role]),
                    "--out", str(source), "--role", role, "--mode", mode, "--example", config["example"]["path"],
                    "--rustfmt", config["rustfmt"]["path"]]
            run_stage("copy-" + key, argv, out, LIMITS["copy_seconds"])
            manifest = R.read(source / "r1-write-phase-source.json")
            frozen = unique(frozen + copy_refs(source, manifest))
        for key in KEYS:
            source, target = out / "sources" / key, targets / key
            require(not target.exists(), "Cargo target was not fresh")
            healthy()
            manifest = R.identity(source / "r1-write-phase-source.json")
            before = host()
            build_env = {"schema": "r1.write-phase-build-environment/v1", "observed_at": R.now(), "host": before,
                         "cwd": str(source), "target_dir": str(target), "target_exists": False, "environment": env}
            env_path = out / (key + "-build-environment.json")
            R.write(env_path, build_env)
            env_ref = R.identity(env_path)
            argv = [config["cargo"]["path"], "build", "--release", "--locked", "-p", "formats", "--example", "sol_codec_bench",
                    "--target", config["target"], "--target-dir", str(target)]
            _, record = run_stage("build-" + key, argv, source, LIMITS["build_seconds"], (env_ref,))
            manifest_body = R.read(manifest["path"])
            verify_refs(copy_refs(source, manifest_body))
            binary = R.identity(target / config["target"] / "release/examples/sol_codec_bench")
            state["copies"][key] = {"source_manifest": manifest, "binary": binary, "build_record": record,
                                    "build_environment": env_ref, "argv": argv, "target_absent_before": True,
                                    "host_before": before, "host_after": host()}
            frozen = unique([*frozen, env_ref, binary, record])
            R.write(out / "result.json", state, exclusive=False)
        healthy()
        for entry in state["copies"].values():
            manifest = R.read(entry["source_manifest"]["path"])
            verify_refs(copy_refs(Path(manifest["output_path"]), manifest))
        state.update(status="completed", ended_at=R.now(), frozen_inputs=frozen, outer_after=containment(owner))
        # The public result stays running until the existing consumer accepts it.
        # A temporary candidate is removed on both validation outcomes.
        fd, temporary = tempfile.mkstemp(prefix=".validation-pending-", suffix=".json", dir=out)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                json.dump(state, stream, sort_keys=True, allow_nan=False)
            R.verify_build({"build_body": state, "build": R.identity(temporary), "host": state["host"],
                            "copies": state["copies"], "issued_at": R.now(), "files": files, "protocol": files["protocol"]})
        finally:
            Path(temporary).unlink()
        healthy()
        state.update(ended_at=R.now(), outer_after=containment(owner))
    except BaseException as error:
        state.update(status="failed", ended_at=R.now(), first_failure={"type": type(error).__name__, "reason": str(error),
                     "stage": str(stage) if stage else None, "available_stage_files": []})
        if stage:
            for path in sorted(stage.rglob("*")):
                if path.is_file():
                    try:
                        state["first_failure"]["available_stage_files"].append(R.identity(path))
                    except (OSError, ValueError):
                        pass
    finally:
        R.write(out / "result.json", state, exclusive=False)
    return state


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("baseline-source", "candidate-source", "example", "inputs", "cargo", "rustc", "rustfmt", "supervisor",
                 "cargo-home", "owner", "out", "targets"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--rustflags", default="-C target-cpu=native")
    args = parser.parse_args()
    require(sys.platform == "linux", "actual build driver is Linux-only")
    require(re.fullmatch(r"[A-Za-z0-9_.-]+", args.target), "invalid target triple")
    config = {"out": args.out.resolve(), "targets": args.targets.resolve(), "cargo_home": args.cargo_home.resolve(strict=True),
              "target": args.target, "rustflags": args.rustflags, "inputs": args.inputs.resolve(strict=True),
              "sources": {role: getattr(args, role + "_source").resolve(strict=True) for role in ("baseline", "candidate")}}
    config.update({key: tool(getattr(args, key), key) for key in ("cargo", "rustc", "rustfmt")})
    require(len({config[key]["sha256"] for key in ("cargo", "rustc", "rustfmt")}) == 3, "shared proxy binaries unsupported")
    config.update({key: R.identity(getattr(args, key)) for key in ("supervisor", "example", "owner")})
    require(shutil.disk_usage(config["out"].parent).free >= 10 * 1024**3
            and shutil.disk_usage(config["targets"].parent).free >= 10 * 1024**3, "need 10GiB free at reports and target roots")
    state = execute(config)
    print(json.dumps({"status": state["status"], "copies": len(state["copies"]), "first_failure": state["first_failure"]}))
    return 0 if state["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
