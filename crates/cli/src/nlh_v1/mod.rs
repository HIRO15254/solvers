//! Common-input dispatch; each product owns its CLI adapter.
mod p1;
pub(crate) mod p2;

use anyhow::{Result, bail};
use std::path::Path;
pub const SCHEMA: &str = "solvers.nlh/v1";
pub use p1::{compatibility_hash, resume};
pub(crate) use p1::{print_artifact, print_done};

/// Reject artifacts that require a removed input family before rebuilding a game.
pub fn require_artifact_config(raw: &str) -> Result<()> {
    let value: toml::Value = toml::from_str(raw)?;
    let schema = value
        .get("schema")
        .and_then(toml::Value::as_str)
        .unwrap_or("missing schema");
    if schema != SCHEMA {
        bail!(
            "removed config family {schema}: re-solve from a solvers.nlh/v1 config (docs/nlh-input-v1.jp.md)"
        );
    }
    Ok(())
}

/// Check embedded input when a reader receives a checkpoint or run directory.
/// Current artifacts continue to the command's normal reader.
pub fn require_current_artifact(path: &Path) -> Result<()> {
    if path.extension().is_some_and(|e| e == "sol") {
        require_artifact_config(&hu_postflop::sol::read_sol(path)?.config_toml)?;
    } else if path.extension().is_some_and(|e| e == "mwsol") {
        let reader = mw_preflop::mwsol::MwSolReader::open(path)?;
        require_artifact_config(&reader.metadata().config_toml)?;
    } else if path.is_dir() {
        let config = path.join(runfiles::RUN_CONFIG_FILE);
        if config.is_file() {
            require_artifact_config(&std::fs::read_to_string(config)?)?;
        }
    } else if path.extension().is_some_and(|e| e == "ckpt") {
        let checkpoint = hu_postflop::checkpoint::read_checkpoint(path)?;
        let raw = checkpoint.config_toml.as_deref().ok_or_else(|| anyhow::anyhow!(
            "removed config family solvers.postflop/v1 checkpoint: re-solve from a solvers.nlh/v1 config (docs/nlh-input-v1.jp.md)"
        ))?;
        require_artifact_config(raw)?;
    } else if path.extension().is_some_and(|e| e == "mwckpt") {
        let checkpoint = mw_preflop::checkpoint::MultiwayCheckpoint::load_unchecked(path)?;
        let raw = checkpoint.config_toml.as_deref().ok_or_else(|| anyhow::anyhow!(
            "removed config family solvers.multiway-preflop/v1 checkpoint: re-solve from a solvers.nlh/v1 config (docs/nlh-input-v1.jp.md)"
        ))?;
        require_artifact_config(raw)?;
    }
    Ok(())
}

pub fn validate(
    raw: &str,
    path: &Path,
    format: crate::validate::ValidationFormat,
    show: bool,
    write: Option<&Path>,
    resources: bool,
) -> Result<()> {
    match spot::Document::parse(raw, path)?.spot.product {
        spot::Product::HuPostflop => p1::validate(raw, path, format, show, write, resources),
        spot::Product::MultiwayPreflop => p2::validate(raw, path, format, show, write, resources),
    }
}

pub fn solve(
    raw: &str,
    path: &Path,
    out: &Path,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
) -> Result<()> {
    match spot::Document::parse(raw, path)?.spot.product {
        spot::Product::HuPostflop => p1::solve(raw, path, out, threads, memory, max_time),
        spot::Product::MultiwayPreflop => p2::solve(raw, path, out, threads, memory, max_time),
    }
}
