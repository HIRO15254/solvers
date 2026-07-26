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

use std::path::{Path, PathBuf};
use std::time::Instant;

use abstraction::{Ehs2Abstraction, Ehs2Params};
use anyhow::{Context, Result, anyhow};
use formats::{
    Estimate, MULTIWAY_SCHEMA_VERSION, MultiwayHistoryNode, MultiwayMetricsRow,
    MultiwayPublicAction, MultiwayPublicState, MultiwaySeatMetrics, MultiwaySeatResult,
    MultiwaySolution, MultiwayStrategyBlock, MultiwayStrategyKey, MultiwayStrategyWeight,
};
use multiway::abstraction::{
    BucketContext, BucketId, MultiwayAbstraction, MultiwayAbstractionBackend,
    TableAbstractionAdapter, ehs2_table_fingerprint,
};
#[cfg(any(feature = "research", test))]
use multiway::abstraction::{
    RolloutKMeansAbstraction, RolloutKMeansBuilder, RolloutKMeansParams, StreetBucketCounts,
};
use multiway::checkpoint::MultiwayCheckpoint;
use multiway::config::{
    AbstractionConfig, AbstractionKind, FieldPlayerConfig, RakeConfig as MultiwayRake, RecallMode,
    UtilityConfig as MultiwayUtility,
};
use multiway::solver::{DEFAULT_PRUNE_THRESHOLD, ProfileEvaluation, SolverConfig};
use multiway::{DealSampler, ExternalSamplingGame, HoldemGame, MultiwaySolver, Street};
use rayon::prelude::*;

use crate::config::{
    AlgorithmSection, GameSection, RakeSection, SolveConfig, StorageKind, UtilitySection,
};

const DEFAULT_MEMORY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;

/// Resolved `run.stop_dev_gain`/`run.stop_confirmations`/
/// `run.stop_eval_period_secs` convergence stop rule; see
/// `crate::config::RunSection::stop_dev_gain` for the full semantics.
/// `None` in [`MultiwaySession::stop_rule`] means the rule is disabled and
/// `sweeps_target` is a plain target rather than a safety cap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopRule {
    /// Threshold, in the run's own utility unit, compared against the
    /// maximum per-seat `deviation_gain_lower_bound` CI upper bound.
    pub dev_gain_threshold: f64,
    /// Consecutive passing evaluations required before stopping.
    pub confirmations: u32,
    /// Wall-clock period, in seconds, between stop-rule evaluations.
    pub eval_period_secs: f64,
    /// Best-response training traversals per seat per stop-rule evaluation
    /// (see `crate::config::RunSection::stop_br_traversals`). `0` disables
    /// the burst.
    pub br_traversals: u64,
}

/// Everything needed to run (or resume) a multiway solve: the constructed
/// solver plus the run parameters resolved from `[run]`.
pub struct MultiwaySession {
    pub solver: MultiwaySolver<HoldemGame<MultiwayAbstractionBackend>>,
    pub sweeps_target: u64,
    pub threads: usize,
    pub evaluation_cadence: u64,
    pub evaluation_samples: u64,
    pub evaluation_seed: u64,
    pub checkpoint_every: Option<u64>,
    pub storage: StorageKind,
    /// Convergence-based stop rule; see [`StopRule`]. `None` unless
    /// `run.stop_dev_gain` is set.
    pub stop_rule: Option<StopRule>,
    /// The exact config text this session was built from, unmodified.
    pub config_toml: String,
    /// Blake3 hash of `config_toml`'s raw bytes (see `formats::config_hash`).
    pub config_hash: [u8; 32],
    /// The multiway game config (seats, blinds, betting), kept around for
    /// display purposes (seat names/positions, button seat).
    pub game_config: multiway::MultiwayConfig,
    pub checkpoint_runtime: Option<multiway::checkpoint::CheckpointRuntimeState>,
    /// Evaluation-only common abstraction metadata. This is absent from
    /// ordinary solve/resume sessions and never contributes to checkpoint
    /// compatibility.
    pub deviation_reference: Option<DeviationReferenceMetadata>,
}

/// Identity and persistence location of an evaluation-only common
/// abstraction attached by [`build_multiway_session_with_deviation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviationReferenceMetadata {
    pub config_hash: [u8; 32],
    pub game_fingerprint: [u8; 32],
    /// Backend plus reference recall semantics, composed by the same helper
    /// used for a primary solver abstraction fingerprint.
    pub abstraction_fingerprint: [u8; 32],
    pub recall: RecallMode,
    pub artifact_cache: Option<PathBuf>,
}

/// Resource facts derived from the production public tree and configured
/// bucket counts without allocating the policy arena or building EHS² tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiwayResourcePreflight {
    pub complete: bool,
    pub recall: RecallMode,
    pub decision_nodes: u64,
    pub terminal_edges: Option<u64>,
    pub icm: Option<MultiwayIcmPreflight>,
    pub policy_columns: Option<u64>,
    pub policy_slots: Option<u64>,
    pub solver_state_bytes: Option<u64>,
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
            return cards::NUM_CLASSES as u32;
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
/// non-retaining tree preflight used by the desktop GUI.
pub fn preflight_multiway_config(raw_toml: &str) -> Result<MultiwayResourcePreflight> {
    crate::multiway_v1::validate_production_contract(raw_toml)
        .context("validating production Multiway Preflop contract")?;
    let config =
        crate::config::parse_solve_config(raw_toml).context("parsing config for preflight")?;
    let SolveConfig {
        game,
        rake,
        utility,
        run,
        ..
    } = config;
    let GameSection::PreflopMultiway(game_config) = game else {
        return Err(anyhow!(
            "multiway resource preflight requires kind = \"preflop-multiway\""
        ));
    };

    let utility = convert_utility(utility)?;
    let rake = convert_rake(rake);
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
            if field_players <= multiway::icm::EXACT_ICM_MAX_PLAYERS {
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
                let prepared_bytes = multiway::icm::prepared_race_memory_bytes(
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
                    prepared_limit_bytes: Some(multiway::icm::MAX_PREPARED_RACE_BYTES as u64),
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
    let _sampler = game
        .deal_sampler()
        .context("compiling table ranges for preflight")?;
    let memory_limit = run
        .max_memory_bytes
        .filter(|bytes| *bytes != u64::MAX)
        .unwrap_or(crate::multiway_v1::PRODUCTION_POLICY_ARENA_LIMIT_BYTES);
    let arena = match multiway::tree::preflight_arena(&game, memory_limit) {
        Ok(arena) => arena,
        Err(multiway::tree::TreeError::MemoryLimit {
            node_count,
            total_columns,
            needed,
            ..
        }) => {
            return Ok(MultiwayResourcePreflight {
                complete: false,
                recall,
                decision_nodes: node_count as u64,
                terminal_edges: None,
                icm,
                policy_columns: Some(total_columns),
                policy_slots: None,
                solver_state_bytes: Some(needed),
            });
        }
        Err(error) => {
            return Err(error).context("counting public tree and sizing the policy arena");
        }
    };

    Ok(MultiwayResourcePreflight {
        complete: true,
        recall,
        decision_nodes: arena.node_count as u64,
        terminal_edges: Some(arena.terminal_edges),
        icm,
        policy_columns: Some(arena.total_columns),
        policy_slots: Some(arena.total_slots),
        solver_state_bytes: Some(arena.estimated_arena_bytes),
    })
}

/// Default [`StopRule::confirmations`] and [`StopRule::eval_period_secs`]
/// when `run.stop_dev_gain` is set but the corresponding key is omitted.
const DEFAULT_STOP_CONFIRMATIONS: u32 = 2;
const DEFAULT_STOP_EVAL_PERIOD_SECS: f64 = 30.0;
/// Default [`StopRule::br_traversals`] when `run.stop_dev_gain` is set but
/// `run.stop_br_traversals` is omitted.
const DEFAULT_STOP_BR_TRAVERSALS: u64 = 2_000;

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
    build_multiway_session_internal(
        raw_toml,
        None,
        resume_checkpoint,
        SessionStoragePolicy::Compatibility,
    )
}

/// Production solve/resume constructor. It accepts only the fixed
/// EHS2/current-street contract and returns only after the complete policy
/// arena has been allocated and page-committed.
pub fn build_production_multiway_session(
    raw_toml: &str,
    resume_checkpoint: Option<&Path>,
) -> Result<MultiwaySession> {
    build_multiway_session_internal(
        raw_toml,
        None,
        resume_checkpoint,
        SessionStoragePolicy::PreallocatedProduction,
    )
}

/// Builds a candidate session with a separately configured common
/// abstraction used only for deviator training/evaluation.
///
/// The reference's public game, ranges, rake, and utility must have the exact
/// same game fingerprint as the candidate. Its algorithm/run sections are not
/// used. The reference is attached before checkpoint restore; core
/// fingerprints intentionally continue to describe only the candidate.
pub fn build_multiway_session_with_deviation(
    raw_toml: &str,
    deviation_raw_toml: &str,
    resume_checkpoint: Option<&Path>,
) -> Result<MultiwaySession> {
    build_multiway_session_internal(
        raw_toml,
        Some(deviation_raw_toml),
        resume_checkpoint,
        SessionStoragePolicy::Compatibility,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionStoragePolicy {
    Compatibility,
    PreallocatedProduction,
}

fn build_multiway_session_internal(
    raw_toml: &str,
    deviation_raw_toml: Option<&str>,
    resume_checkpoint: Option<&Path>,
    storage_policy: SessionStoragePolicy,
) -> Result<MultiwaySession> {
    let config: SolveConfig =
        crate::config::parse_solve_config(raw_toml).context("parsing config")?;
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
    if matches!(storage_policy, SessionStoragePolicy::PreallocatedProduction) {
        validate_production_abstraction(&game_config)?;
        if deviation_raw_toml.is_some() {
            return Err(anyhow!(
                "production sessions do not accept a separately configured deviation abstraction"
            ));
        }
    }
    if run.target_nash_conv.is_some() {
        return Err(anyhow!(
            "multiway profiles do not expose NashConv; remove run.target_nash_conv"
        ));
    }
    let utility = convert_utility(utility)?;
    let rake = convert_rake(rake);
    let (mut game, sampler) = build_multiway_game_from_config(&game_config, &utility, &rake)?;
    let mut deviation_reference = deviation_raw_toml
        .map(|raw| build_deviation_reference(raw, game.game_fingerprint()))
        .transpose()
        .context("building common deviation reference")?;
    if let Some(reference) = deviation_reference.as_mut() {
        let recall = reference.metadata.recall;
        game = game.with_deviation_abstraction(reference.backend(), recall);
    }

    let (
        algorithm_seed,
        exploration_epsilon,
        discount_every,
        discount_until,
        traverser_vector,
        prune,
        prune_threshold_override,
        prune_skip_probability,
    ) = match algorithm {
        AlgorithmSection::ExternalSamplingMccfr {
            seed,
            exploration_epsilon,
            discount_every,
            discount_until,
            traverser_vector,
            prune,
            prune_threshold,
            prune_skip_probability,
        } => (
            seed,
            exploration_epsilon,
            discount_every,
            discount_until,
            traverser_vector,
            prune,
            prune_threshold,
            prune_skip_probability,
        ),
        _ => {
            return Err(anyhow!(
                "preflop-multiway requires schedule = \"external-sampling-mccfr\""
            ));
        }
    };
    if prune && !traverser_vector {
        return Err(anyhow!(
            "algorithm.prune requires algorithm.traverser_vector = true"
        ));
    }
    if let Some(threshold) = prune_threshold_override
        && (!threshold.is_finite() || threshold >= 0.0)
    {
        return Err(anyhow!(
            "algorithm.prune_threshold must be finite and strictly negative, found {threshold}"
        ));
    }
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
    if run
        .stop_dev_gain
        .is_some_and(|threshold| !threshold.is_finite() || threshold <= 0.0)
    {
        return Err(anyhow!(
            "run.stop_dev_gain must be finite and positive when supplied"
        ));
    }
    if run.stop_confirmations == Some(0) {
        return Err(anyhow!(
            "run.stop_confirmations must be positive when supplied"
        ));
    }
    if run
        .stop_eval_period_secs
        .is_some_and(|period| !period.is_finite() || period <= 0.0)
    {
        return Err(anyhow!(
            "run.stop_eval_period_secs must be finite and positive when supplied"
        ));
    }
    let stop_rule = run.stop_dev_gain.map(|dev_gain_threshold| StopRule {
        dev_gain_threshold,
        confirmations: run.stop_confirmations.unwrap_or(DEFAULT_STOP_CONFIRMATIONS),
        eval_period_secs: run
            .stop_eval_period_secs
            .unwrap_or(DEFAULT_STOP_EVAL_PERIOD_SECS),
        br_traversals: run.stop_br_traversals.unwrap_or(DEFAULT_STOP_BR_TRAVERSALS),
    });

    let evaluation_samples = run.evaluation_samples.unwrap_or(256);
    let prune_threshold = if !prune {
        // Ignored by the engine when `prune` is false; keep the documented
        // default so a disabled-pruning solver config is still valid input.
        DEFAULT_PRUNE_THRESHOLD
    } else {
        prune_threshold_override.unwrap_or_else(|| derive_prune_threshold(&utility, &game_config))
    };
    let max_memory_bytes = resolve_policy_memory_limit(storage_policy, run.max_memory_bytes)?;
    let solver_config = SolverConfig {
        seed: run.seed.unwrap_or(algorithm_seed),
        max_memory_bytes,
        max_traversal_depth: 512,
        exploration_epsilon,
        discount_every,
        discount_until,
        sweep_batch: run.sweep_batch.unwrap_or(1),
        traverser_vector,
        prune,
        prune_threshold,
        prune_skip_probability,
    };
    let evaluation_seed = solver_config.seed ^ 0x6576_616c_7561_7465;

    let mut checkpoint_runtime = None;
    let solver = if let Some(path) = resume_checkpoint {
        let expected_configuration =
            multiway::solver::configuration_fingerprint_for_setup(&game, &sampler, solver_config);
        let expected_abstraction = multiway::abstraction_fingerprint_with_recall(
            game.abstraction_fingerprint(),
            game.recall_mode(),
        );
        let checkpoint =
            MultiwayCheckpoint::load(path, expected_configuration, expected_abstraction)
                .with_context(|| format!("reading multiway checkpoint {}", path.display()))?;
        checkpoint_runtime = checkpoint.config_toml.as_ref().map(|_| checkpoint.runtime);
        let solver = match storage_policy {
            SessionStoragePolicy::Compatibility => MultiwaySolver::from_state_with_config(
                game,
                sampler,
                checkpoint.state,
                solver_config,
            ),
            SessionStoragePolicy::PreallocatedProduction => {
                MultiwaySolver::from_state_with_config_preallocated(
                    game,
                    sampler,
                    checkpoint.state,
                    solver_config,
                )
            }
        }
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
        match storage_policy {
            SessionStoragePolicy::Compatibility => {
                MultiwaySolver::new(game, sampler, solver_config)
            }
            SessionStoragePolicy::PreallocatedProduction => {
                MultiwaySolver::new_preallocated(game, sampler, solver_config)
            }
        }
        .context("initializing multiway MCCFR")?
    };

    if matches!(storage_policy, SessionStoragePolicy::PreallocatedProduction) {
        let allocation = solver.policy_arena_allocation().ok_or_else(|| {
            anyhow!("production solver returned without a preallocated policy arena")
        })?;
        if !allocation.pages_committed {
            return Err(anyhow!(
                "production solver returned before policy arena pages were committed"
            ));
        }
    }

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
        stop_rule,
        config_toml: raw_toml.to_string(),
        config_hash: formats::config_hash(raw_toml.as_bytes()),
        game_config,
        checkpoint_runtime,
        deviation_reference: deviation_reference.map(|reference| reference.metadata),
    })
}

fn resolve_policy_memory_limit(
    storage_policy: SessionStoragePolicy,
    configured: Option<u64>,
) -> Result<u64> {
    if matches!(storage_policy, SessionStoragePolicy::Compatibility) {
        return Ok(configured.unwrap_or(DEFAULT_MEMORY_LIMIT));
    }
    let resolved = match configured {
        None | Some(u64::MAX) => crate::multiway_v1::PRODUCTION_POLICY_ARENA_LIMIT_BYTES,
        Some(bytes) => bytes,
    };
    if resolved > crate::multiway_v1::PRODUCTION_POLICY_ARENA_LIMIT_BYTES {
        return Err(anyhow!(
            "production policy arena memory limit is 6GiB ({} bytes); lower \
             run.resources.memory and enforce the separate 8GiB process RSS limit externally",
            crate::multiway_v1::PRODUCTION_POLICY_ARENA_LIMIT_BYTES
        ));
    }
    Ok(resolved)
}

fn validate_production_abstraction(game: &multiway::MultiwayConfig) -> Result<()> {
    if !matches!(game.abstraction.kind, AbstractionKind::Ehs2Table) {
        return Err(anyhow!(
            "MWP001: rollout-kmeans was removed from production because its assignment cache \
             grows during Solve; use canonical v1 kind = \"ehs2-percentile\""
        ));
    }
    if !matches!(game.abstraction.recall, RecallMode::Street) {
        return Err(anyhow!(
            "MWP002: full/bucket-history recall was removed from production because sparse \
             policy memory grows during Solve; production requires current-street preallocation"
        ));
    }
    if !game.abstraction.active_opponent_buckets.is_empty() {
        return Err(anyhow!(
            "MWP001: active-opponent bucket overrides are rollout-only and unavailable in production"
        ));
    }
    Ok(())
}

struct BuiltDeviationReference {
    backend: Option<MultiwayAbstractionBackend>,
    metadata: DeviationReferenceMetadata,
}

impl BuiltDeviationReference {
    fn backend(&mut self) -> MultiwayAbstractionBackend {
        self.backend
            .take()
            .expect("reference backend is moved into the candidate exactly once")
    }
}

fn build_deviation_reference(
    raw_toml: &str,
    candidate_game_fingerprint: [u8; 32],
) -> Result<BuiltDeviationReference> {
    let config: SolveConfig =
        crate::config::parse_solve_config(raw_toml).context("parsing reference config")?;
    let SolveConfig {
        game,
        rake,
        utility,
        algorithm: _,
        run: _,
    } = config;
    let GameSection::PreflopMultiway(game_config) = game else {
        return Err(anyhow!(
            "common deviation reference requires kind = \"preflop-multiway\""
        ));
    };
    let utility = convert_utility(utility)?;
    let rake = convert_rake(rake);
    let (reference_game, _) = build_multiway_game_from_config(&game_config, &utility, &rake)?;
    let reference_game_fingerprint = reference_game.game_fingerprint();
    if candidate_game_fingerprint != reference_game_fingerprint {
        return Err(anyhow!(
            "candidate/reference game fingerprint mismatch: candidate={}, reference={}",
            formats::config_hash_hex(&candidate_game_fingerprint),
            formats::config_hash_hex(&reference_game_fingerprint)
        ));
    }
    let recall = game_config.abstraction.recall;
    let abstraction_fingerprint = multiway::abstraction_fingerprint_with_recall(
        reference_game.abstraction_fingerprint(),
        recall,
    );
    Ok(BuiltDeviationReference {
        backend: Some(reference_game.into_abstraction()),
        metadata: DeviationReferenceMetadata {
            config_hash: formats::config_hash(raw_toml.as_bytes()),
            game_fingerprint: reference_game_fingerprint,
            abstraction_fingerprint,
            recall,
            artifact_cache: game_config.abstraction.artifact_cache,
        },
    })
}

/// Scale factor `derive_prune_threshold` applies to the game's total stakes
/// to get `algorithm.prune_threshold` when the key is omitted. See
/// `derive_prune_threshold`'s doc comment for how this scale was
/// calibrated.
pub(crate) const PRUNE_THRESHOLD_STAKE_FACTOR: f64 = -10.0;

/// Derives `algorithm.prune_threshold` when `algorithm.prune = true` but the
/// key itself is omitted, from the game's stakes: `-10.0 *` the total
/// starting stacks (in bb) for `[utility] kind = "chip-ev"`, or `-10.0 *`
/// the total payouts for `kind = "tournament-icm"`.
///
/// The `-10x` scale was calibrated empirically (paired 200k-sweep runs on a
/// 6-max 100bb auto-shape config): vector-mode bucket regrets are
/// range-weighted *means* over combos, so they grow orders of magnitude
/// slower than the raw per-hand regrets Pluribus's famous very-negative
/// constant was tuned for. At `-10x` total stacks a (bucket, action) only
/// qualifies after roughly 10k+ sweeps of persistent domination (the ratio
/// is stack-depth-invariant, since per-sweep regret deltas also scale with
/// stack depth), which measurably sped up the paired runs, while `-1000x`
/// essentially never activated within realistic run lengths and its
/// bookkeeping made runs marginally slower. Two safety nets keep the
/// comparatively shallow default honest: ~5% of visits still explore a
/// pruned pair, and every batched early-discount event scales negative
/// regrets back toward zero, periodically lifting borderline pairs above
/// the threshold for a full re-check.
fn derive_prune_threshold(
    utility: &MultiwayUtility,
    game_config: &multiway::MultiwayConfig,
) -> f64 {
    let total = match utility {
        MultiwayUtility::ChipEv => game_config
            .seats
            .iter()
            .map(|seat| seat.stack_bb)
            .sum::<f64>(),
        MultiwayUtility::TournamentIcm { payouts, .. } => payouts.iter().sum::<f64>(),
    };
    PRUNE_THRESHOLD_STAKE_FACTOR * total
}

/// Hard cap on the adaptive stop-rule evaluation sample count: matches the
/// ladder cap documented on `run.stop_dev_gain`, enforced by the CLI drive
/// loop (`multiway_solve::run_inner`) via [`run_stop_rule_check`].
pub(crate) const MAX_STOP_RULE_SAMPLES: u64 = 65_536;

/// Mutable state of the wall-clock convergence stop rule (`run.stop_dev_gain`)
/// across one run, threaded through repeated [`run_stop_rule_check`] calls.
pub struct StopRuleState {
    /// Adaptive evaluation sample count: starts at the session's
    /// `evaluation_samples` and doubles (capped at [`MAX_STOP_RULE_SAMPLES`])
    /// whenever a check's CI is too wide to ever settle below the threshold.
    pub samples: u64,
    /// Consecutive passing evaluations so far.
    pub confirmations_met: u32,
    /// Wall clock of the last stop-rule check; the caller compares this
    /// against `StopRule::eval_period_secs` to decide whether to call
    /// [`run_stop_rule_check`] again.
    pub last_eval: Instant,
    /// Monotonically increasing index of the next check, folded into the
    /// deviator-training seed so repeated checks never retrain against the
    /// same random stream.
    pub eval_index: u64,
}

impl StopRuleState {
    pub fn new(initial_samples: u64) -> Self {
        Self {
            samples: initial_samples,
            confirmations_met: 0,
            last_eval: Instant::now(),
            eval_index: 0,
        }
    }
}

/// Trains one burst deviator per seat in parallel, on a deterministic worker
/// pool sized to `threads` rather than relying on rayon's ambient global
/// pool (whose thread count the config doesn't control). Shared by every
/// caller that trains a per-seat best-response burst: the stop-rule check
/// below, and `mw_eval`'s purification sweep.
pub fn train_deviators_parallel(
    solver: &MultiwaySolver<HoldemGame<MultiwayAbstractionBackend>>,
    num_players: usize,
    threads: usize,
    traversals: u64,
    seed: u64,
    variant: multiway::ProfileVariant,
) -> Result<Vec<multiway::DeviatorPolicy>> {
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

/// Reference-evaluation counterpart to [`train_deviators_parallel`] that
/// retains each seat's training coverage report.
pub fn train_deviators_with_reports_parallel(
    solver: &MultiwaySolver<HoldemGame<MultiwayAbstractionBackend>>,
    num_players: usize,
    threads: usize,
    traversals: u64,
    seed: u64,
    variant: multiway::ProfileVariant,
) -> Result<Vec<multiway::DeviatorTrainingResult>> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| anyhow!("building deviator training thread pool: {error}"))?;
    pool.install(|| {
        (0..num_players)
            .into_par_iter()
            .map(|seat| solver.train_deviator_with_report(seat, traversals, seed, variant))
            .collect::<std::result::Result<Vec<_>, _>>()
    })
    .map_err(|error| anyhow!("training reference deviator: {error}"))
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

/// One stop-rule check: an optional best-response burst, the held-out
/// evaluation, threshold/width bookkeeping, adaptive sample doubling, and
/// the confirmations update. Callers are expected to have already checked
/// their own wall-clock trigger (`state.last_eval.elapsed() >=
/// stop_rule.eval_period_secs`) before calling this; it unconditionally
/// performs one check and resets `state.last_eval`.
pub fn run_stop_rule_check(
    solver: &MultiwaySolver<HoldemGame<MultiwayAbstractionBackend>>,
    stop_rule: &StopRule,
    state: &mut StopRuleState,
    num_players: usize,
    threads: usize,
    evaluation_seed: u64,
) -> Result<StopRuleCheck> {
    state.last_eval = Instant::now();
    // Best-response burst: stop-rule evaluations measure a per-seat deviator
    // TRAINED against the frozen current average profile rather than the
    // plain regret-greedy heuristic the ordinary evaluation-cadence rows
    // use, so the deviation-gain numbers reported here are systematically
    // tighter (higher) than a cadence row taken at the same sweep count.
    // That is intentional: the stop decision should use the strongest
    // available deviator, not the cheap heuristic every metrics row gets.
    let training_seed = evaluation_seed ^ 0x6252_5354 ^ state.eval_index;
    state.eval_index += 1;
    let deviators = if stop_rule.br_traversals > 0 {
        Some(
            train_deviators_parallel(
                solver,
                num_players,
                threads,
                stop_rule.br_traversals,
                training_seed,
                multiway::ProfileVariant::default(),
            )
            .context("training best-response deviators for the convergence stop rule")?,
        )
    } else {
        None
    };
    let evaluation = solver
        .evaluate_profile(
            state.samples,
            evaluation_seed,
            deviators.as_deref(),
            multiway::ProfileVariant::default(),
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

    state.confirmations_met = if max_upper < stop_rule.dev_gain_threshold {
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

/// The `[game]`/`[rake]`/`[utility]`-only core of `build_multiway_session`:
/// builds (or loads/retrains) the configured card abstraction and returns a
/// ready-to-use game plus its deal sampler, without touching
/// `[algorithm]`/`[run]` or constructing a solver.
fn build_multiway_game_from_config(
    game_config: &multiway::MultiwayConfig,
    utility: &MultiwayUtility,
    rake: &MultiwayRake,
) -> Result<(HoldemGame<MultiwayAbstractionBackend>, DealSampler)> {
    game_config
        .validate_economics(utility, rake)
        .context("validating multiway game and utility")?;
    let abstraction = match game_config.abstraction.kind {
        AbstractionKind::RolloutKmeans => {
            #[cfg(any(feature = "research", test))]
            {
                MultiwayAbstractionBackend::RolloutKMeans(build_rollout_abstraction(game_config)?)
            }
            #[cfg(not(any(feature = "research", test)))]
            {
                return Err(anyhow!(
                    "MWP001: rollout-kmeans is unavailable in the production binary; \
                     use kind = \"ehs2-table\" or rebuild with --features research"
                ));
            }
        }
        AbstractionKind::Ehs2Table => {
            MultiwayAbstractionBackend::Ehs2Table(build_ehs2_table_abstraction(game_config)?)
        }
    };
    let game = HoldemGame::new(game_config, utility, rake, abstraction)
        .context("building generative multiway game")?;
    let sampler = game.deal_sampler().context("compiling table ranges")?;
    Ok((game, sampler))
}

/// Builds (or loads, or retrains-and-overwrites) the trained
/// rollout/k-means abstraction for `AbstractionKind::RolloutKmeans`.
/// Factored out of `build_multiway_session` so the two backend kinds don't
/// share one branchy block.
#[cfg(any(feature = "research", test))]
fn build_rollout_abstraction(
    game_config: &multiway::MultiwayConfig,
) -> Result<RolloutKMeansAbstraction> {
    let params = RolloutKMeansParams {
        flop_buckets: u32::from(game_config.abstraction.flop_buckets),
        turn_buckets: u32::from(game_config.abstraction.turn_buckets),
        river_buckets: u32::from(game_config.abstraction.river_buckets),
        rollout_samples: game_config.abstraction.rollout_samples,
        seed: game_config.abstraction.seed,
    };
    let mut abstraction_builder = RolloutKMeansBuilder::new(params)
        .points_per_bucket(game_config.abstraction.points_per_bucket)
        .kmeans_iterations(game_config.abstraction.kmeans_iterations);
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
        // A stale/incompatible artifact (wrong version, corrupt, parameter
        // mismatch, ...) is not fatal: retrain from scratch and overwrite it,
        // the same recovery a missing file already gets below. Only an I/O
        // error during the *write* that follows still propagates.
        let reusable = path
            .is_file()
            .then(|| abstraction_builder.load_artifact(path));
        match reusable {
            Some(Ok(abstraction)) => abstraction,
            other => {
                if let Some(Err(error)) = other {
                    eprintln!(
                        "warning: rollout artifact {} could not be reused ({error}); retraining and overwriting it",
                        path.display()
                    );
                }
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
        }
    } else {
        abstraction_builder
            .build()
            .context("training deterministic multiway rollout abstraction")?
    };
    Ok(abstraction)
}

/// Builds (or loads, or rebuilds-and-overwrites -- see
/// `Ehs2Abstraction::load_or_build`) the precomputed EHS² percentile-table
/// abstraction for `AbstractionKind::Ehs2Table`, reusing the same
/// `artifact_cache` config key the rollout backend uses for its own
/// (differently-shaped) artifact. Unlike the rollout backend, there is no
/// separate assignment cache to persist after a solve: the table is fully
/// determined by `params` at build time.
fn build_ehs2_table_abstraction(
    game_config: &multiway::MultiwayConfig,
) -> Result<TableAbstractionAdapter<Ehs2Abstraction>> {
    let params = Ehs2Params {
        flop_buckets: u32::from(game_config.abstraction.flop_buckets),
        turn_buckets: u32::from(game_config.abstraction.turn_buckets),
        river_buckets: u32::from(game_config.abstraction.river_buckets),
    };
    let cache = game_config.abstraction.artifact_cache.as_deref();
    if let Some(parent) = cache
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating ehs2 table cache directory {}", parent.display()))?;
    }
    let start = Instant::now();
    let streets = [
        cards::Street::Flop,
        cards::Street::Turn,
        cards::Street::River,
    ];
    let table = Ehs2Abstraction::load_or_build(params, &streets, cache);
    println!(
        "ehs2 tables: ready in {:.2}s",
        start.elapsed().as_secs_f64()
    );
    Ok(TableAbstractionAdapter::new(
        table,
        ehs2_table_fingerprint(params),
    ))
}

/// Converts the config-schema `[utility]` section into the solver's
/// `MultiwayUtility`, running cheap `validate_economics` checks without
/// paying for a full `build_multiway_session` (which also trains the card
/// abstraction and builds the game).
pub(crate) fn convert_utility(utility: UtilitySection) -> Result<MultiwayUtility> {
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

/// Converts the config-schema `[rake]` section into the solver's
/// `MultiwayRake`.
pub(crate) fn convert_rake(rake: RakeSection) -> MultiwayRake {
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
        RakeSection::Generic {
            rate,
            cap,
            when,
            allocation,
            rounding,
        } => MultiwayRake::Generic {
            rate,
            cap_bb: cap,
            when,
            allocation,
            rounding,
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

fn public_action(action: &multiway::Action) -> MultiwayPublicAction {
    match action {
        multiway::Action::Fold => MultiwayPublicAction::Fold,
        multiway::Action::Check => MultiwayPublicAction::Check,
        multiway::Action::Call { amount, all_in } => MultiwayPublicAction::Call {
            amount_millibb: amount.raw(),
            all_in: *all_in,
        },
        multiway::Action::BetTo {
            to,
            all_in,
            full_raise,
        } => MultiwayPublicAction::BetTo {
            amount_millibb: to.raw(),
            all_in: *all_in,
            full_raise: *full_raise,
        },
        multiway::Action::RaiseTo {
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
    let root = multiway::solver::ExternalSamplingGame::root_state(game);
    let mut pending = vec![(multiway::solver::HistoryKey::ROOT, root)];
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
            let next = multiway::solver::ExternalSamplingGame::next_state_with(
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

/// Builds the exportable `.mwsol` artifact from a solver state snapshot and
/// its final metrics row. Only visited infosets are formal solution entries;
/// absent keys remain explicitly unvisited rather than becoming uniform.
/// Shared by every caller that persists a multiway solve's average strategy.
pub fn make_solution(
    config_toml: &str,
    abstraction_fingerprint: [u8; 32],
    configuration_fingerprint: [u8; 32],
    game: &HoldemGame<MultiwayAbstractionBackend>,
    state: &multiway::solver::SolverState,
    row: &MultiwayMetricsRow,
) -> MultiwaySolution {
    let effective = crate::config::parse_solve_config(config_toml)
        .expect("solution config was validated before solving");
    let algorithm_material =
        serde_json::to_vec(&effective.algorithm).expect("effective algorithm is serializable");
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
        config_fingerprint: formats::config_hash(config_toml.as_bytes()),
        game_fingerprint: game.game_fingerprint(),
        algorithm_fingerprint: formats::config_hash(&algorithm_material),
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
    use crate::config::RakeSection;
    use multiway::abstraction::MultiwayAbstraction;
    use multiway::solver::DEFAULT_PRUNE_SKIP_PROBABILITY;

    #[test]
    fn boundaries_are_positive_and_repeat() {
        assert_eq!(distance_to_boundary(0, 100), 100);
        assert_eq!(distance_to_boundary(99, 100), 1);
        assert_eq!(distance_to_boundary(100, 100), 100);
    }

    #[test]
    fn production_policy_memory_auto_is_six_gib_and_cannot_exceed_it() {
        let limit = crate::multiway_v1::PRODUCTION_POLICY_ARENA_LIMIT_BYTES;
        assert_eq!(
            resolve_policy_memory_limit(SessionStoragePolicy::PreallocatedProduction, None)
                .unwrap(),
            limit
        );
        assert_eq!(
            resolve_policy_memory_limit(
                SessionStoragePolicy::PreallocatedProduction,
                Some(u64::MAX),
            )
            .unwrap(),
            limit
        );
        assert_eq!(
            resolve_policy_memory_limit(SessionStoragePolicy::PreallocatedProduction, Some(limit),)
                .unwrap(),
            limit
        );
        assert!(
            resolve_policy_memory_limit(
                SessionStoragePolicy::PreallocatedProduction,
                Some(limit + 1),
            )
            .unwrap_err()
            .to_string()
            .contains("6GiB")
        );
        assert_eq!(
            resolve_policy_memory_limit(SessionStoragePolicy::Compatibility, Some(u64::MAX))
                .unwrap(),
            u64::MAX
        );
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
        assert_eq!(session.stop_rule, None);
        assert_eq!(session.deviation_reference, None);
        assert!(session.solver.game().deviation_abstraction().is_none());
    }

    #[test]
    fn common_deviation_reference_attaches_before_checkpoint_restore() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let reference_raw = raw
            .replace("flop_buckets = 8", "flop_buckets = 4")
            .replace("turn_buckets = 8", "turn_buckets = 4")
            .replace("river_buckets = 8", "river_buckets = 4")
            .replace("seed = 17", "seed = 23");
        let directory = tempfile::tempdir().unwrap();
        let checkpoint_path = directory.path().join("candidate.mwckpt");

        let mut original = build_multiway_session(raw, None).unwrap();
        original.solver.run_sweeps(1).unwrap();
        let candidate_configuration = original.solver.configuration_fingerprint();
        let candidate_abstraction = original.solver.abstraction_fingerprint();
        MultiwayCheckpoint::capture(&original.solver)
            .write_atomic(&checkpoint_path)
            .unwrap();

        let restored =
            build_multiway_session_with_deviation(raw, &reference_raw, Some(&checkpoint_path))
                .unwrap();
        assert_eq!(restored.solver.metrics().sweeps, 1);
        assert_eq!(
            restored.solver.configuration_fingerprint(),
            candidate_configuration
        );
        assert_eq!(
            restored.solver.abstraction_fingerprint(),
            candidate_abstraction
        );
        let reference = restored
            .deviation_reference
            .as_ref()
            .expect("reference metadata");
        assert_ne!(reference.abstraction_fingerprint, candidate_abstraction);
        assert_eq!(reference.recall, RecallMode::Full);
        assert!(restored.solver.game().deviation_abstraction().is_some());
    }

    #[test]
    fn common_deviation_reference_rejects_a_different_game() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let different_game = raw.replacen("stack_bb = 2.0", "stack_bb = 3.0", 1);
        let error = build_multiway_session_with_deviation(raw, &different_game, None)
            .err()
            .expect("different game must fail");
        let error = format!("{error:#}");
        assert!(
            error.contains("candidate/reference game fingerprint mismatch"),
            "unexpected error: {error}"
        );
    }

    fn with_stop_dev_gain(dev_gain: &str, extra: &str) -> String {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let anchor = "evaluation_cadence = 1\n";
        let spliced = raw.replacen(
            anchor,
            &format!("{anchor}stop_dev_gain = {dev_gain}\n{extra}"),
            1,
        );
        assert_ne!(spliced, raw, "the splice anchor must have matched");
        spliced
    }

    #[test]
    fn stop_dev_gain_defaults_confirmations_and_period_when_omitted() {
        let raw = with_stop_dev_gain("0.5", "");
        let session = build_multiway_session(&raw, None).expect("build multiway session");
        assert_eq!(
            session.stop_rule,
            Some(StopRule {
                dev_gain_threshold: 0.5,
                confirmations: DEFAULT_STOP_CONFIRMATIONS,
                eval_period_secs: DEFAULT_STOP_EVAL_PERIOD_SECS,
                br_traversals: DEFAULT_STOP_BR_TRAVERSALS,
            })
        );
    }

    #[test]
    fn stop_dev_gain_honors_explicit_confirmations_and_period() {
        let raw = with_stop_dev_gain(
            "0.5",
            "stop_confirmations = 5\nstop_eval_period_secs = 12.5\nstop_br_traversals = 123\n",
        );
        let session = build_multiway_session(&raw, None).expect("build multiway session");
        assert_eq!(
            session.stop_rule,
            Some(StopRule {
                dev_gain_threshold: 0.5,
                confirmations: 5,
                eval_period_secs: 12.5,
                br_traversals: 123,
            })
        );
    }

    /// `MultiwaySession` intentionally does not derive `Debug` (it embeds a
    /// live `MultiwaySolver`), so validation-error tests extract the error
    /// string by hand rather than via `Result::unwrap_err`.
    fn build_multiway_session_err(raw: &str) -> String {
        match build_multiway_session(raw, None) {
            Ok(_) => panic!("expected build_multiway_session to fail"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn stop_dev_gain_rejects_nonpositive_thresholds() {
        for bad in ["0.0", "-1.0"] {
            let raw = with_stop_dev_gain(bad, "");
            let error = build_multiway_session_err(&raw);
            assert!(error.contains("stop_dev_gain"), "{bad}: {error}");
        }
    }

    #[test]
    fn stop_confirmations_zero_is_rejected() {
        let raw = with_stop_dev_gain("0.5", "stop_confirmations = 0\n");
        let error = build_multiway_session_err(&raw);
        assert!(error.contains("stop_confirmations"));
    }

    #[test]
    fn stop_eval_period_secs_nonpositive_is_rejected() {
        let raw = with_stop_dev_gain("0.5", "stop_eval_period_secs = 0.0\n");
        let error = build_multiway_session_err(&raw);
        assert!(error.contains("stop_eval_period_secs"));
    }

    /// Splices `extra` right after `discount_until` in `[algorithm]`,
    /// mirroring `with_stop_dev_gain`'s splice-into-`[run]` helper.
    fn with_algorithm_extra(extra: &str) -> String {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let anchor = "discount_until = 10000000\n";
        let spliced = raw.replacen(anchor, &format!("{anchor}{extra}"), 1);
        assert_ne!(spliced, raw, "the splice anchor must have matched");
        spliced
    }

    /// Pruning is exercised only by the dense street-recall vector worker, so
    /// this helper splices `recall = "street"` into `[game.abstraction]`
    /// alongside `with_algorithm_extra`'s `[algorithm]` splice.
    fn with_algorithm_extra_and_street_recall(extra: &str) -> String {
        let raw = with_algorithm_extra(extra);
        let anchor = "seed = 17\n";
        let spliced = raw.replacen(anchor, &format!("{anchor}recall = \"street\"\n"), 1);
        assert_ne!(spliced, raw, "the recall splice anchor must have matched");
        spliced
    }

    #[test]
    fn prune_without_explicit_threshold_derives_from_chip_ev_stakes() {
        let raw = with_algorithm_extra_and_street_recall("traverser_vector = true\nprune = true\n");
        let session = build_multiway_session(&raw, None).expect("build multiway session");
        let config = session.solver.config();
        assert!(config.prune);
        // The smoke config has 3 seats at 2.0 bb each: -10 * 6.0 = -60.0.
        assert_eq!(config.prune_threshold, -60.0);
        assert_eq!(
            config.prune_skip_probability,
            DEFAULT_PRUNE_SKIP_PROBABILITY
        );
    }

    #[test]
    fn prune_with_explicit_threshold_uses_it_verbatim() {
        let raw = with_algorithm_extra_and_street_recall(
            "traverser_vector = true\nprune = true\nprune_threshold = -42.0\nprune_skip_probability = 0.5\n",
        );
        let session = build_multiway_session(&raw, None).expect("build multiway session");
        let config = session.solver.config();
        assert!(config.prune);
        assert_eq!(config.prune_threshold, -42.0);
        assert_eq!(config.prune_skip_probability, 0.5);
    }

    #[test]
    fn prune_disabled_by_default_and_ignores_the_derivation() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let session = build_multiway_session(raw, None).expect("build multiway session");
        let config = session.solver.config();
        assert!(!config.prune);
        assert_eq!(config.prune_threshold, DEFAULT_PRUNE_THRESHOLD);
    }

    #[test]
    fn prune_without_traverser_vector_is_rejected() {
        let raw = with_algorithm_extra("prune = true\n");
        let error = build_multiway_session_err(&raw);
        assert!(error.contains("traverser_vector"), "{error}");
    }

    #[test]
    fn prune_threshold_nonnegative_is_rejected() {
        let raw =
            with_algorithm_extra("traverser_vector = true\nprune = true\nprune_threshold = 0.0\n");
        let error = build_multiway_session_err(&raw);
        assert!(error.contains("prune_threshold"), "{error}");
    }

    #[test]
    fn stale_rollout_artifact_is_retrained_and_overwritten() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let directory = tempfile::tempdir().unwrap();
        let artifact_path = directory.path().join("rollout.mwab");
        std::fs::write(&artifact_path, b"not a valid rollout artifact").unwrap();

        // Splice `artifact_cache` into `[game.abstraction]` right after the
        // existing `seed` key.
        let literal = format!("{:?}", artifact_path.display().to_string());
        let config_with_cache = raw.replacen(
            "seed = 17\n",
            &format!("seed = 17\nartifact_cache = {literal}\n"),
            1,
        );
        assert_ne!(
            config_with_cache, raw,
            "artifact_cache injection must have matched"
        );

        let session = build_multiway_session(&config_with_cache, None)
            .expect("a stale/corrupt artifact must be retrained rather than hard-erroring");
        assert_eq!(session.sweeps_target, 2);

        let reloaded = RolloutKMeansAbstraction::read_artifact(&artifact_path)
            .expect("the retrained artifact must have overwritten the file and round-trip");
        assert_eq!(
            reloaded.fingerprint(),
            session.solver.abstraction_fingerprint()
        );
    }

    #[test]
    fn rollout_training_config_is_wired_and_invalidates_a_mismatched_artifact() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml")
            .replace("flop_buckets = 8", "flop_buckets = 1")
            .replace("turn_buckets = 8", "turn_buckets = 1")
            .replace("river_buckets = 8", "river_buckets = 1")
            .replace("rollout_samples = 8", "rollout_samples = 1");
        let directory = tempfile::tempdir().unwrap();
        let artifact_path = directory.path().join("rollout.mwab");
        let literal = format!("{:?}", artifact_path.display().to_string());
        let first_config = raw.replacen(
            "seed = 17\n",
            &format!(
                "seed = 17\npoints_per_bucket = 1\nkmeans_iterations = 1\n\
                 artifact_cache = {literal}\n"
            ),
            1,
        );
        let first = build_multiway_session(&first_config, None).unwrap();
        let first_rollout = first
            .solver
            .game()
            .abstraction()
            .rollout()
            .expect("configured rollout backend");
        assert_eq!(first_rollout.training_params().points_per_bucket, 1);
        assert_eq!(first_rollout.training_params().kmeans_iterations, 1);
        let first_fingerprint = first_rollout.fingerprint();

        let second_config =
            first_config.replacen("points_per_bucket = 1", "points_per_bucket = 2", 1);
        let second = build_multiway_session(&second_config, None).unwrap();
        let second_rollout = second
            .solver
            .game()
            .abstraction()
            .rollout()
            .expect("configured rollout backend");
        assert_eq!(second_rollout.training_params().points_per_bucket, 2);
        assert_eq!(second_rollout.training_params().kmeans_iterations, 1);
        assert_ne!(second_rollout.fingerprint(), first_fingerprint);

        let overwritten = RolloutKMeansAbstraction::read_artifact(&artifact_path).unwrap();
        assert_eq!(overwritten.training_params().points_per_bucket, 2);
        assert_eq!(overwritten.fingerprint(), second_rollout.fingerprint());
    }

    #[test]
    #[ignore = "builds full EHS2 tables over every canonical board; CI runs it in release with --include-ignored"]
    fn ehs2_table_backend_builds_a_session_and_caches_its_tables() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("ehs2.postcard");

        // Splice `kind = "ehs2-table"` and `artifact_cache` into
        // `[game.abstraction]` right after the existing `seed` key, mirroring
        // `stale_rollout_artifact_is_retrained_and_overwritten`'s splice.
        let literal = format!("{:?}", cache_path.display().to_string());
        let config_with_kind = raw.replacen(
            "seed = 17\n",
            &format!("seed = 17\nkind = \"ehs2-table\"\nartifact_cache = {literal}\n"),
            1,
        );
        assert_ne!(
            config_with_kind, raw,
            "kind/artifact_cache injection must have matched"
        );

        let session = build_multiway_session(&config_with_kind, None)
            .expect("the ehs2-table backend must build a session");
        assert_eq!(session.sweeps_target, 2);
        assert!(
            session.solver.game().abstraction().rollout().is_none(),
            "the ehs2-table backend has no rollout assignment cache to persist"
        );
        assert!(
            cache_path.is_file(),
            "the ehs2 bucket-table cache must be written at build time"
        );
    }
}
