"""Equal-NashConv bounded campaign; portable checks never execute evidence code."""
from __future__ import annotations
import argparse
import datetime as dt
import decimal
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import statistics
import struct
import sys
import time
sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
SHARED = HERE.parent / "showdown-kernel" / "run.py"
spec = importlib.util.spec_from_file_location("trusted_showdown_evidence_helpers", SHARED)
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)
require, decode, read, digest, content, identity, save, join, timestamp = (getattr(shared, name) for name in ("require", "decode", "read", "digest", "content", "identity", "save", "join", "timestamp"))
Store, source_files, live_source, host, host_record, record_bytes, retain_record = (getattr(shared, name) for name in ("Store", "source_files", "live_source", "host", "host_record", "record_bytes", "retain_record"))
FULL_STAGES, BUILD_STAGES, LIMIT_KEYS = shared.FULL_STAGES, shared.BUILD_STAGES, shared.LIMIT_KEYS

def control_paths(protocol):
    paths = {name: HERE / name for name in ("run.py", "protocol.json", "verify.py")}
    paths["shared/showdown-run.py"] = SHARED
    for row in [protocol["target_derivation"]["source_manifest"], *[case["reference"] for case in protocol["cases"].values()]]:
        paths[row["file"]] = HERE / row["file"]
    return paths


def target_ceiling(value):
    require(math.isfinite(value) and value >= 0, "invalid historical NashConv")
    number = decimal.Decimal(str(value))
    return float(number.quantize(decimal.Decimal(1).scaleb(number.adjusted() - 2),
                                rounding=decimal.ROUND_CEILING)) if number else 0.0


def references(store, plan):
    protocol = plan["protocol"]
    row = protocol["target_derivation"]["source_manifest"]
    pin = plan["controls"][row["file"]]
    require(content(pin) == content(row), "historical source pin differs")
    historical = store.json(pin["path"])
    historical_pins = {entry["path"]: content(entry) for entry in historical["files"]}
    expected = {x["path"]: content(x) for x in historical["files"] if x["path"].startswith("crates/") and x["path"] != "crates/cli/examples/hu_scaling_bench.rs"}
    baseline = store.json(plan["arms"]["old"]["manifest"]["path"])
    actual = {x["path"]: content(x) for x in baseline["files"] if x["path"].startswith("crates/") and x["path"] != "crates/cli/examples/hu_scaling_bench.rs"}
    require(actual == expected, "baseline crates differ from historical numeric implementation")
    for case, definition in protocol["cases"].items():
        fixture = protocol["config_directory"] + "/" + definition["file"]
        require(content(plan["inputs"][case]) == historical_pins[fixture], "fixture differs from historical target game")
        reference = definition["reference"]
        pin = plan["controls"][reference["file"]]
        require(content(pin) == content(reference), "historical report pin differs")
        report = store.json(pin["path"])
        nc = report["quality"]["nash_conv"]
        require(nc == reference["achieved_nash_conv"] and target_ceiling(nc) == definition["target_nash_conv"]
                and definition["target_nash_conv"] >= nc, "target differs from prespecified historical ceiling")
        require(report["iterations"] == definition["iterations"] and report["threads"] == 1
                and report["layout"] == "compact" and report["storage"] == "f32", "historical run scope differs")

def schedule(protocol):
    result = []
    for case_index, (case, definition) in enumerate(protocol["cases"].items()):
        for block in range(4):
            order = ("old", "new") if (case_index + block) % 2 == 0 else ("new", "old")
            for arm in order:
                result.append({"case": case, "block": block, "warmup": block == 0, "arm": arm,
                               "iterations": definition["iterations"], "label": f"{case}-b{block}-{arm}"})
    return result

def command(plan, stage):
    case = plan["protocol"]["cases"][stage["case"]]
    return [plan["arms"][stage["arm"]]["binary"]["path"], "--config", plan["inputs"][stage["case"]]["path"],
            "--threads", "1", "--iterations", str(stage["iterations"]), "--layout", "compact",
            "--target-nash-conv", str(case["target_nash_conv"]), "--check-every", str(case["check_every"]),
            "--out", join(plan["output"], "stages", stage["label"], "bench")]

def validate_build(store, arm, role, boot):
    manifest = source_files(store, arm)
    if role == "old":
        require(manifest["base_commit"] == "db9b8742290ad06d472bf2836e017a11434341c6", "wrong old source revision")
        require(all(path == "crates/cli/examples/hu_scaling_bench.rs" or path.startswith("experiments/") for path in manifest["changed_paths_against_base"]), "old source changes exceed benchmark/control overlay")
    store.verify(arm["validation"])
    state = store.json(arm["validation"]["path"])
    expected_mode = "release-build-only" if role == "old" else "full-validation"
    require(state["schema"] == "r1.range-scaling-validation/v1" and state["status"] == "completed"
            and state["mode"] == expected_mode and state["boot_id"] == boot, "build/validation mode, status or boot differs")
    require(state["source_root"] == arm["source"] and state["source_manifest"] == arm["manifest"]
            and state["source_archive"] == arm["archive"] and state["binary"] == arm["binary"], "build source/binary binding")
    require([row["label"] for row in state["stages"]] == list(BUILD_STAGES if role == "old" else FULL_STAGES), "validation stage set")
    for key, expected in {"RUSTUP_TOOLCHAIN": "1.97.0", "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "2",
                          "CARGO_INCREMENTAL": "0", "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0"}.items():
        require(state["environment"][key] == expected, "validation environment differs: " + key)
    require(state["environment"]["CARGO_BUILD_JOBS"] in ("1", "2"), "unbounded build jobs")
    store.verify(state["runner"])
    require(content(state["runner"]) == content(store.pin(join(arm["source"], "experiments/hu-postflop-r1/range-scaling/validate.py"))), "validation runner/source differs")
    tools = list(state["tools"].values()) + [arm["python"]]
    test_counts = {}
    for stage in state["stages"]:
        require(stage["status"] == "passed" and stage["supervisor_exit"] == 0, "validation failed stage")
        record = record_bytes(store, stage["record"], tools)
        executable = arm["python"] if stage["label"] == "docs" else state["tools"]["rustc" if stage["label"] == "toolchain" else "cargo"]
        require(record["argv"] == stage["argv"] and record["resolved_argv"] == [executable["path"], *stage["argv"][1:]]
                and record["cwd"] == arm["source"], "validation invocation/executable differs")
        for pin in (arm["manifest"], arm["archive"], state["runner"], state["tools"]["cargo"], state["tools"]["rustc"],
                    store.pin(join(arm["source"], "tools/run_supervised.py")), arm["python"]):
            require(pin in record["identity_before"], "build record lacks source/tool pin")
        argv = stage["argv"]
        label = stage["label"]
        tails = {"toolchain": ["-Vv"], "fmt": ["fmt", "--all", "--check"], "docs": ["-B", "tools/check_docs.py"],
                 "clippy": ["clippy", "--workspace", "--all-targets", "--target-dir", state["target"], "--", "-D", "warnings"],
                 "workspace-tests": ["test", "--workspace", "--no-fail-fast", "--target-dir", state["target"]],
                 "release-example": ["build", "--release", "-p", "cli", "--example", "hu_scaling_bench", "--target-dir", state["target"]],
                 "release-oracle": ["test", "--release", "-p", "holdem", "--test", "oracle_diff", "--target-dir", state["target"], "--", "--include-ignored"],
                 "release-river-resolve": ["test", "--release", "-p", "cli", "--lib", "--target-dir", state["target"], "sol::tests::river_resolve_accuracy", "--", "--exact", "--ignored"]}
        require(argv[1:] == tails[label], "validation command scope differs")
        require(record["limits"]["timeout_seconds"] == stage["timeout_seconds"], "validation timeout differs")
        text = store.data(record["outputs"]["stdout"]["path"]).decode("utf-8")
        if label == "toolchain":
            require("release: 1.97.0" in text and "host: x86_64-unknown-linux-gnu" in text, "toolchain output differs")
        if label in ("workspace-tests", "release-oracle", "release-river-resolve"):
            rows = [tuple(map(int, row)) for row in re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", text)]
            require(rows and sum(row[0] for row in rows) > 0 and not any(row[1] for row in rows), "test summary missing or failed")
            test_counts[label] = {"summary_count": len(rows), "passed": sum(row[0] for row in rows), "ignored": sum(row[2] for row in rows)}
            if label == "release-river-resolve":
                require(sum(row[0] for row in rows) == 1 and not any(row[2] for row in rows)
                        and "test sol::tests::river_resolve_accuracy ... ok" in text, "ignored river accuracy did not run")
        if label == "docs":
            require(re.search(r"Documentation check passed: \d+ Markdown files\.", text), "documentation check output absent")
    store.verify(arm["binary"])
    require(arm["binary"]["path"] == join(state["target"], "release/examples/hu_scaling_bench"), "build output path differs")
    return {"mode": expected_mode, "stages": len(state["stages"]), "tests": test_counts, "full_workspace_validated": role == "new"}

def quality_values(quality):
    ev, br = quality["solver_ev"], quality["solver_br"]
    require(len(ev) == len(br) == 2 and all(math.isfinite(v) for v in ev + br), "invalid quality values")
    gains = [br[0] - ev[0], br[1] - ev[1]]
    nc = gains[0] + gains[1]
    require(math.isfinite(nc) and quality["nash_conv"] == nc, "NashConv does not equal unclamped deviation sum")
    return gains, nc


def stopping_check(report, definition):
    stop = report["stopping"]
    require(stop["criterion"] == "nash-conv-sum-of-unclamped-deviation-gains"
            and stop["target_nash_conv"] == definition["target_nash_conv"]
            and stop["check_every"] == definition["check_every"]
            and stop["max_iterations"] == definition["iterations"], "stopping controls differ")
    checks = stop["checks"]
    require(checks, "missing stopping checks")
    previous = 0
    for index, row in enumerate(checks):
        expected = min(previous + definition["check_every"], definition["iterations"])
        require(previous < expected and row["iterations"] == expected, "check cadence/cap differs")
        _, nc = quality_values(row)
        require(all(math.isfinite(row[k]) and row[k] >= 0 for k in ("solve_seconds", "quality_seconds")), "invalid check timing")
        if index + 1 < len(checks):
            require(nc > definition["target_nash_conv"], "continued after first passing check")
        previous = expected
    final = checks[-1]
    require(report["iterations"] == final["iterations"] and all(final[k] == report["quality"][k]
            for k in ("solver_ev", "solver_br", "nash_conv")), "final quality differs from stopping check")
    target_met = final["nash_conv"] <= definition["target_nash_conv"]
    require(stop["target_met"] is target_met and stop["reason"] == ("target-met" if target_met else "iteration-cap"), "stop reason differs")
    require(target_met or previous == definition["iterations"], "stopped before target or cap")
    total = sum(row["solve_seconds"] + row["quality_seconds"] for row in checks)
    require(total <= report["timing"]["run_seconds"] + 1e-8
            and report["timing"]["time_to_target_seconds"] == report["timing"]["run_seconds"], "time-to-target excludes check cost")
    return stop


def common_header(data, magic, report):
    require(len(data) >= 32 and data[:8] == magic, "canonical header missing")
    iterations, nodes, actions, normalizer = struct.unpack_from("<QIId", data, 8)
    counts = report["counts"]
    require(all(isinstance(counts[key], int) and counts[key] >= 0 for key in ("nodes", "action_nodes", "deals"))
            and counts["nodes"] > 0 and math.isfinite(counts["normalizer"]) and counts["normalizer"] > 0,
            "invalid canonical game counts/normalizer")
    require(iterations == report["iterations"] and nodes == counts["nodes"] and actions == counts["action_nodes"]
            and struct.pack("<d", normalizer) == struct.pack("<d", counts["normalizer"]), "canonical header/report differs")
    position = 32
    for combos in report["canonical"]["global_combos"]:
        require(position + 2 <= len(data), "truncated support")
        count = struct.unpack_from("<H", data, position)[0]
        position += 2
        require(count == len(combos) and position + count * 6 <= len(data), "canonical support count differs")
        for combo in combos:
            actual, weight = struct.unpack_from("<Hf", data, position)
            require(actual == combo and math.isfinite(weight) and 0 < weight <= 1, "canonical root support/weight differs")
            position += 6
    position += nodes * 8
    require(position + 4 <= len(data), "truncated topology")
    deals = struct.unpack_from("<I", data, position)[0]
    position += 4 + deals * 5
    require(deals == counts["deals"] and position <= len(data), "canonical deals differ")
    # Everything shared by layouts/candidates, excluding magic and stopping
    # iteration count. Includes raw normalizer, root weights, nodes and deals.
    return data[16:position]


def sample(store, plan, stage, record):
    directory = join(plan["output"], "stages", stage["label"], "bench")
    report = store.json(join(directory, "result.json"))
    require(report["schema"] == "r1.hu-scaling-bench/v1" and report["status"] == "completed"
            and report["storage"] == "f32" and report["layout"] == "compact" and report["threads"] == 1
            and 0 < report["iterations"] <= stage["iterations"] and report["config"] == plan["inputs"][stage["case"]]["path"], "benchmark invocation differs")
    require(store.json(record["outputs"]["stdout"]["path"]) == report, "stdout report differs")
    require(content(store.pin(join(directory, "config.original.toml"))) == content(plan["inputs"][stage["case"]]), "config copy differs")
    require(0 < report["timing"]["run_seconds"] <= record["elapsed_seconds"], "invalid solve timer")
    require(isinstance(record["measurement"]["root_os_peak_resident_bytes"], int)
            and record["measurement"]["root_os_peak_resident_bytes"] >= 0
            and record["measurement"]["root_os_peak_source"] == "wait4.ru_maxrss_linux_kib", "native Linux peak RSS missing")
    events = [decode(line) for line in store.data(record["outputs"]["stderr"]["path"]).splitlines() if line.strip()]
    events = [event for event in events if event.get("event") == "phase" and event.get("phase") == "run"]
    require([event["status"] for event in events] == ["started", "completed"], "solve phase events missing")
    start, end = [event["process_elapsed_seconds"] for event in events]
    require(0 <= start <= end <= record["elapsed_seconds"] and report["timing"]["run_seconds"] <= end - start + 1e-8,
            "solve timer is outside recorded run phase")
    require(all(math.isfinite(value) and value >= 0 for value in report["timing"].values() if isinstance(value, (float, int))), "nonfinite timing")
    quality, canonical, counts = report["quality"], report["canonical"], report["counts"]
    gains, nc = quality_values(quality)
    require(quality["deviation_gains"] == gains and quality["exploitability_nash_conv_over_two"] == nc / 2,
            "quality derived fields differ")
    require(all(math.isfinite(v) for key in ("subgame_ev", "subgame_br") for v in quality[key]), "nonfinite reported quality")
    stop = stopping_check(report, plan["protocol"]["cases"][stage["case"]])
    require(stop["target_met"], "NashConv target not reached at iteration cap")
    for values, bits in ((quality["solver_ev"], quality["solver_ev_f64_bits_hex"]),
                         (quality["solver_br"], quality["solver_br_f64_bits_hex"])):
        require(len(values) == len(bits) == 2 and all(math.isfinite(v) and struct.pack(">d", v).hex() == b for v, b in zip(values, bits)), "quality bits differ")
    require(struct.pack(">d", quality["nash_conv"]).hex() == quality["nash_conv_f64_bits_hex"] and math.isfinite(quality["nash_conv"]), "NashConv bits differ")
    combos = canonical["global_combos"]
    require(len(combos) == 2 and all(row == sorted(set(row)) and all(0 <= v < 1326 for v in row) for row in combos), "invalid support IDs")
    require(counts["root_dims"] == counts["retained_support_counts"] == [len(row) for row in combos]
            and canonical["union_global_combos"] == sorted(set(sum(combos, []))), "support counts differ")
    artifacts, headers = {}, []
    for field, name in (("strategy_and_cfv", "canonical.bin"), ("supported_state", "state.bin")):
        pin = store.pin(join(directory, name))
        require(canonical[field]["file"] == name and canonical[field]["bytes"] == pin["bytes"]
                and re.fullmatch(r"[0-9a-f]{64}", canonical[field]["blake3"]), "artifact metadata differs")
        artifacts[name] = pin
        headers.append(common_header(store.data(pin["path"]), b"HUCAN001" if name == "canonical.bin" else b"HUSTA001", report))
    require(headers[0] == headers[1], "canonical/state common header differs")
    rows = [decode(line) for line in store.data(record["outputs"]["samples"]["path"]).splitlines() if line.strip()]
    wall_start, wall_end = [event["unix_ms"] / 1000 for event in events]
    phase_rss = [row["tree_resident_bytes"] for row in rows if wall_start <= timestamp(row["at"]) <= wall_end]
    return {"run_seconds": report["timing"]["run_seconds"], "timing": report["timing"], "counts": counts,
            "iterations": report["iterations"], "stopping": stop, "common_header": digest(headers[0]),
            "run_phase_sampled_peak_tree_resident_bytes": max(phase_rss, default=None),
            "quality": quality, "global_combos": combos, "union_global_combos": canonical["union_global_combos"],
            "artifacts": artifacts, "normalized_config": store.pin(join(directory, "config.normalized.toml")),
            "algorithm": report["algorithm"], "rake": report["rake"], "utility": report["utility"],
            "full_process_seconds": record["elapsed_seconds"], "full_process_memory": record["measurement"]}

def same_solution(store, first, second):
    require(first["iterations"] == second["iterations"], "same-arm stopping iterations differ")
    def check_values(sample):
        return [{key: row[key] for key in ("iterations", "solver_ev", "solver_br", "nash_conv")} for row in sample["stopping"]["checks"]]
    require(check_values(first) == check_values(second), "same-arm stopping trajectory differs")
    for key in ("counts", "quality", "global_combos", "union_global_combos", "algorithm", "rake", "utility"):
        require(first[key] == second[key], "solution metadata differs: " + key)
    for name in ("canonical.bin", "state.bin"):
        a, b = first["artifacts"][name], second["artifacts"][name]
        require(content(a) == content(b) and store.data(a["path"]) == store.data(b["path"]), "original solution bytes differ: " + name)
    a, b = first["normalized_config"], second["normalized_config"]
    require(content(a) == content(b) and store.data(a["path"]) == store.data(b["path"]), "normalized config differs")


def same_game(store, first, second):
    for key in ("counts", "global_combos", "union_global_combos", "algorithm", "rake", "utility", "common_header"):
        require(first[key] == second[key], "old/new game differs: " + key)
    a, b = first["normalized_config"], second["normalized_config"]
    require(content(a) == content(b) and store.data(a["path"]) == store.data(b["path"]), "old/new normalized config differs")


def compare_previous(store, previous, stage, actual):
    key = (stage["case"], stage["arm"])
    if key in previous:
        same_solution(store, previous[key], actual)
    other = (stage["case"], "new" if stage["arm"] == "old" else "old")
    if other in previous:
        same_game(store, previous[other], actual)
    previous.setdefault(key, actual)

def summarize(protocol, entries):
    cases, ratios = {}, []
    for case in protocol["cases"]:
        groups = {arm: [row["sample"] for row in entries if row["stage"]["case"] == case and row["stage"]["arm"] == arm and not row["stage"]["warmup"]] for arm in ("old", "new")}
        require(all(len(rows) == 3 for rows in groups.values()), "incomplete measured case")
        times = {arm: [row["run_seconds"] for row in rows] for arm, rows in groups.items()}
        medians = {arm: statistics.median(values) for arm, values in times.items()}
        ratio = medians["new"] / medians["old"]
        ratios.append(ratio)
        memory = {arm: {key: [row["full_process_memory"][key] for row in rows] for key in ("root_os_peak_resident_bytes", "sampled_peak_tree_resident_bytes")} for arm, rows in groups.items()}
        cases[case] = {"run_seconds": times, "medians": medians, "new_over_old": ratio,
                       "paired_new_faster": sum(a < b for a, b in zip(times["new"], times["old"])),
                       "memory_bytes": memory, "memory_medians_bytes": {arm: {key: statistics.median(values) for key, values in fields.items()} for arm, fields in memory.items()}}
        cases[case].update(
            target_nash_conv=protocol["cases"][case]["target_nash_conv"],
            iterations={arm: [row["iterations"] for row in rows] for arm, rows in groups.items()},
            final_quality={arm: rows[0]["quality"] for arm, rows in groups.items()},
            quality_delta_new_minus_old={key: ([a - b for a, b in zip(groups["new"][0]["quality"][key], groups["old"][0]["quality"][key])]
                if isinstance(groups["new"][0]["quality"][key], list) else groups["new"][0]["quality"][key] - groups["old"][0]["quality"][key])
                for key in ("solver_ev", "solver_br", "nash_conv")},
            final_artifact_sha256={arm: {name: pin["sha256"] for name, pin in rows[0]["artifacts"].items()} for arm, rows in groups.items()},
            timed_components_seconds={arm: {key: [sum(check[key] for check in row["stopping"]["checks"]) for row in rows]
                for key in ("solve_seconds", "quality_seconds")} for arm, rows in groups.items()},
            setup_seconds={arm: {key: [row["timing"][key] for row in rows] for key in ("build_seconds", "solver_init_seconds")} for arm, rows in groups.items()},
            run_phase_sampled_peak_tree_resident_bytes={arm: [row["run_phase_sampled_peak_tree_resident_bytes"] for row in rows] for arm, rows in groups.items()})
    geometric_mean = math.exp(sum(math.log(value) for value in ratios) / len(ratios))
    guard = protocol["guard"]
    return {"cases": cases, "geometric_mean_new_over_old": geometric_mean,
            "numerical_fix_cost_guard_pass": geometric_mean <= guard["geometric_mean_new_over_old_max"] and all(ratio <= guard["every_case_new_over_old_max"] for ratio in ratios),
            "timing_scope": "Solve plus all stopping EV/BR checks and loop overhead, through first passing check; post-stop queries/capture excluded",
            "memory_claim": "None: native peak can include pre-exec parent RSS; sampled phase metrics can miss peaks",
            "r1_certification": False}

def check(out):
    out = Path(out)
    if (out / "prepare-failure.json").is_file():
        failure = read(out / "prepare-failure.json")
        require(failure["schema"] == "r1.exact-mass-prepare-failure/v1" and failure["status"] == "failed"
                and failure["resumable"] is False and failure["error"], "invalid preparation failure")
        retained = (out / "retention.json").is_file()
        if retained:
            Store(out)  # Verify every available original payload, no build claim.
        if (out / "result.json").is_file():
            state = read(out / "result.json")
            require(state["status"] == "failed" and "summary" not in state
                    and all(row["status"] == "skipped" for row in state["stages"]), "prepare failure claims execution")
        return {"schema": "r1.exact-mass-verification/v1", "status": "failed", "phase": "prepare",
                "payload_integrity": "available payloads verified" if retained else "no payloads captured",
                "builds": {}, "samples_passed": 0, "summary": None, "error": failure["error"]}
    store = Store(out)
    plan, state = read(out / "plan.json"), read(out / "result.json")
    require(plan["schema"] == "r1.exact-mass-plan/v1" and state["schema"] == "r1.exact-mass-result/v1", "campaign schema")
    plan_pin = store.pin(join(plan["output"], "plan.json"))
    require(content(plan_pin) == content(identity(out / "plan.json")), "local plan differs")
    protocol = plan["protocol"]
    require(protocol == decode(store.data(plan["controls"]["protocol.json"]["path"])), "protocol bytes differ")
    require(protocol == read(HERE / "protocol.json"), "unexpected frozen protocol")
    require(plan["schedule"] == schedule(protocol) and len(plan["schedule"]) == 32, "schedule differs")
    host_record(plan["host"], protocol)
    require(plan["environment"] == {"RAYON_NUM_THREADS": "1"}, "runner environment differs")
    require(timestamp(plan["deadline_utc"]) > timestamp(plan["created_at"]), "deadline invalid")
    builds = {role: validate_build(store, arm, role, plan["host"]["boot_id"]) for role, arm in plan["arms"].items()}
    require(set(builds) == {"old", "new"}, "arm set differs")
    for name in ("crates/cli/examples/hu_scaling_bench.rs", "tools/run_supervised.py"):
        require(store.data(join(plan["arms"]["old"]["source"], name)) == store.data(join(plan["arms"]["new"]["source"], name)), "old/new measurement definition differs: " + name)
    for pin in plan["controls"].values():
        store.verify(pin)
    paths = control_paths(protocol)
    require(set(plan["controls"]) == set(paths), "control set differs")
    for name, path in paths.items():
        require(content(plan["controls"][name]) == content(identity(path)), "trusted local campaign code differs: " + name)
    references(store, plan)
    store.verify(plan["supervisor"])
    require(plan["supervisor"] == store.pin(join(plan["arms"]["new"]["source"], "tools/run_supervised.py")), "supervisor/source binding")
    for case, pin in plan["inputs"].items():
        store.verify(pin)
        require(pin["path"] == join(plan["arms"]["new"]["source"], protocol["config_directory"], protocol["cases"][case]["file"]), "input path differs from source fixture")
        for arm in plan["arms"].values():
            other = store.pin(join(arm["source"], protocol["config_directory"], protocol["cases"][case]["file"]))
            require(content(pin) == content(other), "old/new fixture differs")
    require(set(plan["inputs"]) == set(protocol["cases"]), "case input set differs")
    require(len(state["stages"]) == 32, "result schedule length")
    passed, first_by_case, failed, failed_records = [], {}, False, []
    previous_end = timestamp(plan["created_at"])
    required_pins = [plan_pin, *plan["controls"].values(), plan["supervisor"], *plan["inputs"].values()]
    required_pins += [pin for arm in plan["arms"].values() for pin in (arm["manifest"], arm["archive"], arm["validation"], arm["binary"])]
    for expected, entry in zip(plan["schedule"], state["stages"]):
        require(entry["stage"] == expected, "result stage order differs")
        status = entry["status"]
        if status in ("skipped", "pending"):
            require(status != "skipped" or entry.get("reason"), "missing skip reason")
            failed = True
            continue
        require(not failed, "executed sample after failure/skip")
        if status == "failed":
            failed = True
            require(entry.get("error"), "failed stage lacks reason")
        else:
            require(status == "passed", "nonterminal stage")
            require("record" in entry, "passed sample missing record")
        if "record" in entry:
            record = record_bytes(store, entry["record"], [plan["python"]], success=status == "passed")
            require(record["argv"] == command(plan, expected) and record["cwd"] == plan["output"], "sample invocation differs")
            if status == "passed":
                require(record["resolved_argv"] == command(plan, expected), "sample executable differs")
                require(record["runtime"]["logical_cpus"] == 4 and record["runtime"]["machine"] == "x86_64", "sample runtime differs")
            require(entry.get("supervisor_exit", record["supervisor_exit_code"]) == record["supervisor_exit_code"], "saved supervisor exit differs")
            require(all(record["limits"][key] == protocol["limits"][value] for key, value in LIMIT_KEYS.items()), "sample bounds differ")
            if status == "passed":
                require(all(pin in record["identity_before"] for pin in required_pins), "sample pin binding missing")
            else:
                unavailable = []
                for pin in record["identity_before"] + record["identity_after"]:
                    if pin["path"] == plan["python"]["path"]:
                        continue
                    try:
                        store.verify(pin)
                    except (ValueError, FileNotFoundError):
                        unavailable.append(pin)
                failed_records.append({"stage": expected["label"], "state": record["state"], "stop_reason": record["stop_reason"],
                                       "supervisor_exit_code": record["supervisor_exit_code"], "identity_unchanged": record["identity_unchanged"],
                                       "identity_bytes_unavailable": unavailable, "cleanup_complete": record["cleanup_complete"]})
        if status != "passed":
            continue
        require(entry["host_before"] == entry["host_after"] == plan["host"] and entry["source_after_verified"] is True, "sample host/source changed")
        started, ended = timestamp(record["created_at"]), timestamp(record["ended_at"])
        require(previous_end <= started <= ended, "sample chronology overlaps or runs backward")
        previous_end = ended
        require(timestamp(record["ended_at"]) <= timestamp(plan["deadline_utc"]), "sample exceeded deadline")
        actual = sample(store, plan, expected, record)
        require(actual == entry["sample"], "saved sample summary differs")
        compare_previous(store, first_by_case, expected, actual)
        passed.append(entry)
    if state["status"] == "completed":
        require(len(passed) == 32 and not failed, "incomplete completed campaign")
        summary = summarize(protocol, passed)
        require(summary == state["summary"], "timing summary differs")
    else:
        require(state["status"] in ("ready", "failed"), "nonterminal campaign")
        require("summary" not in state, "incomplete campaign claims summary")
        require((state["status"] == "ready" and all(row["status"] == "pending" for row in state["stages"]))
                or (state["status"] == "failed" and state.get("error") and all(row["status"] != "pending" for row in state["stages"])), "inconsistent status")
        summary = None
    return {"schema": "r1.exact-mass-verification/v1", "status": state["status"], "payload_integrity": "verified",
            "builds": builds, "samples_passed": len(passed), "failed_records": failed_records, "summary": summary,
            "source_after_scope": "Recorded live rehash, not independent post-run filesystem snapshot",
            "tool_retention": "Compiler/Python identities only; measured binaries and raw outputs retained"}

def prepare_inner(args):
    protocol = read(HERE / "protocol.json")
    require(0 < timestamp(args.deadline_utc) - time.time() < 6 * 3600, "deadline must be within six hours")
    out = args.out.resolve()
    store = Store(out, create=True)
    machine = host(protocol)
    python = identity(sys.executable)
    arms = {}
    for role, root in (("old", args.old_root), ("new", args.new_root)):
        root = root.resolve()
        validation_path = root / "validation/result.json"
        state = read(validation_path)
        arm = {"source": str(root / "source"), "manifest": store.add(root / "source-candidate-manifest.json"),
               "archive": store.add(root / "source-candidate.tar.gz"), "validation": store.add(validation_path),
               "binary": store.add(state["binary"]["path"]), "python": python}
        store.add(state["runner"]["path"])
        for stage in state["stages"]:
            retain_record(store, stage["record"]["path"], [*state["tools"].values(), python])
        live_source(arm)
        validate_build(store, arm, role, machine["boot_id"])
        arms[role] = arm
    require(arms["old"]["source"] != arms["new"]["source"] and arms["old"]["binary"]["path"] != arms["new"]["binary"]["path"], "separate sources/targets required")
    controls = {name: store.add(path) for name, path in control_paths(protocol).items()}
    inputs = {case: store.add(Path(arms["new"]["source"]) / protocol["config_directory"] / definition["file"]) for case, definition in protocol["cases"].items()}
    plan = {"schema": "r1.exact-mass-plan/v1", "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "output": str(out), "protocol": protocol, "schedule": schedule(protocol), "arms": arms, "inputs": inputs,
            "controls": controls, "python": python, "supervisor": store.add(Path(arms["new"]["source"]) / "tools/run_supervised.py"),
            "host": machine, "deadline_utc": args.deadline_utc, "environment": {"RAYON_NUM_THREADS": "1"}}
    save(out / "plan.json", plan)
    store.add(out / "plan.json")
    save(out / "result.json", {"schema": "r1.exact-mass-result/v1", "status": "ready", "stages": [{"stage": stage, "status": "pending"} for stage in plan["schedule"]]})
    check(out)
    print(json.dumps({"status": "ready", "output": str(out)}))


def prepare(args):
    out = args.out.resolve()
    require(not out.exists(), "prepare output must be new")
    out.mkdir(parents=True, exist_ok=False)
    try:
        prepare_inner(args)
    except BaseException as error:
        if out.is_dir():
            # Preparation can fail before a complete plan exists. Preserve a
            # separate terminal diagnostic rather than mislabel partial pins
            # as a portable completed/ready campaign.
            save(out / "prepare-failure.json", {"schema": "r1.exact-mass-prepare-failure/v1",
                "status": "failed", "error": repr(error),
                "ended_at": dt.datetime.now(dt.timezone.utc).isoformat(), "resumable": False})
            if (out / "result.json").is_file():
                state = read(out / "result.json")
                state.update(status="failed", error="prepare: " + repr(error))
                state.pop("summary", None)
                for entry in state["stages"]:
                    entry.update(status="skipped", reason="prepare failed")
                save(out / "result.json", state)
        raise

def live_pins(plan):
    pins = [*plan["controls"].values(), plan["supervisor"], plan["python"], *plan["inputs"].values()]
    pins += [pin for arm in plan["arms"].values() for pin in (arm["manifest"], arm["archive"], arm["validation"], arm["binary"])]
    plan_path = str(Path(plan["output"]) / "plan.json")
    pins.append(read(Path(plan["output"]) / "retention.json")["files"][plan_path])
    for pin in pins:
        require(identity(pin["path"]) == pin, "live identity changed")
    for arm in plan["arms"].values():
        live_source(arm)
    require(host(plan["protocol"]) == plan["host"], "live host changed")
    return pins

def measure(args):
    out = args.out.resolve()
    plan, state = read(out / "plan.json"), read(out / "result.json")
    require(check(out)["status"] == "ready", "measurement can start once only")
    require(str(out) == plan["output"], "live output relocated")
    store = Store(out)
    os.environ["RAYON_NUM_THREADS"] = "1"
    os.environ.pop("R1_SOL_WRITE_PHASE_OUTPUT", None)
    state["status"] = "running"
    save(out / "result.json", state)
    previous = {}
    try:
        live_pins(plan)
        spec = importlib.util.spec_from_file_location("bounded_kernel_supervisor", plan["supervisor"]["path"])
        supervisor = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(supervisor)
        for entry in state["stages"]:
            stage = entry["stage"]
            limits = plan["protocol"]["limits"]
            require(time.time() + limits["sample_timeout_seconds"] + 20 < timestamp(plan["deadline_utc"]), "insufficient deadline for fully bounded sample")
            pins = live_pins(plan)
            directory = out / "stages" / stage["label"]
            directory.mkdir(parents=True)
            entry.update(status="running", host_before=host(plan["protocol"]))
            save(out / "result.json", state)
            argv = ["--record", str(directory / "supervisor.json"), "--cwd", str(out), "--disk-path", str(out)]
            for field, key in LIMIT_KEYS.items():
                argv += ["--" + field.replace("_", "-"), str(limits[key])]
            for pin in pins:
                argv += ["--identity-file", pin["path"]]
            require(time.time() + limits["sample_timeout_seconds"] + 20 < timestamp(plan["deadline_utc"]),
                    "insufficient deadline after live rehash")
            code = supervisor.main(argv + ["--", *command(plan, stage)])
            entry["record"] = store.add(directory / "supervisor.json")
            save(out / "result.json", state)
            retain_record(store, directory / "supervisor.json", [plan["python"]], tolerate_identity_failure=code != 0)
            entry["supervisor_exit"] = code
            if (directory / "bench").exists():
                for path in (directory / "bench").rglob("*"):
                    if path.is_file():
                        store.add(path)
            require(code == 0, "supervisor failed: " + stage["label"])
            record = record_bytes(store, entry["record"], [plan["python"]])
            actual = sample(store, plan, stage, record)
            compare_previous(store, previous, stage, actual)
            live_pins(plan)
            entry.update(status="passed", sample=actual, host_after=host(plan["protocol"]), source_after_verified=True)
            save(out / "result.json", state)
            print(json.dumps({"stage": stage["label"], "run_seconds": actual["run_seconds"], "status": "passed"}), flush=True)
        state.update(status="completed", summary=summarize(plan["protocol"], state["stages"]))
        save(out / "result.json", state)
        check(out)
    except BaseException as error:
        state.update(status="failed", error=repr(error))
        state.pop("summary", None)
        for entry in state["stages"]:
            if entry["status"] == "running":
                entry.update(status="failed", error=repr(error))
                directory = out / "stages" / entry["stage"]["label"]
                try:
                    if (directory / "supervisor.json").is_file():
                        entry["record"] = store.add(directory / "supervisor.json")
                        retain_record(store, directory / "supervisor.json", [plan["python"]], tolerate_identity_failure=True)
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
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase", choices=("prepare", "measure", "check"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--old-root", type=Path)
    parser.add_argument("--new-root", type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args()
    if args.phase == "prepare":
        require(args.old_root is not None and args.new_root is not None and args.deadline_utc, "prepare requires both roots/deadline")
        prepare(args)
    elif args.phase == "measure":
        measure(args)
    else:
        print(json.dumps(check(args.out), indent=2, allow_nan=False))

if __name__ == "__main__":
    main()
