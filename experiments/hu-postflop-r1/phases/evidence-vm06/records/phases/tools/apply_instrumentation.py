#!/usr/bin/env python3
"""Apply exact, source-pinned research instrumentation to a disposable copy."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile

HERE = Path(__file__).resolve().parent
PATCHED = ("crates/cli/src/lib.rs", "crates/cli/src/solve.rs", "crates/cli/src/sol.rs")
MODULE = "crates/cli/src/r1_phase.rs"
MANIFEST = "r1-phase-source-manifest.json"


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def is_build_input(path: str) -> bool:
    parts = Path(path).parts
    return (
        path in ("Cargo.toml", "Cargo.lock")
        or (parts[0] == "crates" and (path.endswith(".rs") or parts[-1] in ("Cargo.toml", "build.rs")))
        or (parts[0] == ".cargo" and path.endswith(".toml"))
    )


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise ValueError(f"{label}: expected one exact insertion site, got {count}")
    return text.replace(old, new, 1)


def patch_sources(source: str, originals: dict[str, bytes], module: bytes) -> dict[str, bytes]:
    texts = {path: data.decode("utf-8") for path, data in originals.items()}
    # Preserve the original newline style; all exact anchors use LF.
    endings = {path: "\r\n" if "\r\n" in text else "\n" for path, text in texts.items()}
    texts = {path: text.replace("\r\n", "\n") for path, text in texts.items()}
    lib = texts[PATCHED[0]]
    lib = replace_once(lib, "pub mod solve;", "pub mod solve;\nmod r1_phase;", "module")
    lib = replace_once(lib, "pub fn main_impl() -> Result<()> {\n", """pub fn main_impl() -> Result<()> {
    let phase_session = r1_phase::Session::start()?;
    let outcome = main_impl_phase_dispatch();
    if let Some(session) = phase_session {
        if let Err(error) = session.finish(&outcome) {
            if outcome.is_ok() { return Err(error); }
            eprintln!("Phase record failed (original command error preserved): {error:#}");
        }
    }
    outcome
}

fn main_impl_phase_dispatch() -> Result<()> {
""", "dispatch wrapper")
    texts[PATCHED[0]] = lib
    solve = texts[PATCHED[1]]
    solve = replace_once(solve, "    let raw_bytes =\n", "    crate::r1_phase::switch(\"input_preparation\");\n    let raw_bytes =\n", "input")
    solve = replace_once(solve, "        solver.run(chunk);", "        crate::r1_phase::measure(\"cfr_updates\", Some(solver.iteration() + chunk), || solver.run(chunk));", "CFR chunk")
    periodic = "let expl = solver.exploitability();" if source == "baseline9632" else "let evaluation = RootEvaluation::measure(solver);"
    replacement = ("let expl = crate::r1_phase::measure(\"periodic_ev_br\", Some(solver.iteration()), || solver.exploitability());"
        if source == "baseline9632" else "let evaluation = crate::r1_phase::measure(\"periodic_ev_br\", Some(solver.iteration()), || RootEvaluation::measure(solver));")
    solve = replace_once(solve, "        " + periodic, "        " + replacement, "periodic evaluation")
    checkpoint = ("formats::write_checkpoint(path, hash, &solver.state())" if source == "baseline9632"
                  else "formats::write_checkpoint_ref(path, hash, &solver.state_ref())")
    solve = replace_once(solve, "    " + checkpoint + "?;", "    crate::r1_phase::measure_result(\"checkpoint\", Some(solver.iteration()), || " + checkpoint + ")?;", "checkpoint")
    # Restrict setup/final-summary anchors to solve_postflop, keeping other games untouched.
    left, sep, tail = solve.partition("fn solve_postflop<S: Storage>(")
    if not sep:
        raise ValueError("missing solve_postflop")
    postflop, sep_end, right = tail.partition("/// Builds the 169-class preflop trunk")
    if not sep_end:
        raise ValueError("missing postflop end")
    postflop = replace_once(postflop, "    // Cheap dry run before", "    crate::r1_phase::switch(\"initialization\");\n    // Cheap dry run before", "initialization")
    postflop = replace_once(postflop, "    let start = Instant::now();", "    crate::r1_phase::switch(\"overhead\");\n    let start = Instant::now();", "end initialization")
    final_anchor = "    let summary = print_done(" if source == "baseline9632" else "    let evaluation = result\n"
    postflop = replace_once(postflop, final_anchor, "    crate::r1_phase::switch(\"final_ev_br\");\n" + final_anchor, "final evaluation")
    if source == "baseline9632":
        postflop = replace_once(postflop, "    checkpoint_now(&solver, hooks)?;", "    crate::r1_phase::switch(\"overhead\");\n    checkpoint_now(&solver, hooks)?;", "end summary")
    else:
        postflop = replace_once(postflop, "    if result.checkpoint_iteration", "    crate::r1_phase::switch(\"overhead\");\n    if result.checkpoint_iteration", "end summary")
    solve = left + sep + postflop + sep_end + right
    # The argument EV calculations precede these println calls; switching here keeps them in final_ev_br.
    solve = replace_once(solve, "    println!(\n        \"done: iterations=", "    crate::r1_phase::switch(\"summary_publish\");\n    println!(\n        \"done: iterations=", "summary publish")
    texts[PATCHED[1]] = solve
    sol = texts[PATCHED[2]]
    anchor = ") -> Result<()> {\n    let tree = &solver.game().tree;"
    sol = replace_once(sol, anchor, ") -> Result<()> {\n    crate::r1_phase::switch(\"sol_preparation\");\n    let tree = &solver.game().tree;", "sol preparation")
    sol = replace_once(sol, "    write_sol(&spec.path, &payload).with_context", "    crate::r1_phase::switch(\"overhead\");\n    crate::r1_phase::measure_result(\"sol_serialization_and_write\", Some(solver.iteration()), || write_sol(&spec.path, &payload)).with_context", "sol write")
    texts[PATCHED[2]] = sol
    result = {path: text.replace("\n", endings[path]).encode("utf-8") for path, text in texts.items()}
    result[MODULE] = module
    return result


def apply(root: Path, source: str) -> dict:
    root = root.resolve(strict=True)
    if root == HERE.parents[2]:
        raise ValueError("refusing to instrument the owning repository; use an extracted copy")
    if not root.is_dir():
        raise ValueError("--root must be a directory")
    if (root / MODULE).exists() or (root / MANIFEST).exists():
        raise ValueError("copy is already instrumented or contains conflicting phase files")
    versions_bytes = (HERE / "source-versions.json").read_bytes()
    version = json.loads(versions_bytes)[source]
    actual_paths = set()
    for prefix in ("crates", ".cargo"):
        base = root / prefix
        if base.exists():
            for path in base.rglob("*"):
                rel = path.relative_to(root).as_posix()
                if path.is_symlink():
                    raise ValueError(f"source symlink is not supported: {rel}")
                if path.is_file() and is_build_input(rel):
                    actual_paths.add(rel)
    actual_paths.update(p for p in ("Cargo.toml", "Cargo.lock") if (root / p).is_file())
    expected = version["build_inputs"]
    if actual_paths != set(expected):
        raise ValueError(f"source file set differs: missing={sorted(set(expected)-actual_paths)}, extra={sorted(actual_paths-set(expected))}")
    for path, digest in expected.items():
        if sha((root / path).read_bytes()) != digest:
            raise ValueError(f"source version/hash mismatch: {path}")
    originals = {path: (root / path).read_bytes() for path in PATCHED}
    template = (HERE / "r1_phase.rs.in").read_bytes()
    patch_id = sha(Path(__file__).read_bytes() + b"\0" + template + b"\0" + versions_bytes)
    module = template.replace(b"@@SOURCE_VERSION@@", source.encode()).replace(b"@@PATCH_ID@@", patch_id.encode())
    changes = patch_sources(source, originals, module)
    manifest = {
        "schema": "r1.phase.source/v1", "source_version": source,
        "source_identity": version["identity"], "instrumentation_id": patch_id,
        "before": expected, "after": dict(expected),
        "modified": list(PATCHED), "added": [MODULE],
    }
    manifest["after"].update({path: sha(data) for path, data in changes.items()})
    # All validation and replacement happen before the first mutation. Roll back ordinary IO failures.
    written = []
    try:
        for path, data in changes.items():
            target = root / path
            with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as temp:
                temporary = Path(temp.name)
                temp.write(data)
            try:
                os.replace(temporary, target)
            finally:
                temporary.unlink(missing_ok=True)
            written.append(path)
        with (root / MANIFEST).open("x", encoding="utf-8", newline="\n") as stream:
            json.dump(manifest, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
    except BaseException:
        for path in reversed(written):
            if path in originals:
                (root / path).write_bytes(originals[path])
            else:
                (root / path).unlink(missing_ok=True)
        (root / MANIFEST).unlink(missing_ok=True)
        raise
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path, help="disposable extracted source tree")
    parser.add_argument("--source", required=True, choices=("baseline9632", "candidate03"))
    args = parser.parse_args()
    manifest = apply(args.root, args.source)
    print(json.dumps({"source_version": args.source, "instrumentation_id": manifest["instrumentation_id"],
                      "manifest": str(args.root.resolve() / MANIFEST)}, indent=2))


if __name__ == "__main__":
    main()
