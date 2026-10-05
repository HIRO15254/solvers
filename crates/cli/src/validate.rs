use anyhow::{Context, Result};
use clap::ValueEnum;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ValidationFormat {
    #[default]
    Human,
    Json,
}

pub fn run(
    path: &Path,
    format: ValidationFormat,
    show: bool,
    write: Option<&Path>,
    resources: bool,
) -> Result<()> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    crate::nlh_v1::validate(&raw, path, format, show, write, resources)
}
