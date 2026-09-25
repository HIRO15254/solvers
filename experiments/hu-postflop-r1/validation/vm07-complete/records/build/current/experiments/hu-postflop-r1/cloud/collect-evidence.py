#!/usr/bin/env python3
"""Collect explicitly named VM evidence roots into a bounded tar.gz bundle.

No remote operations, deletion, shell commands, caches, or recursive symlink
following. Stop all writers before collection. Python 3.11+ standard library.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import tarfile
import tempfile
import time

MAX_BYTES = 2 * 1024**3
EXCLUDED_DIRECTORIES = {"target", ".cache", ".git", "cargo", "rustup"}
COMPACT_SUFFIXES = {".json", ".jsonl", ".log", ".toml", ".txt", ".md", ".csv", ".sha256"}
SCHEMA = "solvers.r1-retention/v1"


def encode(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True, allow_nan=False) + "\n").encode()


def signature(info):
    # CPython 3.13 Windows stat/fstat disagree about ctime (creation vs change
    # time). Content hashes still verify the bytes both before and during packing.
    changed = info.st_ctime_ns if os.name != "nt" else None
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, changed)


def regular_open(path):
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError(f"not a regular file (symlinks are not followed): {path}")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    stream = os.fdopen(descriptor, "rb")
    if signature(os.fstat(stream.fileno())) != signature(before):
        stream.close()
        raise ValueError(f"file changed while opening: {path}")
    return stream, before


def fingerprint(path):
    digest = hashlib.sha256()
    stream, before = regular_open(path)
    with stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
        after = os.fstat(stream.fileno())
    if signature(before) != signature(after) or signature(path.lstat()) != signature(before):
        raise ValueError(f"file changed during hashing; stop its writer first: {path}")
    return digest.hexdigest(), before


def named_value(value):
    if "=" not in value:
        raise argparse.ArgumentTypeError("expected LABEL=VALUE")
    label, raw = value.split("=", 1)
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", label) or not raw:
        raise argparse.ArgumentTypeError("label must be 1-64 letters, digits, '_' or '-'; value is required")
    return label, raw


def fail_walk(error):
    raise error


def roots_from_args(args):
    roots = {}
    for label, raw in args.root:
        if label in roots:
            raise ValueError(f"duplicate root label: {label}")
        path = Path(os.path.abspath(raw))
        if path.is_symlink():
            raise ValueError(f"explicit root may not be a symlink: {path}")
        path = path.resolve(strict=True)
        if not path.is_file() and not path.is_dir():
            raise ValueError(f"root is not a regular file or directory: {path}")
        roots[label] = path
    git = dict(args.git_source)
    if len(git) != len(args.git_source) or not set(git).issubset(roots):
        raise ValueError("git-source labels must be unique and name an explicit root")
    return roots, git


def inventory(roots, git):
    records, excluded = [], []
    seen = set()

    def add(path, label, relative):
        # Overlapping selected roots are rejected rather than duplicating content
        # with ambiguous priority or availability assertions.
        key = os.path.normcase(str(path))
        if key in seen:
            raise ValueError(f"overlapping evidence roots contain the same path: {path}")
        seen.add(key)
        if path.is_symlink():
            target = os.readlink(path)
            data = os.fsencode(target)
            records.append({"kind": "symlink", "root": label, "relative_path": str(relative),
                            "original_path": str(path), "bytes": path.lstat().st_size,
                            "sha256": hashlib.sha256(data).hexdigest(),
                            "hash_scope": "link target text encoded by os.fsencode, not target contents",
                            "link_target": target, "included": False, "archive_member": None,
                            "skip_reason": "symlinks_are_not_followed",
                            "availability_after_vm_removal": "removed-with-VM"})
            return
        digest, info = fingerprint(path)
        records.append({"kind": "regular", "root": label, "relative_path": str(relative),
                        "original_path": str(path), "bytes": info.st_size, "sha256": digest,
                        "hash_scope": "file bytes", "mtime_ns": info.st_mtime_ns,
                        "mode": stat.S_IMODE(info.st_mode),
                        "included": False, "archive_member": None, "skip_reason": "capacity_limit",
                        "availability_after_vm_removal": "removed-with-VM",
                        "_signature": signature(info),
                        "source_git_ref": git.get(label)})

    for label, root in roots.items():
        if root.is_file():
            # Explicit binaries inside target/ are allowed. Target directory
            # traversal remains excluded below.
            add(root, label, root.name)
            continue
        for current, directory_names, file_names in os.walk(root, followlinks=False, onerror=fail_walk):
            current = Path(current)
            for name in sorted(directory_names):
                path = current / name
                if path.is_symlink():
                    add(path, label, path.relative_to(root))
                    directory_names.remove(name)
                elif name.lower() in EXCLUDED_DIRECTORIES:
                    excluded.append({"path": str(path), "reason": "generated_cache_or_build_directory",
                                     "contents_inventoried": False})
                    directory_names.remove(name)
            directory_names.sort()
            for name in sorted(file_names):
                path = current / name
                add(path, label, path.relative_to(root))
    records.sort(key=lambda item: (item["root"], item["relative_path"]))
    for index, record in enumerate(records):
        if record["kind"] == "regular":
            record["archive_member"] = f"files/{index:08d}"
    return records, excluded


def public_records(records):
    return [{key: value for key, value in record.items() if not key.startswith("_")}
            for record in records]


def tar_member_bytes(size):
    return 512 + ((size + 511) // 512) * 512


def conservative_gzip_bound(tar_bytes):
    # Tar stream is padded to 10,240-byte records. A generous 5% plus 64 KiB
    # overhead bounds deflate expansion; CappedWriter also enforces actual size.
    padded = ((tar_bytes + 1024 + 10239) // 10240) * 10240
    return padded + (padded + 19) // 20 + 65536


def select(manifest, records, max_bytes):
    # Reserve room for all inventory rows, including skipped files, before
    # choosing any payload. Selection fields cannot exceed this per-row slack.
    manifest["files"] = public_records(records)
    reserve = len(encode(manifest)) + len(records) * 256 + 65536
    raw_size = tar_member_bytes(reserve)
    if conservative_gzip_bound(raw_size) + reserve + 256 > max_bytes:
        raise ValueError("retention manifest alone exceeds the transfer limit")
    candidates = [record for record in records if record["kind"] == "regular"]
    candidates.sort(key=lambda record: (
        0 if (Path(record["original_path"]).suffix.lower() in COMPACT_SUFFIXES
              and record["bytes"] <= 8 * 1024**2) else 1,
        record["bytes"], record["root"], record["relative_path"]))
    for record in candidates:
        candidate_size = raw_size + tar_member_bytes(record["bytes"])
        if conservative_gzip_bound(candidate_size) + reserve + 256 <= max_bytes:
            raw_size = candidate_size
            record["included"] = True
            record["skip_reason"] = None
            record["availability_after_vm_removal"] = "in-evidence-bundle-pending-download-verification"
        elif record["source_git_ref"] is not None:
            record["availability_after_vm_removal"] = "source-contents-reproducible-from-git"
            record["original_file_bytes_recoverable"] = False
    manifest["files"] = public_records(records)
    return manifest


class CappedWriter:
    def __init__(self, stream, limit):
        self.stream, self.limit, self.bytes = stream, limit, 0

    def write(self, data):
        if self.bytes + len(data) > self.limit:
            raise ValueError("compressed archive exceeds transfer limit")
        written = self.stream.write(data)
        self.bytes += written
        return written

    def flush(self):
        self.stream.flush()


class HashedReader:
    def __init__(self, stream):
        self.stream, self.digest = stream, hashlib.sha256()

    def read(self, size=-1):
        data = self.stream.read(size)
        self.digest.update(data)
        return data


def tar_info(name, size, mode=0o600):
    result = tarfile.TarInfo(name)
    result.size, result.mode, result.mtime = size, mode & 0o777, 0
    result.uid = result.gid = 0
    return result


def write_bundle(temporary, manifest_bytes, records, limit):
    with temporary.open("wb") as output:
        capped = CappedWriter(output, limit)
        with gzip.GzipFile(filename="", fileobj=capped, mode="wb", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w|", format=tarfile.USTAR_FORMAT) as archive:
                archive.addfile(tar_info("retention-manifest.json", len(manifest_bytes)), io.BytesIO(manifest_bytes))
                for record in records:
                    if not record["included"]:
                        continue
                    path = Path(record["original_path"])
                    stream, before = regular_open(path)
                    with stream:
                        if signature(before) != record["_signature"]:
                            raise ValueError(f"file changed before packing: {path}")
                        reader = HashedReader(stream)
                        archive.addfile(tar_info(record["archive_member"], record["bytes"], record["mode"]), reader)
                        if reader.digest.hexdigest() != record["sha256"] or stream.read(1):
                            raise ValueError(f"file contents changed during packing: {path}")
                        if signature(os.fstat(stream.fileno())) != record["_signature"]:
                            raise ValueError(f"file changed during packing: {path}")
                    if signature(path.lstat()) != record["_signature"]:
                        raise ValueError(f"file replaced during packing: {path}")
        output.flush()
        os.fsync(output.fileno())


def collect(args):
    if not 0 < args.max_bytes <= MAX_BYTES:
        raise ValueError("max-bytes must be positive and no greater than 2 GiB")
    roots, git = roots_from_args(args)
    output = Path(os.path.abspath(args.out))
    if any(char in output.name for char in "\r\n"):
        raise ValueError("output filename may not contain newlines")
    outputs = [output, Path(str(output) + ".manifest.json"), Path(str(output) + ".sha256")]
    for path in outputs:
        if path.exists() or path.is_symlink():
            raise FileExistsError(f"refusing to replace existing output: {path}")
        resolved = path.resolve()
        if any(resolved == root or (root.is_dir() and resolved.is_relative_to(root))
               for root in roots.values()):
            raise ValueError("collector outputs must be outside every selected root")
    output.parent.mkdir(parents=True, exist_ok=True)
    records, excluded = inventory(roots, git)
    manifest = {"schema": SCHEMA, "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "archive_filename": output.name, "max_total_output_bytes": args.max_bytes,
                "roots": {label: str(path) for label, path in roots.items()},
                "selection": "records/config/logs <=8 MiB first; then all other files by ascending size",
                "excluded_directories": excluded,
                "availability_note": "included files require download and hash verification before VM removal; skipped originals are lost with VM unless an explicit Git-content reconstruction is recorded",
                "git_source_assertions": git,
                "collector": {"path": str(Path(__file__).resolve()), "sha256": fingerprint(Path(__file__).resolve())[0]}}
    select(manifest, records, args.max_bytes)
    manifest_bytes = encode(manifest)
    descriptor, temporary_name = tempfile.mkstemp(prefix=".r1-evidence-", dir=output.parent)
    os.close(descriptor)
    temporary = Path(temporary_name)
    published = []
    try:
        write_bundle(temporary, manifest_bytes, records, args.max_bytes - len(manifest_bytes) - 256)
        for record in records:
            path = Path(record["original_path"])
            if record["kind"] == "regular" and signature(path.lstat()) != record["_signature"]:
                raise ValueError(f"inventoried file changed before publication: {path}")
            if record["kind"] == "symlink" and os.readlink(path) != record["link_target"]:
                raise ValueError(f"inventoried symlink changed before publication: {path}")
        digest, info = fingerprint(temporary)
        # Publish without overwriting. If any sidecar publication fails, undo
        # only this call's newly created outputs, never an existing file.
        os.link(temporary, output)
        published.append(output)
        for path, content in ((outputs[1], manifest_bytes),
                              (outputs[2], f"{digest}  {output.name}\n".encode())):
            with path.open("xb") as stream:
                published.append(path)
                stream.write(content)
                stream.flush()
                os.fsync(stream.fileno())
        result = {"archive": str(output), "bytes": info.st_size, "sha256": digest,
                  "manifest": str(outputs[1]), "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
                  "total_output_bytes": sum(path.stat().st_size for path in outputs),
                  "included_files": sum(record["included"] for record in records),
                  "skipped_files": sum(not record["included"] for record in records)}
        print(json.dumps(result, sort_keys=True))
        return result
    except BaseException:
        for path in reversed(published):
            path.unlink()
        raise
    finally:
        temporary.unlink(missing_ok=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", action="append", type=named_value, required=True,
                        help="LABEL=PATH; existing explicit file or directory, repeatable")
    parser.add_argument("--git-source", action="append", type=named_value, default=[],
                        help="LABEL=GIT_REF; caller asserts source contents are reproducible from this Git ref")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--max-bytes", type=int, default=MAX_BYTES)
    args = parser.parse_args(argv)
    try:
        collect(args)
    except (OSError, ValueError, tarfile.TarError) as error:
        parser.exit(2, f"evidence collection failed: {type(error).__name__}: {error}\n")


if __name__ == "__main__":
    main()
