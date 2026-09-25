//! Load-only checkpoint benchmark. Does not allocate a solver or train it.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use multiway::MultiwayCheckpoint;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    checkpoint: PathBuf,
}

#[derive(Default)]
struct DigestWriter(blake3::Hasher);

impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let started = Instant::now();
    let checkpoint = MultiwayCheckpoint::load_unchecked(&args.checkpoint)?;
    let load_secs = started.elapsed().as_secs_f64();
    // Hash the complete decoded state/config/runtime with bounded staging.
    // This is outside the load timer; it is not a solver fingerprint.
    let mut digest = DigestWriter::default();
    serde_json::to_writer(&mut digest, &checkpoint)?;
    println!(
        "{}",
        serde_json::json!({
            "schemaVersion": "solvers.multiway-checkpoint-load-bench/v1",
            "checkpoint": args.checkpoint,
            "loadSecs": load_secs,
            "uncompressedBytes": checkpoint.header.uncompressed_len,
            "policyCount": checkpoint.state.policies.len(),
            "sweeps": checkpoint.state.completed_sweeps,
            "decodedCheckpointJsonBlake3": digest.0.finalize().to_hex().as_str(),
        })
    );
    Ok(())
}
