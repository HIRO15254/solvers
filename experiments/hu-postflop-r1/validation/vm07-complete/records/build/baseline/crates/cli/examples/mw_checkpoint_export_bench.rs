//! Measure the non-training cost of restoring a production multiway
//! checkpoint and exporting its `.mwsol` artifact.
//!
//! This example deliberately performs no evaluation or training.  It builds a
//! fresh production session once to measure startup, drops it, restores the
//! checkpoint in a second session, and then times snapshot construction,
//! solution materialization, and the format writer independently.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use blake3::Hasher;
use clap::Parser;
use formats::{MwsolStorage, write_mwsol_with};
use multiway::PolicyArenaAllocation;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "mw_checkpoint_export_bench")]
struct Args {
    /// Original v1 config used to create the checkpoint.
    #[arg(long)]
    config: PathBuf,

    /// Atomic `.mwckpt` snapshot to restore without training.
    #[arg(long)]
    checkpoint: PathBuf,

    /// New output path for the measured `.mwsol` write.  It must not exist.
    #[arg(long)]
    output: PathBuf,

    /// Override threads used by production session construction.
    #[arg(long)]
    threads: Option<usize>,

    /// Override the policy arena budget, for example `48GiB`.
    #[arg(long)]
    memory: Option<String>,

    /// Machine-local EHS2 cache directory.
    #[arg(long)]
    cache_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchOutput {
    schema_version: &'static str,
    config: String,
    checkpoint: String,
    output: String,
    storage: &'static str,
    startup_secs: f64,
    restore_secs: f64,
    snapshot_secs: f64,
    make_solution_secs: f64,
    write_secs: f64,
    verification_secs: f64,
    artifact_bytes: u64,
    artifact_blake3: String,
    solver_state_version: u16,
    sweeps: u64,
    snapshot_histories: usize,
    snapshot_policies: usize,
    histories: usize,
    public_nodes: usize,
    strategy_blocks: usize,
    strategy_weights: usize,
    policy_arena: Option<PolicyArenaAllocation>,
    game_fingerprint: String,
    abstraction_fingerprint: String,
    configuration_fingerprint: String,
    metadata_verified: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;
    if let Some(cache_dir) = args.cache_dir.as_deref() {
        cli::cache::set_root_override(cache_dir);
    }

    let raw_config = fs::read_to_string(&args.config)
        .with_context(|| format!("reading {}", args.config.display()))?;
    let effective_config = cli::multiway_v1::apply_solve_overrides(
        &raw_config,
        args.threads,
        args.memory.as_deref(),
        None,
        Some(&args.config),
    )?;

    // The production builder combines game/EHS setup, public-tree preflight,
    // arena allocation, and optional checkpoint restore.  Two sequential
    // builder calls are intentional: they expose the startup and restore
    // components while keeping only one large arena resident at a time.
    let startup_started = Instant::now();
    let startup_session = cli::session::build_production_multiway_session(&effective_config, None)
        .context("building a clean production session for startup timing")?;
    let startup_secs = startup_started.elapsed().as_secs_f64();
    let startup_arena = startup_session.solver.policy_arena_allocation();
    drop(startup_session);
    eprintln!("startup session: {startup_secs:.3}s");

    let restore_started = Instant::now();
    let session =
        cli::session::build_production_multiway_session(&effective_config, Some(&args.checkpoint))
            .with_context(|| format!("restoring {}", args.checkpoint.display()))?;
    let restore_secs = restore_started.elapsed().as_secs_f64();
    eprintln!("checkpoint restore: {restore_secs:.3}s");

    let solver = &session.solver;
    let snapshot_started = Instant::now();
    let snapshot = solver.snapshot_state();
    let snapshot_secs = snapshot_started.elapsed().as_secs_f64();
    eprintln!("snapshot_state: {snapshot_secs:.3}s");

    let metrics = solver.metrics();
    let elapsed = session.checkpoint_runtime.map_or(0.0, |runtime| {
        runtime.cumulative_solve_millis as f64 / 1000.0
    });
    let mut row = cli::session::metrics_row(
        &metrics,
        vec![0.0; session.game_config.seats.len()],
        elapsed,
        None,
    );
    // A frozen checkpoint has no live stop-rule observation.  Use the
    // accepted persisted-solution status that describes a bounded snapshot,
    // rather than the transient `sampling` phase from `metrics_row`.
    row.phase = "sweep-limit".to_owned();
    let make_solution_started = Instant::now();
    let solution = cli::session::make_solution(
        &session.config_toml,
        solver.abstraction_fingerprint(),
        solver.configuration_fingerprint(),
        solver.game(),
        &snapshot,
        &row,
    );
    let make_solution_secs = make_solution_started.elapsed().as_secs_f64();
    eprintln!("make_solution: {make_solution_secs:.3}s");

    let encoding = cli::multiway_v1::probability_encoding(&effective_config)?;
    let (storage, storage_name) = match encoding {
        cli::multiway_v1::ProbabilityEncoding::U16 => (MwsolStorage::U16, "u16"),
        cli::multiway_v1::ProbabilityEncoding::F32 => (MwsolStorage::F32, "f32"),
    };
    let write_started = Instant::now();
    write_mwsol_with(&args.output, &solution, storage)
        .with_context(|| format!("writing {}", args.output.display()))?;
    let write_secs = write_started.elapsed().as_secs_f64();
    eprintln!("write_mwsol: {write_secs:.3}s");

    let verification_started = Instant::now();
    let (artifact_bytes, artifact_blake3) = hash_file(&args.output)?;
    let metadata_verified = verify_artifact(&args.output, &solution)?;
    let verification_secs = verification_started.elapsed().as_secs_f64();
    eprintln!("artifact verification: {verification_secs:.3}s");

    // `startup_arena` is retained only as a sanity check that the clean and
    // restored builders selected the same arena sizing contract.
    let restored_arena = solver.policy_arena_allocation();
    if startup_arena != restored_arena {
        bail!("startup and restored policy-arena allocations differ");
    }
    let output = BenchOutput {
        schema_version: "solvers.multiway-checkpoint-export-bench/v1",
        config: args.config.display().to_string(),
        checkpoint: args.checkpoint.display().to_string(),
        output: args.output.display().to_string(),
        storage: storage_name,
        startup_secs,
        restore_secs,
        snapshot_secs,
        make_solution_secs,
        write_secs,
        verification_secs,
        artifact_bytes,
        artifact_blake3,
        solver_state_version: multiway::solver::SOLVER_STATE_VERSION,
        sweeps: solver.completed_sweeps(),
        snapshot_histories: snapshot.histories.len(),
        snapshot_policies: snapshot.policies.len(),
        histories: solution.histories.len(),
        public_nodes: solution.public_states.len(),
        strategy_blocks: solution.strategies.len(),
        strategy_weights: solution.strategy_weights.len(),
        policy_arena: restored_arena,
        game_fingerprint: hex(solution.game_fingerprint),
        abstraction_fingerprint: hex(solution.abstraction_fingerprint),
        configuration_fingerprint: hex(solution.configuration_fingerprint),
        metadata_verified,
    };
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, &output)?;
    handle.write_all(b"\n")?;
    handle.flush()?;
    Ok(())
}

fn validate_args(args: &Args) -> Result<()> {
    if args.threads == Some(0) {
        bail!("--threads must be positive");
    }
    if !args.config.is_file() {
        bail!("--config is not a regular file: {}", args.config.display());
    }
    if !args.checkpoint.is_file() {
        bail!(
            "--checkpoint is not a regular file: {}",
            args.checkpoint.display()
        );
    }
    if args.output.exists() {
        bail!("--output already exists; choose a fresh path");
    }
    if let Some(parent) = args.output.parent()
        && !parent.as_os_str().is_empty()
        && !parent.is_dir()
    {
        bail!("output parent is not a directory: {}", parent.display());
    }
    if same_existing_path(&args.output, &args.checkpoint)? {
        bail!("--output must differ from --checkpoint");
    }
    Ok(())
}

fn same_existing_path(left: &std::path::Path, right: &std::path::Path) -> Result<bool> {
    if !right.exists() || !left.exists() {
        return Ok(false);
    }
    Ok(fs::canonicalize(left)? == fs::canonicalize(right)?)
}

fn hash_file(path: &std::path::Path) -> Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hasher = Hasher::new();
    let mut bytes = 0u64;
    // Keep the buffer off the main thread's small default stack.
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes = bytes
            .checked_add(read as u64)
            .context("artifact size overflow")?;
    }
    Ok((bytes, hasher.finalize().to_hex().to_string()))
}

fn verify_artifact(path: &std::path::Path, solution: &formats::MultiwaySolution) -> Result<bool> {
    let expected_strategy_count = solution.strategies.len();
    let expected_game = solution.game_fingerprint;
    let expected_abstraction = solution.abstraction_fingerprint;
    let expected_configuration = solution.configuration_fingerprint;
    let expected_histories = solution.histories.len();
    let expected_public_states = solution.public_states.len();
    let expected_weights = solution.strategy_weights.len();
    let mut reader = formats::MwSolReader::open(path)
        .with_context(|| format!("reopening {} for metadata verification", path.display()))?;
    let metadata = reader.metadata();
    let metadata_verified = reader.strategy_count() == expected_strategy_count
        && metadata.game_fingerprint == expected_game
        && metadata.abstraction_fingerprint == expected_abstraction
        && metadata.configuration_fingerprint == expected_configuration
        && metadata.histories.len() == expected_histories
        && metadata.public_states.len() == expected_public_states
        && metadata.strategy_weights.len() == expected_weights;
    if !metadata_verified {
        bail!("written artifact metadata does not match the in-memory solution");
    }
    // Decode boundary frames as a cheap payload check without turning this
    // diagnostic into a full pass over the strategy section.
    if expected_strategy_count > 0 {
        reader.read_strategy_page(0, 1)?;
        reader.read_strategy_page(expected_strategy_count - 1, 1)?;
    }
    Ok(true)
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn rejects_zero_threads_before_building_a_session() {
        let args = Args::try_parse_from([
            "mw_checkpoint_export_bench",
            "--config",
            "missing.toml",
            "--checkpoint",
            "missing.mwckpt",
            "--output",
            "new.mwsol",
            "--threads",
            "0",
        ])
        .unwrap();
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn clap_requires_an_explicit_new_output() {
        assert!(
            Args::try_parse_from([
                "mw_checkpoint_export_bench",
                "--config",
                "config.toml",
                "--checkpoint",
                "checkpoint.mwckpt",
            ])
            .is_err()
        );
    }

    #[test]
    fn hash_file_works_with_a_small_thread_stack() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.bin");
        let contents = vec![0x5au8; 2 * 1024 * 1024];
        fs::write(&path, &contents).unwrap();
        let expected = blake3::hash(&contents).to_hex().to_string();
        let path_for_thread = path.clone();
        let result = thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || hash_file(&path_for_thread))
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(result, (contents.len() as u64, expected));
    }
}
