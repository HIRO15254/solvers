#!/usr/bin/env python3
"""Build/run four diagnostic processes; original bytes retained, no speed claims."""
from __future__ import annotations
import argparse
import difflib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
CORE = HERE.parent / "run.py"
SHARED = HERE.parent.parent / "showdown-kernel/run.py"
CORE_SHA = "62200fe9df43a111f6088d4f6e824234876baff6caf0c065fd42365a84869129"
SHARED_SHA = "6d21fe34f9f66327bb7f3ff1c50d977687018af45c3593d56726f78e8da44a60"
PROTOCOL_SHA = "2555fbd3ae472373a5094d4636b46c67a82c240538f257b143fd5f6c17f78777"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


for _path, _sha in ((CORE, CORE_SHA), (SHARED, SHARED_SHA)):
    if hashlib.sha256(_path.read_bytes()).hexdigest() != _sha:
        raise ValueError("trusted helper changed: " + str(_path))
core = load("diagnostic_exact_mass_helpers", CORE)
instrument = load("diagnostic_patch", HERE / "instrument.py")
require, identity, content, read, save, Store = core.require, core.identity, core.content, core.read, core.save, core.Store


def files_in_tar(data):
    files = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for row in archive:
            path = instrument.safe_name(row.name)
            if row.isdir():
                continue
            require(row.isfile() and path not in files, "invalid source archive member")
            files[path] = archive.extractfile(row).read()
    return files


def source_bytes(store, plan):
    for key in ("candidate_manifest", "candidate_archive", "source_manifest", "source_archive", "patch", "provenance"):
        store.verify(plan[key])
    require(plan["candidate_manifest"]["sha256"] == instrument.MANIFEST_SHA256 and
            plan["candidate_archive"]["sha256"] == instrument.ARCHIVE_SHA256, "candidate01 pins differ")
    original = files_in_tar(store.data(plan["candidate_archive"]["path"]))
    manifest = store.json(plan["candidate_manifest"]["path"])
    require({p: core.digest(b) for p, b in original.items()} ==
            {r["path"]: content(r) for r in manifest["files"]}, "original candidate archive differs")
    patched = instrument.patch({p: original[p] for p in instrument.EXPECTED})
    expected = {**original, **patched}
    actual = files_in_tar(store.data(plan["source_archive"]["path"]))
    require(actual == expected, "instrumented archive differs from exact candidate patch")
    expected_rows = [{"path": p, **core.digest(b)} for p, b in sorted(expected.items())]
    require(store.json(plan["source_manifest"]["path"])["files"] == expected_rows, "instrumented manifest differs")
    diff = "".join("".join(difflib.unified_diff(original[p].decode().splitlines(keepends=True), patched[p].decode().splitlines(keepends=True), fromfile="a/" + p, tofile="b/" + p)) for p in sorted(patched)).encode()
    require(store.data(plan["patch"]["path"]) == diff, "patch bytes differ")
    provenance = store.json(plan["provenance"]["path"])
    require(provenance["patch_sha256"] == plan["patch"]["sha256"] and
            provenance["instrumented_manifest_sha256"] == plan["source_manifest"]["sha256"] and
            provenance["candidate_archive_sha256"] == instrument.ARCHIVE_SHA256 and
            provenance["candidate_manifest_sha256"] == instrument.MANIFEST_SHA256 and
            provenance["instrument_py_sha256"] == plan["controls"]["instrument.py"]["sha256"], "provenance differs")
    return expected


def live_source(source, expected):
    paths = list(source.rglob("*"))
    require(not any(p.is_symlink() for p in paths), "source symlink")
    actual = {p.relative_to(source).as_posix(): core.digest(p.read_bytes()) for p in paths if p.is_file()}
    require(actual == {p: core.digest(b) for p, b in expected.items()}, "live instrumented source changed")


def diagnostic_counts(report):
    diagnostic = report["mass_diagnostics"]
    require(diagnostic["schema"] == "r1.mass-gate-diagnostic/v1" and
            diagnostic["callers"] == instrument.CALLERS and diagnostic["fields"] == instrument.FIELDS, "diagnostic labels differ")
    counts = diagnostic["counts"]
    require(len(counts) == 7, "diagnostic caller count")
    for row in counts:
        require(len(row) == 10 and all(type(v) is int and 0 <= v < 2**64 for v in row), "diagnostic counters invalid")
        require(row[0] == sum(row[4:8]) and row[8] == row[4] and row[3] <= row[4]
                and row[2] <= row[1] and row[9] <= sum(row[5:8]), "diagnostic counter identities differ")
    require(sum(row[0] for row in counts) > 0, "no diagnostic gate calls")
    return diagnostic


def stage_command(plan, label):
    if label == "toolchain":
        return [plan["tools"]["rustc"]["path"], "-Vv"]
    if label == "release-example":
        return [plan["tools"]["cargo"]["path"], "build", "--release", "-p", "cli", "--example", "hu_scaling_bench", "--target-dir", plan["target"]]
    definition = plan["protocol"]["cases"][label]
    return [plan["binary_path"], "--config", plan["inputs"][label]["path"], "--threads", "1",
            "--iterations", str(definition["iterations"]), "--layout", "compact", "--target-nash-conv", str(definition["target_nash_conv"]),
            "--check-every", str(definition["check_every"]), "--out", core.join(plan["output"], "stages", label, "bench")]


def fixed_pins(plan):
    return [*plan["controls"].values(), *[plan[key] for key in ("source_manifest", "source_archive", "candidate_archive", "candidate_manifest", "patch", "provenance", "supervisor")], *plan["tools"].values()]


def check(out):
    """Portable successful-proof check; failed proofs retain all available bytes."""
    out = Path(out)
    store, plan, state = Store(out), read(out / "plan.json"), read(out / "result.json")
    require(content(store.pin(core.join(plan["output"], "plan.json"))) == content(identity(out / "plan.json")), "plan bytes differ")
    require(plan["schema"] == "r1.mass-gate-diagnostic-plan/v1" and state["schema"] == "r1.mass-gate-diagnostic-result/v1", "diagnostic schemas differ")
    for name, path in (("driver", Path(__file__)), ("instrument.py", HERE / "instrument.py"), ("exact-run.py", CORE), ("showdown-run.py", SHARED)):
        require(content(plan["controls"][name]) == content(identity(path)), "trusted control differs: " + name)
        store.verify(plan["controls"][name])
    require(plan["protocol_pin"]["sha256"] == PROTOCOL_SHA and store.json(plan["protocol_pin"]["path"]) == plan["protocol"], "frozen protocol differs")
    source_bytes(store, plan)
    core.host_record(plan["host"], plan["protocol"])
    for key in ("baseline_plan", "baseline_result", "supervisor"):
        store.verify(plan[key])
    baseline_plan, baseline_state = store.json(plan["baseline_plan"]["path"]), store.json(plan["baseline_result"]["path"])
    require(baseline_state["status"] == "completed" and baseline_plan["protocol"] == plan["protocol"] and
            baseline_plan["arms"]["new"]["manifest"] == plan["candidate_manifest"] and
            baseline_plan["arms"]["new"]["archive"] == plan["candidate_archive"], "baseline binding differs")
    for field in ("boot_id", "machine", "logical_cpus", "affinity", "topology", "cpu_models"):
        require(plan["host"][field] == baseline_plan["host"][field], "baseline host differs")
    require(content(plan["supervisor"]) == core.digest(source_bytes(store, plan)["tools/run_supervised.py"]), "supervisor source differs")
    if "binary" in state:
        store.verify(state["binary"])
        require(state["binary"]["path"] == plan["binary_path"], "binary path differs")
    labels = ["toolchain", "release-example", *plan["protocol"]["cases"]]
    require([row["label"] for row in state["stages"]] == labels, "stage list differs")
    expected_status = "passed" if state["status"] == "completed" else None
    failure = False
    counts = {}
    for row in state["stages"]:
        if expected_status:
            require(row["status"] == expected_status, "completed diagnostic has unfinished stage")
        if row["status"] != "passed":
            failure = True
            require(row["status"] in ("failed", "skipped"), "nonterminal diagnostic stage")
            if "record" in row:
                core.record_bytes(store, row["record"], list(plan["tools"].values()), success=False)
            continue
        require(not failure and row["source_after_verified"] is True and row["host_after"] == plan["host"], "successful stage after failure or changed source/host")
        record = core.record_bytes(store, row["record"], list(plan["tools"].values()))
        argv = stage_command(plan, row["label"])
        require(record["argv"] == argv and record["resolved_argv"] == argv and record["cwd"] == plan["source"], "stage command differs")
        expected_timeout = 30 if row["label"] == "toolchain" else 1200 if row["label"] == "release-example" else 300
        require(record["limits"]["timeout_seconds"] == row["timeout_seconds"] == expected_timeout and
                record["limits"]["memory_limit_bytes"] == 10 * 1024**3 and
                record["limits"]["min_free_memory_bytes"] == 1024**3 and
                record["limits"]["disk_reserve_bytes"] == 4 * 1024**3, "stage limits differ")
        require(core.timestamp(record["ended_at"]) <= core.timestamp(plan["deadline_utc"]), "stage exceeded deadline")
        expected_pins = fixed_pins(plan) + ([state["binary"], plan["inputs"][row["label"]]] if row["label"] in plan["protocol"]["cases"] else [])
        require(row["identity_pins"] == expected_pins, "required identity list differs")
        require(all(pin in record["identity_before"] for pin in row["identity_pins"]), "supervisor identity bindings missing")
        if row["label"] == "toolchain":
            version = store.data(record["outputs"]["stdout"]["path"]).decode()
            require("release: 1.97.0\n" in version and "host: x86_64-unknown-linux-gnu\n" in version, "toolchain differs")
        elif row["label"] in plan["protocol"]["cases"]:
            label = row["label"]
            require(content(plan["inputs"][label]) == content(baseline_plan["inputs"][label]), "baseline input differs")
            stage = {"label": label, "case": label, "arm": "new", "iterations": plan["protocol"]["cases"][label]["iterations"]}
            actual = core.sample(store, plan, stage, record)
            baseline = plan["references"][label]
            matches = [entry for entry in baseline_state["stages"] if entry["stage"]["label"] == label + "-b1-new" and entry["status"] == "passed"]
            require(len(matches) == 1 and matches[0]["sample"] == baseline["sample"], "baseline sample binding differs")
            store.verify(baseline["report"])
            require(baseline["report"]["path"] == core.join(baseline_plan["output"], "stages", label + "-b1-new", "bench", "result.json"), "baseline report path differs")
            core.same_solution(store, baseline["sample"], actual)
            original_report = store.json(baseline["report"]["path"])
            report = store.json(core.join(plan["output"], "stages", label, "bench", "result.json"))
            require(original_report["quality"] == report["quality"] and original_report["iterations"] == report["iterations"], "baseline report values differ")
            counts[label] = diagnostic_counts(report)
            require(row["diagnostics"] == counts[label], "saved counters differ")
    require(state["status"] in ("completed", "failed"), "nonterminal result")
    if state["status"] == "completed":
        require(len(counts) == 4 and state["same_candidate_outputs_exact"] is True, "missing exact output checks")
    return {"schema": "r1.mass-gate-diagnostic-verification/v1", "status": state["status"], "cases_exact": len(counts),
            "counts": counts, "timings_are_performance_evidence": False,
            "scope": "Whole-process aggregate gate calls; selected candidate01 b1 references bound to retained original plan/result. Full original 32-process campaign was independently checked at launch and remains a separate proof. No performance or memory claim."}


def run(args):
    root, target = args.root.resolve(), args.target.resolve()
    source, out = root / "source", root / "run"
    require(not target.exists() and not out.exists(), "fresh target and diagnostic output required")
    out.mkdir()
    state = {"schema": "r1.mass-gate-diagnostic-result/v1", "status": "running", "timings_are_performance_evidence": False,
             "stages": [{"label": label, "status": "pending", "timeout_seconds": limit} for label, limit in
                        [("toolchain", 30), ("release-example", 1200), ("river", 300), ("turn", 300), ("flop", 300), ("narrow-river", 300)]]}
    save(out / "result.json", state)
    store = Store(out, create=True)
    plan = None
    try:
        baseline_out = args.baseline_proof.resolve()
        verified = core.check(baseline_out)
        require(verified["status"] == "completed", "original 32-sample campaign not complete")
        baseline_store, baseline_plan, baseline_state = Store(baseline_out), read(baseline_out / "plan.json"), read(baseline_out / "result.json")
        def adopt(pin):
            baseline_store.verify(pin)
            data = baseline_store.data(pin["path"])
            dest = store.blob(pin)
            if not dest.exists():
                dest.write_bytes(data)
            require(core.digest(dest.read_bytes()) == content(pin), "adopted payload differs")
            store.entries[pin["path"]] = pin
            store.flush()
            return pin
        protocol = baseline_plan["protocol"]
        controls = {name: store.add(path) for name, path in (("driver", Path(__file__)), ("instrument.py", HERE / "instrument.py"), ("exact-run.py", CORE), ("showdown-run.py", SHARED))}
        protocol_pin = adopt(baseline_plan["controls"]["protocol.json"])
        require(protocol_pin["sha256"] == PROTOCOL_SHA, "protocol differs")
        machine = core.host(protocol)
        for field in ("boot_id", "machine", "logical_cpus", "affinity", "topology", "cpu_models"):
            require(machine[field] == baseline_plan["host"][field], "baseline host differs: " + field)
        deadline = core.timestamp(args.deadline_utc)
        require(0 < deadline - time.time() < 6 * 3600, "invalid deadline")
        chain = Path("/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin")
        for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER"):
            require(name not in os.environ, "unexpected build override: " + name)
        environment = dict(CARGO_HOME="/opt/r1/cargo", RUSTUP_HOME="/opt/r1/rustup", RUSTUP_TOOLCHAIN="1.97.0", CARGO_BUILD_JOBS="2",
                           RAYON_NUM_THREADS="1", RUST_TEST_THREADS="2", CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0",
                           RUSTC=str(chain / "rustc"), RUSTDOC=str(chain / "rustdoc"))
        os.environ.update(environment)
        os.environ["PATH"] = str(chain) + ":" + os.environ["PATH"]
        tools = {name: identity(chain / name) for name in ("cargo", "rustc", "rustdoc")}
        tools["python"] = identity(sys.executable)
        # Capture the exact copied source before the build. Every subsequent
        # stage rehashes the same manifest; the tar is a portable byte record.
        source_manifest = store.add(root / "instrumented-source-manifest.json")
        rows = read(source_manifest["path"])["files"]
        archive = root / "instrumented-source.tar.gz"
        require(not archive.exists(), "instrumented archive already exists")
        with tarfile.open(archive, "w:gz") as tar:
            for row in rows:
                path = source / row["path"]
                require(content(identity(path)) == content(row), "instrumented source changed before archive")
                tar.add(path, arcname=row["path"], recursive=False)
        new = baseline_plan["arms"]["new"]
        plan = {"schema": "r1.mass-gate-diagnostic-plan/v1", "source": str(source), "output": str(out), "target": str(target),
                "deadline_utc": args.deadline_utc, "host": machine, "environment": environment, "tools": tools, "controls": controls,
                "protocol": protocol, "protocol_pin": protocol_pin, "candidate_manifest": adopt(new["manifest"]), "candidate_archive": adopt(new["archive"]),
                "source_manifest": source_manifest, "source_archive": store.add(archive), "patch": store.add(root / "instrumentation.patch"),
                "provenance": store.add(root / "provenance.json"), "supervisor": store.add(source / "tools/run_supervised.py"),
                "baseline_plan": store.add(baseline_out / "plan.json"), "baseline_result": store.add(baseline_out / "result.json"),
                "references": {}, "inputs": {}, "binary_path": str(target / "release/examples/hu_scaling_bench")}
        expected = source_bytes(store, plan)
        require(core.digest(expected["tools/run_supervised.py"]) == content(plan["supervisor"]), "supervisor differs from candidate01")
        live_source(source, expected)
        for case, definition in protocol["cases"].items():
            matches = [entry for entry in baseline_state["stages"] if entry["stage"]["label"] == case + "-b1-new" and entry["status"] == "passed"]
            require(len(matches) == 1, "baseline measured reference absent")
            entry = matches[0]
            report_path = core.join(baseline_plan["output"], "stages", entry["stage"]["label"], "bench", "result.json")
            reference = {"report": adopt(baseline_store.pin(report_path)), "sample": entry["sample"]}
            for pin in [*reference["sample"]["artifacts"].values(), reference["sample"]["normalized_config"]]:
                adopt(pin)
            plan["references"][case] = reference
            plan["inputs"][case] = store.add(source / protocol["config_directory"] / definition["file"])
            require(content(plan["inputs"][case]) == content(baseline_plan["inputs"][case]), "input bytes differ")
        save(out / "plan.json", plan)
        store.add(out / "plan.json")
        target.mkdir(parents=True)
        supervisor = load("mass_diagnostic_supervisor", source / "tools/run_supervised.py")
        immutable_pins = fixed_pins(plan)
        for entry in state["stages"]:
            live_source(source, expected)
            require(core.host(protocol) == machine, "host changed")
            require(deadline - time.time() > entry["timeout_seconds"] + 20, "insufficient deadline")
            directory = out / "stages" / entry["label"]
            directory.mkdir(parents=True)
            pins = immutable_pins + ([state["binary"], plan["inputs"][entry["label"]]] if entry["label"] in protocol["cases"] else [])
            for pin in pins:
                require(identity(pin["path"]) == pin, "live identity changed")
            entry.update(status="running", identity_pins=pins)
            save(out / "result.json", state)
            arguments = ["--record", str(directory / "supervisor.json"), "--cwd", str(source), "--disk-path", str(root),
                         "--timeout-seconds", str(entry["timeout_seconds"]), "--memory-limit-bytes", str(10 * 1024**3),
                         "--min-free-memory-bytes", str(1024**3), "--disk-reserve-bytes", str(4 * 1024**3),
                         "--grace-seconds", "5", "--kill-wait-seconds", "5", "--poll-seconds", "0.1"]
            for pin in pins:
                arguments += ["--identity-file", pin["path"]]
            require(deadline - time.time() > entry["timeout_seconds"] + 20, "insufficient deadline after rehash")
            code = supervisor.main(arguments + ["--", *stage_command(plan, entry["label"])])
            entry["record"] = store.add(directory / "supervisor.json")
            save(out / "result.json", state)
            core.retain_record(store, directory / "supervisor.json", list(tools.values()), tolerate_identity_failure=code != 0)
            for path in (directory / "bench").rglob("*"):
                if path.is_file():
                    store.add(path)
            require(code == 0, "supervisor failed: " + entry["label"])
            record = core.record_bytes(store, entry["record"], list(tools.values()))
            if entry["label"] == "toolchain":
                version = store.data(record["outputs"]["stdout"]["path"]).decode()
                require("release: 1.97.0\n" in version and "host: x86_64-unknown-linux-gnu\n" in version, "wrong toolchain")
            elif entry["label"] == "release-example":
                state["binary"] = store.add(plan["binary_path"])
            else:
                case = entry["label"]
                stage = {"label": case, "case": case, "arm": "new", "iterations": protocol["cases"][case]["iterations"]}
                actual = core.sample(store, plan, stage, record)
                core.same_solution(store, plan["references"][case]["sample"], actual)
                entry["diagnostics"] = diagnostic_counts(store.json(str(directory / "bench/result.json")))
            live_source(source, expected)
            entry.update(status="passed", host_after=core.host(protocol), source_after_verified=True)
            require(entry["host_after"] == machine, "host changed")
            save(out / "result.json", state)
            print(json.dumps({"stage": entry["label"], "status": "passed"}), flush=True)
        state.update(status="completed", same_candidate_outputs_exact=True)
        save(out / "result.json", state)
        save(out / "verification.json", check(out))
    except BaseException as error:
        state.update(status="failed", error=repr(error))
        for entry in state["stages"]:
            if entry["status"] == "running":
                entry.update(status="failed", error=repr(error))
                try:
                    directory = out / "stages" / entry["label"]
                    if (directory / "supervisor.json").is_file():
                        entry["record"] = store.add(directory / "supervisor.json")
                        core.retain_record(store, directory / "supervisor.json", list(plan["tools"].values()), tolerate_identity_failure=True)
                    for path in directory.rglob("*"):
                        if path.is_file():
                            store.add(path, changed_identity=True)
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
    parser.add_argument("--baseline-proof", type=Path)
    parser.add_argument("--deadline-utc")
    parser.add_argument("--check", action="store_true", help="Portable check of root/run without original VM")
    args = parser.parse_args()
    if args.check:
        print(json.dumps(check(args.root / "run"), indent=2))
    else:
        require(args.target is not None and args.baseline_proof is not None and args.deadline_utc, "run requires target, baseline-proof and deadline-utc")
        run(args)


if __name__ == "__main__":
    main()
