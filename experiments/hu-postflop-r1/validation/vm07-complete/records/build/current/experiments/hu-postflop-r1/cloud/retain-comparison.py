#!/usr/bin/env python3
"""Verify downloaded collector bundles and retain local comparison evidence.

No cloud, solver, Git mutation, or deletion of existing evidence. Python 3.11+.

retain --bundle vm06=runs/r1-cloud/results.tar.gz --sha256 vm06=SHA256
       --out experiments/hu-postflop-r1/evidence-vm06
       --require-scope performance --require-scope saved-profile --require-scope phases
       [--external /opt/r1/audit-source.tar.gz=LOCAL_SOURCE_ARCHIVE]
verify --directory experiments/hu-postflop-r1/evidence-vm06
self-test

Repeat --bundle/--sha256 to include prior build/reboot evidence. Collector
sidecars (.manifest.json and .sha256) are mandatory. All compact source,
config, JSON/JSONL, stdout/stderr and freeze records up to 1 MiB are copied
byte-for-byte under records/<bundle>/<root>/. SOL/CKPT up to 8 MiB are also
retained. Larger exports, binaries and archives stay in the verified local
bundles; retention.json records their exact member and hash.
--external binds an already retained local file to its recorded VM path.

Readiness is evidence completeness, NEVER solver quality or performance
acceptance. Incomplete/failed records remain intact, with readiness=false and
exit 1. Hash/path corruption exits 2 without publishing a retained directory.
Compact files must subsequently be committed: verify --require-git-tracked
checks index presence, but neither invocation claims an uncommitted file is
already Git-backed. Keep the downloaded bundles after VM/disk removal.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import time
import uuid

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
SCHEMA = "r1.local-comparison-retention/v1"
COLLECTOR_SCHEMA = "solvers.r1-retention/v1"
COMPACT = {".json", ".jsonl", ".log", ".toml", ".txt", ".md", ".csv", ".sha256",
           ".py", ".rs", ".sh", ".in", ".lock", ".yaml", ".yml"}
MAX_MEMBER = 2 * 1024**3
MAX_TOTAL = 8 * 1024**3
MAX_JSON = 32 * 1024**2
HEX = re.compile(r"[0-9a-f]{64}\Z")
LABEL = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}\Z")
EXPECTED = {(case, version, repetition) for case in ("river", "turn", "flop")
            for version in ("baseline", "candidate") for repetition in (1, 2, 3)}


def compact_file(name, size):
    suffix = PurePosixPath(name).suffix.lower()
    if suffix in {".sol", ".ckpt"}:
        return size <= 8 * 1024**2
    return (suffix in COMPACT or PurePosixPath(name).name == "Cargo.lock") and size <= 1024**2


def named(value):
    if "=" not in value:
        raise argparse.ArgumentTypeError("expected LABEL=VALUE")
    label, value = value.split("=", 1)
    if not label or not value:
        raise argparse.ArgumentTypeError("both sides of '=' are required")
    return label, value


def encode(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True, allow_nan=False) + "\n").encode()


def parse_json(raw, context):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"duplicate JSON key in {context}: {key}")
            result[key] = value
        return result
    def constant(value):
        raise ValueError(f"nonfinite JSON constant in {context}: {value}")
    return json.loads(raw, object_pairs_hook=pairs, parse_constant=constant)


def identity(path):
    path = Path(path).absolute()
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError(f"not a regular retained file: {path}")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    after = path.lstat()
    stamp = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns)
    if stamp(before) != stamp(after):
        raise ValueError(f"file changed while hashing: {path}")
    return {"path": str(path.resolve()), "sha256": digest.hexdigest(), "bytes": after.st_size}


def verify_identity(item):
    actual = identity(item["path"])
    if any(actual[key] != item[key] for key in ("path", "sha256", "bytes")):
        raise ValueError(f"retained identity mismatch: {item['path']}")


def relative_name(value):
    if not isinstance(value, str) or not value or "\\" in value:
        raise ValueError(f"unsafe relative evidence path: {value!r}")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ("", ".", "..") for part in value.split("/")):
        raise ValueError(f"unsafe relative evidence path: {value!r}")
    for part in path.parts:
        if re.search(r'[<>:"|?*\x00-\x1f]', part) or part.endswith((".", " ")):
            raise ValueError(f"nonportable evidence path: {value!r}")
        if part.split(".", 1)[0].upper() in {"CON", "PRN", "AUX", "NUL", *[f"COM{i}" for i in range(1, 10)], *[f"LPT{i}" for i in range(1, 10)]}:
            raise ValueError(f"reserved evidence path: {value!r}")
    return path.as_posix()


def write_new(path, raw):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(raw)


def bundle_inventory(label, path, expected, destination, budget, seen_outputs):
    archive_id = identity(path)
    if archive_id["bytes"] > MAX_MEMBER or not HEX.fullmatch(expected) or archive_id["sha256"] != expected:
        raise ValueError(f"downloaded archive SHA-256 mismatch: {label}")
    sidecar_path = Path(str(path) + ".manifest.json")
    checksum_path = Path(str(path) + ".sha256")
    sidecar_id, checksum_id = identity(sidecar_path), identity(checksum_path)
    sidecar = sidecar_path.read_bytes()
    if hashlib.sha256(sidecar).hexdigest() != sidecar_id["sha256"]:
        raise ValueError("collector sidecar changed while reading")
    if len(sidecar) > MAX_JSON:
        raise ValueError("collector manifest exceeds JSON bound")
    manifest = parse_json(sidecar, sidecar_path)
    if manifest.get("schema") != COLLECTOR_SCHEMA:
        raise ValueError("unsupported collector manifest")
    checksum_raw = checksum_path.read_bytes()
    if hashlib.sha256(checksum_raw).hexdigest() != checksum_id["sha256"]:
        raise ValueError("collector checksum changed while reading")
    checksum = checksum_raw.decode("utf-8").strip()
    if checksum != expected + "  " + manifest["archive_filename"]:
        raise ValueError("collector checksum sidecar disagrees with expected digest/name")
    records, members, path_keys = [], {}, set()
    for item in manifest["files"]:
        if not LABEL.fullmatch(item["root"]):
            raise ValueError("invalid collector root label")
        relative = relative_name(item["relative_path"])
        if type(item["bytes"]) is not int or item["bytes"] < 0 or not HEX.fullmatch(item["sha256"]):
            raise ValueError("invalid collector file identity")
        key = (item["root"], relative)
        if key in path_keys:
            raise ValueError("duplicate collector inventory path")
        path_keys.add(key)
        record = dict(item, bundle=label, compact_path=None)
        records.append(record)
        if item["included"] is not True:
            record["local_availability"] = "not_retained_in_bundle"
            continue
        member = item["archive_member"]
        if item["kind"] != "regular" or not re.fullmatch(r"files/[0-9]{8}", member) or member in members:
            raise ValueError("invalid/duplicate included archive member")
        members[member] = record
        record["local_availability"] = "verified_local_bundle_not_git"
        if compact_file(relative, item["bytes"]):
            compact = f"records/{label}/{item['root']}/{relative}"
            if compact.casefold() in seen_outputs:
                raise ValueError("case-insensitive compact destination collision")
            seen_outputs.add(compact.casefold())
            record["compact_path"] = compact
            record["local_availability"] = "compact_file_pending_git_commit_and_verified_local_bundle"
            budget[0] += item["bytes"]
            if budget[0] > budget[1]:
                raise ValueError("compact evidence exceeds limit; never silently skip logs/config/source")
    observed, total = set(), 0
    with tarfile.open(path, "r:gz") as archive:
        for member in archive:
            if member.name in observed or not member.isfile() or member.size > MAX_MEMBER:
                raise ValueError(f"duplicate/nonregular/oversized archive member: {member.name}")
            observed.add(member.name)
            total += member.size
            if total > MAX_TOTAL:
                raise ValueError("uncompressed bundle exceeds bound")
            source = archive.extractfile(member)
            if member.name == "retention-manifest.json":
                if member.size != len(sidecar) or source.read() != sidecar:
                    raise ValueError("internal and downloaded collector manifests differ")
                continue
            record = members.get(member.name)
            if record is None or member.size != record["bytes"]:
                raise ValueError(f"unlisted member or size mismatch: {member.name}")
            target = destination / record["compact_path"] if record["compact_path"] else None
            if target:
                target.parent.mkdir(parents=True, exist_ok=True)
            digest = hashlib.sha256()
            output = target.open("xb") if target else None
            try:
                for block in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(block)
                    if output:
                        output.write(block)
            finally:
                if output:
                    output.close()
                source.close()
            if digest.hexdigest() != record["sha256"]:
                raise ValueError(f"archive payload SHA-256 mismatch: {member.name}")
    if observed != {"retention-manifest.json", *members}:
        raise ValueError("archive member set differs from collector inventory")
    verify_identity(archive_id)
    verify_identity(sidecar_id)
    verify_identity(checksum_id)
    write_new(destination / f"bundles/{label}/collector-manifest.json", sidecar)
    write_new(destination / f"bundles/{label}/collector.sha256", checksum_raw)
    return {"label": label, "archive": archive_id, "manifest": sidecar_id, "checksum": checksum_id,
            "included_files": len(members), "uncompressed_bytes": total,
            "excluded_directories": manifest.get("excluded_directories", [])}, records


def walk(value, pointer=""):
    yield pointer, value
    if isinstance(value, dict):
        for key, child in value.items():
            yield from walk(child, pointer + "/" + str(key))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from walk(child, pointer + "/" + str(index))


def critical_reference(path):
    return (PurePosixPath(path).suffix.lower() in COMPACT | {".sol", ".ckpt", ".gz"}
            or "/target/" in path)


def structured_documents(destination, compact_paths, problems):
    def parsed(raw, record):
        try:
            return parse_json(raw, record)
        except ValueError as error:
            # Corrupted historical records are evidence too. Preserve their
            # exact bytes, but never count them as ready structured evidence.
            problems.append({"reason": "invalid_structured_evidence", "record": record,
                             "error_type": type(error).__name__})
            return None
    for compact_path in compact_paths:
        path = destination / compact_path
        if path.suffix not in (".json", ".jsonl"):
            continue
        if path.stat().st_size > MAX_JSON:
            problems.append({"reason": "structured_evidence_exceeds_parse_bound", "record": compact_path})
            continue
        if path.suffix == ".jsonl":
            with path.open("rb") as stream:
                for number, line in enumerate(stream, 1):
                    if line.strip():
                        record = compact_path + f"#L{number}"
                        yield record, parsed(line, record)
        else:
            yield compact_path, parsed(path.read_bytes(), compact_path)


def inspect_evidence(destination, records, external, required):
    available = {(r["original_path"], r["sha256"], r["bytes"]) for r in records if r["included"] is True}
    available.update((r["original_path"], r["identity"]["sha256"], r["identity"]["bytes"]) for r in external)
    problems, unresolved, provenance, reports = [], [], [], []
    for record in records:
        if record["included"] is not True:
            problems.append({"reason": "inventoried_file_not_retained", "path": record["original_path"]})
    compact_paths = [r["compact_path"] for r in records if r["compact_path"]]
    compact_paths.extend(r["compact_path"] for r in external if r.get("compact_path"))
    scopes = set()
    for path, document in structured_documents(destination, compact_paths, problems):
        for pointer, value in walk(document):
            if isinstance(value, dict) and {"path", "sha256", "bytes"} <= value.keys():
                key = (value["path"], value["sha256"], value["bytes"])
                if isinstance(key[0], str) and isinstance(key[1], str) and HEX.fullmatch(key[1]) and type(key[2]) is int:
                    if any(word in pointer.lower() for word in ("source", "binary", "example", "compiler")):
                        provenance.append({"record": path, "pointer": pointer, "kind": "source_or_binary_identity", "value": value})
                    if key not in available:
                        item = {"record": path, "pointer": pointer, "identity": dict(zip(("path", "sha256", "bytes"), key)),
                                "required": critical_reference(key[0])}
                        unresolved.append(item)
            if isinstance(value, dict):
                for key in ("boot_id", "previous_boot_id", "hostname", "source_version", "instrumentation_id"):
                    if key in value:
                        provenance.append({"record": path, "pointer": pointer + "/" + key, "kind": key, "value": value[key]})
                if "native_cpu_compatibility" in value:
                    provenance.append({"record": path, "pointer": pointer + "/native_cpu_compatibility",
                                       "kind": "native_cpu_compatibility", "value": value["native_cpu_compatibility"]})
        if not isinstance(document, dict):
            continue
        kind = document.get("kind")
        if document.get("schema") == "solvers.supervised-run/v1" and document.get("state") == "completed":
            outputs = document.get("outputs", {})
            for name in ("stdout", "stderr", "samples"):
                if not {"path", "sha256", "bytes"} <= outputs.get(name, {}).keys():
                    problems.append({"reason": "completed_supervisor_output_identity_missing", "record": path, "output": name})
        if kind not in ("comparison", "saved_profile_audits"):
            continue
        runs = document.get("runs", [])
        phase = kind == "comparison" and any("r1_phase" in r.get("solve", {}) for r in runs)
        scope = "phases" if phase else "performance" if kind == "comparison" else "saved-profile"
        scopes.add(scope)
        keys = [(r.get("case"), r.get("version"), r.get("repetition")) for r in runs]
        complete = document.get("state") == "completed" and len(keys) == 18 and set(keys) == EXPECTED
        modes = sorted({r.get("solve", {}).get("r1_phase", {}).get("mode") for r in runs
                        if r.get("solve", {}).get("r1_phase", {}).get("mode")})
        reports.append({"record": path, "scope": scope, "state": document.get("state"), "runs": len(keys),
                        "all_18_unique_original_cases": len(keys) == 18 and set(keys) == EXPECTED, "phase_modes": modes})
        if scope in required and not complete:
            problems.append({"reason": "required_campaign_incomplete", "record": path, "scope": scope})
        if scope in required:
            if not isinstance(document.get("plan"), dict) or not {"path", "sha256", "bytes"} <= document["plan"].keys():
                problems.append({"reason": "required_frozen_plan_identity_missing", "record": path, "scope": scope})
            for index, row in enumerate(runs):
                if kind == "comparison":
                    missing = {"solution.sol", "checkpoint.ckpt", "run.toml", "run.json", "progress.jsonl"} - row.get("artifacts", {}).keys()
                    if missing:
                        problems.append({"reason": "required_run_artifact_identity_missing", "record": path,
                                         "row": index, "artifacts": sorted(missing)})
                    if phase and row.get("solve", {}).get("r1_phase", {}).get("mode") == "on":
                        record_id = row["solve"]["r1_phase"].get("record")
                        if not isinstance(record_id, dict) or not {"path", "sha256", "bytes"} <= record_id.keys():
                            problems.append({"reason": "required_phase_record_identity_missing", "record": path, "row": index})
                elif not isinstance(row.get("report"), dict):
                    problems.append({"reason": "saved_profile_report_missing", "record": path, "row": index})
    for scope in sorted(set(required) - scopes):
        problems.append({"reason": "required_campaign_record_missing", "scope": scope})
    if "phases" in required:
        modes = {mode for report in reports if report["scope"] == "phases" for mode in report["phase_modes"]}
        if modes != {"on", "off"}:
            problems.append({"reason": "phase_on_off_campaigns_missing", "modes": sorted(modes)})
    missing = [item for item in unresolved if item["required"]]
    if missing:
        problems.append({"reason": "required_linked_evidence_missing", "count": len(missing)})
    boot_ids = sorted({p["value"] for p in provenance if p["kind"] in ("boot_id", "previous_boot_id") and isinstance(p["value"], str)})
    return {"ready": not problems, "quality_acceptance": "not_evaluated_by_retainer", "problems": problems,
            "required_scopes": required, "reports": reports, "unresolved_references": unresolved,
            "provenance_observations": provenance, "distinct_boot_ids": boot_ids,
            "multiple_boots_observed": len(boot_ids) > 1,
            "boot_source_policy": "all observations retain their record/pointer; never attribute old validation or native binaries to another boot/source"}


def retain(args):
    bundles, hashes = dict(args.bundle), dict(args.sha256)
    if len(bundles) != len(args.bundle) or len(hashes) != len(args.sha256) or set(bundles) != set(hashes):
        raise ValueError("each unique bundle label requires exactly one expected SHA-256")
    if not all(LABEL.fullmatch(label) for label in bundles):
        raise ValueError("invalid bundle label")
    out = args.out.absolute()
    if not out.resolve().is_relative_to((REPO / "experiments").resolve()) or out.exists() or out.is_symlink():
        raise ValueError("output must be a new directory under this repository's experiments/")
    for parent in (out.parent, *out.parents):
        if parent.is_symlink():
            raise ValueError("symlink in output ancestry")
    if not 0 < args.max_compact_bytes <= MAX_TOTAL:
        raise ValueError("invalid compact byte bound")
    out.parent.mkdir(parents=True, exist_ok=True)
    staging = out.with_name("." + out.name + ".retaining-" + uuid.uuid4().hex)
    staging.mkdir()
    try:
        archives, records, seen = [], [], set()
        budget = [0, args.max_compact_bytes]
        for label, path in bundles.items():
            archive, entries = bundle_inventory(label, Path(path).resolve(strict=True), hashes[label], staging, budget, seen)
            archives.append(archive)
            records.extend(entries)
        external = []
        for index, (original, local) in enumerate(args.external):
            item = {"original_path": original, "identity": identity(local), "compact_path": None,
                    "availability": "verified_local_file_not_implied_git"}
            if compact_file(str(local), item["identity"]["bytes"]):
                name = relative_name(Path(local).name)
                item["compact_path"] = f"external/{index:04d}/{name}"
                budget[0] += item["identity"]["bytes"]
                if budget[0] > budget[1]:
                    raise ValueError("external compact files exceed compact byte bound")
                write_new(staging / item["compact_path"], Path(local).read_bytes())
                actual = identity(staging / item["compact_path"])
                if any(actual[key] != item["identity"][key] for key in ("sha256", "bytes")):
                    raise ValueError("external compact file changed while copying")
                item["availability"] = "copied_compact_file_pending_git_commit"
            external.append(item)
        validation = inspect_evidence(staging, records, external, sorted(set(args.require_scope)))
        for item in external:
            verify_identity(item["identity"])
        manifest = {"schema": SCHEMA, "created_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    "retainer": identity(__file__), "bundles": archives, "external": external,
                    "files": records, "compact_bytes": budget[0], "validation": validation,
                    "availability": "compact files pending Git commit; raw artifacts require retained local bundles/external files",
                    "git_claim": "No files are asserted committed by this program."}
        write_new(staging / "retention.json", encode(manifest))
        write_new(staging / "validation.json", encode(validation))
        local_files = []
        for path in sorted(staging.rglob("*")):
            if path.is_file():
                actual = identity(path)
                local_files.append({"path": path.relative_to(staging).as_posix(), "sha256": actual["sha256"], "bytes": actual["bytes"]})
        write_new(staging / "compact-index.json", encode({"schema": SCHEMA, "files": local_files}))
        staging.rename(out)
        print(json.dumps({"directory": str(out), "ready": validation["ready"], "compact_files": len(local_files),
                          "bundles": len(archives), "problems": validation["problems"]}, sort_keys=True))
        return 0 if validation["ready"] else 1
    finally:
        # Only this invocation's random, checked workspace staging directory.
        if staging.exists():
            if staging.resolve().parent != out.resolve().parent or not staging.name.startswith("." + out.name + ".retaining-"):
                raise ValueError("refusing unsafe staging cleanup")
            shutil.rmtree(staging)


def verify(args):
    root = args.directory.resolve(strict=True)
    index = parse_json((root / "compact-index.json").read_bytes(), root)
    if index.get("schema") != SCHEMA:
        raise ValueError("unsupported compact index")
    expected = {relative_name(row["path"]) for row in index["files"]}
    if len(expected) != len(index["files"]):
        raise ValueError("duplicate compact index path")
    actual_files = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}
    if actual_files != expected | {"compact-index.json"}:
        raise ValueError("compact file set changed")
    for row in index["files"]:
        current = identity(root / row["path"])
        if any(current[key] != row[key] for key in ("sha256", "bytes")):
            raise ValueError(f"compact file changed: {row['path']}")
    manifest = parse_json((root / "retention.json").read_bytes(), root)
    for bundle in manifest["bundles"]:
        for name in ("archive", "manifest", "checksum"):
            verify_identity(bundle[name])
        source = parse_json(Path(bundle["manifest"]["path"]).read_bytes(), bundle["manifest"]["path"])
        if (root / f"bundles/{bundle['label']}/collector-manifest.json").read_bytes() != Path(bundle["manifest"]["path"]).read_bytes():
            raise ValueError("retained collector manifest differs from verified original")
        retained = [r for r in manifest["files"] if r["bundle"] == bundle["label"]]
        if len(retained) != len(source["files"]):
            raise ValueError("retained inventory count differs from verified collector")
        for entry, original in zip(retained, source["files"]):
            if any(entry.get(key) != value for key, value in original.items()):
                raise ValueError("retained inventory identity differs from verified collector")
            if entry["compact_path"]:
                current = identity(root / relative_name(entry["compact_path"]))
                if any(current[key] != entry[key] for key in ("sha256", "bytes")):
                    raise ValueError("compact bytes differ from collector inventory")
    for external in manifest["external"]:
        if external["compact_path"]:
            actual = identity(root / relative_name(external["compact_path"]))
            if any(actual[key] != external["identity"][key] for key in ("sha256", "bytes")):
                raise ValueError("external compact copy identity mismatch")
        else:
            verify_identity(external["identity"])
    validation = inspect_evidence(root, manifest["files"], manifest["external"], manifest["validation"]["required_scopes"])
    if validation != manifest["validation"]:
        raise ValueError("retained evidence validation no longer matches its inputs")
    if args.require_git_tracked:
        relative = root.relative_to(REPO).as_posix()
        result = subprocess.run(["git", "ls-files", "-z", "--", relative], cwd=REPO, check=True, capture_output=True)
        tracked = set(result.stdout.decode().split("\0"))
        if any((root / name).relative_to(REPO).as_posix() not in tracked for name in actual_files):
            raise ValueError("compact evidence is not fully present in the Git index")
    print(json.dumps({"directory": str(root), "verified_compact_files": len(actual_files),
                      "local_bundles_verified": len(manifest["bundles"]),
                      "ready": manifest["validation"]["ready"], "git_index_checked": args.require_git_tracked}, sort_keys=True))
    return 0 if manifest["validation"]["ready"] else 1


def self_test():
    # Tiny synthetic collector bundles; no solver, cloud or retained user files.
    for value in ("../x", "/x", "a/../x", "a\\x", "CON.txt", "x:y", "a//x"):
        try:
            relative_name(value)
        except ValueError:
            pass
        else:
            raise AssertionError(f"unsafe path accepted: {value}")
    for raw in (b'{"x":1,"x":2}', b'{"x":NaN}'):
        try:
            parse_json(raw, "self-test")
        except ValueError:
            pass
        else:
            raise AssertionError("malformed structured evidence accepted")
    assert critical_reference("/opt/r1/run/solution.sol")
    assert critical_reference("/opt/r1/target/current/release/examples/hu_saved_profile_audit")
    assert not critical_reference("/usr/bin/python3.12")
    assert len(EXPECTED) == 18
    work = HERE / (".retention-self-test-" + uuid.uuid4().hex)
    work.mkdir()
    try:
        payloads = {"solution.sol": b"raw policy", "checkpoint.ckpt": b"raw checkpoint",
                    "run.toml": b"fixture = true\n", "source.py": b"print('fixture')\n",
                    "stdout.log": b"observed\n", "stderr.log": b""}
        refs = [{"path": "/vm/fixture/" + name, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
                for name, data in payloads.items()]
        payloads["record.json"] = encode({"inputs": refs, "boot_id": "new-boot", "recovery": {"previous_boot_id": "old-boot"}})
        def make_bundle(name, corrupt=False):
            path = work / (name + ".tar.gz")
            entries = [{"kind": "regular", "root": "fixture", "relative_path": filename,
                        "original_path": "/vm/fixture/" + filename, "bytes": len(data),
                        "sha256": hashlib.sha256(data).hexdigest(), "included": True,
                        "archive_member": f"files/{index:08d}"}
                       for index, (filename, data) in enumerate(payloads.items())]
            manifest = encode({"schema": COLLECTOR_SCHEMA, "archive_filename": path.name, "files": entries})
            with tarfile.open(path, "w:gz") as archive:
                for filename, data in [("retention-manifest.json", manifest),
                                       *[(entry["archive_member"], payloads[entry["relative_path"]]) for entry in entries]]:
                    if corrupt and filename == "files/00000000":
                        data = b"bad policy"
                    info = tarfile.TarInfo(filename)
                    info.size = len(data)
                    archive.addfile(info, io.BytesIO(data))
            digest = identity(path)["sha256"]
            Path(str(path) + ".manifest.json").write_bytes(manifest)
            Path(str(path) + ".sha256").write_text(digest + "  " + path.name + "\n", encoding="utf-8")
            return path, digest
        path, digest = make_bundle("valid")
        out = work / "retained"
        arguments = argparse.Namespace(bundle=[("test", str(path))], sha256=[("test", digest)],
                                       external=[], require_scope=[], max_compact_bytes=1024**2, out=out)
        assert retain(arguments) == 0
        assert verify(argparse.Namespace(directory=out, require_git_tracked=False)) == 0
        retained = parse_json((out / "retention.json").read_bytes(), out)
        assert retained["validation"]["distinct_boot_ids"] == ["new-boot", "old-boot"]
        assert (out / "records/test/fixture/solution.sol").read_bytes() == payloads["solution.sol"]
        assert not compact_file("large-export.log", 1024**2 + 1)
        assert not compact_file("large.sol", 8 * 1024**2 + 1)
        assert (out / "records/test/fixture/stdout.log").read_bytes() == payloads["stdout.log"]
        arguments.out = work / "missing-scope"
        arguments.require_scope = ["performance"]
        assert retain(arguments) == 1
        path, digest = make_bundle("corrupt", True)
        arguments.bundle, arguments.sha256, arguments.out = [("bad", str(path))], [("bad", digest)], work / "bad-output"
        try:
            retain(arguments)
        except ValueError:
            pass
        else:
            raise AssertionError("corrupt member was accepted")
        assert not arguments.out.exists()
        payloads["damaged.json"] = b'\0\0\0\0{broken'
        path, digest = make_bundle("historical-corruption")
        arguments.bundle, arguments.sha256 = [("history", str(path))], [("history", digest)]
        arguments.out, arguments.require_scope = work / "damaged-records", []
        assert retain(arguments) == 1
        assert verify(argparse.Namespace(directory=arguments.out, require_git_tracked=False)) == 1
        assert (arguments.out / "records/history/fixture/damaged.json").read_bytes() == payloads["damaged.json"]
        (out / "records/test/fixture/stdout.log").write_bytes(b"changed\n")
        try:
            verify(argparse.Namespace(directory=out, require_git_tracked=False))
        except ValueError:
            pass
        else:
            raise AssertionError("changed compact evidence was accepted")
    finally:
        if work.resolve().parent != HERE or not work.name.startswith(".retention-self-test-"):
            raise ValueError("refusing unsafe self-test cleanup")
        shutil.rmtree(work)
    print("Self-test passed: bundle/member hashes, compact/raw retention, mixed boots, missing scope, corruption and changed compact evidence.")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    command = sub.add_parser("retain")
    command.add_argument("--bundle", action="append", type=named, required=True)
    command.add_argument("--sha256", action="append", type=named, required=True)
    command.add_argument("--external", action="append", type=named, default=[])
    command.add_argument("--out", required=True, type=Path)
    command.add_argument("--require-scope", choices=("performance", "saved-profile", "phases"), action="append", default=[])
    command.add_argument("--max-compact-bytes", type=int, default=512 * 1024**2)
    command = sub.add_parser("verify")
    command.add_argument("--directory", type=Path, required=True)
    command.add_argument("--require-git-tracked", action="store_true")
    sub.add_parser("self-test")
    args = parser.parse_args(argv)
    try:
        return {"retain": retain, "verify": verify, "self-test": lambda _: self_test()}[args.command](args)
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        print(f"evidence retention failed: {type(error).__name__}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
