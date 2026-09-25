#!/usr/bin/env python3
"""Independently check a relocated, complete 126-sample writer campaign.

Only local reviewed Python is imported (after checking its frozen identity).
No retained executable runs. Collector archives and their two sidecars are
required; pass each archive's SHA256 explicitly. Large canonical files are
stream-compared from an anonymous spool, never extracted to original paths.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import math
from pathlib import Path, PurePosixPath
import re
import statistics

HERE = Path(__file__).resolve().parent
IO_SHA256 = "e57a7c974af5c380d9252a3719a9093a4c5d42fe5d697c9e9e73c519f1531c6f"
CASES = ("river", "turn", "flop")
ARMS = ("baseline-plain", "baseline-off", "baseline-on", "candidate-plain", "candidate-off", "candidate-on")
ORDERS = ((0, 3, 1, 4, 2, 5), (4, 1, 5, 2, 3, 0), (2, 5, 0, 3, 1, 4),
          (3, 0, 4, 1, 5, 2), (1, 4, 2, 5, 0, 3), (5, 2, 3, 0, 4, 1))
ASSURANCE = "sampled process inventory plus operator dedicated-window declaration"
TIMINGS = {"preparation_load_seconds", "open_seconds", "operation_seconds",
           "operation_seconds_per_iteration", "validation_output_seconds"}


def require(ok, why):
    if not ok:
        raise ValueError(why)


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def io_module():
    path = HERE.parent / "verify-retained.py"
    require(hashlib.sha256(path.read_bytes()).hexdigest() == IO_SHA256, "archive verifier changed")
    return load(path, "writer_retained_io")


def instant(value):
    require(isinstance(value, str), "missing timestamp")
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "timestamp needs timezone")
    return result


def number(value, *, positive=False):
    require(type(value) in (int, float) and math.isfinite(value)
            and (value > 0 if positive else value >= 0), "invalid finite measurement")
    return value


def integer(value, *, positive=False):
    require(type(value) is int and (value > 0 if positive else value >= 0), "invalid integer measurement")
    return value


def path_at(ref, path, evidence):
    require(ref["path"] == str(path), "unexpected evidence path: " + str(path))
    require(evidence.identity(ref) == ref, "missing/changed retained file")


def schedule(protocol):
    require(protocol["schema"] == "r1.sol-write-phase-protocol/v1"
            and protocol["cases_in_order"] == list(CASES) and protocol["arms"] == list(ARMS)
            and protocol["excluded_warmup_order"] == list(ARMS)
            and protocol["measured_block_arm_indices"] == [list(x) for x in ORDERS]
            and protocol["samples"] == {"warmup": 18, "measured": 108, "total": 126}
            and protocol["operation"] == "stream-write" and type(protocol["iterations_per_process"]) is int
            and protocol["iterations_per_process"] == 1
            and protocol["calibration"]["ratio_bounds"] == [0.95, 1.05], "prospective protocol differs")
    result = []
    for case in CASES:
        for block, order in enumerate((tuple(range(6)), *ORDERS)):
            for index in order:
                result.append({"index": len(result), "case": case, "arm": ARMS[index],
                               "block": block, "excluded": block == 0})
    return result


def seconds(text):
    require(isinstance(text, str), "invalid systemd duration")
    text = text.replace(" ", "")
    parts = re.findall(r"([0-9]+(?:\.[0-9]+)?)(us|ms|min|s|h|d)", text)
    require(parts and "".join(a + b for a, b in parts) == text, "unbounded systemd duration")
    units = {"us": 1e-6, "ms": 1e-3, "s": 1, "min": 60, "h": 3600, "d": 86400}
    return number(sum(float(a) * units[b] for a, b in parts), positive=True)


def containment(plan):
    environment = plan["environment"]
    limits, unit = environment["limits"], environment["systemd"]
    require(isinstance(limits["memory.max"], str) and limits["memory.max"].isdigit()
            and 0 < int(limits["memory.max"]) <= 4 * 1024**3
            and limits["memory.swap.max"] == "0" and limits["pids.max"].isdigit()
            and 0 < int(limits["pids.max"]) <= 512, "unbounded cgroup memory/swap/pids")
    cpu = limits["cpu.max"].split()
    require(len(cpu) == 2 and all(x.isdigit() and int(x) > 0 for x in cpu)
            and int(cpu[0]) <= int(cpu[1]) * integer(plan["host"]["logical_cpus"], positive=True)
            and bool(limits["cpuset.cpus.effective"]), "unbounded cgroup CPU")
    require(environment["cgroup_path"] == "/sys/fs/cgroup" + unit["ControlGroup"]
            and unit["ControlGroup"].startswith("/") and unit["ActiveState"] == "active"
            and unit["KillMode"] == "control-group" and unit["SendSIGKILL"] == "yes"
            and seconds(unit["RuntimeMaxUSec"]) <= 1500 and seconds(unit["TimeoutStopUSec"]) <= 15,
            "actual active systemd containment missing")
    require(environment["mount"] and type(environment["output_device"]) is int, "output filesystem missing")
    for key in ("cpu", "flags", "boot_id", "kernel", "machine", "system"):
        require(isinstance(plan["host"][key], str) and bool(plan["host"][key]), "host identity missing")


def snapshot(ref, path, evidence, plan, frozen, phase_env):
    path_at(ref, path, evidence)
    value = evidence.read(ref)
    require(value["schema"] == "r1.write-phase-stage-snapshot/v1" and value["status"] == "clear"
            and value["errors"] == [] and value["identities"] == frozen
            and value["host"] == plan["host"] and value["environment"] == plan["environment"]
            and value["cgroup_events"] == plan["cgroup_events"]
            and value["phase_env"] == phase_env, "snapshot input/CPU/boot/cgroup/environment differs")
    scan = value["process_scan"]
    require(scan["status"] == "clear" and scan["foreign_same"] is True
            and scan["no_unrelated_cgroup_members"] is True and scan["assurance"] == ASSURANCE
            and scan["baseline_foreign"] == plan["process_baseline"], "process window check failed")
    return instant(value["observed_at"])


def sol_directory(evidence, ref):
    # Only the small selected SOL is read into memory, never the canonical stream.
    raw = evidence.raw(ref, limit=32 * 1024**2)
    require(len(raw) >= 106 and raw[:10] == b"SLVRSOLV\x03\x00", "wrong SOL v3 header")
    groups = int.from_bytes(raw[98:106], "little")
    directory = 106 + int.from_bytes(raw[50:58], "little")
    require(0 < groups <= 1000000 and directory + groups * 64 <= len(raw), "invalid SOL directory")
    compressed = sum(int.from_bytes(raw[start + 24:start + 28], "little")
                     for start in range(directory, directory + groups * 64, 64))
    return groups, len(raw), compressed


def resource_samples(evidence, ref, record, limits):
    raw = evidence.raw(ref)
    require(raw.endswith(b"\n"), "partial resource sample JSONL")
    rows = [strict_json(line) for line in raw.splitlines()]
    measure = record["measurement"]
    require(len(rows) == integer(measure["sample_count"], positive=True), "resource sample count differs")
    elapsed = [number(x["elapsed_seconds"]) for x in rows]
    require(elapsed == sorted(elapsed), "resource sample order differs")
    require(record["last_sample"] == rows[-1], "last resource sample differs")
    for row in rows:
        require(integer(row["tree_resident_bytes"]) <= limits["memory_bytes"]
                and integer(row["host_available_memory_bytes"]) >= limits["min_free_memory_bytes"]
                and integer(row["disk_free_bytes"]) >= limits["disk_reserve_bytes"], "resource sample exceeded limit")
    require(measure["sampled_peak_tree_resident_bytes"] == max(x["tree_resident_bytes"] for x in rows)
            and measure["max_observed_processes"] == max(len(x["pids"]) for x in rows), "resource peaks differ")
    gap = max((b - a for a, b in zip(elapsed, elapsed[1:])), default=0)
    require(math.isclose(number(measure["max_sample_gap_seconds"]), gap, abs_tol=1e-8, rel_tol=1e-8),
            "resource maximum sampling gap differs")
    number(record["elapsed_seconds"], positive=True)
    integer(measure["root_os_peak_resident_bytes"], positive=True)
    require(measure["root_os_peak_source"] == "wait4.ru_maxrss_linux_kib", "unknown invocation RSS semantics")
    require(rows[-1]["pids"] == [] and rows[-1]["tree_resident_bytes"] == 0, "final process containment not empty")
    return measure


def strict_json(raw):
    def pairs(items):
        out = {}
        for key, value in items:
            require(key not in out, "duplicate JSON key")
            out[key] = value
        return out
    def invalid(_):
        raise ValueError("nonfinite JSON")
    value = json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid)
    json.dumps(value, allow_nan=False)
    return value


def inspect_sample(row, expected, evidence, plan, frozen, validator):
    require({k: row[k] for k in expected} == expected
            and type(row["index"]) is int and type(row["block"]) is int
            and type(row["excluded"]) is bool, "missing/duplicate/reordered campaign sample")
    directory = PurePosixPath(plan["run_root"]) / f'{row["index"]:03d}-{row["case"]}-{row["block"]}-{row["arm"]}'
    files = {"record": "supervisor.json", "report": "output/result.json", "stdout": "stdout.log",
             "stderr": "stderr.log", "samples": "supervisor.samples.jsonl", "guard": "stage-guard.json",
             "canonical": "output/canonical.bin", "root_canonical": "output/root-canonical.bin",
             "rewritten": "output/rewritten.sol"}
    for key, name in files.items():
        path_at(row[key], directory / name, evidence)
    on = row["arm"].endswith("-on")
    phase_env = str(directory / "phase.json") if on else None
    before = snapshot(row["before"], directory / "before.json", evidence, plan, frozen, phase_env)
    after = snapshot(row["after"], directory / "after.json", evidence, plan, frozen, phase_env)
    guard = evidence.read(row["guard"])
    require(guard["schema"] == "r1.write-phase-stage-guard/v1" and guard["status"] == "clear"
            and guard["first_failure"] is None and guard["assurance"] == ASSURANCE
            and guard["poll_seconds"] == 0.05, "stage process/environment monitor failed")
    integer(guard["checks"], positive=True)
    require(number(guard["max_gap_seconds"]) <= plan["protocol_body"]["runner_monitor"]["max_gap_seconds"],
            "dedicated-window monitoring gap exceeded")
    record, report = evidence.read(row["record"]), evidence.read(row["report"])
    require(strict_json(evidence.raw(row["stdout"])) == report, "stdout and saved sample report differ")
    require(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
            and record["stop_reason"] == "completed" and type(record["child_exit_code"]) is int
            and record["child_exit_code"] == 0 and type(record["supervisor_exit_code"]) is int
            and record["supervisor_exit_code"] == 0 and record["cleanup_complete"] is True
            and record["identity_unchanged"] is True and record["errors"] == []
            and record["shell"] is False and record["forced"] is False
            and record["stop_requested_at"] is None, "failed/partial/forced supervisor record")
    role, mode = row["arm"].split("-", 1)
    copy = plan["copies"][role + ("-plain" if mode == "plain" else "-instrumented")]
    original = plan["inputs"][row["case"]]
    argv = [copy["binary"]["path"], original["path"], "stream-write", "1", str(directory / "output")]
    require(record["argv"] == argv == record["resolved_argv"], "wrong sample role/operation/input")
    expected_ids = []
    for ref in (copy["binary"], plan["files"]["python"], plan["files"]["supervisor"], original,
                plan["files"]["runner"], plan["protocol"], plan["files"]["validator"]):
        if ref not in expected_ids:
            expected_ids.append(ref)
    require(record["identity_before"] == expected_ids == record["identity_after"], "supervisor frozen identities differ")
    manifest = evidence.read(copy["source_manifest"])
    require(record["cwd"] == manifest["output_path"], "supervisor source/cwd differs")
    require(record["outputs"] == {k: row[k] for k in ("stdout", "stderr", "samples")}, "supervisor outputs differ")
    limits = plan["limits"]
    required_limits = {"timeout_seconds": limits["per_process_seconds"], "memory_limit_bytes": limits["memory_bytes"],
                       "min_free_memory_bytes": limits["min_free_memory_bytes"], "disk_reserve_bytes": limits["disk_reserve_bytes"]}
    for key, value in required_limits.items():
        require(record["limits"][key] == value, "supervisor limit differs")
    for key in ("grace_seconds", "kill_wait_seconds"):
        require(number(record["limits"][key]) == 5, "supervisor cleanup grace differs")
    require(record["limits"]["poll_seconds"] == 0.05, "resource polling differs")
    started, ended = instant(record["created_at"]), instant(record["ended_at"])
    require(before <= started <= instant(record["started_at"]) <= ended <= after, "stage chronology differs")
    measure = resource_samples(evidence, row["samples"], record, limits)
    require(guard["checks"] == measure["sample_count"], "monitor checks do not cover successful resource samples")
    require(measure == row["measurement"] and record["elapsed_seconds"] <= limits["per_process_seconds"],
            "sample measurement/timeout differs")
    require(report["schema"] == "r1.sol-codec-sample/v1" and report["status"] == "completed"
            and type(report["format_version"]) is int and report["format_version"] == 3
            and report["operation"] == "stream-write" and type(report["iterations"]) is int
            and report["iterations"] == 1 and report["metadata"]["mode"] == "Full"
            and report["metadata"] == row["metadata"] and report["timing"] == row["timing"]
            and report["input"] == row["input"], "sample report differs")
    timing = report["timing"]
    require(set(timing) == TIMINGS, "missing sample timing")
    for value in timing.values():
        number(value)
    require(number(timing["operation_seconds"], positive=True) == timing["operation_seconds_per_iteration"],
            "iteration timer differs")
    count = integer(report["metadata"]["stored_nodes"], positive=True)
    require(report["decoded_strategy_blocks"] == report["decoded_value_blocks"] == count
            and len(report["selected_srefs"]) == count
            and all(type(x) is int and x >= 0 for x in report["selected_srefs"])
            and report["selected_srefs"] == sorted(set(report["selected_srefs"]))
            and report["selected_srefs"][0] == 0
            and report["solve_iterations"] == report["metadata"]["meta"]["iterations"], "full decoded profile missing")
    require(report["input"]["bytes"] == original["bytes"]
            and re.fullmatch("[0-9a-f]{64}", report["input"]["blake3"]), "reported input identity differs")
    require(report["rewritten"] == {"file": "rewritten.sol", **report["input"]}, "rewrite report differs")
    for field in ("raw_strategy_bytes", "raw_value_bytes"):
        integer(report[field], positive=True)
    evidence.same_bytes(original, row["rewritten"])
    for key, name in (("canonical", "canonical.bin"), ("root_canonical", "root-canonical.bin")):
        require(report[key]["file"] == name and report[key]["bytes"] == row[key]["bytes"] > 0
                and re.fullmatch("[0-9a-f]{64}", report[key]["blake3"]), "canonical report differs")
    phase = None
    if on:
        path_at(row["phase"], directory / "phase.json", evidence)
        path_at(row["phase_validation"], directory / "phase-validation.json", evidence)
        phase = evidence.read(row["phase"])
        groups, file_bytes, compressed = sol_directory(evidence, row["rewritten"])
        validation = validator.validate_phase(phase, expected_groups=groups, expected_file_bytes=file_bytes,
                                             expected_compressed_bytes=compressed,
                                             operation_seconds=timing["operation_seconds"])
        require(validation["status"] == "phase_record_valid_not_campaign_acceptance"
                and validation == evidence.read(row["phase_validation"]), "missing/partial/changed phase validation")
    else:
        require(row["phase"] is None and row["phase_validation"] is None, "non-ON phase must be absent, never zero")
        require(not evidence.paths.get(str(directory / "phase.json")), "unexpected retained non-ON phase")
    return {"row": row, "report": report, "phase": phase, "before": before, "after": after}


def spread(values):
    return {"raw": values, "median": statistics.median(values), "min": min(values), "max": max(values)}


def comparison(samples):
    primary, calibration, phases = [], [], []
    measured = [x for x in samples if not x["row"]["excluded"]]
    for case in CASES:
        chosen = {arm: [x for x in measured if x["row"]["case"] == case and x["row"]["arm"] == arm] for arm in ARMS}
        times = {arm: [x["row"]["timing"]["operation_seconds"] for x in rows] for arm, rows in chosen.items()}
        a, b = times["baseline-plain"], times["candidate-plain"]
        primary.append({"case": case, "scope": "plain writer operation only", "baseline_seconds": spread(a),
                        "candidate_seconds": spread(b), "candidate_over_baseline_medians": statistics.median(b) / statistics.median(a),
                        "paired_blocks": [{"block": i + 1, "baseline_seconds": aa, "candidate_seconds": bb,
                                           "candidate_over_baseline": bb / aa} for i, (aa, bb) in enumerate(zip(a, b, strict=True))]})
        for role in ("baseline", "candidate"):
            med = {mode: statistics.median(times[role + "-" + mode]) for mode in ("plain", "off", "on")}
            off_plain, on_off = med["off"] / med["plain"], med["on"] / med["off"]
            eligible = all(0.95 <= x <= 1.05 for x in (off_plain, on_off))
            calibration.append({"case": case, "role": role, "seconds": {mode: spread(times[role + "-" + mode]) for mode in med},
                                "off_over_plain_medians": off_plain, "on_over_off_medians": on_off,
                                "bounds_inclusive": [0.95, 1.05], "phase_attribution": "eligible" if eligible else "not_evaluated"})
            records = [x["phase"] for x in chosen[role + "-on"]]
            names = sorted(records[0]["phase"]["leaves"])
            phases.append({"case": case, "role": role, "phase_attribution": "eligible" if eligible else "not_evaluated",
                           "parent_total_ns": spread([x["parent_total_ns"] for x in records]),
                           "inner_total_ns": spread([x["phase"]["inner_total_ns"] for x in records]),
                           "parent_remainder_ns": spread([x["parent_total_ns"] - x["phase"]["inner_total_ns"] for x in records]),
                           "unclassified_ns": spread([x["phase"]["unclassified_ns"] for x in records]),
                           "leaves_ns": {n: spread([x["phase"]["leaves"][n]["ns"] for x in records]) for n in names},
                           "compression_envelope_inclusive_ns": spread([x["phase"]["compression_envelope_ns"] for x in records]),
                           "compression_nested_file_ns": spread([x["phase"]["compression_nested_file_ns"] for x in records])})
    return {"plain_performance": primary, "calibration": calibration, "on_phase_durations": phases}


def analyze(evidence, state, plan, frozen, validator):
    """Core used after freeze/build provenance; deliberately no standalone CLI bypass."""
    require(state["schema"] == "r1.write-phase-campaign/v1" and state["status"] == "completed"
            and state["first_failure"] is None and state["plan_sha256"] == plan["plan_sha256"]
            and state["run_root"] == plan["run_root"], "incomplete/failed campaign cannot publish comparison")
    expected = schedule(plan["protocol_body"])
    require(len(state["samples"]) == len(expected), "126 samples required including 18 warmups")
    require(plan["limits"] == {"per_process_seconds": 60, "campaign_seconds": 1200, "memory_bytes": 4 * 1024**3,
                               "min_free_memory_bytes": 2 * 1024**3, "disk_reserve_bytes": 4 * 1024**3}, "finite protocol limits differ")
    containment(plan)
    for ref in frozen:
        require(evidence.identity(ref) == ref, "frozen input bytes missing")
    root = PurePosixPath(plan["run_root"])
    first = snapshot(state["initial_snapshot"], root / "initial-snapshot.json", evidence, plan, frozen, None)
    last = snapshot(state["final_snapshot"], root / "final-snapshot.json", evidence, plan, frozen, None)
    start, end = instant(state["started_at"]), instant(state["ended_at"])
    require(instant(plan["issued_at"]) <= start <= first <= last <= end <= instant(plan["window_body"]["deadline_utc"])
            and (end - start).total_seconds() <= plan["limits"]["campaign_seconds"], "campaign chronology/deadline differs")
    checked, previous, reference = [], first, {}
    for row, want in zip(state["samples"], expected, strict=True):
        sample = inspect_sample(row, want, evidence, plan, frozen, validator)
        require(previous <= sample["before"] <= sample["after"] <= last, "samples overlap or lie outside campaign")
        previous = sample["after"]
        case = row["case"]
        if case in reference:
            prior = reference[case]
            for field in ("input", "metadata", "selected_srefs", "raw_strategy_bytes", "raw_value_bytes"):
                require(prior["report"][field] == sample["report"][field], "cross-arm semantic metadata differs")
            for field in ("canonical", "root_canonical"):
                evidence.same_bytes(prior["row"][field], row[field])
        else:
            reference[case] = sample
        checked.append(sample)
    return {"schema": "r1.write-phase-retained-verification/v1", "status": "verified",
            "plan_sha256": plan["plan_sha256"], "samples": 126, "excluded_warmups": 18, "measured_samples": 108,
            "byte_comparison": "streamed original/rewrite and canonical/root-canonical across all six arms and seven blocks",
            **comparison(checked), "r1_acceptance": None,
            "scope": ["Plain candidate/baseline writer timing only; ON/OFF is separate instrumentation calibration.",
                      "Per-record exclusive leaves close exactly; marginal phase medians need not sum to the median total.",
                      "Compression envelope includes nested file writing and is non-additive with the exclusive leaves.",
                      "File writing includes compression-callback file calls, sync is separate; no physical disk or pure CPU claim.",
                      "Whole invocation RSS is not attributed to individual phases.",
                      "Sampled process inventory plus declaration cannot exclude sub-poll concurrent processes.",
                      "Retained compiler/source/binary identities and build records do not independently prove compiler behavior.",
                      "No external-reference quality, solver speed/exploitability, historical-cause or overall R1 acceptance claim."]}


def reviewed_modules(evidence, plan, io):
    paths = {"runner": HERE / "run.py", "validator": HERE / "validate.py", "applier": HERE / "apply.py",
             "runtime": HERE / "runtime.rs.inc", "source_pins": HERE / "source-pins.json",
             "input_selection": HERE.parent / "inputs.json", "supervisor": HERE.parents[3] / "tools/run_supervised.py"}
    for key, local in {**paths, "protocol": HERE / "protocol.json"}.items():
        actual = io.local_identity(local)
        pin = plan["protocol"] if key == "protocol" else plan["files"][key]
        require(all(actual[k] == pin[k] for k in ("bytes", "sha256")), "local reviewed " + key + " differs from freeze")
        evidence.identity(pin)
    return {key: load(paths[key], "retained_writer_" + key) for key in ("runner", "validator")}


def verify_campaign(evidence, run_root, io):
    io.posix(run_root)
    state_ref = evidence.identity(run_root + "/result.json")
    state = evidence.read(state_ref)
    require(state["run_root"] == run_root, "requested campaign root differs")
    require(evidence.identity(state["plan"]) == state["plan"], "retained plan missing")
    plan = evidence.read(state["plan"])
    modules = reviewed_modules(evidence, plan, io)
    runner = modules["runner"]
    runner.Path, runner.read, runner.identity, runner.same_bytes = io.VMPath, evidence.read, evidence.identity, evidence.same_bytes
    runner.verify_plan(plan, live=False)
    frozen = runner.all_frozen_refs(plan)
    report = analyze(evidence, state, plan, frozen, modules["validator"])
    # Stored live analysis is corroboration, not the source of our independent statistics.
    require(evidence.read(run_root + "/comparison.json") == runner.analyze(state, plan), "stored runner comparison changed")
    report.update(campaign=state_ref, plan=state["plan"], bundles=evidence.bundles,
                  verifier=io.local_identity(__file__), archive_verifier=io.local_identity(HERE.parent / "verify-retained.py"),
                  required_retained_files=[{"path": path, "bytes": size, "sha256": sha,
                                            "locations": evidence.entries[(path, size, sha)]["locations"]}
                                           for path, size, sha in sorted(evidence.links)])
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", action="append", nargs=3, metavar=("LABEL", "ARCHIVE", "SHA256"), required=True)
    parser.add_argument("--run-root", required=True, help="original absolute Linux campaign directory")
    parser.add_argument("--out", type=Path, required=True, help="new local report; refuses overwriting")
    args = parser.parse_args()
    io = io_module()
    evidence = io.Evidence()
    try:
        for label, archive, digest in args.bundle:
            evidence.add_bundle(label, Path(archive), digest)
        report = verify_campaign(evidence, args.run_root, io)
        with args.out.open("x", encoding="utf-8", newline="\n") as output:
            json.dump(report, output, indent=2, sort_keys=True, allow_nan=False)
            output.write("\n")
        print(json.dumps({"status": report["status"], "samples": report["samples"], "r1_acceptance": None}))
    finally:
        evidence.close()


if __name__ == "__main__":
    main()
