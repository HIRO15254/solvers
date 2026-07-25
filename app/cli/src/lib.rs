//! `solvers` command-line interface, as a library.
//!
//! This crate is split bin+lib so a future native GUI crate can reuse the
//! exact config schema (`config::SolveConfig`) and multiway session
//! construction (`session::build_multiway_session`) the CLI uses, without
//! depending on `clap`/stdout-driven behavior. `main.rs` is a thin binary
//! shim that just calls [`main_impl`].
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
pub mod bench;
pub mod bridge;
pub mod config;
pub mod config_new;
pub mod inspect;
pub mod multiway_artifact;
pub mod multiway_solve;
pub mod multiway_v1;
#[cfg(feature = "research")]
pub mod mw_eval;
pub mod postflop_setup;
pub mod preflop_setup;
pub mod report;
pub mod resume;
pub mod session;
pub mod sol;
pub mod solve;
pub mod validate;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "solvers", version)]
#[cfg_attr(feature = "research", command(about = "Research poker solver"))]
#[cfg_attr(not(feature = "research"), command(about = "Production poker solver"))]
struct Cli {
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
    },
    /// Solve the game described by a TOML config file.
    Solve {
        /// Path to the config file (see examples/kuhn.toml).
        config: std::path::PathBuf,
        /// Required run directory for Multiway Preflop v1.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Override v1 worker threads for this invocation.
        #[arg(long)]
        threads: Option<usize>,
        /// Override the v1 policy-arena budget, up to the production 6GiB limit.
        #[arg(long)]
        memory: Option<String>,
        /// Override the v1 cumulative solve-time limit.
        #[arg(long)]
        max_time: Option<String>,

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
        /// Multiway v1 checkpoint, or the legacy config used with --checkpoint.
        config: std::path::PathBuf,
        /// Legacy checkpoint path. Omit for a self-contained v1 `.mwckpt`.
        #[arg(long, hide = true)]
        checkpoint: Option<std::path::PathBuf>,
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
        /// Legacy output path; hidden from the v1 command surface.
        #[arg(long, hide = true)]
        output: Option<std::path::PathBuf>,
        /// Betting-line history to export (postflop/preflop; repeatable).
        #[arg(long = "history", default_value = "", hide = true)]
        history: Vec<String>,
        /// Append a metrics row (JSONL) at every exploitability check.
        #[arg(long, hide = true)]
        metrics: Option<std::path::PathBuf>,
    },
    /// Explicit research and benchmarking workflows (research build only).
    #[cfg(feature = "research")]
    Experiment {
        #[command(subcommand)]
        command: ExperimentCommand,
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
enum ConfigCommand {
    /// Print or write a valid v1 configuration template.
    New {
        #[arg(long, value_enum, default_value = "minimal")]
        template: config_new::ConfigTemplate,
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
}
#[cfg(feature = "research")]
#[derive(Subcommand)]
enum ExperimentCommand {
    /// Compare average, last-iterate, or purified checkpoint profiles.
    Profile {
        config: std::path::PathBuf,
        #[arg(long)]
        checkpoint: std::path::PathBuf,
        /// Common card abstraction used only to key trained deviations.
        #[arg(long = "deviator-config")]
        deviator_config: Option<std::path::PathBuf>,
        /// Write the common-reference JSON report to a file. Only valid with
        /// `--deviator-config`; legacy profile output remains stdout-only.
        #[arg(long, requires = "deviator_config")]
        output: Option<std::path::PathBuf>,
        /// Experiment rung recorded in common-reference reports. When
        /// omitted, the report uses a sweep-derived standalone scope.
        #[arg(long, requires = "deviator_config")]
        experiment_rung: Option<String>,
        #[arg(long, default_value_t = 4096)]
        samples: u64,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value = "0.0")]
        purify: String,
        #[arg(long = "br-traversals", default_value_t = 2000)]
        br_traversals: u64,
        #[arg(long, default_value_t = false)]
        current: bool,
    },
    /// Compare two artifacts in the research namespace.
    Compare {
        left: std::path::PathBuf,
        right: std::path::PathBuf,
        #[arg(long)]
        cross_game: bool,
    },
    /// Benchmark named CFR schedules.
    Benchmark {
        config: std::path::PathBuf,
        #[arg(long, value_delimiter = ',')]
        schedules: Vec<String>,
        #[arg(long)]
        iterations: Option<u64>,
        #[arg(long = "metrics-dir")]
        metrics_dir: Option<std::path::PathBuf>,
    },
}

/// Parses `std::env::args()` and dispatches to the requested subcommand.
/// The `solvers` binary's `main` is just `cli::main_impl()`.
pub fn main_impl() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Config { command } => match command {
            ConfigCommand::New { template, out } => config_new::run(template, out.as_deref()),
        },
        Command::Serve {
            origin,
            port,
            threads,
        } => bridge::run(&origin, port, threads),
        Command::Validate {
            config,
            format,
            show_effective,
            write_effective,
        } => validate::run(&config, format, show_effective, write_effective.as_deref()),
        Command::Solve {
            config,
            out,
            threads,
            memory,
            max_time,
            output,
            history,
            metrics,
            checkpoint,
            iterations,
            sol,
            sol_streets,
        } => solve::run(
            &config,
            out.as_deref(),
            threads,
            memory.as_deref(),
            max_time.as_deref(),
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
            metrics,
        } => resume::run(
            &config,
            checkpoint.as_deref(),
            out.as_deref(),
            threads,
            memory.as_deref(),
            max_time.as_deref(),
            output.as_deref(),
            max_sweeps,
            stop_target,
            evaluation_samples,
            evaluation_cadence,
            checkpoint_interval.as_deref(),
            &history,
            metrics.as_deref(),
        ),
        #[cfg(feature = "research")]
        Command::Experiment { command } => match command {
            ExperimentCommand::Profile {
                config,
                checkpoint,
                deviator_config,
                output,
                experiment_rung,
                samples,
                seed,
                purify,
                br_traversals,
                current,
            } => mw_eval::run(
                &config,
                &checkpoint,
                deviator_config.as_deref(),
                output.as_deref(),
                experiment_rung.as_deref(),
                samples,
                seed,
                &purify,
                br_traversals,
                current,
            ),
            ExperimentCommand::Compare {
                left,
                right,
                cross_game,
            } => multiway_artifact::compare(&left, &right, cross_game),
            ExperimentCommand::Benchmark {
                config,
                schedules,
                iterations,
                metrics_dir,
            } => bench::run(&config, &schedules, iterations, metrics_dir.as_deref()),
        },
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

    #[cfg(feature = "research")]
    #[test]
    fn experiment_profile_accepts_common_reference_and_output() {
        let cli = Cli::try_parse_from([
            "solvers",
            "experiment",
            "profile",
            "candidate.toml",
            "--checkpoint",
            "candidate.mwckpt",
            "--deviator-config",
            "reference.toml",
            "--output",
            "report.json",
            "--experiment-rung",
            "s1",
        ])
        .unwrap();
        let Command::Experiment {
            command:
                ExperimentCommand::Profile {
                    deviator_config,
                    output,
                    experiment_rung,
                    ..
                },
        } = cli.command
        else {
            panic!("expected experiment profile");
        };
        assert_eq!(
            deviator_config.as_deref(),
            Some(std::path::Path::new("reference.toml"))
        );
        assert_eq!(output.as_deref(), Some(std::path::Path::new("report.json")));
        assert_eq!(experiment_rung.as_deref(), Some("s1"));
    }

    #[cfg(feature = "research")]
    #[test]
    fn experiment_profile_output_requires_common_reference() {
        assert!(
            Cli::try_parse_from([
                "solvers",
                "experiment",
                "profile",
                "candidate.toml",
                "--checkpoint",
                "candidate.mwckpt",
                "--output",
                "report.json",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "solvers",
                "experiment",
                "profile",
                "candidate.toml",
                "--checkpoint",
                "candidate.mwckpt",
                "--experiment-rung",
                "s1",
            ])
            .is_err()
        );
    }

    #[cfg(not(feature = "research"))]
    #[test]
    fn production_command_surface_omits_experiment_namespace() {
        assert!(Cli::try_parse_from(["solvers", "experiment", "profile"]).is_err());
    }

    #[test]
    fn policy_arena_allocation_failure_uses_the_resource_exit_code() {
        let error =
            anyhow::anyhow!("memory allocation failed for the complete regret buffer (123 bytes)");
        assert_eq!(error_exit_code(&error), 75);
    }
}
