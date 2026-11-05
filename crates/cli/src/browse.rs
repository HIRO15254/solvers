//! `browse`: interactive navigation of saved P2 solutions. The command exists
//! only in builds with the `p2` feature, like `derive` and `evaluate`.
use anyhow::Result;
use std::path::Path;

mod p2;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Color {
    Auto,
    Always,
    Never,
}

pub fn run(file: &Path, config: Option<&Path>, color: Color, selector: &str) -> Result<()> {
    p2::run(file, config, color, selector)
}
