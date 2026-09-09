#!/usr/bin/env python3
"""Build a deterministic, allowlisted source archive for the GCP bootstrap.

The command is intentionally separate from the cloud launcher.  It only reads
the local workspace and writes a tar.gz plus a JSON manifest; it never talks to
gcloud and never uploads anything.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import subprocess
import sys
import tarfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Iterable


ROOT = Path(__file__).resolve().parents[1]
REQUIRED_ROOT_FILES = ("Cargo.toml", "Cargo.lock")
OPTIONAL_ROOT_FILES = ("rust-toolchain.toml",)
LICENSE_FILES = ("LICENSE-MIT", "LICENSE-APACHE", "LICENSE-POLICY.md")
REQUIRED_EXPLICIT_FILES = (
    "tools/gcp_convergence_bootstrap.sh",
    "tools/gcp_multiway_experiment.py",
    "tools/multiway_convergence_bench.py",
)
REQUIRED_BUILD_FILES = (
    "docs/cli-reference.jp.md",
    "docs/solver-config-v1.jp.md",
    "docs/multiway-preflop-v1.jp.md",
)
REQUIRED_EXPLICIT_PREFIXES = ("examples/bench_multiway/",)
EXPLICIT_FILE_SUFFIXES = frozenset({".md", ".py", ".sh", ".toml"})
CRATE_SUFFIXES = frozenset({".md", ".rs", ".toml"})
EXCLUDED_DIRECTORY_PARTS = frozenset(
    {".git", ".agents", ".codex", ".cache", "runs", "target", "credentials", "secrets"}
)


class PackageError(RuntimeError):
    """A source package cannot be made without violating its allowlist."""


@dataclass(frozen=True)
class Candidate:
    path: str
    source: str


def posix_path(path: Path) -> str:
    return path.as_posix()


def excluded_relative(relative: str) -> bool:
    return any(part.lower() in EXCLUDED_DIRECTORY_PARTS for part in PurePosixPath(relative).parts)


def forbidden_filename(name: str) -> bool:
    lowered = name.lower()
    return (
        lowered.startswith("credentials")
        or "credential" in lowered
        or "secret" in lowered
        or lowered in {".env", ".envrc"}
        or lowered.endswith((".pem", ".key", ".p12", ".pfx"))
    )


def ensure_relative_regular(root: Path, relative: str) -> Path:
    """Resolve one candidate without following symlinks outside the root."""

    relative_path = PurePosixPath(relative)
    if (
        relative_path.is_absolute()
        or ".." in relative_path.parts
        or "\\" in relative
        or (len(relative) >= 2 and relative[1] == ":")
    ):
        raise PackageError(f"candidate path is not relative: {relative}")
    if excluded_relative(relative) or forbidden_filename(relative_path.name):
        raise PackageError(f"candidate is excluded from source package: {relative}")
    candidate = root.joinpath(*relative_path.parts)
    try:
        candidate.relative_to(root)
    except ValueError as error:
        raise PackageError(f"candidate escapes workspace: {relative}") from error
    current = root
    for part in relative_path.parts[:-1]:
        current = current / part
        if current.is_symlink():
            raise PackageError(f"symlink ancestor is not allowed in source package: {relative}")
    if candidate.is_symlink():
        raise PackageError(f"symlink is not allowed in source package: {relative}")
    if not candidate.is_file():
        raise PackageError(f"candidate is not a regular file: {relative}")
    resolved_root = root.resolve()
    resolved_candidate = candidate.resolve()
    try:
        resolved_candidate.relative_to(resolved_root)
    except ValueError as error:
        raise PackageError(f"candidate resolves outside workspace: {relative}") from error
    return candidate


def git_tracked_paths(root: Path) -> list[str]:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise PackageError("git ls-files failed; refusing an unverifiable package") from error
    return [path for path in result.stdout.decode("utf-8").split("\0") if path]


def collect_candidates(root: Path, tracked_paths: Iterable[str]) -> list[Candidate]:
    tracked = set(tracked_paths)
    selected: dict[str, Candidate] = {}

    def add(relative: str, source: str) -> None:
        if relative in selected:
            return
        ensure_relative_regular(root, relative)
        selected[relative] = Candidate(relative, source)

    for relative in REQUIRED_ROOT_FILES:
        if relative not in tracked:
            raise PackageError(f"required root file is not git-tracked: {relative}")
        add(relative, "git-tracked")
    for relative in OPTIONAL_ROOT_FILES:
        if relative in tracked or (root / relative).exists():
            add(relative, "git-tracked" if relative in tracked else "explicit-root")
    for relative in LICENSE_FILES:
        if relative in tracked:
            add(relative, "git-tracked")
        elif (root / relative).exists():
            add(relative, "explicit-license")
    for relative in REQUIRED_BUILD_FILES:
        add(relative, "explicit-build-file")

    # Cargo builds need every Rust source and manifest in the workspace.  Walk
    # the explicit crates prefix as well as git's index: final solver changes
    # may still be untracked when the parent creates the package.  Restricting
    # suffixes keeps local credentials and generated binaries out.
    crates_root = root / "crates"
    if crates_root.is_symlink():
        raise PackageError("symlink is not allowed for crates directory")
    if not crates_root.is_dir():
        raise PackageError("required crates directory is missing")
    for path in sorted(crates_root.rglob("*")):
        relative = posix_path(path.relative_to(root))
        if excluded_relative(relative):
            continue
        if forbidden_filename(path.name):
            raise PackageError(f"credential/secret filename is not allowed: {relative}")
        if path.is_symlink():
            raise PackageError(f"symlink is not allowed in source package: {relative}")
        if path.is_file() and Path(relative).suffix.lower() in CRATE_SUFFIXES:
            add(relative, "git-tracked" if relative in tracked else "explicit-prefix")

    # These are deliberately explicit because the benchmark fixtures and GCP
    # helpers are currently untracked during experiment development.
    for relative in REQUIRED_EXPLICIT_FILES:
        add(relative, "explicit-file")
    for prefix in REQUIRED_EXPLICIT_PREFIXES:
        prefix_path = root / Path(prefix)
        if prefix_path.is_symlink():
            raise PackageError(f"symlink is not allowed for explicit directory: {prefix}")
        if not prefix_path.is_dir():
            raise PackageError(f"required explicit directory is missing: {prefix}")
        for path in sorted(prefix_path.rglob("*")):
            relative = posix_path(path.relative_to(root))
            if excluded_relative(relative):
                continue
            if forbidden_filename(path.name):
                raise PackageError(f"credential/secret filename is not allowed: {relative}")
            if path.is_symlink():
                raise PackageError(f"symlink is not allowed in source package: {relative}")
            if path.is_file() and path.suffix.lower() in EXPLICIT_FILE_SUFFIXES:
                add(relative, "explicit-prefix")
            elif path.is_file():
                raise PackageError(f"unallowlisted file under explicit prefix: {relative}")

    return [selected[path] for path in sorted(selected)]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def git_revision(root: Path) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise PackageError("git revision lookup failed; refusing an unverifiable package") from error
    revision = result.stdout.strip()
    if not revision:
        raise PackageError("git revision is empty; refusing an unverifiable package")
    return revision


def file_records(root: Path, candidates: Iterable[Candidate]) -> list[dict[str, object]]:
    records = []
    for candidate in candidates:
        path = ensure_relative_regular(root, candidate.path)
        records.append(
            {
                "path": candidate.path,
                "source": candidate.source,
                "size": path.stat().st_size,
                "sha256": sha256_file(path),
            }
        )
    return records


def archive_bytes(root: Path, candidates: Iterable[Candidate]) -> bytes:
    """Return deterministic gzip/tar bytes for candidates."""

    import io

    output = io.BytesIO()
    with gzip.GzipFile(fileobj=output, mode="wb", mtime=0, filename="") as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
            for candidate in candidates:
                path = ensure_relative_regular(root, candidate.path)
                data = path.read_bytes()
                info = tarfile.TarInfo(candidate.path)
                info.size = len(data)
                info.mode = 0o755 if path.suffix.lower() == ".sh" else 0o644
                info.mtime = 0
                info.uid = 0
                info.gid = 0
                info.uname = ""
                info.gname = ""
                archive.addfile(info, io.BytesIO(data))
    return output.getvalue()


def manifest_for(
    root: Path,
    candidates: list[Candidate],
    archive: bytes | None,
    records: list[dict[str, object]] | None = None,
) -> dict[str, object]:
    files = records if records is not None else file_records(root, candidates)
    return {
        "schema": "solvers.gcp-source-package/v1",
        "root": ".",
        "git_revision": git_revision(root),
        "archive_sha256": hashlib.sha256(archive).hexdigest() if archive is not None else None,
        "file_count": len(files),
        "files": files,
        "excluded": [
            ".git",
            ".agents",
            ".cache",
            ".codex",
            "runs",
            "target",
            "credentials",
            "secrets",
            "credential/secret filenames",
        ],
        "bootstrap": {
            "script": "tools/gcp_convergence_bootstrap.sh",
            "source_archive_destination": "/opt/solvers-experiment/source.tgz",
        },
    }


def atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    try:
        temporary.write_bytes(data)
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def build_package(root: Path, output: Path, manifest_path: Path, dry_run: bool) -> dict[str, object]:
    tracked = git_tracked_paths(root)
    candidates = collect_candidates(root, tracked)
    before = file_records(root, candidates)
    archive = None if dry_run else archive_bytes(root, candidates)
    after = file_records(root, candidates)
    if before != after:
        raise PackageError("workspace changed while creating archive; refusing stale manifest")
    manifest = manifest_for(root, candidates, archive, records=after)
    if not dry_run:
        assert archive is not None
        atomic_write(output, archive)
        atomic_write(manifest_path, (json.dumps(manifest, indent=2, ensure_ascii=False) + "\n").encode())
    return manifest


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, default=ROOT / ".cache" / "gcp-source" / "source.tgz")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--dry-run", action="store_true", help="inventory and hash only; write nothing")
    args = parser.parse_args(argv)
    root = args.root.resolve()
    output = args.output if args.output.is_absolute() else root / args.output
    manifest_path = args.manifest or output.with_suffix(".manifest.json")
    if not manifest_path.is_absolute():
        manifest_path = root / manifest_path
    try:
        manifest = build_package(root, output, manifest_path, args.dry_run)
    except PackageError as error:
        print(f"package error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(manifest, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
