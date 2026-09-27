"""Verify an immutable completed case without reading mutable/later-case receipts.

Only trusted sibling analyze.py is imported. Full retained state streams are read
when invoked on actual proof; tiny unit tests use synthetic metadata only.
"""
from __future__ import annotations

import argparse
import gzip
import json
from pathlib import Path, PurePosixPath

import analyze as base


class CaseEvidence(base.Evidence):
    def __init__(self, root, case):
        base.need(case in base.CASES, "unknown case")
        requested = Path(root).absolute()
        base.need(not requested.is_symlink(), "proof root symlink")
        self.root = requested.resolve(strict=True)
        self.files, self.verified, self.state_cache = {}, set(), {}
        self.checkpoint_path = self.path(f"checkpoints/{case}.json")
        self.checkpoint = record = base.read(self.checkpoint_path)
        base.need(record["schema"] == "r1.research-case-checkpoint/v1"
                  and record["case"] == case and record["status"] == "case_complete"
                  and record["full_matrix_complete"] is False, "case checkpoint identity differs")
        self.origin = PurePosixPath(record["plan"]["path"]).parent
        base.need(self.origin.is_absolute() and ".." not in self.origin.parts,
                  "original proof root differs")
        for ref in record["files"]:
            name = self.name(ref["path"])
            base.relative(name)
            base.need(name not in self.files, "duplicate immutable reference")
            base.need(name not in {"execution.json", "retained.json"}
                      and not name.startswith(("recovery/", "checkpoints/")),
                      "mutable or unrelated receipt reference")
            identity = base.pair(ref)
            base.need(type(identity["bytes"]) is int and 0 <= identity["bytes"] <= 1024**3
                      and isinstance(identity["sha256"], str)
                      and len(identity["sha256"]) == 64
                      and all(c in "0123456789abcdef" for c in identity["sha256"]),
                      "invalid immutable content identity")
            self.files[name] = identity
        base.need(self.name(record["plan"]["path"]) == "plan.json"
                  and self.name(record["build"]["path"]) == "build.json", "plan/build path differs")
        self.plan = base.read(self.require(record["plan"]))
        self.build = base.read(self.require(record["build"]))
        base.need(self.plan["output"] == str(self.origin)
                  and record["boot_id"] == self.plan["host"]["boot_id"], "checkpoint boot/root differs")

    def path(self, name):
        path = super().path(name)
        base.need(all(not p.is_symlink() for p in [path, *path.parents]), "evidence symlink")
        return path

    def require(self, value):
        path = super().require(value)
        name = self.name(value["path"])
        if name not in self.verified:
            base.need(path.is_file() and base.pin(path) == self.files[name],
                      "immutable payload missing or hash differs: " + name)
            self.verified.add(name)
        return path

    def bound(self, name):
        base.need(name in self.files, "immutable payload reference missing: " + name)
        return self.require({"path": str(self.origin / name), **self.files[name]})

    def control(self, suffix):
        original, value, path = super().control(suffix)
        self.bound(value["retained"])
        return original, value, path

    def snapshot(self, stage):
        actual = base.read(self.bound(stage["name"] + "/completed.json"))
        base.need(actual == stage, "completed stage snapshot differs")


def fixed_case_rows(rows, case):
    expected = [r for r in base.schedule() if r["case"] == case]
    base.need(len(rows) == 32, "expected 32 case rows")
    for actual, wanted in zip(rows, expected):
        base.need(all(actual.get(k) == v for k, v in wanted.items())
                  and actual["status"] == "completed", "case schedule/status/order differs")


def verify_build(evidence):
    build, plan = evidence.build, evidence.plan
    base.need(build["schema"] == "r1.worker-scratch-cloud32-phase/v1"
              and build["phase"] == "build" and build["status"] == "completed"
              and build["plan"] == evidence.files["plan.json"]
              and build["counts"] == {"completed": 8, "failed": 0, "skipped": 0}
              and not build["pilot_stages"], "completed build binding differs")
    names = ["toolchain", "build-baseline", "build-worker", "tests-worker",
             "smoke-narrow-baseline-1", "smoke-narrow-worker-1",
             "smoke-narrow-baseline-32", "smoke-narrow-worker-32"]
    stages, binaries = build["stages"], build["binaries"]
    base.need([s["name"] for s in stages] == names and set(binaries) == set(base.ARMS),
              "build stage/binary set differs")
    for stage in stages:
        evidence.snapshot(stage)
    tool = stages[0]
    base.need(tool["kind"] == "toolchain"
              and tool["command"] == [plan["tools"]["rustc"]["path"], "-Vv"], "toolchain command differs")
    base.verify_stage(evidence, tool)
    record = base.read(evidence.require(tool["record"]))
    version = evidence.require(record["outputs"]["stdout"]).read_text()
    base.need(version == build["rustc_version"] and "release: 1.97.0" in version
              and "host: x86_64-unknown-linux-gnu" in version, "native compiler version differs")
    for stage, arm in zip(stages[1:3], base.ARMS):
        target = str(PurePosixPath(plan["workspace"]) / f"{arm}-target")
        base.need(stage["kind"] == "build" and stage["arm"] == arm
                  and stage["command"] == [plan["tools"]["cargo"]["path"], "build", "--locked", "--offline",
                                           "--release", "-j2", "--target-dir", target, "-p", "holdem",
                                           "--example", "flop_cloud32_probe", "--message-format=json"],
                  "native build command differs")
        base.verify_stage(evidence, stage)
        base.need(stage["artifact"] == binaries[arm]
                  and binaries[arm]["path"] == target + "/release/examples/flop_cloud32_probe",
                  "native binary path differs")
        with gzip.open(evidence.require(stage["retained_binary"]), "rb") as stream:
            base.need(base.digest(stream, 64 * 1024**2) == base.pair(binaries[arm]), "native binary bytes differ")
    test = stages[3]
    base.need(test["kind"] == "tests" and test["arm"] == "worker"
              and test["command"] == [plan["tools"]["cargo"]["path"], "test", "--locked", "--offline", "--release", "-j2",
                                      "--target-dir", str(PurePosixPath(plan["workspace"]) / "worker-target"),
                                      "-p", "engine", "-p", "holdem", "-p", "cfr-ref", "--tests"],
              "candidate tests command differs")
    base.verify_stage(evidence, test)
    canonical = None
    for stage, (arm, workers) in zip(stages[4:], (("baseline", 1), ("worker", 1), ("baseline", 32), ("worker", 32))):
        base.need(stage["kind"] == "smoke"
                  and (stage["case"], stage["iterations"], stage["arm"], stage["workers"]) == ("narrow", 2, arm, workers),
                  "smoke conditions differ")
        canonical = base.verify_solve(evidence, stage, binaries, canonical)
    base.need(build["smoke_canonical"] == canonical, "smoke canonical differs")
    base.need(base.utc(build["started_at"]) <= base.utc(stages[0]["started_at"])
              and base.utc(stages[-1]["verified_at"]) <= base.utc(build["ended_at"])
              <= base.utc(plan["deadline_utc"]), "build timestamps differ")
    return binaries


def verify_pilots(evidence, case, binaries, iterations):
    base.need(iterations in base.PILOTS, "case iteration count is not a frozen pilot")
    stages, canonical = [], None
    for n in base.PILOTS:
        if n > iterations:
            # No larger pilot may have completed after the first qualifying one.
            base.need(f"pilot-{case}-{n}/completed.json" not in evidence.files,
                      "pilot ran after selected iteration count")
            continue
        stage = base.read(evidence.bound(f"pilot-{case}-{n}/completed.json"))
        base.need(stage["name"] == f"pilot-{case}-{n}"
                  and (stage["case"], stage["iterations"], stage["arm"], stage["workers"])
                  == (case, n, "baseline", 1) and stage["status"] == "completed"
                  and stage["warmup"] is False and stage["round"] is None,
                  "baseline pilot conditions differ")
        canonical = base.verify_solve(evidence, stage, binaries)
        seconds = stage["result"]["cfr_seconds"]
        base.need((seconds < 4 and n < 128) if n < iterations else (seconds >= 4 or n == 128),
                  "pilot is not the first baseline threshold/cap")
        stages.append(stage)
    return stages, canonical


def case_statistics(rows):
    # Descriptive case-only arithmetic: deliberately no call to summarize(),
    # which requires both inputs and produces the campaign adoption guard.
    groups = []
    for arm in base.ARMS:
        for workers in base.WORKERS:
            selected = [s for s in rows if s["arm"] == arm and s["workers"] == workers and not s["warmup"]]
            base.need([s["round"] for s in selected] == [1, 2, 3], "case measured rounds differ")
            groups.append({"arm": arm, "workers": workers,
                           "cfr_plus_quality_seconds": base.stats([s["result"]["cfr_seconds"] + s["result"]["quality_seconds"] for s in selected]),
                           "root_os_peak_resident_bytes": base.stats([s["root_os_peak_resident_bytes"] for s in selected])})
    return groups


def verify_case(evidence):
    base.verify_sources(evidence)
    case, rows = evidence.checkpoint["case"], evidence.checkpoint["stages"]
    fixed_case_rows(rows, case)
    binaries = verify_build(evidence)
    iterations = rows[0]["iterations"]
    pilots, canonical = verify_pilots(evidence, case, binaries, iterations)
    for stage in rows:
        base.need(stage["iterations"] == iterations, "case iterations changed")
        evidence.snapshot(stage)
        base.verify_solve(evidence, stage, binaries, canonical)
    ordered = evidence.build["stages"] + pilots + rows
    base.need(all(base.utc(a["verified_at"]) <= base.utc(b["started_at"])
                  for a, b in zip(ordered, ordered[1:])), "required stages overlap or order differs")
    base.need(base.utc(evidence.build["ended_at"]) <= base.utc(pilots[0]["started_at"]),
              "case pilot preceded completed build phase")
    return {"schema": "r1.worker-scratch-cloud32-case-analysis/v1", "status": "case_verified",
            "case": case, "boot_id": evidence.checkpoint["boot_id"], "iterations": iterations,
            "full_matrix_complete": False, "guard": {"decision": "not_evaluable"},
            "counts": {"build_prerequisites": 8, "required_pilots": len(pilots), "case_rows": 32,
                       "warmup": 8, "measured": 24},
            "checkpoint": base.pin(evidence.checkpoint_path),
            "required_payload_integrity": "verified", "verified_payloads": len(evidence.verified),
            "unneeded_manifest_references_not_read": len(evidence.files) - len(evidence.verified),
            "unique_state_streams_verified": len(evidence.state_cache), "groups": case_statistics(rows),
            "limitations": ["One immutable completed case; no campaign adoption decision or external quality claim.",
                            "Mutable receipts and unrelated case/wrapper payloads are neither read nor required.",
                            "Removed duplicate state equality relies on the pinned runner's exact comparison receipt; retained canonical streams are fully rehashed."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--case", choices=base.CASES, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    base.need(not args.out.exists() and not args.out.resolve().is_relative_to(args.proof.resolve()),
              "new report outside proof required")
    report = verify_case(CaseEvidence(args.proof, args.case))
    report["reader"] = base.pin(Path(__file__))
    report["trusted_analyzer"] = base.pin(Path(base.__file__))
    with args.out.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(report, stream, indent=2, allow_nan=False)
        stream.write("\n")
    print(json.dumps({"status": report["status"], "case": args.case, "guard": "not_evaluable"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
