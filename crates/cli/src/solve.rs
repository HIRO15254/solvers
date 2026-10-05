//! Solve a common-input P1 or P2 spot into its run directory.
use anyhow::{Context, Result};
use std::path::Path;

pub fn run_cli(
    config_path: &Path,
    out: &Path,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    crate::nlh_v1::solve(&raw, config_path, out, threads, memory, max_time)
}
