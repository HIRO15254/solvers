#!/usr/bin/env python3
"""Recompute final source06 VM06 performance and saved-policy checks offline.

Never executes a retained binary/script, changes immutable evidence, or treats
historical damaged build records as successful evidence. Raw bundle bytes are
required for large artifacts absent from Git; paths are resolved through the
four retention inventories, never by assuming a VM path exists locally.
"""
from __future__ import annotations

import argparse
import copy
import datetime as dt
import hashlib
import importlib.util
import json
import math
from pathlib import Path, PurePosixPath
import re
import statistics
import struct
import sys
import tarfile
import tomllib
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
EXP = HERE.parent
REPO = EXP.parents[1]
PERF = "/opt/r1/candidate-v3/"
PLAN = PERF + "experiments/hu-postflop-r1/pipeline/frozen/vm06-current/plan.json"
REPORT = PERF + "runs/r1-paired-current/comparison.json"
AUDIT_PLAN = "/opt/r1/current-audit-plan/plan.json"
AUDITS = "/opt/r1/current-audits/audits.json"
BUNDLES = {
    "pipeline/evidence-vm06-current": ("vm06-current.tar.gz", "cb4ed8a32bc5ce5b67ebbd18dd17e00262bbd823207f82270a6a92f5ca47a4ee", 943),
    "pipeline/evidence-vm06-v3": ("vm06-v3-results.tar.gz", "3c679810a39b3fac58d394f95387b0d98cec94ccc299f325e5922eefb9264e2d", 763),
    "saved-profile/evidence-vm06-audits": ("vm06-saved-audits.tar.gz", "378f3338a3c4ade87e01e749c33b4e5a232da2e5074385ffc70c64ab50d8d3bb", 247),
    "saved-profile/evidence-vm06-build": ("vm06-recovery.tar.gz", "d40e55c982f8702640e744e550f5c7072a0777bb84cb547d9ee22bc53e2750a5", 118),
}
SUMMARY_FIELDS = ("board", "pot", "effective_stack", "min_bet", "iterations", "ev_oop", "ev_ip", "expl_oop", "expl_ip", "nash_conv", "storage", "streets_stored", "nodes", "stored_nodes")
RUNTIME_ONLY = {"/usr/bin/python3.12", "/usr/bin/b3sum", "/usr/bin/bash",
                "/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/cargo",
                "/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/rustc"}


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha(data): return hashlib.sha256(data).hexdigest()


def document(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result
    def invalid(value): raise ValueError("nonfinite JSON constant: " + value)
    require(b"\0" not in raw, "NUL in adopted evidence")
    return json.loads(raw.decode("utf-8"), object_pairs_hook=pairs, parse_constant=invalid)


def identity(path):
    data = path.read_bytes()
    return {"path": str(path.resolve()), "bytes": len(data), "sha256": sha(data)}


def instant(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "missing timestamp timezone")
    return result


def stats(values):
    require(values and all(type(x) in (float, int) and math.isfinite(x) for x in values), "missing/nonfinite measurement")
    return {"n": len(values), "values": values, "median": statistics.median(values), "min": min(values), "max": max(values)}


class Evidence:
    def __init__(self):
        self.rows, self.data, self.prefixes, self.bundles = {}, {}, {}, []
        self.unavailable_runtime, self.links, self.stages = {}, 0, {}
        for folder, (name, digest, count) in BUNDLES.items():
            home = EXP / folder
            retention = document((home / "retention.json").read_bytes())
            path = REPO / "runs/r1-cloud" / name
            actual = identity(path)
            require(actual["sha256"] == digest, "wrong pinned bundle: " + name)
            require(len(retention["bundles"]) == 1 and retention["bundles"][0]["archive"]["sha256"] == digest, "retention/bundle mismatch")
            index = document((home / "compact-index.json").read_bytes())
            for item in index["files"]:
                target = (home / item["path"]).resolve()
                require(target.is_relative_to(home.resolve()), "unsafe compact path")
                raw = target.read_bytes()
                require(len(raw) == item["bytes"] and sha(raw) == item["sha256"], "compact index mismatch")
            mapping = {r["archive_member"]: r for r in retention["files"]}
            require(len(mapping) == len(retention["files"]) == count, "retention member count mismatch")
            manifest_raw = (home / "bundles" / retention["bundles"][0]["label"] / "collector-manifest.json").read_bytes()
            require(sha(manifest_raw) == retention["bundles"][0]["manifest"]["sha256"], "collector manifest mismatch")
            collector = document(manifest_raw)
            inventory = {r["archive_member"]: r for r in collector["files"]}
            require(set(mapping) == set(inventory), "collector/retention inventory mismatch")
            seen = set()
            with tarfile.open(path, "r:gz") as archive:
                for member in archive:
                    require(member.isfile() and member.name not in seen and member.size <= 64*1024**2, "unsafe/duplicate bundle member")
                    seen.add(member.name)
                    raw = archive.extractfile(member).read()
                    if member.name == "retention-manifest.json":
                        require(raw == manifest_raw, "embedded manifest differs")
                        continue
                    require(member.name in mapping, "unlisted bundle member")
                    row = mapping[member.name]
                    require(row["included"] is True and row["kind"] == "regular", "unretained payload")
                    for key in ("original_path", "bytes", "sha256"):
                        require(row[key] == inventory[member.name][key], "collector row differs")
                    require(len(raw) == row["bytes"] and sha(raw) == row["sha256"], "payload hash mismatch")
                    self.register(row["original_path"], raw, {"bundle": name, "member": member.name, "compact": str(home / row["compact_path"]) if row["compact_path"] else None})
                    if row["compact_path"]:
                        require((home / row["compact_path"]).read_bytes() == raw, "compact payload differs")
            require(seen == set(mapping) | {"retention-manifest.json"}, "bundle is incomplete")
            self.bundles.append({**actual, "payloads_verified": count, "compact_index_files_verified": len(index["files"]),
                                 "retention": identity(home / "retention.json"), "generic_ready_unchanged": retention["validation"]["ready"]})
        for vm, local, prefix in [
            ("/opt/r1/baseline-source.tar.gz", REPO / "runs/r1-cloud/build-source-9632d8b.tar.gz", "/opt/r1/baseline/"),
            ("/opt/r1/candidate-v3-source.tar.gz", EXP / "validation/sources/current-03.tar.gz", PERF),
            ("/opt/r1/audit-source.tar.gz", EXP / "validation/sources/current-06.tar.gz", "/opt/r1/audit-source/")]:
            self.register(vm, local.read_bytes(), {"retained_source": str(local)})
            with tarfile.open(local) as archive:
                for member in archive:
                    if member.isdir(): continue
                    name = PurePosixPath(member.name)
                    require(member.isfile() and not name.is_absolute() and ".." not in name.parts, "unsafe source member")
                    self.register(prefix + member.name, archive.extractfile(member).read(), {"retained_source": str(local), "member": member.name})

    def register(self, path, raw, location):
        key = (path, len(raw), sha(raw))
        self.rows.setdefault(key, []).append(location)
        self.prefixes[key] = raw[:128]
        if len(raw) <= 2*1024**2:
            self.data[key] = raw

    def key(self, item):
        if isinstance(item, str):
            found = [k for k in self.rows if k[0] == item]
            require(len(found) == 1, "absent/ambiguous VM path: " + item)
            return found[0]
        return (item["path"], item["bytes"], item["sha256"])

    def bound(self, item, *, runtime=False):
        key = self.key(item)
        self.links += 1
        if key not in self.rows and runtime and key[0] in RUNTIME_ONLY:
            previous = self.unavailable_runtime.setdefault(key[0], dict(item))
            require(previous == item, "runtime identity changed")
            return key
        require(key in self.rows, "required identity unavailable: " + str(key))
        return key

    def raw(self, item):
        key = self.bound(item)
        require(key in self.data, "large evidence is hash-only in this verifier: " + key[0])
        return self.data[key]

    def json(self, item): return document(self.raw(item))

    def stage(self, stage):
        record = self.json(stage["record"])
        path = stage["record"]["path"]
        require(record["schema"] == "solvers.supervised-run/v1", "wrong supervisor schema")
        for item in (record, stage):
            require(item["state"] == "completed" and item["stop_reason"] == "completed"
                    and item["cleanup_complete"] is True and item["identity_unchanged"] is True, "unsuccessful stage")
        require(stage["exit_code"] == record["child_exit_code"] == record["supervisor_exit_code"] == 0, "nonzero stage exit")
        require(not record["forced"] and not record["errors"] and record["shell"] is False, "forced/error/shell stage")
        require(record["identity_before"] == record["identity_after"], "stage input identity changed")
        for item in record["identity_before"]: self.bound(item, runtime=True)
        require(stage["stdout"] == record["outputs"]["stdout"] and stage["measurement"] == record["measurement"]
                and stage["elapsed_seconds"] == record["elapsed_seconds"], "embedded stage differs from supervisor")
        if path in self.stages:
            require(self.stages[path] == record, "stage record changed")
            return record
        for item in record["outputs"].values(): self.bound(item)
        samples = [document(line) for line in self.raw(record["outputs"]["samples"]).splitlines() if line.strip()]
        m = record["measurement"]
        require(len(samples) == m["sample_count"] > 0 and samples[-1] == record["last_sample"]
                and samples[-1]["pids"] == [], "sample/cleanup mismatch")
        times = [x["elapsed_seconds"] for x in samples]
        require(times == sorted(times) and all(0 <= x <= record["elapsed_seconds"] for x in times), "sample clock mismatch")
        require(max(x["tree_resident_bytes"] for x in samples) == m["sampled_peak_tree_resident_bytes"], "sample peak mismatch")
        require(samples[-1]["root_os_peak_resident_bytes"] == m["root_os_peak_resident_bytes"], "native peak mismatch")
        a, b = instant(record["started_at"]), instant(record["ended_at"])
        require(a <= b and abs((b-a).total_seconds()-record["elapsed_seconds"]) < 0.1, "outer duration mismatch")
        require(all(a <= instant(s["at"]) <= b for s in samples), "sample timestamp outside stage")
        limits = record["limits"]
        require(limits["memory_limit_bytes"] == 40*1024**3 and limits["min_free_memory_bytes"] == 8*1024**3
                and limits["disk_reserve_bytes"] == 10*1024**3 and limits["grace_seconds"] == limits["kill_wait_seconds"] == 5,
                "resource limits changed")
        require(record["elapsed_seconds"] <= limits["timeout_seconds"]+10, "stage exceeded bound")
        self.stages[path] = record
        return record


def normalized_config(raw):
    doc = tomllib.loads(raw.decode("utf-8"))
    require(set(doc) <= {"schema", "game", "algorithm", "run", "rake", "utility"}, "unknown config field")
    defaults = {
        "game": {"min_bet": 1, "iso_merging": False, "preflop_aggressor": "none"},
        "algorithm": {"schedule": "dcfr", "alpha": 1.5, "beta": 0.0, "gamma": 3.0, "pow4_reset": True},
        "run": {"par_chance_depth": 2, "par_min_children": 12},
    }
    allowed = {"game": {"board", "oop_range", "ip_range", "pot", "effective_stack", "tree", *defaults["game"]},
               "algorithm": set(defaults["algorithm"]), "run": {"iterations", "check_every", "target_nash_conv", "threads", "storage", *defaults["run"]}}
    for section, values in defaults.items():
        sub = doc.setdefault(section, {})
        require(set(sub) <= allowed[section], "unknown " + section + " field")
        for key, value in values.items(): sub.setdefault(key, value)
    tree = doc["game"]["tree"]
    require(set(tree) <= {"kind", "script", "include_allin", "params", "max_aggressive_actions"} and tree["kind"] == "script", "unsupported tree")
    tree["script"] = tree["script"].strip()
    tree.setdefault("include_allin", False)
    tree.setdefault("params", {})
    caps = tree.setdefault("max_aggressive_actions", {})
    require(set(caps) <= {"flop", "turn", "river"} and tree["params"] == {}, "unsupported tree parameters")
    for street in ("flop", "turn", "river"): caps.setdefault(street, 2)
    doc.setdefault("rake", {"kind": "none"})
    doc.setdefault("utility", {"kind": "chip-ev"})
    require(doc["schema"] == "solvers.postflop/v1" and doc["rake"] == {"kind": "none"} and doc["utility"] == {"kind": "chip-ev"}, "wrong finite game economics")
    require(doc["run"]["threads"] == 8 and doc["run"]["target_nash_conv"] == 0.04 and doc["run"]["storage"] == "f32", "wrong fixed run conditions")
    return doc


def same_summary(a, b): return all(a[k] == b[k] for k in SUMMARY_FIELDS)


def verify():
    ev = Evidence()
    plan, report, ap, audits = map(ev.json, (PLAN, REPORT, AUDIT_PLAN, AUDITS))
    require(report["state"] == audits["state"] == "completed", "unfinished campaign")
    require(ev.key(report["plan"]) == ev.key(PLAN) and ev.key(ap["original_plan"]) == ev.key(PLAN)
            and ev.key(ap["original_comparison"]) == ev.key(REPORT) and ev.key(audits["plan"]) == ev.key(AUDIT_PLAN), "campaign crosslink mismatch")
    for item in (report["plan"], ap["original_plan"], ap["original_comparison"], audits["plan"], plan["runner"], plan["supervisor"], ap["audit_runner"]): ev.bound(item)
    require(ap["original_runner"] == plan["runner"] and ap["supervisor"] == plan["supervisor"]
            and ap["limits"] == plan["limits"] and ap["threads"] == 8, "audit fixed execution conditions changed")
    require(ap["pair_tolerance"]["absolute"] == 1e-10 and ap["pair_tolerance"]["relative"] == 1e-12, "audit numerical tolerance changed")
    require(plan["order"] == ["baseline", "candidate"]*3 and plan["repetitions_per_binary"] == 3, "unfrozen pair order")
    require([(x["case"], x["iterations"], x["target_nash_conv"]) for x in plan["cases"]] == [("river", 100, .04), ("turn", 100, .04), ("flop", 50, .04)], "fixed cases changed")
    host = plan["host"]
    require(report["host"]["boot_id"] == ap["host"]["boot_id"] == host["boot_id"] and host["logical_cpus"] == 8
            and "AMD EPYC 7B12" in host["cpuinfo"], "CPU/boot differs")
    build = ev.json("/opt/r1/comparison-build/identity-after.json")
    ev.bound(build["before_record"])
    require(build["state"] == "completed" and build["boot_id"] == host["boot_id"] and build["old_targets_reused"] is False, "comparison build boundary wrong")
    for item in build["inputs"]: ev.bound(item, runtime=True)
    require(plan["baseline"]["binary"] == build["binaries"]["baseline"]
            and plan["baseline"]["source_evidence"] == build["sources"]["baseline"], "baseline build differs")
    for version, fmt in (("baseline", 1), ("candidate", 3)):
        require(plan[version]["sol_version"] == fmt, "wrong artifact format")
        ev.bound(plan[version]["binary"]); ev.bound(plan[version]["source_evidence"])
    require(plan["candidate"]["source_evidence"]["sha256"] == "f241167a9c765b839cbe560ec66c9c490a0b0193d8f23ba8de768c65eaaf043c", "final performance must use source06")
    original_plan_path = PERF + "experiments/hu-postflop-r1/pipeline/frozen/vm06-v3/plan.json"
    original_plan = ev.json(original_plan_path)
    require(set(plan) == set(original_plan), "final plan schema differs")
    for field in set(plan) - {"candidate", "cases", "created_at"}:
        require(plan[field] == original_plan[field], "original plan condition changed: " + field)
    require(len(plan["cases"]) == len(original_plan["cases"]) == 3, "original case count differs")
    for current, original in zip(plan["cases"], original_plan["cases"]):
        current, original = copy.deepcopy(current), copy.deepcopy(original)
        current["config"].pop("path"); original["config"].pop("path")
        require(current == original, "original frozen input/quality condition changed")
    pilot = ev.json(plan["pilot"])
    require(pilot["baseline"] == plan["baseline"] and pilot["host"]["boot_id"] == host["boot_id"], "pilot provenance differs")
    conditions = {x["case"]: normalized_config(ev.raw(x["config"])) for x in plan["cases"]}
    for case, calibration in zip(plan["cases"], pilot["cases"]):
        require(calibration["case"] == case["case"] and calibration["summary"]["iterations"] == case["iterations"]
                and calibration["internal_target"]["status"] == "pass", "pilot fixed iteration mismatch")
        original = normalized_config(ev.raw(calibration["config"]))
        original["run"]["iterations"] = case["iterations"]
        require(original == conditions[case["case"]], "post-pilot quality/game conditions changed")
    expected_order = [(c, v, r) for c in ("river", "turn", "flop") for r in (1, 2, 3) for v in ("baseline", "candidate")]
    require([(r["case"], r["version"], r["repetition"]) for r in report["runs"]] == expected_order, "missing/duplicate/out-of-order performance run")
    rows = {}
    for row in report["runs"]:
        key = (row["case"], row["version"], row["repetition"]); rows[key] = row
        cfg = conditions[row["case"]]
        require(row["run_status"] == "completed" and row["expected_iterations"] == cfg["run"]["iterations"], "incomplete/incorrect solve")
        for item in row["artifacts"].values(): ev.bound(item)
        require(normalized_config(ev.raw(row["artifacts"]["run.toml"])) == cfg, "effective run config differs")
        record = ev.stage(row["solve"])
        binary = plan[row["version"]]["binary"]["path"]
        frozen_config = next(c["config"] for c in plan["cases"] if c["case"] == row["case"])
        run_dir = str(PurePosixPath(row["artifacts"]["run.json"]["path"]).parent)
        require(record["argv"] == [binary, "solve", frozen_config["path"], "--out", run_dir, "--sol-streets", "full"], "wrong solve command")
        summary_record = ev.stage(row["summary_read"])
        require(summary_record["argv"] == [binary, "export", row["artifacts"]["solution.sol"]["path"], "summary"], "wrong summary query")
        require(ev.json(row["summary_read"]["stdout"]) == row["summary"], "summary stdout mismatch")
        final = ev.json(row["artifacts"]["run.json"])
        summary = row["summary"]
        gains = [final["explP0"], final["explP1"]]
        require(all(math.isfinite(x) for x in [*gains, final["nashConv"], summary["ev_oop"], summary["ev_ip"]])
                and sum(gains) == final["nashConv"] == summary["nash_conv"] < .04
                and gains == [summary["expl_oop"], summary["expl_ip"]]
                and final["iterations"] == summary["iterations"] == cfg["run"]["iterations"], "presave gain/NC/iteration mismatch")
        require(row["live_final"] == {"g_i": gains, "iterations": final["iterations"], "nash_conv": final["nashConv"], "pot": cfg["game"]["pot"]}, "embedded live result differs")
        artifact = row["artifacts"]["solution.sol"]
        prefix = ev.prefixes[ev.bound(artifact)]
        header = {"version": struct.unpack_from('<H', prefix, 8)[0], "embedded_config_blake3": prefix[10:42].hex(), "iteration": struct.unpack_from('<Q', prefix, 42)[0]}
        require(prefix[:8] == b"SLVRSOLV" and header == artifact["header"] and header["version"] == plan[row["version"]]["sol_version"] and header["iteration"] == cfg["run"]["iterations"], "artifact header mismatch")
        for view, stage in row["profile"].items():
            exported = ev.stage(stage)
            require(exported["argv"] == [binary, "export", artifact["path"], view, "--node", "all"], "wrong profile query")
        require(set(row["profile"]) == {"tree", "strategy", "ev"}, "profile view incomplete")
        if key[2] == 1:
            resume = row["resume"]
            require(resume["status"] == "pass" and same_summary(resume["summary"], summary), "resume summary differs")
            resumed = ev.stage(resume["stage"])
            restore_dir = str(PurePosixPath(run_dir).parent / "restore/run")
            restore_sol = restore_dir + "/solution.sol"
            require(resumed["argv"] == [binary, "resume", run_dir, "--out", restore_dir], "wrong resume input/output")
            resumed_summary = ev.stage(resume["summary_read"])
            require(resumed_summary["argv"] == [binary, "export", restore_sol, "summary"], "wrong resumed summary input")
            require(set(resume["profile"]) == {"tree", "strategy", "ev"}, "resume profile incomplete")
            require(ev.json(resume["summary_read"]["stdout"]) == resume["summary"], "resume stdout differs")
            for view, stage in resume["profile"].items():
                resumed_export = ev.stage(stage)
                require(resumed_export["argv"] == [binary, "export", restore_sol, view, "--node", "all"], "wrong resumed profile query")
                require(stage["stdout"]["sha256"] == row["profile"][view]["stdout"]["sha256"], "resumed profile differs")
        else: require(row["resume"] is None, "unexpected resume repetition")
        require(all(value is None for value in row["phase_timings"].values()), "original phase nulls changed")
    # Stages must not overlap; builds and audit evaluation are outside timed comparison.
    perf_intervals = sorted((instant(r["started_at"]), instant(r["ended_at"])) for r in ev.stages.values())
    require(all(a[1] <= b[0] for a, b in zip(perf_intervals, perf_intervals[1:])), "performance stages overlap")
    require(instant(build["recorded_at_utc"]) < perf_intervals[0][0], "comparison ran before build completed")
    for case in conditions:
        for rep in (1, 2, 3):
            a, b = rows[(case, "baseline", rep)], rows[(case, "candidate", rep)]
            require(same_summary(a["summary"], b["summary"]) and a["live_final"] == b["live_final"], "paired presave profile differs")
            for view in ("tree", "strategy", "ev"):
                require(a["profile"][view]["stdout"]["sha256"] == b["profile"][view]["stdout"]["sha256"], "paired exported profile differs")
    # Re-run the existing source-scoped recovery verifier, never old stages 12/13.
    build_verifier_path = EXP / "saved-profile/vm06-build-verification.py"
    spec = importlib.util.spec_from_file_location("r1_local_build_verifier", build_verifier_path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    recovery = module.verify(SimpleNamespace(evidence=EXP / "saved-profile/evidence-vm06-build", bundle=REPO / "runs/r1-cloud/vm06-recovery.tar.gz", current_source=EXP / "validation/sources/current-06.tar.gz", baseline_source=REPO / "runs/r1-cloud/build-source-9632d8b.tar.gz"))
    require(recovery["status"] == "verified_completed" and recovery["boot_id"] == host["boot_id"], "audit build unverified or different boot")
    candidate_built = recovery["binaries"]["source06_solvers_not_paired_benchmark"]
    require(all(plan["candidate"]["binary"][k] == candidate_built[k] for k in ("path", "bytes", "sha256")), "source06 performance binary differs from verified recovery build")
    require(all(plan["candidate"]["source_evidence"][k] == recovery["source_archives"]["candidate_source06"][k] for k in ("bytes", "sha256")), "source06 source archive differs from recovery")
    require(instant(recovery["ended_utc"]) < perf_intervals[0][0], "performance started before recovery completed")
    prior_comparison = ev.json(PERF + "runs/r1-paired-v3/comparison.json")
    require(instant(prior_comparison["ended_at"]) < perf_intervals[0][0], "source03 comparison overlaps final campaign")
    for version, role in (("baseline", "baseline_example"), ("candidate", "current_example")):
        selected = ap["audit_versions"][version]
        require(all(selected["binary"][k] == recovery["binaries"][role][k] for k in ("path", "bytes", "sha256")), "wrong audit build role")
        require(selected["build_role"] == role and selected["format_version"] == plan[version]["sol_version"]
                and ev.key(selected["source_evidence"]) == ev.key("/opt/r1/audit-pair-recovery/result.json"), "wrong audit source/build role")
        ev.bound(selected["binary"]); ev.bound(selected["source_evidence"])
    require([(x["case"], x["version"], x["repetition"]) for x in ap["runs"]] == expected_order
            and [(x["case"], x["version"], x["repetition"]) for x in audits["runs"]] == expected_order, "audit run coverage/order mismatch")
    ev.stage(ap["b3sum"]["version_stage"])
    saved = {}
    for frozen, row in zip(ap["runs"], audits["runs"]):
        key = (row["case"], row["version"], row["repetition"])
        original, cfg = rows[key], conditions[key[0]]
        for name, artifact_name in (("artifact", "solution.sol"), ("run_config", "run.toml")):
            item = frozen[name]
            require(ev.key(item) == ev.key(original["artifacts"][artifact_name]), "audit input not original paired artifact")
            ev.bound(item)
            stage = frozen["hash_stages"][name]; record = ev.stage(stage)
            require(record["argv"] == [ap["b3sum"]["binary"]["path"], "--no-names", "--", item["path"]], "wrong independent hash command")
            require(ev.raw(stage["stdout"]).decode().strip() == item["blake3"] and re.fullmatch('[0-9a-f]{64}', item["blake3"]), "independent BLAKE3 record differs")
        require(frozen["effective_config"] == cfg and normalized_config(ev.raw(frozen["run_config"])) == cfg, "audit semantics differ")
        require(frozen["effective_config_sha256"] == sha(json.dumps(cfg, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()), "effective config hash differs")
        record = ev.stage(row["stage"])
        require(record["argv"] == [ap["audit_versions"][key[1]]["binary"]["path"], "--sol", frozen["artifact"]["path"], "--threads", "8"], "wrong audit invocation")
        audit = ev.json(row["stage"]["stdout"])
        require(audit == row["report"] and row["run_status"] == "completed", "audit report/stdout mismatch")
        art, calc = audit["artifact"], audit["recomputed"]
        require(art["path"] == frozen["artifact"]["path"] and art["blake3"] == frozen["artifact"]["blake3"]
                and art["bytes"] == frozen["artifact"]["bytes"] and art["config_blake3"] == frozen["run_config"]["blake3"]
                and art["format_version"] == plan[key[1]]["sol_version"] and art["mode"] == "full"
                and art["iterations"] == cfg["run"]["iterations"] and art["node_count"] == original["summary"]["nodes"]
                and art["stored_nodes"] == original["summary"]["stored_nodes"], "evaluated artifact identity/shape differs")
        require(audit["threads"] == 8 and audit["pot_chips"] == cfg["game"]["pot"] and audit["effective_stack_chips"] == cfg["game"]["effective_stack"]
                and audit["rake"] == cfg["rake"] and audit["utility"] == cfg["utility"] and audit["value_basis"] == "subgame_start_utility"
                and audit["ev_offset"] == [cfg["game"]["pot"]/2]*2 and calc["profile"] == "stored_quantized", "audit utility/profile differs")
        require(audit["pre_save_metadata"]["ev"] == [original["summary"]["ev_oop"], original["summary"]["ev_ip"]]
                and audit["pre_save_metadata"]["expl"] == original["live_final"]["g_i"]
                and audit["pre_save_metadata"]["nash_conv"] == original["live_final"]["nash_conv"], "audited presave metadata differs from original summary")
        require(all(math.isfinite(x) for field in ("ev", "br", "gains") for x in calc[field]) and math.isfinite(calc["nash_conv"]), "nonfinite audit values")
        require(calc["gains"] == [b-e for b,e in zip(calc["br"],calc["ev"])] and calc["nash_conv"] == sum(calc["gains"]) < .04, "saved policy fails arithmetic/quality target")
        require(row["validation"]["pre_save_metadata_used_for_quality"] is False and row["validation"]["quality_status"] == "pass", "audit substituted presave quality")
        saved[key] = calc
    cases = []
    for case in conditions:
        measurements = {}
        for name, extract in (
            ("solve_seconds", lambda r:r["solve"]["elapsed_seconds"]),
            ("root_native_peak_bytes", lambda r:r["solve"]["measurement"]["root_os_peak_resident_bytes"]),
            ("sampled_tree_peak_bytes", lambda r:r["solve"]["measurement"]["sampled_peak_tree_resident_bytes"]),
            ("summary_seconds", lambda r:r["summary_read"]["elapsed_seconds"]),
            ("summary_root_native_peak_bytes", lambda r:r["summary_read"]["measurement"]["root_os_peak_resident_bytes"]),
            ("summary_sampled_tree_peak_bytes", lambda r:r["summary_read"]["measurement"]["sampled_peak_tree_resident_bytes"]),
            ("sol_bytes", lambda r:r["artifacts"]["solution.sol"]["bytes"]),
            ("checkpoint_bytes", lambda r:r["artifacts"]["checkpoint.ckpt"]["bytes"])):
            by_version = {v:stats([extract(rows[(case,v,r)]) for r in (1,2,3)]) for v in ("baseline","candidate")}
            left,right=by_version["baseline"],by_version["candidate"]
            missed = name == "summary_sampled_tree_peak_bytes" and any(rows[(case,v,r)]["summary_read"]["measurement"]["max_observed_processes"] == 0 for v in ("baseline","candidate") for r in (1,2,3))
            paired=[100*(b/a-1) if a and not missed else None for a,b in zip(left["values"],right["values"])]
            measurements[name]={**by_version,"candidate_change_pct_of_medians":100*(right["median"]/left["median"]-1) if left["median"] and not missed else None,
                                "paired_change_pct":stats(paired) if all(x is not None for x in paired) else None}
            if name == "summary_sampled_tree_peak_bytes":
                measurements[name]["max_observed_processes"] = {v:[rows[(case,v,r)]["summary_read"]["measurement"]["max_observed_processes"] for r in (1,2,3)] for v in ("baseline","candidate")}
                measurements[name]["memory_change_interpretable"] = not missed
        pair_checks=[]
        for rep in (1,2,3):
            a,b=saved[(case,"baseline",rep)],saved[(case,"candidate",rep)]
            require(a==b,"saved-profile pair not exactly equal")
            pair_checks.append({"repetition":rep,"exported_tree_strategy_ev_exact":True,"saved_profile_exact":True,
                                "effective_config_equal":True,"raw_config_hash_equal":rows[(case,"baseline",rep)]["artifacts"]["run.toml"]["sha256"]==rows[(case,"candidate",rep)]["artifacts"]["run.toml"]["sha256"]})
        sample=saved[(case,"baseline",1)]
        cases.append({"case":case,"iterations":conditions[case]["run"]["iterations"],"target_nash_conv":.04,
                      "presave_nash_conv":rows[(case,"baseline",1)]["live_final"]["nash_conv"],"saved_profile":sample,
                      "saved_exploitability_pct_pot":sample["nash_conv"]/(2*conditions[case]["game"]["pot"])*100,
                      "pairs":pair_checks,"measurements":measurements})
    require(perf_intervals[-1][1] < instant(audits["started_at"]), "audit work overlapped timed campaign")
    return {"schema":"r1.vm06-current-crossbundle-verification/v1","status":"verified_scoped_comparison",
            "assessment":"share_with_caveats","verifier":identity(Path(__file__)),"bundles":ev.bundles,
            "evidence_roots":{name:{"path":path,"bytes":ev.key(path)[1],"sha256":ev.key(path)[2]} for name,path in (("plan",PLAN),("comparison",REPORT),("audit_plan",AUDIT_PLAN),("audits",AUDITS))},
            "resolved_required_reference_uses":ev.links,"verified_supervisor_stages":len(ev.stages),
            "recorded_runtime_identities_without_local_payload":list(ev.unavailable_runtime.values()),
            "source_scope":{"performance_baseline":plan["baseline"],"performance_candidate06":plan["candidate"],
                            "quality_auditor_candidate06":recovery["source_archives"]["candidate_source06"],"audit_build_verifier":identity(build_verifier_path),
                            "audit_build_status":recovery["status"],"excluded_historical_stages":recovery["historical_stage_success_excluded"]},
            "host":{"hostname":host["hostname"],"boot_id":host["boot_id"],"logical_cpus":8,"cpu":build["cpu"]},
            "comparison_period_utc":{"start":report["created_at"],"end":report["ended_at"]},
            "audit_period_utc":{"start":audits["started_at"],"end":audits["ended_at"]},
            "coverage":{"original_solves":18,"exact_export_pairs":9,"resume_checks":6,"saved_profile_audits":18,"exact_saved_profile_pairs":9},
            "original_source03_plan_unchanged_conditions":True,"original_source03_plan":{"path":original_plan_path,"bytes":ev.key(original_plan_path)[1],"sha256":ev.key(original_plan_path)[2]},
            "conditions":conditions,"cases":cases,"formal_external_24_case_acceptance":"not_evaluated",
            "limitations":["Three repetitions per binary on one AMD boot; fixed baseline/candidate alternating order, warm/uncontrolled caches; no population-wide or significance claim.",
                           "root_native_peak is wait4.ru_maxrss, which can include inherited pre-exec/descendant high-water marks; approximately 25 MB floor makes small-process memory deltas unreliable.",
                           "sampled_tree_peak is a sampled lower bound; zero with no observed processes means missed lifetime, never zero RAM.",
                           "Original phase timings remain null. Solve wall time includes setup, CFR, checks and persistence; no phase estimate is substituted.",
                           "Resume covers completed iteration caps, with no further CFR iterations; interrupted-run continuation is outside this evidence.",
                           "BLAKE3 inputs were independently hashed by pinned supervised b3sum on the VM; this offline verifier rehashes SHA-256 bytes and verifies those command/input/output links, without executing b3sum.",
                           "Generic retention.ready remains false. This scoped crossbundle check resolves required experiment links only; old damaged build stages 12/13 are excluded, recorded OS/compiler payload gaps remain explicit.",
                           "This report validates three synthetic Full/F32/chip-EV fixtures, not the 24 external-reference acceptance cases."]}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out",type=Path)
    args=parser.parse_args()
    try:
        report=verify();encoded=json.dumps(report,indent=2,sort_keys=True,allow_nan=False)+"\n"
        if args.out:
            require(not any(args.out.resolve().is_relative_to((EXP/folder).resolve()) for folder in BUNDLES),"report must stay outside immutable evidence")
            with args.out.open('x',encoding='utf-8',newline='\n') as stream:stream.write(encoded)
            print(json.dumps({"status":report["status"],"coverage":report["coverage"],"report":str(args.out)}))
        else:print(encoded,end='')
        return 0
    except (OSError,ValueError,KeyError,TypeError,tarfile.TarError) as error:
        print(f"VM06 current comparison verification failed: {type(error).__name__}: {error}",file=sys.stderr);return 2


if __name__=='__main__':raise SystemExit(main())
