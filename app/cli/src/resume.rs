//! `resume`: continue a checkpointed solve to `run.iterations` TOTAL
//! iterations, using the exact same config that produced the checkpoint
//! (verified by blake3 hash of the config file's raw bytes) and the exact
//! same deterministic game-build path as `solve`, so the result is
//! bit-for-bit identical to an uninterrupted straight solve.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use engine::{F32Storage, I16Storage};

use crate::config::{GameSection, SolveConfig, StorageKind};
use crate::solve::run_with_storage;

#[allow(clippy::too_many_arguments)]
pub fn run(
    config_path: &Path,
    checkpoint_path: Option<&Path>,
    out: Option<&Path>,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    output: Option<&Path>,
    max_sweeps: Option<u64>,
    stop_target: Option<f64>,
    evaluation_samples: Option<u64>,
    evaluation_cadence: Option<u64>,
    checkpoint_interval: Option<&str>,
    histories: &[String],
    metrics: Option<&Path>,
) -> Result<()> {
    let Some(checkpoint_path) = checkpoint_path else {
        return run_self_contained_multiway(
            config_path,
            out,
            threads,
            memory,
            max_time,
            output,
            max_sweeps,
            stop_target,
            evaluation_samples,
            evaluation_cadence,
            checkpoint_interval,
            histories,
            metrics,
        );
    };
    if out.is_some()
        || threads.is_some()
        || memory.is_some()
        || max_time.is_some()
        || max_sweeps.is_some()
        || stop_target.is_some()
        || evaluation_samples.is_some()
        || evaluation_cadence.is_some()
        || checkpoint_interval.is_some()
    {
        return Err(anyhow!(
            "resume overrides are available only for self-contained v1 checkpoints"
        ));
    }
    let raw_bytes =
        std::fs::read(config_path).with_context(|| format!("reading {}", config_path.display()))?;
    let raw = std::str::from_utf8(&raw_bytes).context("config file is not valid UTF-8")?;
    let config: SolveConfig = toml::from_str(raw).context("parsing config")?;
    let config_hash = formats::config_hash(&raw_bytes);

    if matches!(config.game, GameSection::PreflopMultiway(_)) {
        if histories.iter().any(|history| !history.is_empty()) {
            return Err(anyhow!(
                "multiway resume does not export --history selections; use the .mwsol strategy query"
            ));
        }
        return crate::multiway_solve::resume(
            raw,
            config,
            output,
            metrics,
            checkpoint_path,
            config_hash,
            None,
            Some(&crate::CLI_CANCEL),
            true,
            false,
        );
    }

    let checkpoint = formats::read_checkpoint(checkpoint_path)
        .with_context(|| format!("reading checkpoint {}", checkpoint_path.display()))?;
    if checkpoint.config_hash != config_hash {
        return Err(anyhow!(
            "checkpoint {} does not match {}: config hash {} != {} \
             (the checkpoint was produced from a different config; refusing to resume)",
            checkpoint_path.display(),
            config_path.display(),
            formats::config_hash_hex(&checkpoint.config_hash),
            formats::config_hash_hex(&config_hash),
        ));
    }

    println!(
        "resuming from checkpoint: iteration={} target={}",
        checkpoint.iteration, config.run.iterations
    );

    let checkpoint_sink = Some((checkpoint_path, config_hash));
    // A storage-backend mismatch (e.g. the config now says `storage =
    // "i16"` but the checkpoint holds f32 state) surfaces naturally as a
    // `StateMismatch` from `Solver::restore_state` inside `run_with_storage`
    // -- no separate check needed here.
    let result = match config.run.storage {
        StorageKind::F32 => run_with_storage::<F32Storage>(
            config,
            output,
            histories,
            metrics,
            checkpoint_sink,
            Some(checkpoint.state),
        ),
        StorageKind::I16 => run_with_storage::<I16Storage>(
            config,
            output,
            histories,
            metrics,
            checkpoint_sink,
            Some(checkpoint.state),
        ),
    };
    result.map(|_summary| ())
}

#[allow(clippy::too_many_arguments)]
fn run_self_contained_multiway(
    checkpoint_path: &Path,
    out: Option<&Path>,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    output: Option<&Path>,
    max_sweeps: Option<u64>,
    stop_target: Option<f64>,
    evaluation_samples: Option<u64>,
    evaluation_cadence: Option<u64>,
    checkpoint_interval: Option<&str>,
    histories: &[String],
    metrics: Option<&Path>,
) -> Result<()> {
    if output.is_some() || metrics.is_some() {
        return Err(anyhow!(
            "Multiway Preflop v1 resume uses its checkpoint run directory"
        ));
    }
    if histories.iter().any(|history| !history.is_empty()) {
        return Err(anyhow!(
            "Multiway Preflop v1 resume does not accept --history"
        ));
    }
    let checkpoint = multiway::checkpoint::MultiwayCheckpoint::load_unchecked(checkpoint_path)
        .with_context(|| format!("reading {}", checkpoint_path.display()))?;
    let raw = checkpoint.config_toml.ok_or_else(|| {
        anyhow!("checkpoint is not self-contained; pass its config and --checkpoint")
    })?;
    if !crate::multiway_v1::has_v1_schema(&raw)? {
        return Err(anyhow!(
            "self-contained resume requires a Multiway Preflop v1 checkpoint"
        ));
    }
    let raw = crate::multiway_v1::apply_resume_overrides(
        &raw,
        threads,
        memory,
        max_time,
        max_sweeps,
        stop_target,
        evaluation_samples,
        evaluation_cadence,
        checkpoint_interval,
    )?;
    let config = crate::config::parse_solve_config(&raw)?;
    let default_directory = checkpoint_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = out.unwrap_or(default_directory);
    if out.is_some() {
        if directory.exists() {
            if std::fs::read_dir(directory)?.next().is_some() {
                return Err(anyhow!("resume --out directory must be empty"));
            }
        } else {
            std::fs::create_dir_all(directory)
                .with_context(|| format!("creating {}", directory.display()))?;
        }
    }
    let fork_checkpoint = directory.join("checkpoint.mwckpt");
    if out.is_some() {
        std::fs::copy(checkpoint_path, &fork_checkpoint).with_context(|| {
            format!(
                "copying checkpoint {} to {}",
                checkpoint_path.display(),
                fork_checkpoint.display()
            )
        })?;
    }
    let active_checkpoint = if out.is_some() {
        fork_checkpoint.as_path()
    } else {
        checkpoint_path
    };
    let run_path = directory.join("run.json");
    let progress_path = directory.join("progress.jsonl");
    let solution_path = directory.join("solution.mwsol");
    let config_hash = formats::config_hash(raw.as_bytes());
    crate::multiway_solve::resume(
        &raw,
        config,
        Some(&run_path),
        Some(&progress_path),
        active_checkpoint,
        config_hash,
        Some(&solution_path),
        Some(&crate::CLI_CANCEL),
        true,
        stop_target.is_some(),
    )
}
