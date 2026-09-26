#!/usr/bin/env python3
"""Prepare exact plain/instrumented research copies; never build or run them."""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
REVISION = "11e4062ba1735e58b60d12999cb23ed10fd1a163"
LIB = "crates/cli/src/lib.rs"
SOLVE = "crates/cli/src/solve.rs"
SOL = "crates/cli/src/sol.rs"
MODULE = "crates/cli/src/r1_current_phase.rs"
CODEC = "crates/cli/examples/current_phase_codec.rs"


def identity(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def require(ok, message):
    if not ok:
        raise ValueError(message)


def once(text, old, new):
    require(text.count(old) == 1, "missing or ambiguous anchor: " + old[:80])
    return text.replace(old, new, 1)


def instrumentation_id():
    return hashlib.sha256((HERE / "prepare.py").read_bytes() + b"\0" +
                          (HERE / "runtime.rs.in").read_bytes()).hexdigest()


def patch(originals, codec):
    """Pure deterministic transformation: no Rust tool invocation."""
    files = dict(originals)
    lib = files[LIB].decode()
    lib = once(lib, "pub mod solve;", "pub mod solve;\nmod r1_current_phase;")
    lib = once(lib, "pub fn main_impl() -> Result<()> {", """pub fn main_impl() -> Result<()> {
    let session = r1_current_phase::Session::start()?;
    let outcome = r1_current_phase_dispatch();
    if let Some(session) = session {
        if let Err(error) = session.finish(outcome.is_ok(), outcome.as_ref().err().map(|e| format!("{e:#}"))) {
            if outcome.is_ok() { return Err(error); }
            eprintln!("Phase publication failed; original command error preserved: {error:#}");
        }
    }
    outcome
}

fn r1_current_phase_dispatch() -> Result<()> {""")
    files[LIB] = lib.encode()
    solve = files[SOLVE].decode()
    solve = once(solve, "    let raw_bytes =\n", "    crate::r1_current_phase::switch(\"input_preparation\");\n    let raw_bytes =\n")
    solve = once(solve, "        solver.run(chunk);", "        crate::r1_current_phase::measure(\"cfr_updates\", Some(solver.iteration() + chunk), || solver.run(chunk));")
    solve = once(solve, "        let evaluation = RootEvaluation::measure(solver);", "        let evaluation = crate::r1_current_phase::measure(\"periodic_ev_br\", Some(solver.iteration()), || RootEvaluation::measure(solver));")
    solve = once(solve, "    formats::write_checkpoint_ref(path, hash, &solver.state_ref())?;", "    crate::r1_current_phase::measure_result(\"checkpoint\", Some(solver.iteration()), || formats::write_checkpoint_ref(path, hash, &solver.state_ref()))?;")
    left, delimiter, tail = solve.partition("fn solve_postflop<S: Storage>(")
    require(bool(delimiter), "missing postflop start")
    body, end, right = tail.partition("/// Builds the 169-class preflop trunk")
    require(bool(end), "missing postflop end")
    body = once(body, "    // Cheap dry run before", "    crate::r1_current_phase::switch(\"initialization\");\n    // Cheap dry run before")
    body = once(body, "    let start = Instant::now();", "    crate::r1_current_phase::switch(\"overhead\");\n    let start = Instant::now();")
    body = once(body, "    let evaluation = result\n", "    crate::r1_current_phase::switch(\"final_ev_br\");\n    let evaluation = result\n")
    body = once(body, "    if result.checkpoint_iteration", "    crate::r1_current_phase::switch(\"overhead\");\n    if result.checkpoint_iteration")
    solve = left + delimiter + body + end + right
    solve = once(solve, "    println!(\n        \"done: iterations=", "    crate::r1_current_phase::switch(\"summary_publish\");\n    println!(\n        \"done: iterations=")
    files[SOLVE] = solve.encode()
    sol = files[SOL].decode()
    anchor = ") -> Result<()> {\n    let tree = &solver.game().tree;"
    sol = once(sol, anchor, ") -> Result<()> {\n    crate::r1_current_phase::switch(\"sol_preparation\");\n    let tree = &solver.game().tree;")
    sol = once(sol, "    write_sol(&spec.path, &payload).with_context", "    crate::r1_current_phase::switch(\"overhead\");\n    crate::r1_current_phase::measure_result(\"sol_serialization_and_write\", Some(solver.iteration()), || write_sol(&spec.path, &payload)).with_context")
    files[SOL] = sol.encode()
    runtime = (HERE / "runtime.rs.in").read_text(encoding="utf-8")
    files[MODULE] = runtime.replace("@@REVISION@@", REVISION).replace("@@INSTRUMENTATION@@", instrumentation_id()).encode()
    text = codec.decode()
    text = once(text, "fn main() -> Result<(), Box<dyn Error>> {", """#[path = "../src/r1_current_phase.rs"]
mod r1_current_phase;

fn main() -> Result<(), Box<dyn Error>> {
    let session = r1_current_phase::Session::start()?;
    let outcome = run_codec();
    if let Some(session) = session {
        if let Err(error) = session.finish(outcome.is_ok(), outcome.as_ref().err().map(ToString::to_string)) {
            if outcome.is_ok() { return Err(error.into()); }
            eprintln!("Phase publication failed; original codec error preserved: {error:#}");
        }
    }
    outcome
}

fn run_codec() -> Result<(), Box<dyn Error>> {""")
    text = once(text, '        "decode-all" => {\n', '        "decode-all" => {\n            r1_current_phase::switch("codec_decode_all");\n')
    text = once(text, '            stored_nodes = payload.blocks.len() as u64;\n        }', '            r1_current_phase::switch("overhead");\n            stored_nodes = payload.blocks.len() as u64;\n        }')
    text = once(text, '        "read-root" | "read-repeat-chunk" => {\n', '        "read-root" | "read-repeat-chunk" => {\n            r1_current_phase::switch("codec_read_root");\n')
    text = once(text, '            let meta = reader.metadata();', '            r1_current_phase::switch("overhead");\n            let meta = reader.metadata();')
    text = once(text, '            let path = output.join("rewritten.sol");', '            let path = output.join("rewritten.sol");\n            r1_current_phase::switch("codec_stream_write");')
    text = once(text, '            // Outside timing: normal validation plus exact semantic equality.', '            r1_current_phase::switch("overhead");\n            // Outside timing: normal validation plus exact semantic equality.')
    files[CODEC] = text.encode()
    return files


def load_source(source, pins):
    require(source.is_dir() and not source.is_symlink(), "source must be a regular directory")
    actual = set()
    for prefix in ("crates", ".cargo"):
        base = source / prefix
        require(base.is_dir() and not base.is_symlink(), "missing or linked source directory")
        for path in base.rglob("*"):
            require(not path.is_symlink(), "source symlinks forbidden: " + str(path))
            if path.is_file():
                actual.add(path.relative_to(source).as_posix())
    actual.update(name for name in ("Cargo.toml", "Cargo.lock") if (source / name).is_file())
    require(actual == set(pins), "source closure differs (missing or extra Cargo/crates files)")
    files = {}
    for name, expected in pins.items():
        path = source / name
        require(not path.is_symlink(), "source links forbidden")
        raw = path.read_bytes()
        require(identity(raw) == expected, "source pin mismatch: " + name)
        files[name] = raw
    return files


def create_copy(source, out, mode):
    source, out = Path(source).resolve(strict=True), Path(out).resolve()
    require(mode in ("plain", "instrumented"), "unknown mode")
    require(not out.exists(), "output must be new")
    require(not out.is_relative_to(source) and not source.is_relative_to(out), "source/output overlap")
    pins = json.loads((HERE / "source-pins.json").read_bytes())
    require(pins["revision"] == REVISION, "source revision differs")
    originals = load_source(source, pins["files"])
    codec = (HERE / "codec-input.rs").read_bytes()
    require(identity(codec) == pins["codec_input"], "codec helper changed")
    files = patch(originals, codec) if mode == "instrumented" else {**originals, CODEC: codec}
    expected = pins["instrumented_changed"] if mode == "instrumented" else {CODEC: pins["codec_input"]}
    for name, pin in expected.items():
        require(identity(files[name]) == pin, "generated source no longer matches fixed manifest: " + name)
    changed = {name for name in files if files[name] != originals.get(name)}
    require(changed == set(expected), "unexpected generated source changes")
    manifest = {
        "schema": "r1.current-phases-source-copy/v1", "source_revision": REVISION,
        "mode": mode, "instrumentation_id": instrumentation_id(),
        "source_path": str(source), "output_path": str(out),
        "scope": "Exact Cargo/crates closure; docs/research are not compiler inputs or copied",
        "before": pins["files"], "after": {name: identity(raw) for name, raw in sorted(files.items())},
        "controls": {name: identity((HERE / name).read_bytes()) for name in
                     ("prepare.py", "runtime.rs.in", "source-pins.json", "protocol.json", "codec-input.rs")},
        "status": "prepared_not_built",
    }
    diff = "".join("".join(difflib.unified_diff(originals.get(name, b"").decode().splitlines(True),
                     files[name].decode().splitlines(True), fromfile="a/" + name, tofile="b/" + name))
                   for name in sorted(changed))
    out.mkdir(parents=True)
    # No rollback/deletion: a partial new output remains inspectable on write failure.
    for name, raw in files.items():
        path = out / name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as stream:
            stream.write(raw)
    with (out / "instrumentation.patch").open("x", encoding="utf-8", newline="\n") as stream:
        stream.write(diff)
    with (out / "source-copy.json").open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(manifest, stream, indent=2, sort_keys=True)
        stream.write("\n")
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--mode", choices=("plain", "instrumented"), required=True)
    args = parser.parse_args()
    result = create_copy(args.source, args.out, args.mode)
    print(json.dumps({key: result[key] for key in ("status", "source_revision", "mode", "instrumentation_id")}))
