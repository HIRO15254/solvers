//! Resume a self-contained common-input checkpoint or its run directory.
use anyhow::{Context, Result, anyhow};
use std::path::Path;

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
) -> Result<()> {
    // Run config is also part of the artifact contract: an old directory
    // cannot become a common-input run by replacing just its checkpoint.
    if run_directory.is_dir() {
        let config = run_directory.join(runfiles::RUN_CONFIG_FILE);
        if config.is_file() {
            crate::nlh_v1::require_artifact_config(&std::fs::read_to_string(config)?)?;
        }
    }
    let checkpoint = resolve_checkpoint(run_directory)?;
    if checkpoint
        .extension()
        .is_some_and(|extension| extension == "mwckpt")
    {
        let mut payload = mw_preflop::checkpoint::MultiwayCheckpoint::load_unchecked(&checkpoint)
            .with_context(|| format!("reading {}", checkpoint.display()))?;
        let raw = payload.config_toml.take().ok_or_else(||
            anyhow!("removed config family solvers.multiway-preflop/v1 checkpoint without an embedded config; re-solve from a solvers.nlh/v1 config (docs/nlh-input-v1.jp.md)"))?;
        // Drop the first decoded state before the typed P2 resume reloads it
        // and commits the dense arena, avoiding duplicate checkpoint memory.
        drop(payload);
        crate::nlh_v1::require_artifact_config(&raw)?;
        return crate::nlh_v1::p2::resume(
            &raw,
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
    let payload = hu_postflop::checkpoint::CheckpointReader::open(&checkpoint)
        .with_context(|| format!("reading {}", checkpoint.display()))?;
    let embedded = payload.config_toml.as_deref().ok_or_else(||
        anyhow!("removed config family solvers.postflop/v1 checkpoint; re-solve from a solvers.nlh/v1 config (docs/nlh-input-v1.jp.md)"))?;
    crate::nlh_v1::require_artifact_config(embedded)?;
    let directory = if run_directory.is_dir() {
        run_directory
    } else {
        checkpoint
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    };
    let raw = if run_directory.is_dir() {
        std::fs::read_to_string(directory.join(runfiles::RUN_CONFIG_FILE))?
    } else {
        embedded.to_owned()
    };
    drop(payload);
    crate::nlh_v1::require_artifact_config(&raw)?;
    if max_sweeps.is_some()
        || stop_target.is_some()
        || evaluation_samples.is_some()
        || evaluation_cadence.is_some()
    {
        return Err(anyhow!(
            "NLH003: solver overrides are not accepted for P1 resume; only [run] overrides are allowed"
        ));
    }
    crate::nlh_v1::resume(
        &raw,
        directory,
        &checkpoint,
        out,
        threads,
        memory,
        max_time,
        checkpoint_interval,
    )
}

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
        runfiles::RUN_CHECKPOINT_FILE,
        runfiles::RUN_HU_CHECKPOINT_FILE,
    ] {
        let checkpoint = path.join(name);
        if checkpoint.is_file() {
            return Ok(checkpoint);
        }
    }
    let omitted = std::fs::read_to_string(path.join(runfiles::RUN_CONFIG_FILE))
        .ok()
        .and_then(|raw| spot::Document::parse(&raw, &path.join(runfiles::RUN_CONFIG_FILE)).ok())
        .is_some_and(|document| {
            document.spot.product == spot::Product::HuPostflop
                && !document.spot.run.final_checkpoint
        });
    if omitted {
        return Err(anyhow!(
            "run directory {} has no checkpoint; no resumable state was saved (final_checkpoint = false may omit it); re-solve from run.toml",
            path.display()
        ));
    }
    Err(anyhow!(
        "run directory {} has no checkpoint; it never reached its first one",
        path.display()
    ))
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

    #[test]
    fn missing_p2_checkpoint_keeps_the_existing_diagnostic() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join(runfiles::RUN_CONFIG_FILE),
            "schema = 'solvers.nlh/v1'\n[table]\nplayers = 6\nstack_bb = 100\n",
        )
        .unwrap();
        assert_eq!(
            resolve_checkpoint(directory.path())
                .unwrap_err()
                .to_string(),
            format!(
                "run directory {} has no checkpoint; it never reached its first one",
                directory.path().display()
            )
        );
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
        std::fs::write(
            directory.path().join(runfiles::RUN_HU_CHECKPOINT_FILE),
            b"x",
        )
        .unwrap();
        let resolved = resolve_checkpoint(directory.path()).unwrap();
        assert_eq!(resolved.extension().unwrap(), "ckpt");
    }

    fn refused(path: &Path, family: &str) {
        let errors = [
            run(path, None, None, None, None, None, None, None, None, None).unwrap_err(),
            crate::nlh_v1::require_current_artifact(path).unwrap_err(),
        ];
        for error in errors {
            let message = format!("{error:#}");
            assert!(message.contains(family), "{message}");
            assert!(
                message.contains("re-solve from a solvers.nlh/v1 config"),
                "{message}"
            );
            assert!(message.contains("docs/nlh-input-v1.jp.md"), "{message}");
            assert_eq!(crate::error_exit_code(&error), 3);
        }
    }

    fn hu_state() -> hu_engine::SolverState {
        hu_engine::SolverState {
            iteration: 0,
            storage: hu_engine::StorageState::F32 {
                regrets: Vec::new(),
                strategy_sum: Vec::new(),
            },
        }
    }

    #[test]
    fn removed_family_hu_checkpoints_are_refused_embedded_or_legacy() {
        let directory = tempfile::tempdir().unwrap();
        let checkpoint = directory.path().join("checkpoint.ckpt");
        let family = "solvers.postflop/v1";
        hu_postflop::checkpoint::write_checkpoint_with_config(
            &checkpoint,
            [0; 32],
            &hu_state(),
            &format!("schema = '{family}'"),
            0.0,
        )
        .unwrap();
        refused(&checkpoint, family);
        hu_postflop::checkpoint::write_checkpoint(&checkpoint, [0; 32], &hu_state()).unwrap();
        refused(&checkpoint, family);
    }

    #[test]
    fn removed_family_multiway_checkpoint_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let checkpoint = directory.path().join("checkpoint.mwckpt");
        let state = mw_preflop::solver::SolverState {
            schema_version: mw_preflop::solver::SOLVER_STATE_VERSION,
            config: mw_preflop::solver::SolverConfig::default(),
            traversals: 0,
            completed_sweeps: 0,
            next_sample_id: 0,
            total_deal_attempts: 0,
            terminal_evaluations: 0,
            hand_updates: 0,
            histories: Vec::new(),
            policies: Vec::new(),
        };
        let family = "solvers.multiway-preflop/v1";
        mw_preflop::checkpoint::MultiwayCheckpoint::new(state, [0; 32], [0; 32])
            .with_runtime_metadata(format!("schema = '{family}'"), Default::default())
            .write_atomic(&checkpoint)
            .unwrap();
        refused(&checkpoint, family);
    }

    #[test]
    fn removed_family_run_directories_are_refused_before_loading_the_checkpoint() {
        for (family, checkpoint) in [
            ("solvers.postflop/v1", runfiles::RUN_HU_CHECKPOINT_FILE),
            ("solvers.multiway-preflop/v1", runfiles::RUN_CHECKPOINT_FILE),
        ] {
            let directory = tempfile::tempdir().unwrap();
            std::fs::write(
                directory.path().join(runfiles::RUN_CONFIG_FILE),
                format!("schema = '{family}'"),
            )
            .unwrap();
            std::fs::write(directory.path().join(checkpoint), b"unused").unwrap();
            refused(directory.path(), family);
            std::fs::remove_file(directory.path().join(checkpoint)).unwrap();
            refused(directory.path(), family);
        }
    }
}
