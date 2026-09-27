"""Independent portable checks of retained bytes/bindings; no retained-code execution."""
import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile

HERE = Path(__file__).resolve().parent
FLOP = HERE.parent
PHASES = ["build", "solver_allocation", "cfr", "state_write", "ev_p0", "ev_p1", "br_p0", "br_p1", "exploitability"]


def need(value, message):
    if not value:
        raise ValueError(message)


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def stream_pin(stream):
    digest, size = hashlib.sha256(), 0
    while block := stream.read(1024 * 1024):
        digest.update(block)
        size += len(block)
    return {"bytes": size, "sha256": digest.hexdigest()}


def pin(path):
    with path.open("rb") as stream:
        return stream_pin(stream)


def fields(item):
    return {k: item[k] for k in ("bytes", "sha256")}


def norm(path):
    return str(path).replace("\\", "/")


def archive_pins(path):
    result = {}
    with tarfile.open(path, "r:gz") as tar:
        for member in tar:
            name = PurePosixPath(member.name)
            need(member.isfile() and not name.is_absolute() and ".." not in name.parts and member.name not in result, "unsafe source member")
            result[member.name] = stream_pin(tar.extractfile(member))
    return result


def main():
    proof = HERE / "proof01"
    manifest = read(proof / "manifest.json")
    need(manifest["schema"] == "r1-ev-scratch-proof/v1", "schema differs")
    files = {p.relative_to(proof).as_posix() for p in proof.rglob("*") if p.is_file()}
    need(files == set(manifest["files"]) | {"manifest.json"}, "retained file set differs")
    for name, expected in manifest["files"].items():
        need(not PurePosixPath(name).is_absolute() and ".." not in PurePosixPath(name).parts, "unsafe payload name")
        need(not (proof / name).is_symlink() and pin(proof / name) == expected, f"payload changed: {name}")
    for name, expected in manifest["original_raw_files"].items():
        need(manifest["files"][name] == expected, "raw mapping differs")
    for name, expected in manifest["references_from_flop_scaling"].items():
        need(pin(FLOP / name) == expected, f"reference changed: {name}")
    plan, build, execution = [read(proof / name) for name in ("plan.json", "build.json", "execution.json")]
    need(build["plan"] == execution["plan"] == pin(proof / "plan.json"), "plan binding differs")
    need(execution["build"] == pin(proof / "build.json"), "build binding differs")
    need(build["status"] == execution["status"] == "completed" and build["original_sources_unchanged"] and execution["original_sources_unchanged"], "execution incomplete")
    need(len(build["stages"]) == 5 and len(execution["stages"]) == 8 and plan["iterations"] == 2, "fixed matrix differs")
    need(plan["source_differences"] == ["crates/engine/src/solver.rs"], "unexpected declared source change")
    need(pin(proof / "source.tar.gz") == plan["source_archive"] and pin(proof / "solver.patch") == plan["patch"], "source/patch binding differs")
    source = archive_pins(proof / "source.tar.gz")
    expected = {norm(path).removeprefix(norm(plan["source"]) + "/"): value for path, value in plan["snapshot_pins"].items()}
    need(source == expected, "snapshot exact file set differs")
    old = read(FLOP / "optimized/proof01/baseline-build/receipt.json")
    need(plan["compiler"] == old["compiler"], "compiler differs from release baseline")
    previous = {norm(path): value for path, value in old["original_source_pins"].items()}
    changed = [name for name, value in source.items() if not name.startswith("adapters/") and value != previous.get(name)]
    need(changed == ["crates/engine/src/solver.rs"], "actual source differs beyond EV solver patch")
    for name, suffix in [("solve.rs", "native-solve/solve.rs"), ("probe.rs", "alloc-probe/probe.rs")]:
        need(source["adapters/" + name] == pin(FLOP / suffix), "adapter changed")
    controls = {norm(path): value for path, value in plan["controls"].items()}
    need([value for path, value in controls.items() if path.endswith("/ev-scratch/run.py")] == [pin(HERE / "run.py")], "driver changed")
    dependencies = {norm(path): value for path, value in plan["dependency_pins"].items()}
    historical = read(FLOP / "alloc-probe/proof01/plan.json")["searched_dependency_files"]
    baseline_dependencies = {norm(path): value for path, value in historical.items() if "/flop-opt-base01/" in norm(path)}
    need(dependencies == baseline_dependencies, "reused release library pins differ")
    binaries = {}
    for name, expected in manifest["binary_uncompressed"].items():
        with gzip.open(proof / f"{name}.exe.gz", "rb") as stream:
            binaries[name] = stream_pin(stream)
        need(binaries[name] == expected, "binary expansion differs")
    known = dict(dependencies)
    for stage, job in zip(build["stages"], plan["build_jobs"], strict=True):
        name = job["name"]
        artifact = stage["artifact"]
        need(artifact["path"] == job["artifact"], "build artifact path differs")
        argv = job["argv"]
        need(stage["argv"][stage["argv"].index("--") + 1:] == argv, "build plan command differs")
        externs = dict(argv[i + 1].split("=", 1) for i, v in enumerate(argv) if v == "--extern")
        names = {"engine": {"cards", "rayon", "rand", "rand_chacha"}, "game": {"cards", "engine"},
                 "holdem": {"cards", "engine", "game", "hand_index"}}.get(name, {"cards", "engine", "game", "holdem", "rayon"})
        need(set(externs) == names and all(norm(path) in known for path in externs.values()), "extern graph is unbound")
        for dep in names & {"engine", "game", "holdem"}:
            need(norm(externs[dep]) == norm(plan["target"]) + f"/lib{dep}.rlib", "old affected dependency reused")
        known[norm(artifact["path"])] = fields(artifact)
        if name in binaries:
            need(fields(artifact) == binaries[name], "measured binary differs from built artifact")
    quality, states = {}, {}
    for case in ("narrow", "expanded"):
        relative = "native-solve/proof01/shared-state.bin.gz" if case == "narrow" else "optimized/proof01/expanded-state.bin.gz"
        with gzip.open(FLOP / relative, "rb") as stream:
            states[case] = stream_pin(stream)
        need(states[case] == plan["references"][case]["state"], "canonical state differs")
        quality[case] = (FLOP / f"optimized/proof01/matrix/{case}-baseline-1/output/quality.json").read_bytes()
    summaries = []
    conditions = [[case, kind, workers] for case in ("narrow", "expanded") for kind in ("ordinary", "instrumented") for workers in (1, 2)]
    need(plan["conditions"] == conditions, "fixed condition order differs")
    for stage in build["stages"] + execution["stages"]:
        directory = proof / stage["name"]
        record = read(directory / "record.json")
        need(stage["wrapper_exit_code"] == 0 and stage["record"] == pin(directory / "record.json"), "record binding differs")
        need(record["argv"] == stage["argv"][stage["argv"].index("--") + 1:], "invocation differs")
        need(record["state"] == "completed" and record["child_exit_code"] == record["supervisor_exit_code"] == 0, "unsuccessful record")
        need(record["identity_before"] == record["identity_after"] and record["identity_unchanged"], "identity changed")
        need(record["cleanup_complete"] and not record["forced"] and record["last_sample"]["pids"] == [], "cleanup incomplete")
        need(record["bounded_job_settings"] == {"limit_flags": 8704, "job_memory_limit_bytes": 536870912,
             "root_priority_class": 16384, "verified_before_resume": True}, "Job settings differ")
        for key, expected in plan["limits"].items():
            need(record["limits"][key] == expected, "fixed stage limit differs")
        for payload in record["outputs"].values():
            need(pin(directory / norm(payload["path"]).split("/")[-1]) == fields(payload), "raw supervisor output differs")
        identities = {norm(item["path"]): fields(item) for item in record["identity_before"]}
        executable = norm(record["argv"][0])
        need(identities[executable] == (fields(plan["compiler"]) if stage["name"].startswith("build-") else known[executable]), "executable identity differs")
        need([v for p, v in identities.items() if p.endswith("/flop-ev-scratch01/plan.json")] == [pin(proof / "plan.json")], "record plan identity differs")
        need([v for p, v in identities.items() if p.endswith("/ev-scratch/run.py")] == [pin(HERE / "run.py")], "record driver identity differs")
    for stage, (case, kind, workers) in zip(execution["stages"], conditions, strict=True):
        need(stage["name"] == f"run-{case}-{kind}-{workers}", "run order differs")
        artifact_dir = proof / stage["name"] / "artifacts"
        argv = stage["argv"][stage["argv"].index("--") + 1:]
        need(norm(argv[0]) == norm(plan["target"]) + f"/{kind}.exe" and argv[1:4] == [case, str(workers), "2"], "run command differs")
        result = read(artifact_dir / "result.json")
        need(result["status"] == "completed" and result["case"] == case and result["threads"] == workers and result["iterations"] == 2, "result differs")
        state = manifest["deduplicated_original_states"][stage["name"] + "/artifacts/state.bin"]
        need(fields(state) == stage["outputs"]["state.bin"] == states[case] and state["case"] == case, "state dedup binding differs")
        need(stage["fullstate_and_quality_bytes_equal_baseline"], "direct byte equality not recorded")
        need((artifact_dir / "quality.json").read_bytes() == quality[case], "quality raw bytes differ")
        for name, expected in stage["outputs"].items():
            if name != "state.bin":
                need(pin(artifact_dir / name) == expected, "artifact hash differs")
        events = [json.loads(line) for line in (proof / stage["name"] / "record.stdout.log").read_text(encoding="utf-8").splitlines()]
        expected_events = []
        counts = stage.get("counts", [])
        need(len(counts) == (9 if kind == "instrumented" else 0), "count phase count differs")
        for index, phase in enumerate(PHASES):
            expected_events.append({"phase": phase, "status": "started"})
            if counts:
                need(counts[index]["phase"] == phase and counts[index]["schema"] == "r1-phase-allocation/v1", "count schema differs")
                need(all(counts[index][key] == 0 for key in ("alloc_failed_calls", "alloc_zeroed_failed_calls", "realloc_failed_calls")), "allocation failure observed")
                expected_events.append(counts[index])
            expected_events.append({"phase": phase, "status": "completed"})
        need(events == expected_events + [{"phase": "probe", "status": "completed"}], "phase event closure differs")
        if counts:
            quality_counts = counts[4:]
            keys = ["alloc_calls", "alloc_zeroed_calls", "realloc_calls", "dealloc_calls", "alloc_requested_bytes", "alloc_zeroed_requested_bytes", "realloc_requested_new_bytes"]
            total = {key: sum(c[key] for c in quality_counts) for key in keys}
            total["requested_total_bytes"] = sum(total[key] for key in keys[-3:])
            summaries.append({"case": case, "workers": workers, "quality_7_walks": total})
    print(json.dumps({"status": "verified", "payload_files": len(files) - 1, "source_files": len(source),
                      "builds": 5, "solves": 8, "states_deduplicated": len(manifest["deduplicated_original_states"]),
                      "canonical_states": states, "allocation_summaries": summaries,
                      "scope": "retained source/command/byte consistency; not performance, convergence or hermetic rebuild evidence"}, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
