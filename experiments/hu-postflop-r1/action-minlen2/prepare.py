"""Prepare one pinned research source file without changing production."""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
from pathlib import Path

SOURCE = "crates/engine/src/solver.rs"
BASE_SHA256 = "e116e2554915768e0ccef89e86dccb0ffb89d0a2ff309343c0992fc8d7b02e4b"
BEFORE = """        .zip(children.par_chunks_mut(my_reach.len()))
        .enumerate()
        .for_each_init(Scratch::new, |scratch, (a, (view, child_out))| {"""
AFTER = BEFORE.replace("        .for_each_init", "        .with_min_len(2)\n        .for_each_init")


def candidate(original: bytes) -> bytes:
    if hashlib.sha256(original).hexdigest() != BASE_SHA256:
        raise ValueError("Source pin mismatch; do not retarget the frozen candidate")
    text = original.decode("utf-8")
    if text.count(BEFORE) != 1:
        raise ValueError("Expected exactly one CFR action iterator")
    changed = text.replace(BEFORE, AFTER, 1).encode("utf-8")
    if changed.replace(AFTER.encode(), BEFORE.encode(), 1) != original:
        raise ValueError("Unexpected change outside the single insertion")
    return changed


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True,
                        help="New directory holding only research source and metadata")
    args = parser.parse_args()
    root = args.source_root.resolve(strict=True)
    out = args.out.resolve()
    if out.is_relative_to(root / "crates") or out.is_relative_to(root / "target"):
        raise ValueError("Use a separate research directory, outside crates/ and target/")
    original = (root / SOURCE).read_bytes()
    changed = candidate(original)
    patch = "".join(difflib.unified_diff(
        original.decode().splitlines(keepends=True),
        changed.decode().splitlines(keepends=True),
        fromfile="a/" + SOURCE, tofile="b/" + SOURCE,
    )).encode()
    out.mkdir(parents=True, exist_ok=False)
    (out / "solver.rs").write_bytes(changed)
    (out / "candidate.patch").write_bytes(patch)
    (out / "manifest.json").write_text(json.dumps({
        "schema": "solvers.r1.action-minlen2-preparation/v1",
        "source_path": SOURCE,
        "source_sha256": BASE_SHA256,
        "candidate_sha256": hashlib.sha256(changed).hexdigest(),
        "candidate_bytes": len(changed),
        "patch_sha256": hashlib.sha256(patch).hexdigest(),
        "edit": "CFR action child iterator only: with_min_len(2)",
        "rust_compile": "not_run", "performance": None,
        "production_adopted": False,
    }, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
