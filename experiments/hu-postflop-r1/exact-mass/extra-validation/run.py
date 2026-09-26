#!/usr/bin/env python3
"""Four finite ignored tests on fixed candidate04 after its full validation.

Uses the same existing source/target. No retry and no performance interpretation.
"""
from __future__ import annotations
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
SHARED = HERE.parent.parent / "showdown-kernel/run.py"
SHARED_SHA = "6d21fe34f9f66327bb7f3ff1c50d977687018af45c3593d56726f78e8da44a60"
MANIFEST_SHA = "1a84947f6daa9ca1c57d5f48e4914e176643dbe9c787262ded95dcf6aba042d6"
ARCHIVE_SHA = "51068bb8464b57b91d11fe1ce1be848f14cd83e26865661afeeee3c70a3e9cd7"
SUPERVISOR_SHA = "5bd46e106bbc48e971c43c1ea080ed16e22356e83747ec6fb57157013fd029b8"
TESTS = [
    ("postflop", "iso_quotient_matches_full_tree_per_hand"),
    ("postflop", "member_branch_matches_suit_permuted_rep_branch"),
    ("postflop", "i16_storage_matches_f32_on_small_turn_spot"),
    ("rake_icm", "pure_hu_icm_postflop_solve_matches_chip_ev"),
]
ENVIRONMENT = dict(CARGO_HOME="/opt/r1/cargo", RUSTUP_HOME="/opt/r1/rustup", RUSTUP_TOOLCHAIN="1.97.0",
                   CARGO_BUILD_JOBS="2", RAYON_NUM_THREADS="1", RUST_TEST_THREADS="2", CARGO_INCREMENTAL="0",
                   CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0")
HOST_POLICY = {"limits": {"outer_memory_max_bytes": 12 * 1024**3}}


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


if hashlib.sha256(SHARED.read_bytes()).hexdigest() != SHARED_SHA:
    raise ValueError("trusted evidence helper SHA differs")
shared = load("extra_validation_helpers", SHARED)
require, identity, read, save, Store = shared.require, shared.identity, shared.read, shared.save, shared.Store


def source_check(arm):
    shared.live_source(arm)
    rows = read(arm["manifest"]["path"])["files"]
    return shared.digest(json.dumps(sorted(rows, key=lambda row: row["path"]), sort_keys=True, separators=(",", ":")).encode())


def successful_test(stdout, name):
    text = stdout.decode("utf-8")
    require(re.search(r"(?m)^test " + re.escape(name) + r" \.\.\. ok\s*$", text), "exact test did not report success: " + name)
    summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
    require(summaries == [("1", "0", "0")], "expected exactly one successful, nonignored execution")
    return {"passed": 1, "failed": 0, "ignored": 0, "test": name}


def compiler_executables(stdout, target):
    found = {}
    for line in stdout.splitlines():
        if not line.strip():
            continue
        message = shared.decode(line)
        if message.get("reason") != "compiler-artifact" or message.get("target", {}).get("name") not in {row[0] for row in TESTS}:
            continue
        if "test" not in message["target"]["kind"] or not message.get("executable"):
            continue
        name, executable = message["target"]["name"], message["executable"]
        require(name not in found, "duplicate test executable")
        require(PurePosixPath(executable).is_absolute() and PurePosixPath(executable).parent == PurePosixPath(target) / "release/deps", "test executable is outside release target")
        require(message["profile"]["test"] is True, "artifact is not a test harness")
        found[name] = executable
    require(set(found) == {"postflop", "rake_icm"}, "missing test executable artifacts")
    return found


def foundation(store, plan):
    arm = plan["arm"]
    require(arm["manifest"]["sha256"] == MANIFEST_SHA and arm["archive"]["sha256"] == ARCHIVE_SHA, "candidate04 identity differs")
    shared.source_files(store, arm)
    store.verify(plan["foundation"])
    state = store.json(plan["foundation"]["path"])
    require(state["schema"] == "r1.range-scaling-validation/v1" and state["status"] == "completed" and
            state["mode"] == "full-validation" and state["source_root"] == arm["source"] and state["target"] == plan["target"] and
            state["source_manifest"] == arm["manifest"] and state["source_archive"] == arm["archive"] and state["boot_id"] == plan["host"]["boot_id"], "full validation prerequisite differs")
    require([row["label"] for row in state["stages"]] == list(shared.FULL_STAGES), "full validation stage set differs")
    require(all(state["environment"].get(key) == value for key, value in ENVIRONMENT.items()), "validation environment differs")
    for name, pin in state["tools"].items():
        require(plan["tools"][name] == pin, "validation tool differs: " + name)
    for row in state["stages"]:
        require(row["status"] == "passed" and row["supervisor_exit"] == 0, "full validation incomplete")
        record = shared.record_bytes(store, row["record"], list(plan["tools"].values()))
        require(record["argv"] == row["argv"] and record["cwd"] == arm["source"], "validation record command differs")
        if row["label"] == "toolchain":
            version = store.data(record["outputs"]["stdout"]["path"]).decode()
            require("release: 1.97.0\n" in version and "host: x86_64-unknown-linux-gnu\n" in version, "toolchain version differs")
    return state


def build_command(plan):
    return [plan["tools"]["cargo"]["path"], "test", "--locked", "--release", "--no-run", "--message-format=json", "-p", "holdem",
            "--test", "postflop", "--test", "rake_icm", "--target-dir", plan["target"]]


def test_command(binary, name):
    return [binary["path"], name, "--exact", "--ignored", "--test-threads=1"]


def immutable_pins(plan):
    return [plan["arm"]["manifest"], plan["arm"]["archive"], plan["foundation"], plan["runner"], plan["helper"], plan["supervisor"], *plan["tools"].values()]


def verify(out):
    out = Path(out)
    store, plan, state = Store(out), read(out / "plan.json"), read(out / "result.json")
    require(shared.content(store.pin(shared.join(plan["output"], "plan.json"))) == shared.content(identity(out / "plan.json")), "plan bytes differ")
    require(plan["schema"] == "r1.exact-mass-extra-validation-plan/v1" and state["schema"] == "r1.exact-mass-extra-validation-result/v1", "extra validation schema")
    require(shared.content(plan["runner"]) == shared.content(identity(__file__)) and shared.content(plan["helper"]) == shared.content(identity(SHARED)), "trusted checker differs")
    require(plan["supervisor"]["sha256"] == SUPERVISOR_SHA and
            plan["environment"] == {**ENVIRONMENT, "RUSTC": plan["tools"]["rustc"]["path"], "RUSTDOC": plan["tools"]["rustdoc"]["path"]}, "supervisor/environment differs")
    foundation(store, plan)
    require(store.pin(shared.join(plan["arm"]["source"], "tools/run_supervised.py")) == plan["supervisor"], "supervisor/source differs")
    shared.host_record(plan["host"], HOST_POLICY)
    labels = ["build-test-harnesses", *[name for _, name in TESTS]]
    require([row["label"] for row in state["stages"]] == labels, "test stage set/order differs")
    count, stopped = 0, False
    binaries = state.get("binaries", {})
    for row in state["stages"]:
        if row["status"] != "passed":
            stopped = True
            require(row["status"] in ("failed", "skipped"), "nonterminal stage")
            if "record" in row:
                shared.record_bytes(store, row["record"], list(plan["tools"].values()), success=False)
            continue
        require(not stopped and row["source_before"] == row["source_after"] == plan["source_identity"] and row["host_after"] == plan["host"], "source/host changed or success after failure")
        record = shared.record_bytes(store, row["record"], list(plan["tools"].values()))
        build = row["label"] == "build-test-harnesses"
        target_name = next((target for target, name in TESTS if name == row["label"]), None)
        expected = build_command(plan) if build else test_command(binaries[target_name], row["label"])
        required = immutable_pins(plan) + ([] if build else [binaries[target_name]])
        require(record["argv"] == record["resolved_argv"] == expected and record["cwd"] == plan["arm"]["source"], "command differs")
        require(all(pin in record["identity_before"] for pin in required) and row["identity_pins"] == required, "identity binding missing")
        require(record["limits"]["timeout_seconds"] == 600 and record["limits"]["memory_limit_bytes"] == 10 * 1024**3 and
                record["limits"]["min_free_memory_bytes"] == 1024**3 and record["limits"]["disk_reserve_bytes"] == 4 * 1024**3, "limits differ")
        require(shared.timestamp(record["ended_at"]) <= shared.timestamp(plan["deadline_utc"]), "deadline exceeded")
        stdout = store.data(record["outputs"]["stdout"]["path"])
        if build:
            paths = compiler_executables(stdout, plan["target"])
            require(set(binaries) == set(paths), "binary set differs")
            for name, path in paths.items():
                store.verify(binaries[name])
                require(binaries[name]["path"] == path, "Cargo/test executable differs")
        else:
            require(successful_test(stdout, row["label"]) == row["test_result"], "test summary differs")
            count += 1
    require(state["status"] in ("completed", "failed"), "nonterminal result")
    if state["status"] == "completed":
        require(not stopped and count == 4, "completed result requires all four tests")
    return {"schema": "r1.exact-mass-extra-validation-verification/v1", "status": state["status"], "tests_passed": count,
            "source_manifest_sha256": MANIFEST_SHA, "source_archive_sha256": ARCHIVE_SHA,
            "scope": "Candidate04 four exact ignored tests; same-source target reuse after full validation. No performance claim."}


def run(args):
    root, target = args.root.resolve(strict=True), args.target.resolve(strict=True)
    source, out = root / "source", root / "extra-validation"
    require(not out.exists(), "new output required; no retries")
    out.mkdir()
    state = {"schema": "r1.exact-mass-extra-validation-result/v1", "status": "running",
             "stages": [{"label": name, "status": "pending"} for name in ["build-test-harnesses", *[name for _, name in TESTS]]]}
    save(out / "result.json", state)
    store = Store(out, create=True)
    plan = None
    try:
        deadline = shared.timestamp(args.deadline_utc)
        require(0 < deadline - time.time() < 6 * 3600, "invalid deadline")
        arm = {"source": str(source), "manifest": store.add(root / "source-candidate-manifest.json"), "archive": store.add(root / "source-candidate.tar.gz")}
        require(arm["manifest"]["sha256"] == MANIFEST_SHA and arm["archive"]["sha256"] == ARCHIVE_SHA, "candidate04 SHA differs")
        shared.source_files(store, arm)
        before = source_check(arm)
        machine = shared.host(HOST_POLICY)
        prerequisite = read(root / "validation/result.json")
        require(prerequisite["status"] == "completed" and prerequisite["target"] == str(target), "full validation must complete on this target first")
        tools = {name: identity(pin["path"]) for name, pin in prerequisite["tools"].items()}
        require(tools == prerequisite["tools"], "toolchain changed since validation")
        tools["python"] = identity(sys.executable)
        for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER"):
            require(name not in os.environ, "unexpected build override")
        environment = {**ENVIRONMENT, "RUSTC": tools["rustc"]["path"], "RUSTDOC": tools["rustdoc"]["path"]}
        os.environ.update(environment)
        os.environ["PATH"] = str(Path(tools["cargo"]["path"]).parent) + ":" + os.environ["PATH"]
        plan = {"schema": "r1.exact-mass-extra-validation-plan/v1", "output": str(out), "arm": arm, "target": str(target),
                "deadline_utc": args.deadline_utc, "source_identity": before, "host": machine, "environment": environment,
                "foundation": store.add(root / "validation/result.json"), "tools": tools, "runner": store.add(__file__), "helper": store.add(SHARED),
                "supervisor": store.add(source / "tools/run_supervised.py")}
        require(plan["supervisor"]["sha256"] == SUPERVISOR_SHA, "supervisor changed")
        for row in prerequisite["stages"]:
            shared.retain_record(store, row["record"]["path"], list(tools.values()))
        foundation(store, plan)
        save(out / "plan.json", plan)
        store.add(out / "plan.json")
        supervisor = load("extra_validation_supervisor", source / "tools/run_supervised.py")
        for entry in state["stages"]:
            entry["source_before"] = source_check(arm)
            require(shared.host(HOST_POLICY) == machine, "host changed")
            require(deadline - time.time() > 620, "insufficient time for a bounded 600-second stage")
            build = entry["label"] == "build-test-harnesses"
            target_name = next((target for target, name in TESTS if name == entry["label"]), None)
            argv = build_command(plan) if build else test_command(state["binaries"][target_name], entry["label"])
            pins = immutable_pins(plan) + ([] if build else [state["binaries"][target_name]])
            for pin in pins:
                require(identity(pin["path"]) == pin, "live identity changed")
            directory = out / "stages" / entry["label"]
            directory.mkdir(parents=True)
            entry.update(status="running", identity_pins=pins, argv=argv)
            save(out / "result.json", state)
            arguments = ["--record", str(directory / "supervisor.json"), "--cwd", str(source), "--disk-path", str(root),
                         "--timeout-seconds", "600", "--memory-limit-bytes", str(10 * 1024**3), "--min-free-memory-bytes", str(1024**3),
                         "--disk-reserve-bytes", str(4 * 1024**3), "--grace-seconds", "5", "--kill-wait-seconds", "5", "--poll-seconds", "0.1"]
            for pin in pins:
                arguments += ["--identity-file", pin["path"]]
            require(deadline - time.time() > 620, "insufficient deadline after identity checks")
            code = supervisor.main(arguments + ["--", *argv])
            entry["record"] = store.add(directory / "supervisor.json")
            save(out / "result.json", state)
            shared.retain_record(store, directory / "supervisor.json", list(tools.values()), tolerate_identity_failure=code != 0)
            require(code == 0, "supervisor failed")
            record = shared.record_bytes(store, entry["record"], list(tools.values()))
            stdout = store.data(record["outputs"]["stdout"]["path"])
            if build:
                executables = compiler_executables(stdout, str(target))
                require(all(not Path(path).is_symlink() for path in executables.values()), "test executable symlink")
                state["binaries"] = {name: store.add(path) for name, path in executables.items()}
            else:
                entry["test_result"] = successful_test(stdout, entry["label"])
            entry.update(source_after=source_check(arm), host_after=shared.host(HOST_POLICY))
            require(entry["source_before"] == entry["source_after"] == before and entry["host_after"] == machine, "source/host changed")
            entry["status"] = "passed"
            save(out / "result.json", state)
            print(json.dumps({"stage": entry["label"], "status": "passed"}), flush=True)
        state["status"] = "completed"
        save(out / "result.json", state)
        save(out / "verification.json", verify(out))
    except BaseException as error:
        state.update(status="failed", error=repr(error))
        for entry in state["stages"]:
            if entry["status"] == "running":
                entry.update(status="failed", error=repr(error))
                try:
                    path = out / "stages" / entry["label"] / "supervisor.json"
                    if path.is_file():
                        entry["record"] = store.add(path)
                        shared.retain_record(store, path, list(plan["tools"].values()), tolerate_identity_failure=True)
                except (OSError, ValueError) as retention_error:
                    entry["retention_error"] = repr(retention_error)
            elif entry["status"] == "pending":
                entry.update(status="skipped", reason=repr(error))
        save(out / "result.json", state)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--target", type=Path)
    parser.add_argument("--deadline-utc")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        print(json.dumps(verify(args.root / "extra-validation"), indent=2))
    else:
        require(args.target is not None and args.deadline_utc, "target and deadline required")
        run(args)


if __name__ == "__main__":
    main()
