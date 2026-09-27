"""Flat+EV diagnostic wrapper around the unchanged, hash-pinned EV machinery."""
from __future__ import annotations

import argparse
import contextlib
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import tarfile
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
FLOP = HERE.parents[1]
ROOT = FLOP.parents[2]
OUT = ROOT / "runs/flop-flat-ev01"
TARGET = ROOT / "target/flop-flat-ev01"
SHARED = FLOP / "ev-scratch/run.py"
SHARED_SHA = "14f6fd5e95777aa1c7758b7ae99045b2f33452e5381b62a3a43efd6368b3da5d"
EV_SHA = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"
CANDIDATE_SHA = "ec9e5e6b5230684fce6f2315370ceda0e05fb4346eb7e4ff44ea05e6028aabcd"
SOURCE = "crates/engine/src/solver.rs"


def load_shared():
    raw = SHARED.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SHARED_SHA:
        raise ValueError("shared driver exact SHA differs")
    spec = importlib.util.spec_from_file_location("pinned_ev_scratch_runtime", SHARED)
    shared = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(shared)
    return shared


def jobs(shared, plan):
    result = shared.build_jobs(plan)
    shared.need([job["name"] for job in result] == ["engine", "game", "holdem", "ordinary", "instrumented"], "shared job layout differs")
    for job in result[:3]:
        old = "metadata=r1_ev_scratch_" + job["name"]
        shared.need(job["argv"].count(old) == 1, "metadata replacement is not unique")
        job["argv"] = ["metadata=r1_flat_ev01_" + job["name"] if arg == old else arg for arg in job["argv"]]
    for job in result[3:]:
        old = "ev_scratch_" + job["name"]
        shared.need(job["argv"].count(old) == 1, "adapter crate-name replacement is not unique")
        job["argv"] = ["flat_ev01_" + job["name"] if arg == old else arg for arg in job["argv"]]
    return result


def archive(source, destination, snapshot):
    with destination.open("wb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w|") as tar:
            for name in sorted(snapshot):
                path = Path(name)
                info = tarfile.TarInfo(path.relative_to(source).as_posix())
                info.size, info.mode, info.mtime = path.stat().st_size, 0o644, 0
                with path.open("rb") as stream:
                    tar.addfile(info, stream)


def validate_plan(shared, plan):
    shared.need(plan["schema"] == "r1-flat-ev-native/v1", "plan schema differs")
    shared.need(Path(plan["source"]) == OUT / "source" and Path(plan["target"]) == TARGET, "fixed paths differ")
    shared.need(plan["build_jobs"] == jobs(shared, plan), "fresh build graph differs")
    shared.need(plan["conditions"] == [[case, kind, n] for case in ("narrow", "expanded")
                                      for kind in ("ordinary", "instrumented") for n in (1, 2)], "8-condition order differs")
    shared.need(plan["iterations"] == 2 and plan["limits"] == shared.LIMITS, "fixed iteration/bounds differ")
    replacement = plan["snapshot_override"]
    shared.need(replacement["path"] == SOURCE and replacement["production"]["sha256"] == EV_SHA
                and replacement["candidate"]["sha256"] == CANDIDATE_SHA, "override pins differ")
    shared.need(plan["snapshot_pins"][str(OUT / "source" / SOURCE)] == replacement["candidate"], "compiled source pin differs")
    shared.need(plan["current_source_pins"][str(ROOT / SOURCE)] == replacement["production"], "production identity was replaced")
    shared.need(shared.pin(OUT / "solver.patch") == plan["patch"], "patch changed")
    shared.validate_inputs(OUT, plan)


def prepare(shared):
    candidate = HERE.parent / "solver.rs"
    provenance_path = HERE.parent / "provenance.json"
    provenance = shared.read(provenance_path)
    production_pin, candidate_pin = shared.pin(ROOT / SOURCE), shared.pin(candidate)
    shared.need(production_pin["sha256"] == EV_SHA and candidate_pin["sha256"] == CANDIDATE_SHA, "production/candidate SHA differs")
    for key, actual in (("source", production_pin), ("candidate", candidate_pin)):
        shared.need(actual == {k: provenance[key][k] for k in ("bytes", "sha256")}, "candidate provenance mismatch")
    patch = HERE.parent / "candidate.patch"
    shared.need(shared.pin(patch) == {k: provenance["patch"][k] for k in ("bytes", "sha256")}, "candidate patch differs")
    # The shared preparer captures real production EV identity. Its initial
    # snapshot is adjusted below before any native process can be launched.
    with contextlib.redirect_stdout(io.StringIO()):
        shared.prepare(SimpleNamespace(out=OUT, target=TARGET, solver_sha256=EV_SHA))
    plan = shared.read(OUT / "plan.json")
    source = Path(plan["source"])
    copied_solver = source / SOURCE
    shared.need(shared.pin(copied_solver) == production_pin, "initial copied EV solver differs")
    shutil.copyfile(candidate, copied_solver)
    snapshot = {str(path): shared.pin(path) for path in sorted(source.rglob("*")) if path.is_file()}
    changed = [name for name, value in snapshot.items() if value != plan["snapshot_pins"].get(name)]
    shared.need(set(snapshot) == set(plan["snapshot_pins"]) and changed == [str(copied_solver)], "unexpected snapshot change")
    archive(source, OUT / "source.tar.gz", snapshot)
    shutil.copyfile(patch, OUT / "solver.patch")
    plan.update({"schema": "r1-flat-ev-native/v1", "scope": "combined flat chance outputs + EV scratch; exact baseline state/quality and allocation diagnosis only",
                 "snapshot_pins": snapshot, "source_archive": shared.pin(OUT / "source.tar.gz"),
                 "patch": shared.pin(OUT / "solver.patch"),
                 "patch_base": "production EV source " + EV_SHA,
                 "snapshot_override": {"path": SOURCE, "production": production_pin, "candidate": candidate_pin},
                 "shared_driver": {"path": str(SHARED), **shared.pin(SHARED)}})
    for path in (Path(__file__), candidate, provenance_path, patch, HERE.parent / "prepare.py", OUT / "solver.patch"):
        plan["controls"][str(path)] = shared.pin(path)
    plan["build_jobs"] = jobs(shared, plan)
    shared.unchanged(plan["current_source_pins"])
    shared.save(OUT / "plan.json", plan)
    validate_plan(shared, plan)
    print(json.dumps({"prepared": str(OUT), "native_processes_launched": 0, "source_files": len(snapshot),
                      "source_archive": plan["source_archive"], "plan": shared.pin(OUT / "plan.json"),
                      "candidate": candidate_pin, "builds": 5, "solves": 8}, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "build", "run"))
    args = parser.parse_args()
    shared = load_shared()
    if args.phase == "prepare":
        prepare(shared)
    else:
        shared.need(os.name == "nt", "Windows bounded wrapper required")
        validate_plan(shared, shared.read(OUT / "plan.json"))
        shared.execute(SimpleNamespace(phase=args.phase, out=OUT))


if __name__ == "__main__":
    main()
