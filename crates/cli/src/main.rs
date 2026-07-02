//! `solvers` command-line interface.
//!
//! M1 scope: solve toy games from a TOML config, report convergence, and
//! export the average strategy as JSON. The config schema is the seed of
//! the future `formats::SolveConfig` (M3), which will add board/range/tree
//! sections for hold'em and blake3 config hashing.

mod config;
mod solve;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "solvers", about = "Research poker solver", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Solve the game described by a TOML config file.
    Solve {
        /// Path to the config file (see examples/kuhn.toml).
        config: std::path::PathBuf,
        /// Write the average strategy as JSON to this path.
        #[arg(long)]
        output: Option<std::path::PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Solve { config, output } => solve::run(&config, output.as_deref()),
    }
}
