//! Dense current-street external-sampling MCCFR for sampled multiway games.
//!
//! This module intentionally does not call its diagnostics exploitability or
//! Nash convergence: with more than two players the game is general-sum, and
//! bucket abstraction may also introduce imperfect recall.  The solver
//! exposes sampled regret diagnostics and average policies without claiming
//! a two-player zero-sum guarantee.

use std::collections::hash_map::Entry;

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

mod averaging;
mod drift;
mod errors;
mod eval;
mod snapshot;
mod support;
mod workers;

#[cfg(test)]
mod dense_merge_tests;
#[cfg(test)]
mod tests;

pub use drift::{StrategyDriftError, StrategyDriftTracker};
pub use errors::SolverError;
pub(crate) use snapshot::SnapshotError;

use averaging::*;
use drift::StrategyDriftIdentity;
use support::*;
use workers::*;

/// Solver-state version 4 keys range-vector combo-bucket caches by both street
/// and the number of opponents active at that street's start. Version 3
/// introduced scalar-equivalent conditional regret weighting and the
/// independent full-support average-policy pass, but its street-only cache
/// could reuse buckets across counterfactual branches with different player
/// counts. Older checkpoints cannot be resumed without mixing update rules.
pub const SOLVER_STATE_VERSION: u16 = 4;
pub const DEFAULT_EXPLORATION_EPSILON: f64 = 0.06;
pub const UNREACHED_BUCKET: BucketId = u32::MAX;
pub const DEFAULT_DISCOUNT_EVERY: u64 = 100_000;
pub const DEFAULT_DISCOUNT_UNTIL: u64 = 10_000_000;
pub const DEFAULT_PRUNE_THRESHOLD: f64 = -1.0e6;
pub const DEFAULT_PRUNE_SKIP_PROBABILITY: f64 = 0.95;
/// Minimum training visits before an infoset's trained action enters a
/// [`DeviatorPolicy`]; see [`MultiwaySolver::train_deviator`].
pub const MIN_DEVIATOR_POLICY_VISITS: u32 = 8;

fn finalize_drift(totals: Vec<f64>, counts: Vec<u64>) -> Vec<f64> {
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

fn refresh_dense_drift_in_place(
    dense: &DenseStorage,
    tracker: &mut StrategyDriftTracker,
    num_players: usize,
) -> Result<Vec<f64>, StrategyDriftError> {
    let mut totals = vec![0.0; num_players];
    let mut counts = vec![0u64; num_players];
    let mut index = 0usize;
    let mut offset = 0usize;
    let mut failure = None;
    for_each_touched_column(dense, |column, _key, node, range| {
        if failure.is_some() {
            return;
        }
        if tracker.dense_columns.get(index).copied() != Some(column) {
            failure = Some(StrategyDriftError::IncompatibleLayout);
            return;
        }
        let action_count = tracker.dense_action_counts[index] as usize;
        let Some(end) = offset.checked_add(action_count) else {
            failure = Some(StrategyDriftError::CorruptTracker);
            return;
        };
        let Some(previous) = tracker.dense_probabilities.get_mut(offset..end) else {
            failure = Some(StrategyDriftError::CorruptTracker);
            return;
        };
        let current = normalize_nonnegative_f32(&dense.arena.strategy_sum[range.clone()])
            .unwrap_or_else(|| regret_matching_f32(&dense.arena.regrets[range]));
        if previous.len() != current.len() {
            failure = Some(StrategyDriftError::IncompatibleLayout);
            return;
        }
        let value = current
            .iter()
            .zip(previous.iter())
            .map(|(&left, &right)| f64::from((left - right).abs()))
            .sum::<f64>()
            * 0.5;
        previous.copy_from_slice(&current);
        totals[node.actor as usize] += value;
        counts[node.actor as usize] += 1;
        index += 1;
        offset = end;
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if index != tracker.dense_columns.len() || offset != tracker.dense_probabilities.len() {
        return Err(StrategyDriftError::CorruptTracker);
    }
    Ok(finalize_drift(totals, counts))
}

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

    /// Identity of the concrete card-abstraction backend used by
    /// [`Self::bucket`]. [`MultiwaySolver::abstraction_fingerprint`] composes
    /// [`Self::recall_mode`] into this backend identity so checkpoint and
    /// artifact compatibility also covers the policy's private-information
    /// semantics.
    fn abstraction_fingerprint(&self) -> [u8; 32] {
        [0; 32]
    }

    /// Private-recall storage mode the solver should use for this game.
    /// Adapters must opt into [`RecallMode::Street`] to construct a solver.
    /// The historical default remains available for identity and rejection.
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

/// Apply one event in place, replacing each successfully consumed delta with
/// its old f32 slot value. On error, the count names only journaled slots.
fn apply_dense_event_journaled(
    arena: &mut DenseArena,
    event: &mut DenseEvent,
    regret_floor: Option<f32>,
) -> Result<(), (usize, SolverError)> {
    let (column, values, is_regret) = match event {
        DenseEvent::AddRegret { column, values } => (*column, values, true),
        DenseEvent::AddStrategy { column, values } => (*column, values, false),
    };
    let range = arena
        .slot_range_for_column(column, values.len())
        .map_err(|error| (0, SolverError::from(error)))?;
    let targets = if is_regret {
        &mut arena.regrets[range]
    } else {
        &mut arena.strategy_sum[range]
    };
    for (written, (target, value)) in targets.iter_mut().zip(values).enumerate() {
        let previous = *target;
        checked_add_f32(target, *value).map_err(|error| (written, error))?;
        // A successful checked addition implies the previous value was finite.
        // Widening f32 to f64 and back preserves all bits, including signed zero.
        *value = f64::from(previous);
        if is_regret
            && let Some(floor) = regret_floor
            && *target < floor
        {
            *target = floor;
        }
    }
    Ok(())
}

/// Restore exactly the processed prefix, reversing overlaps as well as seats.
/// The failing event's unprocessed suffix still contains original deltas.
fn rollback_dense_events(
    arena: &mut DenseArena,
    deltas: &[DenseTraversalDelta],
    failed_seat: usize,
    failed_event: usize,
    written: usize,
) {
    for seat in (0..=failed_seat).rev() {
        let events = &deltas[seat].events;
        let end = if seat == failed_seat {
            failed_event + 1
        } else {
            events.len()
        };
        for event in (0..end).rev() {
            let (column, values, is_regret) = match &events[event] {
                DenseEvent::AddRegret { column, values } => (*column, values, true),
                DenseEvent::AddStrategy { column, values } => (*column, values, false),
            };
            let count = if seat == failed_seat && event == failed_event {
                written
            } else {
                values.len()
            };
            // An invalid column/shape or first-slot failure wrote nothing; do
            // not resolve that unvalidated range, even during error recovery.
            if count == 0 {
                continue;
            }
            let range = arena
                .slot_range_for_column(column, values.len())
                .expect("journaled event range was validated before writing");
            let targets = if is_regret {
                &mut arena.regrets[range]
            } else {
                &mut arena.strategy_sum[range]
            };
            for slot in (0..count).rev() {
                targets[slot] = values[slot] as f32;
            }
        }
    }
}

/// Normalize a vector traversal's feasible own-hand weights conditional on
/// the sampled opponents and board.
///
/// The deal sampler first draws a complete physical world. After the sampled
/// traverser's hand is discarded, the retained context has marginal
/// probability proportional to the total feasible own-range mass `W_F`.
/// Therefore a Rao-Blackwellized update must use `w(h) / W_F`; using raw
/// weights (or normalizing separately inside each bucket) counts contexts
/// with large `W_F` too often and does not have the scalar estimator's
/// expectation.
fn normalize_feasible_weights(weights: &mut [f64]) -> Result<(), SolverError> {
    let total = weights.iter().sum::<f64>();
    if !total.is_finite() || total <= 0.0 {
        return Err(SolverError::InvalidState(
            "vector traversal has non-positive or non-finite feasible range mass",
        ));
    }
    for weight in weights {
        *weight /= total;
    }
    Ok(())
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicActionDestination {
    PreflopDecision(HistoryKey),
    PostflopBoundary,
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicNodeAction {
    pub label: String,
    pub destination: PublicActionDestination,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicNodeView {
    pub history: HistoryKey,
    pub actor: u8,
    pub street: Street,
    pub active_opponents: u8,
    pub actions: Vec<PublicNodeAction>,
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
    /// Cumulative linear own-reach-weighted strategy numerators. Every exact
    /// public history also carries its fixed uniform-opponent proposal factor
    /// `Q(history)`. That factor cancels in [`Self::average_strategy`] and is
    /// shared by all buckets at the same history, but raw sums are neither
    /// reach probabilities nor comparable across different histories.
    pub strategy_sum: Vec<f32>,
}

impl PolicyColumn {
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
    /// Conservative storage-payload cap for the dense arena estimate. The
    /// retained public tree, abstraction caches, worker scratch, evaluation, allocator overhead, and artifact
    /// staging are additional process memory and require an external process
    /// limit/headroom policy.
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
    /// combo the deal sampler happened to deal that seat. Uses the dense
    /// current-street vector worker. `false` (the default) is the original one-hand-per-traversal algorithm,
    /// byte-identical to before this field existed.
    pub traverser_vector: bool,
    /// Enables Pluribus-style regret-based pruning at traverser decision
    /// nodes in vector mode: a (bucket, action) whose regret-matched
    /// probability is exactly zero and whose accumulated regret is below
    /// [`Self::prune_threshold`] is skipped (with probability
    /// [`Self::prune_skip_probability`]) rather than descended into, saving
    /// the traversal work that would only ever multiply by a zero
    /// probability. This optimization is implemented only by the dense
    /// street-recall vector worker. `false` (the default) is byte-identical to
    /// before this field existed. [`validate_setup`] rejects `true` unless
    /// [`Self::traverser_vector`] is also `true` and the game uses
    /// [`RecallMode::Street`].
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SolverMetrics {
    pub sweeps: u64,
    pub traversals: u64,
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

/// Fixed policy-arena allocation completed before a production solver is
/// returned. Counts cover every reachable public decision node and every
/// current-street bucket at that node, whether or not a traversal has
/// visited the corresponding information set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyArenaAllocation {
    pub nodes: u64,
    pub columns: u64,
    pub slots: u64,
    pub bytes: u64,
    pub pages_committed: bool,
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
    /// Per-seat held-out estimate for unilateral deviations against the
    /// opponents' average profile. The regret-greedy candidate is always
    /// evaluated; when trained deviators are supplied, the reported interval
    /// is a simultaneous envelope over both finite candidates (plus the
    /// no-deviation option). This is a candidate-policy diagnostic, not a
    /// best response, exploitability, or Nash-convergence claim.
    pub deviation_gain_lower_bound: Option<Vec<ProfileEstimate>>,
    /// Baseline-profile strategy sources, indexed by acting seat. Candidate
    /// deviation trajectories are excluded. Empty in older JSON reports.
    #[serde(default)]
    pub candidate_policy_coverage: Vec<CandidatePolicyCoverage>,
}

/// Reach-weighted coverage of the candidate profile on unmodified held-out
/// baseline trajectories.
///
/// Each sampled decision is attributed to the acting seat. A stored strategy
/// means the candidate solver had a policy column for that concrete
/// information key; otherwise evaluation used the documented uniform
/// fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreetVisitCounts {
    pub preflop: u64,
    pub flop: u64,
    pub turn: u64,
    pub river: u64,
}

impl StreetVisitCounts {
    /// Returns the count for one street.
    pub const fn get(self, street: Street) -> u64 {
        match street {
            Street::Preflop => self.preflop,
            Street::Flop => self.flop,
            Street::Turn => self.turn,
            Street::River => self.river,
        }
    }

    /// Sum of all four street counters.
    pub const fn total(self) -> u64 {
        self.preflop
            .saturating_add(self.flop)
            .saturating_add(self.turn)
            .saturating_add(self.river)
    }

    fn checked_increment(&mut self, street: u8) -> Result<(), SolverError> {
        let count = match street {
            0 => &mut self.preflop,
            1 => &mut self.flop,
            2 => &mut self.turn,
            3 => &mut self.river,
            _ => {
                return Err(SolverError::InvalidPrivateInfo("street is outside 0..=3"));
            }
        };
        *count = count.checked_add(1).ok_or(SolverError::CounterOverflow)?;
        Ok(())
    }

    fn checked_add_assign(&mut self, other: Self) -> Result<(), SolverError> {
        self.preflop = self
            .preflop
            .checked_add(other.preflop)
            .ok_or(SolverError::CounterOverflow)?;
        self.flop = self
            .flop
            .checked_add(other.flop)
            .ok_or(SolverError::CounterOverflow)?;
        self.turn = self
            .turn
            .checked_add(other.turn)
            .ok_or(SolverError::CounterOverflow)?;
        self.river = self
            .river
            .checked_add(other.river)
            .ok_or(SolverError::CounterOverflow)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePolicyCoverage {
    pub decision_visits: u64,
    /// Visits to a stored column, including current-regret fallback when its
    /// average mass is zero. This legacy counter measures storage coverage,
    /// not average-policy coverage.
    pub stored_strategy_visits: u64,
    pub uniform_fallback_visits: u64,
    /// Visits that used a stored, positive-mass average strategy.
    #[serde(default)]
    pub average_strategy_visits: u64,
    /// Visits that explicitly requested the current strategy via
    /// [`ProfileVariant::use_current_strategy`].
    #[serde(default)]
    pub current_strategy_visits: u64,
    /// Visits requesting the average that instead used regret matching
    /// because the stored column had zero average mass.
    #[serde(default)]
    pub regret_fallback_visits: u64,
    /// Decision visits attributed to the public street of the candidate
    /// information key. This sums to [`Self::decision_visits`].
    #[serde(default)]
    pub decision_visits_by_street: StreetVisitCounts,
    /// Stored-policy visits by street. This sums to
    /// [`Self::stored_strategy_visits`].
    #[serde(default)]
    pub stored_strategy_visits_by_street: StreetVisitCounts,
    /// Uniform candidate fallback visits by street. This sums to
    /// [`Self::uniform_fallback_visits`].
    #[serde(default)]
    pub uniform_fallback_visits_by_street: StreetVisitCounts,
    #[serde(default)]
    pub average_strategy_visits_by_street: StreetVisitCounts,
    #[serde(default)]
    pub current_strategy_visits_by_street: StreetVisitCounts,
    #[serde(default)]
    pub regret_fallback_visits_by_street: StreetVisitCounts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CandidatePolicySource {
    Average,
    Current,
    RegretFallback,
    UniformFallback,
}

impl CandidatePolicyCoverage {
    /// Fraction of baseline decisions served by a stored average with
    /// positive mass, excluding both forms of regret-matched strategy.
    pub fn average_strategy_fraction(self) -> f64 {
        if self.decision_visits == 0 {
            0.0
        } else {
            self.average_strategy_visits as f64 / self.decision_visits as f64
        }
    }

    /// Legacy storage coverage; use [`Self::average_strategy_fraction`] to
    /// determine whether the requested average was actually observed.
    pub fn stored_strategy_fraction(self) -> f64 {
        if self.decision_visits == 0 {
            0.0
        } else {
            self.stored_strategy_visits as f64 / self.decision_visits as f64
        }
    }

    fn record(&mut self, street: u8, source: CandidatePolicySource) -> Result<(), SolverError> {
        self.decision_visits = self
            .decision_visits
            .checked_add(1)
            .ok_or(SolverError::CounterOverflow)?;
        self.decision_visits_by_street.checked_increment(street)?;
        if source != CandidatePolicySource::UniformFallback {
            self.stored_strategy_visits = self
                .stored_strategy_visits
                .checked_add(1)
                .ok_or(SolverError::CounterOverflow)?;
            self.stored_strategy_visits_by_street
                .checked_increment(street)?;
        } else {
            self.uniform_fallback_visits = self
                .uniform_fallback_visits
                .checked_add(1)
                .ok_or(SolverError::CounterOverflow)?;
            self.uniform_fallback_visits_by_street
                .checked_increment(street)?;
        }
        let (count, by_street) = match source {
            CandidatePolicySource::Average => (
                &mut self.average_strategy_visits,
                &mut self.average_strategy_visits_by_street,
            ),
            CandidatePolicySource::Current => (
                &mut self.current_strategy_visits,
                &mut self.current_strategy_visits_by_street,
            ),
            CandidatePolicySource::RegretFallback => (
                &mut self.regret_fallback_visits,
                &mut self.regret_fallback_visits_by_street,
            ),
            CandidatePolicySource::UniformFallback => return Ok(()),
        };
        *count = count.checked_add(1).ok_or(SolverError::CounterOverflow)?;
        by_street.checked_increment(street)?;
        Ok(())
    }

    fn checked_add_assign(&mut self, other: Self) -> Result<(), SolverError> {
        for (target, value) in [
            (&mut self.decision_visits, other.decision_visits),
            (
                &mut self.stored_strategy_visits,
                other.stored_strategy_visits,
            ),
            (
                &mut self.uniform_fallback_visits,
                other.uniform_fallback_visits,
            ),
            (
                &mut self.average_strategy_visits,
                other.average_strategy_visits,
            ),
            (
                &mut self.current_strategy_visits,
                other.current_strategy_visits,
            ),
            (
                &mut self.regret_fallback_visits,
                other.regret_fallback_visits,
            ),
        ] {
            *target = target
                .checked_add(value)
                .ok_or(SolverError::CounterOverflow)?;
        }
        for (target, value) in [
            (
                &mut self.decision_visits_by_street,
                other.decision_visits_by_street,
            ),
            (
                &mut self.stored_strategy_visits_by_street,
                other.stored_strategy_visits_by_street,
            ),
            (
                &mut self.uniform_fallback_visits_by_street,
                other.uniform_fallback_visits_by_street,
            ),
            (
                &mut self.average_strategy_visits_by_street,
                other.average_strategy_visits_by_street,
            ),
            (
                &mut self.current_strategy_visits_by_street,
                other.current_strategy_visits_by_street,
            ),
            (
                &mut self.regret_fallback_visits_by_street,
                other.regret_fallback_visits_by_street,
            ),
        ] {
            target.checked_add_assign(value)?;
        }
        Ok(())
    }
}

/// A fixed per-seat deviation policy trained by [`MultiwaySolver::train_deviator`]:
/// for each information set it visited during training, the single action index
/// it deviates to. Infosets it never visited fall back to the caller's usual
/// deviation behavior. A policy trained by [`MultiwaySolver::train_deviator`]
/// is replayed by [`MultiwaySolver::evaluate_profile`].
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct ProfileVariant {
    /// Purification threshold (Ganzfried & Sandholm, AAMAS 2012): entries
    /// below the threshold are zeroed and the remainder renormalized;
    /// `0.0` skips the purify call entirely (raw average profile). Must be
    /// finite and in `[0.0, 1.0]`.
    pub purify_threshold: f32,
    /// Evaluate/train against the last-iterate regret-matched current
    /// strategy instead of the linear average profile. Diagnostic only:
    /// plain regret matching carries no last-iterate convergence guarantee.
    /// In multiplayer games, external regret gives a CCE-style statement for
    /// the correlated empirical sequence of joint play, not for the product
    /// of independently normalized per-seat average-policy columns exposed
    /// here.
    pub use_current_strategy: bool,
}

/// Dense current-street external-sampling MCCFR state. Every policy column
/// is preallocated from the enumerated public tree; touched bits distinguish
/// sampled columns from unvisited ones.
pub struct MultiwaySolver<G: ExternalSamplingGame> {
    game: G,
    sampler: DealSampler,
    config: SolverConfig,
    traversals: u64,
    completed_sweeps: u64,
    next_sample_id: u64,
    total_deal_attempts: u64,
    terminal_evaluations: u64,
    hand_updates: u64,
    dense: DenseStorage,
}

/// Composes a backend fingerprint with the private-recall semantics that
/// define an information key. Full recall deliberately preserves the
/// historical backend fingerprint byte-for-byte.
pub fn abstraction_fingerprint_with_recall(backend: [u8; 32], recall: RecallMode) -> [u8; 32] {
    match recall {
        RecallMode::Full => backend,
        RecallMode::Street => {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"solvers.multiway.abstraction.current-street.v1");
            hasher.update(&backend);
            *hasher.finalize().as_bytes()
        }
    }
}

/// Computes the resume/configuration fingerprint before a solver (and, for
/// current-street production, its dense arena) is constructed. Checkpoint
/// loaders use this to reject a mismatched header before allocating and
/// page-committing the complete policy arena.
pub fn configuration_fingerprint_for_setup<G: ExternalSamplingGame>(
    game: &G,
    sampler: &DealSampler,
    config: SolverConfig,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.solver-config.v4");
    hasher.update(&config.seed.to_le_bytes());
    hasher.update(&config.max_traversal_depth.to_le_bytes());
    hasher.update(&config.exploration_epsilon.to_bits().to_le_bytes());
    hasher.update(&config.discount_every.to_le_bytes());
    hasher.update(&config.discount_until.to_le_bytes());
    hasher.update(&config.sweep_batch.to_le_bytes());
    hasher.update(&[u8::from(config.traverser_vector)]);
    hasher.update(&[u8::from(config.prune)]);
    hasher.update(&config.prune_threshold.to_bits().to_le_bytes());
    hasher.update(&config.prune_skip_probability.to_bits().to_le_bytes());
    hasher.update(&sampler.range_fingerprint());
    hasher.update(&game.game_fingerprint());
    *hasher.finalize().as_bytes()
}

fn initialization_pool(threads: usize) -> Result<rayon::ThreadPool, SolverError> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| SolverError::ThreadPoolBuild(error.to_string()))
}

impl<G: ExternalSamplingGame> MultiwaySolver<G> {
    pub fn new(game: G, sampler: DealSampler, config: SolverConfig) -> Result<Self, SolverError> {
        Self::new_internal(
            game,
            sampler,
            config,
            false,
            tree::enumerate_tree_with_limits,
        )
    }

    /// Constructs the production storage layout. This does not return until
    /// every page of the complete current-street policy arena has been touched.
    pub fn new_preallocated(
        game: G,
        sampler: DealSampler,
        config: SolverConfig,
    ) -> Result<Self, SolverError> {
        if !matches!(game.recall_mode(), RecallMode::Street) {
            return Err(SolverError::PreallocatedStorageRequiresStreetRecall);
        }
        Self::new_internal(
            game,
            sampler,
            config,
            true,
            tree::enumerate_tree_with_limits,
        )
    }

    /// Constructs page-committed production storage using an explicitly sized
    /// private pool for public-tree materialization. Resource admission remains
    /// serial and non-retaining; successful node/arena order is identical to
    /// [`Self::new_preallocated`]. A one-thread pool uses serial enumeration.
    ///
    /// Threads are operational, not part of algorithm or checkpoint identity.
    /// Parallel merge can temporarily retain source and destination node slots;
    /// the policy-arena byte limit is not a whole-process memory limit.
    pub fn new_preallocated_with_threads(
        game: G,
        sampler: DealSampler,
        config: SolverConfig,
        threads: usize,
    ) -> Result<Self, SolverError>
    where
        G::State: Send,
    {
        if threads == 0 {
            return Err(SolverError::ZeroThreads);
        }
        if !matches!(game.recall_mode(), RecallMode::Street) {
            return Err(SolverError::PreallocatedStorageRequiresStreetRecall);
        }
        let pool = initialization_pool(threads)?;
        pool.install(|| {
            Self::new_internal(
                game,
                sampler,
                config,
                true,
                tree::enumerate_tree_with_limits_parallel,
            )
        })
    }

    fn new_internal(
        game: G,
        sampler: DealSampler,
        config: SolverConfig,
        commit_pages: bool,
        materialize: TreeMaterializer<G>,
    ) -> Result<Self, SolverError> {
        validate_setup(&game, &sampler, config)?;
        if game.recall_mode() != RecallMode::Street {
            return Err(SolverError::PreallocatedStorageRequiresStreetRecall);
        }
        let dense = DenseStorage::build(
            &game,
            config.max_memory_bytes,
            config.max_traversal_depth,
            commit_pages,
            materialize,
        )?;
        Ok(Self {
            game,
            sampler,
            config,
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
        state: SolverState,
        current_config: SolverConfig,
    ) -> Result<Self, SolverError> {
        Self::from_state_with_config_internal(
            game,
            sampler,
            state,
            current_config,
            false,
            tree::enumerate_tree_with_limits,
        )
    }

    /// Production resume counterpart of [`Self::new_preallocated`]. The full
    /// arena is rebuilt and page-committed before any checkpoint columns are
    /// replayed and before the resumed solver is returned.
    pub fn from_state_with_config_preallocated(
        game: G,
        sampler: DealSampler,
        state: SolverState,
        current_config: SolverConfig,
    ) -> Result<Self, SolverError> {
        if !matches!(game.recall_mode(), RecallMode::Street) {
            return Err(SolverError::PreallocatedStorageRequiresStreetRecall);
        }
        Self::from_state_with_config_internal(
            game,
            sampler,
            state,
            current_config,
            true,
            tree::enumerate_tree_with_limits,
        )
    }

    /// Restores production storage using the same bounded private-pool
    /// materialization as [`Self::new_preallocated_with_threads`]. State version,
    /// configuration/counter checks and serial admission precede retained tree
    /// construction; policy validation precedes page commitment and replay.
    /// Changing construction threads does not change checkpoint compatibility.
    pub fn from_state_with_config_preallocated_with_threads(
        game: G,
        sampler: DealSampler,
        state: SolverState,
        current_config: SolverConfig,
        threads: usize,
    ) -> Result<Self, SolverError>
    where
        G::State: Send,
    {
        if threads == 0 {
            return Err(SolverError::ZeroThreads);
        }
        if !matches!(game.recall_mode(), RecallMode::Street) {
            return Err(SolverError::PreallocatedStorageRequiresStreetRecall);
        }
        let pool = initialization_pool(threads)?;
        pool.install(|| {
            Self::from_state_with_config_internal(
                game,
                sampler,
                state,
                current_config,
                true,
                tree::enumerate_tree_with_limits_parallel,
            )
        })
    }

    fn from_state_with_config_internal(
        game: G,
        sampler: DealSampler,
        mut state: SolverState,
        current_config: SolverConfig,
        commit_pages: bool,
        materialize: TreeMaterializer<G>,
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
        let num_players = game.num_players() as u64;
        if !state.traversals.is_multiple_of(num_players) {
            return Err(SolverError::IncompleteSweepState);
        }
        let expected_sweeps = state.traversals / num_players;
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

        if game.recall_mode() != RecallMode::Street {
            return Err(SolverError::PreallocatedStorageRequiresStreetRecall);
        }
        Self::from_state_dense(game, sampler, state, commit_pages, materialize)
    }

    /// [`Self::from_state_with_config`]'s `RecallMode::Street` path: rebuilds
    /// the dense arena from the game's enumerated tree, then replays the
    /// checkpoint's (already ancestors-of-touched-pruned) histories/policies
    /// into it.
    fn from_state_dense(
        game: G,
        sampler: DealSampler,
        mut state: SolverState,
        commit_pages: bool,
        materialize: TreeMaterializer<G>,
    ) -> Result<Self, SolverError> {
        let mut dense_storage = DenseStorage::build(
            &game,
            state.config.max_memory_bytes,
            state.config.max_traversal_depth,
            false,
            materialize,
        )?;
        state.histories.sort_unstable_by_key(|entry| entry.key);
        state.policies.sort_unstable_by_key(|entry| entry.key);
        let mut previous_history = None;
        for entry in &state.histories {
            validate_history_entry(entry, game.num_players())?;
            if previous_history == Some(entry.key) {
                return Err(SolverError::DuplicateHistory(entry.key));
            }
            previous_history = Some(entry.key);
            if !dense_storage.tree.by_history.contains_key(&entry.key) {
                return Err(SolverError::UnmappedDenseHistory(entry.key));
            }
        }
        let mut previous_policy = None;
        for entry in &state.policies {
            validate_column(
                entry.key,
                &entry.column,
                game.num_players(),
                RecallMode::Street,
            )?;
            if previous_policy == Some(entry.key) {
                return Err(SolverError::DuplicatePolicy(entry.key));
            }
            previous_policy = Some(entry.key);
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
        }
        if commit_pages {
            dense_storage.arena.commit_pages();
        }
        for entry in state.policies {
            let (node_id, bucket) = dense_storage
                .target(entry.key)
                .expect("checkpoint entry was mapped in the validation pass");
            let range = dense_storage
                .arena
                .slot_range(node_id, bucket)
                .expect("checkpoint bucket was validated in the validation pass");
            dense_storage.arena.regrets[range.clone()].copy_from_slice(&entry.column.regrets);
            dense_storage.arena.strategy_sum[range].copy_from_slice(&entry.column.strategy_sum);
            let column = dense_storage.arena.column_id(node_id, bucket)?;
            dense_storage.arena.touched_set(column);
        }

        Ok(Self {
            game,
            sampler,
            config: state.config,
            traversals: state.traversals,
            completed_sweeps: state.completed_sweeps,
            next_sample_id: state.next_sample_id,
            total_deal_attempts: state.total_deal_attempts,
            terminal_evaluations: state.terminal_evaluations,
            hand_updates: state.hand_updates,
            dense: dense_storage,
        })
    }

    pub fn game(&self) -> &G {
        &self.game
    }

    pub fn into_components(self) -> (G, DealSampler, SolverConfig) {
        (self.game, self.sampler, self.config)
    }

    pub fn sampler(&self) -> &DealSampler {
        &self.sampler
    }

    pub fn config(&self) -> SolverConfig {
        self.config
    }

    /// Returns the fixed arena report for current-street storage. Production
    /// callers require `Some` with `pages_committed = true` before sweep 0.
    pub fn policy_arena_allocation(&self) -> Option<PolicyArenaAllocation> {
        Some(PolicyArenaAllocation {
            nodes: self.dense.arena.node_count() as u64,
            columns: self.dense.arena.total_columns(),
            slots: self.dense.arena.total_slots(),
            bytes: self.dense.arena.estimated_bytes(),
            pages_committed: self.dense.arena.pages_committed(),
        })
    }

    /// Fingerprint covering solver knobs, ranges, and public game rules.
    pub fn configuration_fingerprint(&self) -> [u8; 32] {
        configuration_fingerprint_for_setup(&self.game, &self.sampler, self.config)
    }

    /// Identity of the card-abstraction backend and private-recall semantics.
    ///
    /// Full recall deliberately retains the historical backend fingerprint
    /// byte-for-byte. Street recall is domain-separated so it cannot alias a
    /// full-recall checkpoint or solution built from the same buckets.
    pub fn abstraction_fingerprint(&self) -> [u8; 32] {
        abstraction_fingerprint_with_recall(
            self.game.abstraction_fingerprint(),
            self.game.recall_mode(),
        )
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
    /// to 7 extra sweeps versus `sweep_batch = 1`. Cooperative cancellation
    /// never interrupts a batch. A merge error instead rolls back only the
    /// failing sweep; earlier successful sweeps in that batch stay committed.
    pub fn run_sweeps_with_threads_until<F>(
        &mut self,
        sweeps: u64,
        threads: usize,
        should_continue: F,
    ) -> Result<u64, SolverError>
    where
        F: FnMut() -> bool,
    {
        self.run_sweeps_with_threads_until_observed(sweeps, threads, should_continue, |_| {})
    }

    /// [`Self::run_sweeps_with_threads_until`] with a cheap read-only hook
    /// after every committed sweep batch. The hook runs on the drive thread
    /// while the worker pool stays alive, allowing a GUI to publish counters
    /// and one requested preflop-node strategy without forcing a held-out
    /// evaluation or rebuilding the pool.
    pub fn run_sweeps_with_threads_until_observed<F, O>(
        &mut self,
        sweeps: u64,
        threads: usize,
        mut should_continue: F,
        mut after_batch: O,
    ) -> Result<u64, SolverError>
    where
        F: FnMut() -> bool,
        O: FnMut(&Self),
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
                self.merge_sweep_dense(sweep_deltas)?;
                completed = completed
                    .checked_add(1)
                    .ok_or(SolverError::CounterOverflow)?;
            }
            after_batch(self);
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
    ) -> Result<DenseTraversalDelta, SolverError> {
        let mut deal_rng = traversal_deal_rng(self.config.seed, sample_id, traverser);
        let sample = self.sampler.sample_counted(&mut deal_rng)?;
        let mut action_rng = traversal_action_rng(self.config.seed, sample_id, traverser);
        let mut average_rng = average_strategy_action_rng(self.config.seed, sample_id, traverser);
        let mut reach = vec![1.0; self.game.num_players()];
        let dense = &self.dense;
        if self.config.traverser_vector {
            let feasible = self.sampler.feasible_combos(traverser, &sample.world);
            let (combos, weights): (Vec<usize>, Vec<f64>) = feasible.into_iter().unzip();
            let mut normalized_weights = weights.clone();
            normalize_feasible_weights(&mut normalized_weights)?;
            // The averaging traversal tracks one own-reach value per
            // feasible combo.
            let own_reach = vec![1.0; combos.len()];
            let regret_combos = combos.clone();
            let (regret_result, average_result) = rayon::join(
                || -> Result<DenseTraversalDelta, SolverError> {
                    // The regret traversal starts with every feasible combo
                    // active. Pruning is the only operation that shrinks it.
                    let active: Vec<usize> = (0..regret_combos.len()).collect();
                    let mut worker = VectorTraversalWorker::new(
                        &self.game,
                        dense,
                        self.config,
                        regret_combos,
                        weights,
                    )?;
                    worker.traverse(
                        self.game.root_state(),
                        0,
                        &sample.world,
                        traverser,
                        &active,
                        1.0,
                        &mut action_rng,
                        0,
                    )?;
                    Ok(worker.finish(sample_id, traverser, u64::from(sample.attempts)))
                },
                || -> Result<Vec<DenseEvent>, SolverError> {
                    let mut average = DenseAverageStrategyWorker::new(
                        &self.game,
                        dense,
                        self.config,
                        linear_weight,
                    );
                    average.traverse_vector(
                        self.game.root_state(),
                        0,
                        &sample.world,
                        traverser,
                        &combos,
                        &normalized_weights,
                        &own_reach,
                        &mut average_rng,
                        0,
                    )?;
                    Ok(average.finish())
                },
            );
            // Preserve the serial contract: regret errors win when both
            // branches fail, and regret events precede average events.
            let mut delta = regret_result?;
            delta.events.extend(average_result?);
            Ok(delta)
        } else {
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
            let mut delta = worker.finish(sample_id, traverser, u64::from(sample.attempts));
            let mut average =
                DenseAverageStrategyWorker::new(&self.game, dense, self.config, linear_weight);
            average.traverse_scalar(
                self.game.root_state(),
                0,
                &sample.world,
                traverser,
                1.0,
                &mut average_rng,
                0,
            )?;
            delta.events.extend(average.finish());
            Ok(delta)
        }
    }

    /// Replays ordered deltas transactionally into the fixed arena.
    fn merge_sweep_dense(
        &mut self,
        mut deltas: Vec<DenseTraversalDelta>,
    ) -> Result<(), SolverError> {
        let num_players = self.game.num_players();
        if deltas.len() != num_players {
            return Err(SolverError::InvalidState(
                "parallel sweep did not produce one delta per seat",
            ));
        }
        let mut total_deal_attempts = self.total_deal_attempts;
        let mut terminal_evaluations = self.terminal_evaluations;
        let mut hand_updates = self.hand_updates;

        // Validate all identities and counters before changing any arena slot.
        for (seat, delta) in deltas.iter().enumerate() {
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

        let dense = &mut self.dense;
        // Keep the existing per-addition regret floor and rounding order. The
        // consumed f64 delta slots become an exact journal of old f32 values;
        // no arena copy or additional event-sized allocation is necessary.
        let floor = self
            .config
            .prune
            .then_some((1.05 * self.config.prune_threshold) as f32);
        for seat in 0..deltas.len() {
            for event in 0..deltas[seat].events.len() {
                if let Err((written, error)) = apply_dense_event_journaled(
                    &mut dense.arena,
                    &mut deltas[seat].events[event],
                    floor,
                ) {
                    rollback_dense_events(&mut dense.arena, &deltas, seat, event, written);
                    return Err(error);
                }
            }
        }
        // Every column was validated above. Defer these infallible writes so
        // rollback never needs to clear bits or restore touched_count.
        for delta in &deltas {
            for event in &delta.events {
                let column = match event {
                    DenseEvent::AddRegret { column, .. }
                    | DenseEvent::AddStrategy { column, .. } => *column,
                };
                dense.arena.touched_set(column);
            }
        }
        self.traversals = traversals;
        self.next_sample_id = next_sample_id;
        self.completed_sweeps = completed_sweeps;
        self.total_deal_attempts = total_deal_attempts;
        self.terminal_evaluations = terminal_evaluations;
        self.hand_updates = hand_updates;
        self.apply_early_discount();
        Ok(())
    }

    pub fn current_strategy(&self, key: InfoKey) -> Option<Vec<f32>> {
        let dense = &self.dense;
        dense
            .column_view(key)
            .map(|view| regret_matching_f32(view.regrets))
    }

    pub fn average_strategy(&self, key: InfoKey) -> Option<Vec<f32>> {
        let dense = &self.dense;
        dense.column_view(key).map(|view| {
            normalize_nonnegative_f32(view.strategy_sum)
                .unwrap_or_else(|| regret_matching_f32(view.regrets))
        })
    }

    pub fn current_action_probabilities(&self, key: InfoKey) -> Option<Vec<ActionProbability>> {
        let dense = &self.dense;
        dense
            .column_view(key)
            .map(|view| label_probabilities(view.action_labels, regret_matching_f32(view.regrets)))
    }

    pub fn average_action_probabilities(&self, key: InfoKey) -> Option<Vec<ActionProbability>> {
        let dense = &self.dense;
        dense.column_view(key).map(|view| {
            let probabilities = normalize_nonnegative_f32(view.strategy_sum)
                .unwrap_or_else(|| regret_matching_f32(view.regrets));
            label_probabilities(view.action_labels, probabilities)
        })
    }

    pub fn policy(&self, key: InfoKey) -> Option<PolicyColumn> {
        let dense = &self.dense;
        dense.column_view(key).map(|view| PolicyColumn {
            action_labels: view.action_labels.to_vec(),
            regrets: view.regrets.to_vec(),
            strategy_sum: view.strategy_sum.to_vec(),
        })
    }

    pub fn history_entry(&self, key: HistoryKey) -> Option<HistoryEntry> {
        let dense = &self.dense;
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

    /// All enumerated child edges whose parent is `parent`, sorted by
    /// `(actor, action_index)` for a stable UI order. `O(node's actions)`
    /// scan, including untouched children. Meant for a UI polling
    /// live progress, not the hot traversal loop.
    pub fn node_children(&self, parent: HistoryKey) -> Vec<HistoryEntry> {
        let dense = &self.dense;
        let Some(&node_id) = dense.tree.by_history.get(&parent) else {
            return Vec::new();
        };
        let node = &dense.tree.nodes[node_id as usize];
        let mut children: Vec<HistoryEntry> = (0..node.action_labels.len())
            .map(|action_index| HistoryEntry {
                key: parent.child(node.actor as usize, action_index),
                parent,
                actor: node.actor,
                action_index: action_index as u32,
                action_label: node.action_labels[action_index].clone(),
            })
            .collect();
        children.sort_unstable_by_key(|entry| (entry.actor, entry.action_index));
        children
    }

    /// Public metadata for one materialized decision node. Production
    /// current-street solvers use the dense tree, so this is an O(actions)
    /// read with no traversal or strategy scan.
    pub fn public_node_view(&self, history: HistoryKey) -> Option<PublicNodeView> {
        let dense = &self.dense;
        let &node_id = dense.tree.by_history.get(&history)?;
        let node = &dense.tree.nodes[node_id as usize];
        let actions = node
            .action_labels
            .iter()
            .cloned()
            .zip(&node.children)
            .map(|(label, child)| {
                let destination = match *child {
                    tree::Child::Terminal => PublicActionDestination::Terminal,
                    tree::Child::Decision(child_id) => {
                        let child = &dense.tree.nodes[child_id as usize];
                        if child.street == Street::Preflop {
                            PublicActionDestination::PreflopDecision(child.history)
                        } else {
                            PublicActionDestination::PostflopBoundary
                        }
                    }
                };
                PublicNodeAction { label, destination }
            })
            .collect();
        Some(PublicNodeView {
            history,
            actor: node.actor,
            street: node.street,
            active_opponents: node.active_opponents,
            actions,
        })
    }

    /// Current average strategy of every policy column at `history`, sorted
    /// by key. Only *touched* buckets are listed. `O(node's buckets)` scan;
    /// meant for a UI polling
    /// live progress, not the hot traversal loop.
    pub fn strategies_at(&self, history: HistoryKey) -> Vec<(InfoKey, Vec<String>, Vec<f32>)> {
        self.strategies_at_with_mass(history)
            .into_iter()
            .map(|(key, labels, probabilities, _mass)| (key, labels, probabilities))
            .collect()
    }

    /// Same rows as [`Self::strategies_at`], plus each column's raw strategy
    /// mass: `Σ_a strategy_sum[a]`, summed in `f64` to avoid precision loss
    /// over many `f32` accumulators. The mass includes the exact public
    /// history's fixed uniform-opponent proposal factor `Q(history)`; it is
    /// therefore a valid cheap weight for a live range-wide action-frequency
    /// aggregation across buckets at this one history, where `Q` is common,
    /// but is not a reach probability and must not be compared or aggregated
    /// across different histories. See [`PolicyColumn::strategy_sum`].
    pub fn strategies_at_with_mass(
        &self,
        history: HistoryKey,
    ) -> Vec<(InfoKey, Vec<String>, Vec<f32>, f64)> {
        let dense = &self.dense;
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
        let dense = &self.dense;
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
                    action_label: parent.action_labels[node.parent_action_index as usize].clone(),
                }
            })
            .collect();
        histories.sort_unstable_by_key(|entry| entry.key);
        self.finish_snapshot(histories, policies)
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
        let dense = &self.dense;
        for_each_touched_column(dense, |_column, key, node, range| {
            let current = normalize_nonnegative_f32(&dense.arena.strategy_sum[range.clone()])
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

    /// Memory-compact equivalent of [`Self::strategy_drift_refresh`].
    ///
    /// Dense current-street storage uses arena column ids instead of full
    /// [`InfoKey`] values and packs all previous normalized probabilities in
    /// one allocation. Columns and their f32 probabilities are still visited
    /// in the legacy method's node-major order, preserving its subtraction
    /// and f64 reduction order exactly. A tracker binds to its first solver's
    /// complete layout identity; callers must [`StrategyDriftTracker::reset`]
    /// before intentionally reusing it with an incompatible solver.
    pub fn strategy_drift_refresh_compact(
        &self,
        tracker: &mut StrategyDriftTracker,
    ) -> Result<Vec<f64>, StrategyDriftError> {
        let num_players = self.game.num_players();
        let dense = &self.dense;
        let total_columns = dense.arena.total_columns();
        let total_slots = dense.arena.total_slots();
        let identity = StrategyDriftIdentity {
            players: num_players as u8,
            total_columns,
            total_slots,
            configuration: self.configuration_fingerprint(),
            abstraction: self.abstraction_fingerprint(),
        };
        match tracker.identity {
            None => tracker.identity = Some(identity),
            Some(bound) if bound == identity => {}
            Some(_) => return Err(StrategyDriftError::IncompatibleLayout),
        }

        let stored_slots = tracker
            .dense_action_counts
            .iter()
            .try_fold(0usize, |sum, &count| sum.checked_add(count as usize));
        if tracker.dense_columns.len() != tracker.dense_action_counts.len()
            || stored_slots != Some(tracker.dense_probabilities.len())
        {
            return Err(StrategyDriftError::CorruptTracker);
        }

        let current_count = dense.arena.touched_count() as usize;
        if current_count < tracker.dense_columns.len() {
            return Err(StrategyDriftError::IncompatibleLayout);
        }
        if current_count == tracker.dense_columns.len() {
            return refresh_dense_drift_in_place(dense, tracker, num_players);
        }

        let old_columns = std::mem::take(&mut tracker.dense_columns);
        let old_action_counts = std::mem::take(&mut tracker.dense_action_counts);
        let old_probabilities = std::mem::take(&mut tracker.dense_probabilities);
        let mut new_columns = Vec::with_capacity(current_count);
        let mut new_action_counts = Vec::with_capacity(current_count);
        let mut new_probabilities = Vec::with_capacity(old_probabilities.len());
        let mut totals = vec![0.0; num_players];
        let mut counts = vec![0u64; num_players];
        let mut old_index = 0usize;
        let mut old_offset = 0usize;
        let mut failure = None;

        for_each_touched_column(dense, |column, _key, node, range| {
            if failure.is_some() {
                return;
            }
            let current = normalize_nonnegative_f32(&dense.arena.strategy_sum[range.clone()])
                .unwrap_or_else(|| regret_matching_f32(&dense.arena.regrets[range]));
            let value = if old_index < old_columns.len() {
                if column < old_columns[old_index] {
                    0.0
                } else if column == old_columns[old_index] {
                    let action_count = old_action_counts[old_index] as usize;
                    let Some(end) = old_offset.checked_add(action_count) else {
                        failure = Some(StrategyDriftError::CorruptTracker);
                        return;
                    };
                    let Some(previous) = old_probabilities.get(old_offset..end) else {
                        failure = Some(StrategyDriftError::CorruptTracker);
                        return;
                    };
                    if previous.len() != current.len() {
                        failure = Some(StrategyDriftError::IncompatibleLayout);
                        return;
                    }
                    old_index += 1;
                    old_offset = end;
                    current
                        .iter()
                        .zip(previous)
                        .map(|(&left, &right)| f64::from((left - right).abs()))
                        .sum::<f64>()
                        * 0.5
                } else {
                    failure = Some(StrategyDriftError::IncompatibleLayout);
                    return;
                }
            } else {
                0.0
            };
            let Ok(action_count) = u32::try_from(current.len()) else {
                failure = Some(StrategyDriftError::IncompatibleLayout);
                return;
            };
            totals[node.actor as usize] += value;
            counts[node.actor as usize] += 1;
            new_columns.push(column);
            new_action_counts.push(action_count);
            new_probabilities.extend_from_slice(&current);
        });
        if failure.is_none()
            && (old_index != old_columns.len() || old_offset != old_probabilities.len())
        {
            failure = Some(StrategyDriftError::IncompatibleLayout);
        }
        if let Some(error) = failure {
            tracker.dense_columns = old_columns;
            tracker.dense_action_counts = old_action_counts;
            tracker.dense_probabilities = old_probabilities;
            return Err(error);
        }
        tracker.dense_columns = new_columns;
        tracker.dense_action_counts = new_action_counts;
        tracker.dense_probabilities = new_probabilities;

        Ok(finalize_drift(totals, counts))
    }

    pub fn metrics(&self) -> SolverMetrics {
        let num_players = self.game.num_players();
        let mut positive_regret = vec![0.0; num_players];
        let dense = &self.dense;
        for_each_touched_column(dense, |_column, _, node, range| {
            positive_regret[node.actor as usize] += dense.arena.regrets[range]
                .iter()
                .map(|&regret| f64::from(regret.max(0.0)))
                .sum::<f64>();
        });
        let (infosets, memory_bytes) = (dense.arena.touched_count(), dense.arena.estimated_bytes());
        for (player, total) in positive_regret.iter_mut().enumerate() {
            let updates = traversals_for_player(self.traversals, num_players, player);
            if updates > 0 {
                *total /= updates as f64;
            }
        }
        SolverMetrics {
            sweeps: self.completed_sweeps,
            traversals: self.traversals,
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
        let dense = &mut self.dense;
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
