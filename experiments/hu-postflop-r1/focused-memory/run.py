"""Focused native child RSS campaign; trusted reference proof02 is required."""
from __future__ import annotations
import argparse
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("trusted_memory_final_pipeline", HERE.parent / "final-pipeline/run.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
core = base.core
require, read, save, identity, content, join = base.require, base.read, base.save, base.identity, base.content, base.join


def controls():
    return {name: HERE / name for name in ("run.py", "protocol.json", "native_rss.c", "calibration.py")}


def schedule(protocol):
    solves = [stage for stage in base.schedule(protocol) if stage["kind"] == "solve"]
    return ([{"kind": kind, "label": kind} for kind in ("cc-version", "compile", "calibration")]
            + solves + [{**stage, "kind": kind, "label": stage["label"].removesuffix("solve") + kind}
                        for stage in solves for kind in ("summary", "audit", "decode-all")])


def reference(store, plan, directory):
    directory = Path(directory)
    for name, pin in plan["reference"].items():
        store.verify(pin)
        require(content(pin) == plan["memory_protocol"]["reference_files"][name], "fixed proof02 reference pin differs")
        require(content(identity(directory / name)) == content(pin), "external reference bytes differ: " + name)
    report = base.check(directory)
    require(report["status"] == "completed" and report["processes_passed"] == 156, "reference proof02 incomplete")
    return read(directory / "result.json")


def calibration_result(store, plan, state, stage, record):
    directory = join(base.stage_dir(plan, stage), "calibration")
    report = store.json(join(directory, "result.json"))
    gates = plan["memory_protocol"]["calibration"]
    require(report["schema"] == "r1.native-rss-calibration/v1" and report["status"] == "passed", "memory calibration failed")
    require(report["launcher"] == state["native"] and report["parent_allocation_bytes"] == gates["python_allocation_bytes"], "calibration launcher/allocation differs")
    require(all(base.finite(report[key]) and report[key] >= gates["python_vmrss_min_kib"]
                for key in ("parent_vmrss_kib", "parent_vmrss_kib_after")), "large calibration parent missing")
    require(base.finite(report["elapsed_seconds"]) and 0 < report["elapsed_seconds"] < 60
            and report["elapsed_seconds"] <= record["elapsed_seconds"], "calibration elapsed invalid")
    require([case["case"] for case in report["cases"]] == ["small", "large"], "calibration case order differs")
    peaks = []
    for case in report["cases"]:
        label = case["case"]
        child_argv = [state["native"]["path"], "--allocate", str(gates[label + "_bytes"])]
        report_path = join(directory, label + ".native.json")
        require(case["allocation_bytes"] == gates[label + "_bytes"] and case["launcher_returncode"] == 0
                and case["command"] == [state["native"]["path"], "--report", report_path, "--", *child_argv], "calibration command differs")
        require(case["parent_vmrss_kib_before"] >= gates["python_vmrss_min_kib"], "calibration parent pages lost")
        require(case["native_report"]["path"] == report_path, "calibration report path differs")
        for key in ("native_report", "stdout", "stderr"):
            store.verify(case[key])
            require(case[key]["path"].startswith(directory + "/"), "calibration artifact outside stage")
        native = store.json(report_path)
        require(native == case["native"] and native["schema"] == "r1.native-rss/v1"
                and native["source"] == "wait4.ru_maxrss_linux_kib" and native["argv"] == child_argv
                and isinstance(native["child_pid"], int) and native["child_pid"] > 0 and native["exit_code"] == 0
                and native["signaled"] is False and native["term_signal"] is None, "calibration native binding invalid")
        require(base.finite(case["elapsed_seconds"]) and base.finite(native["elapsed_seconds"])
                and 0 < native["elapsed_seconds"] <= case["elapsed_seconds"] <= report["elapsed_seconds"], "calibration case time invalid")
        require(0 < native["launcher_before_fork"]["vmrss_kib"] < gates["launcher_vmrss_max_kib"], "calibration launcher too large")
        require(isinstance(native["ru_maxrss_kib"], int) and native["ru_maxrss_kib"] > 0, "calibration counter invalid")
        peaks.append(native["ru_maxrss_kib"])
    require(peaks[0] < gates["small_peak_max_kib"] and gates["large_peak_min_kib"] <= peaks[1] <= gates["large_peak_max_kib"]
            and peaks[1] > peaks[0] + gates["large_minus_small_min_kib"], "calibration counter response outside fixed gates")
    return report


def commands(plan, state, stage):
    kind = stage["kind"]
    output = join(plan["output"], "stages", stage["label"])
    native = join(plan["output"], "native-rss")
    if kind == "cc-version":
        return [plan["tools"]["cc"]["path"], "--version"]
    if kind == "compile":
        return [plan["tools"]["cc"]["path"], "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                plan["runner_controls"]["native_rss.c"]["path"], "-o", native]
    if kind == "calibration":
        return [plan["python"]["path"], plan["runner_controls"]["calibration.py"]["path"],
                "--launcher", native, "--out", join(output, "calibration")]
    command = base.command(plan, state, stage)
    if kind == "solve":
        return [native, "--report", join(output, "native.json"), "--", *command]
    return command


def comparison(kind, value):
    if kind == "solve":
        return {"live": value["live"], "trajectory": value["trajectory"],
                "checkpoint": content(value["artifacts"]["checkpoint.ckpt"]), "config": content(value["artifacts"]["run.toml"])}
    if kind == "summary":
        return value["report"]
    if kind == "audit":
        return {k: value[k] for k in ("quality", "pre_save", "economics")}
    return value["canonical"]


def native_result(store, plan, stage, record, state):
    result = store.json(join(base.stage_dir(plan, stage), "native.json"))
    require(result["schema"] == "r1.native-rss/v1" and result["source"] == "wait4.ru_maxrss_linux_kib"
            and result["argv"] == base.command(plan, state, stage), "native child invocation differs")
    require(result["exit_code"] == 0 and result["signaled"] is False and result["term_signal"] is None,
            "native child did not finish cleanly")
    require(isinstance(result["child_pid"], int) and result["child_pid"] > 0 and result["child_pid"] != record["pid"], "native child PID invalid")
    require(isinstance(result["ru_maxrss_kib"], int) and result["ru_maxrss_kib"] > 0, "native child peak invalid")
    require(base.finite(result["elapsed_seconds"]) and 0 < result["elapsed_seconds"] <= record["elapsed_seconds"], "native timer invalid")
    require(0 < result["launcher_before_fork"]["vmrss_kib"] < plan["memory_protocol"]["calibration"]["launcher_vmrss_max_kib"], "launcher was not small before fork")
    samples = [base.decode(line) for line in store.data(record["outputs"]["samples"]["path"]).splitlines() if line.strip()]
    require(all(set(row["pids"]) <= {record["pid"], result["child_pid"]} for row in samples), "unexpected subprocess observed")
    return result


def validate_stage(store, plan, state, entry, record, prior):
    stage = entry["stage"]
    kind = stage["kind"]
    if kind == "cc-version":
        require(store.data(record["outputs"]["stdout"]["path"]).strip(), "empty compiler identity")
        return None
    if kind == "compile":
        store.verify(state["native"])
        return None
    if kind == "calibration":
        return calibration_result(store, plan, state, stage, record)
    value = base.sample(store, plan, stage, record)
    prior_sample = next(row["sample"] for row in prior["stages"] if row["stage"]["label"] == stage["label"] and row["status"] == "passed")
    require(comparison(kind, value) == comparison(kind, prior_sample), "own-arm proof02 quality/artifacts differ: " + stage["label"])
    if kind == "solve":
        value["native_child"] = native_result(store, plan, stage, record, state)
    return value


def summary(plan, state):
    result = {}
    for case in plan["protocol"]["cases"]:
        rows = {arm: [row["sample"]["native_child"]["ru_maxrss_kib"] for row in state["stages"]
                      if row["status"] == "passed" and row["stage"]["kind"] == "solve" and row["stage"]["case"] == case
                      and row["stage"]["arm"] == arm and not row["stage"]["warmup"]] for arm in ("old", "new")}
        require(all(len(values) == 3 and min(values) > 0 for values in rows.values()), "missing measured native counter")
        ratio = max(rows["new"]) / min(rows["old"])
        result[case] = {"ru_maxrss_kib": rows, "max_new_over_min_old": ratio, "screen": ratio <= plan["memory_protocol"]["ratio_max"]}
    return {"cases": result, "scope": plan["memory_protocol"]["metric"], "proof02_screen_changed": False}


def pins(plan, state, stage):
    result = [*plan["runner_controls"].values(), *plan["controls"].values(), *plan["inputs"].values(), plan["supervisor"],
              plan["python"], plan["tools"]["cc"], *plan["reference"].values(),
              *[pin for arm in plan["arms"].values() for pin in (arm["manifest"], arm["archive"])]]
    if stage["kind"] not in ("cc-version", "compile"):
        result.append(state["native"])
    if "arm" in stage:
        result += list(state["binaries"][stage["arm"]].values())
    return result


def check(out, reference_dir):
    out = Path(out)
    store = core.Store(out)
    if (out / "prepare-failure.json").exists():
        failure = read(out / "prepare-failure.json")
        require(failure["status"] == "failed" and failure.get("error"), "invalid preparation failure")
        return {"schema": "r1.focused-memory-verification/v1", "status": "failed", "payload_integrity": "verified",
                "provenance_complete": False, "processes_passed": 0,
                "scope": "Available retained CAS bytes verified by Store; preparation failed before complete source/plan/measurement binding",
                "error": failure["error"]}
    plan, state = read(out / "plan.json"), read(out / "result.json")
    require(plan["memory_protocol"] == read(HERE / "protocol.json"), "memory protocol differs")
    require(plan["schema"] == "r1.focused-memory-plan/v1" and state["schema"] == "r1.focused-memory-result/v1", "schema differs")
    for name, path in controls().items():
        require(content(identity(path)) == content(plan["runner_controls"][name]), "trusted memory control differs")
        store.verify(plan["runner_controls"][name])
    prior = reference(store, plan, reference_dir)
    prior_plan = store.json(plan["reference"]["plan.json"]["path"])
    prior_build = store.json(plan["reference"]["build.json"]["path"])
    require(plan["arms"] == prior_plan["arms"] and state["binaries"] == prior_build["binaries"]
            and plan["inputs"] == prior_plan["inputs"] and plan["protocol"] == prior_plan["protocol"]
            and plan["controls"] == prior_plan["controls"] and plan["supervisor"] == prior_plan["supervisor"], "reference production/input bindings differ")
    require(plan["environment"] == prior_plan["environment"] == base.FIXED_ENV
            and plan["controls"]["protocol.json"]["sha256"] == plan["memory_protocol"]["reference_protocol_sha256"], "fixed environment/reference protocol differs")
    require(plan["host"]["boot_id"] == prior_plan["host"]["boot_id"], "memory campaign changed boot")
    core.host_record(plan["host"], plan["protocol"])
    require(base.timestamp(plan["deadline_utc"]) <= base.timestamp(plan["memory_protocol"]["deadline_ceiling_utc"]), "deadline exceeded ceiling")
    base.expected_sources(store, plan)
    for table in state["binaries"].values():
        for pin in table.values():
            store.verify(pin)
    if "native" in state:
        require(state["native"]["path"] == join(plan["output"], "native-rss"), "native build output path differs")
        store.verify(state["native"])
    plan_pin = store.pin(join(plan["output"], "plan.json"))
    require(store.data(plan_pin["path"]) == (out / "plan.json").read_bytes(), "plan bytes changed")
    base.suffix(state["stages"], state["status"], schedule(plan["protocol"]), ())
    previous_end = base.timestamp(plan["created_at"])
    for entry in state["stages"]:
        if entry["status"] in ("pending", "skipped"):
            continue
        st = entry["stage"]
        record, previous_end = base.verify_record(store, plan, entry, commands(plan, state, st), plan["output"], 300,
            previous_end, [*pins(plan, state, st), plan_pin], passed=entry["status"] == "passed")
        if entry["status"] == "passed":
            require(validate_stage(store, plan, state, entry, record, prior) == entry["sample"], "derived stage differs")
    if state["status"] == "completed":
        require(state["summary"] == summary(plan, state), "native summary differs")
    else:
        require(state.get("error") and "summary" not in state, "failed result invalid")
    return {"schema": "r1.focused-memory-verification/v1", "status": state["status"],
            "processes_passed": sum(e["status"] == "passed" for e in state["stages"]),
            "payload_integrity": "verified", "provenance_complete": True, "summary": state.get("summary"),
            "reference": "proof02 externally verified with trusted checkout code"}


def execute(args):
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    store = core.Store(out, create=True)
    try:
        memory = read(HERE / "protocol.json")
        require(time.time() + 320 < base.timestamp(args.deadline_utc) <= base.timestamp(memory["deadline_ceiling_utc"]), "deadline invalid")
        ref = args.reference_proof.resolve()
        reference_pins = {}
        for name, expected in memory["reference_files"].items():
            pin = identity(ref / name)
            require(content(pin) == expected, "reference proof02 pin differs")
            reference_pins[name] = store.add(ref / name)
        prior_plan, prior_build = read(ref / "plan.json"), read(ref / "build.json")
        require(prior_plan["controls"]["protocol.json"]["sha256"] == memory["reference_protocol_sha256"], "reference protocol differs")
        plan = {"schema": "r1.focused-memory-plan/v1", "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                "output": str(out), "deadline_utc": args.deadline_utc, "memory_protocol": memory,
                "reference": reference_pins,
                "external_reference_archive_identity_only": identity(args.reference_archive.resolve()),
                "runner_controls": {name: store.add(path) for name, path in controls().items()},
                "python": identity(sys.executable), "tools": {"cc": identity(args.cc.resolve())},
                **{key: prior_plan[key] for key in ("protocol", "arms", "inputs", "controls", "supervisor", "environment")}}
        plan["host"] = core.host(plan["protocol"])
        require(plan["host"]["boot_id"] == prior_plan["host"]["boot_id"], "boot changed")
        for pin in [*plan["controls"].values(), *plan["inputs"].values(), plan["supervisor"],
                    *[p for arm in plan["arms"].values() for p in (arm["archive"], arm["manifest"])],
                    *[p for table in prior_build["binaries"].values() for p in table.values()]]:
            require(store.add(pin["path"]) == pin, "original pinned file changed")
        base.expected_sources(store, plan)
        base.inventory(plan)
        prior = reference(store, plan, ref)
        save(out / "plan.json", plan)
        store.add(out / "plan.json")
        state = {"schema": "r1.focused-memory-result/v1", "status": "running", "binaries": prior_build["binaries"],
                 "stages": [{"stage": stage, "status": "pending"} for stage in schedule(plan["protocol"])]}
        save(out / "result.json", state)
    except BaseException as error:
        save(out / "prepare-failure.json", {"status": "failed", "error": repr(error)})
        raise
    os.environ.update(plan["environment"])
    for key in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_ENCODED_RUSTFLAGS", "R1_PHASE_OUTPUT", "R1_SOL_WRITE_PHASE_OUTPUT"):
        os.environ.pop(key, None)
    supervisor = base.load("trusted_memory_supervisor", plan["supervisor"]["path"])
    try:
        for entry in state["stages"]:
            stage = entry["stage"]
            require(time.time() + 320 < base.timestamp(plan["deadline_utc"]), "insufficient fixed deadline")
            entry.update(status="running", host_before=core.host(plan["protocol"]), source_before=base.inventory(plan))
            require(entry["host_before"] == plan["host"], "host changed")
            save(out / "result.json", state)
            directory = out / "stages" / stage["label"]
            directory.mkdir(parents=True, exist_ok=False)
            identity_pins = [*pins(plan, state, stage), store.pin(join(plan["output"], "plan.json"))]
            argv = ["--record", str(directory / "supervisor.json"), "--cwd", str(out), "--disk-path", str(out)]
            limits = plan["protocol"]["limits"]
            for key, value in {"timeout_seconds": 300, "memory_limit_bytes": limits["rss_bytes"], "min_free_memory_bytes": limits["min_free_bytes"],
                               "disk_reserve_bytes": limits["disk_reserve_bytes"], "poll_seconds": limits["poll_seconds"],
                               "grace_seconds": limits["grace_seconds"], "kill_wait_seconds": limits["kill_wait_seconds"]}.items():
                argv += ["--" + key.replace("_", "-"), str(value)]
            for pin in identity_pins:
                require(identity(pin["path"]) == pin, "input changed before launch")
                argv += ["--identity-file", pin["path"]]
            require(time.time() + 320 < base.timestamp(plan["deadline_utc"]), "deadline consumed by identities")
            code = supervisor.main([*argv, "--", *commands(plan, state, stage)])
            entry.update(supervisor_exit=code, record=store.add(directory / "supervisor.json"))
            base.retain_stage(store, directory, [plan["python"], *plan["tools"].values()])
            require(code == 0, "supervised process failed")
            record = core.record_bytes(store, entry["record"], [plan["python"], *plan["tools"].values()])
            if stage["kind"] == "compile":
                state["native"] = store.add(out / "native-rss")
            entry["sample"] = validate_stage(store, plan, state, entry, record, prior)
            entry.update(host_after=core.host(plan["protocol"]), source_after=base.inventory(plan))
            require(entry["host_after"] == plan["host"] and entry["source_after"] == entry["source_before"], "host/source changed")
            entry["status"] = "passed"
            save(out / "result.json", state)
            print(json.dumps({"stage": stage["label"], "status": "passed"}), flush=True)
        state.update(status="completed", summary=summary(plan, state))
        save(out / "result.json", state)
    except BaseException as error:
        state.update(status="failed", error=repr(error))
        state.pop("summary", None)
        for entry in state["stages"]:
            if entry["status"] == "running":
                entry.update(status="failed", error=repr(error))
                directory = out / "stages" / entry["stage"]["label"]
                try:
                    if (directory / "supervisor.json").exists():
                        entry["record"] = store.add(directory / "supervisor.json", changed_identity=True)
                    base.retain_stage(store, directory, [plan["python"], *plan["tools"].values()])
                except (OSError, ValueError) as retained:
                    entry["retention_error"] = repr(retained)
            elif entry["status"] == "pending":
                entry.update(status="skipped", reason=repr(error))
        save(out / "result.json", state)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", choices=("run", "check"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--reference-proof", type=Path, required=True)
    parser.add_argument("--reference-archive", type=Path)
    parser.add_argument("--cc", type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args()
    if args.phase == "check":
        print(json.dumps(check(args.out, args.reference_proof), indent=2, allow_nan=False))
    else:
        require(args.reference_archive and args.cc and args.deadline_utc, "run arguments missing")
        execute(args)


if __name__ == "__main__":
    main()
