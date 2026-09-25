#!/usr/bin/env python3
"""Offline VM07/002 evidence join. No solver, retained-code execution, or extraction.

Rehash the three pinned archives, their exact members, and immutable compact copies.
Join build/source/binaries to diagnostic and saved-profile records. --write creates
the three sibling reports; --check compares them. Unretained host executables and
unverified reference semantics are explicit boundaries, never imputed successes.
"""
from __future__ import annotations

import argparse
import copy
import datetime as dt
from decimal import Decimal
import hashlib
import io
import json
import math
from pathlib import Path, PurePosixPath
import re
import tarfile
import tomllib
import unittest

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
BUNDLES = {
    "checks": ("92ec7553e89a2b822622395f902f16ab2cd65e67fdfcfc688a3f7cb6edfdf6fb", 25),
    "build": ("7c754fd93e2b503faa7d3fbbf0fbe6df039156404af1b2928ccd22589bec70ac", 617),
    "diagnostic002": ("86d83fb54a47d383dcfa09a008a3c067a09517bbcc0de3f5294511e1190999f8", 103),
}
SOURCE_SHA = "a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a"
BASELINE_SHA = "3de4afef13082dca9e17c38c9cc5f5d1734855f017d812ecb04e9cdbe0f7da27"
INPUT_SHA = "1a03ea69897c995cc614d763767b3f4f95e0173c03bd52cdebfe6c48e8a11a19"
OBSERVED_SHA = "0f12b8aebaf833032332e2815868a4c3db8405edce64f32a8ae3ca3e1cd7b4df"
VM = "/opt/r1/diagnostic-002-vm07/"
AUDIT = "/opt/r1/diagnostic-002-saved-audit07/"
BUILD = "/opt/r1/codec-build07/"
INPUT = "/opt/r1/diagnostic-inputs07/"
BUILD_NAMES = ["toolchain", "fmt", "clippy", "workspace-tests", "python-tools", "release-cli",
               "release-codec-current", "release-codec-baseline"]
STAGES = ["input_check", "config_validate", "solve", "export_tree", "export_summary", "compare"]
HOST_ONLY = {"/usr/bin/bash", "/usr/bin/python3.12",
             "/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/cargo"}


def require(value, message):
    if not value:
        raise ValueError(message)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def document(raw):
    def pairs(items):
        out = {}
        for key, value in items:
            require(key not in out, "duplicate JSON key")
            out[key] = value
        return out
    def invalid(value):
        raise ValueError("nonfinite JSON constant: " + value)
    require(b"\0" not in raw, "NUL in adopted JSON")
    return json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid)


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode()


def instant(value):
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(parsed.tzinfo is not None, "timestamp lacks timezone")
    return parsed


def match(raw, item):
    require(type(item["bytes"]) is int and len(raw) == item["bytes"] and sha(raw) == item["sha256"],
            "identity mismatch: " + item.get("path", item.get("original_path", "payload")))
    return raw


def safe_name(value):
    path = PurePosixPath(value)
    require(not path.is_absolute() and ".." not in path.parts and "\\" not in value
            and value == path.as_posix() and bool(path.parts), "unsafe archive/compact path")
    return path


def source_files(raw):
    out = {}
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        for member in archive:
            safe_name(member.name)
            if member.isdir():
                continue
            require(member.isfile() and member.name not in out and member.size <= 32 * 1024**2,
                    "invalid source member")
            out[member.name] = archive.extractfile(member).read()
    require(sum(map(len, out.values())) <= 256 * 1024**2, "source exceeds bound")
    return out


class Evidence:
    def __init__(self, cloud):
        self.payloads, self.locations, self.bundles, self.unavailable = {}, {}, [], {}
        for name, (digest, count) in BUNDLES.items():
            path = cloud / ("evidence-vm07-" + name + ".tar.gz")
            raw = path.read_bytes()
            require(sha(raw) == digest, "wrong pinned bundle: " + name)
            sidecar = Path(str(path) + ".manifest.json").read_bytes()
            manifest = document(sidecar)
            require(manifest["schema"] == "solvers.r1-retention/v1", "collector schema differs")
            rows = {r["archive_member"]: r for r in manifest["files"]}
            require(len(rows) == len(manifest["files"]) == count, "collector inventory differs")
            require(Path(str(path) + ".sha256").read_text().strip() == digest + "  " + manifest["archive_filename"],
                    "checksum sidecar differs")
            seen = set()
            with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
                for member in archive:
                    require(member.isfile() and member.name not in seen and member.size <= 64 * 1024**2,
                            "nonregular/duplicate/oversized bundle member")
                    seen.add(member.name)
                    data = archive.extractfile(member).read()
                    if member.name == "retention-manifest.json":
                        require(data == sidecar, "inner/outer manifest differs")
                        continue
                    require(re.fullmatch(r"files/\d{8}", member.name) and member.name in rows,
                            "unexpected collector member")
                    row = rows[member.name]
                    require(row["included"] is True and row["kind"] == "regular", "unretained payload")
                    match(data, row)
                    original = row["original_path"]
                    require(original not in self.payloads or self.payloads[original] == data,
                            "cross-bundle original-path collision")
                    self.payloads[original] = data
                    self.locations.setdefault(original, []).append({"bundle": name, "member": member.name})
            require(seen == set(rows) | {"retention-manifest.json"}, "archive member set differs")
            self.bundles.append({"name": name, "path": str(path.relative_to(REPO)), "sha256": digest,
                                 "bytes": len(raw), "payload_count": count, "all_payloads_verified": True,
                                 "manifest_sha256": sha(sidecar)})

    def get(self, path):
        require(path in self.payloads, "required retained evidence missing: " + path)
        return self.payloads[path]

    def bound(self, item, *, host_optional=False):
        if item["path"] not in self.payloads:
            require(host_optional and item["path"] in HOST_ONLY, "unresolved required link: " + item["path"])
            require(item["path"] not in self.unavailable or self.unavailable[item["path"]] == item,
                    "host executable identity changed")
            self.unavailable[item["path"]] = item
            return None
        return match(self.get(item["path"]), item)

    def doc(self, path):
        return document(self.get(path))

    def compact(self, directory):
        index = document((directory / "compact-index.json").read_bytes())
        indexed = set()
        for row in index["files"]:
            safe_name(row["path"])
            require(row["path"] not in indexed, "duplicate compact path")
            indexed.add(row["path"])
            match((directory / row["path"]).read_bytes(), row)
        actual = {p.relative_to(directory).as_posix() for p in directory.rglob("*") if p.is_file()}
        require(actual == indexed | {"compact-index.json", "retention.json", "validation.json"},
                "compact file set differs")
        retention = document((directory / "retention.json").read_bytes())
        for row in retention["files"]:
            raw = match(self.get(row["original_path"]), row)
            if row["compact_path"]:
                require((directory / row["compact_path"]).read_bytes() == raw, "compact/archive bytes differ")
        return {"path": str(directory.relative_to(REPO)), "compact_files": len(indexed),
                "original_generic_readiness": document((directory / "validation.json").read_bytes())["ready"],
                "compact_index_sha256": sha((directory / "compact-index.json").read_bytes())}


def completed(record):
    require(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
            and type(record["child_exit_code"]) is int and record["child_exit_code"] == 0
            and type(record["supervisor_exit_code"]) is int and record["supervisor_exit_code"] == 0
            and record["stop_reason"] == "completed" and record["cleanup_complete"] is True
            and record["identity_unchanged"] is True and record["forced"] is False
            and record["shell"] is False and not record["errors"] and not record["events"],
            "unsuccessful/interrupted/forced supervised stage")
    require(record["identity_before"] and record["identity_before"] == record["identity_after"],
            "supervised identities changed/missing")


def stage(evidence, path, argv, timeout, disk_gib, poll):
    record = evidence.doc(path)
    completed(record)
    require(record["argv"] == argv, "stage command differs")
    require(record["resolved_argv"][1:] == argv[1:], "resolved command arguments differ")
    require(record["resolved_argv"][0] == record["identity_before"][0]["path"], "unbound executable")
    limits = record["limits"]
    require(limits == {"timeout_seconds": timeout, "grace_seconds": 5, "kill_wait_seconds": 5,
                       "poll_seconds": poll, "memory_limit_bytes": 40 * 1024**3,
                       "min_free_memory_bytes": 8 * 1024**3, "disk_reserve_bytes": disk_gib * 1024**3},
            "stage resource/grace limits differ")
    a, b = instant(record["started_at"]), instant(record["ended_at"])
    require(a <= b and 0 <= record["elapsed_seconds"] <= timeout + 10, "invalid stage interval")
    for item in record["identity_before"]:
        evidence.bound(item, host_optional=True)
    for item in record["outputs"].values():
        evidence.bound(item)
    samples = [document(line) for line in evidence.bound(record["outputs"]["samples"]).splitlines() if line.strip()]
    require(len(samples) == record["measurement"]["sample_count"] > 0, "resource sample count differs")
    require(samples[-1] == record["last_sample"] and samples[-1]["pids"] == [], "containment not empty at end")
    elapsed = [row["elapsed_seconds"] for row in samples]
    require(elapsed == sorted(elapsed) and all(math.isfinite(x) and 0 <= x <= record["elapsed_seconds"] for x in elapsed),
            "nonmonotonic resource samples")
    require(all(a <= instant(row["at"]) <= b for row in samples), "sample timestamp outside stage")
    return record


def tree_check(rows, observed):
    require(observed["case_id"] == "HU-R0-002" and observed["unobserved_menu_frontier"] == [], "reference graph not closed")
    expected, nonterminal, terminal, edges = {}, [], 0, 0
    def action(value):
        if value in ("check", "fold", "call"):
            return {"check": "x", "fold": "f", "call": "c"}[value]
        match_action = re.fullmatch(r"(?:bet|raise(?: to)?|allin) (\d+(?:\.\d+)?)", value)
        require(match_action is not None, "unknown observed monetary action")
        amount = Decimal(match_action[1]) * 100
        require(amount == int(amount), "fractional chip action")
        return "r" + str(int(amount))
    for menu in observed["observed_menus"]:
        history, contributions, actor, increment = "", [0, 0], 0, 100
        for part in menu["history"].split("/") if menu["history"] else []:
            token = action(part.strip())
            require(token == "x" or token.startswith("r"), "terminal in decision history")
            if token.startswith("r"):
                target = int(token[1:])
                delta = target - max(contributions)
                require(delta >= increment or target == 9750, "illegal history full raise")
                increment = max(increment, delta)
                contributions[actor] = target
            history += token
            actor = 1 - actor
        source_tokens = []
        for token in menu["source_history"].split("-") if menu["source_history"] else []:
            source_tokens.append("x" if token == "X" else "r" + str(int(Decimal("97.5" if token == "RAI" else token[1:]) * 100)))
        require("".join(source_tokens) == history, "human/source history mismatch")
        require(menu["actor"] == ("BB", "BTN")[actor] and Decimal(str(menu["remaining_stack_bb_displayed"])) * 100 == 9750 - contributions[actor],
                "observed actor/remaining stack mismatch")
        facing = contributions[1-actor] > contributions[actor]
        labels = []
        for value in menu["actions"]:
            token = action(value)
            if token.startswith("r"):
                target = int(token[1:])
                require(max(contributions) < target <= 9750 and (target-max(contributions) >= increment or target == 9750),
                        "invalid observed raise increment")
                labels.append(("raise to " if facing else "bet ") + str(target))
            else:
                labels.append(value)
            child = history + token
            if token in ("f", "c") or (history == "x" and token == "x"):
                terminal += 1
            else:
                nonterminal.append(child)
            edges += 1
        require(history not in expected and len(labels) == len(set(labels)), "duplicate history/action")
        expected[history] = {"history": history, "street": "river", "actor": ("oop", "ip")[actor],
                             "pot": 550 + sum(contributions), "actions": labels, "stored": True}
    require(len(rows) == len(expected) == 132 and len({r["history"] for r in rows}) == 132, "wrong decision coverage")
    require({r["history"]: r for r in rows} == expected, "actual tree differs from observed menus/states")
    require(sorted(nonterminal) == sorted(set(expected)-{""}) and terminal == 261 and edges == 392,
            "graph closure/count mismatch")
    return {"decision_nodes": 132, "terminal_nodes": terminal, "public_nodes": 393,
            "action_edges": edges, "unobserved_frontier": 0, "all_ordered_menus_actors_pots_stacks_match": True}


def same_config(a, b):
    a, b = copy.deepcopy(a), copy.deepcopy(b)
    for config in (a, b):
        config["game"]["tree"].setdefault("params", {})
        for key, value in (("alpha", 1.5), ("beta", 0.0), ("gamma", 3.0), ("pow4_reset", True)):
            config["algorithm"].setdefault(key, value)
        config["run"].setdefault("par_min_children", 12)
    def equivalent(left, right):
        if isinstance(left, bool) or isinstance(right, bool):
            return type(left) is type(right) and left == right
        if isinstance(left, dict) and isinstance(right, dict):
            return left.keys() == right.keys() and all(equivalent(left[k], right[k]) for k in left)
        if isinstance(left, list) and isinstance(right, list):
            return len(left) == len(right) and all(equivalent(x, y) for x, y in zip(left, right))
        return left == right
    require(equivalent(a, b), "raw/normalized config semantic mismatch")


def profile_check(saved, summary):
    require(saved["schema"] == "solvers.research.hu-saved-profile-audit/v1"
            and saved["recomputed"]["profile"] == "stored_quantized", "wrong saved evaluation domain")
    require(saved["artifact"]["mode"] == "full" and saved["artifact"]["format_version"] == 3
            and saved["artifact"]["source_storage"] == "f32"
            and saved["artifact"]["node_count"] == summary["nodes"] == 393
            and saved["artifact"]["stored_nodes"] == summary["stored_nodes"] == 132
            and saved["artifact"]["iterations"] == summary["iterations"] == 1000, "incomplete/wrong saved profile")
    require(saved["threads"] == 1 and saved["par_chance_depth"] == 0 and saved["par_min_children"] == 12
            and saved["pot_chips"] == 550 and saved["effective_stack_chips"] == 9750
            and saved["value_basis"] == "subgame_start_utility" and saved["ev_offset"] == [275.0, 275.0]
            and saved["zero_sum_terminal_utility"] is False, "wrong saved evaluation setup")
    pre = saved["pre_save_metadata"]
    require(pre == {"iterations": 1000, "expl": [summary["expl_oop"], summary["expl_ip"]],
                    "ev": [summary["ev_oop"], summary["ev_ip"]], "nash_conv": summary["nash_conv"],
                    "storage": "f32", "wall_secs": summary["wall_secs"]}, "live metadata differs from saved header")
    recomputed = saved["recomputed"]
    for key in ("ev", "br", "gains"):
        require(len(recomputed[key]) == 2 and all(type(x) in (int, float) and math.isfinite(x) for x in recomputed[key]), "nonfinite saved metric")
    tolerances = [1e-10 + 1e-12 * max(abs(b), abs(e)) for b, e in zip(recomputed["br"], recomputed["ev"])]
    for e, b, gain, tolerance in zip(recomputed["ev"], recomputed["br"], recomputed["gains"], tolerances):
        require(abs((b-e)-gain) <= tolerance and gain >= -tolerance, "invalid saved BR gain")
    require(recomputed["nash_conv"] == sum(recomputed["gains"])
            and summary["nash_conv"] == summary["expl_oop"] + summary["expl_ip"], "NashConv sum mismatch")
    return {"saved_minus_live_ev_chips": [a-b for a, b in zip(recomputed["ev"], pre["ev"])],
            "saved_minus_live_nash_conv_chips": recomputed["nash_conv"]-pre["nash_conv"],
            "numerical_check": "finite values, both signed BR gain identities, and exact f64 gain sum; no quality threshold"}


def verify(cloud):
    evidence = Evidence(cloud)
    compacts = [evidence.compact(HERE / "evidence-vm07-002"),
                evidence.compact(HERE.parent / "validation/vm07-complete")]
    build = evidence.doc(BUILD + "result.json")
    require(build["schema"] == "r1.codec-build/v1" and build["status"] == "passed"
            and build["planned_stages"] == BUILD_NAMES and [x["name"] for x in build["stages"]] == BUILD_NAMES,
            "source07 build/check stages incomplete")
    require(build["fresh_targets"] == ["/opt/r1/target/codec-current", "/opt/r1/target/codec-baseline"]
            and build["build_environment"]["RUSTUP_TOOLCHAIN"] == "1.97.0", "wrong build target/toolchain")
    start, end = instant(build["started_utc"]), instant(build["ended_utc"])
    require(start < end and (end-start).total_seconds() <= build["dispatch_seconds"] == 7200, "build deadline exceeded")
    timeouts = [30, 60, 1800, 1800, 300, 1800, 1200, 1800]
    stage_reports, last_end = [], start
    for index, (entry, timeout) in enumerate(zip(build["stages"], timeouts)):
        require(entry["status"] == "passed" and entry["exit_code"] == 0, "failed build stage")
        path = BUILD + f"{index:02d}-{entry['name']}/supervisor.json"
        record = stage(evidence, path, entry["argv"], timeout, 12, 0.25)
        require(record["cwd"] == entry["cwd"] and last_end <= instant(record["started_at"])
                <= instant(record["ended_at"]) <= end, "build stage sequence/cwd differs")
        last_end = instant(record["ended_at"])
        stage_reports.append({"phase": "build", "name": entry["name"], "record_path": path,
                              "record_sha256": sha(evidence.get(path)), "elapsed_seconds": record["elapsed_seconds"],
                              "sample_count": record["measurement"]["sample_count"], "verified_completion": True})
    for key in ("supervisor", "driver"):
        evidence.bound(build["identities"][key])
    source_manifests = {}
    for role, root, archive_path, digest in (
        ("current", "/opt/r1/current/", "/opt/r1/source-07.tar.gz", SOURCE_SHA),
        ("baseline", "/opt/r1/baseline-codec/", "/opt/r1/codec-baseline-source.tar.gz", BASELINE_SHA),
    ):
        archive = evidence.get(archive_path)
        require(sha(archive) == digest, "wrong source archive")
        files = source_files(archive)
        if role == "baseline":
            files["crates/formats/examples/sol_codec_bench.rs"] = evidence.get("/opt/r1/current/crates/formats/examples/sol_codec_bench.rs")
        expected_names = {n for n in files if not set(PurePosixPath(n).parts).intersection({".git", "target", "runs", ".cache", "__pycache__"})}
        refs = build["identities"][role]
        require(len(refs) == len({item["path"] for item in refs}) == len(expected_names), "source manifest coverage differs")
        require({item["path"] for item in refs} == {root+n for n in expected_names}, "source manifest names differ")
        for item in refs:
            require(evidence.bound(item) == files[item["path"].removeprefix(root)], "build source differs from source archive")
        source_manifests[role] = {"source_archive_sha256": digest, "verified_source_files": len(refs),
                                  "baseline_only_addition": "identical research example" if role == "baseline" else None}
    binaries = {row["path"]: row for row in build["binaries"]}
    require(len(binaries) == 4, "build binary inventory differs")
    for item in binaries.values():
        evidence.bound(item)
    binding, execution = evidence.doc(AUDIT + "binding.json"), evidence.doc(VM + "execution.json")
    for item in binding.values():
        evidence.bound(item)
    require(binding["build_record"]["path"] == BUILD + "result.json"
            and binding["source_archive"]["sha256"] == SOURCE_SHA
            and binding["input_archive"]["sha256"] == INPUT_SHA
            and binding["binary"] == execution["binary"] == binaries[binding["binary"]["path"]]
            and binding["audit_binary"] == binaries[binding["audit_binary"]["path"]], "diagnostic/build binding differs")
    require(execution["schema"] == "solvers.r1-diagnostic-execution/v1" and execution["case_id"] == "HU-R0-002"
            and execution["state"] == "completed" and execution["source_id"] == "source-07:" + SOURCE_SHA
            and [x["name"] for x in execution["stages"]] == STAGES, "diagnostic incomplete/wrong source")
    for key in ("runner", "supervisor", "binary", "input_archive"):
        evidence.bound(execution[key])
    require(binding["runner"] == execution["runner"] and binding["input_archive"] == execution["input_archive"], "launcher/runner binding differs")
    inputs = source_files(evidence.bound(execution["input_archive"]))
    require(len(inputs) == len(execution["inputs"]) == 51
            and set(inputs) == {"HU-R0-002/"+name for name in execution["inputs"]}, "input archive coverage differs")
    for name, ref in execution["inputs"].items():
        raw = evidence.bound(ref)
        require(ref["path"] == INPUT+name and ref["archive_member"] == "HU-R0-002/"+name
                and raw == inputs[ref["archive_member"]], "archive/extracted input mismatch")
    for artifact in execution["artifacts"].values():
        require(artifact["status"] == "present", "missing diagnostic artifact")
        evidence.bound(artifact)
    run_start, run_end = instant(execution["started_utc"]), instant(execution["finished_utc"])
    require(end < run_start < run_end and execution["seconds"] <= execution["total_seconds_limit"] == 1800,
            "diagnostic order/deadline differs")
    last_end = run_start
    for entry in execution["stages"]:
        path = VM + "stages/" + entry["name"] + "/supervisor.json"
        require(entry["supervisor_record"]["path"] == path, "diagnostic stage path differs")
        evidence.bound(entry["supervisor_record"])
        record = stage(evidence, path, entry["argv"], 600, 10, 0.1)
        require(record["cwd"] == "/opt/r1/current" and last_end <= instant(record["started_at"])
                <= instant(record["ended_at"]) <= run_end, "diagnostic stage sequence/cwd differs")
        for key in ("state", "child_exit_code", "supervisor_exit_code", "stop_reason", "cleanup_complete",
                    "identity_unchanged", "elapsed_seconds", "measurement", "outputs"):
            require(entry[key] == record[key], "diagnostic stage summary differs: " + key)
        paths = {item["path"] for item in record["identity_before"]}
        require({execution["runner"]["path"], execution["supervisor"]["path"], execution["input_archive"]["path"],
                 *(item["path"] for item in execution["inputs"].values())}.issubset(paths), "diagnostic frozen inputs missing")
        last_end = instant(record["ended_at"])
        stage_reports.append({"phase": "diagnostic", "name": entry["name"], "record_path": path,
                              "record_sha256": sha(evidence.get(path)), "elapsed_seconds": record["elapsed_seconds"],
                              "sample_count": record["measurement"]["sample_count"], "verified_completion": True})
    sol = execution["artifacts"]["run/solution.sol"]
    sol_raw = evidence.bound(sol)
    require(sol_raw.startswith(b"SLVRSOLV\x03\x00"), "wrong SOL header")
    audit_argv = [binding["audit_binary"]["path"], "--sol", sol["path"], "--threads", "1"]
    record = stage(evidence, AUDIT + "supervisor.json", audit_argv, 600, 10, 0.25)
    require(run_end < instant(record["started_at"]) and record["cwd"] == "/opt/r1/current", "saved audit ordering/cwd differs")
    sol_identity = {k: sol[k] for k in ("path", "sha256", "bytes")}
    require(sol_identity in record["identity_before"] and sol_identity in record["identity_after"], "SOL not unchanged across saved audit")
    require(binding["audit_binary"] == record["identity_before"][0], "saved audit executable differs")
    saved = document(evidence.bound(record["outputs"]["stdout"]))
    require(saved["artifact"]["path"] == sol["path"] and saved["artifact"]["bytes"] == sol["bytes"], "audit artifact identity differs")
    checkpoint = evidence.bound(execution["artifacts"]["run/checkpoint.ckpt"])
    require(checkpoint[:8] == b"SLVRCKPT" and checkpoint[10:42] == sol_raw[10:42]
            and sol_raw[10:42].hex() == saved["artifact"]["config_blake3"]
            and int.from_bytes(sol_raw[42:50], "little") == int.from_bytes(checkpoint[42:50], "little") == 1000,
            "SOL/checkpoint/audit configuration header or iteration mismatch")
    stage_reports.append({"phase": "saved-profile", "name": "full-profile-audit", "record_path": AUDIT+"supervisor.json",
                          "record_sha256": sha(evidence.get(AUDIT+"supervisor.json")), "elapsed_seconds": record["elapsed_seconds"],
                          "sample_count": record["measurement"]["sample_count"], "verified_completion": True})
    observed = evidence.doc(INPUT + "observed.json")
    require(sha(evidence.get(INPUT+"observed.json")) == OBSERVED_SHA
            and evidence.get(INPUT+"observed.json") == (HERE/"HU-R0-002/observed.json").read_bytes(), "observations changed")
    topology = tree_check(evidence.doc(VM + "tree.json"), observed)
    config = tomllib.loads(evidence.get(INPUT + "diagnostic.toml").decode())
    normalized = tomllib.loads(evidence.get(VM + "run/run.toml").decode())
    same_config(config, normalized)
    require(config["game"]["pot"] == 550 and config["game"]["effective_stack"] == 9750
            and config["game"]["min_bet"] == 100 and config["game"]["iso_merging"] is False
            and config["run"]["iterations"] == 1000 and config["run"]["max_time"] == "30s"
            and config["run"].get("target_nash_conv") is None, "config scope/stop rules changed")
    require(saved["rake"] == config["rake"] and saved["utility"] == config["utility"], "saved utility/config mismatch")
    ranges, masses = {}, []
    for seat, count in (("oop", 493), ("ip", 479)):
        raw = evidence.get(INPUT + seat + "-range.txt")
        require(raw == (HERE/"HU-R0-002"/(seat+"-range.txt")).read_bytes()
                and config["game"][seat+"_range"] == raw.decode().strip(), "raw range changed")
        pairs = {}
        for item in raw.decode().strip().split(","):
            hand, weight = item.split(":")
            hand, weight = hand.strip(), Decimal(weight.strip())
            require(re.fullmatch(r"[2-9TJQKA][cdhs][2-9TJQKA][cdhs]", hand), "invalid range combo")
            cards = frozenset((hand[:2], hand[2:]))
            require(len(cards) == 2 and not cards.intersection(observed["board"]) and cards not in pairs and 0 < weight <= 1, "duplicate/blocked range combo")
            pairs[cards] = weight
        require(len(pairs) == count, "range count differs")
        masses.append(pairs)
        ranges[seat] = {"positive_combos": count, "weight_sum": str(sum(pairs.values())), "sha256": sha(raw)}
    joint = sum((a*b for h, a in masses[0].items() for k, b in masses[1].items() if not h.intersection(k)), Decimal(0))
    require(joint == Decimal("9361.53759450292671"), "compatible joint mass differs")
    summary = evidence.doc(VM + "summary.json")
    numerical = profile_check(saved, summary)
    comparison = evidence.doc(VM + "diagnostic-comparison.json")
    require(comparison["condition_match"] == "unverified" and comparison["quality_status"] == "not_evaluated"
            and comparison["acceptance"] is None and comparison["ev_diagnostic"]["comparison_threshold"] is None,
            "diagnostic overclaims reference quality")
    require(comparison["tree_check"] == {"status": "all_observed_menus_match", "decision_nodes": 132,
                                         "decision_action_edges": 392, "terminal_nodes": 261, "public_nodes": 393}, "recorded tree result differs")
    for kind in ("tree", "summary"):
        require(comparison["exports_sha256"][kind] == sha(evidence.get(VM+kind+".json")), "comparison exports unbound")
    for seat in ("oop", "ip"):
        value = Decimal(str(summary["ev_"+seat]))/100
        reference = Decimal(str(observed["root_ui"][seat+"_ev_bb"]))
        require(comparison["ev_diagnostic"]["root_ev"][seat] == {
            "solver_ev_bb": str(value), "reference_display_ev_bb": str(reference),
            "signed_difference_bb": str(value-reference)}, "reference EV difference arithmetic differs")
    progress = [document(line) for line in evidence.get(VM+"run/progress.jsonl").splitlines() if line.strip()]
    require([r["iteration"] for r in progress] == list(range(10, 1001, 10)), "progress/iteration completion differs")
    require(progress[-1]["nash_conv"] == summary["nash_conv"] and summary["wall_secs"] < 30, "final progress/stop mismatch")
    run = evidence.doc(VM+"run/run.json")
    require(run["iterations"] == 1000 and run["nashConv"] == summary["nash_conv"] and run["wallSecs"] == summary["wall_secs"], "final run metadata differs")
    toolchain = evidence.get(BUILD+"00-toolchain/stdout.log").decode()
    require("rustc 1.97.0" in toolchain and "AMD EPYC 7B12" in toolchain, "unexpected retained build CPU/compiler")
    verification = {"schema": "r1.vm07-002-verification/v1", "specific_evidence_verification": "verified",
                    "scope": "Pinned archives, compact copies, source manifests, 8 build + 6 diagnostic + 1 saved-profile stages; no new solve",
                    "bundles": evidence.bundles, "compact_copies": compacts, "source_manifests": source_manifests,
                    "source_id": execution["source_id"], "build_boot_id": build["boot_id"],
                    "build_cpu": "AMD EPYC 7B12", "compiler_version": "Rust 1.97.0",
                    "boot_binding_limit": "Pinned launcher checked equality to build boot before run; no separate per-stage boot observation",
                    "binaries": {key: binding[key] for key in ("binary", "audit_binary")}, "stages": stage_reports,
                    "inputs": {"archive_sha256": INPUT_SHA, "observed_sha256": OBSERVED_SHA, "ranges": ranges,
                               "compatible_joint_mass": str(joint), "original_config_sha256": sha(evidence.get(INPUT+"diagnostic.toml")),
                               "normalized_config_sha256": sha(evidence.get(VM+"run/run.toml")), "semantic_config_match": True},
                    "actual_tree": topology, "saved_profile_numerical_checks": numerical,
                    "solution": {**sol_identity, "locations": evidence.locations[sol["path"]], "unchanged_across_saved_audit": True,
                                 "availability": "SOL bytes also in immutable compact evidence; large binaries only in ignored local bundles"},
                    "complete_host_binary_retention": not evidence.unavailable,
                    "unretained_host_executables": sorted(evidence.unavailable.values(), key=lambda x: x["path"]),
                    "unretained_boundary": "Within these three archives, before/after hashes agree but host executable bytes are absent; not independently rehashed here. Root reports a later codec bundle retains these bytes; that bundle is outside this verifier's scope.",
                    "outer_cgroup_limit": "Launcher documents systemd bounds; these bundles independently prove inner supervisor state/grace/cleanup only",
                    "artifact_header_binding": {"config_blake3": sol_raw[10:42].hex(), "iteration": 1000,
                                                "SOL_checkpoint_saved_audit_agree": True,
                                                "limit": "Header BLAKE3 values agree; this stdlib verifier does not recompute BLAKE3 or decompress/re-evaluate SOL frames"},
                    "audit_limit": "Saved metrics are from the bound source07 executable; Python checks records and arithmetic, not a new EV/BR solve or independent scalar oracle",
                    "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None}
    report = {"schema": "r1.vm07-002-report/v1", "case_id": "HU-R0-002", "source_id": execution["source_id"],
              "execution": {"diagnostic_started_utc": execution["started_utc"], "diagnostic_finished_utc": execution["finished_utc"],
                            "saved_audit_finished_utc": record["ended_at"], "iterations": 1000,
                            "stop_basis": "iteration budget reached; internal 30s limit not reached; no quality target configured",
                            "solve_internal_wall_seconds": summary["wall_secs"], "solve_supervised_seconds": execution["stages"][2]["elapsed_seconds"],
                            "solve_root_os_peak_resident_bytes": execution["stages"][2]["measurement"]["root_os_peak_resident_bytes"],
                            "memory_scope": "whole child wait4 peak, not solver allocation or phase peak",
                            "all_inner_stages_completed": True},
              "tree": topology, "chips_per_bb": 100, "rake_assumption": comparison["rake_assumption"],
              "live_profile": {"basis": "pre-save average profile in header/summary", "ev_chips": saved["pre_save_metadata"]["ev"],
                               "br_gain_chips": saved["pre_save_metadata"]["expl"], "nash_conv_chips": summary["nash_conv"]},
              "saved_profile": {"basis": "Full stored quantized average strategy reloaded; same compiled diagnostic game", "units": "chips",
                                **saved["recomputed"], "eval_seconds": saved["eval_secs"]},
              "quantization_difference": numerical, "reference_display": observed["root_ui"],
              "live_minus_reference_ev_bb": comparison["ev_diagnostic"]["root_ev"],
              "saved_minus_reference_ev_bb": [str(Decimal(str(e))/100-Decimal(str(observed["root_ui"][seat+"_ev_bb"])))
                                             for e, seat in zip(saved["recomputed"]["ev"], ("oop", "ip"))],
              "saved_nash_conv_bb": saved["recomputed"]["nash_conv"]/100,
              "saved_mean_br_gain_percent_starting_pot": saved["recomputed"]["nash_conv"]/1100*100,
              "comparison_threshold": None, "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None,
              "why_not_certified": ["Reference per-solution version and exploitability metric/denominator are unknown",
                                    "Rake folding/uncalled-wager return/rounding conventions remain unverified; menu equality does not prove terminal utility equality",
                                    "Reference EV is displayed to 0.01 BB, and full reference strategy/BR is unavailable",
                                    "Close aggregate EV cannot establish equal finite-game semantics or equivalent exploitability"],
              "provenance_limit": "3 host executable bytes are absent from these three scoped archives (root reports later codec-bundle retention); original generic readiness is preserved, not promoted to all-evidence success"}
    return verification, report


def markdown(report):
    live, saved = report["live_profile"], report["saved_profile"]
    return f"""# HU-R0-002 / VM07 診断実測

source07の8件の検証・ビルド、6件の診断stage、Full保存戦略の再評価を、3本の回収archiveと元source/input/binaryのhashへ照合した。132判断点の順序付きメニュー・actor・pot・履歴・残stackが観測記録と一致し、261終端、393公開node、未取得frontier 0を確認した。これは観測した行動木の一致であり、参照解との品質認定ではない。

`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null`を維持する。参照の個別版・精度指標と分母、fold時のレーキ、uncalled wagerの返却、丸め規則は未確認で、参照側の全戦略・BRもない。EVが近くても同一有限ゲームや同等exploitabilityは証明できない。

| 評価対象 | BB / OOP EV (BB) | BTN / IP EV (BB) | NashConv (BB) |
|---|---:|---:|---:|
| 参照画面（0.01 BB表示） | 2.37 | 2.76 | 不明 |
| 保存前live平均戦略 | {live['ev_chips'][0]/100:.12f} | {live['ev_chips'][1]/100:.12f} | {live['nash_conv_chips']/100:.12f} |
| Full保存後の量子化戦略 | {saved['ev'][0]/100:.12f} | {saved['ev'][1]/100:.12f} | {saved['nash_conv']/100:.12f} |

保存後の両seat BRは {saved['br'][0]/100:.12f} / {saved['br'][1]/100:.12f} BB、BR gainは {saved['gains'][0]/100:.12f} / {saved['gains'][1]/100:.12f} BB。NashConvは両gainの和であり、その半分を開始pot 5.5 BBで割った値は {report['saved_mean_br_gain_percent_starting_pot']:.12f}% になる。この式をGTO Wizardの精度表記と同一視しない。量子化前後の差はJSONに符号付きで保持し、後付けの許容差・品質閾値は選んでいない。

100 chips/BB、pot 550、stack 9750、493/479正weight combos、isoなし。レーキ5%・cap 0.6 BBを仮定し、現在のruntimeはfoldでも両者の実contributionを含むpotから徴収する。例えばbet 2 BB→foldは0.375 BBとなり、matched-potを基準にすれば0.275 BBである。この差をEV合計から補正・認定していない。

1000反復の予算を完了。内部solve wallは {report['execution']['solve_internal_wall_seconds']:.9f} 秒、監督processは {report['execution']['solve_supervised_seconds']:.9f} 秒、wait4によるchild peak RSSは {report['execution']['solve_root_os_peak_resident_bytes']:,} bytes。内部30秒制限には達しておらず、品質targetも未設定。単独River診断の1回実行であり、性能比較やphase別memoryの証拠ではない。

保存後評価はsource07の研究用helperがFull戦略をロードして行った実測である。このPython verifierはarchive・記録・数値整合を確認し、Rust評価や独立scalar oracleを再実行しない。SOLのSHA-256は保存後auditの前後とも `0bb2bf5a1e3076378a40bb08771754f14a8ad4b20010d5175336c2b4d520ead9` のまま。

source07ビルドで観測されたCPUはAMD EPYC 7B12、Rust 1.97.0、bootは `0727ebb3-36dd-4fae-b978-c629114703ed`。launcherは診断開始前に同じbootを確認するが、独立したstageごとのboot記録はない。systemd外枠はlauncherに記載されており、回収記録から直接確認できるのは内側supervisorの600秒、grace 5秒、kill wait 5秒、40 GiB/8 GiB free/10 GiB disk制限と正常終了・cleanupである。

元のgeneric retention readinessは書き換えない。cross-bundleで必要なsource/binary/input/outputを解決した一方、この3本のarchiveにはbash・Python・Cargoの実行file bytesがなく、記録された前後hashの一致までしか確認できない。rootは後のcodec bundleへ回収したと報告しているが、それは本verifierの対象外であり、この3件を再ハッシュ済みと扱わない。SOLとCKPTのconfig hash・1000反復header、saved auditのconfig hashも一致する。ただしこのstdlib verifierはBLAKE3自体や圧縮frameを再計算しない。SOL/CKPT・compact記録は [evidence-vm07-002](evidence-vm07-002/)、大きなbinaryとsource archiveはJSONに場所・hashを記したlocal bundleへ保持する。

再検証: `python experiments/hu-postflop-r1/reference/vm07-002-verification.py --check`。負例: 同script `--self-test`。詳細は [verification JSON](vm07-002-verification.json) と [実測JSON](vm07-002-report.json)。全bundleの元memberを検査し、retained codeの実行・展開・書換えは行わない。
"""


class NegativeTests(unittest.TestCase):
    def test_duplicate_json_and_nonfinite_rejected(self):
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":1}\0'):
            with self.assertRaises(ValueError):
                document(raw)

    def test_hash_and_size_fail_closed(self):
        for item in ({"bytes": 3, "sha256": sha(b"abd")}, {"bytes": 4, "sha256": sha(b"abc")}):
            with self.assertRaises(ValueError):
                match(b"abc", item)

    def test_partial_tree_cannot_pass(self):
        observed = document((HERE/"HU-R0-002/observed.json").read_bytes())
        with self.assertRaises(ValueError):
            tree_check([], observed)

    def test_timeout_and_cleanup_failure_not_success(self):
        record = document((HERE/"evidence-vm07-002/records/diagnostic/saved-audit/supervisor.json").read_bytes())
        for key, value in (("state", "timeout"), ("supervisor_exit_code", 2), ("cleanup_complete", False), ("forced", True)):
            changed = copy.deepcopy(record)
            changed[key] = value
            with self.assertRaises(ValueError):
                completed(changed)

    def test_material_negative_gain_rejected(self):
        saved = document((HERE/"evidence-vm07-002/records/diagnostic/saved-audit/stdout.json").read_bytes())
        summary = document((HERE/"evidence-vm07-002/records/diagnostic/diagnostic/summary.json").read_bytes())
        saved["recomputed"]["gains"][0] = -0.5
        saved["recomputed"]["br"][0] = saved["recomputed"]["ev"][0]-0.5
        saved["recomputed"]["nash_conv"] = sum(saved["recomputed"]["gains"])
        with self.assertRaises(ValueError):
            profile_check(saved, summary)

    def test_wrong_config_rejected(self):
        config = tomllib.loads((HERE/"HU-R0-002/diagnostic.toml").read_text())
        changed = copy.deepcopy(config)
        changed["rake"]["rate"] = 0
        with self.assertRaises(ValueError):
            same_config(changed, config)

    def test_partial_profile_and_wrong_storage_rejected(self):
        saved = document((HERE/"evidence-vm07-002/records/diagnostic/saved-audit/stdout.json").read_bytes())
        summary = document((HERE/"evidence-vm07-002/records/diagnostic/diagnostic/summary.json").read_bytes())
        for key, value in (("mode", "no-rivers"), ("source_storage", "i16"), ("iterations", 999)):
            changed = copy.deepcopy(saved)
            changed["artifact"][key] = value
            with self.assertRaises(ValueError):
                profile_check(changed, summary)

    def test_wrong_actual_menu_or_pot_rejected(self):
        observed = document((HERE/"HU-R0-002/observed.json").read_bytes())
        rows = document((HERE/"evidence-vm07-002/records/diagnostic/diagnostic/tree.json").read_bytes())
        for key, value in (("pot", 551), ("actions", ["check", "bet 100"])):
            changed = copy.deepcopy(rows)
            changed[0][key] = value
            with self.assertRaises(ValueError):
                tree_check(changed, observed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cloud", type=Path, default=REPO/"runs/r1-cloud")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--write", action="store_true")
    group.add_argument("--check", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(NegativeTests))
        return 0 if result.wasSuccessful() else 1
    verification, report = verify(args.cloud)
    files = {"vm07-002-verification.json": encoded(verification), "vm07-002-report.json": encoded(report),
             "vm07-002-report.md": markdown(report).encode()}
    for name, raw in files.items():
        if args.write:
            (HERE/name).write_bytes(raw)
        elif args.check:
            require((HERE/name).read_bytes() == raw, "generated report differs: " + name)
    print(json.dumps({"specific_evidence_verification": verification["specific_evidence_verification"],
                      "verified_stage_count": len(verification["stages"]), "tree": verification["actual_tree"],
                      "unretained_host_executables": len(verification["unretained_host_executables"]),
                      "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
