#!/usr/bin/env python3
"""Create a new, pinned research source copy; never edit the supplied source."""
import argparse
import datetime as dt
import difflib
import hashlib
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
WRITER = "crates/formats/src/sol_indexed.rs"
LIB = "crates/formats/src/lib.rs"
EXAMPLE = "crates/formats/examples/sol_codec_bench.rs"
EXAMPLE_SHA = "ed972ffce351fcd31dfbab74771a06a059ef0edb43e44d65397e65f9a85cbcea"
SIGNATURE = "pub fn write_sol(path: &Path, payload: &SolPayload) -> Result<(), SolError> {"


def require(ok, why):
    if not ok:
        raise ValueError(why)


def identity(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def replace(text, old, new):
    require(text.count(old) == 1, "source anchor missing or ambiguous: " + old[:90])
    return text.replace(old, new, 1)


def patch_writer(original, runtime):
    require(original.count(SIGNATURE) == 1 and original.endswith("    Ok(())\n}\n"), "writer boundary changed")
    body = original.split(SIGNATURE, 1)[1]
    validation, body = body.split("    // Size without allocating", 1)
    result = "\n    phases.measure(\"validation\", || -> Result<(), SolError> {" + validation + "        Ok(())\n    })?;\n    // Size without allocating" + body
    group_start = result.index("    let mut groups = Vec::new();")
    group_end = result.index("    let meta = SolMetadata {")
    group = result[group_start:group_end]
    result = result[:group_start] + "    let groups = phases.measure(\"size_and_group\", || -> Result<_, SolError> {\n" + group + "        Ok(groups)\n    })?;\n" + result[group_end:]
    result = replace(result, "    let meta = SolMetadata {", "    let meta = phases.measure(\"metadata_prepare\", || SolMetadata {")
    result = replace(result, "        stored_nodes: payload.blocks.len() as u64,\n    };", "        stored_nodes: payload.blocks.len() as u64,\n    });")
    result = replace(result, "postcard::to_allocvec(&meta)?", "phases.measure(\"serialize\", || postcard::to_allocvec(&meta))?")
    result = replace(result, "zstd::encode_all(raw.as_slice(), 0)?", "phases.measure(\"metadata_compress\", || zstd::encode_all(raw.as_slice(), 0))?")
    result = replace(result, "tempfile::NamedTempFile::new_in(dir)?", "phases.measure(\"temp_create\", || tempfile::NamedTempFile::new_in(dir))?")
    result = replace(result, "    let file = temporary.as_file_mut();", "    let mut tapped = r1_write_phase::FileTap { inner: temporary.as_file_mut(), clock: phases };\n    let file = &mut tapped;")
    result = replace(result, "config_hash(payload.config_toml.as_bytes())", "phases.measure(\"hash\", || config_hash(payload.config_toml.as_bytes()))")
    require(result.count("blake3::hash(&raw)") == 2, "hash call count changed")
    result = result.replace("blake3::hash(&raw)", "phases.measure(\"hash\", || blake3::hash(&raw))")
    result = replace(result, "        let pairs: Vec<_> = payload.blocks[group.clone()]", "        let pairs: Vec<_> = phases.measure(\"pair_refs\", || payload.blocks[group.clone()]")
    result = replace(result, "            .collect();", "            .collect());")
    result = replace(result, "postcard::to_allocvec(&pairs)?", "phases.measure(\"serialize\", || postcard::to_allocvec(&pairs))?")
    compression = """        let mut encoder = zstd::Encoder::new(&mut *file, 0)?;
        encoder.include_checksum(true)?;
        encoder.write_all(&raw)?;
        encoder.finish()?;"""
    result = replace(result, compression, "        phases.compress(|| -> Result<(), SolError> {\n" + compression + "\n            Ok(())\n        })?;")
    result = replace(result, "        let compressed_len = file.stream_position()? - offset;", "        let compressed_len = file.stream_position()? - offset;\n        phases.check_chunk_bytes(compressed_len);")
    result = replace(result, "temporary.persist(path).map_err(|e| e.error)?;", "phases.measure(\"persist\", || temporary.persist(path).map_err(|e| e.error))?;")
    measured = "fn r1_write_sol_measured(path: &Path, payload: &SolPayload, phases: &r1_write_phase::Clock) -> Result<(), SolError> {" + result
    # The original complete function is preserved byte-for-byte, used by OFF.
    return original + "\n" + runtime + "\n" + measured


def patch_example(original):
    old = """            let start = Instant::now();
            write_sol(&path, black_box(&payload))?;
            elapsed = start.elapsed().as_secs_f64();"""
    new = """            // Research control is read before the parent timer.
            let phase_path = std::env::var_os("R1_SOL_WRITE_PHASE_OUTPUT");
            if phase_path.as_ref().is_some_and(|p| Path::new(p).exists()) {
                return Err(invalid("phase output already exists").into());
            }
            let start = Instant::now();
            let (write_result, phase_record) = if phase_path.is_some() {
                let (result, record) = formats::r1_write_sol_profiled(&path, black_box(&payload));
                (result, Some(record))
            } else {
                (write_sol(&path, black_box(&payload)), None)
            };
            let parent_duration = start.elapsed();
            elapsed = parent_duration.as_secs_f64();
            // Publication is outside both writer timers, including on a normal Err.
            if let Err(error) = &write_result {
                eprintln!("original writer error (before phase publication): {error}");
            }
            if let (Some(destination), Some(record)) = (phase_path, phase_record) {
                let destination = Path::new(&destination);
                let mut temporary = tempfile::NamedTempFile::new_in(destination.parent().unwrap_or(Path::new(".")))?;
                serde_json::to_writer_pretty(&mut temporary, &json!({
                    "schema": "r1.sol-write-phase-sample/v1",
                    "parent_total_ns": parent_duration.as_nanos(), "phase": record,
                    "writer_error": write_result.as_ref().err().map(ToString::to_string),
                    "scope": "Research writer after own preload; no quality or memory claim"
                }))?;
                temporary.write_all(b"\\n")?;
                temporary.as_file().sync_all()?;
                temporary.persist_noclobber(destination)?;
            }
            write_result?;"""
    return replace(original, old, new)


def create_copy(source, out, role, mode, example, rustfmt=None):
    source, out, example = source.resolve(strict=True), out.resolve(), example.resolve(strict=True)
    require(source.is_dir() and not out.exists(), "source must exist and output must be new")
    require(not out.is_relative_to(source) and not source.is_relative_to(out), "source/output must not overlap")
    pins = json.loads((HERE / "source-pins.json").read_bytes())["roles"][role]
    files = {}
    actual = {p.relative_to(source).as_posix() for p in (source / "crates").rglob("*") if p.is_file()}
    expected = {p for p in pins["files"] if p.startswith("crates/")}
    require(actual in (expected, expected | {EXAMPLE}), "source crates include missing/unpinned files")
    for name, pin in pins["files"].items():
        path = source / name
        require(not path.is_symlink(), "source symlink forbidden")
        require(path.stat().st_size == pin["bytes"], "source revision/hash mismatch: " + name)
        raw = path.read_bytes()
        require(identity(raw) == pin, "source revision/hash mismatch: " + name)
        files[name] = raw
    example_raw = example.read_bytes()
    require(identity(example_raw)["sha256"] == EXAMPLE_SHA, "benchmark example changed")
    if (source / EXAMPLE).exists():
        require((source / EXAMPLE).read_bytes() == example_raw, "source example differs")
    files[EXAMPLE] = example_raw
    before = dict(files)
    formatter = None
    if mode == "instrumented":
        require(rustfmt is not None, "instrumented copy requires explicit --rustfmt executable")
        rustfmt = rustfmt.resolve(strict=True)
        require(rustfmt.stem != "rustup", "provide actual toolchain rustfmt (rustup which rustfmt), not an argv0 proxy")
        formatter = {"path": str(rustfmt), **identity(rustfmt.read_bytes()),
                     "version": subprocess.check_output([str(rustfmt), "--version"], text=True).strip()}
        require(formatter["version"].startswith("rustfmt "), "unexpected formatter version")
        runtime = (HERE / "runtime.rs.inc").read_text(encoding="utf-8")
        files[WRITER] = patch_writer(files[WRITER].decode(), runtime).encode()
        files[LIB] += b"\n// Research-only export; absent from production.\npub use sol_indexed::r1_write_sol_profiled;\n"
        files[EXAMPLE] = patch_example(files[EXAMPLE].decode()).encode()
        for name in (WRITER, LIB, EXAMPLE):
            result = subprocess.run([str(rustfmt), "--edition", "2024", "--config", "skip_children=true", "--emit", "stdout"],
                                    input=files[name], capture_output=True, check=True)
            require(not result.stderr, "rustfmt diagnostic: " + result.stderr.decode(errors="replace"))
            files[name] = result.stdout
        require(files[WRITER].startswith(before[WRITER]), "formatting changed the unmodified writer/source prefix")
        require(identity(rustfmt.read_bytes())["sha256"] == formatter["sha256"], "formatter changed")
    artifacts = {name: identity((HERE / name).read_bytes()) for name in
                 ("apply.py", "runtime.rs.inc", "source-pins.json", "protocol.json", "validate.py")}
    manifest = {"schema": "r1.write-phase-source-copy/v1", "role": role, "mode": mode,
                "revision": pins["revision"], "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                "scope": "Pinned Cargo workspace/crates only; full repository docs/evidence not copied",
                "source_path": str(source), "output_path": str(out), "instrumentation": artifacts,
                "formatter": formatter,
                "before": {name: identity(raw) for name, raw in before.items()},
                "after": {name: identity(raw) for name, raw in files.items()},
                "example_added": EXAMPLE not in pins["files"]}
    patch = "".join("".join(difflib.unified_diff(before[name].decode().splitlines(True), raw.decode().splitlines(True),
                     fromfile="a/" + name, tofile="b/" + name)) for name, raw in files.items() if raw != before[name])
    out.mkdir(parents=True)
    try:
        for name, raw in files.items():
            path = out / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
        (out / "instrumentation.patch").write_text(patch, encoding="utf-8", newline="\n")
        (out / "r1-write-phase-source.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")
    except BaseException:
        # Leave visibly incomplete new output for inspection; never erase source.
        raise
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--role", choices=("baseline", "candidate"), required=True)
    parser.add_argument("--mode", choices=("plain", "instrumented"), required=True)
    parser.add_argument("--example", type=Path, required=True)
    parser.add_argument("--rustfmt", type=Path, help="explicit executable required for instrumented copies; recorded in manifest")
    args = parser.parse_args()
    manifest = create_copy(args.source, args.out, args.role, args.mode, args.example, args.rustfmt)
    print(json.dumps({"status": "prepared_not_built", "revision": manifest["revision"], "mode": args.mode}))


if __name__ == "__main__":
    main()
