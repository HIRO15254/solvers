#!/usr/bin/env python3
"""Offline verification of the pinned VM06 source03 phase calibration.

No retained executable or Python module is executed; no archive is extracted.
Every archive payload is hashed, including exports above the compact-copy cap.
Only the two source03 datasets participate. Source06 is a separate experiment.
"""
from __future__ import annotations

import argparse
import copy
import datetime as dt
import hashlib
import io
import json
import math
from pathlib import Path, PurePosixPath
import statistics
import sys
import tarfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
BUNDLES = {
    "phases": ("vm06-phases.tar.gz", "eef21251eec733ee51b2bf6ab79c0776df1fc5b40a0b837c77a00f29ac42749d", 1461),
    "original": ("vm06-v3-results.tar.gz", "3c679810a39b3fac58d394f95387b0d98cec94ccc299f325e5922eefb9264e2d", 763),
}
BOOT = "159efb96-10fb-4ce4-bb0f-bc2b27ee618f"
INSTRUMENTATION = "aa81ca3e10c1beba324a8d42483294ca54a8b3fe852970a9f924da44fd2e5a3a"
ROOTS = {"on": "/opt/r1/phase-on", "off": "/opt/r1/phase-off",
         "original": "/opt/r1/candidate-v3/runs/r1-paired-v3"}
LEAVES = {"input_preparation", "initialization", "cfr_updates", "periodic_ev_br",
          "checkpoint", "final_ev_br", "summary_publish", "sol_preparation",
          "sol_serialization_and_write", "overhead"}
SUMMARY = {"board", "pot", "effective_stack", "min_bet", "iterations", "ev_oop", "ev_ip",
           "expl_oop", "expl_ip", "nash_conv", "storage", "streets_stored", "nodes", "stored_nodes"}
VERSIONS = {"baseline": "baseline9632", "candidate": "candidate03"}
EXPECTED = {(case, version, rep) for case in ("river", "turn", "flop")
            for version in VERSIONS for rep in (1, 2, 3)}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def identity(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024**2), b""):
            h.update(chunk)
    return {"path": str(path.resolve()), "bytes": path.stat().st_size, "sha256": h.hexdigest()}


def document(raw):
    require(b"\0" not in raw, "NUL in adopted structured evidence")
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def invalid(value):
        raise ValueError("nonfinite JSON constant: " + value)
    return json.loads(raw.decode("utf-8"), object_pairs_hook=pairs, parse_constant=invalid)


def stats(values):
    require(len(values) == 3 and all(type(x) in (float, int) and math.isfinite(x) for x in values),
            "expected three finite measurements")
    return {"values": values, "n": 3, "median": statistics.median(values), "min": min(values), "max": max(values)}


class Evidence:
    def __init__(self, directory):
        self.files, self.raw, self.bundles = {}, {}, {}
        for label, (filename, expected, count) in BUNDLES.items():
            path = directory / filename
            bid = identity(path)
            require(bid["sha256"] == expected, "unexpected " + label + " bundle")
            sidecar = Path(str(path) + ".manifest.json").read_bytes()
            manifest = document(sidecar)
            require(manifest["schema"] == "solvers.r1-retention/v1", "collector schema")
            require(Path(str(path) + ".sha256").read_text().strip() == expected + "  " + filename,
                    "checksum sidecar mismatch")
            members = {row["archive_member"]: row for row in manifest["files"]}
            require(len(members) == count == len(manifest["files"]), "collector count/duplicate")
            seen, total = set(), 0
            with tarfile.open(path, "r|gz") as archive:
                for member in archive:
                    require(member.isfile() and member.name not in seen and member.size <= 128 * 1024**2,
                            "nonregular, duplicate or oversized payload")
                    seen.add(member.name)
                    raw = archive.extractfile(member).read()
                    if member.name == "retention-manifest.json":
                        require(raw == sidecar, "inner/outer manifest mismatch")
                        continue
                    require(member.name in members, "unlisted payload")
                    row = members[member.name]
                    require(row["included"] is True and row["kind"] == "regular"
                            and len(raw) == row["bytes"] and sha(raw) == row["sha256"], "payload identity mismatch")
                    vm = row["original_path"]
                    item = {"path": vm, "bytes": len(raw), "sha256": sha(raw)}
                    require(vm not in self.files or self.files[vm] == item, "conflicting cross-bundle path")
                    self.files[vm] = item
                    # Large exported profiles need byte equality only; never replace them with absent/zero values.
                    if len(raw) <= 2 * 1024**2 or vm.endswith("source.tar.gz"):
                        self.raw[vm] = raw
                    total += len(raw)
            require(seen == {"retention-manifest.json", *members}, "bundle member set differs")
            self.bundles[label] = {"archive": bid, "payloads_verified": count, "payload_bytes": total}
        self.stage_records, self.unretained_system_identities = set(), {}

    def bound(self, item, need_raw=False):
        require(self.files.get(item["path"]) == {k: item[k] for k in ("path", "bytes", "sha256")},
                "unresolved or changed identity: " + item["path"])
        return self.get(item["path"]) if need_raw else None

    def get(self, path):
        require(path in self.raw, "structured evidence not retained in bounded reader: " + path)
        return self.raw[path]

    def read(self, path):
        return document(self.get(path))

    def compact(self, directory, label):
        record = document((directory / "retention.json").read_bytes())
        index = document((directory / "compact-index.json").read_bytes())
        indexed = set()
        for item in index["files"]:
            relative = PurePosixPath(item["path"])
            require(not relative.is_absolute() and ".." not in relative.parts, "unsafe compact index path")
            target = directory / relative
            checked = identity(target)
            require(checked["bytes"] == item["bytes"] and checked["sha256"] == item["sha256"], "compact index identity differs")
            require(target.resolve() not in indexed, "duplicate compact index file")
            indexed.add(target.resolve())
        require({p.resolve() for p in directory.rglob("*") if p.is_file()}
                == indexed | {(directory / "compact-index.json").resolve()}, "compact index file set differs")
        require(record["schema"] == "r1.local-comparison-retention/v1", "compact schema")
        require(len(record["bundles"]) == 1 and record["bundles"][0]["archive"]["sha256"]
                == self.bundles[label]["archive"]["sha256"], "compact bundle mismatch")
        expected, count = set(), 0
        for row in record["files"]:
            self.bound({"path": row["original_path"], "bytes": row["bytes"], "sha256": row["sha256"]})
            if row["compact_path"] is not None:
                relative = PurePosixPath(row["compact_path"])
                require(not relative.is_absolute() and ".." not in relative.parts, "unsafe compact path")
                path = directory / relative
                require(identity(path)["sha256"] == row["sha256"] and path.stat().st_size == row["bytes"],
                        "compact copy differs")
                expected.add(path.resolve())
                count += 1
        require(len(record["files"]) == self.bundles[label]["payloads_verified"], "compact inventory count")
        require({p.resolve() for p in (directory / "records").rglob("*") if p.is_file()} == expected,
                "unexpected/missing compact payload")
        return {"path": str(directory.resolve()), "compact_payload_files_verified": count,
                "compact_files_including_metadata_verified": len(indexed)+1,
                "generic_readiness": record["validation"]["ready"],
                "generic_problems": record["validation"]["problems"],
                "cross_bundle_dependencies_resolved_by_this_verifier": True}

    def stage(self, stage, binary, required_inputs):
        record = document(self.bound(stage["record"], True))
        self.stage_records.add(stage["record"]["path"])
        for item in (record, stage):
            require(item["state"] == "completed" and item["stop_reason"] == "completed"
                    and item["cleanup_complete"] is True and item["identity_unchanged"] is True,
                    "incomplete or changed stage")
        require(record["schema"] == "solvers.supervised-run/v1" and record["child_exit_code"] == 0
                and record["supervisor_exit_code"] == 0 and stage["exit_code"] == 0
                and record["errors"] == [] and record["forced"] is False, "failed stage")
        require(record["identity_before"] == record["identity_after"], "stage identity changed")
        for item in record["identity_before"]:
            if item["path"].startswith("/usr/bin/"):
                prior = self.unretained_system_identities.setdefault(item["path"], item)
                require(prior == item, "system interpreter identity drift")
            else:
                self.bound(item)
        require(all({k: item[k] for k in ("path", "bytes", "sha256")} in record["identity_before"]
                    for item in (binary, *required_inputs)),
                "stage lacks required source/config/binary binding")
        require(record["argv"][0] == binary["path"] and record["resolved_argv"] == record["argv"]
                and record["shell"] is False, "stage command identity")
        for output in record["outputs"].values():
            self.bound(output)
        require(stage["stdout"] == record["outputs"]["stdout"], "stdout identity differs")
        require(stage["measurement"] == record["measurement"]
                and stage["elapsed_seconds"] == record["elapsed_seconds"]
                and math.isfinite(record["elapsed_seconds"]) and record["elapsed_seconds"] >= 0, "measurement differs")
        samples = [document(line) for line in self.bound(record["outputs"]["samples"], True).splitlines()]
        require(len(samples) == record["measurement"]["sample_count"] and samples
                and samples[-1] == record["last_sample"] and not samples[-1]["pids"], "sample completion differs")
        require(max(s["tree_resident_bytes"] for s in samples) == record["measurement"]["sampled_peak_tree_resident_bytes"],
                "sampled peak differs")
        elapsed = [s["elapsed_seconds"] for s in samples]
        require(all(math.isfinite(x) and x >= 0 for x in elapsed) and elapsed == sorted(elapsed), "invalid sample clock")
        return record


def phase_summary(record, source_version):
    require(record["schema"] == "r1.phase/v1" and record["status"] == "completed"
            and record["source_version"] == source_version and record["instrumentation_id"] == INSTRUMENTATION,
            "phase invocation/source mismatch")
    cursor, leaves, updates = 0, {}, []
    for span in record["spans"]:
        start, end = span["start_ns"], span["end_ns"]
        require(type(start) is int and type(end) is int and start == cursor and end >= start
                and span["status"] == "complete", "overlap/gap/failed phase")
        cursor = end
        leaf = leaves.setdefault(span["phase"], {"calls": 0, "duration_ns": 0})
        leaf["calls"] += 1
        leaf["duration_ns"] += end-start
        if span["phase"] == "cfr_updates":
            updates.append(span)
    require(set(leaves) == LEAVES, "missing/unknown phase is not zero")
    require(cursor == record["total_ns"] == record["leaf_sum_ns"], "leaf partition total differs")
    envelope = {"start_ns": updates[0]["start_ns"], "end_ns": updates[-1]["end_ns"],
                "duration_ns": updates[-1]["end_ns"]-updates[0]["start_ns"], "add_to_leaf_sum": False}
    require(record["cfr_inclusive_envelope"] == envelope, "inclusive CFR is not an additive leaf")
    return {"source_version": source_version, "instrumentation_id": INSTRUMENTATION,
            "total_ns": cursor, "leaves": leaves, "cfr_inclusive_envelope": envelope}


def same_profile(left, right):
    return all(left[name]["stdout"]["sha256"] == right[name]["stdout"]["sha256"]
               and left[name]["stdout"]["bytes"] == right[name]["stdout"]["bytes"]
               for name in ("tree", "strategy", "ev"))


def verify(args):
    evidence = Evidence(args.bundles)
    compact = {"phases": evidence.compact(args.evidence, "phases"),
               "original": evidence.compact(args.original_evidence, "original")}
    plan = evidence.read("/opt/r1/phase-plan/plan.json")
    research = plan["phase_research"]
    original_plan = document(evidence.bound(research["original_plan"], True))
    require(research["schema"] == "r1.phase-campaign/v1" and research["instrumented_pilot_performed"] is False,
            "research scope changed")
    require(research["instrumentation_id"] == INSTRUMENTATION, "instrumentation pin changed")
    for key in original_plan.keys() - {"created_at", "baseline", "candidate"}:
        require(plan[key] == original_plan[key], "original plan changed: " + key)
    evidence.bound(research["original_pilot"])
    for item in research["instrumentation_files"].values():
        evidence.bound(item)
    for item in (plan["runner"], plan["supervisor"], plan["pilot"]):
        evidence.bound(item)
    pins = evidence.read(research["instrumentation_files"]["source_versions"]["path"])
    source_proof = {}
    for version, source_version in VERSIONS.items():
        current, old = plan[version], original_plan[version]
        for key in old.keys() - {"binary", "source_evidence"}:
            require(current[key] == old[key], "version conditions changed")
        for item in (current["binary"], old["binary"], old["source_evidence"]):
            evidence.bound(item)
        manifest = document(evidence.bound(current["source_evidence"], True))
        require(manifest["schema"] == "r1.phase.source/v1" and manifest["source_version"] == source_version
                and manifest["instrumentation_id"] == INSTRUMENTATION
                and manifest["before"] == pins[source_version]["build_inputs"], "source manifest/pin mismatch")
        source_raw = evidence.bound(old["source_evidence"], True)
        hashes = {}
        with tarfile.open(fileobj=io.BytesIO(source_raw), mode="r:gz") as archive:
            for member in archive:
                if member.isfile():
                    require(member.name not in hashes, "duplicate source member")
                    hashes[member.name] = sha(archive.extractfile(member).read())
        require(all(hashes.get(name) == digest for name, digest in manifest["before"].items()), "source archive differs from manifest")
        changed = {name for name in manifest["before"] if manifest["after"].get(name) != manifest["before"][name]}
        require(changed == set(manifest["modified"]) == {"crates/cli/src/lib.rs", "crates/cli/src/solve.rs", "crates/cli/src/sol.rs"}
                and set(manifest["after"])-set(manifest["before"]) == set(manifest["added"]) == {"crates/cli/src/r1_phase.rs"},
                "unplanned source changes")
        source_proof[version] = {"original_source": old["source_evidence"], "instrumented_binary": current["binary"],
                                 "manifest": current["source_evidence"], "before_files_verified": len(manifest["before"])}
    before = evidence.read("/opt/r1/phase-build/identity-before.json")
    after = evidence.read("/opt/r1/phase-build/identity-after.json")
    evidence.bound(after["before_record"])
    evidence.bound(after["comparison_build"])
    require(after["state"] == "completed" and before["state"] == "before"
            and before["boot_id"] == after["boot_id"] == BOOT and before["cpu"] == after["cpu"]
            and after["cpu"]["Model name"] == "AMD EPYC 7B12", "phase build CPU/boot/state mismatch")
    require(before["inputs"] == after["inputs"], "phase build inputs changed")
    for item in after["inputs"]:
        evidence.bound(item)
    for version in VERSIONS:
        require(after["versions"][version]["binary"] == plan[version]["binary"]
                and after["versions"][version]["source_manifest"] == plan[version]["source_evidence"], "build output/plan mismatch")

    reports = {mode: evidence.read(root + "/comparison.json") for mode, root in ROOTS.items()}
    rows, summaries = {}, {}
    cases = {c["case"]: c for c in plan["cases"]}
    require(set(cases) == {"river", "turn", "flop"}, "unexpected cases")
    require(plan["host"]["logical_cpus"] == 8 and plan["host"]["rayon_num_threads"] == "8", "thread condition changed")
    for mode, report in reports.items():
        chosen_plan = original_plan if mode == "original" else plan
        require(report["state"] == "completed" and report["host"]["boot_id"] == BOOT, "campaign incomplete/wrong boot")
        evidence.bound(report["plan"])
        require(report["plan"]["path"] == (research["original_plan"]["path"] if mode == "original" else "/opt/r1/phase-plan/plan.json"),
                "wrong campaign plan")
        mapping = {(r["case"], r["version"], r["repetition"]): r for r in report["runs"]}
        require(len(mapping) == len(report["runs"]) == 18 and set(mapping) == EXPECTED, "campaign coverage")
        rows[mode] = mapping
        for key, row in mapping.items():
            case, version, rep = key
            config = cases[case]["config"]
            evidence.bound(config)
            require(row["run_status"] == "completed" and row["presave_consistency"] == "pass"
                    and row["saved_profile_br"] == "not_evaluated" and row["quality_status"] == "not_evaluated", "row status")
            require(all(value is None for value in row["phase_timings"].values()), "original phase fields were injected")
            require(evidence.read(f"{ROOTS[mode]}/{case}-{rep}-{version}/result.json") == row, "row/final result differ")
            for artifact in row["artifacts"].values():
                evidence.bound(artifact)
            run = document(evidence.bound(row["artifacts"]["run.json"], True))
            summary = row["summary"]
            require(document(evidence.bound(row["summary_read"]["stdout"], True)) == summary, "summary not backed by raw export")
            require(row["live_final"] == {"g_i": [run["explP0"], run["explP1"]], "iterations": run["iterations"],
                                          "nash_conv": run["nashConv"], "pot": summary["pot"]}, "live final differs from raw run")
            require(summary["iterations"] == cases[case]["iterations"] == run["iterations"]
                    and summary["expl_oop"] == run["explP0"] and summary["expl_ip"] == run["explP1"]
                    and summary["nash_conv"] == run["nashConv"] and summary["streets_stored"] == "full", "presave summary mismatch")
            gains = row["live_final"]["g_i"]
            require(all(math.isfinite(x) and x >= -1e-10 for x in gains)
                    and math.isclose(sum(gains), run["nashConv"], rel_tol=1e-12, abs_tol=1e-10)
                    and run["nashConv"] < cases[case]["target_nash_conv"] == 0.04, "live quality target fails")
            binary = chosen_plan[version]["binary"]
            required_inputs = [config, report["plan"], chosen_plan["runner"], chosen_plan["supervisor"]]
            solve = evidence.stage(row["solve"], binary, required_inputs)
            require(solve["argv"] == [binary["path"], "solve", config["path"], "--out",
                                      f"{ROOTS[mode]}/{case}-{rep}-{version}/run", "--sol-streets", "full"], "solve argv differs")
            sol = row["artifacts"]["solution.sol"]
            exported = evidence.stage(row["summary_read"], binary, [report["plan"], sol])
            require(exported["argv"] == [binary["path"], "export", sol["path"], "summary"], "summary command differs")
            for name, stage in row["profile"].items():
                exported = evidence.stage(stage, binary, [report["plan"], sol])
                require(exported["argv"] == [binary["path"], "export", sol["path"], name, "--node", "all"],
                        "profile command differs")
            resume = row["resume"]
            if rep == 1:
                require(resume["status"] == "pass" and same_profile(resume["profile"], row["profile"])
                        and all(resume["summary"][f] == summary[f] for f in SUMMARY), "restore output changed")
                for stage in [resume["stage"], resume["summary_read"], *resume["profile"].values()]:
                    evidence.stage(stage, binary, [report["plan"]])
            else:
                require(resume is None, "unexpected restore scope")
            overlay = row["solve"].get("r1_phase")
            if mode == "on":
                require(overlay["status"] == "validated" and overlay["mode"] == "on", "on phase missing")
                require(evidence.read(f"{ROOTS[mode]}/{case}-{rep}-{version}/solve/phase-validation.json") == overlay,
                        "phase validation record differs from comparison overlay")
                raw_phase = document(evidence.bound(overlay["record"], True))
                require(raw_phase["pid"] == solve["pid"] and raw_phase["command_error"] is None,
                        "phase does not belong to completed solve process")
                phase_start = int(raw_phase["started_unix_ns"])/1e9
                require(dt.datetime.fromisoformat(solve["started_at"]).timestamp() <= phase_start
                        <= dt.datetime.fromisoformat(solve["ended_at"]).timestamp(), "phase start outside solve invocation")
                iterations = range(10, 51, 10) if case == "flop" else range(25, 101, 25)
                sequence = [("input_preparation", None), ("initialization", None)]
                sequence += [(name, iteration) for iteration in iterations
                             for name in ("cfr_updates", "periodic_ev_br", "checkpoint")]
                sequence += [("final_ev_br", None), ("summary_publish", None)]
                if version == "baseline":
                    sequence.append(("checkpoint", cases[case]["iterations"]))
                sequence += [("sol_preparation", None), ("sol_serialization_and_write", cases[case]["iterations"])]
                require([(s["phase"], s["iteration"]) for s in raw_phase["spans"] if s["phase"] != "overhead"] == sequence,
                        "source-specific phase call order or iteration boundaries changed")
                validated = phase_summary(raw_phase, VERSIONS[version])
                require(validated == overlay["summary"], "phase stored summary differs")
                summaries[key] = validated
            elif mode == "off":
                require(overlay["status"] == "disabled" and overlay["mode"] == "off", "off phase enabled")
            else:
                require(overlay is None, "phase injected into original record")
    actual_phases = {p for p in evidence.files if p.endswith("/phase.json")}
    require(actual_phases == {r["solve"]["r1_phase"]["record"]["path"] for r in rows["on"].values()}, "unexpected/missing phase outputs")
    actual_stages = {p for p in evidence.files if p.endswith("/supervisor.json")
                     and any(p.startswith(root + "/") for root in ROOTS.values())}
    require(actual_stages == evidence.stage_records and len(actual_stages) == 360, "extra/missing campaign supervisor record")

    calibration = evidence.read("/opt/r1/phase-calibration.json")
    evidence.bound(calibration["plan"])
    for item in calibration["reports"].values():
        evidence.bound(item)
    checks = []
    for key in sorted(EXPECTED):
        original = rows["original"][key]
        for mode in ("off", "on"):
            row = rows[mode][key]
            require(row["live_final"] == original["live_final"]
                    and all(row["summary"][f] == original["summary"][f] for f in SUMMARY)
                    and same_profile(row["profile"], original["profile"])
                    and row["artifacts"]["run.toml"]["sha256"] == original["artifacts"]["run.toml"]["sha256"],
                    "calibration changed config/quality/export")
        checks.append({"case": key[0], "version": key[1], "repetition": key[2], "status": "pass"})
    require(len(calibration["comparisons"]) == 18
            and {(r["case"], r["version"], r["repetition"]) for r in calibration["comparisons"]} == EXPECTED
            and all(r["status"] == "pass" and all(r["checks"].values()) for r in calibration["comparisons"])
            and calibration["eligible"] is True and calibration["saved_profile_br"] == "not_evaluated", "stored calibration disagrees")
    results = []
    stored_analysis = evidence.read("/opt/r1/phase-on/phase-analysis.json")
    require(stored_analysis == calibration["phase_analysis_on"], "calibration phase analysis differs")
    for case in ("river", "turn", "flop"):
        for version in VERSIONS:
            phase = [summaries[(case, version, rep)] for rep in (1, 2, 3)]
            external = {mode: stats([rows[mode][(case, version, rep)]["solve"]["elapsed_seconds"] for rep in (1, 2, 3)]) for mode in ROOTS}
            leaves = {name: {field: stats([r["leaves"][name][field] for r in phase]) for field in ("duration_ns", "calls")} for name in sorted(LEAVES)}
            result = {"case": case, "version": version, "external_solve_seconds": external,
                      "on_over_off_median_ratio": external["on"]["median"]/external["off"]["median"],
                      "off_over_original_median_ratio": external["off"]["median"]/external["original"]["median"],
                      "phase_total_ns": stats([r["total_ns"] for r in phase]), "leaves": leaves,
                      "cfr_inclusive_ns_nonadditive": stats([r["cfr_inclusive_envelope"]["duration_ns"] for r in phase]),
                      "external_minus_internal_ns": stats([external["on"]["values"][i]*1e9-r["total_ns"] for i, r in enumerate(phase)]),
                      "memory_bytes": {mode: {metric: stats([rows[mode][(case, version, rep)]["solve"]["measurement"][metric] for rep in (1, 2, 3)])
                                              for metric in ("root_os_peak_resident_bytes", "sampled_peak_tree_resident_bytes")} for mode in ROOTS}}
            saved = next(r for r in calibration["cases"] if (r["case"], r["version"]) == (case, version))
            require(all(result[field] == saved[field] for field in saved), "calibration medians differ")
            saved_phase = next(r for r in stored_analysis["cases"] if r["case"] == case)["versions"][version]
            require(all(result[field] == saved_phase[field] for field in ("external_solve_seconds", "phase_total_ns", "leaves", "external_minus_internal_ns")
                        if field != "external_solve_seconds") and result["external_solve_seconds"]["on"] == saved_phase["external_solve_seconds"], "phase medians differ")
            results.append(result)
    return {"schema": "r1.vm06-source03-phase-verification/v1", "status": "pass",
            "scope": "source03 phase calibration, not final source06 performance acceptance",
            "verifier": identity(Path(__file__)), "bundles": evidence.bundles, "compact": compact,
            "boot_id": BOOT, "cpu_model": after["cpu"]["Model name"], "source_proof": source_proof,
            "instrumentation_id": INSTRUMENTATION, "supervisor_stages_verified": len(evidence.stage_records),
            "unretained_system_identities_observed_before_after_only": list(evidence.unretained_system_identities.values()),
            "coverage": {mode: len(mapping) for mode, mapping in rows.items()}, "phase_records_verified": len(summaries),
            "leaf_names": sorted(LEAVES), "calibration_output_equivalence": checks,
            "original_record_mutation": False, "saved_quantized_profile_br": "not_evaluated_by_phase_campaign",
            "external_reference_acceptance": None, "cases": results,
            "limitations": ["Three repetitions in separate mode campaigns; warm caches and order/noise are uncontrolled.",
                            "Internal monotonic leaf partition excludes process startup/shutdown and phase file final publication; external minus internal is not a leaf.",
                            "Medians of individual leaves need not sum to the median total; CFR inclusive envelope overlaps leaves and must not be added.",
                            "RSS is invocation-wide, not phase memory. wait4 root high-water may include fork/pre-exec or reaped descendant effects; sampled tree RSS misses short peaks.",
                            "Source manifests/build identity bind recorded outputs; this is not a new build, solver run, independent oracle, or saved-profile BR audit.",
                            "Compact files await Git commit; raw exports/SOL/CKPT/binaries require the pinned local bundles."]}


def self_test():
    spans = [{"phase": leaf, "start_ns": i, "end_ns": i+1, "status": "complete"}
             for i, leaf in enumerate(sorted(LEAVES))]
    update = next(s for s in spans if s["phase"] == "cfr_updates")
    good = {"schema": "r1.phase/v1", "status": "completed", "source_version": "candidate03",
            "instrumentation_id": INSTRUMENTATION, "spans": spans, "total_ns": 10, "leaf_sum_ns": 10,
            "cfr_inclusive_envelope": {"start_ns": update["start_ns"], "end_ns": update["end_ns"], "duration_ns": 1, "add_to_leaf_sum": False}}
    phase_summary(good, "candidate03")
    variants = []
    bad = copy.deepcopy(good); bad["spans"][0]["phase"] = "unknown"; variants.append(bad)
    bad = copy.deepcopy(good); bad["spans"] = bad["spans"][:-1]; variants.append(bad)
    bad = copy.deepcopy(good); bad["spans"][1]["start_ns"] = 0; variants.append(bad)
    bad = copy.deepcopy(good); bad["cfr_inclusive_envelope"]["add_to_leaf_sum"] = True; variants.append(bad)
    bad = copy.deepcopy(good); bad["status"] = "incomplete"; variants.append(bad)
    bad = copy.deepcopy(good); bad["source_version"] = "candidate06"; variants.append(bad)
    for bad in variants:
        try:
            phase_summary(bad, "candidate03")
        except ValueError:
            continue
        raise AssertionError("invalid phase accepted")
    profile = {name: {"stdout": {"sha256": "a", "bytes": 1}} for name in ("tree", "strategy", "ev")}
    changed = copy.deepcopy(profile); changed["ev"]["stdout"]["sha256"] = "b"
    require(not same_profile(profile, changed), "changed export accepted")
    print("self-test: 7 negative cases rejected; complete phase accepted")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundles", type=Path, default=REPO / "runs/r1-cloud")
    parser.add_argument("--evidence", type=Path, default=HERE / "evidence-vm06")
    parser.add_argument("--original-evidence", type=Path, default=HERE.parent / "pipeline/evidence-vm06-v3")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    try:
        report = verify(args)
        output = json.dumps(report, indent=2, ensure_ascii=False, allow_nan=False) + "\n"
        if args.out:
            require(not args.out.resolve().is_relative_to(args.evidence.resolve())
                    and not args.out.resolve().is_relative_to(args.original_evidence.resolve()), "cannot write inside immutable evidence")
            args.out.write_text(output, encoding="utf-8", newline="\n")
            print(json.dumps({"status": report["status"], "stages": report["supervisor_stages_verified"], "phase_records": report["phase_records_verified"]}))
        else:
            print(output, end="")
        return 0
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"phase verification failed: {type(error).__name__}: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
