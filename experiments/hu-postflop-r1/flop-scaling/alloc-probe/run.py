"""Sequential bounded allocation diagnostic; fixed local paths, no retries."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback

import prepare

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
OUT = ROOT / "runs/flop-alloc01"
TARGET = ROOT / "target/flop-alloc01"
WRAPPER = HERE.parent / "native-preflight/run_bounded.py"
FIXTURE = HERE.parent / "fixtures/narrow.toml"
REFERENCE = ROOT / "runs/flop-opt-matrix01/narrow-baseline-1/output"
PHASES = ["build", "solver_allocation", "cfr", "state_write", "ev_p0", "ev_p1", "br_p0", "br_p1", "exploitability"]


def pin(path: Path) -> dict:
    digest, size = hashlib.sha256(), 0
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
            size += len(block)
    return {"bytes": size, "sha256": digest.hexdigest()}


def save(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")


def need(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def unchanged(pins: dict) -> None:
    for path, expected in pins.items():
        need(pin(Path(path)) == expected, f"input changed: {path}")


def same_bytes(a: Path, b: Path) -> bool:
    with a.open("rb") as left, b.open("rb") as right:
        while True:
            x, y = left.read(1024 * 1024), right.read(1024 * 1024)
            if x != y:
                return False
            if not x:
                return True


def main() -> None:
    need(os.name == "nt", "Windows bounded wrapper required")
    need(not OUT.exists() and not TARGET.exists(), "new fixed output and target directories required")
    OUT.mkdir(parents=True)
    TARGET.mkdir(parents=True)
    receipt = {"schema": "r1-allocation-diagnostic/v1", "status": "preparing", "stages": [], "runs": []}
    inputs = {}
    dependency_pins = {}
    reference_pins = {}
    try:
        generated, provenance = prepare.expected()
        for name, expected in generated.items():
            need((HERE / name).read_bytes() == expected, f"generated source changed: {name}")
        need(json.loads((HERE / "provenance.json").read_text()) == provenance, "provenance changed")
        for path in [HERE / "run.py", HERE / "prepare.py", HERE / "probe.rs", HERE / "selftest.rs",
                     HERE / "allocator.rs.in", HERE / "selftest-body.rs.in", HERE / "provenance.json",
                     prepare.BASE, WRAPPER, ROOT / "tools/run_supervised.py", FIXTURE]:
            inputs[str(path)] = pin(path)
        builds = {}
        dependencies = {}
        for role, label in [("baseline", "base"), ("flat", "flat")]:
            prior = ROOT / f"runs/flop-opt-{label}-build01/receipt.json"
            raw = json.loads(prior.read_text())
            need(raw["all_passed"] and raw["snapshot_unchanged"] and raw["original_sources_unchanged"], "prior optimized build failed")
            inputs[str(prior)] = pin(prior)
            for category in ["original_source_pins", "snapshot_pins"]:
                for path, expected in raw[category].items():
                    actual_path = ROOT / path
                    need(pin(actual_path) == expected, f"prior source changed: {path}")
                    inputs[str(actual_path)] = expected
            depdir = ROOT / f"target/flop-opt-{label}01/release/deps"
            inventory = sorted(p for p in depdir.iterdir() if p.suffix in {".rlib", ".dll", ".rmeta"})
            need(bool(inventory), "empty dependency search directory")
            for path in inventory:
                dependency_pins[str(path)] = pin(path)
            deps = {}
            for name in ["cards", "engine", "game", "holdem", "rayon"]:
                found = list(depdir.glob(f"lib{name}-*.rlib"))
                need(len(found) == 1, f"ambiguous exact extern: {role}/{name}")
                deps[name] = found[0]
            dependencies[role] = {"directory": depdir, "externs": deps}
            builds[role] = raw
        rustc = Path(builds["baseline"]["compiler"]["path"])
        need(builds["baseline"]["compiler"] == builds["flat"]["compiler"], "compiler identities differ")
        need(pin(rustc) == {k: builds["baseline"]["compiler"][k] for k in ["bytes", "sha256"]}, "compiler bytes changed")
        inputs[str(rustc)] = pin(rustc)
        inputs[str(Path(sys.executable))] = pin(Path(sys.executable))
        for name in ["state.bin", "quality.json", "result.json", "invocation.json"]:
            reference_pins[str(REFERENCE / name)] = pin(REFERENCE / name)
        need(reference_pins[str(REFERENCE / "state.bin")]["bytes"] == 81_414_344, "reference state size differs")
        need(json.loads((REFERENCE / "result.json").read_text())["status"] == "completed", "reference not completed")
        common = [str(rustc), "--edition=2024", "-C", "opt-level=3", "-C", "lto=thin",
                  "-C", "codegen-units=1", "-C", "target-cpu=native", "-C", "debuginfo=0"]
        selftest = TARGET / "selftest.exe"
        jobs = [
            ("00-selftest-build", common + ["--crate-name", "allocation_selftest", str(HERE / "selftest.rs"), "-o", str(selftest)], selftest),
            ("01-selftest-run", [str(selftest)], None),
        ]
        binaries = {}
        for index, role in enumerate(["baseline", "flat"], 2):
            binary = TARGET / f"{role}.exe"
            dep = dependencies[role]
            command = common + ["--crate-name", "allocation_probe", "-L", f"dependency={dep['directory']}"]
            for name, path in dep["externs"].items():
                command += ["--extern", f"{name}={path}"]
            command += [str(HERE / "probe.rs"), "-o", str(binary)]
            jobs.append((f"{index:02d}-{role}-build", command, binary))
            binaries[role] = binary
        for index, (role, workers) in enumerate([("baseline", 1), ("flat", 1), ("baseline", 2), ("flat", 2)], 4):
            name = f"{index:02d}-{role}-{workers}"
            jobs.append((name, [str(binaries[role]), "narrow", str(workers), "2", str(OUT / name / "output")], None))
        plan = {"schema": receipt["schema"], "scope": "phase allocation counts and exact state/quality only; no performance or memory claim",
                "inputs": inputs, "searched_dependency_files": dependency_pins, "reference_outputs": reference_pins,
                "jobs": [{"name": name, "argv": argv, "artifact": str(artifact) if artifact else None} for name, argv, artifact in jobs],
                "limits": {"wall_seconds_per_stage": 60, "job_commit_bytes": 536870912, "sampled_rss_bytes": 469762048,
                           "host_commit_and_free_reserve_bytes": 1610612736, "disk_reserve_bytes": 1073741824,
                           "priority": "below_normal", "sequential": True, "retry": False},
                "rustc_version": builds["baseline"]["rustc_version"]}
        save(OUT / "plan.json", plan)
        receipt["plan"] = pin(OUT / "plan.json")
        receipt["status"] = "running"
        save(OUT / "execution.json", receipt)
        for name, argv, artifact in jobs:
            unchanged(inputs)
            unchanged(dependency_pins)
            stage_dir = OUT / name
            stage_dir.mkdir()
            command = [sys.executable, "-B", str(WRAPPER), "--record", str(stage_dir / "record.json"),
                       "--cwd", str(ROOT), "--timeout-seconds", "60", "--grace-seconds", "0.2",
                       "--kill-wait-seconds", "5", "--poll-seconds", "0.1", "--memory-limit-bytes", "469762048",
                       "--min-free-memory-bytes", "1610612736", "--disk-reserve-bytes", "1073741824",
                       "--disk-path", str(stage_dir)]
            for path in [HERE / "run.py", HERE / "probe.rs", HERE / "selftest.rs", HERE / "provenance.json", FIXTURE, OUT / "plan.json"]:
                command += ["--identity-file", str(path)]
            command += ["--", *argv]
            with (stage_dir / "wrapper.stdout.log").open("wb") as stdout, (stage_dir / "wrapper.stderr.log").open("wb") as stderr:
                proc = subprocess.run(command, stdout=stdout, stderr=stderr, check=False)
            stage = {"name": name, "command": command, "wrapper_exit_code": proc.returncode}
            receipt["stages"].append(stage)
            save(OUT / "execution.json", receipt)
            need(proc.returncode == 0, f"stage failed: {name}")
            record = json.loads((stage_dir / "record.json").read_text())
            need(record["state"] == "completed" and record["child_exit_code"] == 0 and record["supervisor_exit_code"] == 0,
                 f"record not completed: {name}")
            need(record["cleanup_complete"] and not record["forced"] and record["identity_unchanged"], f"cleanup/identity failed: {name}")
            need(record["last_sample"]["pids"] == [], f"remaining PIDs: {name}")
            need(record["bounded_job_settings"] == {"limit_flags": 8704, "job_memory_limit_bytes": 536870912,
                 "root_priority_class": 16384, "verified_before_resume": True}, f"bounds differ: {name}")
            for output in record["outputs"].values():
                need(pin(Path(output["path"])) == {k: output[k] for k in ["bytes", "sha256"]}, "raw record output changed")
            unchanged(inputs)
            unchanged(dependency_pins)
            stage["record"] = pin(stage_dir / "record.json")
            if artifact:
                stage["artifact"] = {"path": str(artifact), **pin(artifact)}
            elif name == "01-selftest-run":
                lines = [json.loads(line) for line in (stage_dir / "record.stdout.log").read_text().splitlines()]
                need(lines[-1] == {"selftest": "passed", "null_failure_injection": False, "concurrency_stress": False}, "selftest output mismatch")
                receipt["selftest"] = lines
            elif name[0:2] >= "04":
                output = stage_dir / "output"
                result = json.loads((output / "result.json").read_text())
                need(result["status"] == "completed" and result["iterations"] == 2 and result["case"] == "narrow", "probe not completed")
                for field in ["state.bin", "quality.json"]:
                    need(same_bytes(output / field, REFERENCE / field), f"reference byte mismatch: {name}/{field}")
                events = [json.loads(line) for line in (stage_dir / "record.stdout.log").read_text().splitlines()]
                counts = [line for line in events if line.get("event") == "allocation_counts"]
                ordinary = [line for line in events if "event" not in line]
                expected = [{"phase": phase, "status": status} for phase in PHASES for status in ["started", "completed"]]
                expected.append({"phase": "probe", "status": "completed"})
                need(ordinary == expected and [line["phase"] for line in counts] == PHASES, "phase closure differs")
                for count in counts:
                    need(count["schema"] == "r1-phase-allocation/v1", "count schema differs")
                    need(all(type(v) is int and 0 <= v < 2**64 for k, v in count.items() if k not in {"event", "schema", "phase"}), "counter not u64")
                    need(all(count[k] == 0 for k in ["alloc_failed_calls", "alloc_zeroed_failed_calls", "realloc_failed_calls"]), "allocation failure observed")
                receipt["runs"].append({"name": name, "outputs": {p.name: pin(p) for p in sorted(output.iterdir()) if p.is_file()},
                                        "state_reference_bytes_equal": True, "quality_reference_bytes_equal": True, "counts": counts})
            save(OUT / "execution.json", receipt)
            print(json.dumps({"stage": name, "status": "completed"}), flush=True)
        unchanged(reference_pins)
        receipt["status"] = "completed"
        receipt["all_inputs_and_dependencies_unchanged"] = True
    except BaseException as error:
        receipt["status"] = "failed"
        receipt["error"] = repr(error)
        receipt["traceback"] = traceback.format_exc()
        save(OUT / "execution.json", receipt)
        raise
    finally:
        save(OUT / "execution.json", receipt)


if __name__ == "__main__":
    main()
