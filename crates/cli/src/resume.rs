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
    run_directory: &Path,
    out: Option<&Path>,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    max_sweeps: Option<u64>,
    stop_target: Option<f64>,
    evaluation_samples: Option<u64>,
    evaluation_cadence: Option<u64>,
    checkpoint_interval: Option<&str>,
    histories: &[String],
) -> Result<()> {
    let checkpoint = resolve_checkpoint(run_directory)?;
    if checkpoint
        .extension()
        .is_some_and(|extension| extension == "mwckpt")
    {
        return run_self_contained_multiway(
            &checkpoint,
            out,
            threads,
            memory,
            max_time,
            max_sweeps,
            stop_target,
            evaluation_samples,
            evaluation_cadence,
            checkpoint_interval,
        );
    }
    if threads.is_some()
        || memory.is_some()
        || max_time.is_some()
        || max_sweeps.is_some()
        || stop_target.is_some()
        || evaluation_samples.is_some()
        || evaluation_cadence.is_some()
        || checkpoint_interval.is_some()
    {
        return Err(anyhow!(
            "these resume overrides apply to Multiway Preflop v1 runs only"
        ));
    }
    resume_heads_up(run_directory, &checkpoint, out, histories)
}

/// Continues a heads-up, postflop, or toy run from its run directory.
///
/// The directory is self-describing: `run.toml` is the config the run used,
/// and its blake3 hash is what the checkpoint was stamped with, so the two
/// are verified against each other exactly as before.
fn resume_heads_up(
    run_directory: &Path,
    checkpoint_path: &Path,
    out: Option<&Path>,
    histories: &[String],
) -> Result<()> {
    let config_file = run_directory.join(formats::RUN_CONFIG_FILE);
    let raw_bytes = std::fs::read(&config_file)
        .with_context(|| format!("reading {}", config_file.display()))?;
    let raw = std::str::from_utf8(&raw_bytes).context("run config is not valid UTF-8")?;
    let config: SolveConfig =
        crate::config::parse_solve_config(raw).context("parsing the run config")?;
    let config_hash = formats::config_hash(&raw_bytes);

    if matches!(config.game, GameSection::PreflopMultiway(_)) {
        return Err(anyhow!(
            "MWP003: legacy preflop-multiway checkpoints cannot be resumed; historical \
             solutions remain readable, but the rollout/full-recall paths they were produced \
             with no longer exist"
        ));
    }

    let checkpoint = formats::read_checkpoint(checkpoint_path)
        .with_context(|| format!("reading checkpoint {}", checkpoint_path.display()))?;
    if checkpoint.config_hash != config_hash {
        return Err(anyhow!(
            "checkpoint {} does not match {}: config hash {} != {} \
             (the checkpoint was produced from a different config; refusing to resume)",
            checkpoint_path.display(),
            config_file.display(),
            formats::config_hash_hex(&checkpoint.config_hash),
            formats::config_hash_hex(&config_hash),
        ));
    }
    println!(
        "resuming from checkpoint: iteration={} target={}",
        checkpoint.iteration, config.run.iterations
    );

    // A fork copies the run into a fresh directory and continues there, so
    // the original stays exactly as it was left.
    let directory = match out {
        Some(fork) => {
            crate::run_dir::create_empty(fork)?;
            std::fs::write(fork.join(formats::RUN_CONFIG_FILE), raw)?;
            std::fs::copy(checkpoint_path, fork.join(formats::RUN_HU_CHECKPOINT_FILE))?;
            fork
        }
        None => run_directory,
    };
    let paths = crate::run_dir::RunPaths::heads_up(directory);
    let mut recorder = crate::run_dir::RunRecorder::reopen(
        directory,
        crate::solve::game_kind_name(&config.game),
        config.schema.clone(),
        config_hash,
        raw,
        vec!["resume".to_string(), directory.display().to_string()],
    )?;
    let checkpoint_sink = Some((paths.checkpoint.as_path(), config_hash));
    // A storage-backend mismatch (e.g. the config now says `storage =
    // "i16"` but the checkpoint holds f32 state) surfaces naturally as a
    // `StateMismatch` from `Solver::restore_state` inside `run_with_storage`
    // -- no separate check needed here.
    let outcome = match config.run.storage {
        StorageKind::F32 => run_with_storage::<F32Storage>(
            config,
            Some(&paths.strategy),
            histories,
            Some(&paths.progress),
            checkpoint_sink,
            Some(checkpoint.state),
            Some(recorder.events_mut()),
            Some(&crate::CLI_CANCEL),
        ),
        StorageKind::I16 => run_with_storage::<I16Storage>(
            config,
            Some(&paths.strategy),
            histories,
            Some(&paths.progress),
            checkpoint_sink,
            Some(checkpoint.state),
            Some(recorder.events_mut()),
            Some(&crate::CLI_CANCEL),
        ),
    };
    let completion = outcome.as_ref().ok().map(|summary| {
        if summary.canceled {
            "cancelled"
        } else {
            "completed"
        }
        .to_string()
    });
    recorder.finish(outcome.map(|_summary| ()), completion)
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
    // Which checkpoint a run left behind identifies its engine, so the
    // caller routes on the extension rather than re-parsing the config.
    for name in [
        formats::RUN_CHECKPOINT_FILE,
        formats::RUN_HU_CHECKPOINT_FILE,
    ] {
        let checkpoint = path.join(name);
        if checkpoint.is_file() {
            return Ok(checkpoint);
        }
    }
    Err(anyhow!(
        "run directory {} has no checkpoint; it never reached its first one",
        path.display()
    ))
}

#[allow(clippy::too_many_arguments)]
fn run_self_contained_multiway(
    checkpoint_path: &Path,
    out: Option<&Path>,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    max_sweeps: Option<u64>,
    stop_target: Option<f64>,
    evaluation_samples: Option<u64>,
    evaluation_cadence: Option<u64>,
    checkpoint_interval: Option<&str>,
) -> Result<()> {
    let mut checkpoint = multiway::checkpoint::MultiwayCheckpoint::load_unchecked(checkpoint_path)
        .with_context(|| format!("reading {}", checkpoint_path.display()))?;
    let raw = checkpoint.config_toml.take().ok_or_else(|| {
        anyhow!("checkpoint is not self-contained: it carries no config to resume from")
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
    let paths = crate::run_dir::RunPaths::multiway(directory);
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

    /// `resume` takes a run directory. Pointing it at one that never
    /// checkpointed must say so rather than failing deeper in.
    #[test]
    fn a_run_directory_without_a_checkpoint_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let error = resolve_checkpoint(directory.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("never reached its first one"), "{error}");
    }

    /// A checkpoint file that was moved out of its run directory is still
    /// accepted directly, since a self-contained `.mwckpt` carries its config.
    #[test]
    fn a_checkpoint_file_path_is_passed_through() {
        let directory = tempfile::tempdir().unwrap();
        let checkpoint = directory.path().join("moved.mwckpt");
        std::fs::write(&checkpoint, b"x").unwrap();
        assert_eq!(resolve_checkpoint(&checkpoint).unwrap(), checkpoint);
    }

    /// The multiway branch is chosen by the checkpoint's extension, so a
    /// heads-up run directory must not route into it.
    #[test]
    fn a_heads_up_run_directory_resolves_to_its_ckpt() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(formats::RUN_HU_CHECKPOINT_FILE), b"x").unwrap();
        let resolved = resolve_checkpoint(directory.path()).unwrap();
        assert_eq!(resolved.extension().unwrap(), "ckpt");
    }
}
