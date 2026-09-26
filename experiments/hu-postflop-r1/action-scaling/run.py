"""Finite old/new action-scaling comparison. Builds and VM lifecycle are external."""
from __future__ import annotations
import argparse
import datetime as dt
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import statistics
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
RANGE = HERE.parent / "range-scaling"
spec = importlib.util.spec_from_file_location("action_range_portable", RANGE / "verify-retained.py")
portable = importlib.util.module_from_spec(spec)
spec.loader.exec_module(portable)
helper = portable.runner
require, join, content = helper.require, portable.join, helper.content
ROLES = ("old", "new")


def live_identity(path):
    path = Path(path).resolve(strict=True)
    before = path.stat()
    pin = portable.retention.file_pin(path)
    after = path.stat()
    require((before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns), "file changed while hashing")
    return {"path": str(path), **pin}


class LiveStore(portable.Store):
    def __init__(self):
        self.paths, self.virtual = {}, {}
    def open(self, path):
        return Path(path).open("rb")
    def identity(self, path):
        return live_identity(path)
    def bind(self, pin):
        require(live_identity(pin["path"]) == pin, "live file identity differs")
    def add(self, path, data):
        require(content(self.identity(path)) == portable.retention.fingerprint(io.BytesIO(data)), "source bytes differ")


def stages(protocol):
    result = []
    for block in range(4):
        for index, threads in enumerate(protocol["threads"]):
            order = ROLES if (block + index) % 2 == 0 else ROLES[::-1]
            for role in order:
                result.append({"label": f"river-b{block}-t{threads}-{role}", "case": "river", "arm": f"{role}-{threads}",
                               "role": role, "threads": threads, "block": block, "warmup": block == 0,
                               "iterations": protocol["iterations"]})
    return result


def sample_plan(plan):
    return {**plan, "inputs": {"river": plan["pins"]["input"]}, "protocol": {"arms": {
        f"{role}-{threads}": {"layout": "compact", "threads": threads}
        for role in ROLES for threads in plan["protocol"]["threads"]}},
        "pins": {**plan["pins"], "python": plan["pins"]["python"]}}


def command(plan, stage):
    return [plan["pins"][stage["role"] + "_binary"]["path"], "--config", plan["pins"]["input"]["path"],
            "--threads", str(stage["threads"]), "--iterations", str(stage["iterations"]), "--layout", "compact",
            "--out", join(plan["output"], "stages", stage["label"], "bench")]


def validate_containment(value, protocol):
    require(0 < value["effective_memory_max_bytes"] <= protocol["limits"]["outer_memory_max_bytes"], "outer memory differs")
    require(value["effective_cpu_quota"] is None or value["effective_cpu_quota"] >= 32, "CPU quota differs")
    require(any(row.get("memory.swap.max") == "0" for row in value["ancestors"]), "swap is not disabled")


def dependencies():
    return {"runner": HERE / "run.py", "protocol": HERE / "protocol.json", "range_runner": RANGE / "scaling-run.py",
            "range_portable": RANGE / "verify-retained.py", "retention": portable.LEGACY / "retain.py",
            "retention_store": portable.LEGACY / "verify_retained.py"}


def check_pins(store, plan):
    require(plan["schema"] == "r1.action-scaling-plan/v1", "wrong plan")
    protocol = plan["protocol"]
    require(protocol == helper.read(HERE / "protocol.json"), "prospective protocol differs")
    for name, path in dependencies().items():
        require(content(plan["pins"][name]) == content(helper.identity(path)), "trusted helper/runner version differs: " + name)
    for name, pin in plan["pins"].items():
        if name == "python":
            portable.tool_identity(pin)
        else:
            store.bind(pin)
    require(store.read(plan["pins"]["protocol"]["path"]) == protocol, "protocol payload differs")
    require(plan["host"]["logical_cpus"] == 32 and len(plan["host"]["affinity"]) == 32, "32 CPU host required")
    require(dt.datetime.fromisoformat(plan["deadline_utc"]) <= dt.datetime.fromisoformat(protocol["deadline_utc_latest"]), "deadline extended")
    validate_containment(plan["initial_containment"], protocol)
    for role in ROLES:
        arm = plan["sources"][role]
        result = portable.validation(store, plan["pins"][role + "_build_result"]["path"])
        state = store.read(result["path"])
        require(result["outcome"] == "completed" and state["source_root"] == arm["root"], "source build validation not complete")
        require(state["source_manifest"] == plan["pins"][role + "_manifest"]
                and state["source_archive"] == plan["pins"][role + "_archive"]
                and state["binary"] == plan["pins"][role + "_compiled_binary"], "source/build/binary binding differs")
        build = next(row for row in state["stages"] if row["label"] == "release-example")
        require(build["record"] == plan["pins"][role + "_build_record"], "wrong release build record")
        require(content(plan["pins"][role + "_binary"]) == content(state["binary"]), "copied binary differs")
        require(state["boot_id"] == plan["host"]["boot_id"] and len(state["cgroup"]["allowed_cpus"]) == 32, "build was not on this 32 CPU boot")
        require(store.identity(join(arm["root"], "tools/run_supervised.py")) == plan["pins"][role + "_supervisor"], "supervisor not from source")
        require(content(store.identity(join(arm["root"], protocol["config"]))) == content(plan["pins"]["input"]), "old/new input differs")
    require(content(plan["pins"]["old_supervisor"]) == content(plan["pins"]["new_supervisor"]), "supervisor changed")
    frozen = store.read(plan["pins"]["baseline_frozen"]["path"])
    require(frozen["plan"] == plan["pins"]["baseline_plan"] and frozen["iterations"]["river"] == 1000, "baseline fixed River count differs")
    baseline = store.read(frozen["plan"]["path"])
    require(baseline["schema"] == "r1.hu-range-scaling-plan/v1"
            and baseline["host"]["logical_cpus"] == 32 and len(baseline["host"]["affinity"]) == 32,
            "baseline was not on a 32 CPU host")
    require(baseline["pins"]["manifest"] == plan["pins"]["baseline_manifest"]
            and baseline["pins"]["binary"] == plan["pins"]["baseline_binary"]
            and baseline["inputs"]["river"] == plan["pins"]["baseline_input"], "baseline original identities differ")
    original_manifest = store.read(plan["pins"]["baseline_manifest"]["path"])
    require(plan["pins"]["baseline_archive"] == {
        "path": join(PurePosixPath(plan["pins"]["baseline_manifest"]["path"]).parent, "source-candidate.tar.gz"),
        "bytes": original_manifest["archive_bytes"], "sha256": original_manifest["archive_sha256"]},
        "baseline archive/manifest binding differs")
    require(content(plan["pins"]["baseline_manifest"]) == content(plan["pins"]["old_manifest"])
            and content(plan["pins"]["baseline_archive"]) == content(plan["pins"]["old_archive"])
            and content(plan["pins"]["baseline_input"]) == content(plan["pins"]["input"]),
            "baseline source/archive/input differs")


def summarize(protocol, entries):
    result, curves = {}, {}
    for threads in protocol["threads"]:
        groups = {role: [entry["sample"] for entry in entries if entry["stage"]["threads"] == threads
                        and entry["stage"]["role"] == role and not entry["stage"]["warmup"]] for role in ROLES}
        require(all(len(group) == 3 for group in groups.values()), "incomplete measured pairs")
        times = {role: [sample["timing"]["run_seconds"] for sample in group] for role, group in groups.items()}
        medians = {role: statistics.median(values) for role, values in times.items()}
        result[str(threads)] = {"run_seconds": times, "run_medians": medians,
            "new_over_old": medians["new"] / medians["old"],
            "new_strictly_faster_pairs": sum(new < old for new, old in zip(times["new"], times["old"])),
            "phase_medians": {role: {name: statistics.median(sample["timing"][name] for sample in group)
                                      for name in ("build_seconds", "solver_init_seconds", "run_seconds")} for role, group in groups.items()},
            "full_process_seconds": {role: [sample["full_process_seconds"] for sample in group] for role, group in groups.items()},
            "memory": {role: [{"full_process": sample["full_process_memory"], "phases": sample["phase_sampled_memory"]}
                              for sample in group] for role, group in groups.items()}}
    for role in ROLES:
        rows, previous = [], 1
        for threads in protocol["threads"]:
            seconds = result[str(threads)]["run_medians"][role]
            speedup = result["1"]["run_medians"][role] / seconds
            rows.append({"threads": threads, "speedup_over_one": speedup, "parallel_efficiency": speedup / threads,
                         "speedup_over_previous": result[str(previous)]["run_medians"][role] / seconds,
                         "slower_than_previous": seconds > result[str(previous)]["run_medians"][role]})
            previous = threads
        curves[role] = {"rows": rows, "fastest_observed_threads": min(protocol["threads"], key=lambda n: result[str(n)]["run_medians"][role]),
                        "first_adjacent_slowdown_threads": next((row["threads"] for row in rows if row["slower_than_previous"]), None)}
    guard = protocol["guard"]
    passed = result["32"]["new_over_old"] <= guard["new32_over_old32_max"] and result["1"]["new_over_old"] <= guard["new1_over_old1_max"]
    return {"threads": result, "scaling": curves, "descriptive_guard": "pass" if passed else "miss"}


def check(store, output, *, completing=False):
    plan, state = store.read(join(output, "plan.json")), store.read(join(output, "result.json"))
    check_pins(store, plan)
    require(plan["output"] == str(output) and state["schema"] == "r1.action-scaling-result/v1", "result path/schema differs")
    terminal_failure = not completing and state["status"] == "failed"
    require(terminal_failure or state["status"] == ("measuring" if completing else "completed"), "campaign incomplete")
    expected = stages(plan["protocol"])
    require([entry["stage"] for entry in state["stages"]] == expected[:len(state["stages"])], "48-stage schedule differs")
    if terminal_failure:
        require(state.get("error") and [row["stage"] for row in state["skipped"]] == expected[len(state["stages"]):]
                and all(row["reason"] for row in state["skipped"]), "failure/skipped schedule differs")
    else:
        require(len(state["stages"]) == 48 and not state["skipped"], "campaign incomplete")
    reference, previous_end = None, None
    report_plan = sample_plan(plan)
    for entry in state["stages"]:
        stage = entry["stage"]
        if entry["status"] != "passed":
            require(terminal_failure and entry is state["stages"][-1] and entry["status"] == "failed", "failed stage order differs")
            if "record" in entry:
                store.bind(entry["record"])
                record = store.read(entry["record"]["path"])
                require(record["argv"] == command(plan, stage), "failed sample command differs")
                portable.record_bytes(store, record, [*plan["pins"].values(), store.identity(join(output, "plan.json"))], {plan["pins"]["python"]["path"]})
            continue
        require(entry["status"] == "passed", "unpassed sample")
        store.bind(entry["record"])
        record = store.read(entry["record"]["path"])
        require(entry["record"]["path"] == join(output, "stages", stage["label"], "supervisor.json"), "record path differs")
        require(record["argv"] == record["resolved_argv"] == command(plan, stage) and record["cwd"] == str(output), "sample command differs")
        require(portable.successful(record) and entry["supervisor_exit"] == 0, "sample failed")
        portable.record_bytes(store, record, [*plan["pins"].values(), store.identity(join(output, "plan.json"))], {plan["pins"]["python"]["path"]})
        mapping = {"timeout_seconds": "sample_timeout_seconds", "memory_limit_bytes": "rss_bytes", "min_free_memory_bytes": "min_free_bytes",
                   "disk_reserve_bytes": "disk_reserve_bytes", "poll_seconds": "poll_seconds", "grace_seconds": "grace_seconds", "kill_wait_seconds": "kill_wait_seconds"}
        require(all(record["limits"][a] == plan["protocol"]["limits"][b] for a, b in mapping.items()), "sample limits differ")
        require(entry["host_before"] == entry["host_after"] == plan["host"] and record["runtime"]["logical_cpus"] == 32, "sample host differs")
        for name in ("containment_before", "containment_after"):
            validate_containment(entry[name], plan["protocol"])
        start, end = (dt.datetime.fromisoformat(record[name]) for name in ("started_at", "ended_at"))
        require(start <= end <= dt.datetime.fromisoformat(plan["deadline_utc"]) and (previous_end is None or previous_end <= start), "sample chronology/deadline differs")
        previous_end = end
        with portable.portable_runner(store, report_plan) as VPath:
            sample = helper.sample_report(report_plan, stage, VPath(entry["record"]["path"]).parent, record)
            require(sample == entry["sample"], "saved sample differs")
            if reference is not None:
                helper.same_solution(reference, sample)
        reference = sample
    if terminal_failure:
        require("summary" not in state, "failed campaign cannot claim a guard summary")
        return {"outcome": "failed", "passed_samples": sum(entry["status"] == "passed" for entry in state["stages"]),
                "skipped_samples": len(state["skipped"]), "descriptive_guard": "not_evaluated", "error": state["error"]}
    summary = summarize(plan["protocol"], state["stages"])
    if not completing:
        require(state["summary"] == summary, "saved summary differs")
    return summary


def live_pins(plan):
    for pin in plan["pins"].values():
        helper.verify(pin)
    for role in ROLES:
        manifest = helper.read(plan["pins"][role + "_manifest"]["path"])
        require(helper.inventory(Path(plan["sources"][role]["root"])) == {row["path"]: content(row) for row in manifest["files"]}, "source inventory changed")
    require(helper.host(plan["protocol"]) == plan["host"], "boot/CPU changed")
    helper.containment(plan["protocol"])


def prepare(args):
    output = args.out.resolve()
    require(not output.exists(), "new output required")
    protocol = helper.read(HERE / "protocol.json")
    deadline = dt.datetime.fromisoformat(args.deadline_utc)
    require(deadline.tzinfo and time.time() < deadline.timestamp() <= dt.datetime.fromisoformat(protocol["deadline_utc_latest"]).timestamp(), "invalid deadline")
    pins = {name: helper.identity(path) for name, path in dependencies().items()}
    pins["python"] = helper.identity(sys.executable)
    pins["baseline_frozen"] = helper.identity(args.baseline_frozen)
    pins["baseline_plan"] = helper.read(args.baseline_frozen)["plan"]
    sources = {}
    for role in ROLES:
        source = getattr(args, role + "_source").resolve(strict=True)
        require(not output.is_relative_to(source), "output must be outside source")
        manifest = getattr(args, role + "_manifest").resolve(strict=True)
        build = getattr(args, role + "_build_record").resolve(strict=True)
        sources[role] = {"root": str(source)}
        for name, path in {"manifest": manifest, "archive": manifest.parent / "source-candidate.tar.gz",
                           "compiled_binary": getattr(args, role + "_binary"), "build_record": build,
                           "build_result": build.parent.parent.parent / "result.json", "supervisor": source / "tools/run_supervised.py"}.items():
            pins[role + "_" + name] = helper.identity(path)
    baseline = helper.read(pins["baseline_plan"]["path"])
    pins["baseline_manifest"] = baseline["pins"]["manifest"]
    pins["baseline_binary"] = baseline["pins"]["binary"]
    pins["baseline_input"] = baseline["inputs"]["river"]
    pins["baseline_archive"] = helper.identity(Path(pins["baseline_manifest"]["path"]).parent / "source-candidate.tar.gz")
    pins["input"] = baseline["inputs"]["river"]
    output.mkdir(parents=True)
    (output / "stages").mkdir()
    for role in ROLES:
        shutil.copy2(pins[role + "_compiled_binary"]["path"], output / (role + "-hu_scaling_bench"))
        pins[role + "_binary"] = helper.identity(output / (role + "-hu_scaling_bench"))
    plan = {"schema": "r1.action-scaling-plan/v1", "output": str(output), "sources": sources, "pins": pins,
            "protocol": protocol, "host": helper.host(protocol), "initial_containment": helper.containment(protocol), "deadline_utc": deadline.isoformat()}
    check_pins(LiveStore(), plan)
    live_pins(plan)
    helper.save(output / "plan.json", plan, initial=True)
    helper.save(output / "result.json", {"schema": "r1.action-scaling-result/v1", "status": "ready", "stages": [], "skipped": []}, initial=True)


def execute(plan, state, stage):
    limits = plan["protocol"]["limits"]
    require(time.time() + limits["sample_timeout_seconds"] + 20 < dt.datetime.fromisoformat(plan["deadline_utc"]).timestamp(), "insufficient remaining deadline")
    live_pins(plan)
    directory = Path(plan["output"]) / "stages" / stage["label"]
    directory.mkdir()
    entry = {"stage": stage, "status": "running", "host_before": helper.host(plan["protocol"]), "containment_before": helper.containment(plan["protocol"])}
    state["stages"].append(entry)
    helper.save(Path(plan["output"]) / "result.json", state)
    spec = importlib.util.spec_from_file_location("action_supervisor", plan["pins"]["old_supervisor"]["path"])
    supervisor = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(supervisor)
    arguments = ["--record", str(directory / "supervisor.json"), "--cwd", plan["output"], "--disk-path", plan["output"]]
    for flag, key in (("timeout-seconds", "sample_timeout_seconds"), ("memory-limit-bytes", "rss_bytes"), ("min-free-memory-bytes", "min_free_bytes"),
                      ("disk-reserve-bytes", "disk_reserve_bytes"), ("poll-seconds", "poll_seconds"), ("grace-seconds", "grace_seconds"), ("kill-wait-seconds", "kill_wait_seconds")):
        arguments += ["--" + flag, str(limits[key])]
    for pin in [*plan["pins"].values(), helper.identity(Path(plan["output"]) / "plan.json")]:
        arguments += ["--identity-file", pin["path"]]
    os.environ["RAYON_NUM_THREADS"] = str(stage["threads"])
    os.environ.pop("R1_SOL_WRITE_PHASE_OUTPUT", None)
    code = supervisor.main(arguments + ["--", *command(plan, stage)])
    entry.update(status="checking" if code == 0 else "failed", supervisor_exit=code, record=helper.identity(directory / "supervisor.json"),
                 host_after=helper.host(plan["protocol"]), containment_after=helper.containment(plan["protocol"]))
    helper.save(Path(plan["output"]) / "result.json", state)
    require(code == 0, "supervised sample failed")
    record = helper.read(entry["record"]["path"])
    require(portable.successful(record), "supervisor did not complete cleanly")
    entry["sample"] = helper.sample_report(sample_plan(plan), stage, directory, record)
    if len(state["stages"]) > 1:
        helper.same_solution(state["stages"][0]["sample"], entry["sample"])
    live_pins(plan)
    entry["status"] = "passed"
    helper.save(Path(plan["output"]) / "result.json", state)
    print(json.dumps({"stage": stage["label"], "run_seconds": entry["sample"]["timing"]["run_seconds"]}), flush=True)


def measure(output):
    plan, state = helper.read(output / "plan.json"), helper.read(output / "result.json")
    require(state["status"] == "ready" and not state["stages"], "measurement cannot retry or resume")
    state["status"] = "measuring"
    schedule = stages(plan["protocol"])
    try:
        check_pins(LiveStore(), plan)
        for stage in schedule:
            execute(plan, state, stage)
        helper.save(output / "result.json", state)
        state["summary"] = check(LiveStore(), str(output), completing=True)
        state["status"] = "completed"
    except BaseException as error:
        state.update(status="failed", error={"type": type(error).__name__, "message": str(error)})
        state.pop("summary", None)
        if state["stages"] and state["stages"][-1]["status"] != "passed":
            state["stages"][-1]["status"] = "failed"
        state["skipped"] = [{"stage": stage, "reason": str(error)} for stage in schedule[len(state["stages"]):]]
        raise
    finally:
        helper.save(output / "result.json", state)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", choices=("prepare", "measure", "check"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    for role in ROLES:
        for field in ("source", "manifest", "binary", "build-record"):
            parser.add_argument("--" + role + "-" + field, type=Path)
    parser.add_argument("--baseline-frozen", type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args()
    if args.phase == "prepare":
        require(args.baseline_frozen and args.deadline_utc and all(getattr(args, role + "_" + field) for role in ROLES
                for field in ("source", "manifest", "binary", "build_record")), "prepare arguments missing")
        prepare(args)
    elif args.phase == "measure":
        measure(args.out.resolve(strict=True))
    else:
        print(json.dumps(check(LiveStore(), str(args.out.resolve(strict=True))), indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
