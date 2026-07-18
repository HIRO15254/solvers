//! `preflop-solver`: the preflop app's CLI (HU preflop + multiway).
//!
//! A thin `clap` binary over the shared `app-core` library. Accepts
//! `kind = "preflop"` / `"preflop-multiway"` configs (plus Kuhn/Leduc as
//! engine smoke checks); postflop configs are redirected to
//! `postflop-solver`. `serve` runs the authenticated loopback bridge the
//! preflop web UI (`apps/preflop/web`) talks to — see
//! `docs/app-structure.md` for the app-level design.

use anyhow::Result;
use app_core::{App, bench, bridge, ensure_config_kind, mw_eval, resume, sol, solve};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "preflop-solver",
    about = "Preflop solver (HU + multiway)",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the authenticated loopback bridge used by the preflop web UI.
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
        /// Path to the config file (see examples/preflop_hu_100bb.toml).
        config: std::path::PathBuf,
        /// Write the average strategy as JSON to this path.
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Betting-line history to export (repeatable). Defaults to the
        /// root node only.
        #[arg(long = "history", default_value = "")]
        history: Vec<String>,
        /// Append a metrics row (JSONL) at every exploitability check.
        #[arg(long)]
        metrics: Option<std::path::PathBuf>,
        /// Autosave a checkpoint at every exploitability check (and once
        /// more at the end), stamped with this config file's blake3 hash.
        #[arg(long)]
        checkpoint: Option<std::path::PathBuf>,
        /// Overrides `run.iterations` (multiway: `run.sweeps`) for this
        /// invocation only. Does not change the checkpoint's config hash,
        /// which is always derived from the raw config file bytes.
        #[arg(long)]
        iterations: Option<u64>,
        /// Export a compact viewer artifact after the run completes
        /// (multiway configs only: an indexed `.mwsol`).
        #[arg(long)]
        sol: Option<std::path::PathBuf>,
    },
    /// Continue a checkpointed solve to its configured iteration total.
    Resume {
        /// Path to the exact same config file used to produce the
        /// checkpoint (verified by blake3 hash of its raw bytes).
        config: std::path::PathBuf,
        /// Path to the checkpoint file to resume from (and keep
        /// autosaving to).
        #[arg(long)]
        checkpoint: std::path::PathBuf,
        /// Write the average strategy as JSON to this path.
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Betting-line history to export (repeatable).
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
    /// Measures strategy purification/thresholding (Ganzfried & Sandholm,
    /// AAMAS 2012) against a multiway checkpoint's average profile. A dev
    /// tool: restores the checkpoint, runs no further sweeps, and prints
    /// one held-out deviation-gain line per requested threshold.
    MwEval {
        /// Path to the exact same config file used to produce the
        /// checkpoint (multiway configs only).
        config: std::path::PathBuf,
        /// Path to the `.mwckpt` checkpoint to restore and evaluate.
        #[arg(long)]
        checkpoint: std::path::PathBuf,
        /// Held-out Monte Carlo samples per threshold.
        #[arg(long, default_value_t = 4096)]
        samples: u64,
        /// Evaluation RNG seed.
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Comma-separated purification thresholds in `[0.0, 1.0]`.
        #[arg(long, default_value = "0.0")]
        purify: String,
        /// Best-response training traversals per seat per threshold. `0`
        /// disables deviator training (regret-greedy heuristic only).
        #[arg(long = "br-traversals", default_value_t = 2000)]
        br_traversals: u64,
        /// Evaluate the LAST-ITERATE regret-matched current strategy
        /// instead of the linear average (diagnostic: plain regret
        /// matching has no last-iterate guarantee; see
        /// `MultiwaySolver::evaluate_profile`).
        #[arg(long, default_value_t = false)]
        current: bool,
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
        } => {
            ensure_config_kind(&config, App::Preflop)?;
            solve::run(
                &config,
                output.as_deref(),
                &history,
                metrics.as_deref(),
                checkpoint.as_deref(),
                iterations,
                sol.as_deref(),
                sol::SolStreets::NoRivers,
            )
        }
        Command::Resume {
            config,
            checkpoint,
            output,
            history,
            metrics,
        } => {
            ensure_config_kind(&config, App::Preflop)?;
            resume::run(
                &config,
                &checkpoint,
                output.as_deref(),
                &history,
                metrics.as_deref(),
            )
        }
        Command::Bench {
            config,
            schedules,
            iterations,
            metrics_dir,
        } => {
            ensure_config_kind(&config, App::Preflop)?;
            bench::run(&config, &schedules, iterations, metrics_dir.as_deref())
        }
        Command::MwEval {
            config,
            checkpoint,
            samples,
            seed,
            purify,
            br_traversals,
            current,
        } => {
            ensure_config_kind(&config, App::Preflop)?;
            mw_eval::run(
                &config,
                &checkpoint,
                samples,
                seed,
                &purify,
                br_traversals,
                current,
            )
        }
    }
}
