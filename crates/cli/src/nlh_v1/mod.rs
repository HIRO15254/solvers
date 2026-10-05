//! Common-input dispatch; each product owns its CLI adapter.
mod p1;
pub(crate) mod p2;

use anyhow::{Context, Result};
use std::path::Path;
pub const SCHEMA: &str = "solvers.nlh/v1";
pub(crate) use p1::{
    Prepared, bb, convert_history, display_game, prepare, query, schedule, threads,
    warnings_for_hits,
};
pub use p1::{compatibility_hash, resume};

pub fn has_schema(raw: &str) -> Result<bool> {
    let value: toml::Value = toml::from_str(raw).context("parsing config")?;
    Ok(value.get("schema").and_then(toml::Value::as_str) == Some(SCHEMA))
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
