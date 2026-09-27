"""Prepare/build/run the isolated EV-scratch diagnostic; phases never auto-chain."""
from __future__ import annotations

import argparse
import difflib
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import traceback

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
FLOP = HERE.parent
WRAPPER = FLOP / "native-preflight/run_bounded.py"
BASE = FLOP / "optimized/proof01"
ALLOC = FLOP / "alloc-probe"
CRATES = ("cards", "engine", "game", "hand-index", "holdem")
SOURCE_CHANGE = "crates/engine/src/solver.rs"
BASE_DEPS = ROOT / "target/flop-opt-base01/release/deps"
LIMITS = {"timeout_seconds": 60, "grace_seconds": 0.2, "kill_wait_seconds": 5,
          "poll_seconds": 0.1, "memory_limit_bytes": 469762048,
          "min_free_memory_bytes": 1610612736, "disk_reserve_bytes": 1073741824}


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(path):
    digest, length = hashlib.sha256(), 0
    with Path(path).open("rb") as stream:
        while data := stream.read(1024 * 1024):
            digest.update(data)
            length += len(data)
    return {"bytes": length, "sha256": digest.hexdigest()}


def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def save(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")


def unchanged(pins):
    for name, expected in pins.items():
        need(pin(name) == expected, f"pinned input changed: {name}")


def child(path, parent):
    path = path.resolve()
    need(path != parent and path.is_relative_to(parent), f"must be a child of {parent}")
    return path


def inventory(directory):
    return {str(p): pin(p) for p in sorted(directory.iterdir()) if p.suffix in {".rlib", ".rmeta", ".dll"}}


def build_jobs(plan):
    target, source = Path(plan["target"]), Path(plan["source"])
    deps = {name: Path(path) for name, path in plan["reused_externs"].items()}
    flags = [plan["compiler"]["path"], "--edition=2024", "-C", "opt-level=3", "-C", "lto=thin",
             "-C", "codegen-units=1", "-C", "target-cpu=native", "-C", "debuginfo=0",
             "-C", "debug-assertions=no", "-C", "overflow-checks=no", "-C", "embed-bitcode=yes",
             "-L", f"dependency={target}", "-L", f"dependency={plan['dependency_directory']}"]
    jobs = []
    for name, required in [("engine", ("cards", "rayon", "rand", "rand_chacha")),
                           ("game", ("cards", "engine")),
                           ("holdem", ("cards", "engine", "game", "hand_index"))]:
        artifact = target / f"lib{name}.rlib"
        argv = flags + ["--crate-name", name, "--crate-type", "rlib", "-C", f"metadata=r1_ev_scratch_{name}"]
        for dep in required:
            argv += ["--extern", f"{dep}={deps[dep]}"]
        argv += [str(source / f"crates/{name}/src/lib.rs"), "-o", str(artifact)]
        jobs.append({"name": name, "argv": argv, "artifact": str(artifact)})
        deps[name] = artifact
    for name, adapter in [("ordinary", "solve.rs"), ("instrumented", "probe.rs")]:
        artifact = target / f"{name}.exe"
        argv = flags + ["--crate-name", f"ev_scratch_{name}"]
        for dep in ("cards", "engine", "game", "holdem", "rayon"):
            argv += ["--extern", f"{dep}={deps[dep]}"]
        argv += [str(source / "adapters" / adapter), "-o", str(artifact)]
        jobs.append({"name": name, "argv": argv, "artifact": str(artifact)})
    return jobs


def prepare(args):
    out, target = child(args.out, ROOT / "runs"), child(args.target, ROOT / "target")
    need(not out.exists() and not target.exists(), "prepare requires fresh output and target directories")
    candidate = ROOT / SOURCE_CHANGE
    need(pin(candidate)["sha256"] == args.solver_sha256, "candidate solver pin differs")
    baseline = read(BASE / "baseline-build/receipt.json")
    need(baseline["all_passed"] and baseline["snapshot_unchanged"] and baseline["original_sources_unchanged"], "baseline proof incomplete")
    originals = [ROOT / name for name in ["Cargo.toml", "Cargo.lock", ".cargo/config.toml"]]
    for crate in CRATES:
        originals += [ROOT / f"crates/{crate}/Cargo.toml"]
        originals += sorted(p for p in (ROOT / f"crates/{crate}/src").rglob("*") if p.is_file())
    old = {name.replace("\\", "/"): value for name, value in baseline["original_source_pins"].items()}
    changed = [p.relative_to(ROOT).as_posix() for p in originals if pin(p) != old.get(p.relative_to(ROOT).as_posix())]
    need(changed == [SOURCE_CHANGE], f"candidate includes unexpected source differences: {changed}")
    # Reuse the exact searched release inputs from the successful allocation build.
    old_dependencies = read(ALLOC / "proof01/plan.json")["searched_dependency_files"]
    wanted = {str(BASE_DEPS / Path(name.replace("\\", "/")).name): value for name, value in old_dependencies.items()
              if "/target/flop-opt-base01/release/deps/" in name.replace("\\", "/")}
    need(inventory(BASE_DEPS) == wanted, "release dependency inventory changed")
    externs = {}
    for name in ("cards", "hand_index", "rayon", "rand", "rand_chacha"):
        found = list(BASE_DEPS.glob(f"lib{name}-*.rlib"))
        need(len(found) == 1, f"ambiguous reused extern {name}")
        externs[name] = str(found[0])
    compiler = baseline["compiler"]
    need(pin(compiler["path"]) == {k: compiler[k] for k in ("bytes", "sha256")}, "compiler changed")
    adapter_paths = {"solve.rs": FLOP / "native-solve/solve.rs", "probe.rs": ALLOC / "probe.rs"}
    transform = read(ALLOC / "provenance.json")
    need(pin(adapter_paths["solve.rs"]) == transform["inputs"]["../native-solve/solve.rs"], "ordinary adapter differs")
    need(pin(adapter_paths["probe.rs"]) == transform["generated"]["probe.rs"], "allocation adapter differs")
    controls = [Path(__file__), WRAPPER, ROOT / "tools/run_supervised.py", BASE / "manifest.json",
                BASE / "baseline-build/receipt.json", BASE / "baseline-source.tar.gz", ALLOC / "proof01/plan.json",
                ALLOC / "provenance.json", Path(compiler["path"]), Path(sys.executable)] + list(adapter_paths.values())
    controls += [FLOP / f"fixtures/{case}.toml" for case in ("narrow", "expanded")]
    references = {}
    for case in ("narrow", "expanded"):
        gzip_path = FLOP / "native-solve/proof01/shared-state.bin.gz" if case == "narrow" else BASE / "expanded-state.bin.gz"
        quality = BASE / f"matrix/{case}-baseline-1/output/quality.json"
        references[case] = {"state_gzip": str(gzip_path), "compressed": pin(gzip_path),
                            "state": read(BASE / "manifest.json")["shared_state_uncompressed"][case],
                            "quality_path": str(quality), "quality": pin(quality)}
        controls += [gzip_path, quality]
    out.mkdir(parents=True)
    target.mkdir(parents=True)
    source = out / "source"
    for original in originals:
        destination = source / original.relative_to(ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(original, destination)
    (source / "adapters").mkdir()
    for name, path in adapter_paths.items():
        shutil.copyfile(path, source / "adapters" / name)
    snapshot = {str(p): pin(p) for p in sorted(source.rglob("*")) if p.is_file()}
    archive = out / "source.tar.gz"
    with archive.open("wb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w|") as tar:
            for name in snapshot:
                path = Path(name)
                info = tarfile.TarInfo(path.relative_to(source).as_posix())
                info.size, info.mode, info.mtime = path.stat().st_size, 0o644, 0
                with path.open("rb") as stream:
                    tar.addfile(info, stream)
    with tarfile.open(BASE / "baseline-source.tar.gz", "r:gz") as tar:
        old_solver = tar.extractfile(SOURCE_CHANGE).read().decode("utf-8").splitlines(True)
    patch = "".join(difflib.unified_diff(old_solver, candidate.read_text(encoding="utf-8").splitlines(True),
                                        fromfile="baseline/" + SOURCE_CHANGE, tofile="candidate/" + SOURCE_CHANGE))
    (out / "solver.patch").write_text(patch, encoding="utf-8", newline="\n")
    plan = {"schema": "r1-ev-scratch-diagnostic/v1", "source": str(source), "target": str(target),
            "scope": "EV scratch reuse only; baseline comparison by retained exact fullstate/quality; no performance claim",
            "source_differences": changed, "compiler": compiler, "compiler_version": baseline["rustc_version"],
            "current_source_pins": {str(p): pin(p) for p in originals}, "snapshot_pins": snapshot,
            "controls": {str(p): pin(p) for p in controls}, "dependency_directory": str(BASE_DEPS),
            "dependency_pins": wanted, "reused_externs": externs, "source_archive": pin(archive),
            "patch": pin(out / "solver.patch"), "references": references, "limits": LIMITS,
            "conditions": [[case, kind, workers] for case in ("narrow", "expanded")
                           for kind in ("ordinary", "instrumented") for workers in (1, 2)], "iterations": 2}
    plan["build_jobs"] = build_jobs(plan)
    unchanged(plan["current_source_pins"])
    save(out / "plan.json", plan)
    print(json.dumps({"prepared": str(out), "source_files": len(snapshot), "jobs": len(plan["build_jobs"]),
                      "conditions": len(plan["conditions"]), "solver": pin(candidate), "archive": pin(archive)}))


def validate_inputs(out, plan):
    for key in ("snapshot_pins", "controls", "dependency_pins"):
        unchanged(plan[key])
    need(inventory(Path(plan["dependency_directory"])) == plan["dependency_pins"], "dependency file set changed")
    need(pin(out / "source.tar.gz") == plan["source_archive"], "source archive changed")


def stage(out, plan, name, argv, receipt):
    validate_inputs(out, plan)
    directory = out / name
    directory.mkdir()
    cmd = [sys.executable, "-B", str(WRAPPER), "--record", str(directory / "record.json"), "--cwd", str(ROOT)]
    for name, value in LIMITS.items():
        cmd += ["--" + name.replace("_", "-"), str(value)]
    cmd += ["--disk-path", str(directory), "--identity-file", str(out / "plan.json"), "--identity-file", str(HERE / "run.py"), "--", *argv]
    result = subprocess.run(cmd, capture_output=True, check=False)
    (directory / "wrapper.stdout.log").write_bytes(result.stdout)
    (directory / "wrapper.stderr.log").write_bytes(result.stderr)
    item = {"name": directory.name, "argv": cmd, "wrapper_exit_code": result.returncode}
    receipt["stages"].append(item)
    need(result.returncode == 0, f"bounded stage failed: {directory.name}")
    record = read(directory / "record.json")
    need(record["state"] == "completed" and record["child_exit_code"] == record["supervisor_exit_code"] == 0,
         "supervisor not completed")
    need(record["identity_before"] == record["identity_after"] and record["identity_unchanged"], "identity changed")
    need(record["cleanup_complete"] and not record["forced"] and record["last_sample"]["pids"] == [], "cleanup incomplete")
    need(record["bounded_job_settings"] == {"limit_flags": 8704, "job_memory_limit_bytes": 536870912,
         "root_priority_class": 16384, "verified_before_resume": True}, "queried Job settings differ")
    for payload in record["outputs"].values():
        need(pin(payload["path"]) == {k: payload[k] for k in ("bytes", "sha256")}, "raw supervisor output differs")
    validate_inputs(out, plan)
    item["record"] = pin(directory / "record.json")
    return directory, item


def execute(args):
    out = child(args.out, ROOT / "runs")
    plan = read(out / "plan.json")
    filename = "build.json" if args.phase == "build" else "execution.json"
    need(not (out / filename).exists(), "phase already attempted; retries are not automatic")
    receipt = {"status": "running", "plan": pin(out / "plan.json"), "stages": []}
    save(out / filename, receipt)
    try:
        if args.phase == "build":
            need(not any(Path(plan["target"]).iterdir()), "build target is not empty")
            for index, job in enumerate(plan["build_jobs"]):
                _, item = stage(out, plan, f"build-{index:02d}-{job['name']}", job["argv"], receipt)
                item["artifact"] = {"path": job["artifact"], **pin(job["artifact"])}
                save(out / filename, receipt)
        else:
            built = read(out / "build.json")
            need(built["status"] == "completed" and built["plan"] == pin(out / "plan.json"), "candidate build incomplete")
            artifacts = {s["artifact"]["path"]: {k: s["artifact"][k] for k in ("bytes", "sha256")} for s in built["stages"]}
            receipt["build"] = pin(out / "build.json")
            for case, kind, workers in plan["conditions"]:
                unchanged(artifacts)
                name = f"run-{case}-{kind}-{workers}"
                binary = Path(plan["target"]) / f"{kind}.exe"
                argv = [str(binary), case, str(workers), "2", str(out / name / "artifacts")]
                directory, item = stage(out, plan, name, argv, receipt)
                output = directory / "artifacts"
                result = read(output / "result.json")
                need(result["status"] == "completed" and result["case"] == case and result["threads"] == workers
                     and result["iterations"] == 2, "probe result differs")
                reference = plan["references"][case]
                with (output / "state.bin").open("rb") as left, gzip.open(reference["state_gzip"], "rb") as right:
                    while True:
                        a, b = left.read(1024 * 1024), right.read(1024 * 1024)
                        need(a == b, "fullstate differs from baseline")
                        if not a:
                            break
                need((output / "quality.json").read_bytes() == Path(reference["quality_path"]).read_bytes(), "quality bytes differ")
                item["outputs"] = {p.name: pin(p) for p in output.iterdir() if p.is_file()}
                need(item["outputs"]["state.bin"] == reference["state"], "reference state pin differs")
                item["fullstate_and_quality_bytes_equal_baseline"] = True
                if kind == "instrumented":
                    events = [json.loads(line) for line in (directory / "record.stdout.log").read_text(encoding="utf-8").splitlines()]
                    counts = [event for event in events if event.get("event") == "allocation_counts"]
                    need([event["phase"] for event in counts] == ["build", "solver_allocation", "cfr", "state_write", "ev_p0", "ev_p1", "br_p0", "br_p1", "exploitability"], "count phase closure differs")
                    need(all(c[k] == 0 for c in counts for k in ("alloc_failed_calls", "alloc_zeroed_failed_calls", "realloc_failed_calls")), "allocation failure observed")
                    item["counts"] = counts
                unchanged(artifacts)
                save(out / filename, receipt)
        receipt["status"] = "completed"
        receipt["original_sources_unchanged"] = all(pin(path) == expected for path, expected in plan["current_source_pins"].items())
        print(json.dumps({"phase": args.phase, "status": "completed", "stages": len(receipt["stages"])}))
    except BaseException as error:
        receipt["status"] = "failed"
        receipt["error"] = repr(error)
        receipt["traceback"] = traceback.format_exc()
        raise
    finally:
        save(out / filename, receipt)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "build", "run"))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--target", type=Path)
    parser.add_argument("--solver-sha256")
    args = parser.parse_args()
    if args.phase == "prepare":
        need(args.target is not None and args.solver_sha256 is not None, "prepare needs --target and --solver-sha256")
        prepare(args)
    else:
        need(os.name == "nt", "Windows bounded wrapper required")
        execute(args)


if __name__ == "__main__":
    main()
