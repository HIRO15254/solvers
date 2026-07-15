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

pub fn run(
    config_path: &Path,
    checkpoint_path: &Path,
    output: Option<&Path>,
    histories: &[String],
    metrics: Option<&Path>,
) -> Result<()> {
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
            None,
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
