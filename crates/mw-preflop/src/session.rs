//! Typed P2 sessions, resource preflight, metrics, and solution export.

use std::path::Path;
use std::time::Instant;

use crate::abstraction::{
    BucketContext, BucketId, MultiwayAbstraction, MultiwayAbstractionBackend,
    TableAbstractionAdapter, ehs2_table_fingerprint,
};
use crate::card_abstraction::{Ehs2Abstraction, Ehs2Params};
use crate::config::{
    AbstractionConfig, RakeConfig as MultiwayRake, RecallMode, UtilityConfig as MultiwayUtility,
};
use crate::metrics::{Estimate, MULTIWAY_SCHEMA_VERSION, MultiwayMetricsRow, MultiwaySeatMetrics};
use crate::mwsol::{
    MultiwayHistoryNode, MultiwayPublicAction, MultiwayPublicState, MultiwaySeatResult,
    MultiwaySolution, MultiwayStrategyBlock, MultiwayStrategyKey, MultiwayStrategyWeight,
};
use crate::solver::{ProfileEvaluation, SolverConfig};
use crate::{ExternalSamplingGame, HoldemGame, MultiwaySolver, Street};
use anyhow::{Context, Result, anyhow};
use rayon::prelude::*;

use crate::input::Lowered;

/// Resolved P2 convergence stop rule.
/// `None` in [`MultiwaySession::stop_rule`] means the rule is disabled and
/// `sweeps_target` is a plain target rather than a safety cap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopRule {
    /// Threshold, in the run's own utility unit, compared against the
    /// maximum per-seat `deviation_gain_lower_bound` CI upper bound.
    pub dev_gain_threshold: f64,
    /// Consecutive passing evaluations required before stopping.
    pub confirmations: u32,
    /// Best-response training traversals per seat per stop-rule evaluation
    /// `0` disables
    /// the burst.
    pub br_traversals: u64,
}

/// Everything needed to run (or resume) a multiway solve: the constructed
/// solver plus the run parameters resolved from `[run]`.
/// How the run got its card abstraction, for the run's event log.
///
/// Building the EHS² tables takes minutes on a cold cache, so a watcher that
/// sees nothing during that time needs to be told why.
#[derive(Clone, Copy, Debug)]
pub struct AbstractionReady {
    pub cached: bool,
    pub secs: f64,
}

pub struct MultiwaySession {
    pub solver: MultiwaySolver<HoldemGame<MultiwayAbstractionBackend>>,
    /// `None` for a backend that needs no table (the test baseline).
    pub abstraction_ready: Option<AbstractionReady>,
    pub sweeps_target: u64,
    pub threads: usize,
    pub evaluation_cadence: u64,
    pub evaluation_samples: u64,
    pub evaluation_seed: u64,
    /// Convergence-based stop rule resolved from P2 solver settings.
    pub stop_rule: Option<StopRule>,
    /// The exact config text this session was built from, unmodified.
    pub config_toml: String,
    /// Blake3 hash of `config_toml`'s raw bytes (see `runfiles::config_hash`).
    pub config_hash: [u8; 32],
    /// The multiway game config (seats, blinds, betting), kept around for
    /// display purposes (seat names/positions, button seat).
    pub game_config: crate::MultiwayConfig,
    pub checkpoint_runtime: Option<crate::checkpoint::CheckpointRuntimeState>,
}

/// Resource facts derived from the production public tree and configured
/// bucket counts without allocating the policy arena or building EHS² tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiwayResourcePreflight {
    pub complete: bool,
    pub limit: Option<ResourceLimit>,
    pub recall: RecallMode,
    pub decision_nodes: u64,
    pub terminal_edges: Option<u64>,
    pub icm: Option<MultiwayIcmPreflight>,
    pub policy_columns: Option<u64>,
    pub policy_slots: Option<u64>,
    pub solver_state_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceLimit {
    Node,
    Memory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiwayIcmPreflight {
    pub field_players: u64,
    pub paid_places: u64,
    pub mode: IcmPreflightMode,
    pub samples: Option<u64>,
    pub seed: Option<u64>,
    pub prepared_bytes: Option<u64>,
    pub prepared_limit_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IcmPreflightMode {
    Exact,
    Sampled,
}

struct ResourcePreflightAbstraction {
    config: AbstractionConfig,
}

impl MultiwayAbstraction for ResourcePreflightAbstraction {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
        if street == Street::Preflop {
            return nlh::NUM_CLASSES as u32;
        }
        let profile = self.config.buckets_for(active_opponents);
        match street {
            Street::Preflop => unreachable!("preflop returned above"),
            Street::Flop => profile.map_or(u32::from(self.config.flop_buckets), |counts| {
                u32::from(counts.flop_buckets)
            }),
            Street::Turn => profile.map_or(u32::from(self.config.turn_buckets), |counts| {
                u32::from(counts.turn_buckets)
            }),
            Street::River => profile.map_or(u32::from(self.config.river_buckets), |counts| {
                u32::from(counts.river_buckets)
            }),
        }
    }

    fn bucket(&self, _context: BucketContext<'_>) -> BucketId {
        unreachable!("resource preflight never assigns physical hands to buckets")
    }

    fn fingerprint(&self) -> [u8; 32] {
        [0; 32]
    }
}

/// Runs the production contract, range/economics checks, and byte-bounded
/// non-retaining tree preflight behind `solvers validate --resources`.
///
/// The tree is walked without retaining nodes, so this reports the arena a
/// real solve would need without allocating it or building EHS² tables.
#[cfg(test)]
pub(crate) fn preflight_multiway_typed(config: Lowered) -> Result<MultiwayResourcePreflight> {
    Ok(preflight_multiway_impl(config, false)?.0)
}

pub(crate) fn preflight_multiway_with_rule_hits(
    config: Lowered,
) -> Result<(MultiwayResourcePreflight, Vec<bool>)> {
    preflight_multiway_impl(config, true)
}

fn preflight_multiway_impl(
    config: Lowered,
    measure_hits: bool,
) -> Result<(MultiwayResourcePreflight, Vec<bool>)> {
    let Lowered {
        game: game_config,
        rake,
        utility,
        run,
        ..
    } = config;
    game_config
        .validate_economics(&utility, &rake)
        .context("validating multiway game and utility for preflight")?;
    let icm = match &utility {
        MultiwayUtility::ChipEv => None,
        MultiwayUtility::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => {
            let field_players = game_config
                .seats
                .len()
                .checked_add(outside_field.len())
                .context("counting ICM field players")?;
            let paid_places = payouts
                .iter()
                .rposition(|payout| *payout != 0.0)
                .map_or(0, |place| place + 1);
            if field_players <= crate::icm::EXACT_ICM_MAX_PLAYERS {
                Some(MultiwayIcmPreflight {
                    field_players: field_players as u64,
                    paid_places: paid_places as u64,
                    mode: IcmPreflightMode::Exact,
                    samples: None,
                    seed: None,
                    prepared_bytes: None,
                    prepared_limit_bytes: None,
                })
            } else {
                let prepared_bytes = crate::icm::prepared_race_memory_bytes(
                    game_config.seats.len(),
                    outside_field.len(),
                    paid_places,
                    *samples,
                )
                .context("sizing sampled ICM preparation buffers")?;
                Some(MultiwayIcmPreflight {
                    field_players: field_players as u64,
                    paid_places: paid_places as u64,
                    mode: IcmPreflightMode::Sampled,
                    samples: Some(*samples),
                    seed: Some(*seed),
                    prepared_bytes: Some(prepared_bytes as u64),
                    prepared_limit_bytes: Some(crate::icm::MAX_PREPARED_RACE_BYTES as u64),
                })
            }
        }
    };

    let recall = game_config.abstraction.recall;
    let abstraction = ResourcePreflightAbstraction {
        config: game_config.abstraction.clone(),
    };
    let game = HoldemGame::new(
        &game_config,
        &MultiwayUtility::ChipEv,
        &MultiwayRake::None,
        abstraction,
    )
    .context("building public multiway game for preflight")?;
    let game = if measure_hits {
        game.with_tree_rule_hits()
    } else {
        game
    };
    let _sampler = game
        .deal_sampler()
        .context("compiling table ranges for preflight")?;
    let memory_limit = run.memory_bytes;
    let arena = match crate::tree::preflight_arena(&game, memory_limit) {
        Ok(arena) => arena,
        Err(crate::tree::TreeError::MemoryLimit {
            node_count,
            total_columns,
            needed,
            ..
        }) => {
            return Ok((
                MultiwayResourcePreflight {
                    complete: false,
                    limit: Some(ResourceLimit::Memory),
                    recall,
                    decision_nodes: node_count as u64,
                    terminal_edges: None,
                    icm,
                    policy_columns: Some(total_columns),
                    policy_slots: None,
                    solver_state_bytes: Some(needed),
                },
                game.tree_rule_hits().unwrap_or_default(),
            ));
        }
        Err(crate::tree::TreeError::TooManyNodes { limit }) => {
            return Ok((
                MultiwayResourcePreflight {
                    complete: false,
                    limit: Some(ResourceLimit::Node),
                    recall,
                    decision_nodes: limit as u64,
                    terminal_edges: None,
                    icm,
                    policy_columns: None,
                    policy_slots: None,
                    solver_state_bytes: None,
                },
                game.tree_rule_hits().unwrap_or_default(),
            ));
        }
        Err(error) => {
            return Err(error).context("counting public tree and sizing the policy arena");
        }
    };

    Ok((
        MultiwayResourcePreflight {
            complete: true,
            limit: None,
            recall,
            decision_nodes: arena.node_count as u64,
            terminal_edges: Some(arena.terminal_edges),
            icm,
            policy_columns: Some(arena.total_columns),
            policy_slots: Some(arena.total_slots),
            solver_state_bytes: Some(arena.estimated_arena_bytes),
        },
        game.tree_rule_hits().unwrap_or_default(),
    ))
}

/// Build a session from the product's typed settings and effective input.
pub fn build_production_multiway_session(
    input: Lowered,
    effective: String,
    checkpoint: Option<&Path>,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(AbstractionReady),
) -> Result<MultiwaySession> {
    let (table, ready) = build_ehs2_table_abstraction(&input.game, cache_root, on_ready)?;
    wrap_session(
        crate::input::build_session(
            input,
            MultiwayAbstractionBackend::Ehs2Table(table),
            effective,
            checkpoint,
        )?,
        Some(ready),
    )
}

#[cfg(test)]
pub(crate) fn build_multiway_session(
    raw: &str,
    checkpoint: Option<&Path>,
) -> Result<MultiwaySession> {
    let p = crate::prepare::prepare(raw, Path::new("embedded.toml"))?;
    build_test_session(p.lowered, p.effective, checkpoint)
}

#[cfg(test)]
pub(crate) fn build_test_session(
    input: Lowered,
    effective: String,
    checkpoint: Option<&Path>,
) -> Result<MultiwaySession> {
    use crate::abstraction::{FeatureHashAbstraction, FeatureHashParams};
    let a = &input.game.abstraction;
    let abstraction =
        MultiwayAbstractionBackend::FeatureHash(FeatureHashAbstraction::new(FeatureHashParams {
            flop_buckets: u32::from(a.flop_buckets),
            turn_buckets: u32::from(a.turn_buckets),
            river_buckets: u32::from(a.river_buckets),
        })?);
    wrap_session(
        crate::input::build_session(input, abstraction, effective, checkpoint)?,
        None,
    )
}

fn wrap_session(
    session: crate::input::Session<MultiwayAbstractionBackend>,
    ready: Option<AbstractionReady>,
) -> Result<MultiwaySession> {
    let r = session.run;
    Ok(MultiwaySession {
        solver: session.solver,
        abstraction_ready: ready,
        sweeps_target: r.stop.max_sweeps,
        threads: r.threads,
        evaluation_cadence: r.stop.check_every_sweeps,
        evaluation_samples: r.stop.evaluation_samples,
        evaluation_seed: r.evaluation_seed,
        stop_rule: Some(StopRule {
            dev_gain_threshold: r.dev_gain_threshold,
            confirmations: r.stop.confirmations,
            br_traversals: r.stop.deviator_traversals,
        }),
        config_toml: session.config_toml,
        config_hash: session.config_hash,
        game_config: session.game_config,
        checkpoint_runtime: session.checkpoint_runtime,
    })
}

pub(crate) const MAX_STOP_RULE_SAMPLES: u64 = 65_536;

/// Mutable state of the convergence stop rule
/// across one run, threaded through repeated [`run_stop_rule_check`] calls.
pub(crate) struct StopRuleState {
    /// Adaptive evaluation sample count: starts at the session's
    /// `evaluation_samples` and doubles (capped at [`MAX_STOP_RULE_SAMPLES`])
    /// whenever a check's CI is too wide to ever settle below the threshold.
    pub samples: u64,
    /// Consecutive passing evaluations so far.
    pub confirmations_met: u32,
    /// Monotonically increasing index of the next check, folded into the
    /// training and held-out evaluation seeds. Persisted in checkpoints so
    /// resumed checks continue with fresh batches instead of reusing evidence.
    pub eval_index: u64,
}

impl StopRuleState {
    pub fn new(initial_samples: u64) -> Self {
        Self {
            samples: initial_samples,
            confirmations_met: 0,
            eval_index: 0,
        }
    }
}

/// Trains one burst deviator per seat in parallel, on a deterministic worker
/// pool sized to `threads` rather than relying on rayon's ambient global
/// pool (whose thread count the config doesn't control). Shared by every
/// caller that trains a per-seat best-response burst, such as the stop-rule
/// check below.
pub(crate) fn train_deviators_parallel(
    solver: &MultiwaySolver<HoldemGame<MultiwayAbstractionBackend>>,
    num_players: usize,
    threads: usize,
    traversals: u64,
    seed: u64,
    variant: crate::ProfileVariant,
) -> Result<Vec<crate::DeviatorPolicy>> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| anyhow!("building deviator training thread pool: {error}"))?;
    pool.install(|| {
        (0..num_players)
            .into_par_iter()
            .map(|seat| solver.train_deviator(seat, traversals, seed, variant))
            .collect::<std::result::Result<Vec<_>, _>>()
    })
    .map_err(|error| anyhow!("training deviator: {error}"))
}

/// Result of one [`run_stop_rule_check`] call: the evaluation plus what
/// happened, so callers only do IO (printing/event-sending) and decide
/// whether to break their drive loop.
pub struct StopRuleCheck {
    pub evaluation: ProfileEvaluation,
    /// Maximum per-seat `deviation_gain_lower_bound` CI upper bound this
    /// check measured.
    pub max_upper: f64,
    /// Maximum per-seat `deviation_gain_lower_bound` CI width this check
    /// measured.
    pub max_width: f64,
    /// `Some(new_sample_count)` when this check's CI was too wide to ever
    /// settle below the threshold and the adaptive sample count was
    /// doubled (capped at [`MAX_STOP_RULE_SAMPLES`]); `None` otherwise.
    pub samples_doubled_to: Option<u64>,
    /// Whether `state.confirmations_met` (after this check) has reached
    /// `stop_rule.confirmations`.
    pub converged: bool,
}

/// Independent, reproducible streams for a scheduled stop check. Replaying
/// one fixed evaluation batch at every boundary would let a lucky batch
/// count repeatedly toward `confirmations`, even for an unchanged profile.
fn stop_check_seeds(seed: u64, sequence: u64) -> (u64, u64) {
    let derive = |domain: &[u8]| {
        let mut hasher = blake3::Hasher::new();
        hasher.update(domain);
        hasher.update(&seed.to_le_bytes());
        hasher.update(&sequence.to_le_bytes());
        u64::from_le_bytes(hasher.finalize().as_bytes()[..8].try_into().unwrap())
    };
    (
        derive(b"solvers.multiway.stop-training.v1"),
        derive(b"solvers.multiway.stop-held-out.v1"),
    )
}

/// One stop-rule check: an optional best-response burst, the held-out
/// evaluation, threshold/width bookkeeping, adaptive sample doubling, and
/// the confirmations update. Callers are expected to have already checked
/// sweep cadence before calling this; it unconditionally performs one check.
pub(crate) fn run_stop_rule_check(
    solver: &MultiwaySolver<HoldemGame<MultiwayAbstractionBackend>>,
    stop_rule: &StopRule,
    state: &mut StopRuleState,
    num_players: usize,
    threads: usize,
    evaluation_seed: u64,
) -> Result<StopRuleCheck> {
    // One observation has no estimable sample variance. The diagnostic API
    // permits it, but its zero standard error must not certify a stop.
    // Persist and report the actual effective sample count.
    state.samples = state.samples.max(2);
    // Best-response burst: stop-rule evaluations measure a per-seat deviator
    // TRAINED against the frozen current average profile rather than the
    // plain regret-greedy heuristic the ordinary evaluation-cadence rows
    // use, so the deviation-gain numbers reported here are systematically
    // tighter (higher) than a cadence row taken at the same sweep count.
    // That is intentional: the stop decision should use the strongest
    // available deviator, not the cheap heuristic every metrics row gets.
    let (training_seed, held_out_seed) = stop_check_seeds(evaluation_seed, state.eval_index);
    state.eval_index = state
        .eval_index
        .checked_add(1)
        .context("stop-rule evaluation sequence overflow")?;
    let deviators = if stop_rule.br_traversals > 0 {
        Some(
            train_deviators_parallel(
                solver,
                num_players,
                threads,
                stop_rule.br_traversals,
                training_seed,
                crate::ProfileVariant::default(),
            )
            .context("training best-response deviators for the convergence stop rule")?,
        )
    } else {
        None
    };
    let evaluation = solver
        .evaluate_profile(
            state.samples,
            held_out_seed,
            deviators.as_deref(),
            crate::ProfileVariant::default(),
        )
        .context("evaluating multiway profile for the convergence stop rule")?;

    let bounds = evaluation
        .deviation_gain_lower_bound
        .as_ref()
        .expect("evaluate_profile always returns deviation_gain_lower_bound");
    let max_upper = bounds
        .iter()
        .map(|estimate| estimate.ci95[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let max_width = bounds
        .iter()
        .map(|estimate| estimate.ci95[1] - estimate.ci95[0])
        .fold(0.0, f64::max);

    state.confirmations_met = if max_upper <= stop_rule.dev_gain_threshold {
        state.confirmations_met + 1
    } else {
        0
    };

    // The CI is wide enough that the check could never pass even if the
    // true value already converged: double the sample count for subsequent
    // stop-rule checks (capped), so noise shrinks over time instead of
    // blocking convergence forever.
    let samples_doubled_to =
        if max_width > stop_rule.dev_gain_threshold && state.samples < MAX_STOP_RULE_SAMPLES {
            let doubled = state.samples.saturating_mul(2).min(MAX_STOP_RULE_SAMPLES);
            let changed = doubled != state.samples;
            state.samples = doubled;
            changed.then_some(doubled)
        } else {
            None
        };

    Ok(StopRuleCheck {
        evaluation,
        max_upper,
        max_width,
        samples_doubled_to,
        converged: state.confirmations_met >= stop_rule.confirmations,
    })
}

/// Load the EHS² table with the existing cache key and abstraction fingerprint.
pub(crate) fn build_ehs2_table_abstraction(
    game_config: &crate::MultiwayConfig,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(AbstractionReady),
) -> Result<(TableAbstractionAdapter<Ehs2Abstraction>, AbstractionReady)> {
    let params = Ehs2Params {
        flop_buckets: u32::from(game_config.abstraction.flop_buckets),
        turn_buckets: u32::from(game_config.abstraction.turn_buckets),
        river_buckets: u32::from(game_config.abstraction.river_buckets),
    };
    // A config may still name its own cache file; otherwise the table comes
    // from the machine-scoped cache, which is where a 543 MB artifact that
    // depends only on the bucket counts belongs.
    let machine_cache = match game_config.abstraction.artifact_cache {
        Some(_) => None,
        None => crate::card_abstraction::cache_path(cache_root, params)?,
    };
    let cache = game_config
        .abstraction
        .artifact_cache
        .as_deref()
        .or(machine_cache.as_deref());
    if let Some(parent) = cache
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating ehs2 table cache directory {}", parent.display()))?;
    }
    let cached = cache.is_some_and(Path::is_file);
    let start = Instant::now();
    let streets = [nlh::Street::Flop, nlh::Street::Turn, nlh::Street::River];
    let table = Ehs2Abstraction::load_or_build(params, &streets, cache);
    on_ready(AbstractionReady {
        cached,
        secs: start.elapsed().as_secs_f64(),
    });
    Ok((
        TableAbstractionAdapter::new(table, ehs2_table_fingerprint(params)),
        AbstractionReady {
            cached,
            secs: start.elapsed().as_secs_f64(),
        },
    ))
}

/// Sweeps until the next `cadence` boundary (a full `cadence` when already
/// on one). Both the CLI and GUI drive loops size their chunks with this so
/// evaluation/checkpoint cadences fire exactly on their configured multiples.
pub(crate) fn distance_to_boundary(current: u64, cadence: u64) -> u64 {
    cadence - current % cadence
}

/// Assembles one `MultiwayMetricsRow` from solver metrics, drift, elapsed
/// time, and an optional held-out profile evaluation.
pub(crate) fn metrics_row(
    metrics: &crate::SolverMetrics,
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
                candidate_policy_coverage: evaluation
                    .and_then(|value| value.candidate_policy_coverage.get(seat))
                    .map(policy_coverage),
            })
            .collect(),
    }
}

fn profile_estimate(value: &crate::solver::ProfileEstimate) -> Estimate {
    Estimate {
        mean: value.mean,
        stderr: value.stderr,
        ci95: value.ci95,
    }
}

fn policy_coverage(
    value: &crate::CandidatePolicyCoverage,
) -> crate::metrics::MultiwayPolicyCoverage {
    let by_street = |counts: crate::StreetVisitCounts| crate::metrics::MultiwayStreetVisitCounts {
        preflop: counts.preflop,
        flop: counts.flop,
        turn: counts.turn,
        river: counts.river,
    };
    crate::metrics::MultiwayPolicyCoverage {
        decision_visits: value.decision_visits,
        stored_strategy_visits: value.stored_strategy_visits,
        uniform_fallback_visits: value.uniform_fallback_visits,
        average_strategy_visits: value.average_strategy_visits,
        current_strategy_visits: value.current_strategy_visits,
        regret_fallback_visits: value.regret_fallback_visits,
        decision_visits_by_street: by_street(value.decision_visits_by_street),
        stored_strategy_visits_by_street: by_street(value.stored_strategy_visits_by_street),
        uniform_fallback_visits_by_street: by_street(value.uniform_fallback_visits_by_street),
        average_strategy_visits_by_street: by_street(value.average_strategy_visits_by_street),
        current_strategy_visits_by_street: by_street(value.current_strategy_visits_by_street),
        regret_fallback_visits_by_street: by_street(value.regret_fallback_visits_by_street),
    }
}

fn public_action(action: &crate::Action) -> MultiwayPublicAction {
    match action {
        crate::Action::Fold => MultiwayPublicAction::Fold,
        crate::Action::Check => MultiwayPublicAction::Check,
        crate::Action::Call { amount, all_in } => MultiwayPublicAction::Call {
            amount_millibb: amount.raw(),
            all_in: *all_in,
        },
        crate::Action::BetTo {
            to,
            all_in,
            full_raise,
        } => MultiwayPublicAction::BetTo {
            amount_millibb: to.raw(),
            all_in: *all_in,
            full_raise: *full_raise,
        },
        crate::Action::RaiseTo {
            to,
            all_in,
            full_raise,
        } => MultiwayPublicAction::RaiseTo {
            amount_millibb: to.raw(),
            all_in: *all_in,
            full_raise: *full_raise,
        },
    }
}

fn make_public_tree(
    game: &HoldemGame<MultiwayAbstractionBackend>,
) -> (Vec<MultiwayHistoryNode>, Vec<MultiwayPublicState>) {
    let root = crate::solver::ExternalSamplingGame::root_state(game);
    let mut pending = vec![(crate::solver::HistoryKey::ROOT, root)];
    let mut histories = Vec::new();
    let mut public_states = Vec::new();

    while let Some((history, state)) = pending.pop() {
        let actor = state.to_act.map(|seat| seat.0);
        let actions = game.legal_actions(&state);
        public_states.push(MultiwayPublicState {
            history: history.0,
            street: state.street.index() as u8,
            actor,
            pot_millibb: state.pot_size().raw(),
            remaining_stacks_millibb: state
                .seats
                .iter()
                .map(|seat| seat.remaining.raw())
                .collect(),
            legal_actions: actions.iter().map(public_action).collect(),
        });
        let Some(actor) = actor else {
            continue;
        };
        for (action_index, action) in actions.iter().enumerate().rev() {
            let child = history.child(actor as usize, action_index);
            histories.push(MultiwayHistoryNode {
                key: child.0,
                parent: history.0,
                actor,
                action_index: action_index as u32,
                action: public_action(action).label(),
            });
            let next = crate::solver::ExternalSamplingGame::next_state_with(
                game,
                &state,
                &actions,
                action_index,
            );
            pending.push((child, next));
        }
    }

    histories.sort_by_key(|entry| entry.key);
    public_states.sort_by_key(|state| state.history);
    (histories, public_states)
}

/// Identifies both configured algorithm settings and the numerical update
/// semantics. Shared by the run summary and `.mwsol` writers so artifacts
/// from before a solver-state correction remain distinguishable.
pub(crate) fn multiway_algorithm_fingerprint(algorithm: &SolverConfig) -> Result<[u8; 32]> {
    // Retain the existing artifact identity material without a legacy config type.
    #[derive(serde::Serialize)]
    struct Identity {
        schedule: &'static str,
        seed: u64,
        exploration_epsilon: f64,
        discount_every: u64,
        discount_until: u64,
        #[serde(skip_serializing_if = "is_false")]
        traverser_vector: bool,
        #[serde(skip_serializing_if = "is_false")]
        prune: bool,
        #[serde(skip_serializing_if = "is_default_skip")]
        prune_skip_probability: f64,
    }
    fn is_false(value: &bool) -> bool {
        !value
    }
    fn is_default_skip(value: &f64) -> bool {
        *value == crate::solver::DEFAULT_PRUNE_SKIP_PROBABILITY
    }
    let material = serde_json::to_vec(&Identity {
        schedule: "external-sampling-mccfr",
        seed: algorithm.seed,
        exploration_epsilon: algorithm.exploration_epsilon,
        discount_every: algorithm.discount_every,
        discount_until: algorithm.discount_until,
        traverser_vector: algorithm.traverser_vector,
        prune: algorithm.prune,
        prune_skip_probability: algorithm.prune_skip_probability,
    })?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.algorithm.v1");
    hasher.update(&crate::solver::SOLVER_STATE_VERSION.to_le_bytes());
    hasher.update(&material);
    Ok(*hasher.finalize().as_bytes())
}

/// Builds the exportable `.mwsol` artifact from a solver state snapshot and
/// its final metrics row. Only visited infosets are formal solution entries;
/// absent keys remain explicitly unvisited rather than becoming uniform.
/// Shared by every caller that persists a multiway solve's average strategy.
pub(crate) fn make_solution(
    config_toml: &str,
    abstraction_fingerprint: [u8; 32],
    configuration_fingerprint: [u8; 32],
    game: &HoldemGame<MultiwayAbstractionBackend>,
    state: &crate::solver::SolverState,
    row: &MultiwayMetricsRow,
) -> MultiwaySolution {
    let effective = crate::prepare::prepare(config_toml, Path::new("embedded.toml"))
        .expect("solution config was validated before solving");
    let algorithm_fingerprint = multiway_algorithm_fingerprint(&effective.lowered.solver)
        .expect("effective algorithm is serializable");
    let (histories, public_states) = make_public_tree(game);
    let strategy_weights = state
        .policies
        .iter()
        .filter(|entry| entry.column.strategy_sum.iter().any(|value| *value > 0.0))
        .map(|entry| MultiwayStrategyWeight {
            key: MultiwayStrategyKey {
                history: entry.key.history.0,
                actor: entry.key.player,
                street: entry.key.street,
                active_opponents: entry.key.active_opponents,
                bucket_path: entry.key.bucket_path,
            },
            weight: entry
                .column
                .strategy_sum
                .iter()
                .map(|value| f64::from(*value))
                .sum(),
        })
        .collect();
    let strategies = state
        .policies
        .iter()
        .filter(|entry| entry.column.strategy_sum.iter().any(|value| *value > 0.0))
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
        config_fingerprint: runfiles::config_hash(config_toml.as_bytes()),
        game_fingerprint: game.game_fingerprint(),
        algorithm_fingerprint,
        abstraction_fingerprint,
        configuration_fingerprint,
        stop_status: row.phase.clone(),
        chip_unit_bb: 0.001,
        sweeps: row.sweeps,
        approximate_profile: true,
        histories,
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
        public_states,
        strategy_weights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SMOKE: &str = include_str!("../tests/fixtures/preflop_multiway_v1_3max_smoke.toml");
    #[test]
    fn progress_policy_coverage_reports_only_the_held_out_baseline() {
        let session = build_multiway_session(SMOKE, None).unwrap();
        let evaluation = session.solver.evaluate_average_profile(16, 5041).unwrap();
        let metrics = session.solver.metrics();
        let row = metrics_row(&metrics, vec![0.0; 3], 1.0, Some(&evaluation));
        let json = serde_json::to_value(&row).unwrap();
        let mut total_visits = 0;
        for (seat, expected) in evaluation.candidate_policy_coverage.iter().enumerate() {
            let observed = row.seats[seat].candidate_policy_coverage.as_ref().unwrap();
            total_visits += observed.decision_visits;
            assert_eq!(observed.decision_visits, expected.decision_visits);
            assert_eq!(observed.uniform_fallback_visits, observed.decision_visits);
            assert_eq!(observed.stored_strategy_visits, 0);
            assert_eq!(observed.average_strategy_visits, 0);
            assert_eq!(observed.current_strategy_visits, 0);
            assert_eq!(observed.regret_fallback_visits, 0);
            assert_eq!(
                observed.uniform_fallback_visits_by_street,
                observed.decision_visits_by_street
            );
            assert_eq!(
                json["seats"][seat]["candidatePolicyCoverage"]["uniformFallbackVisitsByStreet"]["flop"],
                expected.uniform_fallback_visits_by_street.flop
            );
        }
        assert!(total_visits > 0);
        let without_evaluation = metrics_row(&metrics, vec![0.0; 3], 1.0, None);
        assert!(
            without_evaluation
                .seats
                .iter()
                .all(|seat| seat.candidate_policy_coverage.is_none())
        );
    }

    #[test]
    fn stop_checks_use_fresh_held_out_batches_and_resume_the_sequence() {
        let session =
            build_multiway_session(SMOKE, None).expect("build cheap frozen-profile fixture");
        let rule = StopRule {
            dev_gain_threshold: 1.0e6,
            confirmations: 3,
            br_traversals: 0,
        };
        let mut state = StopRuleState::new(32);
        let seed = 29;
        let first = run_stop_rule_check(&session.solver, &rule, &mut state, 3, 1, seed).unwrap();
        let expected = session
            .solver
            .evaluate_average_profile(32, stop_check_seeds(seed, 0).1)
            .unwrap();
        assert_eq!(first.evaluation, expected);
        assert!(!first.converged);

        // This is exactly the persisted stop-state subset restored by the
        // drive loop, without involving wall-clock time in reproducibility.
        let mut resumed = StopRuleState::new(state.samples);
        resumed.eval_index = state.eval_index;
        resumed.confirmations_met = state.confirmations_met;
        let second = run_stop_rule_check(&session.solver, &rule, &mut state, 3, 1, seed).unwrap();
        let replay = run_stop_rule_check(&session.solver, &rule, &mut resumed, 3, 1, seed).unwrap();
        assert_eq!(second.evaluation, replay.evaluation);
        assert_eq!(
            second.evaluation,
            session
                .solver
                .evaluate_average_profile(32, stop_check_seeds(seed, 1).1)
                .unwrap()
        );
        assert_eq!(state.eval_index, resumed.eval_index);
        assert_ne!(first.evaluation, second.evaluation);
        assert_eq!(state.confirmations_met, 2);

        let mut streams = std::collections::HashSet::new();
        for sequence in 0..64 {
            let (training, held_out) = stop_check_seeds(seed, sequence);
            assert!(streams.insert(training));
            assert!(streams.insert(held_out));
        }
    }

    #[test]
    fn stop_target_includes_equality_at_the_upper_bound() {
        let session = build_multiway_session(SMOKE, None).unwrap();
        let mut state = StopRuleState::new(32);
        let seed = 31;
        let expected = session
            .solver
            .evaluate_average_profile(32, stop_check_seeds(seed, 0).1)
            .unwrap();
        let target = expected
            .deviation_gain_lower_bound
            .unwrap()
            .iter()
            .map(|estimate| estimate.ci95[1])
            .fold(0.0, f64::max);
        let rule = StopRule {
            dev_gain_threshold: target,
            confirmations: 1,
            br_traversals: 0,
        };
        let check = run_stop_rule_check(&session.solver, &rule, &mut state, 3, 1, seed).unwrap();
        assert_eq!(check.max_upper, target);
        assert!(check.converged);
    }

    #[test]
    fn stop_check_requires_variance_samples_and_rejects_sequence_overflow() {
        let session = build_multiway_session(SMOKE, None).unwrap();
        let rule = StopRule {
            dev_gain_threshold: 1.0e6,
            confirmations: 1,
            br_traversals: 0,
        };
        let mut state = StopRuleState::new(1);
        let check = run_stop_rule_check(&session.solver, &rule, &mut state, 3, 1, 17).unwrap();
        assert_eq!(check.evaluation.samples, 2);
        assert_eq!(state.samples, 2);
        state.eval_index = u64::MAX;
        let error = run_stop_rule_check(&session.solver, &rule, &mut state, 3, 1, 17)
            .err()
            .expect("sequence exhaustion must fail rather than reuse a batch");
        assert!(error.to_string().contains("sequence overflow"));
    }

    #[test]
    fn boundaries_are_positive_and_repeat() {
        assert_eq!(distance_to_boundary(0, 100), 100);
        assert_eq!(distance_to_boundary(99, 100), 1);
        assert_eq!(distance_to_boundary(100, 100), 100);
    }

    #[test]
    fn typed_production_sessions_and_resume_preserve_thread_independent_state() {
        use crate::checkpoint::MultiwayCheckpoint;
        let mut p = crate::prepare::prepare(SMOKE, Path::new("smoke.toml")).unwrap();
        p.lowered.run.stop.max_sweeps = 8;
        let mut serial_input = p.lowered.clone();
        serial_input.run.threads = 1;
        let mut parallel_input = p.lowered;
        parallel_input.run.threads = 4;
        let build =
            |input, checkpoint| build_test_session(input, p.effective.clone(), checkpoint).unwrap();
        let mut serial = build(serial_input, None);
        let mut parallel = build(parallel_input.clone(), None);
        assert_eq!(
            serial.solver.policy_arena_allocation(),
            parallel.solver.policy_arena_allocation()
        );
        assert!(
            serial
                .solver
                .policy_arena_allocation()
                .unwrap()
                .pages_committed
        );
        serial
            .solver
            .run_sweeps_with_threads(4, serial.threads)
            .unwrap();
        parallel
            .solver
            .run_sweeps_with_threads(4, parallel.threads)
            .unwrap();
        let checkpoint = MultiwayCheckpoint::capture(&serial.solver);
        assert_eq!(checkpoint, MultiwayCheckpoint::capture(&parallel.solver));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("parallel.mwckpt");
        checkpoint.write_atomic(&path).unwrap();
        let mut resumed = build(parallel_input, Some(path.as_path()));
        assert_eq!(checkpoint, MultiwayCheckpoint::capture(&resumed.solver));
        serial
            .solver
            .run_sweeps_with_threads(2, serial.threads)
            .unwrap();
        resumed
            .solver
            .run_sweeps_with_threads(2, resumed.threads)
            .unwrap();
        assert_eq!(
            MultiwayCheckpoint::capture(&serial.solver),
            MultiwayCheckpoint::capture(&resumed.solver)
        );
    }
}
