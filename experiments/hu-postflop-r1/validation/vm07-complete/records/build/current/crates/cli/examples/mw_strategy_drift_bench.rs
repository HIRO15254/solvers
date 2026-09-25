//! Measure retained previous-profile storage and refresh from one checkpoint.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use multiway::MultiwayCheckpoint;
use multiway::solver::{InfoKey, StrategyDriftTracker};
use serde::Serialize;

#[derive(Clone, Copy, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Legacy,
    Compact,
}

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    checkpoint: PathBuf,
    /// New checkpoint path, written after dropping the drift tracker.
    #[arg(long)]
    output: PathBuf,
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long, default_value_t = 8)]
    threads: usize,
    #[arg(long)]
    memory: String,
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    #[arg(long)]
    source_revision: String,
}

fn digest_file(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 65_536];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(hasher.finalize().to_hex().to_string());
        }
        hasher.update(&buffer[..count]);
    }
}

fn bits(values: Vec<f64>) -> Vec<u64> {
    values.into_iter().map(f64::to_bits).collect()
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        (1..=64).contains(&args.threads),
        "threads must be in 1..=64"
    );
    ensure!(
        !args.source_revision.trim().is_empty(),
        "source revision is required"
    );
    ensure!(!args.output.exists(), "benchmark output must be a new path");
    if let Some(cache) = &args.cache_dir {
        cli::cache::set_root_override(cache);
    }
    let raw = fs::read_to_string(&args.config)?;
    let effective = cli::multiway_v1::apply_solve_overrides(
        &raw,
        Some(args.threads),
        Some(&args.memory),
        None,
        Some(&args.config),
    )?;
    let started = Instant::now();
    let session =
        cli::session::build_production_multiway_session(&effective, Some(&args.checkpoint))
            .context("constructing strategy-drift benchmark solver")?;
    let construction_seconds = started.elapsed().as_secs_f64();
    let metrics = session.solver.metrics();
    let (first, stable, first_seconds, stable_seconds, columns, slots, payload_bytes) = match args
        .mode
    {
        Mode::Legacy => {
            let mut prior = HashMap::new();
            let started = Instant::now();
            let first = session.solver.strategy_drift_refresh(&mut prior);
            let first_seconds = started.elapsed().as_secs_f64();
            let first_columns = prior.len();
            let first_slots = prior.values().map(Vec::len).sum::<usize>();
            let started = Instant::now();
            let stable = session.solver.strategy_drift_refresh(&mut prior);
            let stable_seconds = started.elapsed().as_secs_f64();
            // Match the compact API's payload accounting. Control bytes and
            // allocator bookkeeping are omitted in both representations.
            let columns = prior.len();
            let slots = prior.values().map(Vec::len).sum::<usize>();
            ensure!(
                (columns, slots) == (first_columns, first_slots),
                "stable refresh changed stored shape"
            );
            let payload_bytes = prior.capacity() * (size_of::<InfoKey>() + size_of::<Vec<f32>>())
                + prior
                    .values()
                    .map(|p| p.capacity() * size_of::<f32>())
                    .sum::<usize>();
            (
                first,
                stable,
                first_seconds,
                stable_seconds,
                columns,
                slots,
                payload_bytes,
            )
        }
        Mode::Compact => {
            let mut prior = StrategyDriftTracker::new();
            let started = Instant::now();
            let first = session.solver.strategy_drift_refresh_compact(&mut prior)?;
            let first_seconds = started.elapsed().as_secs_f64();
            let first_shape = (prior.observed_columns(), prior.observed_action_slots());
            let started = Instant::now();
            let stable = session.solver.strategy_drift_refresh_compact(&mut prior)?;
            let stable_seconds = started.elapsed().as_secs_f64();
            ensure!(
                (prior.observed_columns(), prior.observed_action_slots()) == first_shape,
                "stable refresh changed stored shape"
            );
            (
                first,
                stable,
                first_seconds,
                stable_seconds,
                prior.observed_columns(),
                prior.observed_action_slots(),
                prior.retained_payload_bytes(),
            )
        }
    };
    // Trackers are dropped here, before the common checkpoint write, matching
    // production's release before final solution staging. Timers exclude drop,
    // payload accounting, checkpoint I/O and hashing.
    ensure!(
        first.len() == session.game_config.seats.len() && stable.len() == first.len(),
        "drift seat count"
    );
    ensure!(
        first.iter().chain(&stable).all(|&v| v.to_bits() == 0),
        "unchanged profile must have zero drift"
    );
    ensure!(
        columns as u64 == metrics.infosets,
        "drift must retain every stored column"
    );
    let runtime = session.checkpoint_runtime.unwrap_or_default();
    MultiwayCheckpoint::write_solver_atomic(&session.solver, &args.output, &effective, runtime)?;
    println!(
        "{}",
        serde_json::json!({
            "schemaVersion": "solvers.strategy-drift-bench/v1",
            "sourceRevision": args.source_revision,
            "executableBlake3": digest_file(&std::env::current_exe()?)?,
            "config": args.config,
            "inputCheckpoint": args.checkpoint,
            "output": args.output,
            "mode": args.mode,
            "threads": session.threads,
            "sweeps": session.solver.completed_sweeps(),
            "effectiveConfigBlake3": blake3::hash(effective.as_bytes()).to_hex().to_string(),
            "configurationFingerprint": session.solver.configuration_fingerprint(),
            "abstractionFingerprint": session.solver.abstraction_fingerprint(),
            "metrics": metrics,
            "runtime": runtime,
            "constructionElapsedSecs": construction_seconds,
            "firstRefreshSeconds": first_seconds,
            "stableRefreshSeconds": stable_seconds,
            "firstDriftBits": bits(first),
            "stableDriftBits": bits(stable),
            "observedColumns": columns,
            "observedActionSlots": slots,
            "retainedPayloadBytes": payload_bytes,
            "outputBytes": fs::metadata(&args.output)?.len(),
            "outputBlake3": digest_file(&args.output)?,
            "interpretation": "No training. First capture and stable refresh are separate timers; they exclude tracker disposal, payload accounting, checkpoint I/O and hashing. Retained payload is capacity-based and omits allocator/control-byte overhead. Whole-process peak also includes restore and common checkpoint write. No learning-speed or equilibrium claim."
        })
    );
    Ok(())
}
