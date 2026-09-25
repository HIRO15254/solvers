#!/usr/bin/env python3
"""Verify only the retained VM06 recovery build, without running retained code.

The bundle SHA is pinned. Historical bytes are hashed for transport integrity,
but historical stages 12/13 are never used as successful-build evidence.
No Rust, cloud, solver execution, archive extraction or evidence mutation.
Print the report, or --out NEW_JSON outside the immutable evidence directory.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
VMROOT = "/opt/r1/audit-pair-recovery/"
BUNDLE_SHA = "d40e55c982f8702640e744e550f5c7072a0777bb84cb547d9ee22bc53e2750a5"
SOURCE_SHA = "f241167a9c765b839cbe560ec66c9c490a0b0193d8f23ba8de768c65eaaf043c"
BASELINE_SHA = "fdd8c1c014a94c70b56efbd79a36c18f6f1634c20aaf9062660ee3a6133a0197"
STAGES = ["00-toolchain", "01-baseline-pre-verify", "06-current-release",
          "06-current-toolchain-check", "07-seed-baseline-cache", "12-baseline-example-release",
          "13-baseline-final-verify", "14-final-toolchain"]
TIMEOUTS = [60, 120, 1800, 60, 300, 1800, 120, 60]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def file_identity(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return {"path": str(path.resolve()), "bytes": path.stat().st_size, "sha256": digest.hexdigest()}


def decode(raw):
    require(b"\0" not in raw, "NUL in adopted new recovery evidence")
    return raw.decode("utf-8")


def document(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result
    def invalid(value):
        raise ValueError(f"invalid JSON constant: {value}")
    return json.loads(decode(raw), object_pairs_hook=pairs, parse_constant=invalid)


def instant(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "timestamp lacks timezone")
    return result


def cpu_identity(text):
    keys = {"Architecture", "CPU op-mode(s)", "Vendor ID", "Model name", "CPU family", "Model", "Stepping", "Flags"}
    result = {}
    for line in text.splitlines():
        key, sep, value = line.partition(":")
        key, value = key.strip(), value.strip()
        if sep and key in keys:
            require(key not in result, "duplicate CPU field")
            result[key] = sorted(value.split()) if key == "Flags" else value
    require(set(result) == keys, "incomplete CPU observation")
    return result


def read_source(path, expected):
    identity = file_identity(path)
    require(identity["sha256"] == expected, "source archive SHA mismatch")
    files = {}
    with tarfile.open(path, "r:gz") as archive:
        for member in archive:
            if member.isdir():
                continue
            name = PurePosixPath(member.name)
            require(member.isfile() and not name.is_absolute() and ".." not in name.parts
                    and "\\" not in member.name and member.name not in files, "unsafe source member")
            require(member.size <= 32 * 1024**2, "oversized source member")
            files[member.name] = archive.extractfile(member).read()
    require(sum(map(len, files.values())) <= 256 * 1024**2, "source archive exceeds bound")
    return identity, files


def verify(args):
    bundle_id = file_identity(args.bundle)
    require(bundle_id["sha256"] == BUNDLE_SHA, "unexpected recovery bundle")
    retention = document((args.evidence / "retention.json").read_bytes())
    require(len(retention["bundles"]) == 1 and retention["bundles"][0]["archive"]["sha256"] == BUNDLE_SHA,
            "retention does not identify the pinned recovery bundle")
    sidecar = Path(str(args.bundle) + ".manifest.json").read_bytes()
    collector = document(sidecar)
    require(collector["schema"] == "solvers.r1-retention/v1", "unexpected collector schema")
    require(Path(str(args.bundle) + ".sha256").read_text().strip() == BUNDLE_SHA + "  " + collector["archive_filename"],
            "bundle checksum sidecar mismatch")
    members = {row["archive_member"]: row for row in collector["files"]}
    require(len(members) == 118 and len(members) == len(collector["files"]), "unexpected collector inventory")
    require(all(row["included"] is True and row["kind"] == "regular" for row in members.values()), "unretained payload")
    payloads, observed = {}, set()
    with tarfile.open(args.bundle, "r:gz") as archive:
        for member in archive:
            require(member.isfile() and member.name not in observed and member.size <= 64 * 1024**2,
                    "nonregular, duplicate or oversized bundle member")
            observed.add(member.name)
            raw = archive.extractfile(member).read()
            if member.name == "retention-manifest.json":
                require(raw == sidecar, "internal manifest differs from downloaded sidecar")
                continue
            require(member.name in members, "unlisted bundle payload")
            row = members[member.name]
            require(len(raw) == row["bytes"] and sha(raw) == row["sha256"], "bundle payload size/hash mismatch")
            require(row["original_path"] not in payloads, "duplicate VM path in recovery bundle")
            payloads[row["original_path"]] = raw
    require(observed == {"retention-manifest.json", *members}, "bundle member set mismatch")
    for row in retention["files"]:
        require(row["original_path"] in payloads, "retention references absent payload")
        raw = payloads[row["original_path"]]
        require(sha(raw) == row["sha256"] and len(raw) == row["bytes"], "retention inventory mismatch")
        if row["compact_path"]:
            require((args.evidence / row["compact_path"]).read_bytes() == raw, "compact copy differs from bundle")

    def get(path):
        require(path in payloads, f"required new evidence unavailable: {path}")
        return payloads[path]

    def bound(item):
        raw = get(item["path"])
        require(sha(raw) == item["sha256"] and len(raw) == item["bytes"], f"linked evidence differs: {item['path']}")
        return raw

    def read(name):
        return document(get(VMROOT + name))

    result, complete, retained = read("result.json"), read("complete.json"), read("retained-files.json")
    require(result["schema"] == "r1.audit-pair-build/v1" and result["status"] == "completed"
            and result["purpose"] == "saved_profile_quality_only" and complete["status"] == "completed", "new build did not complete")
    bound(complete["result"])
    bound(complete["retained_files"])
    actual_new = {path.removeprefix(VMROOT) for path in payloads if path.startswith(VMROOT)}
    require(actual_new == set(retained) | {"retained-files.json", "complete.json"}, "new retained-files inventory is incomplete")
    for name, item in retained.items():
        require(item["path"] == VMROOT + name, "retained-files VM path mismatch")
        bound(item)
    start, end = instant(result["started_utc"]), instant(result["ended_utc"])
    recovery = result["recovery"]
    require(start < end <= instant(recovery["deadline_utc"]), "new build timestamps exceed its deadline")
    require(start.date() == dt.date(2026, 9, 25) and end.date() == start.date(), "unexpected build date")
    require(math.isfinite(result["elapsed_seconds"]) and abs((end-start).total_seconds()-result["elapsed_seconds"]) < 1,
            "monotonic build interval disagrees with wall timestamps")
    require(recovery["changed_boot"] is True and recovery["previous_boot_id"] != result["boot_id"]
            and complete["changed_boot"] is True and recovery["native_cache_reused_from_prior_boot"] is False,
            "CPU/boot recovery boundary is missing")
    require([entry["name"] for entry in result["stages"]] == STAGES, "unexpected new stage order/coverage")
    stage_reports, previous_end = [], start
    unavailable = {}
    for entry, timeout in zip(result["stages"], TIMEOUTS):
        name = entry["name"]
        record = document(bound(entry["record"]))
        require(entry["record"]["path"] == VMROOT + name + "/supervisor.json", "stage record path mismatch")
        require(record["schema"] == "solvers.supervised-run/v1", "unexpected supervisor schema")
        for state in (entry, record):
            require(state.get("status", state.get("state")) == "completed" and state["child_exit_code"] == 0
                    and state["supervisor_exit_code"] == 0 and state["stop_reason"] == "completed"
                    and state["cleanup_complete"] is True and state["identity_unchanged"] is True, "unsuccessful new stage")
        require(not record["forced"] and not record["errors"] and record["shell"] is False, "new stage had force/error/shell execution")
        require(record["argv"] == entry["argv"] and record["cwd"] == entry["cwd"], "recorded command differs from stage")
        limits = record["limits"]
        require(entry["timeout_seconds"] == timeout == limits["timeout_seconds"] and limits["grace_seconds"] == 5
                and limits["kill_wait_seconds"] == 5 and limits["memory_limit_bytes"] == 40*1024**3
                and limits["min_free_memory_bytes"] == 8*1024**3 and limits["disk_reserve_bytes"] == 10*1024**3,
                "stage resource bound changed")
        a, b = instant(record["started_at"]), instant(record["ended_at"])
        require(previous_end <= a <= b <= end and 0 <= record["elapsed_seconds"] <= timeout + 10, "stage interval/order invalid")
        previous_end = b
        require(record["identity_before"] == record["identity_after"], "supervised input identities changed")
        for item in record["identity_before"]:
            if item["path"] in payloads:
                bound(item)
            else:
                unavailable[item["path"]] = item
        for output in ("stdout", "stderr", "samples"):
            raw = bound(record["outputs"][output])
            decode(raw)
        samples = [document(line) for line in bound(record["outputs"]["samples"]).splitlines() if line.strip()]
        require(len(samples) == record["measurement"]["sample_count"] > 0, "sample count mismatch")
        require(samples[-1] == record["last_sample"] and samples[-1]["pids"] == [], "last sample does not confirm empty containment")
        elapsed = [sample["elapsed_seconds"] for sample in samples]
        require(all(math.isfinite(x) and 0 <= x <= record["elapsed_seconds"] for x in elapsed)
                and elapsed == sorted(elapsed), "nonmonotonic/invalid samples")
        require(all(a <= instant(sample["at"]) <= b for sample in samples), "sample timestamp outside stage")
        stage_reports.append({"name": name, "status": "verified_completed", "record": entry["record"],
                              "stdout": record["outputs"]["stdout"], "stderr": record["outputs"]["stderr"],
                              "samples": record["outputs"]["samples"], "sample_count": len(samples),
                              "started_utc": record["started_at"], "ended_utc": record["ended_at"],
                              "supervised_input_count": len(record["identity_before"]), "elapsed_seconds": record["elapsed_seconds"]})

    cpus = [cpu_identity(decode(get(VMROOT + name + "/stdout.log")))
            for name in ("00-toolchain", "06-current-toolchain-check", "14-final-toolchain")]
    require(cpus[0] == cpus[1] == cpus[2] == recovery["native_cpu_compatibility"]["current"], "CPU changed across new build")
    old_cpu = cpu_identity(decode(get("/opt/r1/audit-pair-build/00-toolchain/stdout.log")))
    require(old_cpu == recovery["native_cpu_compatibility"]["previous"] and old_cpu != cpus[0], "old/new CPU boundary mismatch")
    for name in ("00-toolchain", "06-current-toolchain-check", "14-final-toolchain"):
        text = decode(get(VMROOT + name + "/stdout.log"))
        require("release: 1.97.0\n" in text, "wrong recorded Rust version")
        require(all(item["path"] in text.splitlines() for item in result["identities"]["compiler_binaries"]), "effective toolchain path differs")
    source_id, source = read_source(args.current_source, SOURCE_SHA)
    baseline_id, baseline = read_source(args.baseline_source, BASELINE_SHA)
    for key, identity in (("audit-source.tar.gz", source_id), ("baseline-source.tar.gz", baseline_id)):
        require(all(result["identities"][key][k] == identity[k] for k in ("sha256", "bytes")), "source archive binding differs")
        unavailable.pop(result["identities"][key]["path"], None)
    adapter = document(bound(result["identities"]["baseline_adapter_manifest"]))
    require(all(name in baseline and sha(baseline[name]) == digest for name, digest in adapter["before"].items()), "baseline source inputs differ")
    pins = document(get("/opt/r1/audit-tools/source-pins.json"))
    require(adapter["before"] == pins["build_inputs"], "baseline source pin set differs")
    shared = get("/opt/r1/audit-tools/audit_shared.rs.in")
    example = get("/opt/r1/audit-tools/audit_example.rs.in")
    text = source["crates/cli/src/sol.rs"].decode()
    left = text.index("// --- research-only saved-profile evaluation ")
    require(text[left:text.index("// --- strategy-source seam ", left)].encode() == shared
            and source["crates/cli/examples/hu_saved_profile_audit.rs"] == example, "source06 helper differs from audited shared helper")
    for name in ("01-baseline-pre-verify", "13-baseline-final-verify"):
        checked = document(get(VMROOT + name + "/stdout.log"))
        require(checked["mode"] == "verify" and checked["build_input_diff"] == []
                and checked["patch_id"] == adapter["patch_id"] and checked["baseline_identity"] == pins["identity"], "baseline adapter verification failed")
    cache = read("cache-seed.json")
    require(cache["historical_cache_reused"] is False and cache["cpu"] == cpus[0]
            and all(path.startswith("/opt/r1/target/current-recovery/") for path in cache["copied_directories"])
            and cache["destination"] == "/opt/r1/target/baseline-audit-recovery/release", "cache crossed the CPU boundary")
    binaries = {}
    for role in ("current_example", "baseline_example", "source06_solvers_not_paired_benchmark"):
        item = result["identities"][role]
        bound(item)
        if role in complete:
            require(complete[role] == item, "completion marker binary differs")
        require("-recovery/" in item["path"], "historical native binary selected")
        location = next(row["archive_member"] for row in members.values() if row["original_path"] == item["path"])
        binaries[role] = {**item, "archive_member": location, "availability": "verified ignored local bundle; not Git bytes"}
    assessment = recovery["prior_evidence_assessment"]
    require(set(assessment["excluded_stage_names"]) == {"12-baseline-example-release", "13-baseline-final-verify"}
            and assessment["excluded_stage_success_accepted"] is False, "damaged historical stages not excluded")
    return {"schema": "r1.vm06-recovery-build-verification/v1", "status": "verified_completed",
            "scope": "eight new recovery stages and resulting binaries only; no solver quality/performance acceptance",
            "verifier": file_identity(Path(__file__)), "bundle": bundle_id, "verified_bundle_payloads": len(payloads),
            "verified_recovery_retained_files": len(retained), "stages": stage_reports,
            "started_utc": result["started_utc"], "ended_utc": result["ended_utc"],
            "boot_id": result["boot_id"], "previous_boot_id": recovery["previous_boot_id"],
            "cpu": {"previous": old_cpu, "current": cpus[0], "unchanged_across_new_build": True},
            "source_archives": {"candidate_source06": source_id, "baseline_9632": baseline_id},
            "binaries": binaries, "historical_stage_success_excluded": assessment["excluded_stage_names"],
            "retained_but_not_semantically_adopted_historical_damage": assessment["excluded_text_evidence"],
            "recorded_input_identities_without_payload_in_this_bundle": list(unavailable.values()),
            "limits": ["Runtime/compiler and historical input hashes are recorded, but their missing payloads are not independently rehashed here.",
                       "Workspace tests from the old Intel boot are not recertified on the AMD boot.",
                       "General retention readiness remains false; this scoped verification does not modify it.",
                       "Source06 solvers binary is excluded from the source03 paired performance campaign.",
                       "No saved policy was evaluated by this build verification."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, default=HERE / "evidence-vm06-build")
    parser.add_argument("--bundle", type=Path, default=REPO / "runs/r1-cloud/vm06-recovery.tar.gz")
    parser.add_argument("--current-source", type=Path, default=HERE.parent / "validation/sources/current-06.tar.gz")
    parser.add_argument("--baseline-source", type=Path, default=REPO / "runs/r1-cloud/build-source-9632d8b.tar.gz")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    try:
        report = verify(args)
        encoded = json.dumps(report, indent=2, sort_keys=True, allow_nan=False) + "\n"
        if args.out:
            require(not args.out.resolve().is_relative_to(args.evidence.resolve()), "report must stay outside immutable evidence")
            with args.out.open("x", encoding="utf-8", newline="\n") as output:
                output.write(encoded)
            print(json.dumps({"status": report["status"], "new_stages": len(report["stages"]), "report": str(args.out)}))
        else:
            print(encoded, end="")
        return 0
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        print(f"VM06 build verification failed: {type(error).__name__}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
