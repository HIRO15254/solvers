//! Research-only codec measurement. See experiments/hu-postflop-r1/codec/README.md.
//! Copy this identical file into the baseline checkout; do not change its codec.

use std::error::Error;
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;

use formats::{
    SOL_FORMAT_VERSION, SolMetadata, SolPayload, SolReader, StreetsStored, read_sol, write_sol,
};
use serde_json::json;

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

fn file_hash(path: &Path) -> io::Result<(u64, String)> {
    let mut input = BufReader::new(File::open(path)?);
    let mut digest = blake3::Hasher::new();
    let mut buffer = [0; 65536];
    let mut bytes = 0;
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((bytes, digest.finalize().to_hex().to_string()))
}

fn blob(output: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    output.write_all(&(bytes.len() as u64).to_le_bytes())?;
    output.write_all(bytes)
}

// This independent fixed-width comparison encoding does not use serde/postcard.
// Float bits, raw config bytes, block order and all raw arrays are significant.
fn canonical(path: &Path, payload: &SolPayload) -> io::Result<()> {
    let mut output = BufWriter::new(File::create_new(path)?);
    output.write_all(b"r1.codec.canonical/v1\0")?;
    blob(&mut output, payload.config_toml.as_bytes())?;
    output.write_all(&payload.meta.iterations.to_le_bytes())?;
    for value in payload
        .meta
        .expl
        .into_iter()
        .chain(payload.meta.ev)
        .chain([payload.meta.nash_conv, payload.meta.wall_secs])
    {
        output.write_all(&value.to_bits().to_le_bytes())?;
    }
    blob(&mut output, payload.meta.storage.as_bytes())?;
    output.write_all(&[match payload.mode {
        StreetsStored::Full => 0,
        StreetsStored::NoRivers => 1,
    }])?;
    output.write_all(&payload.node_count.to_le_bytes())?;
    output.write_all(&(payload.blocks.len() as u64).to_le_bytes())?;
    for block in &payload.blocks {
        output.write_all(&block.sref.to_le_bytes())?;
        blob(&mut output, &block.probs)?;
    }
    output.write_all(&(payload.values.len() as u64).to_le_bytes())?;
    for block in &payload.values {
        output.write_all(&block.sref.to_le_bytes())?;
        output.write_all(&block.scale.to_bits().to_le_bytes())?;
        blob(&mut output, &block.values)?;
    }
    output.flush()
}

fn metadata(payload: &SolPayload, stored_nodes: u64) -> SolMetadata {
    SolMetadata {
        config_toml: payload.config_toml.clone(),
        meta: payload.meta.clone(),
        mode: payload.mode,
        node_count: payload.node_count,
        stored_nodes,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 5 {
        return Err(
            invalid("usage: sol_codec_bench INPUT OPERATION ITERATIONS NEW_OUTPUT_DIR").into(),
        );
    }
    let input = Path::new(&args[1]);
    let operation = args[2]
        .to_str()
        .ok_or_else(|| invalid("non-UTF8 operation"))?;
    let iterations: u32 = args[3]
        .to_str()
        .ok_or_else(|| invalid("non-UTF8 iterations"))?
        .parse()?;
    if !matches!(
        operation,
        "decode-all" | "read-root" | "read-repeat-chunk" | "stream-write"
    ) || iterations
        != if operation == "read-repeat-chunk" {
            64
        } else {
            1
        }
    {
        return Err(invalid("unknown operation or iterations differ from frozen protocol").into());
    }
    let output = Path::new(&args[4]);
    fs::create_dir(output)?;
    let before = file_hash(input)?; // Deliberate, identical warm-file policy.
    let mut preparation_seconds = 0.0;
    let mut open_seconds = 0.0;
    let elapsed;
    let payload;
    let stored_nodes;
    let mut written = None;
    match operation {
        "decode-all" => {
            let start = Instant::now();
            payload = read_sol(input)?; // Keep the public reader's complete validation.
            elapsed = start.elapsed().as_secs_f64();
            stored_nodes = payload.blocks.len() as u64;
        }
        "read-root" | "read-repeat-chunk" => {
            let start = Instant::now();
            let mut reader = SolReader::open(input)?;
            open_seconds = start.elapsed().as_secs_f64();
            if reader.stored_srefs().next() != Some(0) {
                return Err(invalid("fixture has no stored root sref 0").into());
            }
            let start = Instant::now();
            let mut last = None;
            for _ in 0..iterations {
                last = Some(black_box(
                    reader
                        .read_node(0)?
                        .ok_or_else(|| invalid("missing root"))?,
                ));
            }
            elapsed = start.elapsed().as_secs_f64();
            let meta = reader.metadata();
            stored_nodes = meta.stored_nodes;
            let (strategy, value) = last.ok_or_else(|| invalid("empty read loop"))?;
            payload = SolPayload {
                config_toml: meta.config_toml.clone(),
                meta: meta.meta.clone(),
                mode: meta.mode,
                node_count: meta.node_count,
                blocks: vec![strategy],
                values: vec![value],
            };
        }
        "stream-write" => {
            let start = Instant::now();
            payload = read_sol(input)?;
            preparation_seconds = start.elapsed().as_secs_f64();
            stored_nodes = payload.blocks.len() as u64;
            let path = output.join("rewritten.sol");
            let start = Instant::now();
            write_sol(&path, black_box(&payload))?;
            elapsed = start.elapsed().as_secs_f64();
            // Outside timing: normal validation plus exact semantic equality.
            if read_sol(&path)? != payload || file_hash(&path)? != before {
                return Err(invalid("rewritten artifact differs from input").into());
            }
            written = Some(json!({"file": "rewritten.sol", "bytes": before.0, "blake3": before.1}));
        }
        _ => unreachable!(),
    }
    if payload.mode != StreetsStored::Full || payload.blocks.is_empty() {
        return Err(invalid("benchmark requires nonempty Full fixture").into());
    }
    let validation_start = Instant::now();
    canonical(&output.join("canonical.bin"), &payload)?;
    let canonical_identity = file_hash(&output.join("canonical.bin"))?;
    let root = SolPayload {
        config_toml: payload.config_toml.clone(),
        meta: payload.meta.clone(),
        mode: payload.mode,
        node_count: payload.node_count,
        blocks: vec![
            payload
                .blocks
                .iter()
                .find(|x| x.sref == 0)
                .ok_or_else(|| invalid("full payload has no root strategy"))?
                .clone(),
        ],
        values: vec![
            payload
                .values
                .iter()
                .find(|x| x.sref == 0)
                .ok_or_else(|| invalid("full payload has no root values"))?
                .clone(),
        ],
    };
    canonical(&output.join("root-canonical.bin"), &root)?;
    let root_identity = file_hash(&output.join("root-canonical.bin"))?;
    if file_hash(input)? != before {
        return Err(invalid("input changed during measurement").into());
    }
    let report = json!({
        "schema": "r1.sol-codec-sample/v1", "status": "completed",
        "format_version": SOL_FORMAT_VERSION, "operation": operation,
        "iterations": iterations, "solve_iterations": payload.meta.iterations,
        "input": {"bytes": before.0, "blake3": before.1},
        "metadata": metadata(&payload, stored_nodes),
        "decoded_strategy_blocks": payload.blocks.len(), "decoded_value_blocks": payload.values.len(),
        "raw_strategy_bytes": payload.blocks.iter().map(|x| x.probs.len() as u64).sum::<u64>(),
        "raw_value_bytes": payload.values.iter().map(|x| x.values.len() as u64).sum::<u64>(),
        "selected_srefs": payload.blocks.iter().map(|x| x.sref).collect::<Vec<_>>(),
        "canonical": {"file": "canonical.bin", "bytes": canonical_identity.0, "blake3": canonical_identity.1},
        "root_canonical": {"file": "root-canonical.bin", "bytes": root_identity.0, "blake3": root_identity.1},
        "rewritten": written,
        "timing": {"preparation_load_seconds": preparation_seconds, "open_seconds": open_seconds,
            "operation_seconds": elapsed, "operation_seconds_per_iteration": elapsed / f64::from(iterations),
            "validation_output_seconds": validation_start.elapsed().as_secs_f64()},
        "scope": "Codec bytes only; cached solve metrics are not recomputed quality values. Writer streams bounded groups from a fully decoded resident payload. Repeated root reads use one reader and re-decode the same chunk."
    });
    let mut result = File::create_new(output.join("result.json"))?;
    serde_json::to_writer_pretty(&mut result, &report)?;
    result.write_all(b"\n")?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
