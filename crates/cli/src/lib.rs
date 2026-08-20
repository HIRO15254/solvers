//! `solvers` command-line interface, as a library.
//!
//! This crate is split bin+lib so the integration tests can drive the config
//! schema (`config::SolveConfig`) and multiway session construction
//! (`session::build_multiway_session`) directly, without going through
//! `clap`/stdout. `main.rs` is a thin binary shim that just calls
//! [`main_impl`].
//!
//! M1 scope: solve toy games from a TOML config, report convergence, and
//! export the average strategy as JSON. The config schema is the seed of
//! the future `formats::SolveConfig` (M3), which will add board/range/tree
//! sections for hold'em and blake3 config hashing.
//!
//! `solve --checkpoint`/`--metrics` autosave progress (via the `formats`
//! crate's `.ckpt`/JSONL formats), and `resume` continues a checkpointed run
//! to its configured iteration total bit-for-bit identically to an
//! uninterrupted solve.

pub static CLI_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static CLI_EXIT_CODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static CLI_INTERRUPT_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(unix)]
extern "C" fn handle_sigint(_signal: libc::c_int) {
    use std::sync::atomic::Ordering;
    if CLI_INTERRUPT_COUNT.fetch_add(1, Ordering::SeqCst) == 0 {
        CLI_CANCEL.store(true, Ordering::SeqCst);
    } else {
        unsafe { libc::_exit(130) }
    }
}

#[cfg(windows)]
unsafe extern "system" fn handle_console_interrupt(control_type: u32) -> i32 {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if control_type != CTRL_C_EVENT && control_type != CTRL_BREAK_EVENT {
        return 0;
    }
    if CLI_INTERRUPT_COUNT.fetch_add(1, Ordering::SeqCst) == 0 {
        CLI_CANCEL.store(true, Ordering::SeqCst);
    } else {
        std::process::exit(130);
    }
    1
}

pub fn install_signal_handler() -> anyhow::Result<()> {
    CLI_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    CLI_EXIT_CODE.store(0, std::sync::atomic::Ordering::SeqCst);
    CLI_INTERRUPT_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
    #[cfg(unix)]
    unsafe {
        if libc::signal(
            libc::SIGINT,
            handle_sigint as *const () as libc::sighandler_t,
        ) == libc::SIG_ERR
        {
            return Err(anyhow::anyhow!("installing SIGINT handler failed"));
        }
    }
    #[cfg(windows)]
    unsafe {
        if windows_sys::Win32::System::Console::SetConsoleCtrlHandler(
            Some(handle_console_interrupt),
            1,
        ) == 0
        {
            return Err(anyhow::anyhow!(
                "installing console interrupt handler failed"
            ));
        }
    }
    Ok(())
}

pub fn error_exit_code(error: &anyhow::Error) -> i32 {
    if error.chain().any(|cause| {
        cause.is::<formats::MwSolError>()
            || cause.is::<formats::SolError>()
            || cause.is::<formats::CheckpointError>()
            || cause.is::<multiway::checkpoint::CheckpointError>()
    }) {
        return 3;
    }
    let message = format!("{error:#}").to_ascii_lowercase();
    if message.contains("unsupported .mw")
        || message.contains("mwp004")
        || message.contains("bad magic")
        || message.contains("fingerprint mismatch")
        || message.contains("belongs to a different")
        || message.contains("config hash") && message.contains("checkpoint")
    {
        3
    } else if message.contains("resource limit")
        || message.contains("memory budget")
        || message.contains("exceeds memory")
        || message.contains("memory limit")
        || message.contains("memory allocation failed")
        || message.contains("arena preflight")
    {
        75
    } else if message.contains("parsing config")
        || message.contains("mwp001")
        || message.contains("mwp002")
        || message.contains("mwp003")
        || message.contains("validating")
        || message.contains("unknown field")
        || message.contains("schema")
        || message.contains("must be")
        || message.contains("requires --out")
    {
        2
    } else {
        1
    }
}
pub mod cache;
pub mod config;
pub mod config_new;
pub mod inspect;
pub mod multiway_artifact;
pub mod multiway_solve;
pub mod multiway_v1;
pub mod postflop_setup;
pub mod preflop_setup;
pub mod report;
pub mod resume;
pub mod run_dir;
pub mod runs;
pub mod session;
pub mod sol;
pub mod solve;
#[cfg(test)]
mod test_fixtures;
pub mod validate;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "solvers", version)]
#[command(about = "Poker solver")]
struct Cli {
    /// Directory for machine-scoped caches (abstraction tables). Defaults to
    /// `SOLVERS_CACHE_DIR`, then the platform's per-user cache directory.
    /// Never write this into a config: a config naming a local path cannot
    /// be sent to another host.
    #[arg(long, global = true)]
    cache_dir: Option<std::path::PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a Multiway Preflop v1 configuration template.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Validate a Multiway Preflop v1 config without starting a solve.
    Validate {
        config: std::path::PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: validate::ValidationFormat,
        /// Print the normalized, default-expanded effective configuration.
        #[arg(long)]
        show_effective: bool,
        /// Write the reparsable effective TOML configuration to this path.
        #[arg(long)]
        write_effective: Option<std::path::PathBuf>,
        /// Also build the public tree without retaining it and report the
        /// policy arena a real solve would need.
        #[arg(long)]
        resources: bool,
    },
    /// Report what a run directory currently says about itself.
    Status {
        run: std::path::PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: runs::ReportFormat,
    },
    /// Follow a run directory's event log until the run stops.
    Watch {
        run: std::path::PathBuf,
        /// Byte offset into `events.jsonl` to resume from. `solvers status`
        /// reports the offset to use.
        #[arg(long, default_value_t = 0)]
        from: u64,
        /// Seconds between polls while the run is still going.
        #[arg(long = "poll-secs", default_value_t = 1.0)]
        poll_secs: f64,
        #[arg(long, value_enum, default_value = "human")]
        format: runs::ReportFormat,
    },
    /// List the run directories under a runs root.
    Runs {
        #[command(subcommand)]
        command: RunsCommand,
    },
    /// Solve the game described by a TOML config file.
    Solve {
        /// Path to the config file (see examples/kuhn.toml).
        config: std::path::PathBuf,
        /// Run directory to create. Every artifact of the run lands here.
        #[arg(long)]
        out: std::path::PathBuf,
        /// Override v1 worker threads for this invocation.
        #[arg(long)]
        threads: Option<usize>,
        /// Override the v1 policy-arena budget (auto resolves to 6GiB).
        #[arg(long)]
        memory: Option<String>,
        /// Override the v1 cumulative solve-time limit.
        #[arg(long)]
        max_time: Option<String>,
        /// Betting-line history to export (postflop/preflop; repeatable).
        /// Defaults to the root node only.
        #[arg(long = "history", default_value = "")]
        history: Vec<String>,
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
        /// Run directory from `solve --out`, or a bare self-contained
        /// `.mwckpt` that was moved out of one.
        run: std::path::PathBuf,
        /// Fork the resumed run into a new, empty directory.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Override worker threads for this resume segment.
        #[arg(long)]
        threads: Option<usize>,
        /// Override the memory budget for this resume segment.
        #[arg(long)]
        memory: Option<String>,
        /// Override the cumulative solve-time limit.
        #[arg(long)]
        max_time: Option<String>,
        /// Override the total sweep ceiling.
        #[arg(long)]
        max_sweeps: Option<u64>,
        /// Override the measured-deviation stop target.
        #[arg(long)]
        stop_target: Option<f64>,
        /// Override samples per stopping evaluation.
        #[arg(long)]
        evaluation_samples: Option<u64>,
        /// Override evaluation and progress cadence in sweeps.
        #[arg(long)]
        evaluation_cadence: Option<u64>,
        /// Override periodic checkpoint cadence (for example, 15m).
        #[arg(long)]
        checkpoint_interval: Option<String>,
        /// Betting-line history to export (postflop/preflop; repeatable).
        #[arg(long = "history", default_value = "")]
        history: Vec<String>,
    },
    /// Inspect a formal .mwsol artifact, or open the legacy postflop explorer.
    Inspect {
        /// Path to a .mwsol artifact or legacy postflop config.
        #[arg(conflicts_with = "sol")]
        config: Option<std::path::PathBuf>,
        /// Public node: root, a 32-digit history key, or slash-separated action labels/indices.
        #[arg(long, default_value = "root")]
        node: String,
        /// Artifact view to render.
        #[arg(long, value_enum, default_value = "node")]
        view: multiway_artifact::InspectView,
        /// Override the acting seat used by the strategy grid.
        #[arg(long)]
        actor: Option<u8>,
        /// Samples for on-demand EV/CI evaluation.
        #[arg(long, default_value_t = 4096)]
        samples: u64,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long = "br-traversals", default_value_t = 20_000)]
        br_traversals: u64,
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
    /// Re-evaluate a formal `.mwsol` average profile with trained deviations.
    Evaluate {
        solution: std::path::PathBuf,
        #[arg(long, default_value_t = 4096)]
        samples: u64,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long = "br-traversals", default_value_t = 20_000)]
        br_traversals: u64,
    },
    /// Export a stable JSON or CSV view from a `.mwsol` artifact.
    Export {
        solution: std::path::PathBuf,
        #[arg(value_enum)]
        view: multiway_artifact::ExportView,
        #[arg(long, value_enum, default_value = "json")]
        format: multiway_artifact::ExportFormat,
        #[arg(long)]
        output: Option<std::path::PathBuf>,
    },
    /// Compare two formal `.mwsol` average profiles.
    Compare {
        left: std::path::PathBuf,
        right: std::path::PathBuf,
        #[arg(long)]
        cross_game: bool,
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

#[derive(Subcommand)]
enum RunsCommand {
    /// List run directories directly under `root`.
    Ls {
        root: std::path::PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: runs::ReportFormat,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Print or write a valid v1 configuration template.
    New {
        #[arg(long, value_enum, default_value = "minimal")]
        template: config_new::ConfigTemplate,
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
}
/// Parses `std::env::args()` and dispatches to the requested subcommand.
/// The `solvers` binary's `main` is just `cli::main_impl()`.
pub fn main_impl() -> Result<()> {
    let cli = Cli::parse();
    if let Some(root) = cli.cache_dir.as_deref() {
        cache::set_root_override(root);
    }
    match cli.command {
        Command::Config { command } => match command {
            ConfigCommand::New { template, out } => config_new::run(template, out.as_deref()),
        },
        Command::Status { run, format } => runs::status(&run, format),
        Command::Watch {
            run,
            from,
            poll_secs,
            format,
        } => runs::watch(
            &run,
            from,
            std::time::Duration::from_secs_f64(poll_secs.max(0.05)),
            format,
            &CLI_CANCEL,
        ),
        Command::Runs { command } => match command {
            RunsCommand::Ls { root, format } => runs::list(&root, format),
        },
        Command::Validate {
            config,
            format,
            show_effective,
            write_effective,
            resources,
        } => validate::run(
            &config,
            format,
            show_effective,
            write_effective.as_deref(),
            resources,
        ),
        Command::Solve {
            config,
            out,
            threads,
            memory,
            max_time,
            history,
            sol_streets,
        } => solve::run(
            &config,
            &out,
            threads,
            memory.as_deref(),
            max_time.as_deref(),
            &history,
            sol_streets,
        ),
        Command::Resume {
            run,
            out,
            threads,
            memory,
            max_time,
            history,
            max_sweeps,
            stop_target,
            evaluation_samples,
            evaluation_cadence,
            checkpoint_interval,
        } => resume::run(
            &run,
            out.as_deref(),
            threads,
            memory.as_deref(),
            max_time.as_deref(),
            max_sweeps,
            stop_target,
            evaluation_samples,
            evaluation_cadence,
            checkpoint_interval.as_deref(),
            &history,
        ),
        Command::Inspect {
            config,
            node,
            view,
            actor,
            samples,
            seed,
            br_traversals,
            iterations,
            target_nash_conv,
            sol,
            river_iterations,
            river_target,
        } => match (config, sol) {
            (Some(config), None) if config.extension().is_some_and(|value| value == "mwsol") => {
                multiway_artifact::inspect(
                    &config,
                    &node,
                    view,
                    actor,
                    samples,
                    seed,
                    br_traversals,
                )
            }
            (Some(config), None) => inspect::run(&config, iterations, target_nash_conv),
            (None, Some(sol)) => inspect::run_sol(&sol, river_iterations, river_target),
            (None, None) => Err(anyhow::anyhow!("inspect requires either <config> or --sol")),
            (Some(_), Some(_)) => {
                unreachable!("clap's conflicts_with prevents both being set")
            }
        },
        Command::Evaluate {
            solution,
            samples,
            seed,
            br_traversals,
        } => multiway_artifact::evaluate(&solution, samples, seed, br_traversals),
        Command::Export {
            solution,
            view,
            format,
            output,
        } => multiway_artifact::export(&solution, view, format, output.as_deref()),
        Command::Compare {
            left,
            right,
            cross_game,
        } => multiway_artifact::compare(&left, &right, cross_game),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_surface_has_no_experiment_namespace() {
        assert!(Cli::try_parse_from(["solvers", "experiment", "profile"]).is_err());
    }

    #[test]
    fn policy_arena_allocation_failure_uses_the_resource_exit_code() {
        let error =
            anyhow::anyhow!("memory allocation failed for the complete regret buffer (123 bytes)");
        assert_eq!(error_exit_code(&error), 75);
    }
}
