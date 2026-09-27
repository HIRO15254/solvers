"""Portable proof01 checker. Reads retained bytes only; executes no retained code.

Run from any directory in the pinned repository checkout: python -B verify.py.
Original Windows paths are identifiers, never paths opened on the verifying host.
"""
from __future__ import annotations

import gzip
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import struct
import tarfile
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
PROOF = HERE / "proof01"
PREFLIGHT = HERE.parent / "native-preflight/proof01"
ARMS = ("baseline-1", "baseline-2", "flat-1", "flat-2")


def need(condition, message):
    if not condition:
        raise ValueError(message)


def norm(value):
    return str(value).replace("\\", "/")


def safe(value):
    p = PurePosixPath(norm(value))
    need(not p.is_absolute() and bool(p.parts) and all(x not in (".", "..") and ":" not in x for x in p.parts), "unsafe relative path")
    return p.as_posix()


def fields(value):
    return {k: value[k] for k in ("bytes", "sha256")}


def digest_stream(stream):
    h, count, prefix = hashlib.sha256(), 0, bytearray()
    while chunk := stream.read(1024 * 1024):
        h.update(chunk)
        count += len(chunk)
        prefix.extend(chunk[:max(0, 512 - len(prefix))])
    return {"bytes": count, "sha256": h.hexdigest()}, bytes(prefix)


def pin(path):
    need(path.is_file() and not path.is_symlink(), f"not a regular file: {path}")
    with path.open("rb") as stream:
        return digest_stream(stream)[0]


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def manifest(directory, exact=False):
    doc = read(directory / "manifest.json")
    listed = {safe(name): value for name, value in doc["files"].items()}
    need(len(listed) == len(doc["files"]), "normalized duplicate file")
    for name, value in listed.items():
        need(pin(directory / name) == fields(value), f"manifest mismatch: {name}")
    if exact:
        actual = {p.relative_to(directory).as_posix() for p in directory.rglob("*") if p.is_file()}
        need(actual == set(listed) | {"manifest.json"}, "unlisted/missing proof payload")
    return doc


def bits(value):
    need(math.isfinite(value), "nonfinite quality scalar")
    return struct.pack(">d", value).hex()


def raw_command(command):
    return command[command.index("--") + 1:]


def externs(command):
    found = {}
    for i, value in enumerate(command):
        if value == "--extern":
            name, path = command[i + 1].split("=", 1)
            need(name not in found, "duplicate extern")
            found[name] = norm(path)
    return found


def record(path, expected_argv=None):
    r = read(path)
    need(r["state"] == r["stop_reason"] == "completed", f"failed process: {path}")
    need(r["child_exit_code"] == r["supervisor_exit_code"] == 0, "nonzero process exit")
    need(r["cleanup_complete"] and not r["forced"] and not r["errors"], "incomplete cleanup")
    need(r["identity_unchanged"] and r["identity_before"] == r["identity_after"], "identity changed")
    need(not r["last_sample"]["pids"], "remaining process")
    need(r["bounded_job_settings"] == {"limit_flags": 8704, "job_memory_limit_bytes": 536870912,
         "root_priority_class": 16384, "verified_before_resume": True}, "Job limits differ")
    need(r["limits"]["hard_job_commit_limit_bytes"] == 536870912 and r["limits"]["timeout_seconds"] <= 60, "bounds differ")
    if expected_argv is not None:
        need(list(map(norm, r["argv"])) == list(map(norm, expected_argv)), "command binding differs")
    need(r["resolved_argv"] == r["argv"], "resolved command differs")
    for kind, value in r["outputs"].items():
        local = path.parent / PurePosixPath(norm(value["path"])).name
        need(pin(local) == fields(value), f"raw {kind} pin differs")
    samples = [json.loads(line) for line in (path.parent / PurePosixPath(norm(r["outputs"]["samples"]["path"])).name).read_text().splitlines()]
    need(len(samples) == r["measurement"]["sample_count"] and samples[-1] == r["last_sample"], "sampling closure differs")
    identities = {norm(item["path"]): fields(item) for item in r["identity_before"]}
    need(len(identities) == len(r["identity_before"]), "duplicate process identity")
    return r, identities


def main():
    m = manifest(PROOF, exact=True)
    pm = manifest(PREFLIGHT)
    need(len(m["files"]) == 75, "unexpected proof payload count")
    need(m["performance_or_convergence_acceptance"] is False and m["cfv_and_public_file_formats_checked"] is False, "proof scope changed")
    original_root = norm(m["source"]["path"]).split("/experiments/")[0] + "/"

    def local_source(original):
        path = norm(original)
        rel = path[len(original_root):] if path.startswith(original_root) else path
        need(not rel.startswith(".cache/") and not rel.startswith("target/"), "cache read forbidden")
        return ROOT / safe(rel)

    need(pin(HERE / "solve.rs") == fields(m["source"]), "adapter pin differs")
    fixture = HERE.parent / "fixtures/narrow.toml"
    static = read(HERE.parent / "fixtures/static-check.json")
    need(pin(fixture) == fields(static["fixtures"]["narrow"]), "fixture pin differs")
    config = tomllib.loads(fixture.read_text())
    need(config["game"]["oop_range"] == "TT+,AQs+,KQs" and config["game"]["ip_range"] == "JJ-99,AQs-ATs,KQs,QJs", "fixed range differs")
    need(config["game"]["board"] == "Qs Jh 2h", "board differs")
    # Independent explicit class enumeration for these two fixed unit ranges.
    ranks, suits = "23456789TJQKA", "cdhs"
    board = {4 * ranks.index(c[0]) + suits.index(c[1]) for c in config["game"]["board"].split()}
    classes = [({8, 9, 10, 11, 12}, {(12, 10), (12, 11), (11, 10)}),
               ({7, 8, 9}, {(12, 10), (12, 9), (12, 8), (11, 10), (10, 9)})]
    combos = []
    for pairs, suited in classes:
        ids = []
        for high in range(52):
            for low in range(high):
                if high in board or low in board:
                    continue
                hi, lo = high // 4, low // 4
                if (hi == lo and hi in pairs) or (high % 4 == low % 4 and (hi, lo) in suited):
                    ids.append(high * (high - 1) // 2 + low)
        combos.append(ids)
    need(list(map(len, combos)) == [34, 30], "independent support dimensions differ")
    need(combos == [static["fixtures"]["narrow"]["ranges"][s]["global_combo_ids"] for s in ("oop", "ip")], "fixture static support differs")

    with gzip.open(PROOF / "shared-state.bin.gz", "rb") as stream:
        state_pin, prefix = digest_stream(stream)
    need(state_pin == m["shared_state_uncompressed"], "full state stream hash differs")
    need(set(m["deduplicated_original_states"]) == set(ARMS), "missing original state dedup entry")
    need(all(value == state_pin for value in m["deduplicated_original_states"].values()), "original state pins differ")
    need(prefix[:8] == b"R1F32S01", "state magic differs")
    header = struct.unpack_from("<8Q", prefix, 8)
    need(header == (2, 2, 367662, 147104, 34, 30, 10176768, 10176768), "state header differs")
    ids = list(struct.unpack_from("<64H", prefix, 72))
    need(ids == combos[0] + combos[1], "state root combo IDs differ")
    need(state_pin["bytes"] == 72 + 128 + 8 * 10176768, "state length differs")
    binary_pins = {}
    for arm in ("baseline", "flat"):
        with gzip.open(PROOF / f"{arm}-probe.exe.gz", "rb") as stream:
            binary_pins[arm], _ = digest_stream(stream)
        need(binary_pins[arm] == m["binary_uncompressed"][arm], "binary original bytes differ")

    flat = read(PROOF / "flat-build/receipt.json")
    candidate = read(HERE.parent / "flat-chance/manifest.json")
    archive = {}
    with tarfile.open(PROOF / "flat-engine-source.tar.gz", "r:gz") as tar:
        for member in tar:
            name = safe(member.name)
            need(member.isfile() and name not in archive and "/" not in name, "unsafe candidate member")
            with tar.extractfile(member) as stream:
                archive[name], _ = digest_stream(stream)
    expected_engine = {p.name for p in (ROOT / "crates/engine/src").glob("*.rs")}
    need(set(archive) == expected_engine and len(archive) == 8, "candidate source set differs")
    for name, value in archive.items():
        if name == "solver.rs":
            need(value == {"bytes": candidate["candidate_bytes"], "sha256": candidate["candidate_sha256"]}, "candidate solver differs")
        else:
            need(pin(ROOT / "crates/engine/src" / name) == value, "unexpected engine edit")
    need(pin(ROOT / "crates/engine/src/solver.rs")["sha256"] == candidate["source_sha256"], "baseline solver differs")
    for path, value in flat["source_pins"].items():
        expected = archive[PurePosixPath(norm(path)).name] if "/.cache/" in norm(path) else pin(local_source(path))
        need(expected == fields(value), f"flat source differs: {path}")
    need(flat["candidate_sha256"] == candidate["candidate_sha256"], "flat receipt candidate differs")
    need(flat["all_passed"] and flat["source_unchanged"] and flat["dependency_unchanged"], "flat build incomplete")

    # Link fresh baseline crate outputs through retained receipts; no rlib/cache access.
    pre = read(PREFLIGHT / "build02/build-receipt.json")
    need(pre["all_stages_passed"] and pre["sources_unchanged"] and pre["cached_external_unchanged"], "baseline build incomplete")
    for path, value in pre["source_pins"].items():
        need(pin(local_source(path)) == fields(value), f"baseline source differs: {path}")
    need([s["name"] for s in pre["stages"]] == ["cards", "engine", "game", "hand_index", "holdem", "probe"], "baseline build order differs")
    known = {norm(p): fields(v) for p, v in pre["cached_external_pins"].items()}
    # Relative cached paths become original absolute identity keys.
    known = {(p if ":/" in p else original_root + p): v for p, v in known.items()}
    semantic = {PurePosixPath(p).name[3:].split("-", 1)[0]: p for p in known}
    depsets = {"cards": {"aya_poker", "serde", "thiserror"}, "engine": {"cards", "rayon", "rand", "rand_chacha"},
               "game": {"cards", "engine"}, "hand_index": {"cards"}, "holdem": {"cards", "engine", "game", "hand_index"}}
    baseline_libraries = {}

    def build_command(command, recpath, name, expected_deps, dep_pins, artifact):
        argv = raw_command(command)
        need(argv[argv.index("--crate-name") + 1] == name, "crate name differs")
        deps = externs(argv)
        need(set(deps) == expected_deps, f"extern set differs: {name}")
        for dep, path in deps.items():
            need(semantic.get(dep) == path, f"wrong arm or semantic library: {dep}")
            need(path in known and path in dep_pins and known[path] == dep_pins[path], "dependency lineage differs")
        need(norm(argv[argv.index("-o") + 1]) == norm(artifact["path"]), "artifact output differs")
        r, identities = record(recpath, argv)
        for path, value in identities.items():
            if path.startswith(original_root) and "/target/" not in path:
                actual = archive[PurePosixPath(path).name] if "/.cache/" in path else pin(local_source(path))
                need(actual == value, "build source identity differs")
        known[norm(artifact["path"])] = fields(artifact)
        semantic[name] = norm(artifact["path"])
        return deps

    for stage in pre["stages"]:
        need(stage["wrapper_exit_code"] == 0, "baseline stage failed")
        name = stage["name"]
        expected = depsets[name] if name != "probe" else {"cards", "engine", "game", "holdem"}
        build_command(stage["command"], PREFLIGHT / "build02" / stage["record"], name if name != "probe" else "flop_native_probe", expected, known.copy(), stage["artifact"])
        if name != "probe":
            baseline_libraries[name] = stage["artifact"]
    base = read(PROOF / "baseline-build/receipt.json")
    need(base["wrapper_exit_code"] == 0 and base["source_unchanged"] and fields(base["source"]) == fields(m["source"]), "baseline probe build failed")
    base_deps = {norm(p): fields(v) for p, v in base["dependencies"].items()}
    build_command(base["command"], PROOF / "baseline-build/build.json", "flop_solve_probe", {"cards", "engine", "game", "holdem", "rayon"}, base_deps, base["binary"])
    need(fields(base["binary"]) == binary_pins["baseline"], "baseline binary lineage differs")
    flat_deps = {norm(p): fields(v) for p, v in flat["dependency_pins"].items()}
    need(all(known.get(p) == v for p, v in flat_deps.items()), "flat reused library pins differ")
    need([s["name"] for s in flat["commands"]] == ["engine", "game", "holdem", "probe"], "flat build order differs")
    for i, stage in enumerate(flat["commands"]):
        need(stage["exit_code"] == 0, "flat build stage failed")
        name = stage["name"]
        expected = depsets[name] if name != "probe" else {"cards", "engine", "game", "holdem", "rayon"}
        build_command(stage["command"], PROOF / f"flat-build/{i:02d}-{name}.json", name if name != "probe" else "flop_solve_probe", expected, flat_deps, stage["artifact"])
        flat_deps[norm(stage["artifact"]["path"])] = fields(stage["artifact"])
    need(fields(flat["commands"][-1]["artifact"]) == binary_pins["flat"], "flat binary lineage differs")

    executions = read(PROOF / "comparison-execution.json")
    need([(x["arm"], x["threads"]) for x in executions] == [("baseline", 2), ("flat", 1), ("flat", 2)], "comparison order differs")
    plan = read(PROOF / "comparison-plan.json")
    need(plan["iterations"] == 2 and plan["conditions"] == [{"arm": x["arm"], "threads": x["threads"]} for x in executions], "comparison plan differs")
    execution_map = {f"{x['arm']}-{x['threads']}": x for x in executions}
    execution_map["baseline-1"] = read(PROOF / "baseline-1/execution.json")
    quality_bytes = None
    for arm in ARMS:
        role, threads = arm.split("-")
        e = execution_map[arm]
        need(e["exit_code"] == 0, "wrapper failed")
        if arm != "baseline-1":
            need(e["state_bytes_equal"] and e["quality_bytes_equal"], "recorded original comparison failed")
        r, identities = record(PROOF / arm / "record.json", raw_command(e["command"]))
        need(r["argv"][1:4] == ["narrow", threads, "2"], "run arguments differ")
        need(identities[norm(r["argv"][0])] == binary_pins[role], "run binary binding differs")
        for path in [m["source"]["path"], original_root + "experiments/hu-postflop-r1/flop-scaling/fixtures/narrow.toml"]:
            need(identities.get(norm(path)) == pin(local_source(path)), "run input binding differs")
        for path, value in identities.items():
            if path.startswith(original_root) and "/target/" not in path:
                need(pin(local_source(path)) == value, "run control identity differs")
        out = PROOF / arm / "output"
        invocation, result = read(out / "invocation.json"), read(out / "result.json")
        need(invocation == {"schema": "r1.flop-native-solve/v1", "case": "narrow", "threads": int(threads), "iterations": 2,
             "planned_iterations": 2, "chance_depth": 2, "min_children": 12, "storage": "f32", "schedule": "dcfr", "alpha": 1.5,
             "beta": 0, "gamma": 3, "pow4_reset": True, "cli_toml_normalized": False, "quality_target": None, "cfv_capture": False}, "invocation differs")
        need(result["status"] == "completed" and result["case"] == "narrow" and result["iterations"] == 2
             and result["threads"] == int(threads) and result["state_bytes"] == state_pin["bytes"]
             and result["state_file"] == "state.bin" and result["quality_file"] == "quality.json"
             and result["cfv_capture"] is False and result["performance_claim"] is False, "incomplete run result")
        events = [json.loads(line) for line in (PROOF / arm / "record.stdout.log").read_text().splitlines()]
        phases = ["build", "solver_allocation", "cfr", "state_write", "ev_p0", "ev_p1", "br_p0", "br_p1", "exploitability"]
        need(events == [{"phase": p, "status": s} for p in phases for s in ("started", "completed")] + [{"phase": "probe", "status": "completed"}], "phase closure differs")
        raw = (out / "quality.json").read_bytes()
        if quality_bytes is None:
            quality_bytes = raw
        need(raw == quality_bytes, "quality bytes differ")
    q = json.loads(quality_bytes)
    need(q["case"] == "narrow" and q["iterations"] == 2 and q["root_support"] == [34, 30]
         and q["normalizer_bits"] == bits(870.0) and q["quality_target"] is None and q["cfv_capture"] is False, "quality scope differs")
    for name in ("ev", "br", "exploitability"):
        need([bits(x) for x in q[name]] == q[name + "_bits"], f"{name} scalar/raw bits differ")
    gains = [q["br"][0] - q["ev"][0], q["br"][1] + q["ev"][0]]
    need([bits(x) for x in gains] == q["exploitability_bits"], "public zero-sum gains differ")
    print(json.dumps({"schema": "r1.flop-native-portable-verification/v1", "status": "verified",
        "proof_manifest": pin(PROOF / "manifest.json"), "preflight_manifest": pin(PREFLIGHT / "manifest.json"),
        "payload_files": len(m["files"]), "candidate_archive_files": len(archive), "unchanged_engine_files": 7,
        "successful_runs": list(ARMS), "state": state_pin, "state_header": list(header), "quality": fields({"bytes": len(quality_bytes), "sha256": hashlib.sha256(quality_bytes).hexdigest()}),
        "nash_conv_chips": sum(gains), "usual_exploitability_chips": sum(gains) / 2,
        "fresh_baseline_libraries": sorted(baseline_libraries), "candidate_rebuilt": [s["name"] for s in flat["commands"]],
        "scope": "Narrow F32/DCFR two iterations; full state and public scalar quality identity only",
        "limitations": ["One deduplicated state stream retained; four original hashes/equality are capture records, not four independent retained streams",
        "Library/compiler/cached transitive dependency bytes are not retained here; library lineage is cross-checked from source and build receipts, not reproduced compilation",
        "Requires pinned repository source checkout; no original cache/target or absolute-path files are opened",
        "No convergence, performance, full-workspace, CFV, checkpoint or SOL wire acceptance"]}, indent=2))


if __name__ == "__main__":
    main()
