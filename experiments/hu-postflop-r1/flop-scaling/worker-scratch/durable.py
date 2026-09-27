"""Small Linux durability boundary for research evidence; no workload execution.

Callers must own/quiesce files until publication and never mutate checkpoint
dependencies afterward. A raised publication error may leave a visible final
name: stop and preserve it; do not retry, overwrite, or infer a committed run.
"""
from __future__ import annotations

import gzip
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile


def _need(value, message):
    if not value:
        raise ValueError(message)


def _platform():
    if os.name != "posix" or not hasattr(os, "O_DIRECTORY"):
        raise NotImplementedError("POSIX directory fsync is required; no durability fallback")


def _path(path):
    path = Path(path).absolute()
    _need(not any(p.is_symlink() for p in (path, *path.parents)), "symlink evidence path")
    return path


def _stamp(path):
    value = path.lstat()
    _need(stat.S_ISREG(value.st_mode), "regular evidence file required")
    return (value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns, value.st_ctime_ns)


def file_ref(path):
    path = _path(path)
    before = _stamp(path)
    digest, length = hashlib.sha256(), 0
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
            length += len(block)
    _need(_stamp(path) == before and length == before[2], "file changed while hashing")
    return {"path": str(path), "bytes": length, "sha256": digest.hexdigest()}


def _pair(ref):
    return {key: ref[key] for key in ("bytes", "sha256")}


def sync_directory(path):
    """Sync a directory entry set, or raise if the platform/filesystem cannot."""
    _platform()
    path = _path(path)
    _need(path.is_dir(), "existing directory required")
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def sync_file(path):
    """Sync an existing, closed regular file; parent directories are separate."""
    _platform()
    path = _path(path)
    before = _stamp(path)
    with path.open("rb") as stream:
        os.fsync(stream.fileno())
    _need(_stamp(path) == before, "file changed while syncing")


def _parents(paths):
    # Include ancestors so a freshly created proof/stage directory also persists.
    directories = {parent for path in paths for parent in Path(path).parents}
    for directory in sorted(directories, key=lambda p: (-len(p.parts), str(p))):
        sync_directory(directory)


def sync_files(refs):
    """Seal expected immutable references, including all containing directories."""
    _platform()
    refs = list(refs)
    paths = [_path(ref["path"]) for ref in refs]
    _need(len(set(paths)) == len(paths), "duplicate file reference")
    for path, expected in zip(paths, refs):
        sync_file(path)
        _need(file_ref(path) == expected, "file reference changed")
    _parents(paths)


def _publish(temporary, path, once):
    if once:
        os.link(temporary, path)  # Exclusive publication, including racing writers.
        temporary.unlink()
    else:
        if path.exists():
            _stamp(path)
        os.replace(temporary, path)
    _parents([path])


def atomic_json(path, value, *, once=False):
    """Publish fsynced JSON, then fsync its directory chain; never mkdir/retry."""
    _platform()
    path = _path(path)
    encoded = (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode("utf-8")
    descriptor, name = tempfile.mkstemp(prefix="." + path.name + ".", suffix=".tmp", dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        _publish(temporary, path, once)
        return file_ref(path)
    finally:
        temporary.unlink(missing_ok=True)


def gzip_verified(source, destination, expected):
    """Durably publish a new gzip only after exact source-byte/hash verification."""
    _platform()
    source, destination = _path(source), _path(destination)
    _need(source != destination and not destination.exists(), "new distinct gzip required")
    before = _stamp(source)
    sync_file(source)
    descriptor, name = tempfile.mkstemp(prefix="." + destination.name + ".", suffix=".tmp", dir=destination.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as raw:
            with source.open("rb") as original, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0, compresslevel=1) as compressed:
                while block := original.read(1024 * 1024):
                    compressed.write(block)
            raw.flush()  # The gzip footer is now written.
            os.fsync(raw.fileno())
        digest, length = hashlib.sha256(), 0
        with gzip.open(temporary, "rb") as compressed, source.open("rb") as original:
            while block := compressed.read(1024 * 1024):
                _need(block == original.read(len(block)), "gzip differs from full source bytes")
                digest.update(block)
                length += len(block)
            _need(original.read(1) == b"", "gzip is truncated")
        _need({"bytes": length, "sha256": digest.hexdigest()} == _pair(expected), "source content pin differs")
        _need(_stamp(source) == before, "source changed during compression")
        _publish(temporary, destination, True)
        return file_ref(destination)
    finally:
        temporary.unlink(missing_ok=True)


def retain_canonical(raw_ref, destination, receipt_path, *, allow_raw_delete=False):
    """Optional small transaction: durable canonical + receipt before raw deletion.

The caller explicitly authorizes only raw_ref.path. This is not an automatic
resume operation. A failure preserves all surviving files for inspection.
"""
    _platform()
    raw, destination, receipt_path = map(_path, (raw_ref["path"], destination, receipt_path))
    _need(len({raw, destination, receipt_path}) == 3 and not receipt_path.exists(), "new distinct retention paths required")
    _need(type(allow_raw_delete) is bool and file_ref(raw) == raw_ref, "raw identity/authorization differs")
    sync_files([raw_ref])
    canonical = gzip_verified(raw, destination, raw_ref)
    receipt = {"schema": "r1.research-canonical-retention/v1", "source": raw_ref,
               "canonical": canonical, "fullbyte_verified": True,
               "raw_deletion_authorized": allow_raw_delete, "raw_removed": False,
               "status": "canonical_durable" if allow_raw_delete else "completed"}
    atomic_json(receipt_path, receipt, once=True)
    if allow_raw_delete:
        _need(file_ref(raw) == raw_ref, "raw changed before authorized deletion")
        raw.unlink()
        _parents([raw])
        receipt.update(raw_removed=True, status="completed")
        atomic_json(receipt_path, receipt)
    return receipt


def current_boot_id():
    _platform()
    return Path("/proc/sys/kernel/random/boot_id").read_text(encoding="ascii").strip()


def _stage_boot(stage, boot_id):
    _need(stage["status"] == "completed"
          and stage["host_before"]["boot_id"] == stage["host_after"]["boot_id"] == boot_id,
          "stage incomplete or from a different boot")


def publish_case(path, *, case, boot_id, plan_ref, build_ref, files, stages):
    """Publish one immutable case checkpoint; schedule/quality checks stay caller-owned.

No dependency on mutable execution.json/retained.json or wrapper progress.
Recovery verifies this manifest and its exact references, never republishes it.
"""
    _platform()
    path = _path(path)
    _need(not path.exists() and isinstance(case, str) and case and isinstance(boot_id, str) and boot_id,
          "new case checkpoint and explicit identity required")
    _need(current_boot_id() == boot_id, "case cannot be published across boots")
    refs = [plan_ref, build_ref, *files]
    _need(all(_path(ref["path"]) != path and Path(ref["path"]).name not in {"execution.json", "retained.json"}
              for ref in refs), "mutable phase/global manifest dependency")
    sync_files(refs)
    plan = json.loads(Path(plan_ref["path"]).read_text(encoding="utf-8"))
    build = json.loads(Path(build_ref["path"]).read_text(encoding="utf-8"))
    _need(plan["host"]["boot_id"] == boot_id and build["status"] == "completed"
          and build["plan"] == _pair(plan_ref) and build["stages"], "original plan/build identity differs")
    stages = list(stages)
    _need(stages and len({stage["name"] for stage in stages}) == len(stages), "nonempty unique case stages required")
    for stage in [*build["stages"], *stages]:
        _stage_boot(stage, boot_id)
    _need(all(stage["case"] == case for stage in stages), "mixed case stages")
    # The caller has validated the full stage/output graph; require the records
    # themselves here so snapshots cannot be detached from their supervisor data.
    references = {ref["path"]: ref for ref in refs}
    for stage in [*build["stages"], *stages]:
        _need(references.get(stage["record"]["path"]) == stage["record"], "stage record not in immutable references")
    _need(current_boot_id() == boot_id, "boot changed before case publication")
    manifest = {"schema": "r1.research-case-checkpoint/v1", "case": case, "boot_id": boot_id,
                "status": "case_complete", "full_matrix_complete": False,
                "plan": plan_ref, "build": build_ref, "files": refs, "stages": stages}
    return atomic_json(path, manifest, once=True)
