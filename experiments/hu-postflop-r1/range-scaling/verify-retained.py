"""Portable range/scaling evidence verification; never executes retained code.

The frozen codec retention container is reused only as a SHA/gzip payload layer.
Multiple matching acquisitions may be supplied; conflicting original aliases
are rejected. Compiler/Python executables can be identity-only, never samples,
source archives, benchmark binaries, runners or canonical output.
"""
from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import importlib.util
import io
import json
from pathlib import Path, PurePosixPath
import re
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
LEGACY = HERE.parent / "codec/context-reuse/linux-spot-20260926"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


retention = load("range_retention_payload", LEGACY / "retain.py")
previous = sys.modules.get("retain")
sys.modules["retain"] = retention
try:
    legacy = load("range_retention_store", LEGACY / "verify_retained.py")
finally:
    if previous is None:
        sys.modules.pop("retain", None)
    else:
        sys.modules["retain"] = previous
runner = load("range_scaling_checked_runner", HERE / "scaling-run.py")
require, content = retention.require, retention.content
join = lambda root, *names: str(PurePosixPath(root).joinpath(*names))


class Store(legacy.Store):
    """Union of verified payload stores with streaming, original-path aliases."""
    def __init__(self, directories):
        self.stores = [legacy.Store(directory) for directory in directories]
        self.paths, self.virtual, self.recovered_aliases = {}, {}, []
        for index, store in enumerate(self.stores):
            for path in store.paths:
                if path in self.paths:
                    require(self.identity(path) == store.identity(path), "conflicting acquisition alias: " + path)
                else:
                    self.paths[path] = (index, path)

    def open(self, path):
        path = str(path)
        if path in self.virtual:
            return io.BytesIO(self.virtual[path])
        require(path in self.paths, "required original payload unavailable: " + path)
        index, original = self.paths[path]
        return self.stores[index].open(original)

    def identity(self, path):
        path = str(path)
        if path in self.virtual:
            pin = retention.fingerprint(io.BytesIO(self.virtual[path]))
        else:
            require(path in self.paths, "required original payload unavailable: " + path)
            index, original = self.paths[path]
            pin = content(self.stores[index].identity(original))
        return {"path": path, **pin}

    def bind(self, pin):
        """A matching verified payload may satisfy a missing identity alias."""
        path = pin["path"]
        if path not in self.paths and path not in self.virtual:
            match = next((name for name in self.paths if content(self.identity(name)) == content(pin)), None)
            require(match is not None, "required bytes not retained: " + path)
            self.paths[path] = self.paths[match]
            self.recovered_aliases.append({"path": path, "from": match, **content(pin)})
        self.bound(pin)


def source(store, root, manifest_pin, archive_pin=None):
    store.bind(manifest_pin)
    manifest = store.read(manifest_pin["path"])
    expected_archive = {"bytes": manifest["archive_bytes"], "sha256": manifest["archive_sha256"]}
    if archive_pin is None:
        path = join(PurePosixPath(manifest_pin["path"]).parent, "source-candidate.tar.gz")
        archive_pin = {"path": path, **expected_archive}
    require(content(archive_pin) == expected_archive, "source archive/manifest binding differs")
    store.bind(archive_pin)
    files = {row["path"]: content(row) for row in manifest["files"]}
    require(len(files) == len(manifest["files"]), "duplicate source path")
    require(re.fullmatch(r"[0-9a-f]{40}", manifest["base_commit"]), "full source revision missing")
    decoded = legacy.archive_files(store.raw(archive_pin["path"]), files, manifest.get("directory_entries", []))
    for name, data in decoded.items():
        store.add(join(root, name), data)
    return manifest, files


def tool_identity(pin):
    require(isinstance(pin["bytes"], int) and pin["bytes"] > 0 and re.fullmatch(r"[0-9a-f]{64}", pin["sha256"]),
            "invalid identity-only tool pin")


def python_tool(path):
    return re.fullmatch(r"python(?:\d+(?:\.\d+)*)?", PurePosixPath(path).name) is not None


def record_bytes(store, record, mandatory, identity_tools):
    require(record["schema"] == "solvers.supervised-run/v1", "wrong supervisor schema")
    require(record["shell"] is False, "shell invocation is outside the protocol")
    require(record["resolved_argv"][1:] == record["argv"][1:]
            and any(pin["path"] == record["resolved_argv"][0] for pin in record["identity_before"]),
            "executable is not identity-bound")
    require(record["identity_unchanged"] and record["identity_before"] == record["identity_after"], "stage identities changed")
    for key in ("identity_before", "identity_after"):
        require(all(pin in record[key] for pin in mandatory), "stage is missing required input/source bindings")
        for pin in record[key]:
            if pin["path"] in identity_tools or python_tool(pin["path"]):
                tool_identity(pin)
                if pin["path"] in store.paths or pin["path"] in store.virtual:
                    store.bound(pin)
            else:
                store.bind(pin)
    require(set(record["outputs"]) == {"stdout", "stderr", "samples"}, "missing raw stage output")
    for pin in record["outputs"].values():
        store.bind(pin)
    count, peak, elapsed, last = 0, 0, -1, None
    with store.open(record["outputs"]["samples"]["path"]) as samples:
        for line in samples:
            sample = retention.read_json(line)
            require(sample["elapsed_seconds"] >= elapsed and sample["tree_resident_bytes"] >= 0,
                    "invalid sampling order or memory")
            elapsed = sample["elapsed_seconds"]
            count, peak, last = count + 1, max(peak, sample["tree_resident_bytes"]), sample
    require(count == record["measurement"]["sample_count"]
            and peak == record["measurement"]["sampled_peak_tree_resident_bytes"], "sample count/peak differs")
    require(last == record.get("last_sample"), "final sample differs")
    if successful(record):
        require(last is not None and last["pids"] == [], "successful cleanup sample missing")


def successful(record):
    return (record["state"] == "completed" and record["supervisor_exit_code"] == 0
            and record["child_exit_code"] == 0 and record["cleanup_complete"]
            and record["identity_unchanged"] and not record["errors"] and not record["forced"]
            and record["stop_reason"] == "completed")


def validation_commands(state, python):
    cargo, target = state["tools"]["cargo"]["path"], state["target"]
    stages = [
        ("toolchain", 30, [state["tools"]["rustc"]["path"], "-Vv"]),
        ("fmt", 60, [cargo, "fmt", "--all", "--check"]),
        ("clippy", 1800, [cargo, "clippy", "--workspace", "--all-targets", "--target-dir", target, "--", "-D", "warnings"]),
        ("workspace-tests", 2400, [cargo, "test", "--workspace", "--target-dir", target]),
        ("docs", 60, [python, "-B", "tools/check_docs.py"]),
        ("release-example", 1800, [cargo, "build", "--release", "-p", "cli", "--example", "hu_scaling_bench", "--target-dir", target]),
        ("release-oracle", 1800, [cargo, "test", "--release", "-p", "holdem", "--test", "oracle_diff", "--target-dir", target, "--", "--include-ignored"]),
        ("release-river-resolve", 1800, [cargo, "test", "--release", "-p", "cli", "--lib", "--target-dir", target,
                                      "sol::tests::river_resolve_accuracy", "--", "--exact", "--ignored"])]
    mode = state.get("mode", "full-validation")
    require(mode in ("full-validation", "release-build-only"), "unknown validation mode")
    return stages if mode == "full-validation" else [stage for stage in stages if stage[0] in ("toolchain", "release-example")]


def test_summary(text):
    rows = [list(map(int, row)) for row in re.findall(
        r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", text)]
    require(rows and sum(row[0] for row in rows) > 0 and all(row[1] == 0 for row in rows), "Rust test execution absent or failed")
    return {"summaries": rows, "totals": [sum(row[index] for row in rows) for index in range(5)]}


def validation(store, result_path):
    state = store.read(result_path)
    require(state["schema"] == "r1.range-scaling-validation/v1" and state["status"] in ("completed", "failed"),
            "validation has no terminal completed/failed result")
    manifest, _ = source(store, state["source_root"], state["source_manifest"], state["source_archive"])
    store.bind(state["runner"])
    for pin in state["tools"].values():
        tool_identity(pin)
    require(0 < int(state["cgroup"]["memory_max"]) <= 12 * 1024**3, "validation memory limit differs")
    require(state["environment"]["RUSTUP_TOOLCHAIN"] == "1.97.0"
            and state["environment"]["CARGO_BUILD_JOBS"] == "2"
            and state["environment"]["RAYON_NUM_THREADS"] == "1"
            and state["environment"]["RUST_TEST_THREADS"] == "2", "validation environment differs")
    stages = state["stages"]
    mode = state.get("mode", "full-validation")
    count = len(validation_commands(state, "python"))
    require(stages and len(stages) <= count, "invalid validation stage count")
    tool_paths = {pin["path"] for pin in state["tools"].values()}
    rows, failed, binary = [], None, None
    for index, stage in enumerate(stages):
        store.bind(stage["record"])
        require(stage["record"]["path"] == join(PurePosixPath(result_path).parent, "stages", stage["label"], "supervisor.json"), "stage record location differs")
        record = store.read(stage["record"]["path"])
        label, timeout, argv = validation_commands(state, record["argv"][0])[index]
        recorded_argv = record["argv"]
        if label == "workspace-tests":
            require(recorded_argv.count("--no-fail-fast") <= 1, "duplicate no-fail-fast flag")
            recorded_argv = [arg for arg in recorded_argv if arg != "--no-fail-fast"]
        require(stage["label"] == label and stage["argv"] == record["argv"] and recorded_argv == argv
                and stage["timeout_seconds"] == timeout == record["limits"]["timeout_seconds"]
                and record["cwd"] == state["source_root"], "validation stage command/order differs")
        if label == "docs":
            require(python_tool(argv[0]) and python_tool(record["resolved_argv"][0]), "docs executable is not recorded Python")
        else:
            require(record["resolved_argv"][0] == argv[0], "validation resolved executable differs")
        limits = record["limits"]
        require(limits["memory_limit_bytes"] == 10 * 1024**3 and limits["min_free_memory_bytes"] == 1024**3
                and limits["disk_reserve_bytes"] == 4 * 1024**3 and limits["grace_seconds"] == 5
                and limits["kill_wait_seconds"] == 5 and limits["poll_seconds"] == 0.1, "validation limits differ")
        mandatory = [state["source_manifest"], state["source_archive"], state["runner"], state["tools"]["cargo"], state["tools"]["rustc"],
                     store.identity(join(state["source_root"], "tools/run_supervised.py"))]
        record_bytes(store, record, mandatory, tool_paths)
        require(stage["supervisor_exit"] == record["supervisor_exit_code"], "stage exit differs")
        passed = successful(record)
        require(stage["status"] == ("passed" if passed else "failed"), "validation success claim differs")
        report = {"label": label, "outcome": record["state"], "stop_reason": record["stop_reason"],
                  "child_exit_code": record["child_exit_code"], "supervisor_exit": record["supervisor_exit_code"],
                  "cleanup_complete": record["cleanup_complete"], "record": stage["record"]}
        text = store.raw(record["outputs"]["stdout"]["path"]).decode("utf-8")
        if passed:
            if label == "toolchain":
                require("release: 1.97.0" in text and "host: x86_64-unknown-linux-gnu" in text, "wrong toolchain output")
            if label in ("workspace-tests", "release-oracle", "release-river-resolve"):
                report["tests"] = test_summary(text)
            if label == "release-river-resolve":
                require("test sol::tests::river_resolve_accuracy ... ok" in text
                        and report["tests"]["totals"][:3] == [1, 0, 0], "river resolve ignored test did not run exactly once")
            if label == "release-example":
                binary = state.get("binary")
                require(binary and binary["path"] == join(state["target"], "release/examples/hu_scaling_bench"), "compiled binary binding missing")
                store.bind(binary)
        else:
            require(index == len(stages) - 1 and state["status"] == "failed", "failed stage followed by more work or success claim")
            failed = report
        rows.append(report)
    if state["status"] == "completed":
        require(len(stages) == count and failed is None and binary, "completed validation is incomplete")
    else:
        require(state.get("error"), "failed validation needs recorded failure")
    return {"path": result_path, "outcome": state["status"], "mode": mode,
            "full_workspace_validated": mode == "full-validation" and state["status"] == "completed",
            "source_revision": manifest["base_commit"],
            "source_manifest": state["source_manifest"], "source_archive": state["source_archive"],
            "runner": state["runner"], "binary": binary, "stages": rows, "failed_stage": failed,
            "recorded_error": state.get("error"),
            "unexecuted_stages": [name for name, _, _ in validation_commands(state, "python")][len(stages):],
            "source_check_scope": "Archive exact bytes/set and any retained source aliases are verified; source-after checks are recorded runner assertions, not a separately retained filesystem snapshot."}


def scaling_pins(store, plan):
    manifest, files = source(store, plan["source"], plan["pins"]["manifest"])
    require(files == plan["source_files"] and manifest["base_commit"] == plan["source_revision"], "planned source differs")
    require(store.identity(join(plan["source"], "tools/run_supervised.py")) == plan["pins"]["supervisor"],
            "supervisor is not the pinned source implementation")
    require(plan["protocol"] == retention.read_json((HERE / "protocol.json").read_bytes()), "prospective protocol differs")
    require(plan["host"]["logical_cpus"] == plan["protocol"]["logical_cpus"] == 32
            and len(plan["host"]["affinity"]) == 32, "32-CPU measurement host missing")
    for name, pin in plan["pins"].items():
        if name == "python":
            tool_identity(pin)
        else:
            store.bind(pin)
    require(content(plan["pins"]["runner"]) == retention.file_pin(HERE / "scaling-run.py"),
            "retained runner version differs from trusted verifier adapter")
    require(store.read(plan["pins"]["protocol"]["path"]) == plan["protocol"], "retained protocol bytes differ")
    require(content(plan["pins"]["binary"]) == content(plan["pins"]["compiled_binary"]), "copied benchmark differs")
    for case, pin in plan["inputs"].items():
        store.bind(pin)
        require(pin["path"] == join(plan["source"], "experiments/hu-postflop-r1/range-scaling/configs", plan["protocol"]["cases"][case]["file"]),
                "input is not the pinned source config")
    require(set(plan["inputs"]) == set(plan["protocol"]["cases"]), "input case set differs")
    require(dt.datetime.fromisoformat(plan["deadline_utc"]) <= dt.datetime.fromisoformat(plan["protocol"]["deadline_utc_latest"]), "deadline was extended")
    build = store.read(plan["pins"]["build_record"]["path"])
    require(successful(build) and build["cwd"] == plan["source"], "build provenance failed")
    require(PurePosixPath(build["argv"][0]).name == "cargo" and build["resolved_argv"][0] == build["argv"][0],
            "build resolved executable differs")
    require(all(token in build["argv"] for token in ("build", "--release", "cli", "--example", "hu_scaling_bench")), "wrong benchmark build")
    tool_paths = {pin["path"] for pin in build["identity_before"] if PurePosixPath(pin["path"]).name in ("cargo", "rustc", "rustdoc", "rustfmt", "clippy-driver")}
    archive = store.identity(join(PurePosixPath(plan["pins"]["manifest"]["path"]).parent, "source-candidate.tar.gz"))
    record_bytes(store, build, [plan["pins"]["manifest"], archive, plan["pins"]["supervisor"]], tool_paths)


@contextlib.contextmanager
def portable_runner(store, plan):
    """Reuse the trusted check/summary code with only its read operations routed."""
    class StorePath(PurePosixPath):
        def read_text(self, encoding="utf-8"):
            return store.raw(str(self)).decode(encoding)

    def verify(pin):
        if pin == plan["pins"]["python"]:
            tool_identity(pin)
        else:
            store.bind(pin)

    def equal(a, b):
        require(content(a) == content(b), "canonical identity differs")
        store.bind(a)
        store.bind(b)
        store.equal(a["path"], b["path"])

    replacement = {"Path": StorePath, "read": lambda path: store.read(str(path)),
                   "identity": lambda path: store.identity(str(path)), "verify": verify,
                   "pins": lambda _plan, live: require(not live, "portable verifier cannot perform live checks"),
                   "equal_files": equal}
    original = {key: getattr(runner, key) for key in replacement}
    try:
        for key, value in replacement.items():
            setattr(runner, key, value)
        yield StorePath
    finally:
        for key, value in original.items():
            setattr(runner, key, value)


def scaling(store, plan_path):
    plan = store.read(plan_path)
    require(plan["schema"] == "r1.hu-range-scaling-plan/v1" and plan_path == join(plan["output"], "plan.json"), "wrong scaling plan")
    scaling_pins(store, plan)
    state = store.read(join(plan["output"], "result.json"))
    require(state["schema"] == "r1.hu-range-scaling-result/v1", "wrong scaling result")
    require(state["status"] == "completed", "portable performance verification requires completed campaign; retain failed raw proof separately")
    # Bind every raw output and identity before the reused checker sees them.
    for entry in state["stages"]:
        store.bind(entry["record"])
        record = store.read(entry["record"]["path"])
        require(record["resolved_argv"][0] == plan["pins"]["binary"]["path"], "sample resolved executable differs")
        record_bytes(store, record, list(plan["pins"].values()) + list(plan["inputs"].values()), {plan["pins"]["python"]["path"]})
        require(record["runtime"]["logical_cpus"] == 32, "sample did not run on 32-CPU host")
        for when in ("containment_before", "containment_after"):
            limits = entry[when]
            require(0 < limits["effective_memory_max_bytes"] <= plan["protocol"]["limits"]["outer_memory_max_bytes"], "sample cgroup memory differs")
            require(limits["effective_cpu_quota"] is None or limits["effective_cpu_quota"] >= 32, "sample CPU quota is insufficient")
            require(any(item.get("memory.swap.max") == "0" for item in limits["ancestors"]), "sample swap is not disabled")
        for pin in entry["sample"]["artifacts"].values():
            store.bind(pin)
        store.bind(entry["sample"]["report"])
    with portable_runner(store, plan) as StorePath:
        summary = runner.check(StorePath(plan["output"]))
    return {"path": plan_path, "outcome": "completed", "pilot_count": 4, "warm_measured_count": 112,
            "canonical_original_bytes_equal": True, "summary": summary,
            "runner": plan["pins"]["runner"], "binary": plan["pins"]["binary"]}


def verify(directories):
    store = Store(directories)
    validations, plans = [], []
    # Original result/plan paths are enough; do not interpret arbitrary JSON.
    for path in list(store.paths):
        if path.endswith("/result.json"):
            document = store.read(path)
            if document.get("schema") == "r1.range-scaling-validation/v1":
                validations.append(validation(store, path))
        elif path.endswith("/plan.json"):
            document = store.read(path)
            if document.get("schema") == "r1.hu-range-scaling-plan/v1":
                plans.append(path)
    measurements = [scaling(store, path) for path in plans]
    require(validations or measurements, "no range/scaling evidence found")
    return {"schema": "r1.range-scaling-portable-verification/v1", "payload_integrity": "verified",
            "validation": validations, "scaling": measurements, "recovered_content_aliases": store.recovered_aliases,
            "identity_only": "Compiler and Python executables are bound by recorded size/SHA; their executable bytes need not be retained.",
            "retention_container": "r1.context-linux-retention/v1 is reused only for exact compressed payload storage, not as a claim of codec experiment validation."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--retained", type=Path, action="append", required=True)
    parser.add_argument("--expect-validation", choices=("completed", "failed"))
    parser.add_argument("--expect-scaling", choices=("completed",))
    args = parser.parse_args()
    report = verify(args.retained)
    if args.expect_validation:
        require(report["validation"] and all(item["outcome"] == args.expect_validation for item in report["validation"]), "unexpected validation outcome")
    if args.expect_scaling:
        require(report["scaling"] and all(item["outcome"] == args.expect_scaling for item in report["scaling"]), "unexpected scaling outcome")
    print(json.dumps(report, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
