"""Bounded final CLI/artifact comparison. Check is portable and executes no evidence."""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import shutil
import statistics
import struct
import sys
import time
import tomllib

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


SHARED = HERE.parent / "showdown-kernel/run.py"
EXACT = HERE.parent / "exact-mass/run.py"
core = load("trusted_final_pipeline_store", SHARED)
exact = load("trusted_final_pipeline_foundation", EXACT)
require, read, decode, save, digest, content, identity, join, timestamp = (
    getattr(core, x) for x in ("require", "read", "decode", "save", "digest", "content", "identity", "join", "timestamp"))
Store, source_files, live_source, record_bytes, retain_record = (
    getattr(core, x) for x in ("Store", "source_files", "live_source", "record_bytes", "retain_record"))
OVERLAYS = {"crates/cli/examples/hu_pipeline_probe.rs": "hu_pipeline_probe.rs",
            "crates/formats/examples/sol_codec_bench.rs": "sol_codec_bench.rs"}
BINS = {"cli": "release/solvers", "audit": "release/examples/hu_saved_profile_audit",
        "probe": "release/examples/hu_pipeline_probe", "codec": "release/examples/sol_codec_bench"}
KINDS = ("solve", "summary", "audit", "decode-all", "read-root", "stream-write")
FIXED_ENV = {"RUSTUP_TOOLCHAIN": "1.97.0", "CARGO_BUILD_JOBS": "2", "CARGO_INCREMENTAL": "0",
             "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "CARGO_PROFILE_RELEASE_DEBUG": "0",
             "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "2", "RUSTFLAGS": "-C target-cpu=native"}


def finite(value):
    return isinstance(value, (float, int)) and not isinstance(value, bool) and math.isfinite(value)


def stable(value):
    return digest(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode())


def controls():
    files = {name: HERE / name for name in ("run.py", "verify.py", "protocol.json", "source-pins.json",
                                          "hu_pipeline_probe.rs", "sol_codec_bench.rs")}
    files.update({f"configs/{case}.toml": HERE / "configs" / f"{case}.toml" for case in ("river", "turn", "flop")})
    files.update({"shared/showdown-run.py": SHARED, "shared/exact-run.py": EXACT})
    return files


def schedule(protocol):
    stages = []
    for case in protocol["cases"]:
        for arm in ("old", "new"):
            stages.append({"kind": "census", "case": case, "arm": arm, "block": None, "warmup": False,
                           "label": f"{case}-census-{arm}"})
    samples = []
    for ci, case in enumerate(protocol["cases"]):
        for block in range(4):
            for arm in (("old", "new") if (ci + block) % 2 == 0 else ("new", "old")):
                samples.append({"case": case, "arm": arm, "block": block, "warmup": block == 0})
    # Keep bulky decoded canonical retention after the primary solve comparison.
    # Other retention can still raise the parent's inherited RSS floor.
    for sample_stage in samples:
        stages.append({**sample_stage, "kind": "solve", "label": f"{sample_stage['case']}-b{sample_stage['block']}-{sample_stage['arm']}-solve"})
    for sample_stage in samples:
        for kind in (*KINDS[1:], *(("checkpoint",) if sample_stage["warmup"] else ())):
            stages.append({**sample_stage, "kind": kind, "label": f"{sample_stage['case']}-b{sample_stage['block']}-{sample_stage['arm']}-{kind}"})
    return stages


def build_schedule():
    return [{"label": "toolchain", "kind": "toolchain", "arm": None}] + [
        {"label": f"{arm}-{kind}", "kind": kind, "arm": arm}
        for arm in ("old", "new") for kind in ("cli", "codec", "probe-clippy", "codec-clippy")]


def stage_dir(plan, stage):
    return join(plan["output"], "stages", stage["label"])


def run_dir(plan, stage):
    return join(plan["output"], "stages", f"{stage['case']}-b{stage['block']}-{stage['arm']}-solve", "run")


def command(plan, builds, stage):
    kind, arm = stage["kind"], stage["arm"]
    bins = builds["binaries"][arm]
    directory = stage_dir(plan, stage)
    config = plan["inputs"][stage["case"]]["path"]
    sol = join(run_dir(plan, stage), "solution.sol")
    if kind == "solve":
        return [bins["cli"]["path"], "solve", config, "--out", join(directory, "run"), "--sol-streets", "full"]
    if kind == "summary":
        return [bins["cli"]["path"], "export", sol, "summary"]
    if kind == "audit":
        return [bins["audit"]["path"], "--sol", sol, "--threads", "1"]
    if kind in ("decode-all", "read-root", "stream-write"):
        return [bins["codec"]["path"], sol, kind, "1", join(directory, "codec")]
    if kind == "census":
        return [bins["probe"]["path"], "--mode", "census", "--config", config, "--out", join(directory, "probe")]
    require(kind == "checkpoint", "unknown stage kind")
    return [bins["probe"]["path"], "--mode", "checkpoint", "--checkpoint", join(run_dir(plan, stage), "checkpoint.ckpt"),
            "--out", join(directory, "probe")]


def build_command(plan, stage):
    if stage["kind"] == "toolchain":
        return [plan["tools"]["rustc"]["path"], "-Vv"]
    target = plan["arms"][stage["arm"]]["target"]
    if stage["kind"] in ("probe-clippy", "codec-clippy"):
        package, example = ("cli", "hu_pipeline_probe") if stage["kind"] == "probe-clippy" else ("formats", "sol_codec_bench")
        return [plan["tools"]["cargo"]["path"], "clippy", "--locked", "--release", "-p", package, "--example", example,
                "--target-dir", target, "--", "-D", "warnings"]
    common = [plan["tools"]["cargo"]["path"], "build", "--locked", "--release"]
    if stage["kind"] == "cli":
        return common + ["-p", "cli", "--bin", "solvers", "--example", "hu_saved_profile_audit",
                         "--example", "hu_pipeline_probe", "--target-dir", target]
    return common + ["-p", "formats", "--example", "sol_codec_bench", "--target-dir", target]


def expected_sources(store, plan):
    refs = store.json(plan["controls"]["source-pins.json"]["path"])
    require(refs["schema"] == "r1.final-pipeline-source-pins/v1", "source reference schema")
    for role, arm in plan["arms"].items():
        manifest = source_files(store, arm)
        require(manifest["base_commit"] == plan["protocol"]["revisions"][role] == refs["arms"][role]["revision"], "source revision differs")
        expected = {row["path"]: content(row) for row in refs["arms"][role]["files"]}
        actual = {row["path"]: content(row) for row in manifest["files"]}
        for path, pin in expected.items():
            if path not in OVERLAYS:
                require(actual.get(path) == pin, "production/source input changed: " + role + ":" + path)
        require({p for p in actual if p.startswith("crates/")} == {p for p in expected if p.startswith("crates/")} | set(OVERLAYS), "crate file set differs")
        cargo_input = lambda p: p.startswith(".cargo/") or ("/" not in p and (p.startswith("Cargo") or p.startswith("rust-toolchain")))
        require({p for p in actual if cargo_input(p)} == {p for p in expected if cargo_input(p)}, "Cargo configuration file set differs")
        for path, control in OVERLAYS.items():
            require(actual.get(path) == content(plan["controls"][control]), "research overlay differs")
    for case, spec in plan["protocol"]["cases"].items():
        pin = plan["inputs"][case]
        store.verify(pin)
        raw = store.data(pin["path"])
        require(content(pin) == content(plan["controls"][f"configs/{case}.toml"]), "input/control differs")
        require(raw.count(b"threads = 1") == 1 and hashlib.sha256(raw.replace(b"threads = 1", b"threads = 8")).hexdigest()
                == spec["original_sha256"], "fixture differs beyond thread count")
        doc = tomllib.loads(raw.decode())
        require(doc["schema"] == "solvers.postflop/v1" and doc["run"]["threads"] == 1 and doc["run"]["storage"] == "f32"
                and doc["run"]["iterations"] == spec["iterations"] and doc["run"]["check_every"] == spec["check_every"]
                and doc["run"]["target_nash_conv"] == spec["target_nash_conv"] and doc["game"]["iso_merging"] is False, "fixture scope differs")


def foundation(store, plan):
    store.verify(plan["foundation_plan"])
    previous = store.json(plan["foundation_plan"]["path"])
    arm = previous["arms"]["new"]
    definition = plan["protocol"]["foundation"]
    require(arm["manifest"]["sha256"] == definition["source_manifest_sha256"]
            and arm["archive"]["sha256"] == definition["archive_sha256"], "wrong historical validation source")
    result = exact.validate_build(store, arm, "new", previous["host"]["boot_id"])
    counts = result["tests"]["workspace-tests"]
    require(counts == {"summary_count": definition["workspace_summaries"], "passed": definition["workspace_passed"],
                       "ignored": definition["workspace_ignored"]}, "historical full tests differ")
    refs = store.json(plan["controls"]["source-pins.json"]["path"])["arms"]["new"]["files"]
    historical = {x["path"]: content(x) for x in store.json(arm["manifest"]["path"])["files"]}
    require(all(historical.get(x["path"]) == content(x) for x in refs), "current source not covered by full validation")
    return {"scope": "historical source-identical production validation, not rerun or same-boot validation", **result}


def merge_store(store, previous):
    for path, pin in previous.entries.items():
        require(path not in store.entries or store.entries[path] == pin, "historical alias collision")
        if not store.blob(pin).exists():
            shutil.copyfile(previous.blob(pin), store.blob(pin))
        store.entries[path] = pin
    for pin in previous.versions:
        if not store.blob(pin).exists():
            shutil.copyfile(previous.blob(pin), store.blob(pin))
        if pin not in store.versions:
            store.versions.append(pin)
    store.flush()


def inventory(plan):
    answer = {}
    for role, arm in plan["arms"].items():
        live_source(arm)
        rows = read(arm["manifest"]["path"])["files"]
        answer[role] = {"files": len(rows), "inventory": stable(sorted(rows, key=lambda x: x["path"]))}
    return answer


def expected_inventory(store, plan):
    return {role: {"files": len(store.json(arm["manifest"]["path"])["files"]),
                   "inventory": stable(sorted(store.json(arm["manifest"]["path"])["files"], key=lambda x: x["path"]))}
            for role, arm in plan["arms"].items()}


def header(data, magic, version):
    require(len(data) >= 50 and data[:8] == magic and int.from_bytes(data[8:10], "little") == version, "artifact header/version")
    return {"version": version, "config_hash": data[10:42].hex(), "iteration": int.from_bytes(data[42:50], "little")}


def codec_canonical(data):
    magic = b"r1.codec.canonical/v1\0"
    require(data.startswith(magic) and len(data) >= len(magic) + 8, "canonical header")
    pos = len(magic)
    length = int.from_bytes(data[pos:pos + 8], "little")
    # Six metadata f64 values: expl[2], ev[2], NC, wall_secs. Exclude only wall_secs.
    wall = pos + 8 + length + 8 + 5 * 8
    require(wall + 8 <= len(data), "truncated canonical metadata")
    require(math.isfinite(struct.unpack("<d", data[wall:wall + 8])[0]), "nonfinite canonical wall time")
    h = hashlib.sha256()
    h.update(data[:wall]); h.update(b"\0" * 8); h.update(data[wall + 8:])
    return {"bytes": len(data), "sha256_without_wall_secs": h.hexdigest()}


def nonnegative_quality(gains, nash_conv, tolerance):
    require(all(finite(x) and x >= -tolerance for x in [*gains, nash_conv]), "materially negative or nonfinite quality")


def trajectory(rows, definition, live, negative_tolerance=1e-6):
    require(rows and all(isinstance(row, dict) for row in rows), "empty progress trajectory")
    expected = list(range(definition["check_every"], definition["iterations"] + 1, definition["check_every"]))
    if not expected or expected[-1] != definition["iterations"]:
        expected.append(definition["iterations"])
    actual = []
    for index, row in enumerate(rows):
        require(row["iteration"] == expected[index] if index < len(expected) else False, "progress cadence differs")
        require(all(finite(row[key]) for key in ("expl_p0", "expl_p1", "nash_conv", "elapsed_secs")), "nonfinite progress")
        nonnegative_quality([row["expl_p0"], row["expl_p1"]], row["nash_conv"], negative_tolerance)
        require(row["expl_p0"] + row["expl_p1"] == row["nash_conv"], "progress gain sum differs")
        require(row["elapsed_secs"] >= 0 and (index == 0 or row["elapsed_secs"] >= rows[index - 1]["elapsed_secs"]), "progress clock went backward")
        if index < len(rows) - 1:
            require(row["nash_conv"] >= definition["target_nash_conv"], "continued after first passing check")
        actual.append({key: row[key] for key in ("iteration", "expl_p0", "expl_p1", "nash_conv")})
    require(rows[-1]["iteration"] == live["iterations"] and rows[-1]["nash_conv"] == live["nashConv"]
            and [rows[-1]["expl_p0"], rows[-1]["expl_p1"]] == [live["explP0"], live["explP1"]], "final progress/live mismatch")
    require(live["nashConv"] < definition["target_nash_conv"], "live quality target not reached")
    return actual


def artifact_pin(store, directory, name, meta):
    pin = store.pin(join(directory, name))
    require(meta["bytes"] == pin["bytes"] and re.fullmatch(r"[0-9a-f]{64}", meta["blake3"]), "artifact metadata invalid")
    return pin


def sample(store, plan, stage, record):
    kind, role, case = stage["kind"], stage["arm"], stage["case"]
    definition = plan["protocol"]["cases"][case]
    directory = stage_dir(plan, stage)
    data = {"seconds": record["elapsed_seconds"], "memory": record["measurement"]}
    require(finite(data["seconds"]) and data["seconds"] > 0, "invalid process time")
    if kind == "solve":
        run = run_dir(plan, stage)
        live = store.json(join(run, "run.json"))
        manifest = store.json(join(run, "manifest.json"))
        require(manifest["state"] == "completed" and manifest["gameKind"] == "postflop" and manifest["failure"] is None, "CLI run did not complete")
        require(live["kind"] == "postflop" and all(finite(live[key]) for key in ("explP0", "explP1", "nashConv", "wallSecs")), "invalid live result")
        require(0 <= live["wallSecs"] <= record["elapsed_seconds"], "reported loop exceeds process time")
        rows = [decode(line) for line in store.data(join(run, "progress.jsonl")).splitlines() if line.strip()]
        data["trajectory"] = trajectory(rows, definition, live, plan["protocol"]["quality_negative_tolerance_chips"])
        data["live"] = {k: v for k, v in live.items() if k != "wallSecs"}
        data["reported_loop_seconds"] = live["wallSecs"]
        data["artifacts"] = {name: store.pin(join(run, name)) for name in ("solution.sol", "checkpoint.ckpt", "run.toml")}
        hsol = header(store.data(data["artifacts"]["solution.sol"]["path"]), b"SLVRSOLV", plan["protocol"]["versions"][role]["sol"])
        hckpt = header(store.data(data["artifacts"]["checkpoint.ckpt"]["path"]), b"SLVRCKPT", plan["protocol"]["versions"][role]["checkpoint"])
        require(hsol["iteration"] == hckpt["iteration"] == live["iterations"]
                and hsol["config_hash"] == hckpt["config_hash"] == manifest["configHash"], "SOL/CKPT/run identity mismatch")
        data["headers"] = {"sol": hsol, "checkpoint": hckpt}
        # Single-process source plus PID observations are prerequisites, not a proof that samples catch every short-lived child.
        samples = [decode(line) for line in store.data(record["outputs"]["samples"]["path"]).splitlines() if line.strip()]
        data["root_only_observed"] = all(not row["pids"] or row["pids"] == [record["pid"]] for row in samples)
        return data
    report = store.json(record["outputs"]["stdout"]["path"])
    if kind == "summary":
        require(report["storage"] == "f32" and report["streets_stored"] == "full", "summary scope")
        require(all(finite(report[x]) for x in ("ev_oop", "ev_ip", "expl_oop", "expl_ip", "nash_conv", "wall_secs")), "nonfinite summary")
        data["report"] = {k: v for k, v in report.items() if k != "wall_secs"}
    elif kind == "audit":
        require(report["schema"] == "solvers.research.hu-saved-profile-audit/v1" and report["threads"] == 1
                and report["artifact"]["format_version"] == plan["protocol"]["versions"][role]["sol"]
                and report["artifact"]["mode"] == "full" and report["artifact"]["source_storage"] == "f32"
                and report["artifact"]["path"] == join(run_dir(plan, stage), "solution.sol"), "audit invocation/scope")
        require(report["value_basis"] == "subgame_start_utility" and report["zero_sum_terminal_utility"] is True
                and report["pot_chips"] == 20 and report["effective_stack_chips"] == 60, "audit game/unit scope")
        q = report["recomputed"]
        require(q["profile"] == "stored_quantized" and all(len(q[k]) == 2 and all(finite(x) for x in q[k]) for k in ("ev", "br", "gains")), "invalid saved profile values")
        nonnegative_quality(q["gains"], q["nash_conv"], plan["protocol"]["quality_negative_tolerance_chips"])
        require(q["gains"] == [b - e for b, e in zip(q["br"], q["ev"])] and q["nash_conv"] == sum(q["gains"])
                and finite(q["nash_conv"]) and q["nash_conv"] < definition["target_nash_conv"], "saved profile quality target failed")
        require(all(finite(report[k]) and 0 <= report[k] <= record["elapsed_seconds"] for k in ("input_hash_secs", "load_secs", "eval_secs")), "invalid audit timer")
        require(sum(report[k] for k in ("input_hash_secs", "load_secs", "eval_secs")) <= record["elapsed_seconds"], "audit timers exceed whole process")
        artifact_pin(store, run_dir(plan, stage), "solution.sol", report["artifact"])
        data.update(quality=q, pre_save={k: v for k, v in report["pre_save_metadata"].items() if k != "wall_secs"},
                    artifact=report["artifact"], timing={k: report[k] for k in ("input_hash_secs", "load_secs", "eval_secs")},
                    economics={k: report[k] for k in ("rake", "utility", "value_basis", "ev_offset")})
    elif kind in ("decode-all", "read-root", "stream-write"):
        output = join(directory, "codec")
        require(report == store.json(join(output, "result.json")), "codec stdout differs")
        require(report["schema"] == "r1.sol-codec-sample/v1" and report["status"] == "completed"
                and report["operation"] == kind and report["iterations"] == 1
                and report["format_version"] == plan["protocol"]["versions"][role]["sol"], "codec scope")
        solpin = artifact_pin(store, run_dir(plan, stage), "solution.sol", report["input"])
        require(set(report["timing"]) == {"preparation_load_seconds", "open_seconds", "operation_seconds", "operation_seconds_per_iteration", "validation_output_seconds"}, "codec timing field set")
        require(all(finite(x) and 0 <= x <= record["elapsed_seconds"] for x in report["timing"].values()), "invalid codec time")
        require(report["timing"]["operation_seconds_per_iteration"] == report["timing"]["operation_seconds"]
                and sum(v for k, v in report["timing"].items() if k != "operation_seconds_per_iteration") <= record["elapsed_seconds"], "codec timer arithmetic differs")
        require(report["metadata"]["config_toml"].encode() == store.data(join(run_dir(plan, stage), "run.toml")), "codec embedded config differs")
        pins = {key: artifact_pin(store, output, name, report[key]) for key, name in (("canonical", "canonical.bin"), ("root_canonical", "root-canonical.bin"))}
        if kind == "read-root":
            require(report["selected_srefs"] == [0] and report["decoded_strategy_blocks"] == report["decoded_value_blocks"] == 1, "partial read scope")
        if kind == "stream-write":
            pin = artifact_pin(store, output, "rewritten.sol", report["rewritten"])
            require(content(pin) == content(solpin) and store.data(pin["path"]) == store.data(solpin["path"]), "writer changed original bytes")
        else:
            require(report["rewritten"] is None, "unexpected rewrite")
        data.update(timing=report["timing"], metadata=report["metadata"], pins=pins,
                    canonical={key: codec_canonical(store.data(pin["path"])) for key, pin in pins.items()})
    else:
        require(report["schema"] == "solvers.hu-pipeline-probe/v1" and report["mode"] == kind, "probe scope")
        output = join(directory, "probe")
        require(report == store.json(join(output, "report.json")), "probe stdout/report differs")
        if kind == "census":
            pins = {name: artifact_pin(store, output, name, report["semantic_files"][name]) for name in ("input.json", "tree.jsonl")}
            inputs = store.json(pins["input.json"]["path"])
            require(inputs["normalizer_f64_bits"] == report["normalizer_f64_bits"] and inputs["zero_sum"] is True, "invalid census normalizer/utility")
            require(isinstance(report["normalizer_f64_bits"], int) and 0 < report["normalizer_f64_bits"] < 0x7ff0000000000000, "invalid normalizer bits")
            data.update(pins=pins, normalizer_f64_bits=report["normalizer_f64_bits"], layout=report["layout_diagnostics"])
        else:
            ckpt = store.pin(join(run_dir(plan, stage), "checkpoint.ckpt"))
            h = header(store.data(ckpt["path"]), b"SLVRCKPT", plan["protocol"]["versions"][role]["checkpoint"])
            require(report["format_version"] == h["version"] and report["iteration"] == h["iteration"]
                    and report["config_hash_hex"] == h["config_hash"] and report["arrays"]["kind"] == "f32"
                    and report["input"]["bytes"] == ckpt["bytes"], "checkpoint decode identity")
            data.update(arrays=report["arrays"], state=artifact_pin(store, output, "state.bin", report["state.bin"]))
    return data


def groups(store, entries, complete=False):
    passed = {row["stage"]["label"]: row for row in entries if row["status"] == "passed"}
    first = {}
    for row in passed.values():
        stage, value = row["stage"], row["sample"]
        kind, case, arm = stage["kind"], stage["case"], stage["arm"]
        key = (kind, case, arm)
        if kind == "census":
            other = passed.get(f"{case}-census-{'new' if arm == 'old' else 'old'}")
            if other:
                for name in ("input.json", "tree.jsonl"):
                    require(content(value["pins"][name]) == content(other["sample"]["pins"][name]), "old/new semantic census differs")
        elif kind == "solve":
            signature = {"live": value["live"], "trajectory": value["trajectory"],
                         "checkpoint": content(value["artifacts"]["checkpoint.ckpt"]), "config": content(value["artifacts"]["run.toml"])}
            require(key not in first or first[key] == stable(signature), "same-arm solve/state differs")
            first[key] = stable(signature)
        else:
            base = f"{case}-b{stage['block']}-{arm}"
            solve = passed.get(base + "-solve")
            require(solve is not None, "dependent stage lacks solve")
            live = solve["sample"]["live"]
            if kind == "summary":
                r = value["report"]
                require(r["iterations"] == live["iterations"] and r["nash_conv"] == live["nashConv"]
                        and [r["expl_oop"], r["expl_ip"]] == [live["explP0"], live["explP1"]], "summary/live disagreement")
                signature = r
            elif kind == "audit":
                r = value["pre_save"]
                require(r["iterations"] == live["iterations"] and r["nash_conv"] == live["nashConv"]
                        and r["expl"] == [live["explP0"], live["explP1"]], "audit/live disagreement")
                summary = passed[base + "-summary"]["sample"]["report"]
                require(r["ev"] == [summary["ev_oop"], summary["ev_ip"]], "audit/summary EV disagreement")
                signature = {"quality": value["quality"], "pre_save": r, "economics": value["economics"]}
            elif kind in ("decode-all", "read-root", "stream-write"):
                metadata = value["metadata"]
                summary = passed[base + "-summary"]["sample"]["report"]
                presave = passed[base + "-audit"]["sample"]["pre_save"]
                require({k: v for k, v in metadata["meta"].items() if k != "wall_secs"} == presave
                        and metadata["node_count"] == summary["nodes"] and metadata["stored_nodes"] == summary["stored_nodes"]
                        and metadata["mode"] == "Full", "codec metadata/summary/audit bridge differs")
                signature = value["canonical"]
                decoded = passed.get(base + "-decode-all")
                if decoded:
                    require(content(value["pins"]["root_canonical"]) == content(decoded["sample"]["pins"]["root_canonical"]), "partial/full root bytes differ")
                    if kind == "stream-write":
                        require(content(value["pins"]["canonical"]) == content(decoded["sample"]["pins"]["canonical"]), "read/write canonical differs")
            else:
                census = passed[f"{case}-census-{arm}"]["sample"]
                require(value["arrays"]["regrets"] == value["arrays"]["strategy_sum"] == census["layout"]["storage_len"], "checkpoint/census storage shape differs")
                signature = content(value["state"])
            require(key not in first or first[key] == stable(signature), "same-arm artifact/profile differs: " + kind)
            first[key] = stable(signature)
    if complete:
        require(len(passed) == 156, "incomplete required process count")


def rss_bound(old, new):
    all_rows = old + new
    eligible = all(row["root_only_observed"] and row["memory"]["root_os_peak_source"] == "wait4.ru_maxrss_linux_kib"
                   and finite(row["memory"]["root_os_peak_resident_bytes"])
                   and finite(row["memory"]["sampled_peak_tree_resident_bytes"])
                   and row["memory"]["root_os_peak_resident_bytes"] >= row["memory"]["sampled_peak_tree_resident_bytes"] > 0 for row in all_rows)
    return max(row["memory"]["root_os_peak_resident_bytes"] for row in new) / min(
        row["memory"]["sampled_peak_tree_resident_bytes"] for row in old) if eligible else None


def summarize(protocol, entries):
    result, solve_ratios, io_screens = {}, [], []
    for case in protocol["cases"]:
        item = {}
        for kind in KINDS:
            rows = {arm: [row["sample"] for row in entries if row["status"] == "passed" and row["stage"]["case"] == case
                           and row["stage"]["arm"] == arm and row["stage"]["kind"] == kind and not row["stage"]["warmup"]]
                    for arm in ("old", "new")}
            require(all(len(values) == 3 for values in rows.values()), "missing measured block")
            values = {arm: [r["timing"]["operation_seconds"] + (r["timing"]["open_seconds"] if kind == "read-root" else 0)
                            if kind in ("decode-all", "read-root", "stream-write") else r["seconds"] for r in rs] for arm, rs in rows.items()}
            medians = {arm: statistics.median(v) for arm, v in values.items()}
            require(medians["old"] > 0, "zero denominator timing")
            ratio = medians["new"] / medians["old"]
            item[kind] = {"seconds": values, "medians": medians, "new_over_old": ratio,
                          "paired_new_faster": sum(n < o for n, o in zip(values["new"], values["old"]))}
            if kind == "solve":
                solve_ratios.append(ratio)
                item[kind]["memory"] = {arm: [r["memory"] for r in rs] for arm, rs in rows.items()}
                item[kind]["os_rss_bound_ratio"] = rss_bound(rows["old"], rows["new"])
                item[kind]["artifact_bytes"] = {arm: {name: [r["artifacts"][name]["bytes"] for r in rs] for name in ("solution.sol", "checkpoint.ckpt")} for arm, rs in rows.items()}
                item[kind]["iterations"] = {arm: [r["live"]["iterations"] for r in rs] for arm, rs in rows.items()}
            elif kind in ("decode-all", "read-root", "stream-write"):
                eligible = medians["old"] >= protocol["guard"]["io_eligibility_old_seconds"]
                screen = ratio <= protocol["guard"]["io_regression_ratio_max"] if eligible else None
                item[kind]["regression_screen"] = screen
                if screen is not None:
                    io_screens.append(screen)
            elif kind == "audit":
                item[kind]["saved_quality"] = {arm: [r["quality"] for r in rs] for arm, rs in rows.items()}
        result[case] = item
    geo = math.exp(sum(math.log(r) for r in solve_ratios) / len(solve_ratios))
    bound = result["flop"]["solve"]["os_rss_bound_ratio"]
    return {"cases": result, "solve_geomean_new_over_old": geo,
            "time_screen": geo <= protocol["guard"]["all_case_solve_geomean_ratio_max"] and all(result[c]["solve"]["new_over_old"] <= protocol["guard"]["turn_flop_solve_median_ratio_max"] for c in ("turn", "flop")),
            "memory_screen": bound <= protocol["guard"]["flop_os_rss_bound_ratio_max"] if bound is not None else None,
            "io_screen": all(io_screens) if io_screens else None, "phase_timings": None,
            "scope": "Separate descriptive screens; no overall R1 certification or physical/phase RSS claim."}


def timeout(plan, stage, building=False):
    limits = plan["protocol"]["limits"]
    return limits["build_timeout_seconds"] if building else limits[
        "sample_timeout_seconds" if stage["kind"] in ("solve", "audit", "census", "checkpoint") else "query_timeout_seconds"]


def common_pins(plan):
    return [*plan["controls"].values(), *plan["inputs"].values(), plan["supervisor"], plan["python"],
            *plan["tools"].values(), plan["foundation_plan"],
            *[pin for arm in plan["arms"].values() for pin in (arm["archive"], arm["manifest"])]]


def live_check(plan, builds=None):
    pins = common_pins(plan)
    if builds and builds["status"] == "completed":
        pins += [pin for bins in builds["binaries"].values() for pin in bins.values()]
    for pin in pins:
        require(identity(pin["path"]) == pin, "live file changed: " + pin["path"])
    require(core.host(plan["protocol"]) == plan["host"], "host/boot changed")
    return inventory(plan)


def verify_record(store, plan, entry, argv, cwd, seconds, previous_end, pins, *, passed):
    require("record" in entry or not passed, "passed stage lacks original record")
    if "record" not in entry:
        return None, previous_end
    record = record_bytes(store, entry["record"], [plan["python"], *plan["tools"].values()], success=passed)
    if not passed:
        return record, previous_end
    require(record["argv"] == argv and record["resolved_argv"] == argv and record["cwd"] == cwd, "stage command/resolved executable differs")
    limits = plan["protocol"]["limits"]
    expected = {"timeout_seconds": seconds, "memory_limit_bytes": limits["rss_bytes"],
                "min_free_memory_bytes": limits["min_free_bytes"], "disk_reserve_bytes": limits["disk_reserve_bytes"],
                "poll_seconds": limits["poll_seconds"], "grace_seconds": limits["grace_seconds"], "kill_wait_seconds": limits["kill_wait_seconds"]}
    require(all(record["limits"][k] == v for k, v in expected.items()), "supervisor bound differs")
    require(entry["supervisor_exit"] == 0 and all(pin in record["identity_before"] for pin in pins), "missing command identity")
    require(entry["host_before"] == entry["host_after"] == plan["host"], "recorded host changed")
    require(entry["source_before"] == entry["source_after"] == expected_inventory(store, plan), "source inventory claim differs")
    started, child, ended = map(timestamp, (record["created_at"], record["started_at"], record["ended_at"]))
    require(previous_end <= started <= child <= ended <= timestamp(plan["deadline_utc"]), "stage chronology/deadline differs")
    require(finite(record["elapsed_seconds"]) and 0 < record["elapsed_seconds"] <= seconds + limits["grace_seconds"] + limits["kill_wait_seconds"] + 5, "invalid elapsed time")
    return record, ended


def suffix(entries, state, expected, allow_ready):
    require([row["stage"] for row in entries] == expected, "stage schedule differs")
    statuses = [row["status"] for row in entries]
    require(all(s in ("pending", "passed", "failed", "skipped") for s in statuses), "unknown/nonterminal stage status")
    if state == "completed":
        require(all(s == "passed" for s in statuses), "incomplete successful schedule")
    elif state in allow_ready:
        require(all(s == "pending" for s in statuses), "ready schedule already used")
    elif state == "failed":
        require("pending" not in statuses and statuses.count("failed") <= 1, "invalid failed suffix")
        first = next((i for i, s in enumerate(statuses) if s != "passed"), len(statuses))
        require(all(s in ("failed", "skipped") for s in statuses[first:])
                and all(s == "skipped" for s in statuses[first + 1:]), "noncontiguous failed/skipped suffix")
    else:
        raise ValueError("unsupported/nonterminal campaign status")


def check(out):
    out = Path(out)
    store = Store(out)
    if (out / "prepare-failure.json").exists():
        failure = read(out / "prepare-failure.json")
        require(failure["status"] == "failed" and failure.get("error"), "invalid prepare failure")
        return {"schema": "r1.final-pipeline-verification/v1", "status": "failed", "scope": "prepare failed; no measurement or complete provenance claim",
                "error": failure["error"], "payload_integrity": "verified"}
    plan, builds, result = (read(out / name) for name in ("plan.json", "build.json", "result.json"))
    require(plan["schema"] == "r1.final-pipeline-plan/v1" and result["schema"] == "r1.final-pipeline-result/v1"
            and builds["schema"] == "r1.final-pipeline-build/v1", "campaign schema")
    protocol = plan["protocol"]
    require(protocol == read(HERE / "protocol.json") and protocol["status"] == "frozen", "protocol is not frozen/current")
    require(plan["environment"] == FIXED_ENV, "build/run environment differs")
    require(list(protocol["cases"]) == ["river", "turn", "flop"] and protocol["threads"] == 1
            and protocol["warmup_blocks"] == 1 and protocol["measured_blocks"] == 3, "protocol scope changed")
    core.host_record(plan["host"], protocol)
    for key, pin in plan["controls"].items():
        store.verify(pin)
        require(key in controls() and content(identity(controls()[key])) == content(pin), "trusted control differs: " + key)
    require(set(plan["controls"]) == set(controls()), "control set differs")
    expected_sources(store, plan)
    require(store.pin(join(plan["arms"]["new"]["source"], "tools/run_supervised.py")) == plan["supervisor"], "supervisor/source binding differs")
    history = foundation(store, plan)
    plan_pin = store.pin(join(plan["output"], "plan.json"))
    require(store.data(plan_pin["path"]) == (out / "plan.json").read_bytes(), "plan original bytes changed")
    shared_pins = common_pins(plan) + [plan_pin]
    suffix(builds["stages"], builds["status"], build_schedule(), ("pending",))
    previous_end = timestamp(plan["created_at"])
    for entry in builds["stages"]:
        st = entry["stage"]
        cwd = plan["arms"][st["arm"]]["source"] if st["arm"] else plan["arms"]["new"]["source"]
        record, previous_end = verify_record(store, plan, entry, build_command(plan, st), cwd, timeout(plan, st, True), previous_end, shared_pins, passed=entry["status"] == "passed")
        if entry["status"] == "passed" and st["kind"] == "toolchain":
            text = store.data(record["outputs"]["stdout"]["path"]).decode()
            require("release: 1.97.0" in text and "host: x86_64-unknown-linux-gnu" in text, "wrong toolchain output")
    if builds["status"] == "completed":
        require(set(builds["binaries"]) == {"old", "new"}, "missing built arm")
        for role, arm in plan["arms"].items():
            require(set(builds["binaries"][role]) == set(BINS), "missing built executable")
            for key, relative in BINS.items():
                pin = builds["binaries"][role][key]
                store.verify(pin)
                require(pin["path"] == join(arm["target"], relative), "wrong build output path")
        require(builds["host"] == plan["host"] and builds["source_after"] == expected_inventory(store, plan), "build source/host changed")
    else:
        require(result["status"] in ("awaiting_build", "failed"), "measurement without complete build")
    suffix(result["stages"], result["status"], schedule(protocol), ("ready", "awaiting_build"))
    passed = []
    if result["status"] in ("ready", "completed"):
        require(builds["status"] == "completed", "missing successful build")
    for entry in result["stages"]:
        if entry["status"] == "pending" or entry["status"] == "skipped":
            continue
        st = entry["stage"]
        argv = command(plan, builds, st)
        pins = shared_pins + [store.pin(join(plan["output"], "build.json"))] + [pin for bins in builds["binaries"].values() for pin in bins.values()]
        if st["kind"] not in ("census", "solve"):
            pins += [store.pin(join(run_dir(plan, st), name)) for name in ("solution.sol", "checkpoint.ckpt", "run.toml")]
        record, previous_end = verify_record(store, plan, entry, argv, plan["output"], timeout(plan, st), previous_end, pins, passed=entry["status"] == "passed")
        if entry["status"] == "passed":
            actual = sample(store, plan, st, record)
            require(actual == entry["sample"], "derived sample differs")
            passed.append(entry)
    groups(store, passed, result["status"] == "completed")
    if result["status"] == "completed":
        summary = summarize(protocol, passed)
        require(summary == result["summary"], "summary recomputation differs")
    else:
        require("summary" not in result, "partial result claims summary")
        summary = None
    if result["status"] == "failed":
        require(result.get("error"), "failure reason missing")
    return {"schema": "r1.final-pipeline-verification/v1", "status": result["status"], "build_status": builds["status"],
            "processes_passed": len(passed), "historical_validation": history, "payload_integrity": "verified",
            "summary": summary, "limitations": ["BLAKE3 report strings are bound to native output, not independently recomputed by Python",
                "source-after is recorded live inventory verification", "OS RSS counter bound, not exact physical or per-phase peak",
                "census does not enumerate terminal evaluator internals or infer public card IDs from sparse masks"]}


def environment(plan):
    for key in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_ENCODED_RUSTFLAGS", "R1_PHASE_OUTPUT", "R1_SOL_WRITE_PHASE_OUTPUT"):
        os.environ.pop(key, None)
    os.environ.update(plan["environment"])
    os.environ["RUSTC"] = plan["tools"]["rustc"]["path"]
    os.environ["PATH"] = str(Path(plan["tools"]["rustc"]["path"]).parent) + os.pathsep + os.environ.get("PATH", "")


def prepare(args):
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    store = Store(out, create=True)
    try:
        protocol = read(HERE / "protocol.json")
        require(protocol["status"] == "frozen", "protocol is draft; root must freeze before deployment")
        machine = core.host(protocol)
        require(time.time() + 120 < timestamp(args.deadline_utc) <= time.time() + 3 * 3600, "deadline must be within next3hours")
        arms = {}
        for role in ("old", "new"):
            root, target = getattr(args, role + "_root").resolve(), getattr(args, role + "_target").resolve()
            require(not target.exists(), "target must be fresh and absent")
            require(not target.is_relative_to(root / "source") and not out.is_relative_to(root / "source"), "output/target inside source")
            arms[role] = {"source": str(root / "source"), "target": str(target),
                          "manifest": store.add(root / "source-candidate-manifest.json"), "archive": store.add(root / "source-candidate.tar.gz")}
        require(arms["old"]["source"] != arms["new"]["source"] and arms["old"]["target"] != arms["new"]["target"], "sources/targets must differ")
        separated = [out, *[Path(arm[key]) for arm in arms.values() for key in ("source", "target")]]
        require(not any(a.is_relative_to(b) or b.is_relative_to(a) for i, a in enumerate(separated) for b in separated[i + 1:]), "source/target/output roots overlap")
        control = {key: store.add(path) for key, path in controls().items()}
        (out / "inputs").mkdir()
        inputs = {}
        for case in protocol["cases"]:
            path = out / "inputs" / (case + ".toml")
            path.write_bytes((HERE / "configs" / (case + ".toml")).read_bytes())
            inputs[case] = store.add(path)
        old_store = Store(args.validation_proof)
        merge_store(store, old_store)
        plan = {"schema": "r1.final-pipeline-plan/v1", "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                "output": str(out), "protocol": protocol, "host": machine, "deadline_utc": args.deadline_utc,
                "arms": arms, "inputs": inputs, "controls": control, "python": identity(sys.executable),
                "tools": {"cargo": identity(args.cargo), "rustc": identity(args.rustc)}, "environment": FIXED_ENV,
                "supervisor": store.add(Path(arms["new"]["source"]) / "tools/run_supervised.py"),
                "foundation_plan": store.add(args.validation_proof / "plan.json")}
        expected_sources(store, plan)
        foundation(store, plan)
        live_check(plan)
        save(out / "plan.json", plan); store.add(out / "plan.json")
        save(out / "build.json", {"schema": "r1.final-pipeline-build/v1", "status": "pending", "binaries": {},
                                  "stages": [{"stage": st, "status": "pending"} for st in build_schedule()]})
        save(out / "result.json", {"schema": "r1.final-pipeline-result/v1", "status": "awaiting_build",
                                   "stages": [{"stage": st, "status": "pending"} for st in schedule(protocol)]})
        check(out)
    except BaseException as error:
        save(out / "prepare-failure.json", {"status": "failed", "error": repr(error), "time": dt.datetime.now(dt.timezone.utc).isoformat()})
        raise
    print(json.dumps({"status": "awaiting_build", "output": str(out)}))


def retain_directory(store, directory):
    for path in sorted(directory.rglob("*")):
        require(not path.is_symlink(), "output symlink")
        if path.is_file():
            store.add(path, changed_identity=True)


def retain_stage(store, directory, tools):
    # Always sweep available outputs even when a recorded output pin mismatches.
    errors = []
    try:
        if (directory / "supervisor.json").exists():
            retain_record(store, directory / "supervisor.json", tools, tolerate_identity_failure=True)
    except (OSError, ValueError) as error:
        errors.append(repr(error))
    try:
        retain_directory(store, directory)
    except (OSError, ValueError) as error:
        errors.append(repr(error))
    require(not errors, "stage retention failures: " + "; ".join(errors))


def execute(plan, builds, state, entry, store, supervisor, *, building):
    st = entry["stage"]
    seconds = timeout(plan, st, building)
    require(time.time() + seconds + 20 < timestamp(plan["deadline_utc"]), "insufficient remaining deadline")
    entry.update(status="running", host_before=core.host(plan["protocol"]), source_before=live_check(plan, builds if not building else None))
    state_path = Path(plan["output"]) / ("build.json" if building else "result.json")
    save(state_path, state)
    directory = Path(plan["output"]) / ("build-stages" if building else "stages") / st["label"]
    directory.mkdir(parents=True, exist_ok=False)
    cwd = plan["arms"][st["arm"]]["source"] if building and st["arm"] else plan["arms"]["new"]["source"] if building else plan["output"]
    command_argv = build_command(plan, st) if building else command(plan, builds, st)
    pins = common_pins(plan) + [store.pin(join(plan["output"], "plan.json"))]
    if not building:
        pins += [store.pin(join(plan["output"], "build.json"))] + [pin for bins in builds["binaries"].values() for pin in bins.values()]
        if st["kind"] not in ("solve", "census"):
            pins += [store.pin(join(run_dir(plan, st), name)) for name in ("solution.sol", "checkpoint.ckpt", "run.toml")]
    args = ["--record", str(directory / "supervisor.json"), "--cwd", cwd, "--disk-path", plan["output"]]
    limits = plan["protocol"]["limits"]
    for key, value in {"timeout_seconds": seconds, "memory_limit_bytes": limits["rss_bytes"], "min_free_memory_bytes": limits["min_free_bytes"],
                       "disk_reserve_bytes": limits["disk_reserve_bytes"], "poll_seconds": limits["poll_seconds"], "grace_seconds": limits["grace_seconds"], "kill_wait_seconds": limits["kill_wait_seconds"]}.items():
        args += ["--" + key.replace("_", "-"), str(value)]
    for pin in pins:
        args += ["--identity-file", pin["path"]]
    require(time.time() + seconds + 20 < timestamp(plan["deadline_utc"]), "deadline consumed during source checks")
    code = supervisor.main(args + ["--", *command_argv])
    entry["record"] = store.add(directory / "supervisor.json")
    entry["supervisor_exit"] = code
    save(state_path, state)
    retain_stage(store, directory, [plan["python"], *plan["tools"].values()])
    require(code == 0, "supervised process failed: " + st["label"])
    record = record_bytes(store, entry["record"], [plan["python"], *plan["tools"].values()])
    if not building:
        entry["sample"] = sample(store, plan, st, record)
    entry.update(host_after=core.host(plan["protocol"]), source_after=live_check(plan, builds if not building else None), status="passed")
    save(state_path, state)
    print(json.dumps({"stage": st["label"], "status": "passed", "seconds": record["elapsed_seconds"]}), flush=True)


def mark_failure(plan, state, store, error, *, building):
    state.update(status="failed", error=repr(error)); state.pop("summary", None)
    for entry in state["stages"]:
        if entry["status"] == "running":
            entry.update(status="failed", error=repr(error))
            directory = Path(plan["output"]) / ("build-stages" if building else "stages") / entry["stage"]["label"]
            try:
                if (directory / "supervisor.json").exists():
                    entry["record"] = store.add(directory / "supervisor.json", changed_identity=True)
                retain_stage(store, directory, [plan["python"], *plan["tools"].values()])
            except (OSError, ValueError) as failure:
                entry["retention_error"] = repr(failure)
        elif entry["status"] == "pending":
            entry.update(status="skipped", reason=repr(error))
    save(Path(plan["output"]) / ("build.json" if building else "result.json"), state)


def run(args, *, building):
    out = args.out.resolve()
    current = check(out)
    require(current["status"] == ("awaiting_build" if building else "ready"), "phase can run once, in order")
    plan, builds, result = (read(out / name) for name in ("plan.json", "build.json", "result.json"))
    require(str(out) == plan["output"], "live output was relocated")
    state = builds if building else result
    store = Store(out)
    state["status"] = "running"
    save(out / ("build.json" if building else "result.json"), state)
    environment(plan)
    try:
        if building:
            require(all(not Path(arm["target"]).exists() for arm in plan["arms"].values()), "fresh target appeared after prepare")
        live_check(plan, builds if not building else None)
        supervisor = load("trusted_live_final_supervisor", plan["supervisor"]["path"])
        for entry in state["stages"]:
            execute(plan, builds, state, entry, store, supervisor, building=building)
            if not building:
                try:
                    groups(store, state["stages"])
                except BaseException:
                    # A process may exit0 but fail a cross-sample invariant.
                    # Preserve its record as a failed semantic stage, not a passed prefix.
                    entry["status"] = "running"
                    raise
        if building:
            builds["binaries"] = {role: {key: store.add(Path(arm["target"]) / rel) for key, rel in BINS.items()} for role, arm in plan["arms"].items()}
            builds.update(status="completed", host=core.host(plan["protocol"]), source_after=live_check(plan))
            save(out / "build.json", builds)
            result["status"] = "ready"; save(out / "result.json", result)
            check(out)
            store.add(out / "build.json")
        else:
            state.update(status="completed", summary=summarize(plan["protocol"], state["stages"]))
            save(out / "result.json", state)
            check(out)
    except BaseException as error:
        mark_failure(plan, state, store, error, building=building)
        if building:
            mark_failure(plan, result, store, error, building=False)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", choices=("prepare", "build", "measure", "check"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    for name in ("old-root", "new-root", "old-target", "new-target", "cargo", "rustc", "validation-proof"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--deadline-utc")
    args = parser.parse_args()
    if args.phase == "prepare":
        require(all(getattr(args, x) for x in ("old_root", "new_root", "old_target", "new_target", "cargo", "rustc", "validation_proof", "deadline_utc")), "prepare arguments missing")
        prepare(args)
    elif args.phase == "check":
        print(json.dumps(check(args.out), indent=2, allow_nan=False))
    else:
        run(args, building=args.phase == "build")


if __name__ == "__main__":
    main()
