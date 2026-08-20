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
            &resolve_checkpoint(config_path)?,
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
    let mut config: SolveConfig = toml::from_str(raw).context("parsing config")?;
    let config_hash = formats::config_hash(&raw_bytes);
    apply_legacy_multiway_max_sweeps(&mut config, max_sweeps)?;

    if matches!(config.game, GameSection::PreflopMultiway(_)) {
        return Err(anyhow!(
            "MWP003: legacy preflop-multiway checkpoints cannot be resumed; historical \
             solutions remain readable, but the rollout/full-recall paths they were produced \
             with no longer exist"
        ));
    }

    if matches!(config.game, GameSection::PreflopMultiway(_)) {
        println!(
            "resuming research multiway checkpoint to target={}",
            config.run.sweeps.unwrap_or(config.run.iterations)
        );
        return crate::multiway_solve::resume(
            raw,
            config,
            output,
            metrics,
            checkpoint_path,
            config_hash,
            None,
            Some(&crate::CLI_CANCEL),
            false,
            true,
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

fn apply_legacy_multiway_max_sweeps(
    config: &mut SolveConfig,
    max_sweeps: Option<u64>,
) -> Result<()> {
    let Some(max_sweeps) = max_sweeps else {
        return Ok(());
    };
    if max_sweeps == 0 {
        return Err(anyhow!("--max-sweeps must be positive"));
    }
    if !matches!(config.game, GameSection::PreflopMultiway(_)) {
        return Err(anyhow!(
            "--max-sweeps is available only for Multiway Preflop checkpoints"
        ));
    }
    config.run.sweeps = Some(max_sweeps);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
/// Accepts either a run directory or a checkpoint file.
///
/// A run directory is the documented input -- it is what `solve --out`
/// produces and what `status`/`watch` read -- so pointing `resume` at one is
/// the normal case. A bare `.mwckpt` still works for a checkpoint that was
/// moved out of its directory.
fn resolve_checkpoint(path: &Path) -> Result<std::path::PathBuf> {
    if !path.is_dir() {
        return Ok(path.to_path_buf());
    }
    let checkpoint = path.join(formats::RUN_CHECKPOINT_FILE);
    if !checkpoint.is_file() {
        return Err(anyhow!(
            "run directory {} has no {}; it never reached its first checkpoint",
            path.display(),
            formats::RUN_CHECKPOINT_FILE
        ));
    }
    Ok(checkpoint)
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
    let mut checkpoint = multiway::checkpoint::MultiwayCheckpoint::load_unchecked(checkpoint_path)
        .with_context(|| format!("reading {}", checkpoint_path.display()))?;
    let raw = checkpoint.config_toml.take().ok_or_else(|| {
        anyhow!("checkpoint is not self-contained; pass its config and --checkpoint")
    })?;
    // `multiway_solve::resume` reloads the checkpoint after lowering the
    // embedded config. Drop this first decoded state before that second load
    // and before the full dense arena is page-committed; otherwise a
    // self-contained resume retains two complete checkpoint states at the
    // preallocation peak.
    drop(checkpoint);
    if !crate::multiway_v1::has_v1_schema(&raw)? {
        return Err(anyhow!(
            "self-contained resume requires a Multiway Preflop v1 checkpoint"
        ));
    }
    let (_, historical_noop_pruning) = crate::config::solution_artifact_compatible_config(&raw)?;
    if historical_noop_pruning {
        return Err(anyhow!(
            "this checkpoint uses the historical bucket-history + regret-based pruning \
             combination whose pruning bit was a no-op; solution artifacts remain readable, \
             but resuming this checkpoint requires a future explicit offline migration"
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
    let paths = crate::run_dir::RunPaths::new(directory);
    let config_hash = formats::config_hash(raw.as_bytes());
    let mut recorder = crate::run_dir::RunRecorder::reopen(
        directory,
        "preflop-multiway",
        Some(crate::multiway_v1::SCHEMA.to_string()),
        config_hash,
        &raw,
        vec!["resume".to_string(), directory.display().to_string()],
    )?;
    let outcome = crate::multiway_solve::resume_observed(
        &raw,
        config,
        Some(&paths.result),
        Some(&paths.progress),
        active_checkpoint,
        config_hash,
        Some(&paths.solution),
        Some(&crate::CLI_CANCEL),
        stop_target.is_some(),
        true,
        &mut |observation| recorder.observe(&observation),
    );
    let completion = crate::run_dir::completion_status(directory);
    recorder.finish(outcome, completion)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTIWAY: &str = crate::test_fixtures::LOWERED_3MAX;

    #[test]
    fn legacy_multiway_max_sweeps_is_a_positive_total_target() {
        let mut config: SolveConfig = toml::from_str(MULTIWAY).unwrap();
        apply_legacy_multiway_max_sweeps(&mut config, Some(2_000)).unwrap();
        assert_eq!(config.run.sweeps, Some(2_000));
        assert!(
            apply_legacy_multiway_max_sweeps(&mut config, Some(0))
                .unwrap_err()
                .to_string()
                .contains("must be positive")
        );
    }

    #[test]
    fn legacy_non_multiway_rejects_max_sweeps() {
        let mut config: SolveConfig = toml::from_str(
            r#"
[game]
kind = "kuhn"

[run]
iterations = 1
"#,
        )
        .unwrap();
        assert!(
            apply_legacy_multiway_max_sweeps(&mut config, Some(2))
                .unwrap_err()
                .to_string()
                .contains("only for Multiway Preflop")
        );
    }
}
