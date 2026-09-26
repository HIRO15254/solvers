"""Portable current-phases evidence checks; never execute retained code."""
from __future__ import annotations

import argparse
import difflib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import re
import stat
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent


def load_trusted(name, path):
    """Only callers supply checkout-relative paths, never evidence paths."""
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


core = load_trusted("trusted_current_phases_checker_store", HERE.parent / "showdown-kernel/run.py")
require = core.require


def regular_file(path):
    require(stat.S_ISREG(path.lstat().st_mode) and not path.is_symlink(),
            "retained input is not a regular file: " + str(path))


def retained_store(out):
    """Validate the complete CAS, including identity versions, before interpretation."""
    regular_file(out / "retention.json")
    payload = out / "payload"
    require(payload.is_dir() and not payload.is_symlink(), "invalid payload directory")
    index = core.read(out / "retention.json")
    require(index.get("schema") == "r1.showdown-kernel-retention/v1", "retention schema")
    require(isinstance(index.get("files"), dict) and isinstance(index.get("identity_versions", []), list),
            "retention inventory types")
    versions = index.get("identity_versions", [])
    require(all(isinstance(pin, dict) for pin in versions), "retention version pin type")
    pins = list(index["files"].items()) + [(pin.get("path"), pin) for pin in versions]
    for original, pin in pins:
        require(isinstance(pin, dict) and set(pin) == {"path", "bytes", "sha256"}, "retention pin shape")
        require(isinstance(original, str) and original and pin["path"] == original, "retention original path")
        require(type(pin["bytes"]) is int and pin["bytes"] >= 0, "retention byte count")
        require(isinstance(pin["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", pin["sha256"]), "retention SHA")
        regular_file(payload / pin["sha256"])
    return core.Store(out)


def terminal_suffix(state, expected):
    """No retry, gap, or execution after the first failure may be certified."""
    require(state.get("status") in ("completed", "failed"), "campaign is not terminal")
    entries = state.get("stages")
    require(isinstance(entries, list) and all(isinstance(row, dict) for row in entries), "stage entries")
    require([row.get("stage") for row in entries] == expected, "fixed stage schedule differs")
    statuses = [row.get("status") for row in entries]
    require(all(value in ("passed", "failed", "skipped") for value in statuses), "nonterminal stage status")
    if state["status"] == "completed":
        require(all(value == "passed" for value in statuses), "completed campaign is incomplete")
    else:
        require(isinstance(state.get("error"), str) and state["error"], "missing failure reason")
        require(state.get("summary") is None, "failed campaign claims summary")
        first = next((i for i, value in enumerate(statuses) if value != "passed"), len(statuses))
        require(all(value == "skipped" for value in statuses[first + 1:]), "execution/retry after first failure")
    for row in entries:
        if row["status"] == "passed":
            require("record" in row, "passed stage lacks supervisor record")
        elif row["status"] == "skipped":
            require(not any(key in row for key in ("record", "sample", "supervisor_exit")),
                    "skipped stage contains execution evidence")
    return {key: statuses.count(key) for key in ("passed", "failed", "skipped")}


def runner_module():
    return load_trusted("trusted_current_phases_checker_runner", HERE / "runner.py")


def absolute_identifier(value):
    require(isinstance(value, str) and value.startswith("/") and "\\" not in value
            and all(part not in ("", ".", "..") for part in value.split("/")[1:]),
            "invalid original absolute path")
    return value


def validate_plan(store, plan, runner):
    require(plan.get("schema") == "r1.current-phases-plan/v1", "plan schema")
    absolute_identifier(plan["output"])
    absolute_identifier(plan["workspace"])
    out_id, workspace_id = PurePosixPath(plan["output"]), PurePosixPath(plan["workspace"])
    require(not out_id.is_relative_to(workspace_id) and not workspace_id.is_relative_to(out_id), "workspace/output overlap")
    require(plan["protocol"] == core.read(HERE / "protocol.json"), "frozen protocol differs")
    require(plan["quality_protocol"] == core.read(HERE.parent / "final-pipeline/protocol.json"), "quality protocol differs")
    paths = runner.controls()
    require(set(plan["controls"]) == set(paths), "trusted control set differs")
    for name, pin in plan["controls"].items():
        store.verify(pin)
        require(core.content(pin) == core.content(core.identity(paths[name])), "trusted control differs: " + name)
    frozen = core.read(HERE / "freeze.json")
    for name, expected in {**frozen["files"], **frozen["external_inputs"]}.items():
        require(core.content(core.identity(HERE / name)) == expected, "frozen preparation differs: " + name)
    require(set(plan["inputs"]) == set(runner.CASES), "input case set differs")
    for case, pin in plan["inputs"].items():
        require(pin == plan["controls"]["configs/" + case + ".toml"], "input config binding differs")
    store.verify(plan["supervisor"])
    supervisor = HERE.parent / "tools/run_supervised.py"
    if not supervisor.is_file():
        supervisor = HERE.parents[2] / "tools/run_supervised.py"
    require(core.content(plan["supervisor"]) == core.content(core.identity(supervisor)),
            "trusted supervisor differs")
    require(set(plan["tools"]) == {"rustc", "cargo", "cc"}, "tool set differs")
    for pin in [plan["python"], *plan["tools"].values()]:
        require(set(pin) == {"path", "bytes", "sha256"} and type(pin["bytes"]) is int and pin["bytes"] > 0
                and re.fullmatch(r"[0-9a-f]{64}", pin["sha256"]), "tool identity malformed")
        absolute_identifier(pin["path"])
    env = plan["environment"]
    require(set(env) == set(runner.base.FIXED_ENV) | {"RUSTC", "CARGO_NET_OFFLINE", "CARGO_HOME", "RUSTUP_HOME", "PATH",
                                                    "HOME", "TMPDIR", "LANG", "LC_ALL", "TZ"},
            "environment field set differs")
    require(all(env.get(key) == value for key, value in runner.base.FIXED_ENV.items()), "fixed environment differs")
    require(env["RUSTC"] == plan["tools"]["rustc"]["path"] and env["CARGO_NET_OFFLINE"] == "true"
            and env["PATH"] == str(PurePosixPath(env["RUSTC"]).parent) + ":/usr/bin:/bin", "tool/offline environment differs")
    require({key: env[key] for key in ("TMPDIR", "LANG", "LC_ALL", "TZ")}
            == {"TMPDIR": "/tmp", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC"}, "locale/temp environment differs")
    for name in ("CARGO_HOME", "RUSTUP_HOME", "HOME"):
        absolute_identifier(env[name])
    refs = core.read(HERE.parent / "focused-memory/protocol.json")["reference_files"]
    require(set(plan["reference"]) == set(refs), "proof02 reference set differs")
    for name, expected in refs.items():
        pin = plan["reference"][name]
        store.verify(pin)
        require(core.content(pin) == expected, "proof02 reference differs: " + name)
    validate_copies(store, plan, runner)
    store.verify(plan["launch_record"])
    require(store.json(plan["launch_record"]["path"]) == plan["launch"], "launch record bytes differ")
    launch = plan["launch"]
    require(launch["schema"] == "r1.current-phases-launch/v1", "launch schema")
    bounds = runner.deadline_contract(launch["instance_created_utc"], plan["work_deadline_utc"],
                                      plan["stop_deadline_utc"], core.timestamp(plan["created_at"]))
    require(launch["work_deadline_utc"] == plan["work_deadline_utc"] and launch["stop_deadline_utc"] == plan["stop_deadline_utc"],
            "launch deadline binding differs")
    unit_started = core.timestamp(launch["unit_started_utc"])
    require(bounds["created"] <= unit_started <= core.timestamp(plan["created_at"])
            and runner.base.finite(launch["runtime_max_seconds"])
            and 0 < launch["runtime_max_seconds"] <= bounds["work"] - unit_started, "unit lifetime exceeds work deadline")
    require(abs(runner.systemd_seconds(launch["runtime_max_systemd"]) - launch["runtime_max_seconds"]) <= .000001,
            "unit text/numeric lifetime differs")
    machine = plan["host"]
    core.host_record(machine, {"limits": {"outer_memory_max_bytes": 12884901888}})
    require(machine["boot_id"] == launch["boot_id"] and machine["kernel"] and machine["cpu_flags"], "host/boot identity missing")
    cg, unit = machine["cgroup"], machine["unit"]
    require(cg["cpu_weight"] == "100" and unit["ControlGroup"] == cg["path"].removeprefix("/sys/fs/cgroup")
            and unit["KillMode"] == "control-group" and unit["SendSIGKILL"] == "yes" and unit["CPUWeight"] == "100"
            and unit["MemoryMax"] == "12884901888" and unit["MemorySwapMax"] == "0" and unit["ActiveState"] == "active"
            and unit["RuntimeMaxUSec"] == launch["runtime_max_systemd"], "recorded unit containment differs")


def validate_copies(store, plan, runner):
    pins = core.read(HERE / "source-pins.json")
    require(set(plan["copies"]) == {"plain", "instrumented"}, "source copy set differs")
    probe_name = "crates/cli/examples/hu_pipeline_probe.rs"
    probe = core.content(plan["controls"]["final/hu_pipeline_probe.rs"])
    source_paths = []
    for arm, info in plan["copies"].items():
        require(info["source"] == core.join(plan["workspace"], "sources", arm)
                and info["target"] == core.join(plan["workspace"], "targets", arm), "source/target path differs")
        require(info["manifest"] == store.pin(core.join(info["source"], "source-copy.json")), "copy manifest binding differs")
        manifest = store.json(info["manifest"]["path"])
        after = {**pins["files"], **(pins["instrumented_changed"] if arm == "instrumented" else {runner.prep.CODEC: pins["codec_input"]})}
        require(manifest["schema"] == "r1.current-phases-source-copy/v1" and manifest["status"] == "prepared_not_built"
                and manifest["source_revision"] == runner.prep.REVISION and manifest["mode"] == arm
                and manifest["instrumentation_id"] == runner.prep.instrumentation_id()
                and manifest["output_path"] == info["source"] and manifest["before"] == pins["files"]
                and manifest["after"] == after, "source copy provenance differs: " + arm)
        source_paths.append(absolute_identifier(manifest["source_path"]))
        control_names = ("prepare.py", "runtime.rs.in", "source-pins.json", "protocol.json", "codec-input.rs")
        require(manifest["controls"] == {name: core.content(plan["controls"][name]) for name in control_names}, "copy controls differ")
        patch_path = core.join(info["source"], "instrumentation.patch")
        if arm == "instrumented":
            patch = (HERE / "generated-review.patch").read_bytes()
        else:
            codec = (HERE / "codec-input.rs").read_bytes().decode()
            patch = "".join(difflib.unified_diff([], codec.splitlines(True), fromfile="a/" + runner.prep.CODEC,
                                              tofile="b/" + runner.prep.CODEC)).encode()
        require(store.data(patch_path) == patch, "generated patch differs: " + arm)
        expected = {**after, probe_name: probe, "source-copy.json": core.content(info["manifest"]),
                    "instrumentation.patch": core.digest(patch)}
        require(info["files"] == expected, "source overlay file set/content differs: " + arm)
        prefix = info["source"] + "/"
        retained_names = {path[len(prefix):] for path in store.entries if path.startswith(prefix)}
        require(retained_names == set(expected), "retained source closure differs: " + arm)
        for name, content_pin in expected.items():
            require(core.content(store.pin(core.join(info["source"], name))) == content_pin, "retained source bytes differ: " + name)
    require(len(set(source_paths)) == 1, "copies have different original sources")
    original = PurePosixPath(source_paths[0])
    for root in (plan["output"], plan["workspace"]):
        other = PurePosixPath(root)
        require(not original.is_relative_to(other) and not other.is_relative_to(original), "original/work/output roots overlap")


def expected_identity_pins(store, plan, active, stage, runner):
    pins = runner.common_pins(plan, active) + [store.pin(core.join(plan["output"], "plan.json"))]
    if stage["kind"] in ("audit", "checkpoint", "stream-write", "decode-all", "read-root"):
        directory = runner.solve_dir(plan, runner.input_stage(stage))
        pins += [store.pin(core.join(directory, name)) for name in ("solution.sol", "checkpoint.ckpt", "run.toml")]
    return pins


def activate_binary(store, plan, state, active, stage, *, required):
    kind = stage["kind"]
    if kind == "build":
        arm = stage["arm"]
        bins = state["binaries"].get(arm)
        require(bins is not None or not required, "successful build lacks binary pins")
        if bins is None:
            return
        require(set(bins) == {"cli", "codec", "audit", "probe"}, "built executable set differs")
        relative = {"cli": "release/solvers", "codec": "release/examples/current_phase_codec",
                    "audit": "release/examples/hu_saved_profile_audit", "probe": "release/examples/hu_pipeline_probe"}
        for name, pin in bins.items():
            require(pin["path"] == core.join(plan["copies"][arm]["target"], relative[name]), "built executable path differs")
            store.verify(pin)
        active["binaries"][arm] = bins
    elif kind in ("compile-native", "compile-memory"):
        name = "native-rss" if kind == "compile-native" else "memory-probe"
        pin = state["native_binaries"].get(name)
        require(pin is not None or not required, "successful native compile lacks binary pin")
        if pin is not None:
            require(pin["path"] == core.join(plan["output"], name), "native executable path differs")
            store.verify(pin)
            active["native_binaries"][name] = pin


def supervisor_identity(plan, entry, command):
    supplied = entry["identity_pins"]
    by_path = {pin["path"]: pin for pin in supplied}
    require(command[0] in by_path, "supervised executable is not pinned")
    ordered = [by_path[command[0]], plan["python"], plan["supervisor"], *supplied]
    result, seen = [], {}
    for pin in ordered:
        require(pin["path"] not in seen or seen[pin["path"]] == pin, "conflicting supervisor identity")
        if pin["path"] not in seen:
            seen[pin["path"]] = pin
            result.append(pin)
    return result


def check(out):
    out = Path(out).resolve(strict=True)
    store = retained_store(out)
    runner = runner_module()
    regular_file(out / "result.json")
    state_raw = (out / "result.json").read_bytes()
    state = core.decode(state_raw)
    require(state.get("schema") == "r1.current-phases-result/v1", "result schema")
    counts = terminal_suffix(state, runner.schedule())
    if not (out / "plan.json").exists():
        require(state["status"] == "failed" and counts["skipped"] == len(runner.schedule())
                and not state.get("binaries") and not state.get("native_binaries"), "invalid prepare failure")
        require(any(path.endswith("/result.json") and store.data(path) == state_raw for path in store.entries),
                "prepare-failure original result bytes missing")
        return {"schema": "r1.current-phases-verification/v1", "status": "failed", "scope": "failed_prepare",
                "provenance_complete": False, "payload_integrity": "verified", "counts": counts,
                "error": state["error"], "summary": None}
    regular_file(out / "plan.json")
    plan_raw = (out / "plan.json").read_bytes()
    plan = core.decode(plan_raw)
    for name, raw in (("plan.json", plan_raw), ("result.json", state_raw)):
        require(store.data(core.join(plan["output"], name)) == raw, "original metadata bytes differ: " + name)
    validate_plan(store, plan, runner)
    require(isinstance(state.get("binaries"), dict) and isinstance(state.get("native_binaries"), dict), "binary inventories missing")
    active = {"binaries": {}, "native_binaries": {}}
    previous_end = core.timestamp(plan["created_at"])
    derived = []
    for entry in state["stages"]:
        stage, status = entry["stage"], entry["status"]
        if status == "skipped":
            continue
        if "identity_pins" in entry:
            require(entry["identity_pins"] == expected_identity_pins(store, plan, active, stage, runner),
                    "stage identity set differs from completed prefix")
        if "record" in entry:
            require("identity_pins" in entry, "executed stage lacks identity pins")
            require(entry["record"]["path"] == core.join(plan["output"], "stages", stage["label"], "supervisor.json"),
                    "stage record path differs")
            record = core.record_bytes(store, entry["record"], [plan["python"], *plan["tools"].values()], success=status == "passed")
            require(record["argv"] == record["resolved_argv"] == runner.command(plan, active, stage)
                    and record["cwd"] == runner.stage_cwd(plan, stage), "stage command differs")
            require(entry["environment"] == runner.stage_environment(plan, stage), "stage environment differs")
            require(all(record["limits"][key] == value for key, value in runner.supervisor_limits(stage).items()), "stage limits differ")
            created = core.timestamp(record["created_at"])
            require(previous_end <= created and created + stage["timeout"] + 20 < core.timestamp(plan["work_deadline_utc"]),
                    "stage chronology/start deadline differs")
            if record.get("ended_at") is not None:
                ended = core.timestamp(record["ended_at"])
                require(created <= ended <= core.timestamp(plan["work_deadline_utc"]), "stage end deadline differs")
                if record.get("started_at") is not None:
                    require(created <= core.timestamp(record["started_at"]) <= ended, "child chronology differs")
                previous_end = ended
            if status == "passed":
                require(record["identity_before"] == supervisor_identity(plan, entry, record["argv"]), "supervisor identity pins differ")
                require(runner.base.finite(record["elapsed_seconds"]) and 0 < record["elapsed_seconds"] <= stage["timeout"] + 15,
                        "stage duration exceeds finite bound")
        if status == "passed":
            actual = runner.verify_stage(store, plan, state, entry)
            require(actual == entry.get("sample"), "derived stage sample differs")
            derived.append(entry)
        activate_binary(store, plan, state, active, stage, required=status == "passed")
    require(active["binaries"] == state["binaries"] and active["native_binaries"] == state["native_binaries"],
            "unbuilt/future binary appears in result")
    if state["status"] == "completed":
        require(previous_end <= core.timestamp(state["completed_at"]) <= core.timestamp(plan["work_deadline_utc"]),
                "final quality checks exceeded global work deadline")
    comparison_error = None
    try:
        runner.compare_completed(store, plan, state, complete=state["status"] == "completed")
    except (ValueError, KeyError) as error:
        require(state["status"] == "failed", "completed cross-artifact comparison failed: " + str(error))
        comparison_error = str(error)
    summary = runner.summary(state) if state["status"] == "completed" else None
    require(state.get("summary") == summary, "recomputed summary differs")
    return {"schema": "r1.current-phases-verification/v1", "status": state["status"],
            "scope": "current source fixed synthetic cases", "provenance_complete": True,
            "payload_integrity": "verified", "counts": counts, "summary": summary,
            "error": state.get("error"), "comparison_error": comparison_error,
            "limitations": ["No retained code executed; compiler/Python executable identities only.",
                            "Recorded host/source checks are not an independent attestation of the original machine.",
                            "Memory values are Linux process counters, not exact physical phase memory.",
                            "No external reference or overall R1 acceptance is certified."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expect", choices=("completed", "failed"))
    args = parser.parse_args()
    result = check(args.out)
    if args.expect:
        require(result["status"] == args.expect, "unexpected terminal state")
    print(json.dumps(result, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
