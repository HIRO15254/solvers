"""Portable allocation-proof byte/provenance checks; never executes retained code."""
from __future__ import annotations

import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import struct
import tarfile

import prepare
from run import HERE, PHASES, need, pin


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def norm(path: str) -> str:
    return path.replace("\\", "/")


def fields(item: dict) -> dict:
    return {key: item[key] for key in ["bytes", "sha256"]}


def stream_pin(stream) -> tuple[dict, bytes]:
    digest, size, prefix = hashlib.sha256(), 0, b""
    while block := stream.read(1024 * 1024):
        if not prefix:
            prefix = block[:256]
        digest.update(block)
        size += len(block)
    return {"bytes": size, "sha256": digest.hexdigest()}, prefix


def checked_files(directory: Path, manifest: dict) -> None:
    actual = {p.relative_to(directory).as_posix() for p in directory.rglob("*") if p.is_file()}
    need(actual == set(manifest["files"]) | {"manifest.json"}, "retained exact file set differs")
    for name, expected in manifest["files"].items():
        relative = PurePosixPath(name)
        need(not relative.is_absolute() and ".." not in relative.parts, "unsafe manifest path")
        path = directory / name
        need(not path.is_symlink() and pin(path) == expected, f"retained bytes differ: {name}")


def main() -> None:
    proof = HERE / "proof01"
    manifest = read(proof / "manifest.json")
    need(manifest["schema"] == "r1-allocation-probe-proof/v1", "schema differs")
    checked_files(proof, manifest)
    for name, expected in manifest["references_from_flop_scaling"].items():
        need(pin(HERE.parent / name) == expected, f"reference bytes differ: {name}")
    plan = read(proof / "plan.json")
    execution = read(proof / "execution.json")
    need(execution["plan"] == pin(proof / "plan.json"), "plan binding differs")
    need(execution["status"] == "completed" and execution["all_inputs_and_dependencies_unchanged"], "execution incomplete")
    need(len(execution["stages"]) == 8 and len(execution["runs"]) == 4, "stage count differs")
    generated, provenance = prepare.expected()
    need(read(HERE / "provenance.json") == provenance, "derived source provenance differs")
    for name, raw in generated.items():
        need((HERE / name).read_bytes() == raw, f"derived source differs: {name}")
    inputs = {norm(path): value for path, value in plan["inputs"].items()}
    for name in ["run.py", "prepare.py", "probe.rs", "selftest.rs", "allocator.rs.in", "selftest-body.rs.in", "provenance.json"]:
        matches = [value for path, value in inputs.items() if path.endswith("/alloc-probe/" + name)]
        need(matches == [pin(HERE / name)], f"current control/input differs: {name}")
    prior = {}
    for role, label in [("baseline", "base"), ("flat", "flat")]:
        archive = HERE.parent / f"optimized/proof01/{role}-source.tar.gz"
        receipt = HERE.parent / f"optimized/proof01/{role}-build/receipt.json"
        previous = read(receipt)
        prior[role] = previous
        need(previous["all_passed"] and previous["snapshot_unchanged"] and previous["original_sources_unchanged"], "prior source proof incomplete")
        need([value for path, value in inputs.items() if path.endswith(f"/runs/flop-opt-{label}-build01/receipt.json")] == [pin(receipt)], "prior receipt binding differs")
        expected = {norm(path).split(f"/.cache/flop-opt-{label}01/", 1)[-1]: value for path, value in inputs.items()
                    if f"/.cache/flop-opt-{label}01/" in path}
        found = {}
        with tarfile.open(archive, "r:gz") as tar:
            for member in tar:
                need(member.isfile() and not member.name.startswith("/") and ".." not in PurePosixPath(member.name).parts, "unsafe source archive")
                need(member.name not in found, "duplicate source member")
                found[member.name] = stream_pin(tar.extractfile(member))[0]
        need(found == expected, "source archive/snapshot exact set differs")
    native = read(HERE.parent / "native-solve/proof01/manifest.json")
    with gzip.open(HERE.parent / "native-solve/proof01/shared-state.bin.gz", "rb") as stream:
        shared, prefix = stream_pin(stream)
    need(shared == native["shared_state_uncompressed"] == manifest["shared_state_uncompressed"], "shared state bytes differ")
    need(prefix[:8] == b"R1F32S01", "state magic differs")
    need(struct.unpack("<8Q", prefix[8:72]) == (2, 2, 367662, 147104, 34, 30, 10176768, 10176768), "state shape differs")
    need(len(manifest["original_states_deduplicated"]) == 4 and all(value == shared for value in manifest["original_states_deduplicated"].values()), "deduplicated state pins differ")
    binary_pins = {}
    for name, expected in manifest["binaries_uncompressed"].items():
        with gzip.open(proof / f"{name}.exe.gz", "rb") as stream:
            binary_pins[name] = stream_pin(stream)[0]
        need(binary_pins[name] == expected, f"binary bytes differ: {name}")
    mappings = manifest["raw_mappings"]
    for raw, mapping in mappings.items():
        need(pin(proof / mapping["retained"]) == fields(mapping), f"raw mapping mismatch: {raw}")
    need(len(set(item["retained"] for item in mappings.values())) == len(mappings), "duplicate retained aliases")
    summaries = []
    reference_quality = (HERE.parent / "optimized/proof01/matrix/narrow-baseline-1/output/quality.json").read_bytes()
    expected_names = ["00-selftest-build", "01-selftest-run", "02-baseline-build", "03-flat-build", "04-baseline-1", "05-flat-1", "06-baseline-2", "07-flat-2"]
    need([s["name"] for s in execution["stages"]] == expected_names, "fixed order differs")
    for stage, job in zip(execution["stages"], plan["jobs"], strict=True):
        name = stage["name"]
        directory = proof / name
        record = read(directory / "record.json")
        need(stage["wrapper_exit_code"] == 0 and stage["record"] == pin(directory / "record.json"), "record binding differs")
        need(record["argv"] == job["argv"] == stage["command"][stage["command"].index("--") + 1:], "command binding differs")
        need(record["state"] == "completed" and record["child_exit_code"] == record["supervisor_exit_code"] == 0, "stage failed")
        need(record["cleanup_complete"] and not record["forced"] and record["identity_unchanged"] and record["last_sample"]["pids"] == [], "cleanup or identity differs")
        need(record["identity_before"] == record["identity_after"], "identity snapshots differ")
        need(record["bounded_job_settings"] == {"limit_flags": 8704, "job_memory_limit_bytes": 536870912, "root_priority_class": 16384, "verified_before_resume": True}, "queried Job differs")
        need(record["limits"]["timeout_seconds"] == 60 and record["limits"]["memory_limit_bytes"] == 469762048
             and record["limits"]["min_free_memory_bytes"] == 1610612736 and record["limits"]["disk_reserve_bytes"] == 1073741824, "bounds differ")
        for output in record["outputs"].values():
            need(pin(directory / norm(output["path"]).rsplit("/", 1)[-1]) == fields(output), "raw supervisor output differs")
        identity = {norm(item["path"]): fields(item) for item in record["identity_before"]}
        executable = norm(record["argv"][0])
        if name.endswith("build"):
            need(identity[executable] == fields(prior["baseline"]["compiler"]), "compiler identity differs")
            artifact_role = name.split("-")[1]
            need(fields(stage["artifact"]) == binary_pins[artifact_role], "build binary identity differs")
            if artifact_role != "selftest":
                argv = record["argv"]
                externs = {argv[i + 1].split("=", 1)[0]: argv[i + 1].split("=", 1)[1] for i, x in enumerate(argv) if x == "--extern"}
                need(set(externs) == {"cards", "engine", "game", "holdem", "rayon"}, "extern semantic set differs")
                label = "base" if artifact_role == "baseline" else "flat"
                dependencies = {norm(path): value for path, value in plan["searched_dependency_files"].items()}
                for path in externs.values():
                    need(norm(path) in dependencies and f"/target/flop-opt-{label}01/release/deps/" in norm(path), "extern search binding differs")
        else:
            binary_role = "selftest" if name == "01-selftest-run" else name.split("-")[1]
            need(identity[executable] == binary_pins[binary_role], "run binary identity differs")
        events = [json.loads(line) for line in (directory / "record.stdout.log").read_text(encoding="utf-8").splitlines()] if not name.endswith("build") else []
        if name == "01-selftest-run":
            need(events == execution["selftest"] and events[-1] == {"selftest": "passed", "null_failure_injection": False, "concurrency_stress": False}, "selftest result differs")
            keys = ["alloc_calls", "alloc_requested_bytes", "alloc_failed_calls", "alloc_zeroed_calls", "alloc_zeroed_requested_bytes", "alloc_zeroed_failed_calls", "realloc_calls", "realloc_requested_new_bytes", "realloc_old_layout_bytes", "realloc_failed_calls", "dealloc_calls", "dealloc_layout_bytes"]
            need([events[0][key] for key in keys] == [1, 64, 0, 1, 32, 0, 1, 128, 64, 0, 2, 160], "selftest numeric gate differs")
        elif name[:2] >= "04":
            run = next(r for r in execution["runs"] if r["name"] == name)
            artifact_dir = directory / "artifacts"
            need((artifact_dir / "quality.json").read_bytes() == reference_quality, "quality bytes differ")
            need(run["outputs"]["state.bin"] == shared and run["state_reference_bytes_equal"] and run["quality_reference_bytes_equal"], "recorded direct byte check differs")
            for filename, expected in run["outputs"].items():
                if filename != "state.bin":
                    need(pin(artifact_dir / filename) == expected, "artifact pin differs")
            counts = [event for event in events if event.get("event") == "allocation_counts"]
            need(counts == run["counts"] and [c["phase"] for c in counts] == PHASES, "count/phase closure differs")
            expected_events = []
            for count in counts:
                need(all(count[k] == 0 for k in ["alloc_failed_calls", "alloc_zeroed_failed_calls", "realloc_failed_calls"]), "failed allocation observed")
                expected_events += [{"phase": count["phase"], "status": "started"}, count, {"phase": count["phase"], "status": "completed"}]
            expected_events.append({"phase": "probe", "status": "completed"})
            need(events == expected_events, "event ordering differs")
            def aggregate(selected: list[dict]) -> dict:
                keys = ["alloc_calls", "alloc_zeroed_calls", "realloc_calls", "dealloc_calls", "alloc_requested_bytes", "alloc_zeroed_requested_bytes", "realloc_requested_new_bytes"]
                result = {key: sum(c[key] for c in selected) for key in keys}
                result["requested_total_bytes"] = sum(result[k] for k in keys[-3:])
                return result
            summaries.append({"name": name, "cfr": aggregate([c for c in counts if c["phase"] == "cfr"]),
                              "quality_7_walks": aggregate([c for c in counts if c["phase"] in PHASES[4:]])})
    print(json.dumps({"status": "verified", "stages": 8, "solves": 4, "payload_files": len(manifest["files"]),
                      "shared_state": shared, "quality": prepare.pin(reference_quality), "summaries": summaries,
                      "scope": "retained byte/provenance consistency and observed allocation counts; not a hermetic rebuild or performance result"}, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
