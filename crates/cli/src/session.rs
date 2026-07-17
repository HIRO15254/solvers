//! Shared multiway session construction, factored out of
//! `multiway_solve::run_inner` so both the `solvers` CLI and (eventually) a
//! GUI worker build the exact same `MultiwaySolver` from a TOML config.
//!
//! This module intentionally stops short of anything CLI-specific: no
//! stdout printing, no `run.storage` restriction (the CLI still hard-rejects
//! non-f32 storage in `multiway_solve.rs`; the GUI is expected to support
//! `i16` later), and no checkpoint-cadence loop. It only builds the pieces a
//! caller needs to drive one: a ready-to-run `MultiwaySolver`, the run
//! parameters resolved from `[run]`, and the raw config text/hash to stamp
//! onto whatever artifact the caller eventually writes.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use formats::{
    Estimate, MULTIWAY_SCHEMA_VERSION, MultiwayHistoryNode, MultiwayMetricsRow,
    MultiwaySeatMetrics, MultiwaySeatResult, MultiwaySolution, MultiwayStrategyBlock,
    MultiwayStrategyKey,
};
use multiway::abstraction::{
    RolloutKMeansAbstraction, RolloutKMeansBuilder, RolloutKMeansParams, StreetBucketCounts,
};
use multiway::checkpoint::MultiwayCheckpoint;
use multiway::config::{
    FieldPlayerConfig, RakeConfig as MultiwayRake, UtilityConfig as MultiwayUtility,
};
use multiway::solver::{InfoKey, PolicyEntry, ProfileEvaluation, SolverConfig};
use multiway::{HoldemGame, MultiwaySolver};

use crate::config::{
    AlgorithmSection, GameSection, RakeSection, SolveConfig, StorageKind, UtilitySection,
};

const DEFAULT_MEMORY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;

/// Everything needed to run (or resume) a multiway solve: the constructed
/// solver plus the run parameters resolved from `[run]`.
pub struct MultiwaySession {
    pub solver: MultiwaySolver<HoldemGame<RolloutKMeansAbstraction>>,
    pub sweeps_target: u64,
    pub threads: usize,
    pub evaluation_cadence: u64,
    pub evaluation_samples: u64,
    pub evaluation_seed: u64,
    pub checkpoint_every: Option<u64>,
    pub storage: StorageKind,
    /// The exact config text this session was built from, unmodified.
    pub config_toml: String,
    /// Blake3 hash of `config_toml`'s raw bytes (see `formats::config_hash`).
    pub config_hash: [u8; 32],
    /// The multiway game config (seats, blinds, betting), kept around for
    /// display purposes (seat names/positions, button seat).
    pub game_config: multiway::MultiwayConfig,
}

/// Parses `raw_toml`, validates it as a `kind = "preflop-multiway"` config,
/// and builds a ready-to-run (or ready-to-resume) [`MultiwaySession`].
///
/// `resume_checkpoint`, when `Some`, restores solver state from a
/// `.mwckpt` file (always f32 internally, regardless of `run.storage`) and
/// verifies its configuration/abstraction fingerprints match this config
/// before returning.
pub fn build_multiway_session(
    raw_toml: &str,
    resume_checkpoint: Option<&Path>,
) -> Result<MultiwaySession> {
    let config: SolveConfig = toml::from_str(raw_toml).context("parsing config")?;
    let SolveConfig {
        game,
        rake,
        utility,
        algorithm,
        run,
    } = config;
    let GameSection::PreflopMultiway(game_config) = game else {
        return Err(anyhow!(
            "multiway solve path requires kind = \"preflop-multiway\""
        ));
    };
    if run.target_nash_conv.is_some() {
        return Err(anyhow!(
            "multiway profiles do not expose NashConv; remove run.target_nash_conv"
        ));
    }

    let utility = convert_utility(utility)?;
    let rake = convert_rake(rake);
    game_config
        .validate_economics(&utility, &rake)
        .context("validating multiway game and utility")?;

    let (algorithm_seed, exploration_epsilon, discount_every, discount_until, traverser_vector) =
        match algorithm {
            AlgorithmSection::ExternalSamplingMccfr {
                seed,
                exploration_epsilon,
                discount_every,
                discount_until,
                traverser_vector,
            } => (
                seed,
                exploration_epsilon,
                discount_every,
                discount_until,
                traverser_vector,
            ),
            _ => {
                return Err(anyhow!(
                    "preflop-multiway requires schedule = \"external-sampling-mccfr\""
                ));
            }
        };
    let sweeps = run.sweeps.unwrap_or(run.iterations);
    if sweeps == 0 {
        return Err(anyhow!("run.sweeps must be positive for a multiway solve"));
    }
    let evaluation_cadence = run.evaluation_cadence.unwrap_or(run.check_every);
    if evaluation_cadence == 0 {
        return Err(anyhow!("run.evaluation_cadence must be positive"));
    }
    if run.checkpoint_every == Some(0) {
        return Err(anyhow!(
            "run.checkpoint_every must be positive when supplied"
        ));
    }
    let threads = run.threads.unwrap_or_else(rayon::current_num_threads);
    if threads == 0 {
        return Err(anyhow!("run.threads must be positive"));
    }
    if run.evaluation_samples == Some(0) {
        return Err(anyhow!(
            "run.evaluation_samples must be positive when supplied"
        ));
    }
    if run.sweep_batch == Some(0) {
        return Err(anyhow!("run.sweep_batch must be positive when supplied"));
    }

    let evaluation_samples = run.evaluation_samples.unwrap_or(256);
    let params = RolloutKMeansParams {
        flop_buckets: u32::from(game_config.abstraction.flop_buckets),
        turn_buckets: u32::from(game_config.abstraction.turn_buckets),
        river_buckets: u32::from(game_config.abstraction.river_buckets),
        rollout_samples: game_config.abstraction.rollout_samples,
        seed: game_config.abstraction.seed,
    };
    let mut abstraction_builder = RolloutKMeansBuilder::new(params);
    for profile in &game_config.abstraction.active_opponent_buckets {
        abstraction_builder = abstraction_builder
            .active_opponent_buckets(
                profile.active_opponents,
                StreetBucketCounts {
                    flop: u32::from(profile.flop_buckets),
                    turn: u32::from(profile.turn_buckets),
                    river: u32::from(profile.river_buckets),
                },
            )
            .context("configuring active-opponent bucket budgets")?;
    }
    let abstraction = if let Some(path) = game_config.abstraction.artifact_cache.as_deref() {
        if path.is_file() {
            abstraction_builder
                .load_artifact(path)
                .with_context(|| format!("loading rollout artifact {}", path.display()))?
        } else {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("creating rollout artifact directory {}", parent.display())
                })?;
            }
            let abstraction = abstraction_builder
                .build()
                .context("training deterministic multiway rollout abstraction")?;
            abstraction
                .write_artifact(path)
                .with_context(|| format!("writing rollout artifact {}", path.display()))?;
            abstraction
        }
    } else {
        abstraction_builder
            .build()
            .context("training deterministic multiway rollout abstraction")?
    };
    let game = HoldemGame::new(&game_config, &utility, &rake, abstraction)
        .context("building generative multiway game")?;
    let sampler = game.deal_sampler().context("compiling table ranges")?;
    let solver_config = SolverConfig {
        seed: run.seed.unwrap_or(algorithm_seed),
        max_memory_bytes: run.max_memory_bytes.unwrap_or(DEFAULT_MEMORY_LIMIT),
        max_traversal_depth: 512,
        exploration_epsilon,
        discount_every,
        discount_until,
        sweep_batch: run.sweep_batch.unwrap_or(1),
        traverser_vector,
    };
    let evaluation_seed = solver_config.seed ^ 0x6576_616c_7561_7465;
    let solver = if let Some(path) = resume_checkpoint {
        let checkpoint = MultiwayCheckpoint::load_unchecked(path)
            .with_context(|| format!("reading multiway checkpoint {}", path.display()))?;
        let solver =
            MultiwaySolver::from_state_with_config(game, sampler, checkpoint.state, solver_config)
                .context("restoring multiway MCCFR state")?;
        if solver.configuration_fingerprint() != checkpoint.header.configuration_fingerprint {
            return Err(anyhow!(
                "checkpoint belongs to different table rules or ranges"
            ));
        }
        if solver.abstraction_fingerprint() != checkpoint.header.abstraction_fingerprint {
            return Err(anyhow!(
                "checkpoint belongs to a different card abstraction"
            ));
        }
        solver
    } else {
        MultiwaySolver::new(game, sampler, solver_config).context("initializing multiway MCCFR")?
    };
    if solver.metrics().sweeps > sweeps {
        return Err(anyhow!(
            "checkpoint already contains {} sweeps, exceeding target {}",
            solver.metrics().sweeps,
            sweeps
        ));
    }

    Ok(MultiwaySession {
        solver,
        sweeps_target: sweeps,
        threads,
        evaluation_cadence,
        evaluation_samples,
        evaluation_seed,
        checkpoint_every: run.checkpoint_every,
        storage: run.storage,
        config_toml: raw_toml.to_string(),
        config_hash: formats::config_hash(raw_toml.as_bytes()),
        game_config,
    })
}

/// `pub` (rather than `pub(crate)`) so the native GUI can run cheap
/// `validate_economics` checks against the live Setup-tab model without
/// paying for a full `build_multiway_session` (which also trains the card
/// abstraction and builds the game).
pub fn convert_utility(utility: UtilitySection) -> Result<MultiwayUtility> {
    Ok(match utility {
        UtilitySection::ChipEv => MultiwayUtility::ChipEv,
        UtilitySection::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => MultiwayUtility::TournamentIcm {
            outside_field: outside_field
                .into_iter()
                .map(|player| FieldPlayerConfig {
                    name: player.name,
                    stack_bb: player.stack_bb,
                })
                .collect(),
            payouts,
            samples,
            seed,
        },
        UtilitySection::Icm { .. } => {
            return Err(anyhow!(
                "legacy HU utility kind = \"icm\" is not valid for preflop-multiway; use kind = \"tournament-icm\""
            ));
        }
    })
}

/// `pub` for the same reason as [`convert_utility`].
pub fn convert_rake(rake: RakeSection) -> MultiwayRake {
    match rake {
        RakeSection::None => MultiwayRake::None,
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => MultiwayRake::PercentCap {
            rate,
            cap_bb: cap / multiway::types::CHIPS_PER_BB as f64,
            no_flop_no_drop,
        },
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => MultiwayRake::GgPreflop {
            rate,
            cap_bb: cap / multiway::types::CHIPS_PER_BB as f64,
            exempt_pot_bb: exempt_pot as f64 / 1_000.0,
        },
    }
}

/// Sweeps until the next `cadence` boundary (a full `cadence` when already
/// on one). Both the CLI and GUI drive loops size their chunks with this so
/// evaluation/checkpoint cadences fire exactly on their configured multiples.
pub fn distance_to_boundary(current: u64, cadence: u64) -> u64 {
    cadence - current % cadence
}

/// Per-seat average strategy L1 drift since `prior`, used for the
/// convergence-diagnostics metrics row.
pub fn strategy_drift(
    policies: &[PolicyEntry],
    prior: &HashMap<InfoKey, Vec<f32>>,
    seats: usize,
) -> Vec<f64> {
    let mut totals = vec![0.0; seats];
    let mut counts = vec![0u64; seats];
    for entry in policies {
        let current = entry.column.average_strategy();
        let value = prior.get(&entry.key).map_or(0.0, |previous| {
            current
                .iter()
                .zip(previous)
                .map(|(&left, &right)| f64::from((left - right).abs()))
                .sum::<f64>()
                * 0.5
        });
        totals[entry.key.player as usize] += value;
        counts[entry.key.player as usize] += 1;
    }
    totals
        .into_iter()
        .zip(counts)
        .map(|(total, count)| {
            if count == 0 {
                0.0
            } else {
                total / count as f64
            }
        })
        .collect()
}

/// Assembles one `MultiwayMetricsRow` from solver metrics, drift, elapsed
/// time, and an optional held-out profile evaluation.
pub fn metrics_row(
    metrics: &multiway::SolverMetrics,
    drift: Vec<f64>,
    elapsed_secs: f64,
    evaluation: Option<&ProfileEvaluation>,
) -> MultiwayMetricsRow {
    MultiwayMetricsRow {
        schema_version: MULTIWAY_SCHEMA_VERSION,
        phase: "sampling".to_string(),
        sweeps: metrics.sweeps,
        traversals: metrics.traversals,
        elapsed_secs,
        infosets: metrics.infosets,
        memory_bytes: metrics.memory_bytes,
        traversals_per_second: if elapsed_secs > 0.0 {
            metrics.traversals as f64 / elapsed_secs
        } else {
            0.0
        },
        hand_updates: metrics.hand_updates,
        hand_updates_per_second: if elapsed_secs > 0.0 {
            metrics.hand_updates as f64 / elapsed_secs
        } else {
            0.0
        },
        seats: metrics
            .average_positive_regret
            .iter()
            .enumerate()
            .map(|(seat, &regret)| MultiwaySeatMetrics {
                seat: seat as u8,
                profile_ev: evaluation
                    .and_then(|value| value.seats.get(seat))
                    .map(profile_estimate),
                average_positive_regret: regret,
                strategy_drift_l1: drift[seat],
                deviation_gain_lower_bound: evaluation
                    .and_then(|value| value.deviation_gain_lower_bound.as_ref())
                    .and_then(|values| values.get(seat))
                    .map(profile_estimate),
            })
            .collect(),
    }
}

fn profile_estimate(value: &multiway::solver::ProfileEstimate) -> Estimate {
    Estimate {
        mean: value.mean,
        stderr: value.stderr,
        ci95: value.ci95,
    }
}

/// Builds the exportable `.mwsol` artifact from a solver state snapshot and
/// its final metrics row. Shared by every caller that persists a multiway
/// solve's average strategy (the CLI's `--mwsol`, and eventually the GUI's
/// "Finish" action).
pub fn make_solution(
    config_toml: &str,
    abstraction_fingerprint: [u8; 32],
    state: &multiway::solver::SolverState,
    row: &MultiwayMetricsRow,
) -> MultiwaySolution {
    let strategies = state
        .policies
        .iter()
        .map(|entry| MultiwayStrategyBlock {
            key: MultiwayStrategyKey {
                history: entry.key.history.0,
                actor: entry.key.player,
                street: entry.key.street,
                active_opponents: entry.key.active_opponents,
                bucket_path: entry.key.bucket_path,
            },
            actions: entry.column.action_labels.clone(),
            probabilities: entry.column.average_strategy(),
        })
        .collect();
    MultiwaySolution {
        schema_version: MULTIWAY_SCHEMA_VERSION,
        config_toml: config_toml.to_string(),
        abstraction_fingerprint,
        sweeps: row.sweeps,
        approximate_profile: true,
        histories: state
            .histories
            .iter()
            .map(|entry| MultiwayHistoryNode {
                key: entry.key.0,
                parent: entry.parent.0,
                actor: entry.actor,
                action_index: entry.action_index,
                action: entry.action_label.clone(),
            })
            .collect(),
        seats: row
            .seats
            .iter()
            .map(|seat| MultiwaySeatResult {
                seat: seat.seat,
                profile_ev: seat.profile_ev.clone(),
                average_positive_regret: seat.average_positive_regret,
                strategy_drift_l1: seat.strategy_drift_l1,
                deviation_gain_lower_bound: seat.deviation_gain_lower_bound.clone(),
            })
            .collect(),
        strategies,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RakeSection;

    #[test]
    fn boundaries_are_positive_and_repeat() {
        assert_eq!(distance_to_boundary(0, 100), 100);
        assert_eq!(distance_to_boundary(99, 100), 1);
        assert_eq!(distance_to_boundary(100, 100), 100);
    }

    #[test]
    fn shared_rake_chip_units_convert_to_multiway_bb() {
        let rake = convert_rake(RakeSection::PercentCap {
            rate: 0.05,
            cap: 3_500.0,
            no_flop_no_drop: true,
        });
        assert!(matches!(
            rake,
            MultiwayRake::PercentCap {
                cap_bb: 3.5,
                no_flop_no_drop: true,
                ..
            }
        ));
    }

    #[test]
    fn legacy_hu_icm_is_rejected_on_multiway_path() {
        let error = convert_utility(UtilitySection::Icm {
            payouts: [1.0, 0.0],
        })
        .unwrap_err();
        assert!(error.to_string().contains("tournament-icm"));
    }

    #[test]
    fn multiway_example_builds_a_session() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let session = build_multiway_session(raw, None).expect("build multiway session");
        assert_eq!(session.sweeps_target, 2);
        assert_eq!(session.config_toml, raw);
    }
}
