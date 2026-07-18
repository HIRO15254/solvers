//! Shared application core for the two solver apps.
//!
//! The project ships two applications — a **preflop solver** (HU preflop +
//! multiway, `apps/preflop/cli`, bin `preflop-solver`) and a **postflop
//! solver** (Mode A exact postflop, `apps/postflop/cli`, bin
//! `postflop-solver`) — each a thin `clap` binary over this library. See
//! `docs/app-structure.md` for the app-level design.
//!
//! This crate owns everything the app binaries share: the TOML config
//! schema ([`config::SolveConfig`]), the solve/resume/bench drivers, the
//! authenticated loopback bridge the web UIs talk to ([`bridge`]), the
//! `.sol` viewer machinery, and the multiway session construction that the
//! native GUI (`apps/preflop/gui`) also reuses without depending on
//! `clap`/stdout-driven behavior.
//!
//! History: M1 grew the TOML-config solve/export loop, M3 added the
//! research-workflow slice (`.ckpt` autosave/resume via `formats`, JSONL
//! metrics, schedule `bench`), and the 2026-07 app restructure split the
//! former combined `solvers` binary into the two per-app binaries.

pub mod auto_run;
pub mod bench;
pub mod bridge;
pub mod config;
pub mod inspect;
pub mod multiway_solve;
pub mod mw_eval;
pub mod node_eval;
pub mod postflop_setup;
pub mod preflop_setup;
pub mod report;
pub mod resume;
pub mod session;
pub mod sol;
pub mod solve;

use std::path::Path;

use anyhow::{Context, Result, anyhow};

use crate::config::{GameSection, SolveConfig};

/// Which of the two solver apps a binary is. Each app CLI gates configs
/// through [`ensure_config_kind`] before dispatching, so a config file is
/// only ever solved by the app it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    /// `preflop-solver`: HU preflop (`kind = "preflop"`) and multiway
    /// (`kind = "preflop-multiway"`).
    Preflop,
    /// `postflop-solver`: Mode A exact postflop (`kind = "postflop"`).
    Postflop,
}

/// Rejects a config whose `game.kind` belongs to the other app, pointing
/// the user at the right binary. Toy games (Kuhn/Leduc) are engine
/// smoke-check configs and are accepted by both apps.
pub fn ensure_config_kind(config_path: &Path, app: App) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config: SolveConfig = toml::from_str(&raw).context("parsing config")?;
    match (app, &config.game) {
        (App::Preflop, GameSection::Postflop { .. }) => Err(anyhow!(
            "{} is a postflop config; solve it with `postflop-solver` instead",
            config_path.display()
        )),
        (App::Postflop, GameSection::Preflop { .. } | GameSection::PreflopMultiway(_)) => {
            Err(anyhow!(
                "{} is a preflop config; solve it with `preflop-solver` instead",
                config_path.display()
            ))
        }
        _ => Ok(()),
    }
}
