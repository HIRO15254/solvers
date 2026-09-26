#!/usr/bin/env python3
"""Bundle a quiesced final-pipeline proof without executing its retained code.

Includes the entire CAS and available top-level state, even for incomplete or
corrupt runs. Other raw bytes are deduplicated, never dropped: recovery-manifest
maps every original file to an archive member. --extra LABEL=PATH selects external
verification, deployment and control evidence explicitly. Stop writers first.
Extraction into an empty directory leaves plan/build/result/retention/payload in
the layout expected by the separate trusted final-pipeline/verify.py.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import stat
import tarfile
import tempfile

SCHEMA = "r1.final-pipeline-recovery/v1"
REQUIRED = ("plan.json", "build.json", "result.json", "retention.json")


def encode(value):
    return (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def decode(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result
    def number(text):
        value = float(text)
        require(math.isfinite(value), "nonfinite JSON number")
        return value
    def constant(text):
        raise ValueError("nonfinite JSON constant: " + text)
    return json.loads(raw, object_pairs_hook=pairs, parse_float=number, parse_constant=constant)


def signature(info):
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns,
            info.st_ctime_ns if os.name != "nt" else None)


def no_link(path):
    info = path.lstat()
    require(not stat.S_ISLNK(info.st_mode) and not (getattr(info, "st_file_attributes", 0) & 0x400),
            "symlink/reparse point forbidden: " + str(path))
    return info


def explicit_path(raw):
    path = Path(os.path.abspath(raw))
    for ancestor in (*reversed(path.parents), path):
        no_link(ancestor)
    return path


def regular_open(path):
    before = no_link(path)
    require(stat.S_ISREG(before.st_mode), "not a regular file: " + str(path))
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0))
    stream = os.fdopen(fd, "rb")
    if signature(os.fstat(stream.fileno())) != signature(before):
        stream.close()
        raise ValueError("changed while opening: " + str(path))
    return stream, before


def fingerprint(path):
    stream, before = regular_open(path)
    digest = hashlib.sha256()
    with stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
        require(signature(os.fstat(stream.fileno())) == signature(before), "changed during read: " + str(path))
    require(signature(no_link(path)) == signature(before), "replaced during read: " + str(path))
    return {"bytes": before.st_size, "sha256": digest.hexdigest()}, signature(before)


def walk(root):
    info = no_link(root)
    if stat.S_ISREG(info.st_mode):
        return [(root.name, root)]
    require(stat.S_ISDIR(info.st_mode), "root is not regular/directory: " + str(root))
    answer = []
    def fail(error):
        raise error
    for current, directories, files in os.walk(root, followlinks=False, onerror=fail):
        for name in sorted(directories):
            require(stat.S_ISDIR(no_link(Path(current) / name).st_mode), "non-directory in traversal")
        directories.sort()
        for name in sorted(files):
            path = Path(current) / name
            require(stat.S_ISREG(no_link(path).st_mode), "non-regular evidence: " + str(path))
            answer.append((path.relative_to(root).as_posix(), path))
    return sorted(answer)


def safe_member(name):
    path = PurePosixPath(name)
    require(name and not path.is_absolute() and ".." not in path.parts and "\\" not in name
            and ":" not in name and not any(ord(c) < 32 for c in name), "unsafe archive member: " + name)
    return name


def named(value):
    label, separator, raw = value.partition("=")
    if not separator or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", label) or not raw:
        raise argparse.ArgumentTypeError("expected LABEL=PATH with a simple unique label")
    return label, raw


def plan_bundle(proof, extras):
    roots = {"proof": explicit_path(proof)}
    require(roots["proof"].is_dir(), "proof must be a directory")
    for label, raw in extras:
        require(label.lower() not in {x.lower() for x in roots} and label.lower() != "collector", "duplicate/reserved extra label")
        roots[label] = explicit_path(raw)
    for i, left in enumerate(roots.values()):
        for right in list(roots.values())[i + 1:]:
            require(not left.is_relative_to(right) and not right.is_relative_to(left), "overlapping selected roots")
    rows, inventories = [], {}
    for label, root in roots.items():
        inventories[label] = walk(root)
        for relative, path in inventories[label]:
            pin, sig = fingerprint(path)
            rows.append({"root": label, "relative_path": relative, "original_path": str(path),
                         **pin, "_signature": sig, "_path": path})
    # The collector itself is evidence, not executable input to verification.
    collector = Path(__file__).resolve()
    pin, sig = fingerprint(collector)
    rows.append({"root": "collector", "relative_path": collector.name, "original_path": str(collector),
                 **pin, "_signature": sig, "_path": collector})
    top_names = {row["relative_path"].split("/")[0] for row in rows if row["root"] == "proof"}
    namespace, number = "recovery", 0
    while namespace in top_names:
        number += 1
        namespace = "recovery-" + str(number)
    members, by_hash = {}, {}
    def include(row, name):
        name = safe_member(name)
        require(not any(name == old or name.startswith(old + "/") or old.startswith(name + "/") for old in members),
                "archive member file/directory collision: " + name)
        row["archive_member"] = name
        members[name] = row
        by_hash.setdefault((row["sha256"], row["bytes"]), name)
    # Keep the verifier's required layout and all original payload files.
    for row in rows:
        rel = row["relative_path"]
        if row["root"] == "proof" and ("/" not in rel or rel.startswith("payload/")):
            require(rel != "recovery-manifest.json", "reserved top-level manifest name")
            include(row, rel)
    for row in rows:
        if "archive_member" in row:
            continue
        key = (row["sha256"], row["bytes"])
        if key in by_hash:
            row["archive_member"] = by_hash[key]
        elif row["root"] == "proof":
            include(row, namespace + "/" + row["sha256"])
        else:
            include(row, namespace + "/external/" + row["root"] + "/" + row["relative_path"])
    proof_rows = {row["relative_path"]: row for row in rows if row["root"] == "proof"}
    issues, states = [], {}
    def original_json(name):
        raw = (roots["proof"] / name).read_bytes()
        row = proof_rows[name]
        require(len(raw) == row["bytes"] and hashlib.sha256(raw).hexdigest() == row["sha256"], "metadata changed: " + name)
        return decode(raw)
    for name in ("build.json", "result.json", "prepare-failure.json", "verification.json"):
        if name in proof_rows:
            try:
                states[name] = original_json(name).get("status")
            except (ValueError, AttributeError) as error:
                issues.append({"file": name, "error": repr(error)})
    if "retention.json" in proof_rows:
        try:
            retained = original_json("retention.json")
            if retained.get("schema") != "r1.showdown-kernel-retention/v1":
                issues.append({"file": "retention.json", "error": "unexpected retention schema"})
            pins = list(retained["files"].values()) + retained.get("identity_versions", [])
            for pin in pins:
                blob = proof_rows.get("payload/" + pin["sha256"])
                if blob is None or any(blob[key] != pin[key] for key in ("sha256", "bytes")):
                    issues.append({"file": pin.get("path"), "error": "retention pin payload missing or mismatched", "pin": pin})
        except (ValueError, TypeError, KeyError, AttributeError) as error:
            issues.append({"file": "retention.json", "error": repr(error)})
    for name, row in proof_rows.items():
        if name.startswith("payload/") and name != "payload/" + row["sha256"]:
            issues.append({"file": name, "error": "CAS member name does not match actual bytes"})
    public = [{k: v for k, v in row.items() if not k.startswith("_")} for row in rows]
    manifest = {"schema": SCHEMA, "scope": "Original-byte recovery only; not campaign success or verified identity bindings",
                "writers_quiesced_by_caller": True, "roots": {k: str(v) for k, v in roots.items()},
                "recorded_states": states, "missing_required_files": [name for name in REQUIRED if name not in proof_rows],
                "retention_issues": issues, "files": public, "original_files": len(rows), "archive_files": len(members),
                "original_bytes": sum(row["bytes"] for row in rows), "unique_archive_bytes": sum(row["bytes"] for row in members.values()),
                "external_labels": list(extras_label for extras_label in roots if extras_label != "proof"),
                "recovery_namespace": namespace,
                "publication": "The .sha256 sidecar is published last. Its presence marks complete publication; check all three files before use. Incomplete outputs are never overwritten.",
                "restore": "Extract regular archive members to an empty directory; trusted verify.py uses the root. Recovery aliases map other originals to included member bytes."}
    return manifest, members, rows, roots, inventories


def write_archive(path, manifest, members, max_bytes):
    class Capped:
        def __init__(self, stream):
            self.stream, self.count = stream, 0
        def write(self, data):
            require(self.count + len(data) <= max_bytes, "archive exceeds --max-bytes; no partial selection is allowed")
            count = self.stream.write(data)
            self.count += count
            return count
        def flush(self):
            self.stream.flush()
    def info(name, size):
        item = tarfile.TarInfo(safe_member(name))
        item.size, item.mode, item.mtime = size, 0o600, 0
        return item
    with path.open("wb") as raw:
        with gzip.GzipFile(filename="", fileobj=Capped(raw), mode="wb", compresslevel=6, mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w|", format=tarfile.PAX_FORMAT) as archive:
                data = encode(manifest)
                archive.addfile(info("recovery-manifest.json", len(data)), io.BytesIO(data))
                for name, row in sorted(members.items()):
                    stream, before = regular_open(row["_path"])
                    with stream:
                        require(signature(before) == row["_signature"], "file changed before packing")
                        digest = hashlib.sha256()
                        class Reader:
                            def read(self, size=-1):
                                value = stream.read(size)
                                digest.update(value)
                                return value
                        archive.addfile(info(name, row["bytes"]), Reader())
                        require(digest.hexdigest() == row["sha256"] and not stream.read(1), "file changed during packing")
                        require(signature(os.fstat(stream.fileno())) == row["_signature"], "file changed during packing")
        raw.flush()
        os.fsync(raw.fileno())


def collect(args):
    require(args.quiesced, "stop all writers first and pass --quiesced")
    require(0 < args.max_bytes <= 8 * 1024**3, "--max-bytes must be positive and at most 8 GiB")
    manifest, members, rows, roots, inventories = plan_bundle(args.proof, args.extra)
    output = Path(os.path.abspath(args.out))
    require(not any(c in output.name for c in "\r\n"), "invalid output name")
    outputs = [output, Path(str(output) + ".manifest.json"), Path(str(output) + ".sha256")]
    for path in outputs:
        require(not path.exists() and not path.is_symlink(), "refusing to replace output: " + str(path))
        require(not any(path.resolve().is_relative_to(root) for root in roots.values()), "output inside input")
    output.parent.mkdir(parents=True, exist_ok=True)
    explicit_path(output.parent)
    fd, temporary = tempfile.mkstemp(prefix=".final-proof-", dir=output.parent)
    os.close(fd)
    temporary, published = Path(temporary), []
    try:
        write_archive(temporary, manifest, members, args.max_bytes)
        for row in rows:
            require(signature(no_link(row["_path"])) == row["_signature"], "evidence changed before publication")
        for label, root in roots.items():
            require(walk(root) == inventories[label], "evidence file set changed during packing")
        archive_pin, _ = fingerprint(temporary)
        os.link(temporary, output)
        published.append(output)
        for path, data in ((outputs[1], encode(manifest)), (outputs[2], f"{archive_pin['sha256']}  {output.name}\n".encode())):
            side_fd, side_name = tempfile.mkstemp(prefix=".final-proof-sidecar-", dir=output.parent)
            try:
                with os.fdopen(side_fd, "wb") as stream:
                    stream.write(data)
                    stream.flush()
                    os.fsync(stream.fileno())
                os.link(side_name, path)
                published.append(path)
            finally:
                Path(side_name).unlink(missing_ok=True)
        return {"archive": str(output), **archive_pin, "manifest": str(outputs[1]),
                "manifest_sha256": hashlib.sha256(encode(manifest)).hexdigest(),
                "original_files": len(rows), "archive_files": len(members),
                "retention_issue_count": len(manifest["retention_issues"]), "missing_required_files": manifest["missing_required_files"]}
    except BaseException:
        for path in reversed(published):
            path.unlink()
        raise
    finally:
        temporary.unlink(missing_ok=True)


def check_bundle(path):
    """Read all archived bytes, aliases and sidecars; never extract or execute."""
    path = explicit_path(path)
    pin, _ = fingerprint(path)
    checksum = explicit_path(str(path) + ".sha256").read_bytes()
    require(checksum == f"{pin['sha256']}  {path.name}\n".encode(), "archive checksum sidecar differs")
    manifest_bytes = explicit_path(str(path) + ".manifest.json").read_bytes()
    manifest = decode(manifest_bytes)
    require(manifest["schema"] == SCHEMA, "recovery manifest schema")
    expected = {}
    for row in manifest["files"]:
        member = safe_member(row["archive_member"])
        value = {k: row[k] for k in ("bytes", "sha256")}
        require(member not in expected or expected[member] == value, "alias content differs")
        require(not any(member.startswith(old + "/") or old.startswith(member + "/") for old in expected),
                "archive member file/directory collision")
        expected[member] = value
    actual, seen_manifest = {}, False
    with tarfile.open(path, "r:gz") as archive:
        for member in archive:
            name = safe_member(member.name)
            require(member.isfile(), "non-regular archive member")
            with archive.extractfile(member) as stream:
                if name == "recovery-manifest.json":
                    require(not seen_manifest and member.size == len(manifest_bytes)
                            and stream.read() == manifest_bytes, "embedded recovery manifest differs")
                    seen_manifest = True
                    continue
                require(name not in actual and name in expected, "duplicate/unexpected archive member")
                digest = hashlib.sha256()
                size = 0
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    size += len(block)
                    digest.update(block)
                actual[name] = {"bytes": size, "sha256": digest.hexdigest()}
    require(seen_manifest and actual == expected and len(actual) == manifest["archive_files"], "archive file set/content mismatch")
    require(len(manifest["files"]) == manifest["original_files"], "original file count mismatch")
    return {"schema": SCHEMA, "status": "bytes_verified", "archive": str(path), **pin,
            "original_files": manifest["original_files"], "archive_files": len(actual),
            "retention_issue_count": len(manifest["retention_issues"]), "missing_required_files": manifest["missing_required_files"],
            "scope": "Recovery-byte integrity only; campaign verification is separate"}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proof", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--check", type=Path, help="verify archive and both sidecars without extraction")
    parser.add_argument("--extra", action="append", type=named, default=[])
    parser.add_argument("--quiesced", action="store_true")
    parser.add_argument("--max-bytes", type=int, default=2 * 1024**3)
    args = parser.parse_args(argv)
    try:
        if args.check:
            require(not args.proof and not args.out and not args.extra and not args.quiesced, "--check is read-only and exclusive")
            result = check_bundle(args.check)
        else:
            require(args.proof and args.out, "--proof and --out are required")
            result = collect(args)
        print(json.dumps(result, sort_keys=True))
    except (OSError, ValueError, tarfile.TarError) as error:
        parser.exit(2, f"recovery bundle failed: {type(error).__name__}: {error}\n")


if __name__ == "__main__":
    main()
