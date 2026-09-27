"""Strict descriptive narrow-block reader for an interrupted Cloud32 campaign.

Imports only the pinned neighboring trusted analyzer. Never reconstructs damaged
JSON from logs, never edits original/stale manifests, and never emits a decision.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ANALYZER_SHA = "098375aa70c6e854882fba7b2e5b49fe01a146203c5b84cc793fd0222ebcac1e"
ANALYZER = HERE / "analyze.py"
if hashlib.sha256(ANALYZER.read_bytes()).hexdigest() != ANALYZER_SHA:
    raise ValueError("Trusted neighboring analyzer pin differs")
spec = importlib.util.spec_from_file_location("trusted_cloud32_partial_analyzer", ANALYZER)
a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a)
need, read, pair = a.need, a.read, a.pair


def proof_index(manifest, origin):
    """Select original proof members only; recovery/package is a separate scope."""
    need(manifest["schema"] == "r1.flat-ev-vm14-recovery/v1" and manifest["proof_present"] is True
         and manifest["unit_quiescence_checked"] is True, "recovery envelope differs")
    seen, files = set(), {}
    for row in manifest["files"]:
        name = a.relative(row["member"]).as_posix()
        need(name not in seen, "duplicate recovery member")
        seen.add(name)
        if name.startswith("recovery/"):
            need(not PurePosixPath(row["original"]).is_relative_to(origin),
                 "original proof file disguised as recovery-only member")
            continue
        need(name != "recovery-manifest.json", "reserved recovery member")
        need(row["original"] == str(origin / name), "original proof path binding differs")
        need(isinstance(row["bytes"], int) and 0 <= row["bytes"] <= 1024**3, "invalid recovered byte count")
        files[name] = pair(row)
    need({"plan.json", "build.json", "execution.json", "retained.json"}.issubset(files), "required original proof metadata missing")
    return files


class RecoveredEvidence(a.Evidence):
    def __init__(self, root, manifest_path):
        # Deliberately do not invoke Evidence.__init__: retained.json may be a
        # stale pre-matrix snapshot and must remain untouched.
        self.root = Path(root).resolve(strict=True)
        need(self.root.is_dir() and not self.root.is_symlink(), "regular proof root required")
        self.recovery_manifest = read(manifest_path)
        # Parse the original plan strictly, solely to bind original absolute paths.
        # No alternate log, partial JSON, or current-host facts are accepted.
        self.plan = read(self.root / "plan.json")
        self.origin = PurePosixPath(self.plan["output"])
        need(self.origin.is_absolute(), "measurement proof origin is not absolute")
        self.files = proof_index(self.recovery_manifest, self.origin)
        actual = set()
        for path in self.root.rglob("*"):
            need(not path.is_symlink(), "recovery proof symlink")
            if path.is_file():
                name = path.relative_to(self.root).as_posix()
                if not name.startswith("recovery/") and name != "recovery-manifest.json":
                    actual.add(name)
            else:
                need(path.is_dir(), "nonregular recovery proof entry")
        need(actual == set(self.files), "original proof recovery membership differs")
        for name, expected in self.files.items():
            need(a.pin(self.path(name)) == expected, "recovered original proof bytes differ: " + name)
        self.state_cache = {}
        self.recovery_pin = a.pin(manifest_path)


def verify_build(evidence, build):
    plan = evidence.plan
    need(build["schema"] == "r1.flat-ev-cloud32-phase/v1" and build["phase"] == "build"
         and build["status"] == "completed" and build["plan"] == evidence.files["plan.json"]
         and build["counts"] == {"completed": 7, "failed": 0, "skipped": 0}
         and not build["pilot_stages"], "original build/smoke phase incomplete")
    expected = ["toolchain", "build-baseline", "build-flat", "smoke-narrow-baseline-1", "smoke-narrow-flat-1",
                "smoke-narrow-baseline-32", "smoke-narrow-flat-32"]
    need([s["name"] for s in build["stages"]] == expected, "build/smoke schedule differs")
    binaries = build["binaries"]
    need(set(binaries) == set(a.ARMS), "both native binaries required")
    for stage in build["stages"][:3]:
        a.verify_stage(evidence, stage)
        if stage["kind"] == "build":
            arm = stage["arm"]
            target = str(PurePosixPath(plan["workspace"]) / f"{arm}-target")
            need(stage["command"] == [plan["tools"]["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2",
                                      "--target-dir", target, "-p", "holdem", "--example", "flop_cloud32_probe", "--message-format=json"],
                 "native build argv differs")
            need(stage["artifact"] == binaries[arm] and binaries[arm]["path"] == target + "/release/examples/flop_cloud32_probe",
                 "native binary binding differs")
            with gzip.open(evidence.require(stage["retained_binary"]), "rb") as stream:
                need(a.digest(stream, 64 * 1024**2) == pair(binaries[arm]), "retained native binary differs")
    need("release: 1.97.0" in build["rustc_version"] and "host: x86_64-unknown-linux-gnu" in build["rustc_version"],
         "native toolchain version differs")
    canonical = None
    for stage, (arm, worker) in zip(build["stages"][3:], (("baseline", 1), ("flat", 1), ("baseline", 32), ("flat", 32))):
        need((stage["case"], stage["iterations"], stage["arm"], stage["workers"]) == ("narrow", 2, arm, worker),
             "Linux smoke condition differs")
        canonical = a.verify_solve(evidence, stage, binaries, canonical)
    need(build["smoke_canonical"] == canonical, "Linux smoke canonical differs")
    need(a.utc(build["started_at"]) <= a.utc(build["stages"][0]["started_at"])
         and a.utc(build["stages"][-1]["verified_at"]) <= a.utc(build["ended_at"]) <= a.utc(plan["deadline_utc"]),
         "build phase times differ")
    return binaries


def narrow_rows(execution):
    rows = execution["stages"]
    a.fixed_rows(rows)
    need(execution["status"] in {"running", "failed"} and any(r["status"] != "completed" for r in rows),
         "partial reader only accepts an interrupted matrix")
    narrow = rows[:48]
    need(all(r["case"] == "narrow" and r["status"] == "completed" for r in narrow), "all fixed narrow48 rows must be complete")
    need(sum(r["warmup"] for r in narrow) == 12, "narrow warmup count differs")
    return narrow


def describe(rows):
    """No acceptance thresholds, selected fastest rows, or expanded statistics."""
    need(len(rows) == 48 and all(r["case"] == "narrow" and r["status"] == "completed" for r in rows), "narrow48 required")
    expected = a.schedule()[:48]
    need(all(all(row[k] == value for k, value in wanted.items()) for row, wanted in zip(rows, expected)), "narrow order differs")
    groups = []
    for arm in a.ARMS:
        for workers in a.WORKERS:
            selected = [r for r in rows if r["arm"] == arm and r["workers"] == workers and not r["warmup"]]
            need([r["round"] for r in selected] == [1, 2, 3], "all three measured narrow rounds required")
            need(len({r["iterations"] for r in selected}) == 1, "repeated iteration counts differ")
            values = [{"cfr_seconds": r["result"]["cfr_seconds"], "quality_7_walk_seconds": r["result"]["quality_seconds"],
                       "cfr_plus_quality_seconds": r["result"]["cfr_seconds"] + r["result"]["quality_seconds"],
                       "construction_seconds": r["result"]["build_seconds"], "state_write_seconds": r["result"]["state_write_seconds"],
                       "whole_process_seconds": r["process_seconds"], "root_os_peak_resident_bytes": r["root_os_peak_resident_bytes"]} for r in selected]
            groups.append({"case": "narrow", "arm": arm, "workers": workers, "iterations": selected[0]["iterations"],
                           "stages": [r["name"] for r in selected],
                           "metrics": {metric: a.stats([v[metric] for v in values]) for metric in a.METRICS}})
    return groups


def verify_narrow(evidence):
    a.verify_sources(evidence)
    build, execution = read(evidence.path("build.json")), read(evidence.path("execution.json"))
    need(execution["schema"] == "r1.flat-ev-cloud32-phase/v1" and execution["phase"] == "matrix"
         and execution["plan"] == evidence.files["plan.json"] and execution["build"] == evidence.files["build.json"],
         "original matrix phase binding differs")
    rows = narrow_rows(execution)
    binaries = verify_build(evidence, build)
    need(execution["binaries"] == binaries, "matrix binaries differ from native build")
    pilots = execution["pilot_stages"]
    need(len(pilots) == 8, "original pilot plan incomplete")
    selected = None
    verified_pilots = []
    for stage, (case, n) in zip(pilots, [(c, n) for c in a.CASES for n in a.PILOTS]):
        need(stage["name"] == f"pilot-{case}-{n}" and (stage["case"], stage["iterations"], stage["arm"], stage["workers"])
             == (case, n, "baseline", 1), "original pilot condition differs")
        if case != "narrow":
            continue
        if selected is not None:
            need(stage["status"] == "skipped" and stage["reason"] == "baseline already selected smaller N", "unexpected later narrow pilot")
            continue
        canonical = a.verify_solve(evidence, stage, binaries)
        verified_pilots.append(stage)
        seconds = stage["result"]["cfr_seconds"]
        if seconds >= 4 or n == 128:
            selected = {"iterations": n, "cfr_seconds": seconds, "short_timing": seconds < 4,
                        "canonical": canonical, "stage": stage["name"]}
    need(selected is not None and execution["selected"]["narrow"] == selected, "narrow pilot selection differs")
    for row in rows:
        need(row["iterations"] == selected["iterations"], "narrow matrix changed selected N")
        a.verify_solve(evidence, row, binaries, selected["canonical"])
    ordered = build["stages"] + verified_pilots + rows
    need(all(a.utc(x["verified_at"]) <= a.utc(y["started_at"]) for x, y in zip(ordered, ordered[1:])), "verified narrow work overlapped/reordered")
    need(a.utc(execution["started_at"]) <= a.utc(verified_pilots[0]["started_at"]), "matrix start time differs")
    expanded = execution["stages"][48:]
    allowed = {"pending", "running", "completed", "failed", "skipped"}
    need(all(r["status"] in allowed for r in expanded), "unknown expanded row status")
    return {"status": "verified_descriptive_partial", "recovered_payload_integrity": "verified",
            "main_receipt_status": execution["status"], "full_matrix_complete": False,
            "measurement_host": evidence.plan["host"],
            "counts": {"narrow_fixed": 48, "narrow_warmup_excluded": 12, "narrow_measured": 36,
                       "expanded_expected": 48, "expanded_recorded_status_counts": {s: sum(r["status"] == s for r in expanded) for s in sorted(allowed)}},
            "selected_narrow": {k: v for k, v in selected.items() if k != "canonical"},
            "canonical_quality": read(evidence.require(selected["canonical"]["quality"])),
            "groups": describe(rows), "unique_canonical_streams_verified": len(evidence.state_cache),
            "recovery_manifest": evidence.recovery_pin,
            "original_metadata": {name: evidence.files[name] for name in ("plan.json", "build.json", "execution.json", "retained.json")}}


def analyze_partial(proof, manifest):
    result = {"schema": "r1.flat-ev-cloud32-narrow-partial/v1", "status": "not_evaluable",
              "recovered_payload_integrity": "not_verified", "full_matrix_complete": False,
              "limitations": ["The fixed 96-condition campaign was interrupted. This is only the predeclared complete narrow block; expanded has recorded completion counts only.",
                              "Recovery-manifest hashes establish bytes captured after restart, not the pre-interruption completeness of unflushed pages. Original JSON must parse strictly and each earlier stage/output pin must still match.",
                              "The original stale retained.json is preserved only. The reader's in-memory payload index comes from the separate recovery manifest; no original file is rewritten.",
                              "No damaged metadata is reconstructed from logs. No samples are replaced, selected by speed, or combined across boots.",
                              "CPU topology/boot are the recorded measurement host, not the restarted recovery host. All 48 narrow stage identities must match the original host.",
                              "Each retained canonical is fully rehashed. Deleted duplicate state equality relies on the pinned runner's original full-byte comparison receipts.",
                              "Quality timing aggregates seven public value walks. RSS is the root-process Linux wait4 counter, not a simultaneous physical/process-tree peak.",
                              "These fixed-iteration typed-fixture results are descriptive only and do not establish external reference accuracy or whole-R1 completion."]}
    try:
        evidence = RecoveredEvidence(proof, manifest)
        result["recovered_payload_integrity"] = "verified"
        result.update(verify_narrow(evidence))
    except (ValueError, KeyError, TypeError, OSError, EOFError, a.tarfile.TarError) as error:
        result["error"] = str(error)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--recovery-manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    need(not args.out.exists() and not args.out.resolve().is_relative_to(args.proof.resolve()), "new report outside original proof required")
    result = analyze_partial(args.proof, args.recovery_manifest)
    result["reader"] = a.pin(Path(__file__))
    result["trusted_analyzer"] = {"sha256": ANALYZER_SHA}
    with args.out.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(result, stream, indent=2, ensure_ascii=False, allow_nan=False)
        stream.write("\n")
    print(json.dumps({"status": result["status"], "out": str(args.out)}))
    return 0 if result["status"] == "verified_descriptive_partial" else 2


if __name__ == "__main__":
    raise SystemExit(main())
