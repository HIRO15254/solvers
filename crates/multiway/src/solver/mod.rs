//! Sparse, lazy external-sampling MCCFR for sampled multiway games.
//!
//! This module intentionally does not call its diagnostics exploitability or
//! Nash convergence: with more than two players the game is general-sum, and
//! bucket abstraction may also introduce imperfect recall.  The solver
//! exposes sampled regret diagnostics and average policies without claiming
//! a two-player zero-sum guarantee.

use std::collections::hash_map::Entry;
use std::mem::size_of;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::abstraction::{BucketId, BucketPath};
use crate::config::RecallMode;
use crate::sampler::{DealSampler, SampleError, SampledWorld};
use crate::tree::{self, Child, DenseArena, NodeId, PublicTree, TreeError};
use crate::types::{MAX_SEATS, MIN_SEATS, Street};

pub use crate::tree::DenseNodeContext;

mod errors;
mod eval;
mod support;
mod workers;

#[cfg(test)]
mod tests;

pub use errors::SolverError;
pub use eval::evaluate_node_actions;

use support::*;
use workers::*;

pub const SOLVER_STATE_VERSION: u16 = 2;
pub const DEFAULT_EXPLORATION_EPSILON: f64 = 0.06;
pub const UNREACHED_BUCKET: BucketId = u32::MAX;
pub const DEFAULT_DISCOUNT_EVERY: u64 = 100_000;
pub const DEFAULT_DISCOUNT_UNTIL: u64 = 10_000_000;
pub const DEFAULT_PRUNE_THRESHOLD: f64 = -1.0e6;
pub const DEFAULT_PRUNE_SKIP_PROBABILITY: f64 = 0.95;
/// Minimum training visits before an infoset's trained action enters a
/// [`DeviatorPolicy`]; see [`MultiwaySolver::train_deviator`].
pub const MIN_DEVIATOR_POLICY_VISITS: u32 = 8;
const ENTRY_OVERHEAD_BYTES: u64 = 64;
const HISTORY_OVERHEAD_BYTES: u64 = 48;

/// Adapter contract between a poker state machine and the generic sampled
/// solver.  Chance is sampled once, up front, in [`SampledWorld`]; therefore
/// every non-terminal state returned here is a player decision.
///
/// Action indices and their order must be stable for a given public state.
/// They form part of the lazy public-history key used by checkpoints.
pub trait ExternalSamplingGame: Send + Sync {
    type State: Clone;
    /// Precomputed node expansion, produced once per visited node by
    /// [`Self::node_actions`] and threaded through every other per-node
    /// method below instead of letting each of them recompute it.
    type Actions;

    fn num_players(&self) -> usize;
    fn root_state(&self) -> Self::State;

    /// Acting seat, or `None` iff the state is terminal.
    fn actor(&self, state: &Self::State) -> Option<usize>;

    /// Expands a node's legal actions. Called exactly once per visited
    /// node; the result is handed to [`Self::num_actions_of`],
    /// [`Self::write_action_label`], and [`Self::next_state_with`] instead
    /// of each of them re-deriving it from `state`.
    fn node_actions(&self, state: &Self::State) -> Self::Actions;
    fn num_actions_of(&self, actions: &Self::Actions) -> usize;
    fn next_state_with(
        &self,
        state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State;
    /// Appends the stable, user-facing label for `action_index` to `out`.
    /// Labels at an information set must be non-empty and unique. Callers
    /// that want just this label should clear `out` first; hot-path
    /// validation instead reuses one scratch buffer across an entire node's
    /// action list without allocating a `Vec<String>`.
    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String);

    /// Convenience wrapper over [`Self::write_action_label`] that returns an
    /// owned label. Prefer `write_action_label` with a reused buffer on any
    /// path visited once per node.
    fn action_label_of(&self, actions: &Self::Actions, action_index: usize) -> String {
        let mut label = String::new();
        self.write_action_label(actions, action_index, &mut label);
        label
    }

    /// Full private recall through the current street. Future streets must
    /// be marked [`UNREACHED_BUCKET`], never derived from the sampled runout.
    /// Active opponents excludes `actor` and is part of the information set.
    fn bucket(&self, state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo;

    /// Writes one finite utility per seat at a terminal state.
    fn terminal_utilities(&self, state: &Self::State, world: &SampledWorld, utilities: &mut [f64]);

    /// Identity of public rules, stacks, action sizing, settlement, and
    /// utility.  Override this in production adapters to protect resumes.
    fn game_fingerprint(&self) -> [u8; 32] {
        [0; 32]
    }

    /// Identity of the concrete card abstraction used by [`Self::bucket`].
    fn abstraction_fingerprint(&self) -> [u8; 32] {
        [0; 32]
    }

    /// Private-recall storage mode the solver should use for this game.
    /// Toy/test games and any adapter that never opts into
    /// [`RecallMode::Street`] can rely on the default.
    fn recall_mode(&self) -> RecallMode {
        RecallMode::Full
    }

    /// Bucket cardinality for `(street, active_opponents)`: the same count
    /// source [`Self::bucket`] uses to pick a cluster set. Only consulted to
    /// preallocate the dense arena in [`RecallMode::Street`]; a `Full`-only
    /// adapter may leave the default (panicking) implementation.
    fn bucket_count(&self, street: Street, active_opponents: u8) -> u32 {
        let _ = (street, active_opponents);
        unimplemented!(
            "bucket_count is required only to preallocate a RecallMode::Street dense arena"
        )
    }

    /// Purely public (card-independent) per-node context needed to
    /// preallocate the dense arena in [`RecallMode::Street`]; see
    /// [`DenseNodeContext`]. Only consulted in that mode.
    fn dense_node_context(&self, state: &Self::State) -> DenseNodeContext {
        let _ = state;
        unimplemented!(
            "dense_node_context is required only to preallocate a RecallMode::Street dense arena"
        )
    }

    /// Current-street abstraction bucket for an arbitrary hole `combo`,
    /// ignoring whatever combo `world` actually dealt `actor`. Only
    /// consulted by the vector-traverser dense path
    /// ([`crate::solver::SolverConfig::traverser_vector`]), which is valid
    /// only under [`RecallMode::Street`] and therefore only ever needs the
    /// bucket for the state's current street.
    fn bucket_for_combo(
        &self,
        state: &Self::State,
        world: &SampledWorld,
        actor: usize,
        combo: usize,
    ) -> BucketId {
        let _ = (state, world, actor, combo);
        unimplemented!("bucket_for_combo is required only for traverser_vector mode")
    }

    /// Batch counterpart of [`Self::bucket_for_combo`]: one bucket per entry
    /// of `combos`, all against the state's current street/board context.
    fn buckets_for_combos(
        &self,
        state: &Self::State,
        world: &SampledWorld,
        actor: usize,
        combos: &[usize],
    ) -> Vec<BucketId> {
        combos
            .iter()
            .map(|&combo| self.bucket_for_combo(state, world, actor, combo))
            .collect()
    }

    /// Vector-traverser terminal evaluation: appends one utility per entry
    /// of `combos` (in order) to `out`, holding every other seat's cards and
    /// the board fixed at whatever `world` already carries and substituting
    /// each `combos[i]` for `traverser`'s own hole cards in turn. Only
    /// consulted by [`crate::solver::SolverConfig::traverser_vector`] mode.
    fn terminal_utilities_for_combos(
        &self,
        state: &Self::State,
        world: &SampledWorld,
        traverser: usize,
        combos: &[usize],
        out: &mut Vec<f64>,
    ) {
        let _ = (state, world, traverser, combos, out);
        unimplemented!("terminal_utilities_for_combos is required only for traverser_vector mode")
    }
}

/// Resolves the average strategy for one information set, so
/// [`evaluate_node_actions`] can run identically over a live
/// [`MultiwaySolver`]'s in-memory storage (sparse or dense) or a loaded
/// `formats::MultiwaySolution`. Both sources key their storage by exactly
/// the fields [`InfoKey`] already carries -- the blake3-chained public
/// history, the acting seat, its street, its active-opponent count, and its
/// bucket path -- so `InfoKey` is the minimal common lookup signature.
///
/// A lookup miss -- an infoset the training run never actually visited -- is
/// not an error: implementors return `None`, and [`evaluate_node_actions`]
/// falls back to uniform probability over that node's legal actions.
pub trait AverageStrategyLookup {
    /// Average-strategy probabilities at `key`, normalized to sum to `1`.
    /// `None` iff `key` was never visited.
    fn lookup(&self, key: InfoKey) -> Option<Vec<f64>>;
}

impl<G: ExternalSamplingGame> AverageStrategyLookup for MultiwaySolver<G> {
    fn lookup(&self, key: InfoKey) -> Option<Vec<f64>> {
        self.average_strategy(key)
            .map(|probabilities| probabilities.iter().map(|&p| f64::from(p)).collect())
    }
}

fn profile_estimate(mean: f64, sum_squared_error: f64, samples: u64) -> ProfileEstimate {
    let stderr = standard_error(sum_squared_error, samples);
    let radius = 1.96 * stderr;
    ProfileEstimate {
        mean,
        stderr,
        ci95: [mean - radius, mean + radius],
    }
}

fn nonnegative_gain_estimate(
    paired_mean: f64,
    sum_squared_error: f64,
    samples: u64,
) -> ProfileEstimate {
    let stderr = standard_error(sum_squared_error, samples);
    let radius = 1.96 * stderr;
    ProfileEstimate {
        mean: paired_mean.max(0.0),
        stderr,
        ci95: [
            (paired_mean - radius).max(0.0),
            (paired_mean + radius).max(0.0),
        ],
    }
}

fn standard_error(sum_squared_error: f64, samples: u64) -> f64 {
    if samples > 1 {
        let sample_variance = sum_squared_error / (samples - 1) as f64;
        (sample_variance / samples as f64).sqrt()
    } else {
        0.0
    }
}

fn regret_greedy_action(regrets: &[f32]) -> usize {
    regrets
        .iter()
        .enumerate()
        .max_by(|(left_index, left), (right_index, right)| {
            left.total_cmp(right)
                .then_with(|| right_index.cmp(left_index))
        })
        .map(|(index, _)| index)
        .expect("policy columns always have at least one action")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PrivateInfo {
    pub street: u8,
    pub active_opponents: u8,
    pub bucket_path: [BucketId; 4],
}

impl PrivateInfo {
    pub fn from_path(street: Street, active_opponents: u8, path: BucketPath) -> Self {
        let street_index = street.index();
        let mut bucket_path = [UNREACHED_BUCKET; 4];
        let full_path = path.as_array();
        bucket_path[..=street_index].copy_from_slice(&full_path[..=street_index]);
        Self {
            street: street_index as u8,
            active_opponents,
            bucket_path,
        }
    }

    pub fn current_bucket(self) -> BucketId {
        self.bucket_path[self.street as usize]
    }

    /// Street-recall (imperfect-recall) constructor: only the current
    /// street's bucket is populated; every other street is
    /// [`UNREACHED_BUCKET`], including earlier ones (unlike
    /// [`Self::from_path`], which keeps every already-reached street).
    pub fn from_current_bucket(street: Street, active_opponents: u8, bucket: BucketId) -> Self {
        let street_index = street.index();
        let mut bucket_path = [UNREACHED_BUCKET; 4];
        bucket_path[street_index] = bucket;
        Self {
            street: street_index as u8,
            active_opponents,
            bucket_path,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HistoryKey(pub [u8; 16]);

impl HistoryKey {
    pub const ROOT: Self = Self([0; 16]);

    pub fn child(self, actor: usize, action_index: usize) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.history.v1");
        hasher.update(&self.0);
        hasher.update(&(actor as u64).to_le_bytes());
        hasher.update(&(action_index as u64).to_le_bytes());
        let mut key = [0u8; 16];
        key.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        Self(key)
    }
}

/// One edge in the compact visited public-history trie. The root is implicit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub key: HistoryKey,
    pub parent: HistoryKey,
    pub actor: u8,
    pub action_index: u32,
    pub action_label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InfoKey {
    pub history: HistoryKey,
    pub player: u8,
    pub street: u8,
    pub active_opponents: u8,
    pub bucket_path: [BucketId; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyColumn {
    /// Stable index-to-action mapping persisted with checkpoints.
    pub action_labels: Vec<String>,
    /// Cumulative sampled counterfactual regrets.
    pub regrets: Vec<f32>,
    /// Cumulative reach-weighted strategy numerators.
    pub strategy_sum: Vec<f32>,
}

impl PolicyColumn {
    fn zeroed(action_labels: Vec<String>) -> Self {
        let num_actions = action_labels.len();
        Self {
            action_labels,
            regrets: vec![0.0; num_actions],
            strategy_sum: vec![0.0; num_actions],
        }
    }

    pub fn num_actions(&self) -> usize {
        self.regrets.len()
    }

    pub fn current_strategy(&self) -> Vec<f32> {
        regret_matching_f32(&self.regrets)
    }

    pub fn average_strategy(&self) -> Vec<f32> {
        normalize_nonnegative_f32(&self.strategy_sum).unwrap_or_else(|| self.current_strategy())
    }

    pub fn current_action_probabilities(&self) -> Vec<ActionProbability> {
        self.labeled(self.current_strategy())
    }

    pub fn average_action_probabilities(&self) -> Vec<ActionProbability> {
        self.labeled(self.average_strategy())
    }

    fn labeled(&self, probabilities: Vec<f32>) -> Vec<ActionProbability> {
        self.action_labels
            .iter()
            .cloned()
            .zip(probabilities)
            .map(|(action, probability)| ActionProbability {
                action,
                probability,
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyEntry {
    pub key: InfoKey,
    pub column: PolicyColumn,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionProbability {
    pub action: String,
    pub probability: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolverConfig {
    pub seed: u64,
    /// Conservative cap for sparse policy payload plus hash-entry overhead.
    pub max_memory_bytes: u64,
    /// Guard against a malformed game adapter producing a cycle.
    pub max_traversal_depth: u32,
    /// Uniform exploration mixed into sampled non-traverser actions.
    pub exploration_epsilon: f64,
    /// Batched early discount cadence in completed sweeps.
    pub discount_every: u64,
    /// Stop discounting once this completed sweep is reached. Zero disables it.
    pub discount_until: u64,
    /// Number of complete sweeps run against the same strategy snapshot per
    /// parallel drive iteration (see [`MultiwaySolver::run_sweeps_with_threads_until`]).
    /// `1` (the default) is bit-identical to the pre-batching solver; values
    /// above `1` trade slightly staler within-batch updates for restored
    /// parallel efficiency on tables whose per-seat traversal cost is
    /// imbalanced. Must be at least `1`.
    pub sweep_batch: u64,
    /// Enables "vector-traverser" external sampling: one traversal updates
    /// every feasible hole combo of the sampled traverser seat at once
    /// against the same sampled opponents/board, instead of only the one
    /// combo the deal sampler happened to deal that seat. Only valid when
    /// [`ExternalSamplingGame::recall_mode`] is [`RecallMode::Street`] (the
    /// dense arena); [`validate_setup`] rejects `true` under
    /// [`RecallMode::Full`]. `false` (the default) is the original
    /// one-hand-per-traversal algorithm, byte-identical to before this field
    /// existed.
    pub traverser_vector: bool,
    /// Enables Pluribus-style regret-based pruning at traverser decision
    /// nodes in vector mode: a (bucket, action) whose regret-matched
    /// probability is exactly zero and whose accumulated regret is below
    /// [`Self::prune_threshold`] is skipped (with probability
    /// [`Self::prune_skip_probability`]) rather than descended into, saving
    /// the traversal work that would only ever multiply by a zero
    /// probability. `false` (the default) is byte-identical to before this
    /// field existed. [`validate_setup`] rejects `true` unless
    /// [`Self::traverser_vector`] is also `true`.
    pub prune: bool,
    /// Regret threshold (utility units) below which a zero-probability
    /// (bucket, action) becomes a pruning candidate. Must be finite and
    /// strictly negative when [`Self::prune`] is enabled; ignored otherwise.
    pub prune_threshold: f64,
    /// Probability of actually skipping a prunable (bucket, action) on a
    /// given visit (Pluribus used `0.95`). Must be finite and in `[0, 1]`
    /// when [`Self::prune`] is enabled; ignored otherwise.
    pub prune_skip_probability: f64,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            max_memory_bytes: 4 * 1024 * 1024 * 1024,
            max_traversal_depth: 512,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolverState {
    pub schema_version: u16,
    pub config: SolverConfig,
    pub traversals: u64,
    pub completed_sweeps: u64,
    pub next_sample_id: u64,
    pub total_deal_attempts: u64,
    pub terminal_evaluations: u64,
    /// Sum, over every traversal so far, of the number of individual hand
    /// updates it performed: `1` for every traversal under the original
    /// scalar algorithm (each updates exactly one sampled traverser hand),
    /// or `|F|` (the feasible traverser combo count) for a
    /// [`SolverConfig::traverser_vector`] traversal. This is the number to
    /// compare against a range-based solver's "hands/s".
    pub hand_updates: u64,
    /// Sorted by key. Together with the implicit root this is the compact
    /// public action-history trie used by result explorers.
    pub histories: Vec<HistoryEntry>,
    /// Sorted by [`InfoKey`] when captured, making checkpoint bytes stable.
    pub policies: Vec<PolicyEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SolverMetrics {
    pub sweeps: u64,
    pub traversals: u64,
    pub terminal_evaluations: u64,
    pub infosets: u64,
    pub memory_bytes: u64,
    pub total_deal_attempts: u64,
    pub mean_deal_attempts: f64,
    /// See [`SolverState::hand_updates`].
    pub hand_updates: u64,
    /// Sum of positive cumulative regrets divided by completed updates for
    /// each seat.  This is a sampled diagnostic, not exploitability.
    pub average_positive_regret: Vec<f64>,
}

/// Dense-arena preflight numbers; see [`MultiwaySolver::dense_arena_stats`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenseArenaStats {
    pub node_count: u64,
    pub total_columns: u64,
    pub total_slots: u64,
    pub estimated_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileEstimate {
    pub mean: f64,
    pub stderr: f64,
    pub ci95: [f64; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileEvaluation {
    pub samples: u64,
    pub total_deal_attempts: u64,
    pub seats: Vec<ProfileEstimate>,
    /// Per-seat held-out estimate for one fixed regret-greedy unilateral
    /// deviation (and the no-deviation option) against opponents' average
    /// profile. This is a conservative candidate-policy diagnostic, not a
    /// best response, exploitability, or Nash-convergence claim.
    pub deviation_gain_lower_bound: Option<Vec<ProfileEstimate>>,
}

/// A fixed per-seat deviation policy trained by [`MultiwaySolver::train_deviator`]:
/// for each information set it visited during training, the single action index
/// it deviates to. Infosets it never visited fall back to the caller's usual
/// deviation behavior (see [`MultiwaySolver::evaluate_profile`]).
#[derive(Clone, Debug, PartialEq)]
pub struct DeviatorPolicy {
    pub seat: usize,
    pub actions: FxHashMap<InfoKey, u16>,
}

/// Which profile [`MultiwaySolver::evaluate_profile`] replays / a deviator
/// trained by [`MultiwaySolver::train_deviator`] trains against. The
/// default (`purify_threshold: 0.0`, `use_current_strategy: false`) is the
/// plain linear average profile, unpurified -- byte-identical to the
/// pre-refactor `evaluate_average_profile`/`train_deviator` behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProfileVariant {
    /// Purification threshold (Ganzfried & Sandholm, AAMAS 2012): entries
    /// below the threshold are zeroed and the remainder renormalized;
    /// `0.0` skips the purify call entirely (raw average profile). Must be
    /// finite and in `[0.0, 1.0]`.
    pub purify_threshold: f32,
    /// Evaluate/train against the last-iterate regret-matched current
    /// strategy instead of the linear average profile. Diagnostic only:
    /// plain regret matching carries no last-iterate convergence guarantee
    /// (the average is the object with the CCE-style bound).
    pub use_current_strategy: bool,
}

/// One hand-group's row of [`evaluate_node_actions`]'s per-action EV
/// estimates. "Group" here is the acting seat's current-street bucket at the
/// evaluated node -- for a preflop node this is exactly the 169-class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeActionGroupEvaluation {
    /// The acting seat's current-street bucket (the group id).
    pub group: BucketId,
    /// This group's share of the total self-normalized importance weight
    /// across every group that appeared in the sample. Every returned
    /// group's `weight_share` sums to `1` across [`NodeActionEvaluation::groups`]
    /// (subject to floating-point rounding).
    pub weight_share: f64,
    /// The group's own average-strategy probabilities at the evaluated node
    /// (uniform on a lookup miss), aligned with
    /// [`NodeActionEvaluation::action_labels`]; sums to `1`.
    pub frequencies: Vec<f64>,
    /// Per-action EV estimate for hands in this group, aligned with
    /// [`NodeActionEvaluation::action_labels`]. See [`evaluate_node_actions`]
    /// for the estimator.
    pub actions: Vec<ProfileEstimate>,
    /// Samples that landed in this group with nonzero path weight.
    pub samples: u64,
}

/// One action's range-wide aggregate across every group, produced by
/// [`evaluate_node_actions`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeActionAggregate {
    pub ev: ProfileEstimate,
    /// `Σ_g weight_share(g) · frequencies(g)[action]`: the weighted-average
    /// looked-up action frequency across the reached range. This is the
    /// profile's own frequency (not derived from the EV samples), reported
    /// alongside the EV for a GTO-Wizard-style per-action row.
    pub frequency: f64,
}

/// Per-hand-group, per-action expected-utility estimate of "take this action
/// now, then everyone (including the actor) plays the current average
/// strategy to the end of the hand", produced by [`evaluate_node_actions`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeActionEvaluation {
    /// The acting seat at the evaluated node.
    pub actor: usize,
    /// Stable index-to-action mapping, shared by every group row and the
    /// aggregate row.
    pub action_labels: Vec<String>,
    pub samples: u64,
    pub total_deal_attempts: u64,
    /// Sorted by [`NodeActionGroupEvaluation::group`].
    pub groups: Vec<NodeActionGroupEvaluation>,
    /// Aligned with `action_labels`.
    pub aggregate: Vec<NodeActionAggregate>,
}

/// Sparse external-sampling MCCFR state.  A policy column exists only after
/// `(public history, player, bucket)` is visited by a sampled traversal --
/// unless [`Self::dense`] is `Some`, in which case every column for
/// [`RecallMode::Street`]'s enumerated tree was preallocated up front and
/// `policies`/`histories`/`approx_memory_bytes` below are unused (always
/// empty/zero).
pub struct MultiwaySolver<G: ExternalSamplingGame> {
    game: G,
    sampler: DealSampler,
    config: SolverConfig,
    policies: FxHashMap<InfoKey, PolicyColumn>,
    histories: FxHashMap<HistoryKey, HistoryEntry>,
    approx_memory_bytes: u64,
    traversals: u64,
    completed_sweeps: u64,
    next_sample_id: u64,
    total_deal_attempts: u64,
    terminal_evaluations: u64,
    hand_updates: u64,
    dense: Option<DenseStorage>,
}

impl<G: ExternalSamplingGame> MultiwaySolver<G> {
    pub fn new(game: G, sampler: DealSampler, config: SolverConfig) -> Result<Self, SolverError> {
        validate_setup(&game, &sampler, config)?;
        let dense = match game.recall_mode() {
            RecallMode::Full => None,
            RecallMode::Street => Some(DenseStorage::build(&game, config.max_memory_bytes)?),
        };
        Ok(Self {
            game,
            sampler,
            config,
            policies: FxHashMap::default(),
            histories: FxHashMap::default(),
            approx_memory_bytes: 0,
            traversals: 0,
            completed_sweeps: 0,
            next_sample_id: 0,
            total_deal_attempts: 0,
            terminal_evaluations: 0,
            hand_updates: 0,
            dense,
        })
    }

    pub fn with_defaults(game: G, sampler: DealSampler) -> Result<Self, SolverError> {
        Self::new(game, sampler, SolverConfig::default())
    }

    pub fn from_state(
        game: G,
        sampler: DealSampler,
        state: SolverState,
    ) -> Result<Self, SolverError> {
        let current_config = state.config;
        Self::from_state_with_config(game, sampler, state, current_config)
    }

    /// Restores algorithm state while adopting the caller's current
    /// operational memory budget. All settings that affect sampling or
    /// updates must still match the checkpoint exactly.
    pub fn from_state_with_config(
        game: G,
        sampler: DealSampler,
        mut state: SolverState,
        current_config: SolverConfig,
    ) -> Result<Self, SolverError> {
        validate_setup(&game, &sampler, current_config)?;
        if state.schema_version != SOLVER_STATE_VERSION {
            return Err(SolverError::StateVersion {
                found: state.schema_version,
                expected: SOLVER_STATE_VERSION,
            });
        }
        if !resume_configs_match(state.config, current_config) {
            return Err(SolverError::ResumeConfigurationMismatch);
        }
        state.config.max_memory_bytes = current_config.max_memory_bytes;
        let expected_sweeps = state.traversals / game.num_players() as u64;
        if state.completed_sweeps != expected_sweeps {
            return Err(SolverError::InvalidState(
                "completed sweep count is inconsistent with traversals",
            ));
        }
        if state.next_sample_id != state.traversals {
            return Err(SolverError::InvalidState(
                "next sample id is inconsistent with traversals",
            ));
        }

        if matches!(game.recall_mode(), RecallMode::Street) {
            return Self::from_state_dense(game, sampler, state);
        }

        let mut histories: FxHashMap<HistoryKey, HistoryEntry> = FxHashMap::default();
        histories.reserve(state.histories.len());
        let mut approx_memory_bytes = 0u64;
        for entry in state.histories {
            validate_history_entry(&entry, game.num_players())?;
            approx_memory_bytes = approx_memory_bytes
                .checked_add(history_memory_bytes(&entry.action_label)?)
                .ok_or(SolverError::MemoryAccountingOverflow)?;
            let key = entry.key;
            if histories.insert(key, entry).is_some() {
                return Err(SolverError::DuplicateHistory(key));
            }
        }
        validate_history_graph(&histories)?;

        let mut policies: FxHashMap<InfoKey, PolicyColumn> = FxHashMap::default();
        policies.reserve(state.policies.len());
        for entry in state.policies {
            validate_column(
                entry.key,
                &entry.column,
                game.num_players(),
                RecallMode::Full,
            )?;
            if entry.key.history != HistoryKey::ROOT && !histories.contains_key(&entry.key.history)
            {
                return Err(SolverError::InvalidState(
                    "policy refers to an unknown public history",
                ));
            }
            approx_memory_bytes = approx_memory_bytes
                .checked_add(entry_memory_bytes(&entry.column.action_labels)?)
                .ok_or(SolverError::MemoryAccountingOverflow)?;
            if policies.insert(entry.key, entry.column).is_some() {
                return Err(SolverError::DuplicatePolicy(entry.key));
            }
        }
        if approx_memory_bytes > state.config.max_memory_bytes {
            return Err(SolverError::MemoryLimit {
                limit: state.config.max_memory_bytes,
                needed: approx_memory_bytes,
            });
        }

        Ok(Self {
            game,
            sampler,
            config: state.config,
            policies,
            histories,
            approx_memory_bytes,
            traversals: state.traversals,
            completed_sweeps: state.completed_sweeps,
            next_sample_id: state.next_sample_id,
            total_deal_attempts: state.total_deal_attempts,
            terminal_evaluations: state.terminal_evaluations,
            hand_updates: state.hand_updates,
            dense: None,
        })
    }

    /// [`Self::from_state_with_config`]'s `RecallMode::Street` path: rebuilds
    /// the dense arena from the game's enumerated tree, then replays the
    /// checkpoint's (already ancestors-of-touched-pruned) histories/policies
    /// into it.
    fn from_state_dense(
        game: G,
        sampler: DealSampler,
        state: SolverState,
    ) -> Result<Self, SolverError> {
        let mut dense_storage = DenseStorage::build(&game, state.config.max_memory_bytes)?;
        for entry in &state.histories {
            validate_history_entry(entry, game.num_players())?;
            if !dense_storage.tree.by_history.contains_key(&entry.key) {
                return Err(SolverError::UnmappedDenseHistory(entry.key));
            }
        }
        for entry in state.policies {
            validate_column(
                entry.key,
                &entry.column,
                game.num_players(),
                RecallMode::Street,
            )?;
            let (node_id, bucket) = dense_storage
                .target(entry.key)
                .ok_or(SolverError::UnmappedDenseEntry { key: entry.key })?;
            let node = &dense_storage.tree.nodes[node_id as usize];
            if node.action_labels != entry.column.action_labels {
                return Err(SolverError::ActionLabelsChanged { key: entry.key });
            }
            let range = dense_storage.arena.slot_range(node_id, bucket)?;
            if range.len() != entry.column.regrets.len() {
                return Err(SolverError::ActionCountChanged {
                    key: entry.key,
                    stored: range.len(),
                    current: entry.column.regrets.len(),
                });
            }
            dense_storage.arena.regrets[range.clone()].copy_from_slice(&entry.column.regrets);
            dense_storage.arena.strategy_sum[range].copy_from_slice(&entry.column.strategy_sum);
            let column = dense_storage.arena.column_id(node_id, bucket)?;
            dense_storage.arena.touched_set(column);
        }

        Ok(Self {
            game,
            sampler,
            config: state.config,
            policies: FxHashMap::default(),
            histories: FxHashMap::default(),
            approx_memory_bytes: 0,
            traversals: state.traversals,
            completed_sweeps: state.completed_sweeps,
            next_sample_id: state.next_sample_id,
            total_deal_attempts: state.total_deal_attempts,
            terminal_evaluations: state.terminal_evaluations,
            hand_updates: state.hand_updates,
            dense: Some(dense_storage),
        })
    }

    pub fn game(&self) -> &G {
        &self.game
    }

    pub fn sampler(&self) -> &DealSampler {
        &self.sampler
    }

    pub fn config(&self) -> SolverConfig {
        self.config
    }

    /// Fingerprint covering solver knobs, ranges, and public game rules.
    pub fn configuration_fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.solver-config.v4");
        hasher.update(&self.config.seed.to_le_bytes());
        hasher.update(&self.config.max_traversal_depth.to_le_bytes());
        hasher.update(&self.config.exploration_epsilon.to_bits().to_le_bytes());
        hasher.update(&self.config.discount_every.to_le_bytes());
        hasher.update(&self.config.discount_until.to_le_bytes());
        hasher.update(&self.config.sweep_batch.to_le_bytes());
        hasher.update(&[u8::from(self.config.traverser_vector)]);
        hasher.update(&[u8::from(self.config.prune)]);
        hasher.update(&self.config.prune_threshold.to_bits().to_le_bytes());
        hasher.update(&self.config.prune_skip_probability.to_bits().to_le_bytes());
        hasher.update(&self.sampler.range_fingerprint());
        hasher.update(&self.game.game_fingerprint());
        *hasher.finalize().as_bytes()
    }

    pub fn abstraction_fingerprint(&self) -> [u8; 32] {
        self.game.abstraction_fingerprint()
    }

    /// Runs complete sweeps using a deterministic ordered-delta batch.
    ///
    /// Every seat traversal in a sweep reads the same strategy snapshot.
    /// Workers only produce local update events; those events are merged by
    /// sample id (equivalently seat order within the sweep). Consequently,
    /// changing `threads` cannot change f32 accumulation order or checkpoint
    /// bytes.
    pub fn run_sweeps(&mut self, sweeps: u64) -> Result<(), SolverError> {
        self.run_sweeps_with_threads(sweeps, 1)
    }

    /// Runs deterministic parallel sweeps until the requested count is
    /// reached or `should_continue` returns false at a sweep boundary.
    ///
    /// The Rayon pool is built once for the whole call. The return value is
    /// the number of complete, transactionally merged sweeps from this call.
    pub fn run_sweeps_with_threads(
        &mut self,
        sweeps: u64,
        threads: usize,
    ) -> Result<(), SolverError> {
        self.run_sweeps_with_threads_until(sweeps, threads, || true)
            .map(|_| ())
    }

    /// Sweep batching: `config.sweep_batch` complete sweeps at a time are
    /// dispatched against the *same* strategy snapshot as one `batch *
    /// num_players`-wide `rayon` fan-out, instead of one `num_players`-wide
    /// fan-out per sweep. This restores parallel efficiency on tables whose
    /// per-seat traversal cost is imbalanced (a `num_players <= 9`-way fan-out
    /// underuses a machine with many more cores), at the cost of every
    /// traversal within a batch reading a snapshot that is up to
    /// `sweep_batch - 1` sweeps staler than the sequential algorithm would
    /// have used for later sweeps in the batch -- the standard mini-batch
    /// MCCFR trade-off. `sweep_batch == 1` (the default) reduces to exactly
    /// the pre-batching one-sweep-at-a-time schedule: same task count, same
    /// per-task sample ids, same per-task linear weight, so it is
    /// bit-identical to today's checkpoints and thread-count invariance.
    ///
    /// Every task `j` in `0..batch * num_players` is `(sweep_offset,
    /// traverser) = (j / num_players, j % num_players)`, reads sample id
    /// `first_sample_id + j`, and uses linear weight
    /// `completed_sweeps_at_batch_start + sweep_offset + 1` -- computed from
    /// the task's own sweep offset rather than the live `self.completed_sweeps`,
    /// so later sweeps in the batch still get their own (correctly larger)
    /// weight even though every task in the batch reads one shared snapshot.
    /// Deltas are collected in task order (an `IndexedParallelIterator`) and
    /// merged one sweep at a time, in `sweep_offset` order, through the
    /// existing single-sweep [`Self::merge_sweep`] -- unmodified, since it
    /// already validates and advances by `self.next_sample_id`, which is
    /// exactly what each successive sweep's slice of tasks used.
    ///
    /// `should_continue` is polled once per *batch* rather than once per
    /// sweep, so cancellation granularity coarsens to whole batches: a
    /// `sweep_batch = 8` run can overshoot a requested stopping point by up
    /// to 7 extra sweeps versus `sweep_batch = 1`. Batches are committed as a
    /// whole -- there is no partial-batch commit -- matching `merge_sweep`'s
    /// existing all-or-nothing-per-sweep contract.
    pub fn run_sweeps_with_threads_until<F>(
        &mut self,
        sweeps: u64,
        threads: usize,
        mut should_continue: F,
    ) -> Result<u64, SolverError>
    where
        F: FnMut() -> bool,
    {
        if threads == 0 {
            return Err(SolverError::ZeroThreads);
        }
        if !self
            .traversals
            .is_multiple_of(self.game.num_players() as u64)
        {
            return Err(SolverError::IncompleteSweepState);
        }
        let additional_traversals = sweeps
            .checked_mul(self.game.num_players() as u64)
            .ok_or(SolverError::TraversalCountOverflow)?;
        self.traversals
            .checked_add(additional_traversals)
            .ok_or(SolverError::TraversalCountOverflow)?;
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|error| SolverError::ThreadPoolBuild(error.to_string()))?;
        let num_players = self.game.num_players() as u64;
        let mut completed = 0u64;
        let mut sweeps_remaining = sweeps;

        while sweeps_remaining > 0 {
            if !should_continue() {
                break;
            }
            let batch = self.config.sweep_batch.min(sweeps_remaining);
            let completed_sweeps_at_batch_start = self.completed_sweeps;
            let first_sample_id = self.next_sample_id;
            let total_tasks = batch
                .checked_mul(num_players)
                .ok_or(SolverError::CounterOverflow)?;
            let results = pool.install(|| {
                (0..total_tasks)
                    .into_par_iter()
                    .map(|task| {
                        let sweep_offset = task / num_players;
                        let traverser = (task % num_players) as usize;
                        let sample_id = first_sample_id
                            .checked_add(task)
                            .ok_or(SolverError::CounterOverflow)?;
                        let linear_weight = completed_sweeps_at_batch_start
                            .checked_add(sweep_offset)
                            .and_then(|value| value.checked_add(1))
                            .ok_or(SolverError::CounterOverflow)?
                            as f64;
                        self.generate_traversal_delta(sample_id, traverser, linear_weight)
                    })
                    .collect::<Vec<_>>()
            });
            // IndexedParallelIterator::collect preserves task order. Resolve
            // errors in that same order as well, rather than whichever worker
            // happened to finish first.
            let mut deltas = Vec::with_capacity(total_tasks as usize);
            for result in results {
                deltas.push(result?);
            }
            // Merge one sweep at a time, in sweep order, through the
            // unmodified single-sweep merge: each `num_players`-sized chunk
            // is exactly one sweep's deltas in seat order.
            let mut deltas = deltas.into_iter();
            for _ in 0..batch {
                let sweep_deltas: Vec<_> = (&mut deltas).take(num_players as usize).collect();
                self.merge_sweep(sweep_deltas)?;
                completed = completed
                    .checked_add(1)
                    .ok_or(SolverError::CounterOverflow)?;
            }
            sweeps_remaining -= batch;
        }
        Ok(completed)
    }

    /// `linear_weight` is the caller's chosen weight for this traversal's
    /// sweep, computed from the batch-start sweep count plus the sweep's
    /// offset within the batch (see
    /// [`Self::run_sweeps_with_threads_until`]) rather than read from
    /// `self.completed_sweeps` here, so every traversal in a batch can use
    /// its own sweep's weight even though they all read the same policy
    /// snapshot.
    fn generate_traversal_delta(
        &self,
        sample_id: u64,
        traverser: usize,
        linear_weight: f64,
    ) -> Result<AnyTraversalDelta, SolverError> {
        let mut deal_rng = traversal_deal_rng(self.config.seed, sample_id, traverser);
        let sample = self.sampler.sample_counted(&mut deal_rng)?;
        let mut action_rng = traversal_action_rng(self.config.seed, sample_id, traverser);
        let mut reach = vec![1.0; self.game.num_players()];
        match &self.dense {
            None => {
                let mut worker = TraversalWorker::new(self, linear_weight);
                worker.traverse(
                    self.game.root_state(),
                    &sample.world,
                    traverser,
                    HistoryKey::ROOT,
                    &mut reach,
                    1.0,
                    &mut action_rng,
                    0,
                )?;
                Ok(AnyTraversalDelta::Sparse(worker.finish(
                    sample_id,
                    traverser,
                    u64::from(sample.attempts),
                )))
            }
            Some(dense) if self.config.traverser_vector => {
                let feasible = self.sampler.feasible_combos(traverser, &sample.world);
                let (combos, weights): (Vec<usize>, Vec<f64>) = feasible.into_iter().unzip();
                // Own-reach starts at 1.0 for every feasible combo; unlike
                // `reach` (seat-indexed, used by the scalar/dense-scalar
                // workers), this is combo-indexed and threaded separately
                // (see `VectorTraversalWorker::traverse`).
                let own_reach = vec![1.0; combos.len()];
                // The traversal starts with every feasible combo active;
                // pruning (see `VectorTraversalWorker::traverse`) is the
                // only thing that ever shrinks this subset further down the
                // tree.
                let active: Vec<usize> = (0..combos.len()).collect();
                let mut worker = VectorTraversalWorker::new(
                    &self.game,
                    dense,
                    self.config,
                    linear_weight,
                    combos,
                    weights,
                );
                worker.traverse(
                    self.game.root_state(),
                    0,
                    &sample.world,
                    traverser,
                    &active,
                    &own_reach,
                    1.0,
                    &mut action_rng,
                    0,
                )?;
                Ok(AnyTraversalDelta::Dense(worker.finish(
                    sample_id,
                    traverser,
                    u64::from(sample.attempts),
                )))
            }
            Some(dense) => {
                let mut worker =
                    DenseTraversalWorker::new(&self.game, dense, self.config, linear_weight);
                worker.traverse(
                    self.game.root_state(),
                    0,
                    &sample.world,
                    traverser,
                    &mut reach,
                    1.0,
                    &mut action_rng,
                    0,
                )?;
                Ok(AnyTraversalDelta::Dense(worker.finish(
                    sample_id,
                    traverser,
                    u64::from(sample.attempts),
                )))
            }
        }
    }

    /// Replays a complete sweep into scratch columns first. This makes the
    /// merge transactional: a memory or numeric limit never leaves a partial
    /// sweep whose workers would have to be regenerated from a lost snapshot.
    ///
    /// This was measured (see `docs/` history / task report) against a
    /// key-sharded parallel variant that fanned events out into
    /// [`MERGE_SHARDS`]-many buckets and applied them with rayon: at this
    /// benchmark's event volume (a few hundred events per sweep), sharding
    /// was consistently ~5% *slower* than this serial version, at both 32 and
    /// 8 shards -- the fixed per-sweep overhead (shard `Vec` allocation, two
    /// rayon fork-join dispatches, and merging per-shard scratch maps back
    /// together) exceeded the serial work it replaced. This straightforward
    /// version is kept as the faster variant.
    fn merge_sweep(&mut self, deltas: Vec<AnyTraversalDelta>) -> Result<(), SolverError> {
        if self.dense.is_some() {
            let deltas = deltas
                .into_iter()
                .map(|delta| match delta {
                    AnyTraversalDelta::Dense(delta) => Ok(delta),
                    AnyTraversalDelta::Sparse(_) => Err(SolverError::InvalidState(
                        "a sparse traversal delta was produced by a dense (Street-recall) solver",
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            return self.merge_sweep_dense(deltas);
        }
        let deltas = deltas
            .into_iter()
            .map(|delta| match delta {
                AnyTraversalDelta::Sparse(delta) => Ok(delta),
                AnyTraversalDelta::Dense(_) => Err(SolverError::InvalidState(
                    "a dense traversal delta was produced by a sparse (full-recall) solver",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;

        let num_players = self.game.num_players();
        if deltas.len() != num_players {
            return Err(SolverError::InvalidState(
                "parallel sweep did not produce one delta per seat",
            ));
        }

        let mut scratch_policies: FxHashMap<InfoKey, PolicyColumn> = FxHashMap::default();
        let mut scratch_histories: FxHashMap<HistoryKey, HistoryEntry> = FxHashMap::default();
        let mut memory_bytes = self.approx_memory_bytes;
        let mut total_deal_attempts = self.total_deal_attempts;
        let mut terminal_evaluations = self.terminal_evaluations;
        let mut hand_updates = self.hand_updates;

        for (seat, delta) in deltas.into_iter().enumerate() {
            let expected_sample_id = self
                .next_sample_id
                .checked_add(seat as u64)
                .ok_or(SolverError::CounterOverflow)?;
            if delta.sample_id != expected_sample_id || delta.traverser != seat {
                return Err(SolverError::InvalidState(
                    "parallel traversal deltas are not in sample-id order",
                ));
            }
            total_deal_attempts = total_deal_attempts
                .checked_add(delta.deal_attempts)
                .ok_or(SolverError::CounterOverflow)?;
            terminal_evaluations = terminal_evaluations
                .checked_add(delta.terminal_evaluations)
                .ok_or(SolverError::CounterOverflow)?;
            hand_updates = hand_updates
                .checked_add(delta.hand_updates)
                .ok_or(SolverError::CounterOverflow)?;

            for event in delta.events {
                match event {
                    TraversalEvent::EnsurePolicy { key, action_labels } => {
                        let stored = scratch_policies
                            .get(&key)
                            .or_else(|| self.policies.get(&key));
                        if let Some(column) = stored {
                            if column.num_actions() != action_labels.len() {
                                return Err(SolverError::ActionCountChanged {
                                    key,
                                    stored: column.num_actions(),
                                    current: action_labels.len(),
                                });
                            }
                            if column.action_labels != action_labels {
                                return Err(SolverError::ActionLabelsChanged { key });
                            }
                        } else {
                            memory_bytes = memory_bytes
                                .checked_add(entry_memory_bytes(&action_labels)?)
                                .ok_or(SolverError::MemoryAccountingOverflow)?;
                            if memory_bytes > self.config.max_memory_bytes {
                                return Err(SolverError::MemoryLimit {
                                    limit: self.config.max_memory_bytes,
                                    needed: memory_bytes,
                                });
                            }
                            scratch_policies.insert(key, PolicyColumn::zeroed(action_labels));
                        }
                    }
                    TraversalEvent::EnsureHistory(entry) => {
                        let stored = scratch_histories
                            .get(&entry.key)
                            .or_else(|| self.histories.get(&entry.key));
                        if let Some(stored) = stored {
                            if stored != &entry {
                                return Err(SolverError::HistoryCollision(entry.key));
                            }
                        } else {
                            memory_bytes = memory_bytes
                                .checked_add(history_memory_bytes(&entry.action_label)?)
                                .ok_or(SolverError::MemoryAccountingOverflow)?;
                            if memory_bytes > self.config.max_memory_bytes {
                                return Err(SolverError::MemoryLimit {
                                    limit: self.config.max_memory_bytes,
                                    needed: memory_bytes,
                                });
                            }
                            scratch_histories.insert(entry.key, entry);
                        }
                    }
                    TraversalEvent::AddRegret { key, values } => {
                        if let Entry::Vacant(entry) = scratch_policies.entry(key) {
                            let column =
                                self.policies.get(&key).ok_or(SolverError::InvalidState(
                                    "regret delta refers to an uninitialized policy",
                                ))?;
                            entry.insert(column.clone());
                        }
                        let column = scratch_policies
                            .get_mut(&key)
                            .expect("inserted or copied above");
                        if column.regrets.len() != values.len() {
                            return Err(SolverError::ActionCountChanged {
                                key,
                                stored: column.regrets.len(),
                                current: values.len(),
                            });
                        }
                        for (target, value) in column.regrets.iter_mut().zip(values) {
                            checked_add_f32(target, value)?;
                        }
                    }
                    TraversalEvent::AddStrategy { key, values } => {
                        if let Entry::Vacant(entry) = scratch_policies.entry(key) {
                            let column =
                                self.policies.get(&key).ok_or(SolverError::InvalidState(
                                    "strategy delta refers to an uninitialized policy",
                                ))?;
                            entry.insert(column.clone());
                        }
                        let column = scratch_policies
                            .get_mut(&key)
                            .expect("inserted or copied above");
                        if column.strategy_sum.len() != values.len() {
                            return Err(SolverError::ActionCountChanged {
                                key,
                                stored: column.strategy_sum.len(),
                                current: values.len(),
                            });
                        }
                        for (target, value) in column.strategy_sum.iter_mut().zip(values) {
                            checked_add_f32(target, value)?;
                        }
                    }
                }
            }
        }

        let added = num_players as u64;
        let traversals = self
            .traversals
            .checked_add(added)
            .ok_or(SolverError::CounterOverflow)?;
        let next_sample_id = self
            .next_sample_id
            .checked_add(added)
            .ok_or(SolverError::CounterOverflow)?;
        let completed_sweeps = self
            .completed_sweeps
            .checked_add(1)
            .ok_or(SolverError::CounterOverflow)?;

        self.policies.extend(scratch_policies);
        self.histories.extend(scratch_histories);
        self.approx_memory_bytes = memory_bytes;
        self.total_deal_attempts = total_deal_attempts;
        self.terminal_evaluations = terminal_evaluations;
        self.hand_updates = hand_updates;
        self.traversals = traversals;
        self.next_sample_id = next_sample_id;
        self.completed_sweeps = completed_sweeps;
        self.apply_early_discount();
        Ok(())
    }

    /// Dense-mode counterpart of [`Self::merge_sweep`]: applies every seat's
    /// column adds directly into the preallocated arena, in seat order, with
    /// the same [`checked_add_f32`] used by the sparse path (so accumulation
    /// order -- and therefore the result -- is independent of thread count).
    /// There is nothing to "ensure" (no `EnsurePolicy`/`EnsureHistory`
    /// bookkeeping): every column already exists; only its touched bit and
    /// values change.
    fn merge_sweep_dense(&mut self, deltas: Vec<DenseTraversalDelta>) -> Result<(), SolverError> {
        let num_players = self.game.num_players();
        if deltas.len() != num_players {
            return Err(SolverError::InvalidState(
                "parallel sweep did not produce one delta per seat",
            ));
        }
        let dense = self
            .dense
            .as_mut()
            .expect("merge_sweep_dense only called when dense storage exists");

        let mut total_deal_attempts = self.total_deal_attempts;
        let mut terminal_evaluations = self.terminal_evaluations;
        let mut hand_updates = self.hand_updates;

        for (seat, delta) in deltas.into_iter().enumerate() {
            let expected_sample_id = self
                .next_sample_id
                .checked_add(seat as u64)
                .ok_or(SolverError::CounterOverflow)?;
            if delta.sample_id != expected_sample_id || delta.traverser != seat {
                return Err(SolverError::InvalidState(
                    "parallel traversal deltas are not in sample-id order",
                ));
            }
            total_deal_attempts = total_deal_attempts
                .checked_add(delta.deal_attempts)
                .ok_or(SolverError::CounterOverflow)?;
            terminal_evaluations = terminal_evaluations
                .checked_add(delta.terminal_evaluations)
                .ok_or(SolverError::CounterOverflow)?;
            hand_updates = hand_updates
                .checked_add(delta.hand_updates)
                .ok_or(SolverError::CounterOverflow)?;

            for event in delta.events {
                match event {
                    DenseEvent::AddRegret { column, values } => {
                        let range = dense.arena.slot_range_for_column(column, values.len())?;
                        dense.arena.touched_set(column);
                        // Regret floor, Pluribus-style: only meaningful once
                        // pruning is enabled (it exists to bound how long a
                        // heavily-pruned action needs to recover once it
                        // stops being a pruning candidate, and to keep the
                        // ever-more-negative accumulation from overflowing
                        // f32); 5% more negative than `prune_threshold` so a
                        // floored regret still satisfies the "below
                        // threshold" pruning test.
                        let floor = self
                            .config
                            .prune
                            .then_some((1.05 * self.config.prune_threshold) as f32);
                        for (target, value) in dense.arena.regrets[range].iter_mut().zip(values) {
                            checked_add_f32(target, value)?;
                            if let Some(floor) = floor
                                && *target < floor
                            {
                                *target = floor;
                            }
                        }
                    }
                    DenseEvent::AddStrategy { column, values } => {
                        let range = dense.arena.slot_range_for_column(column, values.len())?;
                        dense.arena.touched_set(column);
                        for (target, value) in
                            dense.arena.strategy_sum[range].iter_mut().zip(values)
                        {
                            checked_add_f32(target, value)?;
                        }
                    }
                }
            }
        }

        let added = num_players as u64;
        self.traversals = self
            .traversals
            .checked_add(added)
            .ok_or(SolverError::CounterOverflow)?;
        self.next_sample_id = self
            .next_sample_id
            .checked_add(added)
            .ok_or(SolverError::CounterOverflow)?;
        self.completed_sweeps = self
            .completed_sweeps
            .checked_add(1)
            .ok_or(SolverError::CounterOverflow)?;
        self.total_deal_attempts = total_deal_attempts;
        self.terminal_evaluations = terminal_evaluations;
        self.hand_updates = hand_updates;
        self.apply_early_discount();
        Ok(())
    }

    /// Runs a resumable number of individual player traversals.  Traversers
    /// rotate in seat order; one complete rotation is a sweep.
    #[cfg(test)]
    fn run_traversals(&mut self, count: u64) -> Result<(), SolverError> {
        let num_players = self.game.num_players();
        for _ in 0..count {
            let traverser = (self.traversals % num_players as u64) as usize;
            let mut deal_rng = traversal_deal_rng(self.config.seed, self.next_sample_id, traverser);
            let sample = self.sampler.sample_counted(&mut deal_rng)?;
            let mut action_rng =
                traversal_action_rng(self.config.seed, self.next_sample_id, traverser);
            let mut reach = vec![1.0; num_players];
            let root = self.game.root_state();
            self.traverse(
                root,
                &sample.world,
                traverser,
                HistoryKey::ROOT,
                &mut reach,
                1.0,
                &mut action_rng,
                0,
            )?;

            self.total_deal_attempts = self
                .total_deal_attempts
                .checked_add(u64::from(sample.attempts))
                .ok_or(SolverError::CounterOverflow)?;
            self.traversals = self
                .traversals
                .checked_add(1)
                .ok_or(SolverError::CounterOverflow)?;
            self.next_sample_id = self
                .next_sample_id
                .checked_add(1)
                .ok_or(SolverError::CounterOverflow)?;
            let previous_sweeps = self.completed_sweeps;
            self.completed_sweeps = self.traversals / num_players as u64;
            if self.completed_sweeps != previous_sweeps {
                self.apply_early_discount();
            }
        }
        Ok(())
    }

    pub fn current_strategy(&self, key: InfoKey) -> Option<Vec<f32>> {
        match &self.dense {
            None => self.policies.get(&key).map(PolicyColumn::current_strategy),
            Some(dense) => dense
                .column_view(key)
                .map(|view| regret_matching_f32(view.regrets)),
        }
    }

    pub fn average_strategy(&self, key: InfoKey) -> Option<Vec<f32>> {
        match &self.dense {
            None => self.policies.get(&key).map(PolicyColumn::average_strategy),
            Some(dense) => dense.column_view(key).map(|view| {
                normalize_nonnegative_f32(view.strategy_sum)
                    .unwrap_or_else(|| regret_matching_f32(view.regrets))
            }),
        }
    }

    pub fn current_action_probabilities(&self, key: InfoKey) -> Option<Vec<ActionProbability>> {
        match &self.dense {
            None => self
                .policies
                .get(&key)
                .map(PolicyColumn::current_action_probabilities),
            Some(dense) => dense.column_view(key).map(|view| {
                label_probabilities(view.action_labels, regret_matching_f32(view.regrets))
            }),
        }
    }

    pub fn average_action_probabilities(&self, key: InfoKey) -> Option<Vec<ActionProbability>> {
        match &self.dense {
            None => self
                .policies
                .get(&key)
                .map(PolicyColumn::average_action_probabilities),
            Some(dense) => dense.column_view(key).map(|view| {
                let probabilities = normalize_nonnegative_f32(view.strategy_sum)
                    .unwrap_or_else(|| regret_matching_f32(view.regrets));
                label_probabilities(view.action_labels, probabilities)
            }),
        }
    }

    pub fn policy(&self, key: InfoKey) -> Option<PolicyColumn> {
        match &self.dense {
            None => self.policies.get(&key).cloned(),
            Some(dense) => dense.column_view(key).map(|view| PolicyColumn {
                action_labels: view.action_labels.to_vec(),
                regrets: view.regrets.to_vec(),
                strategy_sum: view.strategy_sum.to_vec(),
            }),
        }
    }

    pub fn history_entry(&self, key: HistoryKey) -> Option<HistoryEntry> {
        match &self.dense {
            None => self.histories.get(&key).cloned(),
            Some(dense) => {
                let &node_id = dense.tree.by_history.get(&key)?;
                let node = &dense.tree.nodes[node_id as usize];
                let parent_id = node.parent?;
                let parent = &dense.tree.nodes[parent_id as usize];
                Some(HistoryEntry {
                    key,
                    parent: parent.history,
                    actor: parent.actor,
                    action_index: node.parent_action_index,
                    action_label: parent.action_labels[node.parent_action_index as usize].clone(),
                })
            }
        }
    }

    /// All visited history entries whose parent is `parent`, sorted by
    /// `(actor, action_index)` for a stable UI order. `O(histories)` scan in
    /// sparse mode; `O(node's actions)` in dense mode (the enumerated tree
    /// already knows every child, touched or not). Meant for a UI polling
    /// live progress, not the hot traversal loop.
    pub fn node_children(&self, parent: HistoryKey) -> Vec<HistoryEntry> {
        let mut children: Vec<HistoryEntry> = match &self.dense {
            None => self
                .histories
                .values()
                .filter(|entry| entry.parent == parent)
                .cloned()
                .collect(),
            Some(dense) => {
                let Some(&node_id) = dense.tree.by_history.get(&parent) else {
                    return Vec::new();
                };
                let node = &dense.tree.nodes[node_id as usize];
                (0..node.action_labels.len())
                    .map(|action_index| HistoryEntry {
                        key: parent.child(node.actor as usize, action_index),
                        parent,
                        actor: node.actor,
                        action_index: action_index as u32,
                        action_label: node.action_labels[action_index].clone(),
                    })
                    .collect()
            }
        };
        children.sort_unstable_by_key(|entry| (entry.actor, entry.action_index));
        children
    }

    /// Current average strategy of every policy column at `history`, sorted
    /// by key. In dense mode only *touched* buckets are listed, matching
    /// sparse mode's "only visited columns exist" semantics. `O(policies)`
    /// (sparse) or `O(node's buckets)` (dense) scan; meant for a UI polling
    /// live progress, not the hot traversal loop.
    pub fn strategies_at(&self, history: HistoryKey) -> Vec<(InfoKey, Vec<String>, Vec<f32>)> {
        self.strategies_at_with_mass(history)
            .into_iter()
            .map(|(key, labels, probabilities, _mass)| (key, labels, probabilities))
            .collect()
    }

    /// Same rows as [`Self::strategies_at`], plus each column's raw strategy
    /// mass: `Σ_a strategy_sum[a]` (the linear-CFR reach-weighted
    /// visitation mass -- see [`PolicyColumn::strategy_sum`]), summed in
    /// `f64` to avoid precision loss over many `f32` accumulators. This is
    /// the correct cheap weight for a live "range-wide action frequency"
    /// aggregation over a node's buckets: unlike a bucket count, it is
    /// reach-weighted, and unlike re-deriving weights from
    /// [`Self::evaluate_node_actions`], it costs nothing beyond the scan
    /// [`Self::strategies_at`] already performs.
    pub fn strategies_at_with_mass(
        &self,
        history: HistoryKey,
    ) -> Vec<(InfoKey, Vec<String>, Vec<f32>, f64)> {
        let mut rows: Vec<(InfoKey, Vec<String>, Vec<f32>, f64)> = match &self.dense {
            None => self
                .policies
                .iter()
                .filter(|(key, _)| key.history == history)
                .map(|(&key, column)| {
                    let mass = column
                        .strategy_sum
                        .iter()
                        .map(|&value| f64::from(value))
                        .sum();
                    (
                        key,
                        column.action_labels.clone(),
                        column.average_strategy(),
                        mass,
                    )
                })
                .collect(),
            Some(dense) => {
                let Some(&node_id) = dense.tree.by_history.get(&history) else {
                    return Vec::new();
                };
                let node = &dense.tree.nodes[node_id as usize];
                let bucket_count = dense.arena.bucket_count_of(node_id);
                let mut rows = Vec::new();
                for bucket in 0..bucket_count {
                    let column = dense
                        .arena
                        .column_id(node_id, bucket)
                        .expect("bucket is within this node's range");
                    if !dense.arena.is_touched(column) {
                        continue;
                    }
                    let range = dense
                        .arena
                        .slot_range(node_id, bucket)
                        .expect("bucket is within this node's range");
                    let strategy_sum = &dense.arena.strategy_sum[range.clone()];
                    let mass = strategy_sum.iter().map(|&value| f64::from(value)).sum();
                    let probabilities = normalize_nonnegative_f32(strategy_sum)
                        .unwrap_or_else(|| regret_matching_f32(&dense.arena.regrets[range]));
                    rows.push((
                        dense.info_key_for(node_id, bucket),
                        node.action_labels.clone(),
                        probabilities,
                        mass,
                    ));
                }
                rows
            }
        };
        rows.sort_unstable_by_key(|(key, _, _, _)| *key);
        rows
    }

    /// Resolves a public-history hash to its root-to-node action-label path.
    pub fn resolve_history(&self, mut key: HistoryKey) -> Option<Vec<String>> {
        let mut reversed = Vec::new();
        while key != HistoryKey::ROOT {
            let entry = self.history_entry(key)?;
            reversed.push(entry.action_label);
            key = entry.parent;
        }
        reversed.reverse();
        Some(reversed)
    }

    pub fn snapshot_state(&self) -> SolverState {
        match &self.dense {
            None => {
                let mut histories: Vec<_> = self.histories.values().cloned().collect();
                histories.sort_unstable_by_key(|entry| entry.key);
                let mut policies: Vec<_> = self
                    .policies
                    .iter()
                    .map(|(&key, column)| PolicyEntry {
                        key,
                        column: column.clone(),
                    })
                    .collect();
                policies.sort_unstable_by_key(|entry| entry.key);
                self.finish_snapshot(histories, policies)
            }
            Some(dense) => {
                let mut policies = Vec::new();
                let mut history_node_ids: std::collections::BTreeSet<NodeId> =
                    std::collections::BTreeSet::new();
                for (node_index, node) in dense.tree.nodes.iter().enumerate() {
                    let node_id = node_index as NodeId;
                    let bucket_count = dense.arena.bucket_count_of(node_id);
                    for bucket in 0..bucket_count {
                        let column = dense
                            .arena
                            .column_id(node_id, bucket)
                            .expect("bucket is within this node's range");
                        if !dense.arena.is_touched(column) {
                            continue;
                        }
                        let range = dense
                            .arena
                            .slot_range(node_id, bucket)
                            .expect("bucket is within this node's range");
                        policies.push(PolicyEntry {
                            key: dense.info_key_for(node_id, bucket),
                            column: PolicyColumn {
                                action_labels: node.action_labels.clone(),
                                regrets: dense.arena.regrets[range.clone()].to_vec(),
                                strategy_sum: dense.arena.strategy_sum[range].to_vec(),
                            },
                        });
                        // Record every ancestor of a touched column (the
                        // reachable trie a viewer needs), stopping at the
                        // first already-recorded ancestor (its own ancestors
                        // are therefore already present) or at the implicit
                        // root (which never gets a `HistoryEntry`).
                        let mut ancestor = Some(node_id);
                        while let Some(id) = ancestor {
                            let ancestor_node = &dense.tree.nodes[id as usize];
                            if ancestor_node.parent.is_none() {
                                break;
                            }
                            if !history_node_ids.insert(id) {
                                break;
                            }
                            ancestor = ancestor_node.parent;
                        }
                    }
                }
                policies.sort_unstable_by_key(|entry| entry.key);
                let mut histories: Vec<HistoryEntry> = history_node_ids
                    .into_iter()
                    .map(|node_id| {
                        let node = &dense.tree.nodes[node_id as usize];
                        let parent_id = node.parent.expect("root excluded above");
                        let parent = &dense.tree.nodes[parent_id as usize];
                        HistoryEntry {
                            key: node.history,
                            parent: parent.history,
                            actor: parent.actor,
                            action_index: node.parent_action_index,
                            action_label: parent.action_labels[node.parent_action_index as usize]
                                .clone(),
                        }
                    })
                    .collect();
                histories.sort_unstable_by_key(|entry| entry.key);
                self.finish_snapshot(histories, policies)
            }
        }
    }

    fn finish_snapshot(
        &self,
        histories: Vec<HistoryEntry>,
        policies: Vec<PolicyEntry>,
    ) -> SolverState {
        SolverState {
            schema_version: SOLVER_STATE_VERSION,
            config: self.config,
            traversals: self.traversals,
            completed_sweeps: self.completed_sweeps,
            next_sample_id: self.next_sample_id,
            total_deal_attempts: self.total_deal_attempts,
            terminal_evaluations: self.terminal_evaluations,
            hand_updates: self.hand_updates,
            histories,
            policies,
        }
    }

    /// Completed-sweep counter without the O(infosets) work of [`metrics`].
    /// Drive loops should use this for chunk bookkeeping and reserve
    /// [`metrics`] for boundaries where the full diagnostics are consumed.
    pub fn completed_sweeps(&self) -> u64 {
        self.completed_sweeps
    }

    /// Cumulative individual player traversals so far (see
    /// [`SolverState::traversals`]), without the O(infosets) work of
    /// [`metrics`]. Cheap enough for a rate computation every drive-loop
    /// chunk.
    pub fn traversals(&self) -> u64 {
        self.traversals
    }

    /// Cumulative hand updates so far (see [`SolverState::hand_updates`]),
    /// without the O(infosets) work of [`metrics`]. Cheap enough for a rate
    /// computation every drive-loop chunk.
    pub fn hand_updates(&self) -> u64 {
        self.hand_updates
    }

    /// Dense-arena preflight numbers, or `None` in `RecallMode::Full` (there
    /// is no arena to report on). Useful for a CLI/GUI to print what the
    /// `RecallMode::Street` preallocation actually cost before training.
    pub fn dense_arena_stats(&self) -> Option<DenseArenaStats> {
        self.dense.as_ref().map(|dense| DenseArenaStats {
            node_count: dense.arena.node_count() as u64,
            total_columns: dense.arena.total_columns(),
            total_slots: dense.arena.total_slots(),
            estimated_bytes: dense.arena.estimated_bytes(),
        })
    }

    /// Per-seat average-strategy L1 drift since `prior`, refreshing `prior`
    /// in place. Keys are visited in sorted order, so the per-seat f64 sums
    /// are identical to computing the same statistic from a sorted
    /// [`snapshot_state`] — without cloning every policy column.
    pub fn strategy_drift_refresh(
        &self,
        prior: &mut std::collections::HashMap<InfoKey, Vec<f32>>,
    ) -> Vec<f64> {
        let num_players = self.game.num_players();
        let mut totals = vec![0.0; num_players];
        let mut counts = vec![0u64; num_players];
        match &self.dense {
            None => {
                let mut keys: Vec<_> = self.policies.keys().copied().collect();
                keys.sort_unstable();
                for key in keys {
                    let column = self.policies.get(&key).expect("key came from policy map");
                    let current = column.average_strategy();
                    let value = prior.get(&key).map_or(0.0, |previous| {
                        current
                            .iter()
                            .zip(previous)
                            .map(|(&left, &right)| f64::from((left - right).abs()))
                            .sum::<f64>()
                            * 0.5
                    });
                    totals[key.player as usize] += value;
                    counts[key.player as usize] += 1;
                    prior.insert(key, current);
                }
            }
            Some(dense) => {
                for_each_touched_column(dense, |key, node, range| {
                    let current =
                        normalize_nonnegative_f32(&dense.arena.strategy_sum[range.clone()])
                            .unwrap_or_else(|| regret_matching_f32(&dense.arena.regrets[range]));
                    let value = prior.get(&key).map_or(0.0, |previous| {
                        current
                            .iter()
                            .zip(previous)
                            .map(|(&left, &right)| f64::from((left - right).abs()))
                            .sum::<f64>()
                            * 0.5
                    });
                    totals[node.actor as usize] += value;
                    counts[node.actor as usize] += 1;
                    prior.insert(key, current);
                });
            }
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

    pub fn metrics(&self) -> SolverMetrics {
        let num_players = self.game.num_players();
        let mut positive_regret = vec![0.0; num_players];
        let (infosets, memory_bytes) = match &self.dense {
            None => {
                let mut keys: Vec<_> = self.policies.keys().copied().collect();
                keys.sort_unstable();
                for key in keys {
                    let column = self.policies.get(&key).expect("key came from policy map");
                    positive_regret[key.player as usize] += column
                        .regrets
                        .iter()
                        .map(|&regret| f64::from(regret.max(0.0)))
                        .sum::<f64>();
                }
                (self.policies.len() as u64, self.approx_memory_bytes)
            }
            Some(dense) => {
                for_each_touched_column(dense, |_, node, range| {
                    positive_regret[node.actor as usize] += dense.arena.regrets[range]
                        .iter()
                        .map(|&regret| f64::from(regret.max(0.0)))
                        .sum::<f64>();
                });
                (dense.arena.touched_count(), dense.arena.estimated_bytes())
            }
        };
        for (player, total) in positive_regret.iter_mut().enumerate() {
            let updates = traversals_for_player(self.traversals, num_players, player);
            if updates > 0 {
                *total /= updates as f64;
            }
        }
        SolverMetrics {
            sweeps: self.completed_sweeps,
            traversals: self.traversals,
            terminal_evaluations: self.terminal_evaluations,
            infosets,
            memory_bytes,
            total_deal_attempts: self.total_deal_attempts,
            mean_deal_attempts: if self.traversals == 0 {
                0.0
            } else {
                self.total_deal_attempts as f64 / self.traversals as f64
            },
            hand_updates: self.hand_updates,
            average_positive_regret: positive_regret,
        }
    }

    #[cfg(test)]
    fn strategy_for(
        &mut self,
        key: InfoKey,
        actions: &G::Actions,
    ) -> Result<Vec<f64>, SolverError> {
        let num_actions = self.game.num_actions_of(actions);
        if let Some(column) = self.policies.get(&key) {
            if column.num_actions() != num_actions {
                return Err(SolverError::ActionCountChanged {
                    key,
                    stored: column.num_actions(),
                    current: num_actions,
                });
            }
            let mut scratch = String::new();
            for (index, label) in column.action_labels.iter().enumerate() {
                scratch.clear();
                self.game.write_action_label(actions, index, &mut scratch);
                if *label != scratch {
                    return Err(SolverError::ActionLabelsChanged { key });
                }
            }
            return Ok(regret_matching(&column.regrets));
        }

        let action_labels: Vec<String> = (0..num_actions)
            .map(|index| self.game.action_label_of(actions, index))
            .collect();
        validate_action_labels(&action_labels)?;

        let added = entry_memory_bytes(&action_labels)?;
        let needed = self
            .approx_memory_bytes
            .checked_add(added)
            .ok_or(SolverError::MemoryAccountingOverflow)?;
        if needed > self.config.max_memory_bytes {
            return Err(SolverError::MemoryLimit {
                limit: self.config.max_memory_bytes,
                needed,
            });
        }
        self.policies
            .insert(key, PolicyColumn::zeroed(action_labels));
        self.approx_memory_bytes = needed;
        Ok(vec![1.0 / num_actions as f64; num_actions])
    }

    #[cfg(test)]
    fn record_history(
        &mut self,
        parent: HistoryKey,
        actor: usize,
        action_index: usize,
        action_label: &str,
    ) -> Result<HistoryKey, SolverError> {
        let key = parent.child(actor, action_index);
        if let Some(existing) = self.histories.get(&key) {
            if existing.parent != parent
                || existing.actor as usize != actor
                || existing.action_index as usize != action_index
                || existing.action_label != action_label
            {
                return Err(SolverError::HistoryCollision(key));
            }
            return Ok(key);
        }
        let actor = u8::try_from(actor).map_err(|_| SolverError::HistoryIndexOverflow)?;
        let action_index =
            u32::try_from(action_index).map_err(|_| SolverError::HistoryIndexOverflow)?;
        let added = history_memory_bytes(action_label)?;
        let needed = self
            .approx_memory_bytes
            .checked_add(added)
            .ok_or(SolverError::MemoryAccountingOverflow)?;
        if needed > self.config.max_memory_bytes {
            return Err(SolverError::MemoryLimit {
                limit: self.config.max_memory_bytes,
                needed,
            });
        }
        self.histories.insert(
            key,
            HistoryEntry {
                key,
                parent,
                actor,
                action_index,
                action_label: action_label.to_string(),
            },
        );
        self.approx_memory_bytes = needed;
        Ok(key)
    }

    fn apply_early_discount(&mut self) {
        let sweep = self.completed_sweeps;
        if self.config.discount_until == 0
            || sweep >= self.config.discount_until
            || !sweep.is_multiple_of(self.config.discount_every)
        {
            return;
        }
        let event = sweep / self.config.discount_every;
        let factor = (event as f64 / (event + 1) as f64) as f32;
        match &mut self.dense {
            None => {
                let mut keys: Vec<_> = self.policies.keys().copied().collect();
                keys.sort_unstable();
                for key in keys {
                    let column = self.policies.get_mut(&key).expect("key came from map");
                    for value in column.regrets.iter_mut().chain(&mut column.strategy_sum) {
                        *value *= factor;
                    }
                }
            }
            // The arena is one flat, node-major-ordered array covering
            // every enumerated column (touched or not); scaling every slot
            // linearly is equivalent to (and cheaper than) sorting and
            // scaling only the touched ones, since an untouched slot is
            // always zero and `0.0 * factor == 0.0`.
            Some(dense) => {
                for value in dense
                    .arena
                    .regrets
                    .iter_mut()
                    .chain(dense.arena.strategy_sum.iter_mut())
                {
                    *value *= factor;
                }
            }
        }
    }
}
