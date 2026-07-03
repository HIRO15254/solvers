//! `solvers` command-line interface.
//!
//! M1 scope: solve toy games from a TOML config, report convergence, and
//! export the average strategy as JSON. The config schema is the seed of
//! the future `formats::SolveConfig` (M3), which will add board/range/tree
//! sections for hold'em and blake3 config hashing.

mod config;
mod inspect;
mod postflop_setup;
mod report;
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
        /// Betting-line history to export (postflop only; repeatable).
        /// Defaults to the root node only.
        #[arg(long = "history", default_value = "")]
        history: Vec<String>,
    },
    /// Solve a postflop config, then explore the resulting strategy
    /// interactively (a small UPI-subset REPL).
    Inspect {
        /// Path to the config file (must be `kind = "postflop"`).
        config: std::path::PathBuf,
        /// Overrides `run.iterations` from the config.
        #[arg(long)]
        iterations: Option<u64>,
        /// Overrides `run.target_nash_conv` from the config.
        #[arg(long)]
        target_nash_conv: Option<f64>,
    },
    /// Solve the same postflop config across multiple boards and write a
    /// CSV report (one row per board).
    Report {
        /// Path to the config file (must be `kind = "postflop"`); its own
        /// `board` field is ignored in favor of `--boards`/`--boards-file`.
        config: std::path::PathBuf,
        /// Comma-separated boards, each 3/4/5 cards (e.g.
        /// "Ks7h2d,Ks7h2c" or "Ks 7h 2d,Ks 7h 2c").
        #[arg(long)]
        boards: Option<String>,
        /// Path to a file with one board per line (blank lines and lines
        /// starting with `#` are skipped).
        #[arg(long = "boards-file")]
        boards_file: Option<std::path::PathBuf>,
        /// Write the CSV report to this path instead of stdout.
        #[arg(long)]
        output: Option<std::path::PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Solve {
            config,
            output,
            history,
        } => solve::run(&config, output.as_deref(), &history),
        Command::Inspect {
            config,
            iterations,
            target_nash_conv,
        } => inspect::run(&config, iterations, target_nash_conv),
        Command::Report {
            config,
            boards,
            boards_file,
            output,
        } => report::run(
            &config,
            boards.as_deref(),
            boards_file.as_deref(),
            output.as_deref(),
        ),
    }
}
