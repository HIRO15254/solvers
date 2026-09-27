"""Verify retained optimized Flop proof; never execute archived code or binaries.

Run with Python 3.11+ from the pinned repository checkout. Original host paths
are identity labels only. No cache, target, registry or original run is opened.
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
DEBUG = HERE.parent / "native-solve/proof01"
CRATES = ("cards", "engine", "game", "hand-index", "holdem")
CONDITIONS = [(case, arm, workers) for case in ("narrow", "expanded")
              for arm, workers in (("baseline", 1), ("baseline", 2), ("flat", 1), ("flat", 2))]
HOST_LIBS = {"unicode-ident", "proc-macro2", "quote", "quickdiv", "fastrand",
             "aya_base", "miniphf", "syn", "aya_codegen"}
RUNTIME_LIBS = {"aya_base", "aya_poker", "cards", "cfg-if", "crossbeam-deque", "crossbeam-epoch",
                "crossbeam-utils", "either", "engine", "fastrand", "flop-solve-opt-probe", "game",
                "getrandom", "hand-index", "holdem", "ppv-lite86", "quickdiv", "rand", "rand_chacha",
                "rand_core", "rayon", "rayon-core", "serde", "serde_core", "thiserror", "zerocopy"}
BUILD_SCRIPTS = {"proc-macro2", "quote", "crossbeam-utils", "serde_core", "zerocopy",
                 "aya_poker", "thiserror", "serde", "rayon-core"}


def need(ok, message):
    if not ok:
        raise ValueError(message)


def norm(value):
    return str(value).replace("\\", "/")


def safe(value):
    text = norm(value)
    p = PurePosixPath(text)
    need(not p.is_absolute() and p.parts and all(x not in ("", ".", "..") and ":" not in x
         for x in text.split("/")), f"unsafe relative path: {text}")
    return p.as_posix()


def fields(value):
    return {k: value[k] for k in ("bytes", "sha256")}


def digest(stream):
    h, count, prefix = hashlib.sha256(), 0, bytearray()
    while chunk := stream.read(1024 * 1024):
        h.update(chunk)
        count += len(chunk)
        prefix.extend(chunk[:max(0, 1024 - len(prefix))])
    return {"bytes": count, "sha256": h.hexdigest()}, bytes(prefix)


def pin(path):
    need(path.is_file() and not path.is_symlink(), f"not a regular file: {path}")
    with path.open("rb") as stream:
        return digest(stream)[0]


def byte_pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def bits(value):
    need(math.isfinite(value), "nonfinite quality")
    return struct.pack(">d", value).hex()


def raw_command(command):
    return command[command.index("--") + 1:]


def compressed(path, expected):
    with gzip.open(path, "rb") as stream:
        value, prefix = digest(stream)
    need(value == fields(expected), f"uncompressed hash differs: {path.name}")
    return value, prefix


def manifest():
    doc = read(PROOF / "manifest.json")
    listed = {safe(k): fields(v) for k, v in doc["files"].items()}
    need(len(listed) == len(doc["files"]), "duplicate manifest path")
    actual = {p.relative_to(PROOF).as_posix() for p in PROOF.rglob("*") if p.is_file()}
    need(actual == set(listed) | {"manifest.json"}, "unlisted or missing proof payload")
    for name, expected in listed.items():
        need(pin(PROOF / name) == expected, f"payload hash differs: {name}")
    return doc


def record(path, argv, timeout, identities):
    r = read(path)
    need(r["state"] == r["stop_reason"] == "completed", f"process failed: {path}")
    need(r["child_exit_code"] == r["supervisor_exit_code"] == 0, "nonzero process exit")
    need(r["cleanup_complete"] and not r["forced"] and not r["errors"], "incomplete cleanup")
    need(r["identity_unchanged"] and r["identity_before"] == r["identity_after"], "identity changed")
    need(not r["last_sample"]["pids"] and r["last_sample"]["tree_resident_bytes"] == 0, "remaining process")
    need(list(map(norm, r["argv"])) == list(map(norm, argv)) and r["resolved_argv"] == r["argv"], "argv differs")
    need(r["bounded_job_settings"] == {"limit_flags": 8704, "job_memory_limit_bytes": 536870912,
         "root_priority_class": 16384, "verified_before_resume": True}, "Job settings differ")
    expected_limits = {"timeout_seconds": timeout, "grace_seconds": 0.2, "poll_seconds": 0.1,
                       "memory_limit_bytes": 469762048, "hard_job_commit_limit_bytes": 536870912,
                       "minimum_available_commit_before_launch_bytes": 1610612736,
                       "root_priority_class": 16384, "min_free_memory_bytes": 1610612736,
                       "disk_reserve_bytes": 1073741824}
    need(all(r["limits"][k] == v for k, v in expected_limits.items()), "process limits differ")
    need(r["host_before"]["commit_available_bytes"] >= 1610612736, "start commit reserve differs")
    observed = {norm(v["path"]): fields(v) for v in r["identity_before"]}
    need(len(observed) == len(r["identity_before"]), "duplicate identity")
    # Python is recorded consistently across processes; its bytes are not retained.
    python = {k: v for k, v in observed.items() if PurePosixPath(k).name.lower() == "python.exe"}
    need(len(python) == 1 and set(observed) == set(identities) | set(python), "identity set differs")
    need(all(observed[k] == v for k, v in identities.items()), "identity hash differs")
    for kind, value in r["outputs"].items():
        local = path.parent / PurePosixPath(norm(value["path"])).name
        need(pin(local) == fields(value), f"{kind} output hash differs")
    samples = [json.loads(s) for s in (path.parent / PurePosixPath(norm(r["outputs"]["samples"]["path"])).name).read_text().splitlines()]
    need(len(samples) == r["measurement"]["sample_count"] and samples[-1] == r["last_sample"], "sample closure differs")
    need(r["research_wrapper"]["wrapper_sha256"] == pin(HERE.parent / "native-preflight/run_bounded.py")["sha256"]
         and r["research_wrapper"]["base_sha256"] == pin(ROOT / "tools/run_supervised.py")["sha256"], "supervisor source differs")
    return python


def source_archive(arm, receipt):
    entries = {}
    with tarfile.open(PROOF / f"{arm}-source.tar.gz", "r:gz") as tar:
        for member in tar:
            name = safe(member.name)
            need(member.isfile() and name not in entries and member.size <= 2 * 1024 * 1024, "unsafe source archive member")
            with tar.extractfile(member) as stream:
                entries[name] = stream.read()
    source_prefixes = {norm(p).split("/crates/")[0] for p in receipt["snapshot_pins"] if "/crates/" in norm(p)}
    need(len(source_prefixes) == 1, "snapshot prefix differs")
    prefix = source_prefixes.pop() + "/"
    expected = {}
    for name, value in receipt["snapshot_pins"].items():
        name = norm(name)
        need(name.startswith(prefix), "snapshot outside source")
        expected[safe(name[len(prefix):])] = fields(value)
    need({k: byte_pin(v) for k, v in entries.items()} == expected, "source archive differs from captured snapshot")
    original = {safe(k): fields(v) for k, v in receipt["original_source_pins"].items()}
    for name, value in original.items():
        need(not name.startswith((".cache/", "target/", "runs/")), "original cache path forbidden")
        need(pin(ROOT / name) == value, f"pinned checkout source differs: {name}")
    candidate = read(HERE.parent / "flat-chance/manifest.json")
    wanted = {"Cargo.toml", "Cargo.lock", ".cargo/config.toml", "probe/Cargo.toml", "probe/main.rs"}
    for crate in CRATES:
        manifest_name = f"crates/{crate}/Cargo.toml"
        wanted.add(manifest_name)
        old = tomllib.loads((ROOT / manifest_name).read_text())
        old.pop("dev-dependencies", None)
        old.pop("bench", None)
        need(tomllib.loads(entries[manifest_name].decode()) == old, "runtime crate manifest changed")
        for path in (ROOT / f"crates/{crate}/src").rglob("*"):
            if not path.is_file():
                continue
            rel = path.relative_to(ROOT).as_posix()
            wanted.add(rel)
            value = byte_pin(entries[rel])
            if arm == "flat" and rel == "crates/engine/src/solver.rs":
                need(value == {"bytes": candidate["candidate_bytes"], "sha256": candidate["candidate_sha256"]}, "candidate solver differs")
            else:
                need(value == pin(path), f"unexpected source edit: {rel}")
    need(set(entries) == wanted, "source file set differs")
    need(pin(ROOT / "crates/engine/src/solver.rs")["sha256"] == candidate["source_sha256"], "baseline solver differs")
    root_manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    root_manifest["workspace"]["members"] = [f"crates/{x}" for x in CRATES] + ["probe"]
    need(tomllib.loads(entries["Cargo.toml"].decode()) == root_manifest, "workspace transform differs")
    need(entries[".cargo/config.toml"] == (ROOT / ".cargo/config.toml").read_bytes(), "target config differs")
    need(byte_pin(entries["probe/main.rs"]) == pin(HERE.parent / "native-solve/solve.rs"), "adapter differs")
    probe = tomllib.loads(entries["probe/Cargo.toml"].decode())
    need(probe == {"package": {"name": "flop-solve-opt-probe", "version": "0.0.0", "edition": {"workspace": True}},
                   "bin": [{"name": "flop-solve-opt-probe", "path": "main.rs"}],
                   "dependencies": {x: {"workspace": True} for x in ("cards", "engine", "game", "holdem", "rayon")}}, "probe manifest differs")
    lock = tomllib.loads(entries["Cargo.lock"].decode())
    key = lambda p: (p["name"], p["version"], p.get("source"), p.get("checksum"))
    known = {key(p) for p in tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]}
    registry = [key(p) for p in lock["package"] if p.get("source")]
    need(len(registry) == 31 and set(registry) <= known and list(map(list, registry)) == receipt["locked_registry_packages"], "registry lock/checksum differs")
    return entries, prefix, registry


def build(arm, binary):
    directory = PROOF / f"{arm}-build"
    r = read(directory / "receipt.json")
    need(r["schema"] == "r1.flop-optimized-build/v1" and r["arm"] == arm, "build receipt scope differs")
    need(r["all_passed"] and r["snapshot_unchanged"] and r["original_sources_unchanged"], "build/source closure failed")
    need(r["profile"] == {"lto": "thin", "codegen-units": 1}, "release profile declaration differs")
    need(r["limits"] == {"job_commit_bytes": 536870912, "build_wall_seconds": 180, "below_normal": True, "jobs": 1}, "build limits differ")
    need(fields(r["binary"]) == binary, "build binary hash differs")
    entries, prefix, registry = source_archive(arm, r)
    need([s["name"] for s in r["stages"]] == ["metadata", "build"], "build stage order differs")
    need(all(s["exit_code"] == 0 for s in r["stages"]), "build wrapper failed")
    source_root = norm(raw_command(r["stages"][0]["command"])[0])  # Cargo path, not opened.
    need(source_root == norm(r["cargo"]["path"]), "Cargo path binding differs")
    metadata_record = read(directory / "metadata.json")
    absolute_source = norm(metadata_record["cwd"]).rstrip("/")
    original_root = absolute_source[: -len(prefix.rstrip("/"))]
    need(absolute_source == original_root + prefix.rstrip("/"), "source label differs")
    controls = {original_root + "tools/run_supervised.py": pin(ROOT / "tools/run_supervised.py"),
                original_root + "experiments/hu-postflop-r1/flop-scaling/native-preflight/run_bounded.py": pin(HERE.parent / "native-preflight/run_bounded.py")}
    identities = {**controls, norm(r["cargo"]["path"]): fields(r["cargo"]), norm(r["compiler"]["path"]): fields(r["compiler"]),
                  absolute_source + "/Cargo.toml": byte_pin(entries["Cargo.toml"]),
                  absolute_source + "/probe/main.rs": byte_pin(entries["probe/main.rs"])}
    need(raw_command(r["stages"][0]["command"]) == [r["cargo"]["path"], "metadata", "--offline", "--format-version=1"], "metadata command differs")
    python = record(directory / "metadata.json", raw_command(r["stages"][0]["command"]), 30, identities)
    identities[absolute_source + "/Cargo.lock"] = byte_pin(entries["Cargo.lock"])
    argv = raw_command(r["stages"][1]["command"])
    target = norm(argv[argv.index("--target-dir") + 1]).rstrip("/")
    need(argv == [r["cargo"]["path"], "build", "--release", "--offline", "--locked", "-j1", "--target-dir", argv[argv.index("--target-dir") + 1], "-p", "flop-solve-opt-probe", "--message-format=json"], "release command differs")
    need(target.startswith(original_root + "target/") and norm(r["binary"]["path"]) == target + "/release/flop-solve-opt-probe.exe", "target binding differs")
    need(record(directory / "build.json", argv, 180, identities) == python, "Python identity differs")
    metadata = read(directory / "metadata.stdout.log")
    packages = {p["id"]: p for p in metadata["packages"]}
    local = {p["name"]: p for p in packages.values() if p["source"] is None}
    need(set(local) == set(CRATES) | {"flop-solve-opt-probe"}, "local package set differs")
    need(norm(metadata["workspace_root"]) == absolute_source, "metadata source root differs")
    for name, p in local.items():
        rel = "probe/Cargo.toml" if name == "flop-solve-opt-probe" else f"crates/{name}/Cargo.toml"
        need(norm(p["manifest_path"]) == absolute_source + "/" + rel, "metadata uses outside local source")
    locked = {(x[0], x[1], x[2]) for x in registry}
    need({(p["name"], p["version"], p["source"]) for p in packages.values() if p["source"]} == locked, "resolved registry set differs")
    events = [json.loads(line) for line in (directory / "build.stdout.log").read_text().splitlines()]
    need(events[-1] == {"reason": "build-finished", "success": True}, "Cargo completion differs")
    artifacts = [x for x in events if x["reason"] == "compiler-artifact"]
    need(len(artifacts) == 46 and sum(x["reason"] == "build-script-executed" for x in events) == 9
         and all(x["reason"] in ("compiler-artifact", "build-script-executed", "build-finished") for x in events), "Cargo event set differs")
    runtime, host, signatures, host_libraries, custom, macros = set(), set(), [], set(), set(), set()
    for a in artifacts:
        need(a["package_id"] in packages and not a["fresh"], "stale or unknown Cargo artifact")
        p = packages[a["package_id"]]
        kind = a["target"]["kind"]
        profile = a["profile"]
        need({k: v for k, v in profile.items() if k != "opt_level"} == {"debuginfo": 0, "debug_assertions": False, "overflow_checks": False, "test": False}, "Cargo artifact profile differs")
        is_host = kind in (["custom-build"], ["proc-macro"]) or (profile["opt_level"] == "0" and p["name"] in HOST_LIBS)
        need(profile["opt_level"] == ("0" if is_host else "3"), "runtime artifact not optimized")
        need(kind in (["lib"], ["bin"], ["custom-build"], ["proc-macro"]), "unexpected target kind")
        need(any(a["target"] == t for t in p["targets"]), "artifact target differs from resolved package")
        need(all(norm(f).startswith(target + "/release/") for f in a["filenames"]), "artifact outside fresh release target")
        if p["source"] is None:
            rel = "probe/main.rs" if p["name"] == "flop-solve-opt-probe" else f"crates/{p['name']}/src/lib.rs"
            need(norm(a["target"]["src_path"]) == absolute_source + "/" + rel, "workspace artifact source differs")
        (host if is_host else runtime).add(p["name"])
        if kind == ["custom-build"]:
            custom.add(p["name"])
        elif kind == ["proc-macro"]:
            macros.add(p["name"])
        elif is_host:
            host_libraries.add(p["name"])
        signatures.append((p["name"], p["version"], tuple(kind), profile["opt_level"], tuple(a["features"])))
    need(runtime == RUNTIME_LIBS and host_libraries == HOST_LIBS and custom == BUILD_SCRIPTS
         and macros == {"serde_derive", "thiserror-impl"} and len(set(signatures)) == 46, "fixed artifact set differs")
    executed = [x for x in events if x["reason"] == "build-script-executed"]
    need({packages[x["package_id"]]["name"] for x in executed} == BUILD_SCRIPTS, "build script closure differs")
    for event in executed:
        need(not event["linked_libs"] and not event["linked_paths"] and not event["env"]
             and norm(event["out_dir"]).startswith(target + "/release/build/"), "build script output boundary differs")
    need(sum(a["executable"] == r["binary"]["path"] for a in artifacts) == 1, "executable Cargo binding differs")
    return r, original_root, controls, python, sorted(signatures), {"runtime_packages": sorted(runtime), "host_packages": sorted(host), "artifacts": len(artifacts)}


def main():
    m = manifest()
    need(len(m["files"]) == 105 and m["builder"] == pin(HERE / "build.py")
         and m["runner"] == pin(HERE / "run.py"), "manifest control binding differs")
    need(m["narrow_state_reference"] == "../../native-solve/proof01/shared-state.bin.gz", "narrow reference label differs")
    binaries = {arm: compressed(PROOF / f"{arm}-probe.exe.gz", m["binary_uncompressed"][arm])[0] for arm in ("baseline", "flat")}
    built = {arm: build(arm, binaries[arm]) for arm in ("baseline", "flat")}
    need(built["baseline"][1:5] == built["flat"][1:5], "arm control/compiler profile differs")
    for key in ("compiler", "cargo", "rustc_version", "original_source_pins", "locked_registry_packages", "profile"):
        need(built["baseline"][0][key] == built["flat"][0][key], f"arm build basis differs: {key}")
    plan, executions = read(PROOF / "matrix/plan.json"), read(PROOF / "matrix/execution.json")
    need(plan["conditions"] == list(map(list, CONDITIONS)) and len(executions) == 8, "matrix set differs")
    need(plan["exact_iterations"] == 2 and plan["wall_seconds_each"] == 60 and plan["job_commit_bytes"] == 536870912
         and plan["rss_trigger_bytes"] == 469762048 and plan["stop_on_failure_or_mismatch"], "matrix bounds differ")
    need(plan["adapter"] == pin(HERE.parent / "native-solve/solve.rs") and plan["runner"] == pin(HERE / "run.py"), "matrix source differs")
    debug_manifest = read(DEBUG / "manifest.json")
    states, headers = {}, {}
    fixture_data = read(HERE.parent / "fixtures/static-check.json")["fixtures"]
    for case, dimensions, length in (("narrow", [34, 30], 10176768), ("expanded", [63, 160], 35459676)):
        fixture = HERE.parent / f"fixtures/{case}.toml"
        need(pin(fixture) == fields(fixture_data[case]), "fixture pin differs")
        path = DEBUG / "shared-state.bin.gz" if case == "narrow" else PROOF / "expanded-state.bin.gz"
        state, prefix = compressed(path, m["shared_state_uncompressed"][case])
        if case == "narrow":
            need(state == debug_manifest["shared_state_uncompressed"], "previous debug state differs")
        header = struct.unpack_from("<8Q", prefix, 8)
        need(prefix[:8] == b"R1F32S01" and header == (2, 2, 367662, 147104, *dimensions, length, length), "F32 header differs")
        combo_ids = [x for seat in ("oop", "ip") for x in fixture_data[case]["ranges"][seat]["global_combo_ids"]]
        need(list(struct.unpack_from(f"<{sum(dimensions)}H", prefix, 72)) == combo_ids, "root combo IDs differ")
        need(state["bytes"] == 72 + 2 * sum(dimensions) + 8 * length, "full state length differs")
        states[case], headers[case] = state, list(header)
    need(set(m["deduplicated_original_states"]) == {f"{c}-{a}-{w}" for c, a, w in CONDITIONS}, "dedup capture set differs")
    qualities = {}
    for expected, e in zip(CONDITIONS, executions):
        case, arm, workers = expected
        key = f"{case}-{arm}-{workers}"
        need((e["case"], e["arm"], e["workers"]) == expected and e["exit_code"] == 0 and "failure" not in e, "execution order/failure differs")
        need(e["state_equals_case_baseline1"] and e["quality_equals_case_baseline1"], "capture equality failed")
        need(e["state"] == states[case] == m["deduplicated_original_states"][key], "original state capture differs")
        br, original_root, controls, python, _, _ = built[arm]
        receipt_pin = pin(PROOF / f"{arm}-build/receipt.json")
        need(fields(plan["build_receipts"][arm]) == receipt_pin, "matrix build receipt differs")
        identities = {**controls, norm(br["binary"]["path"]): binaries[arm],
                      norm(plan["build_receipts"][arm]["path"]): receipt_pin,
                      original_root + "experiments/hu-postflop-r1/flop-scaling/native-solve/solve.rs": plan["adapter"],
                      original_root + f"experiments/hu-postflop-r1/flop-scaling/fixtures/{case}.toml": pin(HERE.parent / f"fixtures/{case}.toml")}
        argv = raw_command(e["command"])
        need(argv[:4] == [br["binary"]["path"], case, str(workers), "2"] and len(argv) == 5
             and norm(argv[4]).endswith(f"/{key}/output"), "solve argv differs")
        directory = PROOF / "matrix" / key
        need(record(directory / "record.json", argv, 60, identities) == python, "solve Python identity differs")
        invocation, result = read(directory / "output/invocation.json"), read(directory / "output/result.json")
        need(invocation == {"schema": "r1.flop-native-solve/v1", "case": case, "threads": workers, "iterations": 2,
             "planned_iterations": 2, "chance_depth": 2, "min_children": 12, "storage": "f32", "schedule": "dcfr",
             "alpha": 1.5, "beta": 0, "gamma": 3, "pow4_reset": True, "cli_toml_normalized": False,
             "quality_target": None, "cfv_capture": False}, "invocation differs")
        need(result["status"] == "completed" and result["case"] == case and result["threads"] == workers
             and result["iterations"] == 2 and result["state_bytes"] == states[case]["bytes"]
             and result["state_file"] == "state.bin" and result["quality_file"] == "quality.json"
             and result["cfv_capture"] is False and result["performance_claim"] is False, "solve completion differs")
        phases = ["build", "solver_allocation", "cfr", "state_write", "ev_p0", "ev_p1", "br_p0", "br_p1", "exploitability"]
        events = [json.loads(x) for x in (directory / "record.stdout.log").read_text().splitlines()]
        need(events == [{"phase": p, "status": s} for p in phases for s in ("started", "completed")]
             + [{"phase": "probe", "status": "completed"}], "phase event closure differs")
        raw = (directory / "output/quality.json").read_bytes()
        need(byte_pin(raw) == e["quality"], "quality capture hash differs")
        qualities.setdefault(case, raw)
        need(raw == qualities[case], "quality bytes differ within case")
        if case == "narrow":
            need(e["state_equals_previous_debug"] and e["quality_equals_previous_debug"], "debug capture comparison failed")
            need(raw == (DEBUG / "baseline-1/output/quality.json").read_bytes(), "debug quality differs")
    summary = {}
    for case, raw in qualities.items():
        q = json.loads(raw)
        dims, normalizer = ([34, 30], 870.0) if case == "narrow" else ([63, 160], 8700.0)
        need(q["case"] == case and q["iterations"] == 2 and q["root_support"] == dims
             and q["normalizer_bits"] == bits(normalizer) and q["quality_target"] is None and q["cfv_capture"] is False, "quality scope differs")
        for field in ("ev", "br", "exploitability"):
            need([bits(x) for x in q[field]] == q[field + "_bits"], "quality bits differ")
        gains = [q["br"][0] - q["ev"][0], q["br"][1] + q["ev"][0]]
        need(list(map(bits, gains)) == q["exploitability_bits"], "public zero-sum gain formula differs")
        summary[case] = {"state": states[case], "header": headers[case], "quality": byte_pin(raw),
                         "ev": q["ev"], "br": q["br"], "public_gains": gains,
                         "nash_conv_chips": sum(gains), "usual_exploitability_chips": sum(gains) / 2}
    print(json.dumps({"schema": "r1.flop-optimized-portable-verification/v1", "status": "verified",
          "proof_manifest": pin(PROOF / "manifest.json"), "payload_files": len(m["files"]),
          "successful_runs": [f"{c}-{a}-{w}" for c, a, w in CONDITIONS], "cases": summary,
          "builds": {a: built[a][5] for a in built},
          "limitations": ["Two iterations, workers 1/2 only; no convergence, performance, full-workspace or 32-worker acceptance",
              "One canonical state per case; eight original state hashes and byte equality are captured records, not eight retained streams",
              "Source archives and final executables retained; registry source, intermediate libraries, compiler DLL/sysroot/linker not retained or recompiled by this checker",
              "Cargo artifact profiles confirm opt3 runtime and opt0 host build tools; thin LTO/target-cpu are retained declarations, not independent disassembly proof",
              "Pinned repository checkout required; original host cache/target/run paths never opened",
              "No CFV, checkpoint, SOL wire or external solver certification"]}, indent=2))


if __name__ == "__main__":
    main()
