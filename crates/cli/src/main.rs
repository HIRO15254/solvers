//! `solvers` command-line interface.
//!
//! M1 scope: solve toy games from a TOML config, report convergence, and
//! export the average strategy as JSON. The config schema is the seed of
//! the future `formats::SolveConfig` (M3), which will add board/range/tree
//! sections for hold'em and blake3 config hashing.
//!
//! M3 adds the research-workflow slice: `solve --checkpoint`/`--metrics`
//! autosave progress (via the `formats` crate's `.ckpt`/JSONL formats),
//! `resume` continues a checkpointed run to its configured iteration total
//! bit-for-bit identically to an uninterrupted solve, and `bench` compares
//! CFR schedules (dcfr/cfr-plus/vanilla/linear-cfr/hs-dcfr) on the same
//! config in one pass.

mod bench;
mod bridge;
mod config;
mod inspect;
mod multiway_solve;
mod postflop_setup;
mod preflop_setup;
mod report;
mod resume;
mod sol;
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
    /// Run the authenticated loopback bridge used by the local web UI.
    Serve {
        /// Exact browser Origin allowed by CORS (scheme, host, and port).
        #[arg(long, default_value = "http://localhost:3000")]
        origin: String,
        /// Loopback TCP port. Use 0 to select an ephemeral free port.
        #[arg(long, default_value_t = 38127)]
        port: u16,
        /// Rayon worker threads owned by the bridge process.
        #[arg(long)]
        threads: Option<usize>,
    },
    /// Solve the game described by a TOML config file.
    Solve {
        /// Path to the config file (see examples/kuhn.toml).
        config: std::path::PathBuf,
        /// Write the average strategy as JSON to this path.
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Betting-line history to export (postflop/preflop; repeatable).
        /// Defaults to the root node only.
        #[arg(long = "history", default_value = "")]
        history: Vec<String>,
        /// Append a metrics row (JSONL) at every exploitability check.
        #[arg(long)]
        metrics: Option<std::path::PathBuf>,
        /// Autosave a checkpoint at every exploitability check (and once
        /// more at the end), stamped with this config file's blake3 hash.
        #[arg(long)]
        checkpoint: Option<std::path::PathBuf>,
        /// Overrides `run.iterations` for this invocation only. Does not
        /// change the checkpoint's config hash, which is always derived
        /// from the raw config file bytes.
        #[arg(long)]
        iterations: Option<u64>,
        /// Export a compact `.sol` viewer artifact after the run completes
        /// (postflop configs only; browse it later with `inspect --sol`).
        #[arg(long)]
        sol: Option<std::path::PathBuf>,
        /// Which streets get stored strategy blocks in the `.sol` export.
        /// `no-rivers` (default) omits river action nodes -- the viewer
        /// re-solves them lazily on demand; `full` stores every action node
        /// (and is forced regardless of this flag when the config itself
        /// starts on the river).
        #[arg(long = "sol-streets", value_enum, default_value = "no-rivers")]
        sol_streets: sol::SolStreets,
    },
    /// Continue a checkpointed solve to `run.iterations` total iterations.
    Resume {
        /// Path to the exact same config file used to produce the
        /// checkpoint (verified by blake3 hash of its raw bytes).
        config: std::path::PathBuf,
        /// Path to the `.ckpt` file to resume from (and keep autosaving to).
        #[arg(long)]
        checkpoint: std::path::PathBuf,
        /// Write the average strategy as JSON to this path.
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Betting-line history to export (postflop/preflop; repeatable).
        #[arg(long = "history", default_value = "")]
        history: Vec<String>,
        /// Append a metrics row (JSONL) at every exploitability check.
        #[arg(long)]
        metrics: Option<std::path::PathBuf>,
    },
    /// Solve the same config once per named CFR schedule and print a
    /// wall-clock/exploitability comparison table.
    Bench {
        /// Path to the config file; its own `[algorithm]` section is
        /// ignored in favor of `--schedules`.
        config: std::path::PathBuf,
        /// Comma-separated schedule names (dcfr, cfr-plus, vanilla,
        /// linear-cfr, hs-dcfr), each run with its project-default
        /// parameters.
        #[arg(long, value_delimiter = ',')]
        schedules: Vec<String>,
        /// Overrides `run.iterations` for every schedule.
        #[arg(long)]
        iterations: Option<u64>,
        /// Writes one `<schedule>.jsonl` metrics file per schedule into
        /// this directory (created if missing).
        #[arg(long = "metrics-dir")]
        metrics_dir: Option<std::path::PathBuf>,
    },
    /// Solve a postflop config, then explore the resulting strategy
    /// interactively (a small UPI-subset REPL). Exactly one of `config` or
    /// `--sol` is required.
    Inspect {
        /// Path to the config file (must be `kind = "postflop"`). Mutually
        /// exclusive with `--sol`.
        #[arg(conflicts_with = "sol")]
        config: Option<std::path::PathBuf>,
        /// Overrides `run.iterations` from the config (live solve only).
        #[arg(long)]
        iterations: Option<u64>,
        /// Overrides `run.target_nash_conv` from the config (live solve
        /// only).
        #[arg(long)]
        target_nash_conv: Option<f64>,
        /// Load a pre-solved `.sol` viewer artifact instead of solving a
        /// live config. Mutually exclusive with `config`.
        #[arg(long, conflicts_with = "config")]
        sol: Option<std::path::PathBuf>,
        /// Planned iteration budget for a river subgame's lazy re-solve
        /// (`--sol` only).
        #[arg(long = "river-iterations", default_value_t = 500)]
        river_iterations: u64,
        /// Stop a river re-solve early once its subgame's `nash_conv` drops
        /// below this (`--sol` only).
        #[arg(long = "river-target")]
        river_target: Option<f64>,
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
        Command::Serve {
            origin,
            port,
            threads,
        } => bridge::run(&origin, port, threads),
        Command::Solve {
            config,
            output,
            history,
            metrics,
            checkpoint,
            iterations,
            sol,
            sol_streets,
        } => solve::run(
            &config,
            output.as_deref(),
            &history,
            metrics.as_deref(),
            checkpoint.as_deref(),
            iterations,
            sol.as_deref(),
            sol_streets,
        ),
        Command::Resume {
            config,
            checkpoint,
            output,
            history,
            metrics,
        } => resume::run(
            &config,
            &checkpoint,
            output.as_deref(),
            &history,
            metrics.as_deref(),
        ),
        Command::Bench {
            config,
            schedules,
            iterations,
            metrics_dir,
        } => bench::run(&config, &schedules, iterations, metrics_dir.as_deref()),
        Command::Inspect {
            config,
            iterations,
            target_nash_conv,
            sol,
            river_iterations,
            river_target,
        } => match (config, sol) {
            (Some(config), None) => inspect::run(&config, iterations, target_nash_conv),
            (None, Some(sol)) => inspect::run_sol(&sol, river_iterations, river_target),
            (None, None) => Err(anyhow::anyhow!("inspect requires either <config> or --sol")),
            (Some(_), Some(_)) => {
                unreachable!("clap's conflicts_with prevents both being set")
            }
        },
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
