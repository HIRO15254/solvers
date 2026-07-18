//! `bench`: solve the same config once per named CFR schedule (using each
//! schedule's project-default parameters) and print a wall-clock /
//! exploitability comparison table. Reuses `solve::run_with_storage` for
//! the actual solve, so there's exactly one convergence loop in the
//! codebase -- this subcommand only adds schedule selection and reporting.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use engine::{F32Storage, I16Storage};

use crate::config::{AlgorithmSection, SolveConfig, StorageKind, default_gamma0};
use crate::solve::{RunSummary, run_with_storage};

pub fn run(
    config_path: &Path,
    schedules: &[String],
    iterations: Option<u64>,
    metrics_dir: Option<&Path>,
) -> Result<()> {
    let raw_bytes =
        std::fs::read(config_path).with_context(|| format!("reading {}", config_path.display()))?;
    let raw = std::str::from_utf8(&raw_bytes).context("config file is not valid UTF-8")?;

    if let Some(dir) = metrics_dir {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }

    let mut rows: Vec<(String, RunSummary)> = Vec::with_capacity(schedules.len());
    for name in schedules {
        // Re-parsed fresh per schedule (cheap) rather than cloned: several
        // `SolveConfig` sections (e.g. `GameSection`) intentionally don't
        // derive `Clone` since production call sites only ever build one
        // per run.
        let mut config: SolveConfig = toml::from_str(raw).context("parsing config")?;
        config.algorithm = algorithm_for_schedule(name)?;
        if let Some(it) = iterations {
            config.run.iterations = it;
        }
        let metrics_path = metrics_dir.map(|dir| dir.join(format!("{name}.jsonl")));

        println!("=== schedule: {name} ===");
        let summary = match config.run.storage {
            StorageKind::F32 => run_with_storage::<F32Storage>(
                config,
                None,
                &[],
                metrics_path.as_deref(),
                None,
                None,
            )?,
            StorageKind::I16 => run_with_storage::<I16Storage>(
                config,
                None,
                &[],
                metrics_path.as_deref(),
                None,
                None,
            )?,
        };
        rows.push((name.clone(), summary));
    }

    print_table(&rows);
    Ok(())
}

/// Maps a kebab-case schedule name to an `[algorithm]` section using that
/// schedule's project-default parameters (see `config::AlgorithmSection`'s
/// own `Default` impl and per-variant `#[serde(default = ...)]` fns).
fn algorithm_for_schedule(name: &str) -> Result<AlgorithmSection> {
    Ok(match name {
        "vanilla" => AlgorithmSection::Vanilla,
        "cfr-plus" => AlgorithmSection::CfrPlus,
        "dcfr" => AlgorithmSection::default(),
        "linear-cfr" => AlgorithmSection::LinearCfr,
        "hs-dcfr" => AlgorithmSection::HsDcfr {
            gamma0: default_gamma0(),
        },
        other => {
            return Err(anyhow!(
                "unknown schedule {other:?} (expected one of: dcfr, cfr-plus, vanilla, linear-cfr, hs-dcfr)"
            ));
        }
    })
}

/// Fixed-width comparison table: one row per schedule.
fn print_table(rows: &[(String, RunSummary)]) {
    println!(
        "{:<12}{:>12}{:>10}{:>14}{:>14}{:>14}",
        "schedule", "iterations", "wall_s", "expl_p0", "expl_p1", "nash_conv"
    );
    for (name, s) in rows {
        println!(
            "{:<12}{:>12}{:>10.3}{:>14.3e}{:>14.3e}{:>14.3e}",
            name,
            s.iterations,
            s.wall.as_secs_f64(),
            s.expl_p0,
            s.expl_p1,
            s.nash_conv,
        );
    }
}
