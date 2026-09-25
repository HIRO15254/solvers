#!/usr/bin/env python3
"""Offline, scoped verification of the immutable VM07 checks/build bundles.

Never execute retained code or extract archives. Missing raw bundles fail closed.
Default output is JSON on stdout; --out creates a new report without overwriting.
Historical unresolved references remain reported, not repaired or waived globally.
"""
from __future__ import annotations

import argparse
from collections import Counter
import datetime as dt
import hashlib
import io
import json
import math
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
VM = "/opt/r1/codec-build07/"
CARGO = "/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/cargo"
BUNDLES = {
    "checks": ("92ec7553e89a2b822622395f902f16ab2cd65e67fdfcfc688a3f7cb6edfdf6fb", 2562647,
               "73733ca1a05f216f18405f5e3459276926e785f81a57f74dcbdd5c7f91899143", 25),
    "build": ("7c754fd93e2b503faa7d3fbbf0fbe6df039156404af1b2928ccd22589bec70ac", 9327291,
              "5afcfdfface202b38d2cf72d9cabc2e5c170ff6a588244c44fb472f5077de17f", 617),
}
INDEX_SHA = "ea53c710d5d3cc1673455d2edc984ab978d83eaeb5de7e67fc2a83f17c922a80"
SOURCE_SHA = "a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a"
BASELINE_SHA = "3de4afef13082dca9e17c38c9cc5f5d1734855f017d812ecb04e9cdbe0f7da27"
BENCH_SHA = "ed972ffce351fcd31dfbab74771a06a059ef0edb43e44d65397e65f9a85cbcea"
BOOT = "0727ebb3-36dd-4fae-b978-c629114703ed"
NAMES = ["toolchain", "fmt", "clippy", "workspace-tests", "python-tools", "release-cli",
         "release-codec-current", "release-codec-baseline"]
COMMANDS = [
    ["/bin/bash", "-euo", "pipefail", "-c", "rustc -Vv; cargo -V; uname -a; lscpu; free -b"],
    [CARGO, "fmt", "--all", "--check"],
    [CARGO, "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"],
    [CARGO, "test", "--locked", "--workspace", "--", "--test-threads=2"],
    ["/usr/bin/python3", "-m", "unittest", "discover", "-s", "tools/tests", "-v"],
    [CARGO, "build", "--locked", "--release", "-p", "cli", "--bin", "solvers", "--example", "hu_saved_profile_audit"],
    [CARGO, "build", "--locked", "--release", "-p", "formats", "--example", "sol_codec_bench"],
    [CARGO, "build", "--locked", "--release", "-p", "formats", "--example", "sol_codec_bench"],
]
TIMEOUTS = [30, 60, 1800, 1800, 300, 1800, 1200, 1800]
COMPAT_TESTS = [
    "byte_fields_have_literal_canonical_postcard_bytes",
    "byte_fields_preserve_json_arrays_and_integer_validation",
    "legacy_v1_payload_decodes_unchanged_but_container_is_still_rejected",
    "truncated_byte_fields_and_excessive_declared_lengths_are_rejected",
    "v3_chunks_keep_legacy_bytes_checksums_and_compression",
    "byte_fields_match_legacy_sequence_bytes_at_varint_boundaries",
]
HISTORICAL = {
    "cloud/preempted-05.json": 7, "cloud/preempted-06.json": 16, "cloud/transfers.json": 38,
    "pipeline/current-report.json": 20, "pipeline/vm06-comparison-report.json": 13,
}


def require(value, message):
    if not value:
        raise ValueError(message)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def text(raw):
    require(b"\0" not in raw, "NUL in adopted textual evidence")
    return raw.decode("utf-8")


def document(raw):
    def pairs(items):
        out = {}
        for key, value in items:
            require(key not in out, f"duplicate JSON key: {key}")
            out[key] = value
        return out
    def invalid(value):
        raise ValueError(f"nonfinite JSON constant: {value}")
    return json.loads(text(raw), object_pairs_hook=pairs, parse_constant=invalid)


def stamp(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "naive timestamp")
    return result


def safe_relative(name):
    path = PurePosixPath(name)
    require(not path.is_absolute() and ".." not in path.parts and "\\" not in name,
            f"unsafe relative path: {name}")
    return path


def identity(path, raw):
    return {"path": path, "bytes": len(raw), "sha256": sha(raw)}


def match(raw, item):
    require(type(item["bytes"]) is int and len(raw) == item["bytes"] and sha(raw) == item["sha256"],
            f"size/hash mismatch: {item.get('path', item.get('original_path'))}")
    return raw


def source_members(raw, expected):
    require(sha(raw) == expected, "source archive hash mismatch")
    files, total = {}, 0
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        for member in archive:
            safe_relative(member.name)
            if member.isdir():
                continue
            require(member.isfile() and member.name not in files and member.size <= 32 * 1024**2,
                    "unsafe/duplicate/oversized source member")
            total += member.size
            require(total <= 128 * 1024**2, "source decompressed size exceeds bound")
            files[member.name] = archive.extractfile(member).read()
    return files


def rust_counts(log):
    rows = re.findall(r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
                      r"(\d+) measured; (\d+) filtered out; finished in (\S+)s$", log, re.M)
    require(rows and all(row[0] == "ok" for row in rows), "missing or failed Rust summaries")
    counts = dict(zip(["passed", "failed", "ignored", "measured", "filtered_out"],
                      [sum(int(row[i]) for row in rows) for i in range(1, 6)]))
    lines = re.findall(r"^test (.+?) \.\.\. (ok|ignored(?:, [^\n]*)?|FAILED)$", log, re.M)
    require(sum(status == "ok" for _, status in lines) == counts["passed"], "Rust passed line mismatch")
    require(sum(status.startswith("ignored") for _, status in lines) == counts["ignored"], "Rust ignored line mismatch")
    runs = [int(n) for n in re.findall(r"^running (\d+) tests?$", log, re.M)]
    require(len(runs) == len(rows) and sum(runs) == sum(counts.values()), "Rust test totals incomplete")
    require(counts == {"passed": 901, "failed": 0, "ignored": 31, "measured": 0, "filtered_out": 0},
            "unexpected frozen workspace result")
    for name in COMPAT_TESTS:
        require(lines.count((name, "ok")) == 1, f"compatibility test absent/duplicated: {name}")
    return {"summary_blocks": len(rows), **counts, "compatibility_tests_passed": COMPAT_TESTS,
            "ignored_tests_executed": False}


def verify(evidence, bundle_dir):
    index_raw = (evidence / "compact-index.json").read_bytes()
    require(sha(index_raw) == INDEX_SHA, "immutable compact index changed")
    index = document(index_raw)
    require(len(index["files"]) == 630, "unexpected compact inventory")
    indexed = set()
    for row in index["files"]:
        safe_relative(row["path"])
        require(row["path"] not in indexed, "duplicate compact path")
        indexed.add(row["path"])
        match((evidence / row["path"]).read_bytes(), row)
    actual_files = {p.relative_to(evidence).as_posix() for p in evidence.rglob("*") if p.is_file()}
    require(actual_files == indexed | {"compact-index.json"}, "unexpected/missing selected file")
    retention = document((evidence / "retention.json").read_bytes())
    validation = document((evidence / "validation.json").read_bytes())
    require(retention["validation"] == validation, "retention validation copy differs")
    require(len(retention["bundles"]) == 2 and len(retention["files"]) == 642, "retention scope changed")

    payloads, locations, bundles, manifest_rows = {}, {}, [], {}
    for label, (digest, size, manifest_hash, count) in BUNDLES.items():
        archive_path = bundle_dir / f"evidence-vm07-{label}.tar.gz"
        require(archive_path.stat().st_size == size, f"{label} raw bundle size differs")
        raw_archive = archive_path.read_bytes()
        require(len(raw_archive) == size and sha(raw_archive) == digest, f"{label} raw bundle changed")
        sidecar = Path(str(archive_path) + ".manifest.json").read_bytes()
        require(sha(sidecar) == manifest_hash, "collector manifest changed")
        collector = document(sidecar)
        checksum = Path(str(archive_path) + ".sha256").read_bytes()
        require(text(checksum).strip() == digest + "  " + collector["archive_filename"], "checksum sidecar differs")
        rb = next(x for x in retention["bundles"] if x["label"] == label)
        match(raw_archive, rb["archive"])
        match(sidecar, rb["manifest"])
        match(checksum, rb["checksum"])
        require(collector["schema"] == "solvers.r1-retention/v1" and len(collector["files"]) == count,
                "unexpected collector schema/count")
        rows = {row["archive_member"]: row for row in collector["files"]}
        require(len(rows) == count and all(r["included"] is True and r["kind"] == "regular" for r in rows.values()),
                "duplicate or skipped payload")
        seen, total = set(), 0
        with tarfile.open(fileobj=io.BytesIO(raw_archive), mode="r:gz") as archive:
            for member in archive:
                require(member.isfile() and member.name not in seen and member.size <= 64 * 1024**2,
                        "nonregular, duplicate or oversized bundle member")
                seen.add(member.name)
                total += member.size
                require(total <= 128 * 1024**2, "bundle decompressed size exceeds bound")
                raw = archive.extractfile(member).read()
                if member.name == "retention-manifest.json":
                    require(raw == sidecar, "embedded collector manifest differs")
                    continue
                require(member.name in rows, "unlisted payload")
                row = rows[member.name]
                match(raw, row)
                original = row["original_path"]
                if original in payloads:
                    require(payloads[original] == raw, "different bytes at same cross-bundle VM path")
                payloads[original] = raw
                location = {"bundle": label, "archive_path": f"runs/r1-cloud/{archive_path.name}",
                            "archive_sha256": digest, "member": member.name}
                locations.setdefault(original, []).append(location)
                manifest_rows[(label, original)] = row
        require(seen == {"retention-manifest.json", *rows}, "bundle member set differs")
        bundles.append({"label": label, "path": f"runs/r1-cloud/{archive_path.name}", "bytes": size,
                        "sha256": digest, "manifest_sha256": manifest_hash, "verified_payloads": count,
                        "skipped_payloads": 0, "availability": "verified_local_ignored_bundle_not_git"})
    require(len(manifest_rows) == 642, "duplicate manifest VM path")
    retained_keys = set()
    for row in retention["files"]:
        key = (row["bundle"], row["original_path"])
        require(key not in retained_keys and key in manifest_rows, "invalid retained payload")
        retained_keys.add(key)
        source = manifest_rows[key]
        for field in ("bytes", "sha256", "archive_member", "included", "kind"):
            require(row[field] == source[field], f"retention differs: {field}")
        if row["compact_path"]:
            require((evidence / row["compact_path"]).read_bytes() == payloads[row["original_path"]], "selected copy differs")
    require(retained_keys == set(manifest_rows), "incomplete retained inventory")

    def get(path):
        require(path in payloads, f"required VM07 evidence missing: {path}")
        return payloads[path]

    def bound(item):
        return match(get(item["path"]), item)

    def ref(path):
        return {**identity(path, get(path)), "locations": locations[path]}

    result = document(get(VM + "result.json"))
    require(result["schema"] == "r1.codec-build/v1" and result["status"] == "passed" and "error" not in result,
            "build did not complete")
    require(result["boot_id"] == BOOT and result["outer_cgroup_required"] is True and result["dispatch_seconds"] == 7200,
            "unexpected host/dispatch scope")
    require(result["planned_stages"] == NAMES and [s["name"] for s in result["stages"]] == NAMES,
            "stage order/coverage differs")
    begin, end = stamp(result["started_utc"]), stamp(result["ended_utc"])
    require(begin < end and begin.date() == end.date() == dt.date(2026, 9, 25), "invalid build interval")
    elapsed = result["elapsed_seconds"]
    require(math.isfinite(elapsed) and 0 < elapsed < 7200 and abs((end-begin).total_seconds()-elapsed) < 1,
            "build monotonic/wall interval mismatch")
    source_current = source_members(get("/opt/r1/source-07.tar.gz"), SOURCE_SHA)
    source_baseline = source_members(get("/opt/r1/codec-baseline-source.tar.gz"), BASELINE_SHA)
    require(len(source_current) == 332 and len(source_baseline) == 240, "source member coverage changed")
    bench = "crates/formats/examples/sol_codec_bench.rs"
    require(bench not in source_baseline and sha(source_current[bench]) == BENCH_SHA, "unexpected benchmark source")
    source_baseline[bench] = source_current[bench]
    for label, prefix, source in [("current", "/opt/r1/current/", source_current),
                                  ("baseline", "/opt/r1/baseline-codec/", source_baseline)]:
        rows = result["identities"][label]
        expected_paths = {prefix + name for name in source}
        require(len(rows) == len(expected_paths) and {x["path"] for x in rows} == expected_paths,
                "build source identity coverage differs")
        for row in rows:
            require(bound(row) == source[row["path"].removeprefix(prefix)], "source archive/build bytes differ")
    bound(result["identities"]["supervisor"])
    bound(result["identities"]["driver"])
    require(result["fresh_targets"] == ["/opt/r1/target/codec-current", "/opt/r1/target/codec-baseline"], "target scope differs")
    require(result["build_environment"]["RUSTUP_TOOLCHAIN"] == "1.97.0"
            and result["build_environment"]["CARGO_BUILD_JOBS"] == "4"
            and result["build_environment"]["CARGO_INCREMENTAL"] == "0", "build environment differs")

    stage_reports, runtime_unretained, last_end = [], {}, begin
    for i, (stage, argv, timeout) in enumerate(zip(result["stages"], COMMANDS, TIMEOUTS)):
        prefix = VM + f"{i:02d}-{stage['name']}/"
        record = document(get(prefix + "supervisor.json"))
        cwd = "/opt/r1/baseline-codec" if i == 7 else "/opt/r1/current"
        target = "/opt/r1/target/codec-baseline" if i == 7 else "/opt/r1/target/codec-current"
        require(stage["argv"] == record["argv"] == argv and stage["cwd"] == record["cwd"] == cwd
                and stage["target"] == target, "stage command/source differs")
        require(stage["status"] == "passed" and stage["exit_code"] == 0 and type(stage["exit_code"]) is int,
                "failed build stage")
        require(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
                and record["stop_reason"] == "completed" and record["cleanup_complete"] is True
                and record["identity_unchanged"] is True and record["forced"] is False
                and record["shell"] is False and record["errors"] == []
                and record["events"] == [] and record["stop_requested_at"] is None,
                "incomplete/error/forced supervisor stage")
        require(type(record["child_exit_code"]) is int and type(record["supervisor_exit_code"]) is int
                and record["child_exit_code"] == record["supervisor_exit_code"] == 0, "nonzero stage exit")
        require(record["identity_before"] == record["identity_after"], "identity changed during stage")
        for item in record["identity_before"]:
            if item["path"] in payloads:
                bound(item)
            else:
                require(item["path"] in {CARGO, "/usr/bin/bash", "/usr/bin/python3.12"}, "unretained source input")
                if item["path"] in runtime_unretained:
                    require(runtime_unretained[item["path"]] == item, "runtime identity differs between stages")
                runtime_unretained[item["path"]] = item
        limits = record["limits"]
        require(limits == {"timeout_seconds": timeout, "grace_seconds": 5, "kill_wait_seconds": 5,
                           "poll_seconds": 0.25, "memory_limit_bytes": 40*1024**3,
                           "min_free_memory_bytes": 8*1024**3, "disk_reserve_bytes": 12*1024**3}, "resource limits differ")
        t0, t1 = stamp(stage["started_utc"]), stamp(stage["ended_utc"])
        s0, s1 = stamp(record["started_at"]), stamp(record["ended_at"])
        require(last_end <= t0 <= stamp(record["created_at"]) <= s0 <= s1 <= t1 <= end, "stage timestamps overlap/outside build")
        last_end = t1
        require(0 <= record["elapsed_seconds"] < timeout and math.isfinite(record["elapsed_seconds"])
                and abs((s1-s0).total_seconds()-record["elapsed_seconds"]) < 1, "stage interval mismatch")
        require(record["runtime"]["logical_cpus"] == 8 and record["runtime"]["machine"] == "x86_64"
                and record["containment"]["kind"] == "posix_session_process_group"
                and record["last_sample"]["pids"] == [], "runtime/cleanup scope differs")
        logs = {}
        for kind, name in [("stdout", "stdout.log"), ("stderr", "stderr.log"), ("samples", "samples.jsonl")]:
            require(record["outputs"][kind]["path"] == prefix + name, "output points outside stage")
            logs[kind] = text(bound(record["outputs"][kind]))
        samples = [document(line.encode()) for line in logs["samples"].splitlines() if line]
        require(len(samples) == record["measurement"]["sample_count"] and samples, "sample coverage mismatch")
        require(samples[-1]["pids"] == [] and all(math.isfinite(x["elapsed_seconds"]) for x in samples), "invalid final sample")
        stage_reports.append({"name": stage["name"], "status": "passed", "argv": argv, "cwd": cwd,
                              "record": ref(prefix + "supervisor.json"), "elapsed_seconds": record["elapsed_seconds"],
                              "cleanup_complete": True, "output_hashes_verified": True})

    workspace = rust_counts(text(get(VM + "03-workspace-tests/stdout.log")))
    pylog = text(get(VM + "04-python-tools/stderr.log"))
    require(re.search(r"^Ran 32 tests in [0-9.]+s$", pylog, re.M)
            and re.search(r"^OK \(skipped=3\)$", pylog, re.M), "Python tests did not complete")
    pyok = re.findall(r"^test_.* \.\.\. ok$", pylog, re.M)
    pyskip = re.findall(r"^(test_.*) \.\.\. skipped (.*)$", pylog, re.M)
    require(len(pyok) == 29 and len(pyskip) == 3 and all("test_windows_" in x[0] for x in pyskip), "Python test count mismatch")
    binary_reports = []
    require(len(result["binaries"]) == 4 and len({x["path"] for x in result["binaries"]}) == 4, "binary count differs")
    expected_binaries = ["/opt/r1/target/codec-current/release/solvers",
                         "/opt/r1/target/codec-current/release/examples/hu_saved_profile_audit",
                         "/opt/r1/target/codec-current/release/examples/sol_codec_bench",
                         "/opt/r1/target/codec-baseline/release/examples/sol_codec_bench"]
    require([x["path"] for x in result["binaries"]] == expected_binaries, "binary identity scope differs")
    for item in result["binaries"]:
        raw = bound(item)
        require(raw.startswith(b"\x7fELF"), "retained binary is not ELF")
        binary_reports.append({**ref(item["path"]), "availability": "verified_local_ignored_bundle_not_git"})

    historical_counts = Counter()
    for item in validation["unresolved_references"]:
        if item["required"]:
            prefix = "records/build/current/experiments/hu-postflop-r1/"
            require(item["record"].startswith(prefix), "unresolved required VM07 runtime/build evidence")
            short = item["record"].removeprefix(prefix)
            require(short in HISTORICAL, "unrecognized unresolved historical owner")
            historical_counts[short] += 1
    require(dict(historical_counts) == HISTORICAL and validation["ready"] is False
            and validation["problems"] == [{"count": 94, "reason": "required_linked_evidence_missing"}],
            "generic retainer boundary differs")
    toolchain = text(get(VM + "00-toolchain/stdout.log"))
    require("rustc 1.97.0" in toolchain and "cargo 1.97.0" in toolchain
            and "AMD EPYC 7B12" in toolchain and "solvers-r1-20260925-07" in toolchain, "toolchain/host log differs")
    rustc = "/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/rustc"
    require(get(rustc).startswith(b"\x7fELF"), "retained rustc is not ELF")
    product = lambda path: path.startswith("crates/") or path in {"Cargo.toml", "Cargo.lock", ".cargo/config.toml"}
    differences = sorted(name for name in source_current.keys() | source_baseline.keys()
                         if product(name) and source_current.get(name) != source_baseline.get(name))
    require(differences == ["crates/formats/src/sol.rs", "crates/formats/tests/sol_byte_serde.rs",
                            "crates/holdem/tests/oracle_river.rs"],
            "codec baseline has unexpected product differences")
    oracle = "crates/holdem/tests/oracle_river.rs"
    require(source_current[oracle].replace(b"\r\n", b"\n") == source_baseline[oracle]
            and source_current[oracle].count(b"\r\n") == 486
            and b"\r" not in source_baseline[oracle]
            and sha(source_baseline[oracle]) == "05fbd787da6c12d286b7ba0967b2581bb603ea7b14194af7fb342b081b613de1",
            "oracle test difference is not solely the recorded CRLF/LF conversion")
    return {
        "schema": "r1.vm07-scoped-validation/v1", "status": "passed", "quality_acceptance": "not_evaluated",
        "scope": "VM07 source07 checks and eight codec-build stages only; no performance/solver/external acceptance",
        "verifier": identity("experiments/hu-postflop-r1/validation/verify-vm07.py", Path(__file__).read_bytes()),
        "immutable_index_sha256": INDEX_SHA, "compact_files_verified": 630,
        "bundles": bundles, "payloads_verified": 642, "distinct_vm_paths": len(payloads),
        "result": ref(VM + "result.json"), "started_utc": result["started_utc"], "ended_utc": result["ended_utc"],
        "elapsed_seconds": elapsed, "boot_id": BOOT, "host": "solvers-r1-20260925-07",
        "cpu": "AMD EPYC 7B12", "logical_cpus": 8, "rust_toolchain": "1.97.0",
        "sources": {"current": {**ref("/opt/r1/source-07.tar.gz"), "build_files_verified": 332},
                    "codec_baseline": {**ref("/opt/r1/codec-baseline-source.tar.gz"), "archive_files_verified": 240,
                                       "build_files_verified": 241, "added_identical_benchmark_sha256": BENCH_SHA},
                    "byte_difference_paths": differences,
                    "production_difference_paths": ["crates/formats/src/sol.rs"],
                    "new_test_paths": ["crates/formats/tests/sol_byte_serde.rs"],
                    "test_line_endings_only": {"path": oracle,
                                               "current": identity(oracle, source_current[oracle]),
                                               "baseline": identity(oracle, source_baseline[oracle]),
                                               "crlf_count_current": 486,
                                               "after_crlf_to_lf_exact": True,
                                               "baseline_matches_independently_inspected_git_blob": "88ffa5d:" + oracle},
                    "baseline_is_original_r0_9632": False},
        "driver": result["identities"]["driver"], "supervisor": result["identities"]["supervisor"],
        "build_isolation": {"fresh_targets": result["fresh_targets"],
                            "evidence": "Pinned driver rejects existing targets and checks source/boot before and after each stage.",
                            "outer_systemd_configuration_retained": ref("/home/PC_User/setup-codec-vm07.sh"),
                            "outer_systemd_runtime_properties_independently_observed": False},
        "stages": stage_reports, "workspace_tests": workspace,
        "python_tests": {"run": 32, "passed": 29, "skipped": 3, "failed": 0, "skipped_names": [x[0] for x in pyskip]},
        "binaries": binary_reports, "retained_rustc": ref(rustc),
        "runtime_identities_without_retained_bytes": list(runtime_unretained.values()),
        "generic_retainer": {"ready": False, "required_unresolved": 94,
                             "required_unresolved_by_historical_record": dict(historical_counts),
                             "other_optional_unresolved": len(validation["unresolved_references"])-94,
                             "scope_disposition": "All 94 required references belong to five historical JSON files inside source07; none belongs to VM07 build/check records. No historical reference is repaired or certified here."},
        "limitations": ["Raw bundles and retained binaries are local ignored evidence, not Git-backed; both bundles are required to rerun this verifier.",
                        "Ordinary workspace tests leave 31 ignored tests unexecuted; Python leaves three Windows-only tests skipped.",
                        "Cargo, Python and bash bytes were not retained; pre/post hashes are recorded and consistent, not independently rehashed here.",
                        "The scoped pass does not change generic retainer ready=false or certify historical runs, numerical quality or R1 acceptance."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, default=HERE / "vm07-complete")
    parser.add_argument("--bundle-dir", type=Path, default=REPO / "runs/r1-cloud")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    try:
        report = verify(args.evidence, args.bundle_dir)
        rendered = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
        if args.out:
            require(not args.out.resolve().is_relative_to(args.evidence.resolve()), "do not mutate immutable evidence")
            with args.out.open("x", encoding="utf-8", newline="\n") as out:
                out.write(rendered)
        else:
            print(rendered, end="")
        return 0
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        print(json.dumps({"status": "failed", "error": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
