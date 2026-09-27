"""Eight existing generic tests against the frozen flat+EV native engine.

Preparation binds a completed native build; build/run are separate, explicit
phases. Every native command uses the existing fixed 512 MiB Job supervisor.
This checks correctness across worker counts, not performance scaling.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import traceback

HERE = Path(__file__).resolve().parent
NATIVE_DRIVER = HERE.parent / "native/run.py"
NATIVE_DRIVER_SHA = "f777056c8ca7d95795db2d5f19885ff4906ed09f8e87bfe776f0498576e5dd93"
ROOT = HERE.parents[4]
OUT = ROOT / "runs/flop-flat-ev-tests01"
TARGET = ROOT / "target/flop-flat-ev-tests01"
ENV = {"RAYON_NUM_THREADS": "2", "RUST_TEST_THREADS": "1"}
NAMES = {
    "parallel": [
        "parallel_chance_fanout_is_bitwise_deterministic",
        "selected_value_recording_preserves_ancestor_values_and_storage",
        "mapped_chance_and_action_siblings_preserve_f32_state",
        "mapped_chance_and_action_siblings_preserve_i16_state",
        "zero_mapped_chance_preserves_f32_state_and_all_values",
        "zero_mapped_chance_preserves_i16_state_and_all_values",
    ],
    "values": ["untrained_empty_and_variable_value_spaces_f32", "untrained_empty_and_variable_value_spaces_i16"],
}


def load():
    if hashlib.sha256(NATIVE_DRIVER.read_bytes()).hexdigest() != NATIVE_DRIVER_SHA:
        raise ValueError("Native driver changed")
    spec = importlib.util.spec_from_file_location("flat_ev_native_for_generic", NATIVE_DRIVER)
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    return native, native.load_shared()


def inputs(shared):
    manifest = shared.read(HERE / "inputs.json")
    shared.need(manifest["schema"] == "r1-flat-ev-generic-inputs/v1", "Input schema differs")
    snapshots = {str(HERE / name): value for name, value in manifest["snapshots"].items()}
    originals = {str(ROOT / name): value for name, value in manifest["originals"].items()}
    shared.unchanged(snapshots)
    shared.unchanged(originals)
    generated = (HERE / "inputs/parallel.rs").read_bytes() + b"\n" + (HERE / "inputs/zero_chance_tests.rs.in").read_bytes()
    shared.need(generated == (HERE / "inputs/parallel_zero.rs").read_bytes(), "Generated fixture differs")
    return snapshots, originals


def completed_native(native, shared):
    plan = shared.read(native.OUT / "plan.json")
    native.validate_plan(shared, plan)
    built = shared.read(native.OUT / "build.json")
    shared.need(built["status"] == "completed" and built["plan"] == shared.pin(native.OUT / "plan.json"), "Native build not completed")
    shared.need(len(built["stages"]) == 5, "Native build stage count differs")
    engine = built["stages"][0]["artifact"]
    shared.need(engine["path"] == str(native.TARGET / "libengine.rlib"), "Native engine path differs")
    shared.need(shared.pin(engine["path"]) == {k: engine[k] for k in ("bytes", "sha256")}, "Native engine bytes differ")
    for stage in built["stages"]:
        shared.need(stage["wrapper_exit_code"] == 0, "Native stage failed")
        record = native.OUT / stage["name"] / "record.json"
        shared.need(shared.pin(record) == stage["record"], "Native build record differs")
    return plan, engine


def jobs(native_plan, engine):
    # Preserve all release flags and both dependency search paths verbatim.
    engine_argv = native_plan["build_jobs"][0]["argv"]
    flags = engine_argv[:engine_argv.index("--crate-name")]
    result = []
    for name, source in (("parallel", "parallel_zero.rs"), ("values", "value_scratch.rs")):
        binary = TARGET / f"{name}.exe"
        argv = flags + ["--test", "--crate-name", f"flat_ev_generic_{name}", "-C", f"metadata=r1_flat_ev_generic_{name}"]
        for dep in ("cards", "rayon"):
            argv += ["--extern", f"{dep}={native_plan['reused_externs'][dep]}"]
        argv += ["--extern", f"engine={engine['path']}", str(HERE / "inputs" / source), "-o", str(binary)]
        result.append({"name": name, "argv": argv, "artifact": str(binary),
                       "run_argv": [str(binary), "--test-threads=1", *NAMES[name]], "expected_tests": NAMES[name]})
    return result


def prepare(native, shared):
    shared.need(not OUT.exists() and not TARGET.exists(), "Preparation requires new output and target directories")
    snapshots, originals = inputs(shared)
    native_plan, engine = completed_native(native, shared)
    controls = dict(native_plan["controls"])
    for path in (Path(__file__), HERE / "inputs.json", NATIVE_DRIVER,
                 native.OUT / "plan.json", native.OUT / "build.json", Path(engine["path"])):
        controls[str(path)] = shared.pin(path)
    for stage in shared.read(native.OUT / "build.json")["stages"]:
        path = native.OUT / stage["name"] / "record.json"
        controls[str(path)] = shared.pin(path)
    OUT.mkdir(parents=True)
    TARGET.mkdir(parents=True)
    # Exact native archive plus the separately tracked fixture snapshot.
    shutil.copyfile(native.OUT / "source.tar.gz", OUT / "source.tar.gz")
    plan = {"schema": "r1-flat-ev-generic/v1", "scope": "eight existing generic correctness tests, no performance claim",
            "compiler": native_plan["compiler"], "compiler_version": native_plan["compiler_version"],
            "native_plan": {"path": str(native.OUT / "plan.json"), **shared.pin(native.OUT / "plan.json")},
            "native_build": {"path": str(native.OUT / "build.json"), **shared.pin(native.OUT / "build.json")},
            "native_engine": engine, "environment": ENV, "limits": shared.LIMITS,
            "dependency_directory": native_plan["dependency_directory"], "dependency_pins": native_plan["dependency_pins"],
            "snapshot_pins": {**native_plan["snapshot_pins"], **snapshots},
            "current_source_pins": {**native_plan["current_source_pins"], **originals},
            "controls": controls, "source_archive": shared.pin(OUT / "source.tar.gz"),
            "test_inputs": snapshots, "jobs": jobs(native_plan, engine)}
    shared.unchanged(plan["current_source_pins"])
    shared.save(OUT / "plan.json", plan)
    validate(native, shared, plan)
    print(json.dumps({"prepared": str(OUT), "native_processes_launched": 0, "jobs": len(plan["jobs"]),
                      "expected_tests": sum(len(j["expected_tests"]) for j in plan["jobs"])}))


def validate(native, shared, plan):
    shared.need(plan["schema"] == "r1-flat-ev-generic/v1" and plan["environment"] == ENV, "Generic plan differs")
    shared.need(plan["limits"] == shared.LIMITS, "Resource bounds differ")
    native_plan, engine = completed_native(native, shared)
    shared.need(plan["jobs"] == jobs(native_plan, engine) and plan["native_engine"] == engine, "Native test graph differs")
    shared.validate_inputs(OUT, plan)
    shared.unchanged(plan["current_source_pins"])
    snapshots, _ = inputs(shared)
    shared.need(plan["test_inputs"] == snapshots, "Test input binding differs")


def execute(native, shared, phase):
    plan = shared.read(OUT / "plan.json")
    validate(native, shared, plan)
    path = OUT / ("build.json" if phase == "build" else "execution.json")
    shared.need(not path.exists(), "Phase already attempted; retries are not automatic")
    if phase == "build":
        shared.need(not any(TARGET.iterdir()), "Test target is not empty")
    else:
        built = shared.read(OUT / "build.json")
        shared.need(built["status"] == "completed" and built["plan"] == shared.pin(OUT / "plan.json"), "Test build incomplete")
        shared.need(len(built["stages"]) == 2, "Test build count differs")
        for item in built["stages"]:
            artifact = item["artifact"]
            shared.need(shared.pin(artifact["path"]) == {k: artifact[k] for k in ("bytes", "sha256")}, "Test binary changed")
    receipt = {"status": "running", "plan": shared.pin(OUT / "plan.json"), "environment": ENV, "stages": []}
    shared.save(path, receipt)
    previous_env = {name: os.environ.get(name) for name in ENV}
    os.environ.update(ENV)
    try:
        for index, job in enumerate(plan["jobs"]):
            validate(native, shared, plan)
            argv = job["argv"] if phase == "build" else job["run_argv"]
            directory, item = shared.stage(OUT, plan, f"{phase}-{index:02d}-{job['name']}", argv, receipt)
            if phase == "build":
                item["artifact"] = {"path": job["artifact"], **shared.pin(job["artifact"])}
            else:
                output = (directory / "record.stdout.log").read_text(encoding="utf-8")
                passed = re.findall(r"^test (\S+) \.\.\. ok$", output, re.MULTILINE)
                shared.need(sorted(passed) == sorted(job["expected_tests"]), "Successful test names differ")
                summary = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", output)
                shared.need(summary == [(str(len(passed)), "0", "0")], "Raw test counts differ")
                item["tests_passed"] = passed
            shared.save(path, receipt)
        validate(native, shared, plan)
        receipt["status"] = "completed"
    except Exception as error:
        receipt.update({"status": "failed", "error": str(error), "traceback": traceback.format_exc()})
        raise
    finally:
        for name, value in previous_env.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value
        shared.save(path, receipt)
    print(json.dumps({"phase": phase, "status": receipt["status"], "stages": len(receipt["stages"])}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "build", "run"))
    args = parser.parse_args()
    native, shared = load()
    if args.phase == "prepare":
        prepare(native, shared)
    else:
        shared.need(os.name == "nt", "Windows bounded wrapper required")
        execute(native, shared, args.phase)


if __name__ == "__main__":
    main()
