"""Retain or independently verify the eight-test flat+EV generic proof.

Verification reads only the retained payloads. Toolchain and registry dependency
artifacts are identified by recorded pins; their complete installations are not
copied. No retained Python source or binary is executed during verification.
"""
from __future__ import annotations

import argparse
import ast
import gzip
import hashlib
import io
import json
import math
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
RAW = ROOT / "runs/flop-flat-ev-tests01"
NATIVE = ROOT / "runs/flop-flat-ev01"
TARGET = ROOT / "target/flop-flat-ev-tests01"
FLOP = HERE.parents[1]


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def safe(name):
    p = PurePosixPath(name)
    need(bool(name) and not p.is_absolute() and ".." not in p.parts and ":" not in name and str(p) == name, "Unsafe proof path")
    return name


def literal(source, name):
    for node in ast.parse(source.decode("utf-8")).body:
        if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == name for t in node.targets):
            return ast.literal_eval(node.value)
    raise ValueError(f"Missing literal {name}")


def retain(proof):
    need(not proof.exists() and proof.parent == HERE, "Use a fresh direct proof directory here")
    files = {"generic/" + p.relative_to(RAW).as_posix(): p for p in RAW.rglob("*") if p.is_file()}
    for name in ("plan.json", "build.json"):
        files["native/" + name] = NATIVE / name
    build = json.loads((NATIVE / "build.json").read_text(encoding="utf-8"))
    need(build["status"] == "completed" and len(build["stages"]) == 5, "Native build incomplete")
    engine_stage = build["stages"][0]
    for p in (NATIVE / engine_stage["name"]).iterdir():
        if p.is_file():
            files["native/" + engine_stage["name"] + "/" + p.name] = p
    files["binaries/engine.rlib"] = Path(engine_stage["artifact"]["path"])
    for name in ("parallel", "values"):
        files[f"binaries/{name}.exe"] = TARGET / f"{name}.exe"
    sources = [HERE / "run.py", HERE / "inputs.json", Path(__file__), *sorted((HERE / "inputs").iterdir()),
               HERE.parent / "native/run.py", FLOP / "ev-scratch/run.py", FLOP / "native-preflight/run_bounded.py",
               ROOT / "tools/run_supervised.py"]
    for p in sources:
        files["sources/" + p.relative_to(ROOT).as_posix()] = p
    # Stage records are immutable at this point; copy exact bytes, then verify.
    before = {name: pin(p.read_bytes()) for name, p in files.items()}
    proof.mkdir()
    manifest = {"schema": "r1-flat-ev-generic-proof/v1", "files": {}}
    for name, path in sorted(files.items()):
        data = path.read_bytes()
        need(pin(data) == before[name], "Input changed while retaining")
        packed = gzip.compress(data, compresslevel=1, mtime=0)
        destination = safe(name + ".gz")
        target = proof / destination
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(packed)
        manifest["files"][name] = {"path": destination, "original_path": str(path.resolve()),
                                    "original": pin(data), "compressed": pin(packed)}
    need(before == {name: pin(p.read_bytes()) for name, p in files.items()}, "Input changed after retention")
    (proof / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return verify(proof)


def verify(proof):
    manifest = json.loads((proof / "manifest.json").read_text(encoding="utf-8"))
    need(manifest["schema"] == "r1-flat-ev-generic-proof/v1", "Proof schema differs")
    blobs, absolute, paths = {}, {}, {"manifest.json"}
    for name, item in manifest["files"].items():
        safe(name)
        path = safe(item["path"])
        need(path not in paths and item["original_path"] not in absolute, "Duplicate payload identity")
        paths.add(path)
        packed = (proof / path).read_bytes()
        need(pin(packed) == item["compressed"], f"Compressed hash differs: {name}")
        data = gzip.decompress(packed)
        need(pin(data) == item["original"], f"Original hash differs: {name}")
        blobs[name] = data
        absolute[item["original_path"]] = data
    need(paths == {p.relative_to(proof).as_posix() for p in proof.rglob("*") if p.is_file()}, "Proof inventory differs")
    read = lambda name: json.loads(blobs[name])
    gp, np = read("generic/plan.json"), read("native/plan.json")
    gb, ge, nb = read("generic/build.json"), read("generic/execution.json"), read("native/build.json")
    need(gp["schema"] == "r1-flat-ev-generic/v1" and np["schema"] == "r1-flat-ev-native/v1", "Plan schema differs")
    need(gb["status"] == ge["status"] == nb["status"] == "completed", "A phase failed")
    need(len(gb["stages"]) == len(ge["stages"]) == 2 and len(nb["stages"]) == 5, "Stage counts differ")
    need(gb["plan"] == ge["plan"] == pin(blobs["generic/plan.json"]) and nb["plan"] == pin(blobs["native/plan.json"]), "Phase plan binding differs")
    for field, name in (("native_plan", "native/plan.json"), ("native_build", "native/build.json")):
        need({k: gp[field][k] for k in ("bytes", "sha256")} == pin(blobs[name]), f"{field} binding differs")
    need(gp["native_engine"] == nb["stages"][0]["artifact"], "Native engine receipt differs")
    need(pin(blobs["binaries/engine.rlib"]) == {k: gp["native_engine"][k] for k in ("bytes", "sha256")}, "Native engine bytes differ")
    archive = blobs["generic/source.tar.gz"]
    need(pin(archive) == gp["source_archive"] == np["source_archive"], "Native source archive differs")
    archived = {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        for member in tar:
            name = safe(member.name)
            need(member.isfile() and name not in archived, "Invalid archive member")
            archived[name] = tar.extractfile(member).read()
    source_paths = {str(PureWindowsPath(np["source"]) / name): pin(data) for name, data in archived.items()}
    need(source_paths == np["snapshot_pins"], "Full candidate source snapshot differs")
    need(pin(archived["crates/engine/src/solver.rs"])["sha256"] == "ec9e5e6b5230684fce6f2315370ceda0e05fb4346eb7e4ff44ea05e6028aabcd", "Candidate solver differs")
    inputs_name = next(name for name in blobs if name.endswith("generic-checks/inputs.json"))
    input_manifest = read(inputs_name)
    prefix = inputs_name.removesuffix("inputs.json")
    fixture_pins = {}
    for name, expected in input_manifest["snapshots"].items():
        need(pin(blobs[prefix + name]) == expected, "Fixture bytes differ")
        original_path = manifest["files"][prefix + name]["original_path"]
        fixture_pins[original_path] = expected
    need(fixture_pins == gp["test_inputs"], "Fixture plan binding differs")
    for name, expected in input_manifest["originals"].items():
        need(pin(blobs[prefix + "inputs/" + PurePosixPath(name).name]) == expected, "Original fixture pin differs")
    need(blobs[prefix + "inputs/parallel_zero.rs"] == blobs[prefix + "inputs/parallel.rs"] + b"\n" + blobs[prefix + "inputs/zero_chance_tests.rs.in"], "Fixture concatenation differs")
    names = literal(blobs[prefix + "run.py"], "NAMES")
    need([len(names[k]) for k in ("parallel", "values")] == [6, 2] and len(set(sum(names.values(), []))) == 8, "Eight-test selection differs")
    need(gp["environment"] == gb["environment"] == ge["environment"] == {"RAYON_NUM_THREADS": "2", "RUST_TEST_THREADS": "1"}, "Test environment differs")
    for path, data in absolute.items():
        if path in gp["controls"]:
            need(pin(data) == gp["controls"][path], "Retained control binding differs")

    def stage(prefix, item, expected_argv):
        r = read(prefix + "/record.json")
        need(pin(blobs[prefix + "/record.json"]) == item["record"], "Stage record pin differs")
        need(item["wrapper_exit_code"] == r["supervisor_exit_code"] == r["child_exit_code"] == 0, "Nonzero stage exit")
        need(r["state"] == r["stop_reason"] == "completed" and r["shell"] is False, "Abnormal stage completion")
        need(r["argv"] == r["resolved_argv"] == expected_argv, "Stage command differs")
        need(r["cleanup_complete"] is True and r["forced"] is False and r["last_sample"]["pids"] == [], "Incomplete cleanup")
        need(r["errors"] == r["events"] == [] and r["stop_requested_at"] is None, "Supervisor errors or stops")
        need(r["identity_before"] == r["identity_after"] and r["identity_unchanged"] is True, "Stage identity changed")
        need(r["containment"]["kind"] == "windows_job_kill_on_close_suspended_assignment", "Containment differs")
        need(r["bounded_job_settings"] == {"limit_flags": 8704, "job_memory_limit_bytes": 536870912,
                                          "root_priority_class": 16384, "verified_before_resume": True}, "Queried Job differs")
        expected_limits = {**gp["limits"], "hard_job_commit_limit_bytes": 536870912,
                           "minimum_available_commit_before_launch_bytes": 1610612736, "root_priority_class": 16384}
        need(r["limits"] == expected_limits and r["limits"]["timeout_seconds"] == 60, "Stage limits differ")
        need(r["host_before"]["commit_available_bytes"] >= 1610612736, "Launch commit reserve differs")
        for identity in r["identity_before"]:
            if identity["path"] in absolute:
                need(pin(absolute[identity["path"]]) == {k: identity[k] for k in ("bytes", "sha256")}, "Retained identity differs")
        for key, suffix in (("stdout", "record.stdout.log"), ("stderr", "record.stderr.log"), ("samples", "record.samples.jsonl")):
            item_pin = r["outputs"][key]
            need(pin(blobs[prefix + "/" + suffix]) == {k: item_pin[k] for k in ("bytes", "sha256")}, "Raw output pin differs")
            need(absolute[item_pin["path"]] == blobs[prefix + "/" + suffix], "Raw output path differs")
        samples = [json.loads(line) for line in blobs[prefix + "/record.samples.jsonl"].splitlines()]
        m = r["measurement"]
        need(len(samples) == m["sample_count"] and len(samples) >= 2 and samples[-1] == r["last_sample"], "Samples differ")
        elapsed = [s["elapsed_seconds"] for s in samples]
        need(all(math.isfinite(x) and x >= 0 for x in elapsed) and elapsed == sorted(elapsed), "Sample time differs")
        need(max(s["tree_resident_bytes"] for s in samples) == m["sampled_peak_tree_resident_bytes"], "Resident peak differs")
        need(max(len(s["pids"]) for s in samples) == m["max_observed_processes"], "Process count differs")
        need(math.isclose(max(b - a for a, b in zip(elapsed, elapsed[1:])), m["max_sample_gap_seconds"], abs_tol=1e-7), "Sample gap differs")
        need(elapsed[-1] <= r["elapsed_seconds"] <= 65.2, "Stage wall bound differs")
        return {"name": prefix, "elapsed_seconds": r["elapsed_seconds"], "sample_count": len(samples),
                "job_peak_commit_bytes": m["job_os_peak_commit_bytes"], "sampled_peak_resident_bytes": m["sampled_peak_tree_resident_bytes"]}

    rows = [stage("native/" + nb["stages"][0]["name"], nb["stages"][0], np["build_jobs"][0]["argv"])]
    flags = np["build_jobs"][0]["argv"]
    flags = flags[:flags.index("--crate-name")]
    passed = []
    for i, name in enumerate(("parallel", "values")):
        job = gp["jobs"][i]
        expected = flags + ["--test", "--crate-name", f"flat_ev_generic_{name}", "-C", f"metadata=r1_flat_ev_generic_{name}"]
        for dep in ("cards", "rayon"):
            expected += ["--extern", f"{dep}={np['reused_externs'][dep]}"]
        fixture = "parallel_zero.rs" if name == "parallel" else "value_scratch.rs"
        expected += ["--extern", f"engine={gp['native_engine']['path']}", manifest["files"][prefix + "inputs/" + fixture]["original_path"], "-o", job["artifact"]]
        need(job["argv"] == expected and job["run_argv"] == [job["artifact"], "--test-threads=1", *names[name]], "Generated test command differs")
        artifact = gb["stages"][i]["artifact"]
        need(artifact["path"] == job["artifact"] and pin(blobs[f"binaries/{name}.exe"]) == {k: artifact[k] for k in ("bytes", "sha256")}, "Test binary differs")
        rows.append(stage("generic/" + gb["stages"][i]["name"], gb["stages"][i], expected))
        test_prefix = "generic/" + ge["stages"][i]["name"]
        rows.append(stage(test_prefix, ge["stages"][i], job["run_argv"]))
        output = blobs[test_prefix + "/record.stdout.log"].decode("utf-8")
        tests = re.findall(r"^test (\S+) \.\.\. ok\r?$", output, re.MULTILINE)
        need(sorted(tests) == sorted(names[name]) == sorted(ge["stages"][i]["tests_passed"]), "Raw passed test names differ")
        need(re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", output) == [(str(len(tests)), "0", "0")], "Raw test result differs")
        passed += tests
    return {"status": "passed", "payload_files": len(blobs), "tests_passed": passed,
            "candidate_source": pin(archived["crates/engine/src/solver.rs"]), "native_engine": pin(blobs["binaries/engine.rlib"]),
            "archived_source_files": len(archived), "fixture_files": len(fixture_pins), "stages": rows,
            "scope": "Eight generic tests and reused engine build; compiler/dependency installation pins retained, no performance claim"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("retain", "verify"))
    parser.add_argument("proof", type=Path)
    args = parser.parse_args()
    result = retain(args.proof.resolve()) if args.phase == "retain" else verify(args.proof.resolve())
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
