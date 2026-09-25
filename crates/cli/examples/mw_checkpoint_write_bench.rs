//! Compare owned capture and borrowed checkpoint writing from the same state.

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use multiway::MultiwayCheckpoint;
use serde::Serialize;

#[derive(Clone, Copy, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Owned,
    Borrowed,
}

#[derive(Parser)]
struct Args {
    /// Production Multiway v1 configuration.
    #[arg(long)]
    config: PathBuf,
    /// Restore a retained checkpoint; no training occurs in this mode.
    #[arg(long, conflicts_with = "sweeps", required_unless_present = "sweeps")]
    checkpoint: Option<PathBuf>,
    /// Fresh training budget, separate from the checkpoint-writing timer.
    #[arg(long, required_unless_present = "checkpoint")]
    sweeps: Option<u64>,
    /// A new output path; existing files are rejected by this benchmark.
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

fn unix_millis() -> Result<u128> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
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
    if let Some(sweeps) = args.sweeps {
        ensure!(sweeps > 0, "fresh sweeps must be positive");
    }
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
    let mut session =
        cli::session::build_production_multiway_session(&effective, args.checkpoint.as_deref())
            .context("constructing checkpoint-writing benchmark solver")?;
    let construction_seconds = started.elapsed().as_secs_f64();
    let training_seconds = if let Some(sweeps) = args.sweeps {
        let started = Instant::now();
        session
            .solver
            .run_sweeps_with_threads(sweeps, session.threads)?;
        ensure!(
            session.solver.completed_sweeps() == sweeps,
            "incomplete training budget"
        );
        Some(started.elapsed().as_secs_f64())
    } else {
        None
    };
    let metrics = session.solver.metrics();
    // Both modes use identical persisted metadata. Do not inject elapsed
    // benchmark time into the checkpoint being compared byte-for-byte.
    let runtime = session.checkpoint_runtime.unwrap_or_default();
    let write_started_unix_ms = unix_millis()?;
    let started = Instant::now();
    match args.mode {
        Mode::Owned => MultiwayCheckpoint::capture(&session.solver)
            .with_runtime_metadata(&effective, runtime)
            .write_atomic(&args.output)?,
        Mode::Borrowed => MultiwayCheckpoint::write_solver_atomic(
            &session.solver,
            &args.output,
            &effective,
            runtime,
        )?,
    }
    // Owned mode includes capture and temporary DTO disposal. Borrowed mode
    // includes index preparation and disposal. Compression/fsync are common.
    let write_seconds = started.elapsed().as_secs_f64();
    let write_finished_unix_ms = unix_millis()?;
    let output_blake3 = digest_file(&args.output)?;
    println!(
        "{}",
        serde_json::json!({
            "schemaVersion": "solvers.multiway-checkpoint-write-bench/v1",
            "sourceRevision": args.source_revision,
            "executableBlake3": digest_file(&std::env::current_exe()?)?,
            "mode": args.mode,
            "config": args.config,
            "inputCheckpoint": args.checkpoint,
            "freshSweeps": args.sweeps,
            "output": args.output,
            "threads": session.threads,
            "effectiveConfigBlake3": blake3::hash(effective.as_bytes()).to_hex().to_string(),
            "configurationFingerprint": session.solver.configuration_fingerprint(),
            "abstractionFingerprint": session.solver.abstraction_fingerprint(),
            "runtime": runtime,
            "metrics": metrics,
            "constructionSeconds": construction_seconds,
            "trainingSeconds": training_seconds,
            "writeSeconds": write_seconds,
            "writeStartedUnixMs": write_started_unix_ms,
            "writeFinishedUnixMs": write_finished_unix_ms,
            "outputBytes": fs::metadata(&args.output)?.len(),
            "outputBlake3": output_blake3,
            "interpretation": "Storage benchmark only. The write timer includes preparation, compression, fsync, atomic persist and temporary snapshot/index disposal. Output hashing and final solver disposal are outside it. No equilibrium or learning-speed claim."
        })
    );
    Ok(())
}
