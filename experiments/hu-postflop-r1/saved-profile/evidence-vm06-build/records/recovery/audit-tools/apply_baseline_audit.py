#!/usr/bin/env python3
"""Apply/verify a source-pinned v1 quality audit in a disposable baseline copy."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile

HERE = Path(__file__).resolve().parent
SOL = "crates/cli/src/sol.rs"
MODULE = "crates/cli/src/r1_saved_profile_audit.rs"
EXAMPLE = "crates/cli/examples/hu_saved_profile_audit.rs"
MANIFEST = "r1-saved-profile-source-manifest.json"
START = "// --- research-only saved-profile evaluation "
END = "// --- strategy-source seam "
INCLUDE = 'include!("r1_saved_profile_audit.rs");\n\n'


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def replace_once(text: str, old: str, new: str) -> str:
    if text.count(old) != 1:
        raise ValueError(f"expected one exact source anchor: {old!r}")
    return text.replace(old, new, 1)


def build_input(path: str) -> bool:
    parts = Path(path).parts
    return (
        path in ("Cargo.toml", "Cargo.lock")
        or (parts[0] == "crates" and (path.endswith(".rs") or parts[-1] == "Cargo.toml"))
        or (parts[0] == ".cargo" and path.endswith(".toml"))
    )


def input_hashes(root: Path) -> dict[str, str]:
    result = {}
    for prefix in ("crates", ".cargo"):
        for path in (root / prefix).rglob("*"):
            if path.is_symlink():
                raise ValueError(f"source symlink is unsupported: {path}")
            relative = path.relative_to(root).as_posix()
            if path.is_file() and build_input(relative):
                result[relative] = sha(path.read_bytes())
    for name in ("Cargo.toml", "Cargo.lock"):
        path = root / name
        if path.is_symlink():
            raise ValueError(f"source symlink is unsupported: {path}")
        if path.is_file():
            result[name] = sha(path.read_bytes())
    return dict(sorted(result.items()))


def require_same(actual: dict, expected: dict) -> None:
    differences = sorted(key for key in actual.keys() | expected.keys() if actual.get(key) != expected.get(key))
    if differences:
        raise ValueError(f"source build-input manifest diff is not zero: {differences}")


def bundle(reference_root: Path | None = None) -> tuple[dict, bytes, bytes, str]:
    pins_bytes = (HERE / "source-pins.json").read_bytes()
    pins = json.loads(pins_bytes)
    shared = (HERE / "audit_shared.rs.in").read_bytes()
    example = (HERE / "audit_example.rs.in").read_bytes()
    adapter = (HERE / "baseline_adapter.rs.in").read_bytes()
    if sha(shared) != pins["candidate_shared_sha256"] or sha(example) != pins["candidate_example_sha256"]:
        raise ValueError("frozen candidate helper/example digest mismatch")
    if reference_root is not None:
        source = (reference_root / SOL).read_text(encoding="utf-8")
        left = source.index(START)
        source = source[left:source.index(END, left)].encode("utf-8")
        if source != shared or (reference_root / EXAMPLE).read_bytes() != example:
            raise ValueError("candidate reference helper/example differs from frozen snapshot04 template")
    port = replace_once(shared.decode("utf-8"),
        "let metadata = formats::SolReader::open(path)?.metadata().clone();",
        "let metadata = read_baseline_audit_metadata(path)?;")
    port = replace_once(port, "let loaded = load_sol(path, 0, None)?;",
        "let loaded = load_sol(path, 0, None)?;\n        validate_baseline_audit_shapes(&loaded)?;\n        let node_count = loaded.pf_game.game.tree.nodes.len() as u64;")
    port = replace_once(port, "format_version: formats::SOL_FORMAT_VERSION,", "format_version: 1,")
    port = replace_once(port, "node_count: metadata.node_count,", "node_count,")
    module = adapter + b"\n" + port.rstrip().encode("utf-8") + b"\n"
    patch_id = sha(Path(__file__).read_bytes() + b"\0" + pins_bytes + b"\0" + shared + b"\0" + adapter + b"\0" + example)
    return pins, module, example, patch_id


def patched_sol(original: bytes) -> bytes:
    text = original.decode("utf-8")
    ending = "\r\n" if "\r\n" in text else "\n"
    text = replace_once(text.replace("\r\n", "\n"), END, INCLUDE + END)
    return text.replace("\n", ending).encode("utf-8")


def atomic_write(path: Path, data: bytes) -> None:
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def run(root: Path, mode: str, reference_root: Path | None = None) -> dict:
    root = root.resolve(strict=True)
    if root == HERE.parents[2] or not root.is_dir():
        raise ValueError("use a disposable extracted baseline copy, not the owning repository")
    pins, module, example, patch_id = bundle(reference_root)
    expected = pins["build_inputs"]
    if mode == "verify":
        manifest = json.loads((root / MANIFEST).read_text(encoding="utf-8"))
        modified = (root / SOL).read_bytes()
        ending = b"\r\n" if b"\r\n" in modified else b"\n"
        include = INCLUDE.replace("\n", ending.decode()).encode()
        if modified.count(include) != 1:
            raise ValueError("baseline audit include marker missing or duplicated")
        original = modified.replace(include, b"", 1)
        if sha(original) != expected[SOL]:
            raise ValueError("baseline sol.rs changed outside the research include")
    else:
        for name in (MODULE, EXAMPLE, MANIFEST):
            if (root / name).exists():
                raise ValueError(f"conflicting existing audit file: {name}")
        require_same(input_hashes(root), expected)
        original = (root / SOL).read_bytes()
    changes = {SOL: patched_sol(original), MODULE: module, EXAMPLE: example}
    after = dict(expected)
    after.update({name: sha(data) for name, data in changes.items()})
    description = {
        "schema": "r1.saved-profile.source/v1",
        "baseline_identity": pins["identity"],
        "candidate_snapshot04_archive_sha256": pins["candidate_snapshot04_archive_sha256"],
        "candidate_shared_sha256": pins["candidate_shared_sha256"],
        "patch_id": patch_id,
        "before": expected,
        "after": after,
        "modified": [SOL],
        "added": [MODULE, EXAMPLE],
        "unchanged_build_input_diff": [],
        "purpose": "quality_only_not_performance",
    }
    if mode == "verify":
        if manifest != description:
            raise ValueError("applied manifest differs from this pinned adapter")
        require_same(input_hashes(root), after)
    elif mode == "apply":
        written = []
        try:
            for name, data in changes.items():
                atomic_write(root / name, data)
                written.append(name)
            require_same(input_hashes(root), after)
            with (root / MANIFEST).open("x", encoding="utf-8", newline="\n") as stream:
                json.dump(description, stream, indent=2, sort_keys=True)
                stream.write("\n")
        except BaseException:
            for name in reversed(written):
                if name == SOL:
                    atomic_write(root / name, original)
                else:
                    (root / name).unlink(missing_ok=True)
            (root / MANIFEST).unlink(missing_ok=True)
            raise
    return {"mode": mode, "patch_id": patch_id, "build_input_diff": [],
            "manifest": str(root / MANIFEST), "baseline_identity": pins["identity"]}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True, help="pristine disposable baseline9632 source copy")
    parser.add_argument("--reference-root", type=Path, help="optional snapshot04 helper/example equality check")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--apply", action="store_true")
    mode.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    selected = "apply" if args.apply else "verify" if args.verify else "check"
    print(json.dumps(run(args.root, selected, args.reference_root), indent=2))


if __name__ == "__main__":
    main()
