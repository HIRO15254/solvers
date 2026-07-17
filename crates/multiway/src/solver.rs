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

pub const SOLVER_STATE_VERSION: u16 = 2;
pub const DEFAULT_EXPLORATION_EPSILON: f64 = 0.06;
pub const UNREACHED_BUCKET: BucketId = u32::MAX;
pub const DEFAULT_DISCOUNT_EVERY: u64 = 100_000;
pub const DEFAULT_DISCOUNT_UNTIL: u64 = 10_000_000;
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

#[derive(Clone, Debug)]
enum TraversalEvent {
    EnsurePolicy {
        key: InfoKey,
        action_labels: Vec<String>,
    },
    EnsureHistory(HistoryEntry),
    AddRegret {
        key: InfoKey,
        values: Vec<f64>,
    },
    AddStrategy {
        key: InfoKey,
        values: Vec<f64>,
    },
}

#[derive(Clone, Debug)]
struct TraversalDelta {
    sample_id: u64,
    traverser: usize,
    deal_attempts: u64,
    terminal_evaluations: u64,
    hand_updates: u64,
    events: Vec<TraversalEvent>,
}

/// Dense-mode traversal event: an add into one arena column's regret or
/// strategy-sum slots, addressed by the wire-format `column_id` from
/// [`DenseArena::column_id`].
#[derive(Clone, Debug)]
enum DenseEvent {
    AddRegret { column: u32, values: Vec<f64> },
    AddStrategy { column: u32, values: Vec<f64> },
}

#[derive(Clone, Debug)]
struct DenseTraversalDelta {
    sample_id: u64,
    traverser: usize,
    deal_attempts: u64,
    terminal_evaluations: u64,
    hand_updates: u64,
    events: Vec<DenseEvent>,
}

/// One traversal's delta, produced by whichever worker
/// [`MultiwaySolver::generate_traversal_delta`] dispatched to. Every delta in
/// a solver's lifetime carries the same variant (decided once at
/// construction by [`ExternalSamplingGame::recall_mode`]); the mismatched
/// case in [`MultiwaySolver::merge_sweep`] is defensive only.
enum AnyTraversalDelta {
    Sparse(TraversalDelta),
    Dense(DenseTraversalDelta),
}

struct TraversalWorker<'a, G: ExternalSamplingGame> {
    game: &'a G,
    policies: &'a FxHashMap<InfoKey, PolicyColumn>,
    histories: &'a FxHashMap<HistoryKey, HistoryEntry>,
    config: SolverConfig,
    linear_weight: f64,
    events: Vec<TraversalEvent>,
    local_policies: FxHashMap<InfoKey, Vec<String>>,
    local_histories: FxHashMap<HistoryKey, HistoryEntry>,
    terminal_evaluations: u64,
}

/// Node-major dense-arena storage backing [`RecallMode::Street`]. Built once
/// at solver construction/resume time from the game's fully enumerated
/// public tree; never grows afterward.
struct DenseStorage {
    tree: PublicTree,
    arena: DenseArena,
}

/// Borrowed view of one dense-arena column, mirroring [`PolicyColumn`]'s
/// fields without cloning until a caller actually needs an owned copy.
struct DenseColumnView<'a> {
    action_labels: &'a [String],
    regrets: &'a [f32],
    strategy_sum: &'a [f32],
}

impl DenseStorage {
    fn build<G: ExternalSamplingGame>(
        game: &G,
        max_memory_bytes: u64,
    ) -> Result<Self, SolverError> {
        let tree = tree::enumerate_tree(game)?;
        let arena = tree::build_arena(game, &tree, max_memory_bytes)?;
        Ok(Self { tree, arena })
    }

    /// Resolves `key` to its `(node, bucket)` dense-arena slot, independent
    /// of whether that column has been touched yet. `None` iff `key` cannot
    /// possibly correspond to any node in the enumerated tree.
    fn target(&self, key: InfoKey) -> Option<(NodeId, BucketId)> {
        let &node_id = self.tree.by_history.get(&key.history)?;
        let node = self.tree.nodes.get(node_id as usize)?;
        if node.actor != key.player
            || node.street.index() as u8 != key.street
            || node.active_opponents != key.active_opponents
        {
            return None;
        }
        let street = key.street as usize;
        if street >= key.bucket_path.len() {
            return None;
        }
        Some((node_id, key.bucket_path[street]))
    }

    /// Like [`Self::target`], but additionally requires the column to have
    /// been touched by at least one sampled update -- the dense-mode
    /// equivalent of sparse's "this `InfoKey` was never visited".
    fn column_view(&self, key: InfoKey) -> Option<DenseColumnView<'_>> {
        let (node_id, bucket) = self.target(key)?;
        let column = self.arena.column_id(node_id, bucket).ok()?;
        if !self.arena.is_touched(column) {
            return None;
        }
        let range = self.arena.slot_range(node_id, bucket).ok()?;
        let node = &self.tree.nodes[node_id as usize];
        Some(DenseColumnView {
            action_labels: &node.action_labels,
            regrets: &self.arena.regrets[range.clone()],
            strategy_sum: &self.arena.strategy_sum[range],
        })
    }

    fn info_key_for(&self, node_id: NodeId, bucket: BucketId) -> InfoKey {
        let node = &self.tree.nodes[node_id as usize];
        let mut bucket_path = [UNREACHED_BUCKET; 4];
        bucket_path[node.street.index()] = bucket;
        InfoKey {
            history: node.history,
            player: node.actor,
            street: node.street.index() as u8,
            active_opponents: node.active_opponents,
            bucket_path,
        }
    }
}

/// Visits every touched column of `dense`'s arena in node-id order (the
/// arena's own node-major layout), calling `visit(key, node, slot_range)`
/// once per touched `(node, bucket)`. Shared by [`MultiwaySolver::metrics`]
/// and [`MultiwaySolver::strategy_drift_refresh`], the two per-seat
/// dense-mode aggregations that must scan the whole arena deterministically.
fn for_each_touched_column<'a>(
    dense: &'a DenseStorage,
    mut visit: impl FnMut(InfoKey, &'a tree::TreeNode, std::ops::Range<usize>),
) {
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
            visit(dense.info_key_for(node_id, bucket), node, range);
        }
    }
}

fn label_probabilities(labels: &[String], probabilities: Vec<f32>) -> Vec<ActionProbability> {
    labels
        .iter()
        .cloned()
        .zip(probabilities)
        .map(|(action, probability)| ActionProbability {
            action,
            probability,
        })
        .collect()
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
        hasher.update(b"solvers.multiway.solver-config.v3");
        hasher.update(&self.config.seed.to_le_bytes());
        hasher.update(&self.config.max_traversal_depth.to_le_bytes());
        hasher.update(&self.config.exploration_epsilon.to_bits().to_le_bytes());
        hasher.update(&self.config.discount_every.to_le_bytes());
        hasher.update(&self.config.discount_until.to_le_bytes());
        hasher.update(&self.config.sweep_batch.to_le_bytes());
        hasher.update(&[u8::from(self.config.traverser_vector)]);
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
                        for (target, value) in dense.arena.regrets[range].iter_mut().zip(values) {
                            checked_add_f32(target, value)?;
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
        let mut rows: Vec<(InfoKey, Vec<String>, Vec<f32>)> = match &self.dense {
            None => self
                .policies
                .iter()
                .filter(|(key, _)| key.history == history)
                .map(|(&key, column)| {
                    (key, column.action_labels.clone(), column.average_strategy())
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
                    let probabilities =
                        normalize_nonnegative_f32(&dense.arena.strategy_sum[range.clone()])
                            .unwrap_or_else(|| regret_matching_f32(&dense.arena.regrets[range]));
                    rows.push((
                        dense.info_key_for(node_id, bucket),
                        node.action_labels.clone(),
                        probabilities,
                    ));
                }
                rows
            }
        };
        rows.sort_unstable_by_key(|(key, _, _)| *key);
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

    /// Held-out Monte Carlo evaluation of the stored average profile.
    ///
    /// Every sample has its own `(seed, sample_id)` substream. The method is
    /// read-only: it does not advance training counters, change policy sums,
    /// or share the training traversal's random stream. It also evaluates a
    /// fixed candidate deviation for every seat on the same held-out physical
    /// worlds. The candidate chooses the largest stored cumulative regret at
    /// each visited information set and otherwise retains average-profile
    /// play. The reported gain is paired against the baseline profile, then
    /// transformed with the always-available no-deviation option: mean and
    /// confidence endpoints are clamped at zero. This is only a lower bound
    /// for that candidate set, never a full best-response calculation.
    pub fn evaluate_average_profile(
        &self,
        samples: u64,
        seed: u64,
    ) -> Result<ProfileEvaluation, SolverError> {
        if samples == 0 {
            return Err(SolverError::ZeroEvaluationSamples);
        }
        let num_players = self.game.num_players();
        let mut means = vec![0.0; num_players];
        let mut m2 = vec![0.0; num_players];
        let mut gain_means = vec![0.0; num_players];
        let mut gain_m2 = vec![0.0; num_players];
        let mut total_deal_attempts = 0u64;

        for sample_id in 0..samples {
            let mut deal_rng = evaluation_deal_rng(seed, sample_id);
            let sample = self.sampler.sample_counted(&mut deal_rng)?;
            total_deal_attempts = total_deal_attempts
                .checked_add(u64::from(sample.attempts))
                .ok_or(SolverError::CounterOverflow)?;
            let mut profile_rng = evaluation_action_rng(seed, sample_id, None);
            let utilities = self.evaluate_world(&sample.world, &mut profile_rng, None)?;
            let count = (sample_id + 1) as f64;
            for seat in 0..num_players {
                let delta = utilities[seat] - means[seat];
                means[seat] += delta / count;
                m2[seat] += delta * (utilities[seat] - means[seat]);

                let mut deviation_rng = evaluation_action_rng(seed, sample_id, Some(seat));
                let deviation =
                    self.evaluate_world(&sample.world, &mut deviation_rng, Some(seat))?;
                let gain = deviation[seat] - utilities[seat];
                let gain_delta = gain - gain_means[seat];
                gain_means[seat] += gain_delta / count;
                gain_m2[seat] += gain_delta * (gain - gain_means[seat]);
            }
        }

        let seats = means
            .into_iter()
            .zip(m2)
            .map(|(mean, sum_squared_error)| profile_estimate(mean, sum_squared_error, samples))
            .collect();
        let deviation_gain_lower_bound = gain_means
            .into_iter()
            .zip(gain_m2)
            .map(|(mean, sum_squared_error)| {
                nonnegative_gain_estimate(mean, sum_squared_error, samples)
            })
            .collect();
        Ok(ProfileEvaluation {
            samples,
            total_deal_attempts,
            seats,
            deviation_gain_lower_bound: Some(deviation_gain_lower_bound),
        })
    }

    fn evaluate_world(
        &self,
        world: &SampledWorld,
        rng: &mut ChaCha20Rng,
        deviator: Option<usize>,
    ) -> Result<Vec<f64>, SolverError> {
        let num_players = self.game.num_players();
        let mut state = self.game.root_state();
        let mut history = HistoryKey::ROOT;
        for depth in 0..=self.config.max_traversal_depth {
            let Some(actor) = self.game.actor(&state) else {
                let mut utilities = vec![0.0; num_players];
                self.game.terminal_utilities(&state, world, &mut utilities);
                if let Some((seat, &utility)) = utilities
                    .iter()
                    .enumerate()
                    .find(|(_, utility)| !utility.is_finite())
                {
                    return Err(SolverError::NonFiniteUtility { seat, utility });
                }
                return Ok(utilities);
            };
            if actor >= num_players {
                return Err(SolverError::InvalidActor { actor, num_players });
            }
            let actions = self.game.node_actions(&state);
            let num_actions = self.game.num_actions_of(&actions);
            if num_actions == 0 {
                return Err(SolverError::NoActions { actor });
            }
            let private = self.game.bucket(&state, world, actor);
            validate_private_info(private, num_players, self.game.recall_mode())?;
            let key = InfoKey {
                history,
                player: actor as u8,
                street: private.street,
                active_opponents: private.active_opponents,
                bucket_path: private.bucket_path,
            };
            let labels = (0..num_actions)
                .map(|index| self.game.action_label_of(&actions, index))
                .collect::<Vec<_>>();
            validate_action_labels(&labels)?;
            // `policy` already dispatches on storage mode, so this evaluation
            // loop needs no dense/sparse branch of its own.
            let stored = self.policy(key);
            let strategy = if let Some(column) = &stored {
                if column.action_labels != labels {
                    return Err(SolverError::ActionLabelsChanged { key });
                }
                column.average_strategy()
            } else {
                vec![1.0 / num_actions as f32; num_actions]
            };
            let action = if deviator == Some(actor) {
                match &stored {
                    Some(column) => regret_greedy_action(&column.regrets),
                    None => sample_profile_action(&strategy, rng),
                }
            } else {
                sample_profile_action(&strategy, rng)
            };
            state = self.game.next_state_with(&state, &actions, action);
            history = history.child(actor, action);
            if depth == self.config.max_traversal_depth {
                return Err(SolverError::DepthLimit {
                    limit: self.config.max_traversal_depth,
                });
            }
        }
        unreachable!("depth loop returns at its upper bound")
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn traverse(
        &mut self,
        state: G::State,
        world: &SampledWorld,
        traverser: usize,
        history: HistoryKey,
        reach: &mut [f64],
        sample_importance: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
    ) -> Result<f64, SolverError> {
        if depth > self.config.max_traversal_depth {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }

        let Some(actor) = self.game.actor(&state) else {
            let mut utilities = vec![0.0; self.game.num_players()];
            self.game.terminal_utilities(&state, world, &mut utilities);
            if let Some((seat, &utility)) = utilities
                .iter()
                .enumerate()
                .find(|(_, utility)| !utility.is_finite())
            {
                return Err(SolverError::NonFiniteUtility { seat, utility });
            }
            self.terminal_evaluations = self
                .terminal_evaluations
                .checked_add(1)
                .ok_or(SolverError::CounterOverflow)?;
            return Ok(utilities[traverser]);
        };

        let num_players = self.game.num_players();
        if actor >= num_players {
            return Err(SolverError::InvalidActor { actor, num_players });
        }
        let actions = self.game.node_actions(&state);
        let num_actions = self.game.num_actions_of(&actions);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        let private = self.game.bucket(&state, world, actor);
        validate_private_info(private, num_players, RecallMode::Full)?;
        let key = InfoKey {
            history,
            player: actor as u8,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        };
        let strategy = self.strategy_for(key, &actions)?;
        let mut label_buf = String::new();

        if actor == traverser {
            let mut action_values = vec![0.0; num_actions];
            for (action, value) in action_values.iter_mut().enumerate() {
                let old_reach = reach[actor];
                reach[actor] *= strategy[action];
                label_buf.clear();
                self.game
                    .write_action_label(&actions, action, &mut label_buf);
                let child_history = self.record_history(history, actor, action, &label_buf)?;
                let next = self.game.next_state_with(&state, &actions, action);
                *value = self.traverse(
                    next,
                    world,
                    traverser,
                    child_history,
                    reach,
                    sample_importance,
                    rng,
                    depth + 1,
                )?;
                reach[actor] = old_reach;
            }
            let node_value = strategy
                .iter()
                .zip(&action_values)
                .map(|(&probability, &value)| probability * value)
                .sum::<f64>();
            let column = self
                .policies
                .get_mut(&key)
                .expect("policy inserted before traversal");
            for (regret, &value) in column.regrets.iter_mut().zip(&action_values) {
                checked_add_f32(regret, sample_importance * (value - node_value))?;
            }
            Ok(node_value)
        } else {
            {
                let column = self
                    .policies
                    .get_mut(&key)
                    .expect("policy inserted before traversal");
                let linear_weight = (self.completed_sweeps + 1) as f64;
                for (sum, &probability) in column.strategy_sum.iter_mut().zip(&strategy) {
                    checked_add_f32(sum, linear_weight * reach[actor] * probability)?;
                }
            }
            let (action, sampling_probability) =
                sample_exploratory_action(&strategy, self.config.exploration_epsilon, rng);
            let importance = strategy[action] / sampling_probability;
            let child_importance = sample_importance * importance;
            if !child_importance.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            let old_reach = reach[actor];
            reach[actor] *= strategy[action];
            label_buf.clear();
            self.game
                .write_action_label(&actions, action, &mut label_buf);
            let child_history = self.record_history(history, actor, action, &label_buf)?;
            let next = self.game.next_state_with(&state, &actions, action);
            let result = self.traverse(
                next,
                world,
                traverser,
                child_history,
                reach,
                child_importance,
                rng,
                depth + 1,
            );
            reach[actor] = old_reach;
            // Exploration samples q rather than sigma. This importance
            // ratio keeps the recursive target-policy value unbiased.
            let weighted_value = result? * importance;
            if !weighted_value.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            Ok(weighted_value)
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

impl<'a, G: ExternalSamplingGame> TraversalWorker<'a, G> {
    fn new(solver: &'a MultiwaySolver<G>, linear_weight: f64) -> Self {
        Self {
            game: &solver.game,
            policies: &solver.policies,
            histories: &solver.histories,
            config: solver.config,
            linear_weight,
            events: Vec::new(),
            local_policies: FxHashMap::default(),
            local_histories: FxHashMap::default(),
            terminal_evaluations: 0,
        }
    }

    fn finish(self, sample_id: u64, traverser: usize, deal_attempts: u64) -> TraversalDelta {
        TraversalDelta {
            sample_id,
            traverser,
            deal_attempts,
            terminal_evaluations: self.terminal_evaluations,
            // The scalar algorithm updates exactly one sampled hand per
            // traversal; see `SolverState::hand_updates`.
            hand_updates: 1,
            events: self.events,
        }
    }

    /// Resolves the acting strategy at `key`. When a policy column already
    /// exists (the common case after the first sweep), this validates the
    /// current node's labels against the stored ones one at a time through a
    /// reused scratch buffer instead of collecting a fresh `Vec<String>`. A
    /// full owned label vector is only materialized when a policy is seen
    /// for the very first time in this traversal (the `EnsurePolicy` event
    /// payload and the `local_policies` cache both need to own it for later
    /// comparisons within the same worker).
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
        if let Some(labels) = self.local_policies.get(&key) {
            if labels.len() != num_actions {
                return Err(SolverError::ActionLabelsChanged { key });
            }
            let mut scratch = String::new();
            for (index, label) in labels.iter().enumerate() {
                scratch.clear();
                self.game.write_action_label(actions, index, &mut scratch);
                if *label != scratch {
                    return Err(SolverError::ActionLabelsChanged { key });
                }
            }
        } else {
            let action_labels: Vec<String> = (0..num_actions)
                .map(|index| self.game.action_label_of(actions, index))
                .collect();
            validate_action_labels(&action_labels)?;
            self.local_policies.insert(key, action_labels.clone());
            self.events
                .push(TraversalEvent::EnsurePolicy { key, action_labels });
        }
        // A policy first encountered during this sweep is intentionally
        // uniform for every worker. Deltas from another seat are not visible
        // until the ordered merge after all traversals finish.
        Ok(vec![1.0 / num_actions as f64; num_actions])
    }

    /// Records (or validates a repeat visit to) one edge of the public
    /// history trie. The common case — this exact `(parent, actor,
    /// action_index)` was already recorded, by this worker or an earlier
    /// sweep — is resolved by comparing fields against the stored entry
    /// without allocating; a fresh owned label is only built on the first
    /// visit, when there is something new to insert.
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
        if let Some(existing) = self.local_histories.get(&key) {
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
        let entry = HistoryEntry {
            key,
            parent,
            actor,
            action_index,
            action_label: action_label.to_string(),
        };
        self.local_histories.insert(key, entry.clone());
        self.events.push(TraversalEvent::EnsureHistory(entry));
        Ok(key)
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse(
        &mut self,
        state: G::State,
        world: &SampledWorld,
        traverser: usize,
        history: HistoryKey,
        reach: &mut [f64],
        sample_importance: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
    ) -> Result<f64, SolverError> {
        if depth > self.config.max_traversal_depth {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }

        let Some(actor) = self.game.actor(&state) else {
            let mut utilities = vec![0.0; self.game.num_players()];
            self.game.terminal_utilities(&state, world, &mut utilities);
            if let Some((seat, &utility)) = utilities
                .iter()
                .enumerate()
                .find(|(_, utility)| !utility.is_finite())
            {
                return Err(SolverError::NonFiniteUtility { seat, utility });
            }
            self.terminal_evaluations = self
                .terminal_evaluations
                .checked_add(1)
                .ok_or(SolverError::CounterOverflow)?;
            return Ok(utilities[traverser]);
        };

        let num_players = self.game.num_players();
        if actor >= num_players {
            return Err(SolverError::InvalidActor { actor, num_players });
        }
        let actions = self.game.node_actions(&state);
        let num_actions = self.game.num_actions_of(&actions);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        let private = self.game.bucket(&state, world, actor);
        validate_private_info(private, num_players, RecallMode::Full)?;
        let key = InfoKey {
            history,
            player: actor as u8,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        };
        let strategy = self.strategy_for(key, &actions)?;
        let mut label_buf = String::new();

        if actor == traverser {
            let mut action_values = vec![0.0; num_actions];
            for (action, value) in action_values.iter_mut().enumerate() {
                let old_reach = reach[actor];
                reach[actor] *= strategy[action];
                label_buf.clear();
                self.game
                    .write_action_label(&actions, action, &mut label_buf);
                let child_history = self.record_history(history, actor, action, &label_buf)?;
                let next = self.game.next_state_with(&state, &actions, action);
                *value = self.traverse(
                    next,
                    world,
                    traverser,
                    child_history,
                    reach,
                    sample_importance,
                    rng,
                    depth + 1,
                )?;
                reach[actor] = old_reach;
            }
            let node_value = strategy
                .iter()
                .zip(&action_values)
                .map(|(&probability, &value)| probability * value)
                .sum::<f64>();
            if !node_value.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            // Reuse `action_values` in place as the AddRegret payload
            // instead of collecting a second Vec.
            for value in action_values.iter_mut() {
                *value = sample_importance * (*value - node_value);
            }
            self.events.push(TraversalEvent::AddRegret {
                key,
                values: action_values,
            });
            Ok(node_value)
        } else {
            let (action, sampling_probability) =
                sample_exploratory_action(&strategy, self.config.exploration_epsilon, rng);
            // `strategy[action]` is needed below (for `reach` and the
            // importance ratio); capture it before `strategy` is reused in
            // place as the AddStrategy payload.
            let chosen_probability = strategy[action];
            let importance = chosen_probability / sampling_probability;
            let child_importance = sample_importance * importance;
            if !child_importance.is_finite() {
                return Err(SolverError::NumericOverflow);
            }

            let mut values = strategy;
            for probability in values.iter_mut() {
                *probability *= self.linear_weight * reach[actor];
            }
            self.events
                .push(TraversalEvent::AddStrategy { key, values });

            let old_reach = reach[actor];
            reach[actor] *= chosen_probability;
            label_buf.clear();
            self.game
                .write_action_label(&actions, action, &mut label_buf);
            let child_history = self.record_history(history, actor, action, &label_buf)?;
            let next = self.game.next_state_with(&state, &actions, action);
            let result = self.traverse(
                next,
                world,
                traverser,
                child_history,
                reach,
                child_importance,
                rng,
                depth + 1,
            );
            reach[actor] = old_reach;
            let weighted_value = result? * importance;
            if !weighted_value.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            Ok(weighted_value)
        }
    }
}

/// Dense-mode counterpart of [`TraversalWorker`]. Walks the state alongside
/// its precomputed [`NodeId`], so child lookups are a `Vec` index into
/// [`crate::tree::TreeNode::children`] instead of a blake3
/// [`HistoryKey::child`] hash, and there is no `EnsurePolicy`/`EnsureHistory`
/// bookkeeping at all: every column already exists in the arena.
struct DenseTraversalWorker<'a, G: ExternalSamplingGame> {
    game: &'a G,
    tree: &'a PublicTree,
    arena: &'a DenseArena,
    config: SolverConfig,
    linear_weight: f64,
    events: Vec<DenseEvent>,
    terminal_evaluations: u64,
}

impl<'a, G: ExternalSamplingGame> DenseTraversalWorker<'a, G> {
    fn new(game: &'a G, dense: &'a DenseStorage, config: SolverConfig, linear_weight: f64) -> Self {
        Self {
            game,
            tree: &dense.tree,
            arena: &dense.arena,
            config,
            linear_weight,
            events: Vec::new(),
            terminal_evaluations: 0,
        }
    }

    fn finish(self, sample_id: u64, traverser: usize, deal_attempts: u64) -> DenseTraversalDelta {
        DenseTraversalDelta {
            sample_id,
            traverser,
            deal_attempts,
            terminal_evaluations: self.terminal_evaluations,
            hand_updates: 1,
            events: self.events,
        }
    }

    fn terminal_value(
        &mut self,
        state: &G::State,
        world: &SampledWorld,
        traverser: usize,
    ) -> Result<f64, SolverError> {
        let mut utilities = vec![0.0; self.game.num_players()];
        self.game.terminal_utilities(state, world, &mut utilities);
        if let Some((seat, &utility)) = utilities
            .iter()
            .enumerate()
            .find(|(_, utility)| !utility.is_finite())
        {
            return Err(SolverError::NonFiniteUtility { seat, utility });
        }
        self.terminal_evaluations = self
            .terminal_evaluations
            .checked_add(1)
            .ok_or(SolverError::CounterOverflow)?;
        Ok(utilities[traverser])
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse(
        &mut self,
        state: G::State,
        node_id: NodeId,
        world: &SampledWorld,
        traverser: usize,
        reach: &mut [f64],
        sample_importance: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
    ) -> Result<f64, SolverError> {
        if depth > self.config.max_traversal_depth {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }

        let Some(actor) = self.game.actor(&state) else {
            return self.terminal_value(&state, world, traverser);
        };

        let num_players = self.game.num_players();
        if actor >= num_players {
            return Err(SolverError::InvalidActor { actor, num_players });
        }
        let actions = self.game.node_actions(&state);
        let num_actions = self.game.num_actions_of(&actions);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        let node = &self.tree.nodes[node_id as usize];
        if node.action_labels.len() != num_actions {
            return Err(SolverError::TreeNodeMismatch {
                node: node_id,
                expected: node.action_labels.len(),
                found: num_actions,
            });
        }
        let private = self.game.bucket(&state, world, actor);
        validate_private_info(private, num_players, RecallMode::Street)?;
        let bucket = private.current_bucket();
        let column = self.arena.column_id(node_id, bucket)?;
        let range = self.arena.slot_range(node_id, bucket)?;
        let strategy = regret_matching(&self.arena.regrets[range]);

        if actor == traverser {
            let mut action_values = vec![0.0; num_actions];
            for (action, value) in action_values.iter_mut().enumerate() {
                let old_reach = reach[actor];
                reach[actor] *= strategy[action];
                let next_state = self.game.next_state_with(&state, &actions, action);
                *value = match node.children[action] {
                    Child::Terminal => self.terminal_value(&next_state, world, traverser)?,
                    Child::Decision(child_id) => self.traverse(
                        next_state,
                        child_id,
                        world,
                        traverser,
                        reach,
                        sample_importance,
                        rng,
                        depth + 1,
                    )?,
                };
                reach[actor] = old_reach;
            }
            let node_value = strategy
                .iter()
                .zip(&action_values)
                .map(|(&probability, &value)| probability * value)
                .sum::<f64>();
            if !node_value.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            for value in action_values.iter_mut() {
                *value = sample_importance * (*value - node_value);
            }
            self.events.push(DenseEvent::AddRegret {
                column,
                values: action_values,
            });
            Ok(node_value)
        } else {
            let (action, sampling_probability) =
                sample_exploratory_action(&strategy, self.config.exploration_epsilon, rng);
            let chosen_probability = strategy[action];
            let importance = chosen_probability / sampling_probability;
            let child_importance = sample_importance * importance;
            if !child_importance.is_finite() {
                return Err(SolverError::NumericOverflow);
            }

            let mut values = strategy;
            for probability in values.iter_mut() {
                *probability *= self.linear_weight * reach[actor];
            }
            self.events.push(DenseEvent::AddStrategy { column, values });

            let old_reach = reach[actor];
            reach[actor] *= chosen_probability;
            let next_state = self.game.next_state_with(&state, &actions, action);
            let result = match node.children[action] {
                Child::Terminal => self.terminal_value(&next_state, world, traverser),
                Child::Decision(child_id) => self.traverse(
                    next_state,
                    child_id,
                    world,
                    traverser,
                    reach,
                    child_importance,
                    rng,
                    depth + 1,
                ),
            };
            reach[actor] = old_reach;
            let weighted_value = result? * importance;
            if !weighted_value.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            Ok(weighted_value)
        }
    }
}

/// "Vector-traverser" counterpart of [`DenseTraversalWorker`]: one traversal
/// updates every feasible hole combo of the sampled `traverser` seat at
/// once. See the module-level algorithm write-up in `docs/` (task report)
/// for the derivation; in short:
///
/// * The tree is walked exactly once per traversal, same as
///   [`DenseTraversalWorker`], except every value flowing back up the
///   recursion is a `Vec<f64>` (one entry per feasible combo, in
///   [`Self::combos`] order) instead of a scalar.
/// * At an opponent node, the acting seat's own single dealt card decides
///   its sampled action exactly as before (their line cannot depend on the
///   traverser's hypothetical hand, which is what makes one sampled line
///   valid for the whole vector); the returned vector is just the child
///   vector scaled by the same scalar importance ratio used today.
/// * At a traverser node, every action is explored (as today); each
///   feasible combo's own per-street bucket picks which arena column's
///   regret-matched strategy weighs its node value, and the regret add for
///   `(bucket, action)` is the *feasible-weighted mean* of that bucket's
///   member combos' `(value(action) - node_value)`, matching the expected
///   per-infoset scalar-ES update in aggregate. Buckets with no feasible
///   member are simply never visited, so they get no update.
/// * The average strategy (`strategy_sum`) accumulates densely at every
///   traverser node, over every feasible combo, instead of on opponents'
///   sampled lines: for bucket b it adds
///   `linear_weight * (sum_{h in F, B(h)=b} weight(h) * own_reach(h)) * sigma_b`,
///   where `own_reach(h)` is combo h's own-strategy reach product from the
///   traverser nodes visited earlier in this same traversal (opponent nodes
///   leave it unchanged, since another seat's sampled action doesn't bear on
///   the traverser's own strategy reach). The opponent branch pushes no
///   `AddStrategy` event at all. This exists because, at equal wall time,
///   vector mode runs an order of magnitude fewer sweeps than scalar mode
///   (each sweep is a full-width traversal rather than one sampled hand), so
///   accumulating the average only where an opponent's single-hand line
///   happens to sample it starves it relative to the (already dense)
///   regret updates; the fix is to make the average dense too.
struct VectorTraversalWorker<'a, G: ExternalSamplingGame> {
    game: &'a G,
    tree: &'a PublicTree,
    arena: &'a DenseArena,
    config: SolverConfig,
    linear_weight: f64,
    events: Vec<DenseEvent>,
    terminal_evaluations: u64,
    /// Feasible traverser combos for this traversal's sampled world (`F` in
    /// the design doc): positive-weight in the traverser's range and
    /// disjoint from every other seat's sampled hole cards and the sampled
    /// runout. Fixed for the whole traversal.
    combos: Vec<usize>,
    /// `weights[i]` is `combos[i]`'s range weight, aligned by index.
    weights: Vec<f64>,
    /// Per-street combo -> bucket table, built lazily the first time a
    /// traverser decision node on that street is visited and reused for
    /// every later traverser node on the same street within this traversal
    /// (board and bucket-active-opponents are fixed for a whole street; see
    /// `TreeNode::bucket_active_opponents`).
    bucket_cache: [Option<Vec<BucketId>>; 4],
}

impl<'a, G: ExternalSamplingGame> VectorTraversalWorker<'a, G> {
    fn new(
        game: &'a G,
        dense: &'a DenseStorage,
        config: SolverConfig,
        linear_weight: f64,
        combos: Vec<usize>,
        weights: Vec<f64>,
    ) -> Self {
        Self {
            game,
            tree: &dense.tree,
            arena: &dense.arena,
            config,
            linear_weight,
            events: Vec::new(),
            terminal_evaluations: 0,
            combos,
            weights,
            bucket_cache: [None, None, None, None],
        }
    }

    fn finish(self, sample_id: u64, traverser: usize, deal_attempts: u64) -> DenseTraversalDelta {
        DenseTraversalDelta {
            sample_id,
            traverser,
            deal_attempts,
            terminal_evaluations: self.terminal_evaluations,
            hand_updates: self.combos.len() as u64,
            events: self.events,
        }
    }

    fn terminal_vector(
        &mut self,
        state: &G::State,
        world: &SampledWorld,
        traverser: usize,
    ) -> Result<Vec<f64>, SolverError> {
        let mut values = Vec::new();
        self.game
            .terminal_utilities_for_combos(state, world, traverser, &self.combos, &mut values);
        if values.len() != self.combos.len() {
            return Err(SolverError::InvalidState(
                "terminal_utilities_for_combos returned the wrong number of values",
            ));
        }
        for &value in &values {
            if !value.is_finite() {
                return Err(SolverError::NonFiniteUtility {
                    seat: traverser,
                    utility: value,
                });
            }
        }
        self.terminal_evaluations = self
            .terminal_evaluations
            .checked_add(1)
            .ok_or(SolverError::CounterOverflow)?;
        Ok(values)
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse(
        &mut self,
        state: G::State,
        node_id: NodeId,
        world: &SampledWorld,
        traverser: usize,
        own_reach: &[f64],
        sample_importance: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
    ) -> Result<Vec<f64>, SolverError> {
        if depth > self.config.max_traversal_depth {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }

        let Some(actor) = self.game.actor(&state) else {
            return self.terminal_vector(&state, world, traverser);
        };

        let num_players = self.game.num_players();
        if actor >= num_players {
            return Err(SolverError::InvalidActor { actor, num_players });
        }
        let actions = self.game.node_actions(&state);
        let num_actions = self.game.num_actions_of(&actions);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        let node = &self.tree.nodes[node_id as usize];
        if node.action_labels.len() != num_actions {
            return Err(SolverError::TreeNodeMismatch {
                node: node_id,
                expected: node.action_labels.len(),
                found: num_actions,
            });
        }

        if actor == traverser {
            let combos_len = self.combos.len();
            let street_index = node.street.index();
            if self.bucket_cache[street_index].is_none() {
                let table = self
                    .game
                    .buckets_for_combos(&state, world, traverser, &self.combos);
                self.bucket_cache[street_index] = Some(table);
            }
            // Cloned (not borrowed) so the recursive `self.traverse` calls
            // below, which need `&mut self`, don't conflict with a live
            // borrow of `self.bucket_cache`. `BucketId` is a `u32`, so this
            // is a cheap per-visit copy.
            let bucket_table: Vec<BucketId> = self.bucket_cache[street_index]
                .as_ref()
                .expect("populated above")
                .clone();

            let mut bucket_strategy: FxHashMap<BucketId, Vec<f64>> = FxHashMap::default();
            for &bucket in &bucket_table {
                if let Entry::Vacant(entry) = bucket_strategy.entry(bucket) {
                    let range = self.arena.slot_range(node_id, bucket)?;
                    entry.insert(regret_matching(&self.arena.regrets[range]));
                }
            }

            let mut action_values: Vec<Vec<f64>> = Vec::with_capacity(num_actions);
            for action in 0..num_actions {
                let next_state = self.game.next_state_with(&state, &actions, action);
                let child_value = match node.children[action] {
                    Child::Terminal => self.terminal_vector(&next_state, world, traverser)?,
                    Child::Decision(child_id) => {
                        // Per-combo reach for this action's subtree: each
                        // feasible combo's own bucket picks its own
                        // regret-matched probability of taking `action`.
                        let mut child_reach = vec![0.0; combos_len];
                        for idx in 0..combos_len {
                            let strategy = &bucket_strategy[&bucket_table[idx]];
                            child_reach[idx] = own_reach[idx] * strategy[action];
                        }
                        self.traverse(
                            next_state,
                            child_id,
                            world,
                            traverser,
                            &child_reach,
                            sample_importance,
                            rng,
                            depth + 1,
                        )?
                    }
                };
                if child_value.len() != combos_len {
                    return Err(SolverError::InvalidState(
                        "vector traversal child value width changed mid-traversal",
                    ));
                }
                action_values.push(child_value);
            }

            let mut node_value = vec![0.0; combos_len];
            for (idx, value) in node_value.iter_mut().enumerate() {
                let strategy = &bucket_strategy[&bucket_table[idx]];
                let mut total = 0.0;
                for (action, values) in action_values.iter().enumerate() {
                    total += strategy[action] * values[idx];
                }
                if !total.is_finite() {
                    return Err(SolverError::NumericOverflow);
                }
                *value = total;
            }

            // Weighted per-bucket regret aggregation: for bucket b, action
            // a, `sum_{h in F, B(h)=b} weight(h) * (v_a(h) - n(h))`, divided
            // by `sum_{h in F, B(h)=b} weight(h)` -- the feasible-weighted
            // mean described on `Self`. The same pass also accumulates
            // `sum_{h in F, B(h)=b} weight(h) * own_reach(h)`, the dense
            // average-strategy mass for bucket b at this node (see `Self`'s
            // doc comment) -- deliberately a separate accumulator from
            // `bucket_weight_sum`, which stays reach-free for the regret
            // aggregation above.
            let mut bucket_weight_sum: FxHashMap<BucketId, f64> = FxHashMap::default();
            let mut bucket_diff_sum: FxHashMap<BucketId, Vec<f64>> = FxHashMap::default();
            let mut bucket_reach_weight_sum: FxHashMap<BucketId, f64> = FxHashMap::default();
            for idx in 0..combos_len {
                let bucket = bucket_table[idx];
                let weight = self.weights[idx];
                *bucket_weight_sum.entry(bucket).or_insert(0.0) += weight;
                *bucket_reach_weight_sum.entry(bucket).or_insert(0.0) += weight * own_reach[idx];
                let diffs = bucket_diff_sum
                    .entry(bucket)
                    .or_insert_with(|| vec![0.0; num_actions]);
                for (action, values) in action_values.iter().enumerate() {
                    diffs[action] += weight * (values[idx] - node_value[idx]);
                }
            }
            for (bucket, weight_sum) in bucket_weight_sum {
                // Every bucket present in `bucket_table` has at least one
                // feasible member with positive range weight, so this is
                // always strictly positive; the guard is defensive only.
                if weight_sum <= 0.0 {
                    continue;
                }
                let mut diffs = bucket_diff_sum
                    .remove(&bucket)
                    .expect("weight sum and diff sum are populated together above");
                for value in diffs.iter_mut() {
                    let scaled = sample_importance * (*value / weight_sum);
                    if !scaled.is_finite() {
                        return Err(SolverError::NumericOverflow);
                    }
                    *value = scaled;
                }
                let column = self.arena.column_id(node_id, bucket)?;
                self.events.push(DenseEvent::AddRegret {
                    column,
                    values: diffs,
                });

                // Dense average-strategy accumulation: unlike regret, this
                // is not divided by `weight_sum` -- it mirrors the scalar
                // and old vector opponent-branch update
                // (`linear_weight * reach[actor] * sigma`), just summed
                // over every feasible combo in the bucket instead of the
                // one sampled hand. A zero mass (every member combo's line
                // was pruned upstream by a zero-probability ancestor
                // action) contributes nothing, so it's skipped rather than
                // pushing a no-op event.
                let mass = bucket_reach_weight_sum.remove(&bucket).unwrap_or(0.0);
                if mass > 0.0 {
                    let sigma = &bucket_strategy[&bucket];
                    let values: Vec<f64> = sigma
                        .iter()
                        .map(|&probability| self.linear_weight * mass * probability)
                        .collect();
                    self.events.push(DenseEvent::AddStrategy { column, values });
                }
            }

            Ok(node_value)
        } else {
            let private = self.game.bucket(&state, world, actor);
            validate_private_info(private, num_players, RecallMode::Street)?;
            let bucket = private.current_bucket();
            let range = self.arena.slot_range(node_id, bucket)?;
            let strategy = regret_matching(&self.arena.regrets[range]);
            let (action, sampling_probability) =
                sample_exploratory_action(&strategy, self.config.exploration_epsilon, rng);
            let chosen_probability = strategy[action];
            let importance = chosen_probability / sampling_probability;
            let child_importance = sample_importance * importance;
            if !child_importance.is_finite() {
                return Err(SolverError::NumericOverflow);
            }

            // No strategy_sum accumulation here (vector mode moved it to
            // the dense traverser-node accumulation above): this seat's own
            // reach is irrelevant to the traverser's `own_reach` vector, so
            // it is passed through unchanged.
            let next_state = self.game.next_state_with(&state, &actions, action);
            let result = match node.children[action] {
                Child::Terminal => self.terminal_vector(&next_state, world, traverser),
                Child::Decision(child_id) => self.traverse(
                    next_state,
                    child_id,
                    world,
                    traverser,
                    own_reach,
                    child_importance,
                    rng,
                    depth + 1,
                ),
            };
            let mut result = result?;
            for value in result.iter_mut() {
                *value *= importance;
                if !value.is_finite() {
                    return Err(SolverError::NumericOverflow);
                }
            }
            Ok(result)
        }
    }
}

fn resume_configs_match(mut stored: SolverConfig, mut current: SolverConfig) -> bool {
    // This is a process resource guard, not part of the sampled algorithm.
    // Raising it after a resource-limit checkpoint must not alter results.
    stored.max_memory_bytes = 0;
    current.max_memory_bytes = 0;
    // `sweep_batch` is compared like every other algorithm knob (seed,
    // epsilon, discount cadence): batch size changes which linear weights
    // and how much staleness a within-batch update carries, so resuming
    // with a different batch size is rejected rather than silently mixed
    // into one checkpoint's history.
    stored == current
}

fn validate_setup<G: ExternalSamplingGame>(
    game: &G,
    sampler: &DealSampler,
    config: SolverConfig,
) -> Result<(), SolverError> {
    let num_players = game.num_players();
    if !(MIN_SEATS..=MAX_SEATS).contains(&num_players) {
        return Err(SolverError::PlayerCount { found: num_players });
    }
    if sampler.num_players() != num_players {
        return Err(SolverError::SamplerPlayerCount {
            game: num_players,
            sampler: sampler.num_players(),
        });
    }
    if config.max_memory_bytes == 0 {
        return Err(SolverError::ZeroMemoryLimit);
    }
    if config.max_traversal_depth == 0 {
        return Err(SolverError::ZeroDepthLimit);
    }
    if !config.exploration_epsilon.is_finite() || !(0.0..=1.0).contains(&config.exploration_epsilon)
    {
        return Err(SolverError::InvalidExploration {
            epsilon: config.exploration_epsilon,
        });
    }
    if config.discount_every == 0 {
        return Err(SolverError::ZeroDiscountCadence);
    }
    if config.sweep_batch == 0 {
        return Err(SolverError::ZeroSweepBatch);
    }
    if config.traverser_vector && !matches!(game.recall_mode(), RecallMode::Street) {
        return Err(SolverError::VectorTraverserRequiresStreetRecall);
    }
    Ok(())
}

/// `Full` recall requires every already-reached street (`0..=info.street`)
/// to carry a real bucket and every later street to stay
/// [`UNREACHED_BUCKET`]. `Street` recall (imperfect recall) is stricter in
/// the other direction: *only* the current street's slot may be non-
/// sentinel -- earlier streets are deliberately never populated (see
/// [`PrivateInfo::from_current_bucket`]), not just later ones.
fn validate_private_info(
    info: PrivateInfo,
    num_players: usize,
    recall: RecallMode,
) -> Result<(), SolverError> {
    if info.street > 3 {
        return Err(SolverError::InvalidPrivateInfo("street is outside 0..=3"));
    }
    if info.active_opponents == 0 || info.active_opponents as usize >= num_players {
        return Err(SolverError::InvalidPrivateInfo(
            "active opponents is outside 1..players",
        ));
    }
    match recall {
        RecallMode::Full => {
            let reached = info.street as usize + 1;
            if info.bucket_path[..reached].contains(&UNREACHED_BUCKET) {
                return Err(SolverError::InvalidPrivateInfo(
                    "reached street has sentinel bucket",
                ));
            }
            if info.bucket_path[reached..]
                .iter()
                .any(|&bucket| bucket != UNREACHED_BUCKET)
            {
                return Err(SolverError::InvalidPrivateInfo(
                    "future street bucket was exposed",
                ));
            }
        }
        RecallMode::Street => {
            let current = info.street as usize;
            if info.bucket_path[current] == UNREACHED_BUCKET {
                return Err(SolverError::InvalidPrivateInfo(
                    "current street has sentinel bucket",
                ));
            }
            if info
                .bucket_path
                .iter()
                .enumerate()
                .any(|(index, &bucket)| index != current && bucket != UNREACHED_BUCKET)
            {
                return Err(SolverError::InvalidPrivateInfo(
                    "non-current street bucket was exposed under street recall",
                ));
            }
        }
    }
    Ok(())
}

fn validate_column(
    key: InfoKey,
    column: &PolicyColumn,
    num_players: usize,
    recall: RecallMode,
) -> Result<(), SolverError> {
    if key.player as usize >= num_players {
        return Err(SolverError::InvalidState("policy player is out of range"));
    }
    validate_private_info(
        PrivateInfo {
            street: key.street,
            active_opponents: key.active_opponents,
            bucket_path: key.bucket_path,
        },
        num_players,
        recall,
    )?;
    validate_action_labels(&column.action_labels)?;
    if column.regrets.is_empty()
        || column.regrets.len() != column.strategy_sum.len()
        || column.regrets.len() != column.action_labels.len()
    {
        return Err(SolverError::InvalidState(
            "policy vectors are empty or have different lengths",
        ));
    }
    if column
        .regrets
        .iter()
        .chain(&column.strategy_sum)
        .any(|value| !value.is_finite())
    {
        return Err(SolverError::InvalidState(
            "policy contains a non-finite value",
        ));
    }
    if column.strategy_sum.iter().any(|&value| value < 0.0) {
        return Err(SolverError::InvalidState(
            "strategy sums must be non-negative",
        ));
    }
    Ok(())
}

fn validate_history_entry(entry: &HistoryEntry, num_players: usize) -> Result<(), SolverError> {
    if entry.key == HistoryKey::ROOT
        || entry.actor as usize >= num_players
        || entry.action_label.is_empty()
        || entry.key
            != entry
                .parent
                .child(entry.actor as usize, entry.action_index as usize)
    {
        return Err(SolverError::InvalidState("invalid public history entry"));
    }
    Ok(())
}

fn validate_history_graph(
    histories: &FxHashMap<HistoryKey, HistoryEntry>,
) -> Result<(), SolverError> {
    for entry in histories.values() {
        let mut key = entry.parent;
        for _ in 0..=histories.len() {
            if key == HistoryKey::ROOT {
                break;
            }
            key = histories
                .get(&key)
                .ok_or(SolverError::InvalidState(
                    "public history has an unknown parent",
                ))?
                .parent;
        }
        if key != HistoryKey::ROOT {
            return Err(SolverError::InvalidState("public history contains a cycle"));
        }
    }
    Ok(())
}

fn validate_action_labels(labels: &[String]) -> Result<(), SolverError> {
    if labels.iter().any(|label| label.is_empty()) {
        return Err(SolverError::InvalidActionLabels("empty action label"));
    }
    for (index, label) in labels.iter().enumerate() {
        if labels[..index].contains(label) {
            return Err(SolverError::InvalidActionLabels("duplicate action label"));
        }
    }
    Ok(())
}

fn entry_memory_bytes(action_labels: &[String]) -> Result<u64, SolverError> {
    let labels = action_labels.iter().try_fold(0u64, |total, label| {
        total
            .checked_add(size_of::<String>() as u64)
            .and_then(|value| value.checked_add(label.len() as u64))
            .ok_or(SolverError::MemoryAccountingOverflow)
    })?;
    let vector_bytes = (action_labels.len() as u64)
        .checked_mul(2 * size_of::<f32>() as u64)
        .ok_or(SolverError::MemoryAccountingOverflow)?;
    ((size_of::<InfoKey>() + size_of::<PolicyColumn>()) as u64)
        .checked_add(ENTRY_OVERHEAD_BYTES)
        .and_then(|value| value.checked_add(vector_bytes))
        .and_then(|value| value.checked_add(labels))
        .ok_or(SolverError::MemoryAccountingOverflow)
}

fn history_memory_bytes(action_label: &str) -> Result<u64, SolverError> {
    (size_of::<HistoryKey>() as u64)
        .checked_add(size_of::<HistoryEntry>() as u64)
        .and_then(|value| value.checked_add(HISTORY_OVERHEAD_BYTES))
        .and_then(|value| value.checked_add(action_label.len() as u64))
        .ok_or(SolverError::MemoryAccountingOverflow)
}

fn regret_matching(regrets: &[f32]) -> Vec<f64> {
    let sum = regrets
        .iter()
        .map(|&regret| f64::from(regret.max(0.0)))
        .sum::<f64>();
    if sum > 0.0 {
        regrets
            .iter()
            .map(|&regret| f64::from(regret.max(0.0)) / sum)
            .collect()
    } else {
        vec![1.0 / regrets.len() as f64; regrets.len()]
    }
}

fn regret_matching_f32(regrets: &[f32]) -> Vec<f32> {
    let sum = regrets.iter().map(|&regret| regret.max(0.0)).sum::<f32>();
    if sum > 0.0 {
        regrets
            .iter()
            .map(|&regret| regret.max(0.0) / sum)
            .collect()
    } else {
        vec![1.0 / regrets.len() as f32; regrets.len()]
    }
}

fn normalize_nonnegative_f32(values: &[f32]) -> Option<Vec<f32>> {
    let sum = values.iter().copied().sum::<f32>();
    (sum > 0.0).then(|| values.iter().map(|&value| value / sum).collect())
}

fn checked_add_f32(target: &mut f32, delta: f64) -> Result<(), SolverError> {
    let value = f64::from(*target) + delta;
    if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
        return Err(SolverError::NumericOverflow);
    }
    *target = value as f32;
    Ok(())
}

fn sample_exploratory_action(
    strategy: &[f64],
    epsilon: f64,
    rng: &mut ChaCha20Rng,
) -> (usize, f64) {
    let needle = rng.gen_range(0.0..1.0);
    let mut cumulative = 0.0;
    let uniform = epsilon / strategy.len() as f64;
    for (action, &probability) in strategy.iter().enumerate() {
        let sampling_probability = (1.0 - epsilon) * probability + uniform;
        cumulative += sampling_probability;
        if needle < cumulative {
            return (action, sampling_probability);
        }
    }
    let action = strategy.len() - 1;
    let sampling_probability = (1.0 - epsilon) * strategy[action] + uniform;
    (action, sampling_probability)
}

fn sample_profile_action(strategy: &[f32], rng: &mut ChaCha20Rng) -> usize {
    let needle = rng.gen_range(0.0..1.0);
    let mut cumulative = 0.0;
    for (action, &probability) in strategy.iter().enumerate() {
        cumulative += f64::from(probability);
        if needle < cumulative {
            return action;
        }
    }
    strategy.len() - 1
}

fn traversal_deal_rng(seed: u64, sample_id: u64, traverser: usize) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.mccfr-deal.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    hasher.update(&(traverser as u64).to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

fn traversal_action_rng(seed: u64, sample_id: u64, traverser: usize) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.mccfr-actions.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    hasher.update(&(traverser as u64).to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

fn evaluation_deal_rng(seed: u64, sample_id: u64) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.profile-evaluation-deal.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

fn evaluation_action_rng(seed: u64, sample_id: u64, deviator: Option<usize>) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    match deviator {
        Some(player) => {
            hasher.update(b"solvers.multiway.profile-evaluation-deviation.v1");
            hasher.update(&(player as u64).to_le_bytes());
        }
        None => {
            hasher.update(b"solvers.multiway.profile-evaluation-baseline.v1");
        }
    }
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

fn traversals_for_player(traversals: u64, num_players: usize, player: usize) -> u64 {
    traversals / num_players as u64 + u64::from((player as u64) < traversals % num_players as u64)
}

#[derive(Debug, thiserror::Error)]
pub enum SolverError {
    #[error(transparent)]
    Sample(#[from] SampleError),
    #[error("multiway solver requires {MIN_SEATS}..={MAX_SEATS} players, found {found}")]
    PlayerCount { found: usize },
    #[error("game has {game} players but sampler has {sampler}")]
    SamplerPlayerCount { game: usize, sampler: usize },
    #[error("memory limit must be positive")]
    ZeroMemoryLimit,
    #[error("traversal depth limit must be positive")]
    ZeroDepthLimit,
    #[error("exploration epsilon must be finite and in [0, 1], found {epsilon}")]
    InvalidExploration { epsilon: f64 },
    #[error("invalid private information: {0}")]
    InvalidPrivateInfo(&'static str),
    #[error("action labels are invalid: {0}")]
    InvalidActionLabels(&'static str),
    #[error("action labels changed at {key:?}")]
    ActionLabelsChanged { key: InfoKey },
    #[error("actor {actor} is outside a {num_players}-player game")]
    InvalidActor { actor: usize, num_players: usize },
    #[error("non-terminal state for actor {actor} has no actions")]
    NoActions { actor: usize },
    #[error("public game traversal exceeded depth limit {limit}")]
    DepthLimit { limit: u32 },
    #[error("terminal utility for seat {seat} is not finite: {utility}")]
    NonFiniteUtility { seat: usize, utility: f64 },
    #[error("policy action count changed at {key:?}: stored {stored}, current {current}")]
    ActionCountChanged {
        key: InfoKey,
        stored: usize,
        current: usize,
    },
    #[error("sparse policy memory cap {limit} bytes exceeded; next node needs {needed} bytes")]
    MemoryLimit { limit: u64, needed: u64 },
    #[error("discount cadence must be positive")]
    ZeroDiscountCadence,
    #[error("sweep batch size must be positive")]
    ZeroSweepBatch,
    #[error("multiway parallel worker count must be positive")]
    ZeroThreads,
    #[error("failed to build deterministic multiway worker pool: {0}")]
    ThreadPoolBuild(String),
    #[error("checkpoint algorithm configuration does not match the current solver configuration")]
    ResumeConfigurationMismatch,
    #[error("complete parallel sweeps cannot start from a partial-sweep solver state")]
    IncompleteSweepState,
    #[error("profile evaluation sample count must be positive")]
    ZeroEvaluationSamples,
    #[error("numeric accumulation exceeded f32 storage")]
    NumericOverflow,
    #[error("counter overflow")]
    CounterOverflow,
    #[error("traversal count overflow")]
    TraversalCountOverflow,
    #[error("policy memory accounting overflow")]
    MemoryAccountingOverflow,
    #[error("solver state version {found} is unsupported (expected {expected})")]
    StateVersion { found: u16, expected: u16 },
    #[error("invalid solver state: {0}")]
    InvalidState(&'static str),
    #[error("duplicate policy key in solver state: {0:?}")]
    DuplicatePolicy(InfoKey),
    #[error("duplicate public history key in solver state: {0:?}")]
    DuplicateHistory(HistoryKey),
    #[error("public history hash collision or unstable action label at {0:?}")]
    HistoryCollision(HistoryKey),
    #[error("public history actor/action index does not fit checkpoint format")]
    HistoryIndexOverflow,
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error(
        "dense tree node {node} expected {expected} actions but the game produced {found}; the \
         betting tree changed since the arena was preallocated"
    )]
    TreeNodeMismatch {
        node: NodeId,
        expected: usize,
        found: usize,
    },
    #[error("dense policy entry at {key:?} does not map to an enumerated dense arena slot")]
    UnmappedDenseEntry { key: InfoKey },
    #[error("dense history entry {0:?} does not match the enumerated public tree")]
    UnmappedDenseHistory(HistoryKey),
    #[error(
        "SolverConfig::traverser_vector requires recall = \"street\" (the dense arena); the \
         current game uses RecallMode::Full"
    )]
    VectorTraverserRequiresStreetRecall,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cards::Range;
    use rand::RngCore;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ToyState {
        Choose,
        Terminal(usize),
    }

    #[derive(Clone, Copy)]
    struct DominatedChoice;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PrefixState {
        Opponent,
        Hero,
        Terminal(usize),
    }

    #[derive(Clone, Copy)]
    struct PrefixImportanceGame;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ThreePlayerOracleState {
        Opponent,
        Hero {
            opponent_action: usize,
        },
        Terminal {
            opponent_action: usize,
            hero_action: usize,
        },
    }

    #[derive(Clone, Copy)]
    struct ThreePlayerOracleGame;

    const ORACLE_HERO_PAYOFFS: [[f64; 2]; 2] = [[4.0, 0.0], [-2.0, 2.0]];

    impl ExternalSamplingGame for PrefixImportanceGame {
        type State = PrefixState;
        // Cheap for a toy game: the label/count logic only needs to know
        // which node kind it is, so the state itself doubles as the
        // precomputed action list.
        type Actions = PrefixState;

        fn num_players(&self) -> usize {
            2
        }

        fn root_state(&self) -> Self::State {
            PrefixState::Opponent
        }

        fn actor(&self, state: &Self::State) -> Option<usize> {
            match state {
                PrefixState::Opponent => Some(1),
                PrefixState::Hero => Some(0),
                PrefixState::Terminal(_) => None,
            }
        }

        fn node_actions(&self, state: &Self::State) -> Self::Actions {
            *state
        }

        fn num_actions_of(&self, actions: &Self::Actions) -> usize {
            usize::from(!matches!(actions, PrefixState::Terminal(_))) * 2
        }

        fn next_state_with(
            &self,
            _state: &Self::State,
            actions: &Self::Actions,
            action_index: usize,
        ) -> Self::State {
            match actions {
                PrefixState::Opponent => PrefixState::Hero,
                PrefixState::Hero => PrefixState::Terminal(action_index),
                PrefixState::Terminal(_) => panic!("terminal state has no child"),
            }
        }

        fn write_action_label(
            &self,
            actions: &Self::Actions,
            action_index: usize,
            out: &mut String,
        ) {
            let labels = match actions {
                PrefixState::Opponent => ["left", "right"],
                PrefixState::Hero => ["win", "pass"],
                PrefixState::Terminal(_) => panic!("terminal state has no actions"),
            };
            out.push_str(labels[action_index]);
        }

        fn bucket(
            &self,
            _state: &Self::State,
            _world: &SampledWorld,
            _actor: usize,
        ) -> PrivateInfo {
            PrivateInfo {
                street: 0,
                active_opponents: 1,
                bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
            }
        }

        fn terminal_utilities(
            &self,
            state: &Self::State,
            _world: &SampledWorld,
            utilities: &mut [f64],
        ) {
            let PrefixState::Terminal(action) = *state else {
                panic!("not terminal")
            };
            utilities[0] = f64::from(action == 0);
            utilities[1] = -utilities[0];
        }
    }

    impl ExternalSamplingGame for ThreePlayerOracleGame {
        type State = ThreePlayerOracleState;
        type Actions = ThreePlayerOracleState;

        fn num_players(&self) -> usize {
            3
        }

        fn root_state(&self) -> Self::State {
            ThreePlayerOracleState::Opponent
        }

        fn actor(&self, state: &Self::State) -> Option<usize> {
            match state {
                ThreePlayerOracleState::Opponent => Some(1),
                ThreePlayerOracleState::Hero { .. } => Some(0),
                ThreePlayerOracleState::Terminal { .. } => None,
            }
        }

        fn node_actions(&self, state: &Self::State) -> Self::Actions {
            *state
        }

        fn num_actions_of(&self, actions: &Self::Actions) -> usize {
            usize::from(!matches!(actions, ThreePlayerOracleState::Terminal { .. })) * 2
        }

        fn next_state_with(
            &self,
            _state: &Self::State,
            actions: &Self::Actions,
            action_index: usize,
        ) -> Self::State {
            match *actions {
                ThreePlayerOracleState::Opponent => ThreePlayerOracleState::Hero {
                    opponent_action: action_index,
                },
                ThreePlayerOracleState::Hero { opponent_action } => {
                    ThreePlayerOracleState::Terminal {
                        opponent_action,
                        hero_action: action_index,
                    }
                }
                ThreePlayerOracleState::Terminal { .. } => {
                    panic!("terminal state has no child")
                }
            }
        }

        fn write_action_label(
            &self,
            actions: &Self::Actions,
            action_index: usize,
            out: &mut String,
        ) {
            let labels = match actions {
                ThreePlayerOracleState::Opponent => ["left", "right"],
                ThreePlayerOracleState::Hero { .. } => ["take", "pass"],
                ThreePlayerOracleState::Terminal { .. } => {
                    panic!("terminal state has no actions")
                }
            };
            out.push_str(labels[action_index]);
        }

        fn bucket(
            &self,
            _state: &Self::State,
            _world: &SampledWorld,
            _actor: usize,
        ) -> PrivateInfo {
            PrivateInfo {
                street: 0,
                active_opponents: 2,
                bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
            }
        }

        fn terminal_utilities(
            &self,
            state: &Self::State,
            _world: &SampledWorld,
            utilities: &mut [f64],
        ) {
            let ThreePlayerOracleState::Terminal {
                opponent_action,
                hero_action,
            } = *state
            else {
                panic!("not terminal")
            };
            utilities[0] = ORACLE_HERO_PAYOFFS[opponent_action][hero_action];
            // Deliberately general-sum: neither opponent payoff is the
            // negative of the traverser's payoff.
            utilities[1] = [[1.0, 3.0], [5.0, -1.0]][opponent_action][hero_action];
            utilities[2] = [[2.0, -1.0], [1.0, 4.0]][opponent_action][hero_action];
        }
    }

    fn full_enumeration_oracle_regret(
        opponent_strategy: [f64; 2],
        hero_strategy: [f64; 2],
    ) -> [f64; 2] {
        let mut regrets = [0.0; 2];
        for opponent_action in 0..2 {
            let values = ORACLE_HERO_PAYOFFS[opponent_action];
            let node_value = hero_strategy
                .iter()
                .zip(values)
                .map(|(&probability, value)| probability * value)
                .sum::<f64>();
            for action in 0..2 {
                regrets[action] +=
                    opponent_strategy[opponent_action] * (values[action] - node_value);
            }
        }
        regrets
    }

    impl ExternalSamplingGame for DominatedChoice {
        type State = ToyState;
        type Actions = ToyState;

        fn num_players(&self) -> usize {
            2
        }

        fn root_state(&self) -> Self::State {
            ToyState::Choose
        }

        fn actor(&self, state: &Self::State) -> Option<usize> {
            matches!(state, ToyState::Choose).then_some(0)
        }

        fn node_actions(&self, state: &Self::State) -> Self::Actions {
            *state
        }

        fn num_actions_of(&self, actions: &Self::Actions) -> usize {
            usize::from(matches!(actions, ToyState::Choose)) * 2
        }

        fn next_state_with(
            &self,
            _state: &Self::State,
            actions: &Self::Actions,
            action_index: usize,
        ) -> Self::State {
            assert_eq!(*actions, ToyState::Choose);
            ToyState::Terminal(action_index)
        }

        fn write_action_label(
            &self,
            _actions: &Self::Actions,
            action_index: usize,
            out: &mut String,
        ) {
            let label = match action_index {
                0 => "best",
                1 => "dominated",
                _ => panic!("action out of range"),
            };
            out.push_str(label);
        }

        fn bucket(
            &self,
            _state: &Self::State,
            _world: &SampledWorld,
            _actor: usize,
        ) -> PrivateInfo {
            PrivateInfo {
                street: 0,
                active_opponents: 1,
                bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
            }
        }

        fn terminal_utilities(
            &self,
            state: &Self::State,
            _world: &SampledWorld,
            utilities: &mut [f64],
        ) {
            let ToyState::Terminal(action) = *state else {
                panic!("not terminal")
            };
            utilities[0] = if action == 0 { 1.0 } else { -1.0 };
            utilities[1] = -utilities[0];
        }

        fn game_fingerprint(&self) -> [u8; 32] {
            *blake3::hash(b"dominated-choice-v1").as_bytes()
        }
    }

    fn solver(seed: u64, memory: u64) -> MultiwaySolver<DominatedChoice> {
        solver_with_batch(seed, memory, 1)
    }

    fn solver_with_batch(
        seed: u64,
        memory: u64,
        sweep_batch: u64,
    ) -> MultiwaySolver<DominatedChoice> {
        MultiwaySolver::new(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed,
                max_memory_bytes: memory,
                max_traversal_depth: 16,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: 5,
                discount_until: 100,
                sweep_batch,
                traverser_vector: false,
            },
        )
        .unwrap()
    }

    fn root_key() -> InfoKey {
        InfoKey {
            history: HistoryKey::ROOT,
            player: 0,
            street: 0,
            active_opponents: 1,
            bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        }
    }

    #[test]
    fn dominated_action_disappears_from_current_and_average_policy() {
        let mut solver = solver(11, 1 << 20);
        solver.run_sweeps(100).unwrap();
        assert!(solver.current_strategy(root_key()).unwrap()[0] > 0.99);
        assert!(solver.average_strategy(root_key()).unwrap()[0] > 0.95);
        assert_eq!(
            solver.average_action_probabilities(root_key()).unwrap()[0].action,
            "best"
        );
        let metrics = solver.metrics();
        assert_eq!(metrics.sweeps, 100);
        assert_eq!(metrics.traversals, 200);
        assert_eq!(metrics.average_positive_regret.len(), 2);
        assert!(metrics.infosets >= 1);
    }

    #[test]
    fn batched_discount_scales_regret_and_strategy_at_cadence() {
        let mut solver = solver(1, 1 << 20);
        solver.config.discount_every = 2;
        solver.config.discount_until = 5;
        solver.run_sweeps(1).unwrap();
        assert_eq!(solver.policy(root_key()).unwrap().regrets, vec![1.0, -1.0]);
        solver.run_sweeps(1).unwrap();
        let column = solver.policy(root_key()).unwrap();
        assert_eq!(column.regrets, vec![0.5, -1.5]);
        assert_eq!(column.strategy_sum, vec![1.25, 0.25]);
    }

    #[test]
    fn held_out_profile_evaluation_is_deterministic_and_read_only() {
        let mut solver = solver(91, 1 << 20);
        solver.run_sweeps(25).unwrap();
        let before = solver.snapshot_state();
        let a = solver.evaluate_average_profile(128, 2026).unwrap();
        let b = solver.evaluate_average_profile(128, 2026).unwrap();
        assert_eq!(a, b);
        assert_eq!(solver.snapshot_state(), before);
        assert_eq!(a.samples, 128);
        assert_eq!(a.seats.len(), 2);
        assert!(a.seats[0].mean > 0.8);
        let gains = a.deviation_gain_lower_bound.as_ref().unwrap();
        assert_eq!(gains.len(), 2);
        assert!(
            gains
                .iter()
                .all(|gain| gain.mean >= 0.0 && gain.ci95[0] >= 0.0)
        );
        assert_eq!(gains[1].mean, 0.0);
        assert_eq!(gains[1].stderr, 0.0);
        assert!(
            a.seats
                .iter()
                .all(|seat| seat.ci95[0] <= seat.mean && seat.mean <= seat.ci95[1])
        );
    }

    #[test]
    fn ordered_deltas_make_thread_counts_and_resume_bit_identical() {
        let mut uninterrupted = solver(777, 1 << 20);
        uninterrupted.run_sweeps_with_threads(40, 4).unwrap();

        let mut single_threaded = solver(777, 1 << 20);
        single_threaded.run_sweeps_with_threads(40, 1).unwrap();
        assert_eq!(
            uninterrupted.snapshot_state(),
            single_threaded.snapshot_state()
        );

        let mut first_half = solver(777, 1 << 20);
        first_half.run_sweeps_with_threads(17, 1).unwrap();
        let state = first_half.snapshot_state();
        let mut resumed = MultiwaySolver::from_state(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state,
        )
        .unwrap();
        resumed.run_sweeps_with_threads(23, 4).unwrap();

        assert_eq!(uninterrupted.snapshot_state(), resumed.snapshot_state());
        assert_eq!(uninterrupted.metrics(), resumed.metrics());
    }

    #[test]
    fn sweep_batching_is_bit_identical_across_thread_counts() {
        let mut single_threaded = solver_with_batch(99, 1 << 20, 4);
        single_threaded.run_sweeps_with_threads(24, 1).unwrap();

        let mut multi_threaded = solver_with_batch(99, 1 << 20, 4);
        multi_threaded.run_sweeps_with_threads(24, 8).unwrap();

        assert_eq!(
            single_threaded.snapshot_state(),
            multi_threaded.snapshot_state()
        );
        assert_eq!(single_threaded.metrics(), multi_threaded.metrics());

        // An uneven final batch (24 sweeps is not a multiple of the batch
        // size below) still lands on exactly the requested sweep count.
        let mut uneven = solver_with_batch(99, 1 << 20, 5);
        assert_eq!(
            uneven
                .run_sweeps_with_threads_until(24, 4, || true)
                .unwrap(),
            24
        );
        assert_eq!(uneven.completed_sweeps(), 24);
    }

    #[test]
    fn sweep_batching_reduces_regret_comparably_to_unbatched() {
        const SWEEPS: u64 = 400;

        // Disables early discounting for this comparison (unlike the
        // `solver`/`solver_with_batch` fixtures above, which intentionally
        // discount every 5 sweeps to exercise that path elsewhere): batching
        // interacts with aggressive discounting cadence in ways unrelated to
        // what this test checks, and would otherwise dominate the
        // batch-vs-unbatched difference in an already-tiny toy-game regret
        // signal.
        fn solver_without_discount(seed: u64, sweep_batch: u64) -> MultiwaySolver<DominatedChoice> {
            MultiwaySolver::new(
                DominatedChoice,
                DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
                SolverConfig {
                    seed,
                    max_memory_bytes: 1 << 20,
                    max_traversal_depth: 16,
                    exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                    discount_every: DEFAULT_DISCOUNT_EVERY,
                    discount_until: DEFAULT_DISCOUNT_UNTIL,
                    sweep_batch,
                    traverser_vector: false,
                },
            )
            .unwrap()
        }

        fn mean_positive_regret(regrets: &[f64]) -> f64 {
            regrets.iter().sum::<f64>() / regrets.len() as f64
        }

        let mut baseline = solver_without_discount(4242, 1);
        baseline.run_sweeps_with_threads(SWEEPS, 1).unwrap();
        let baseline_regret = mean_positive_regret(&baseline.metrics().average_positive_regret);

        let mut batched = solver_without_discount(4242, 4);
        batched.run_sweeps_with_threads(SWEEPS, 1).unwrap();
        let batched_regret = mean_positive_regret(&batched.metrics().average_positive_regret);

        assert!(baseline_regret.is_finite() && baseline_regret >= 0.0);
        assert!(batched_regret.is_finite() && batched_regret >= 0.0);
        // Both must actually be converging (small relative to the game's
        // unit-scale payoffs), and neither must be more than 2x the other --
        // an absolute floor keeps the ratio check meaningful once both sides
        // are already near zero.
        let tolerance = (baseline_regret.max(batched_regret) * 2.0).max(1e-3);
        assert!(
            batched_regret <= tolerance && baseline_regret <= tolerance,
            "batch=4 average positive regret {batched_regret} should be within 2x of batch=1's {baseline_regret}"
        );
    }

    #[test]
    fn resume_rejects_mismatched_sweep_batch() {
        let state = solver_with_batch(21, 1 << 20, 1).snapshot_state();
        let mut changed = state.config;
        changed.sweep_batch = 4;
        assert!(matches!(
            MultiwaySolver::from_state_with_config(
                DominatedChoice,
                DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
                state,
                changed,
            ),
            Err(SolverError::ResumeConfigurationMismatch)
        ));
    }

    #[test]
    fn zero_sweep_batch_is_rejected() {
        let mut config = SolverConfig {
            seed: 1,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: 5,
            discount_until: 100,
            sweep_batch: 1,
            traverser_vector: false,
        };
        config.sweep_batch = 0;
        assert!(matches!(
            MultiwaySolver::new(
                DominatedChoice,
                DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
                config,
            ),
            Err(SolverError::ZeroSweepBatch)
        ));
    }

    #[test]
    fn interruptible_parallel_run_polls_boundaries_and_preserves_determinism() {
        let mut interrupted = solver(0x1234, 1 << 20);
        let mut polls = 0u64;
        let completed = interrupted
            .run_sweeps_with_threads_until(40, 4, || {
                polls += 1;
                polls <= 7
            })
            .unwrap();
        assert_eq!(completed, 7);
        assert_eq!(polls, 8);

        let mut reference = solver(0x1234, 1 << 20);
        reference.run_sweeps_with_threads(7, 1).unwrap();
        assert_eq!(interrupted.snapshot_state(), reference.snapshot_state());
    }

    #[test]
    fn resource_limit_state_resumes_with_a_larger_operational_limit() {
        let mut limited = solver(55, 1);
        assert!(matches!(
            limited.run_sweeps_with_threads(1, 2),
            Err(SolverError::MemoryLimit { limit: 1, .. })
        ));
        let fingerprint = limited.configuration_fingerprint();
        let state = limited.snapshot_state();
        let mut expanded_config = state.config;
        expanded_config.max_memory_bytes = 1 << 20;

        let mut resumed = MultiwaySolver::from_state_with_config(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state,
            expanded_config,
        )
        .unwrap();
        assert_eq!(resumed.config().max_memory_bytes, 1 << 20);
        assert_eq!(resumed.configuration_fingerprint(), fingerprint);
        resumed.run_sweeps_with_threads(10, 4).unwrap();

        let mut reference = solver(55, 1 << 20);
        reference.run_sweeps_with_threads(10, 1).unwrap();
        assert_eq!(resumed.snapshot_state(), reference.snapshot_state());
    }

    #[test]
    fn restore_rejects_algorithm_changes_even_when_memory_limit_changes() {
        let state = solver(9, 1 << 10).snapshot_state();
        let mut changed = state.config;
        changed.max_memory_bytes = 1 << 20;
        changed.exploration_epsilon = 0.5;
        assert!(matches!(
            MultiwaySolver::from_state_with_config(
                DominatedChoice,
                DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
                state,
                changed,
            ),
            Err(SolverError::ResumeConfigurationMismatch)
        ));
    }

    #[test]
    fn independent_action_substream_ignores_deal_rng_consumption() {
        let mut short_deal = traversal_deal_rng(41, 19, 1);
        let mut long_deal = traversal_deal_rng(41, 19, 1);
        let _ = short_deal.next_u64();
        for _ in 0..10_000 {
            let _ = long_deal.next_u64();
        }

        let mut after_short = traversal_action_rng(41, 19, 1);
        let mut after_long = traversal_action_rng(41, 19, 1);
        let left: Vec<_> = (0..32).map(|_| after_short.next_u64()).collect();
        let right: Vec<_> = (0..32).map(|_| after_long.next_u64()).collect();
        assert_eq!(left, right);
    }

    #[test]
    fn resume_rejects_inconsistent_sample_id() {
        let solver = solver(17, 1 << 20);
        let mut state = solver.snapshot_state();
        state.next_sample_id = 1;
        assert!(matches!(
            MultiwaySolver::from_state(
                DominatedChoice,
                DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
                state
            ),
            Err(SolverError::InvalidState(
                "next sample id is inconsistent with traversals"
            ))
        ));
    }

    #[test]
    fn memory_cap_stops_before_allocating_first_column() {
        let mut limited = solver(0, 1);
        let before = limited.snapshot_state();
        assert!(matches!(
            limited.run_sweeps_with_threads(1, 4),
            Err(SolverError::MemoryLimit { limit: 1, .. })
        ));
        assert_eq!(limited.snapshot_state(), before);
        assert_eq!(limited.metrics().infosets, 0);
        assert_eq!(limited.metrics().traversals, 0);

        // merge_sweep's transactional guarantee also means the solver stays
        // resumable after a MemoryLimit error: raising the memory budget and
        // continuing from the untouched (pre-failure) state must produce the
        // exact same result as a solver that had that budget from the start.
        let mut expanded_config = limited.config();
        expanded_config.max_memory_bytes = 1 << 20;
        let mut resumed = MultiwaySolver::from_state_with_config(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            before,
            expanded_config,
        )
        .unwrap();
        resumed.run_sweeps_with_threads(5, 4).unwrap();

        let mut reference = solver(0, 1 << 20);
        reference.run_sweeps_with_threads(5, 4).unwrap();
        assert_eq!(resumed.snapshot_state(), reference.snapshot_state());
    }

    #[test]
    fn exploration_corrects_regret_updates_below_sampled_opponent_actions() {
        let mut solver = MultiwaySolver::new(
            PrefixImportanceGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed: 73,
                max_memory_bytes: 1 << 20,
                max_traversal_depth: 16,
                exploration_epsilon: 1.0,
                discount_every: 1,
                discount_until: 0,
                sweep_batch: 1,
                traverser_vector: false,
            },
        )
        .unwrap();
        let opponent_key = InfoKey {
            history: HistoryKey::ROOT,
            player: 1,
            street: 0,
            active_opponents: 1,
            bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        };
        solver
            .strategy_for(opponent_key, &PrefixState::Opponent)
            .unwrap();
        solver.policies.get_mut(&opponent_key).unwrap().regrets = vec![9.0, 1.0];

        solver.run_traversals(1).unwrap();

        let (hero_key, hero_column) = solver
            .policies
            .iter()
            .find(|(key, _)| key.player == 0)
            .expect("sampled branch creates the hero information set");
        let target_probability = if hero_key.history == HistoryKey::ROOT.child(1, 0) {
            0.9
        } else {
            assert_eq!(hero_key.history, HistoryKey::ROOT.child(1, 1));
            0.1
        };
        let importance = target_probability / 0.5;
        assert!((f64::from(hero_column.regrets[0]) - 0.5 * importance).abs() < 1e-6);
        assert!((f64::from(hero_column.regrets[1]) + 0.5 * importance).abs() < 1e-6);
    }

    #[test]
    fn three_player_one_step_estimator_matches_full_enumeration_oracle() {
        let mut solver = MultiwaySolver::new(
            ThreePlayerOracleGame,
            DealSampler::new(vec![Range::full(), Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed: 0x5eed,
                max_memory_bytes: 1 << 20,
                max_traversal_depth: 16,
                // Sampling q is uniform while the target opponent strategy
                // below is [0.75, 0.25], exercising importance correction.
                exploration_epsilon: 1.0,
                discount_every: 1,
                discount_until: 0,
                sweep_batch: 1,
                traverser_vector: false,
            },
        )
        .unwrap();
        let opponent_key = InfoKey {
            history: HistoryKey::ROOT,
            player: 1,
            street: 0,
            active_opponents: 2,
            bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        };
        solver
            .strategy_for(opponent_key, &ThreePlayerOracleState::Opponent)
            .unwrap();
        solver.policies.get_mut(&opponent_key).unwrap().regrets = vec![3.0, 1.0];

        let oracle = full_enumeration_oracle_regret([0.75, 0.25], [0.5, 0.5]);
        assert_eq!(oracle, [1.0, -1.0]);
        let mut terminal = [0.0; 3];
        ThreePlayerOracleGame.terminal_utilities(
            &ThreePlayerOracleState::Terminal {
                opponent_action: 0,
                hero_action: 0,
            },
            &solver
                .sampler
                .sample(&mut traversal_deal_rng(9, 0, 0))
                .unwrap(),
            &mut terminal,
        );
        assert_ne!(terminal.iter().sum::<f64>(), 0.0);

        const SAMPLES: u64 = 20_000;
        let mut means = [0.0; 2];
        let mut m2 = [0.0; 2];
        for sample_id in 0..SAMPLES {
            let delta = solver.generate_traversal_delta(sample_id, 0, 1.0).unwrap();
            let AnyTraversalDelta::Sparse(delta) = delta else {
                panic!("full-recall solver must produce a sparse traversal delta")
            };
            let values = delta
                .events
                .into_iter()
                .find_map(|event| match event {
                    TraversalEvent::AddRegret { key, values } if key.player == 0 => Some(values),
                    _ => None,
                })
                .expect("traverser update must be present");
            assert_eq!(values.len(), 2);
            let count = (sample_id + 1) as f64;
            for action in 0..2 {
                let difference = values[action] - means[action];
                means[action] += difference / count;
                m2[action] += difference * (values[action] - means[action]);
            }
        }
        for action in 0..2 {
            let variance = m2[action] / (SAMPLES - 1) as f64;
            let stderr = (variance / SAMPLES as f64).sqrt();
            assert!(
                (means[action] - oracle[action]).abs() <= 5.0 * stderr,
                "action {action}: sample mean {}, oracle {}, stderr {stderr}",
                means[action],
                oracle[action]
            );
        }
    }

    #[test]
    fn history_keys_are_stable_and_action_sensitive() {
        assert_eq!(HistoryKey::ROOT.child(0, 1), HistoryKey::ROOT.child(0, 1));
        assert_ne!(HistoryKey::ROOT.child(0, 0), HistoryKey::ROOT.child(0, 1));
        assert_ne!(HistoryKey::ROOT.child(0, 1), HistoryKey::ROOT.child(1, 1));
    }

    #[test]
    fn visited_history_trie_resolves_stable_action_labels() {
        let mut solver = solver(5, 1 << 20);
        solver.run_traversals(1).unwrap();
        let best = HistoryKey::ROOT.child(0, 0);
        let dominated = HistoryKey::ROOT.child(0, 1);
        assert_eq!(solver.resolve_history(HistoryKey::ROOT), Some(Vec::new()));
        assert_eq!(solver.resolve_history(best), Some(vec!["best".to_string()]));
        assert_eq!(
            solver.resolve_history(dominated),
            Some(vec!["dominated".to_string()])
        );
        let state = solver.snapshot_state();
        assert_eq!(state.histories.len(), 2);
        assert!(
            state
                .histories
                .windows(2)
                .all(|pair| pair[0].key < pair[1].key)
        );
    }

    #[test]
    fn node_children_and_strategies_at_expose_a_live_average_profile() {
        let mut solver = solver(7, 1 << 20);
        solver.run_sweeps(20).unwrap();

        let children = solver.node_children(HistoryKey::ROOT);
        assert!(!children.is_empty(), "expected root's children to exist");
        let mut labels: Vec<&str> = children
            .iter()
            .map(|entry| entry.action_label.as_str())
            .collect();
        labels.sort_unstable();
        assert_eq!(labels, vec!["best", "dominated"]);
        assert!(children.windows(2).all(|pair| {
            (pair[0].actor, pair[0].action_index) <= (pair[1].actor, pair[1].action_index)
        }));

        let rows = solver.strategies_at(HistoryKey::ROOT);
        assert!(!rows.is_empty(), "expected a live policy at root");
        for (_, action_labels, probabilities) in &rows {
            assert_eq!(action_labels.len(), probabilities.len());
            let sum: f32 = probabilities.iter().sum();
            assert!((sum - 1.0).abs() < 1e-3, "probabilities summed to {sum}");
        }
    }

    // --- RecallMode::Street (dense arena) -----------------------------------

    /// Single-decision-node game shaped exactly like [`DominatedChoice`], but
    /// opted into `RecallMode::Street` with a trivial one-bucket abstraction.
    /// Because the game shape, seeds, and per-node math are otherwise
    /// identical, a dense-mode run must reproduce the *exact* regret/
    /// strategy-sum trajectory `DominatedChoice`'s sparse tests already pin
    /// (see [`batched_discount_scales_regret_and_strategy_at_cadence`]) --
    /// strong evidence the dense traversal/merge path computes the same
    /// numbers as the sparse one, just through a different storage backend.
    #[derive(Clone, Copy)]
    struct DenseDominatedChoice;

    impl ExternalSamplingGame for DenseDominatedChoice {
        type State = ToyState;
        type Actions = ToyState;

        fn num_players(&self) -> usize {
            2
        }

        fn root_state(&self) -> Self::State {
            ToyState::Choose
        }

        fn actor(&self, state: &Self::State) -> Option<usize> {
            matches!(state, ToyState::Choose).then_some(0)
        }

        fn node_actions(&self, state: &Self::State) -> Self::Actions {
            *state
        }

        fn num_actions_of(&self, actions: &Self::Actions) -> usize {
            usize::from(matches!(actions, ToyState::Choose)) * 2
        }

        fn next_state_with(
            &self,
            _state: &Self::State,
            actions: &Self::Actions,
            action_index: usize,
        ) -> Self::State {
            assert_eq!(*actions, ToyState::Choose);
            ToyState::Terminal(action_index)
        }

        fn write_action_label(
            &self,
            _actions: &Self::Actions,
            action_index: usize,
            out: &mut String,
        ) {
            let label = match action_index {
                0 => "best",
                1 => "dominated",
                _ => panic!("action out of range"),
            };
            out.push_str(label);
        }

        fn bucket(
            &self,
            _state: &Self::State,
            _world: &SampledWorld,
            _actor: usize,
        ) -> PrivateInfo {
            PrivateInfo::from_current_bucket(Street::Preflop, 1, 0)
        }

        fn terminal_utilities(
            &self,
            state: &Self::State,
            _world: &SampledWorld,
            utilities: &mut [f64],
        ) {
            let ToyState::Terminal(action) = *state else {
                panic!("not terminal")
            };
            utilities[0] = f64::from(action == 0) * 2.0 - 1.0;
            utilities[1] = -utilities[0];
        }

        fn recall_mode(&self) -> RecallMode {
            RecallMode::Street
        }

        fn bucket_count(&self, _street: Street, _active_opponents: u8) -> u32 {
            1
        }

        fn dense_node_context(&self, _state: &Self::State) -> DenseNodeContext {
            DenseNodeContext {
                street: Street::Preflop,
                active_opponents: 1,
                bucket_active_opponents: 1,
            }
        }
    }

    /// Two-decision-node game (player 0 then player 1, each choosing between
    /// two labeled actions) with a real, world-dependent two-bucket
    /// abstraction, used for the dense-arena tests that need more than one
    /// tree node and more than one touched bucket per node.
    #[derive(Clone, Copy)]
    struct DenseToyGame;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum DenseToyState {
        First,
        Second { first_action: usize },
        Terminal { first_action: usize },
    }

    impl ExternalSamplingGame for DenseToyGame {
        type State = DenseToyState;
        type Actions = DenseToyState;

        fn num_players(&self) -> usize {
            2
        }

        fn root_state(&self) -> Self::State {
            DenseToyState::First
        }

        fn actor(&self, state: &Self::State) -> Option<usize> {
            match state {
                DenseToyState::First => Some(0),
                DenseToyState::Second { .. } => Some(1),
                DenseToyState::Terminal { .. } => None,
            }
        }

        fn node_actions(&self, state: &Self::State) -> Self::Actions {
            *state
        }

        fn num_actions_of(&self, actions: &Self::Actions) -> usize {
            usize::from(!matches!(actions, DenseToyState::Terminal { .. })) * 2
        }

        fn next_state_with(
            &self,
            _state: &Self::State,
            actions: &Self::Actions,
            action_index: usize,
        ) -> Self::State {
            match *actions {
                DenseToyState::First => DenseToyState::Second {
                    first_action: action_index,
                },
                DenseToyState::Second { first_action } => DenseToyState::Terminal { first_action },
                DenseToyState::Terminal { .. } => panic!("terminal state has no child"),
            }
        }

        fn write_action_label(
            &self,
            actions: &Self::Actions,
            action_index: usize,
            out: &mut String,
        ) {
            let labels = match actions {
                DenseToyState::First => ["best", "dominated"],
                DenseToyState::Second { .. } => ["call", "fold"],
                DenseToyState::Terminal { .. } => panic!("terminal state has no actions"),
            };
            out.push_str(labels[action_index]);
        }

        fn bucket(&self, _state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo {
            let bucket = (world.hole_combo(actor) % 2) as u32;
            PrivateInfo::from_current_bucket(Street::Preflop, 1, bucket)
        }

        fn terminal_utilities(
            &self,
            state: &Self::State,
            _world: &SampledWorld,
            utilities: &mut [f64],
        ) {
            let DenseToyState::Terminal { first_action } = *state else {
                panic!("not terminal")
            };
            // Action 0 ("best") is dominant for player 0 regardless of
            // player 1's action or either player's bucket.
            utilities[0] = f64::from(first_action == 0) * 2.0 - 1.0;
            utilities[1] = -utilities[0];
        }

        fn recall_mode(&self) -> RecallMode {
            RecallMode::Street
        }

        fn bucket_count(&self, _street: Street, _active_opponents: u8) -> u32 {
            2
        }

        fn dense_node_context(&self, _state: &Self::State) -> DenseNodeContext {
            DenseNodeContext {
                street: Street::Preflop,
                active_opponents: 1,
                bucket_active_opponents: 1,
            }
        }

        fn bucket_for_combo(
            &self,
            _state: &Self::State,
            _world: &SampledWorld,
            _actor: usize,
            combo: usize,
        ) -> BucketId {
            (combo % 2) as u32
        }

        fn terminal_utilities_for_combos(
            &self,
            state: &Self::State,
            world: &SampledWorld,
            traverser: usize,
            combos: &[usize],
            out: &mut Vec<f64>,
        ) {
            // Card-independent terminal, same as `terminal_utilities`: every
            // combo gets the same constant.
            let mut utilities = vec![0.0; 2];
            self.terminal_utilities(state, world, &mut utilities);
            out.clear();
            out.resize(combos.len(), utilities[traverser]);
        }
    }

    fn dense_dominated_solver(seed: u64) -> MultiwaySolver<DenseDominatedChoice> {
        MultiwaySolver::new(
            DenseDominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed,
                max_memory_bytes: 1 << 20,
                max_traversal_depth: 16,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: 5,
                discount_until: 100,
                sweep_batch: 1,
                traverser_vector: false,
            },
        )
        .unwrap()
    }

    fn dense_toy_solver(seed: u64, sweep_batch: u64) -> MultiwaySolver<DenseToyGame> {
        MultiwaySolver::new(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed,
                max_memory_bytes: 1 << 20,
                max_traversal_depth: 16,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch,
                traverser_vector: false,
            },
        )
        .unwrap()
    }

    fn dense_vector_toy_solver(seed: u64, sweep_batch: u64) -> MultiwaySolver<DenseToyGame> {
        MultiwaySolver::new(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed,
                max_memory_bytes: 1 << 20,
                max_traversal_depth: 16,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch,
                traverser_vector: true,
            },
        )
        .unwrap()
    }

    #[test]
    fn vector_traverser_rejects_full_recall_game() {
        let result = MultiwaySolver::new(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                traverser_vector: true,
                ..SolverConfig::default()
            },
        );
        let error = match result {
            Ok(_) => panic!("expected a recall-mode validation error"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            SolverError::VectorTraverserRequiresStreetRecall
        ));
    }

    #[test]
    fn vector_traverser_hand_updates_exceed_one_and_matches_scalar_metrics_shape() {
        let mut vector = dense_vector_toy_solver(77, 1);
        vector.run_sweeps_with_threads(6, 1).unwrap();
        let metrics = vector.metrics();
        // Every traversal's feasible set has close to (but at most) `C(50,
        // 2)` combos (full range minus the ~7 dead cards each world deals);
        // six sweeps of two traversals each is comfortably more than one
        // hand update per traversal.
        assert!(metrics.hand_updates > metrics.traversals);

        let mut scalar = dense_toy_solver(77, 1);
        scalar.run_sweeps_with_threads(6, 1).unwrap();
        // The scalar algorithm updates exactly one hand per traversal.
        assert_eq!(scalar.metrics().hand_updates, scalar.metrics().traversals);
    }

    #[test]
    fn vector_traverser_is_deterministic_across_thread_counts_and_reruns() {
        let mut single_threaded = dense_vector_toy_solver(4104, 1);
        single_threaded.run_sweeps_with_threads(20, 1).unwrap();

        let mut multi_threaded = dense_vector_toy_solver(4104, 1);
        multi_threaded.run_sweeps_with_threads(20, 8).unwrap();

        assert_eq!(
            single_threaded.snapshot_state(),
            multi_threaded.snapshot_state()
        );
        assert_eq!(single_threaded.metrics(), multi_threaded.metrics());

        let mut rerun = dense_vector_toy_solver(4104, 1);
        rerun.run_sweeps_with_threads(20, 4).unwrap();
        assert_eq!(single_threaded.snapshot_state(), rerun.snapshot_state());
    }

    #[test]
    fn vector_traverser_sweep_batch_is_internally_consistent() {
        let mut batch_four_a = dense_vector_toy_solver(606, 4);
        batch_four_a.run_sweeps_with_threads(12, 1).unwrap();
        let mut batch_four_b = dense_vector_toy_solver(606, 4);
        batch_four_b.run_sweeps_with_threads(12, 6).unwrap();
        assert_eq!(batch_four_a.snapshot_state(), batch_four_b.snapshot_state());
    }

    #[test]
    fn resume_rejects_mismatched_traverser_vector() {
        let state = dense_toy_solver(21, 1).snapshot_state();
        let mut changed = state.config;
        changed.traverser_vector = true;
        assert!(matches!(
            MultiwaySolver::from_state_with_config(
                DenseToyGame,
                DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
                state,
                changed,
            ),
            Err(SolverError::ResumeConfigurationMismatch)
        ));
    }

    #[test]
    fn vector_traverser_checkpoint_v5_round_trip_and_resume() {
        let mut solver = dense_vector_toy_solver(303, 1);
        solver.run_sweeps_with_threads(10, 2).unwrap();

        let checkpoint = crate::checkpoint::MultiwayCheckpoint::capture(&solver);
        assert_eq!(
            checkpoint.header.version,
            crate::checkpoint::CHECKPOINT_VERSION
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vector.mwckpt");
        checkpoint.write_atomic(&path).unwrap();
        let loaded = crate::checkpoint::MultiwayCheckpoint::load(
            &path,
            solver.configuration_fingerprint(),
            solver.abstraction_fingerprint(),
        )
        .unwrap();
        assert_eq!(loaded.state, solver.snapshot_state());
        assert!(loaded.state.config.traverser_vector);
        assert!(loaded.state.hand_updates > 0);

        let mut resumed = MultiwaySolver::from_state(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            loaded.state,
        )
        .unwrap();
        resumed.run_sweeps_with_threads(5, 2).unwrap();

        let mut reference = dense_vector_toy_solver(303, 1);
        reference.run_sweeps_with_threads(15, 2).unwrap();
        assert_eq!(resumed.snapshot_state(), reference.snapshot_state());
    }

    #[test]
    fn street_recall_preflight_builds_a_dense_arena_at_construction() {
        let solver = dense_dominated_solver(1);
        let stats = solver
            .dense_arena_stats()
            .expect("dense mode has arena stats");
        assert_eq!(stats.node_count, 1);
        assert_eq!(stats.total_columns, 1);
        assert_eq!(stats.total_slots, 2);
        assert!(stats.estimated_bytes > 0);

        let toy = dense_toy_solver(1, 1);
        let toy_stats = toy.dense_arena_stats().expect("dense mode has arena stats");
        // Root (2 buckets) + two `Second` children (2 buckets each) = 6
        // columns, each with 2 actions = 12 slots.
        assert_eq!(toy_stats.node_count, 3);
        assert_eq!(toy_stats.total_columns, 6);
        assert_eq!(toy_stats.total_slots, 12);
    }

    #[test]
    fn street_recall_preflight_fails_before_allocating_when_memory_limit_is_tiny() {
        let result = MultiwaySolver::new(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed: 1,
                max_memory_bytes: 1,
                max_traversal_depth: 16,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch: 1,
                traverser_vector: false,
            },
        );
        let error = match result {
            Ok(_) => panic!("expected a memory-limit preflight error"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            SolverError::Tree(TreeError::MemoryLimit { .. })
        ));
    }

    #[test]
    fn street_recall_reproduces_the_sparse_dominated_choice_trajectory() {
        // Same single-decision-node shape and seed as `DominatedChoice`'s
        // sparse `dominated_action_disappears_from_current_and_average_policy`
        // test, so this pins the dense traversal/merge math against the
        // sparse path's already-established behavior.
        let mut solver = dense_dominated_solver(1);
        solver.run_sweeps(1).unwrap();
        assert_eq!(
            solver.policy(root_key_street()).unwrap().regrets,
            vec![1.0, -1.0]
        );
        solver.run_sweeps(100).unwrap();
        assert!(solver.current_strategy(root_key_street()).unwrap()[0] > 0.99);
        assert!(solver.average_strategy(root_key_street()).unwrap()[0] > 0.95);
        assert_eq!(
            solver
                .average_action_probabilities(root_key_street())
                .unwrap()[0]
                .action,
            "best"
        );
    }

    fn root_key_street() -> InfoKey {
        InfoKey {
            history: HistoryKey::ROOT,
            player: 0,
            street: 0,
            active_opponents: 1,
            bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        }
    }

    #[test]
    fn street_recall_snapshot_info_keys_carry_unreached_in_noncurrent_slots() {
        let mut solver = dense_toy_solver(3, 1);
        solver.run_sweeps(30).unwrap();
        let snapshot = solver.snapshot_state();
        assert!(!snapshot.policies.is_empty());
        for entry in &snapshot.policies {
            let street = entry.key.street as usize;
            for (index, &bucket) in entry.key.bucket_path.iter().enumerate() {
                if index == street {
                    assert_ne!(bucket, UNREACHED_BUCKET);
                } else {
                    assert_eq!(bucket, UNREACHED_BUCKET);
                }
            }
        }
        // Every policy's history must resolve to an ancestor already present
        // in the pruned (ancestors-of-touched) history trie.
        for entry in &snapshot.policies {
            let mut key = entry.key.history;
            while key != HistoryKey::ROOT {
                let found = snapshot.histories.iter().find(|history| history.key == key);
                let Some(found) = found else {
                    panic!("missing ancestor history entry for a touched policy")
                };
                key = found.parent;
            }
        }
    }

    #[test]
    fn street_recall_is_deterministic_across_thread_counts() {
        let mut single_threaded = dense_toy_solver(2024, 1);
        single_threaded.run_sweeps_with_threads(48, 1).unwrap();

        let mut multi_threaded = dense_toy_solver(2024, 1);
        multi_threaded.run_sweeps_with_threads(48, 8).unwrap();

        assert_eq!(
            single_threaded.snapshot_state(),
            multi_threaded.snapshot_state()
        );
        assert_eq!(single_threaded.metrics(), multi_threaded.metrics());
    }

    #[test]
    fn street_recall_sweep_batch_one_and_four_are_each_internally_consistent() {
        // `sweep_batch` legitimately trades staleness for parallel
        // efficiency (see `run_sweeps_with_threads_until`'s docs), so a
        // batch-of-4 run is not expected to bit-match a batch-of-1 run; each
        // is checked for thread-count invariance against *itself* instead.
        let mut batch_one_a = dense_toy_solver(909, 1);
        batch_one_a.run_sweeps_with_threads(24, 1).unwrap();
        let mut batch_one_b = dense_toy_solver(909, 1);
        batch_one_b.run_sweeps_with_threads(24, 6).unwrap();
        assert_eq!(batch_one_a.snapshot_state(), batch_one_b.snapshot_state());

        let mut batch_four_a = dense_toy_solver(909, 4);
        batch_four_a.run_sweeps_with_threads(24, 1).unwrap();
        let mut batch_four_b = dense_toy_solver(909, 4);
        batch_four_b.run_sweeps_with_threads(24, 6).unwrap();
        assert_eq!(batch_four_a.snapshot_state(), batch_four_b.snapshot_state());
    }

    #[test]
    fn street_recall_checkpoint_round_trip_and_resume() {
        let mut solver = dense_toy_solver(55, 1);
        solver.run_sweeps_with_threads(30, 2).unwrap();

        let checkpoint = crate::checkpoint::MultiwayCheckpoint::capture(&solver);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dense.mwckpt");
        checkpoint.write_atomic(&path).unwrap();
        let loaded = crate::checkpoint::MultiwayCheckpoint::load(
            &path,
            solver.configuration_fingerprint(),
            solver.abstraction_fingerprint(),
        )
        .unwrap();
        assert_eq!(loaded.state, solver.snapshot_state());

        let mut resumed = MultiwaySolver::from_state(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            loaded.state,
        )
        .unwrap();
        assert_eq!(resumed.snapshot_state(), solver.snapshot_state());
        resumed.run_sweeps_with_threads(10, 2).unwrap();

        let mut reference = dense_toy_solver(55, 1);
        reference.run_sweeps_with_threads(40, 2).unwrap();
        assert_eq!(resumed.snapshot_state(), reference.snapshot_state());
        assert_eq!(resumed.metrics(), reference.metrics());
    }

    #[test]
    fn street_recall_discount_fires_and_matches_sparse_arithmetic() {
        // Same seed/game/config shape as `batched_discount_scales_regret_and_strategy_at_cadence`.
        let mut solver = dense_dominated_solver(1);
        solver.config.discount_every = 2;
        solver.config.discount_until = 5;
        solver.run_sweeps(1).unwrap();
        assert_eq!(
            solver.policy(root_key_street()).unwrap().regrets,
            vec![1.0, -1.0]
        );
        solver.run_sweeps(1).unwrap();
        let column = solver.policy(root_key_street()).unwrap();
        assert_eq!(column.regrets, vec![0.5, -1.5]);
        assert_eq!(column.strategy_sum, vec![1.25, 0.25]);
    }

    #[test]
    fn street_recall_regret_trends_down_as_sweeps_accumulate() {
        let mut solver = dense_toy_solver(31, 1);
        solver.run_sweeps(20).unwrap();
        let early = solver.metrics().average_positive_regret;
        let early_mean = early.iter().sum::<f64>() / early.len() as f64;

        solver.run_sweeps(2_000).unwrap();
        let late = solver.metrics().average_positive_regret;
        let late_mean = late.iter().sum::<f64>() / late.len() as f64;

        assert!(early_mean.is_finite() && early_mean >= 0.0);
        assert!(late_mean.is_finite() && late_mean >= 0.0);
        assert!(
            late_mean < early_mean,
            "expected average positive regret to trend down: early {early_mean}, late {late_mean}"
        );
    }

    #[test]
    fn street_recall_node_children_and_strategies_at_use_the_enumerated_tree() {
        let mut solver = dense_toy_solver(6, 1);
        solver.run_sweeps(50).unwrap();

        let children = solver.node_children(HistoryKey::ROOT);
        assert_eq!(children.len(), 2);
        let mut labels: Vec<&str> = children
            .iter()
            .map(|entry| entry.action_label.as_str())
            .collect();
        labels.sort_unstable();
        assert_eq!(labels, vec!["best", "dominated"]);

        for child in &children {
            let resolved = solver
                .history_entry(child.key)
                .expect("child is enumerated");
            assert_eq!(resolved, *child);
        }

        let rows = solver.strategies_at(HistoryKey::ROOT);
        assert!(!rows.is_empty());
        for (_, action_labels, probabilities) in &rows {
            assert_eq!(action_labels.len(), probabilities.len());
            let sum: f32 = probabilities.iter().sum();
            assert!((sum - 1.0).abs() < 1e-3, "probabilities summed to {sum}");
        }
    }

    /// Every toy dense game above only ever reaches `Street::Preflop`
    /// (street index `0`), so `validate_private_info`'s street-recall branch
    /// (only the *current* street's slot is non-sentinel, including when
    /// that street is not `0`) was never exercised by them: an earlier draft
    /// of the `RecallMode::Street` branch there reused the full-recall
    /// invariant unconditionally and panicked/errored the instant a real,
    /// multi-street game reached the flop. This drives a real
    /// [`crate::holdem::HoldemGame`] far enough to guarantee flop/turn/river
    /// nodes are visited, as a regression guard.
    #[test]
    fn street_recall_holdem_game_reaches_every_street_without_error() {
        use crate::abstraction::FeatureHashAbstraction;
        use crate::config::{
            AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
            SeatConfig, UtilityConfig,
        };
        use crate::holdem::HoldemGame;
        use crate::types::SeatId;

        let mut config = MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    // Shallow stacks so a meaningful fraction of sampled
                    // hands actually reach the turn/river instead of
                    // resolving preflop.
                    stack_bb: 6.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            abstraction: AbstractionConfig::default(),
        };
        config.abstraction.flop_buckets = 4;
        config.abstraction.turn_buckets = 4;
        config.abstraction.river_buckets = 4;
        config.abstraction.recall = RecallMode::Street;

        let game = HoldemGame::new(
            &config,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
                flop_buckets: 4,
                turn_buckets: 4,
                river_buckets: 4,
            })
            .unwrap(),
        )
        .unwrap();
        let sampler = game.deal_sampler().unwrap();
        let mut solver = MultiwaySolver::new(
            game,
            sampler,
            SolverConfig {
                seed: 4,
                max_memory_bytes: 1 << 24,
                max_traversal_depth: 64,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch: 1,
                traverser_vector: false,
            },
        )
        .unwrap();
        solver.run_sweeps_with_threads(200, 4).unwrap();

        let snapshot = solver.snapshot_state();
        assert!(!snapshot.policies.is_empty());
        let mut streets_seen = [false; 4];
        for entry in &snapshot.policies {
            streets_seen[entry.key.street as usize] = true;
            let street = entry.key.street as usize;
            for (index, &bucket) in entry.key.bucket_path.iter().enumerate() {
                if index == street {
                    assert_ne!(bucket, UNREACHED_BUCKET);
                } else {
                    assert_eq!(bucket, UNREACHED_BUCKET);
                }
            }
        }
        assert!(
            streets_seen.iter().all(|&seen| seen),
            "expected every street to be reached at least once: {streets_seen:?}"
        );
    }

    #[test]
    fn vector_traverser_regret_trends_down_as_sweeps_accumulate() {
        let mut solver = dense_vector_toy_solver(31, 1);
        solver.run_sweeps(20).unwrap();
        let early = solver.metrics().average_positive_regret;
        let early_mean = early.iter().sum::<f64>() / early.len() as f64;

        solver.run_sweeps(2_000).unwrap();
        let late = solver.metrics().average_positive_regret;
        let late_mean = late.iter().sum::<f64>() / late.len() as f64;

        assert!(early_mean.is_finite() && early_mean >= 0.0);
        assert!(late_mean.is_finite() && late_mean >= 0.0);
        assert!(
            late_mean < early_mean,
            "expected average positive regret to trend down: early {early_mean}, late {late_mean}"
        );
    }

    /// Vector mode's average strategy must accumulate densely at a
    /// traverser node on the very first sweep it is visited, not just on
    /// whichever single hand an opponent traversal happened to sample.
    ///
    /// Before this change, the vector opponent branch pushed `AddStrategy`
    /// only for the acting seat's one dealt-hand bucket (mirroring scalar),
    /// and the traverser branch pushed none at all. With three seats, the
    /// root is visited as "opponent" only on the two sweeps' traversals
    /// where a *different* seat is the traverser, each adding mass for
    /// exactly one sampled class -- so two sweeps (six traversals) could
    /// give the root at most a handful of classes with positive
    /// `strategy_sum` under the old behavior. All 169 preflop classes are
    /// feasible at the root under full ranges, and this change makes every
    /// traverser-seat sweep touch every feasible one of them at once, so
    /// after just two sweeps nearly all 169 should already show mass.
    #[test]
    fn vector_traverser_root_strategy_sum_is_dense_after_few_sweeps() {
        use crate::abstraction::FeatureHashAbstraction;
        use crate::config::{
            AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
            SeatConfig, UtilityConfig,
        };
        use crate::holdem::HoldemGame;
        use crate::types::SeatId;

        let mut config = MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 6.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            abstraction: AbstractionConfig::default(),
        };
        config.abstraction.flop_buckets = 2;
        config.abstraction.turn_buckets = 2;
        config.abstraction.river_buckets = 2;
        config.abstraction.recall = RecallMode::Street;

        let game = HoldemGame::new(
            &config,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
                flop_buckets: 2,
                turn_buckets: 2,
                river_buckets: 2,
            })
            .unwrap(),
        )
        .unwrap();
        let sampler = game.deal_sampler().unwrap();
        let mut solver = MultiwaySolver::new(
            game,
            sampler,
            SolverConfig {
                seed: 55,
                max_memory_bytes: 1 << 24,
                max_traversal_depth: 64,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch: 1,
                traverser_vector: true,
            },
        )
        .unwrap();
        solver.run_sweeps(2).unwrap();

        let snapshot = solver.snapshot_state();
        let root_buckets_with_mass = snapshot
            .policies
            .iter()
            .filter(|entry| entry.key.history == HistoryKey::ROOT)
            .filter(|entry| entry.column.strategy_sum.iter().any(|&value| value > 0.0))
            .count();
        assert!(
            root_buckets_with_mass >= 100,
            "expected dense root strategy_sum coverage close to all 169 classes, found {root_buckets_with_mass}"
        );
    }

    /// Same shape as [`street_recall_holdem_game_reaches_every_street_without_error`]
    /// but with `traverser_vector` enabled: a real, multi-street `HoldemGame`
    /// still reaches every street, produces only finite regrets/metrics, and
    /// `hand_updates` accumulates faster than `traversals` (the whole point
    /// of the mode).
    #[test]
    fn vector_traverser_holdem_game_reaches_every_street_without_error() {
        use crate::abstraction::FeatureHashAbstraction;
        use crate::config::{
            AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
            SeatConfig, UtilityConfig,
        };
        use crate::holdem::HoldemGame;
        use crate::types::SeatId;

        let mut config = MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 6.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            abstraction: AbstractionConfig::default(),
        };
        config.abstraction.flop_buckets = 4;
        config.abstraction.turn_buckets = 4;
        config.abstraction.river_buckets = 4;
        config.abstraction.recall = RecallMode::Street;

        let game = HoldemGame::new(
            &config,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
                flop_buckets: 4,
                turn_buckets: 4,
                river_buckets: 4,
            })
            .unwrap(),
        )
        .unwrap();
        let sampler = game.deal_sampler().unwrap();
        let mut solver = MultiwaySolver::new(
            game,
            sampler,
            SolverConfig {
                seed: 4,
                max_memory_bytes: 1 << 24,
                max_traversal_depth: 64,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch: 1,
                traverser_vector: true,
            },
        )
        .unwrap();
        solver.run_sweeps_with_threads(60, 4).unwrap();

        let snapshot = solver.snapshot_state();
        assert!(!snapshot.policies.is_empty());
        let mut streets_seen = [false; 4];
        for entry in &snapshot.policies {
            streets_seen[entry.key.street as usize] = true;
            let street = entry.key.street as usize;
            for (index, &bucket) in entry.key.bucket_path.iter().enumerate() {
                if index == street {
                    assert_ne!(bucket, UNREACHED_BUCKET);
                } else {
                    assert_eq!(bucket, UNREACHED_BUCKET);
                }
            }
        }
        assert!(
            streets_seen.iter().all(|&seen| seen),
            "expected every street to be reached at least once: {streets_seen:?}"
        );
        let metrics = solver.metrics();
        assert!(metrics.hand_updates > metrics.traversals);
        for &regret in &metrics.average_positive_regret {
            assert!(regret.is_finite());
        }
    }

    /// ICM smoke test: a small vector-traverser + `RecallMode::Street` +
    /// `TournamentIcm` solve runs without error and produces finite
    /// utilities/regrets. Exercises the terminal ICM path (which is exact
    /// for this seat count) inside `terminal_utilities_for_combos`, called
    /// up to hundreds of times per traversal instead of once.
    #[test]
    fn vector_traverser_icm_smoke_test() {
        use crate::abstraction::FeatureHashAbstraction;
        use crate::config::{
            AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, FieldPlayerConfig,
            MultiwayConfig, RakeConfig, SeatConfig, UtilityConfig,
        };
        use crate::holdem::HoldemGame;
        use crate::types::SeatId;

        let mut config = MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 8.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            abstraction: AbstractionConfig::default(),
        };
        config.abstraction.flop_buckets = 3;
        config.abstraction.turn_buckets = 3;
        config.abstraction.river_buckets = 3;
        config.abstraction.recall = RecallMode::Street;

        let utility = UtilityConfig::TournamentIcm {
            outside_field: vec![FieldPlayerConfig {
                name: "field".into(),
                stack_bb: 40.0,
            }],
            payouts: vec![100.0, 60.0, 30.0, 0.0],
            samples: 4_000,
            seed: 11,
        };

        let game = HoldemGame::new(
            &config,
            &utility,
            &RakeConfig::None,
            FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
                flop_buckets: 3,
                turn_buckets: 3,
                river_buckets: 3,
            })
            .unwrap(),
        )
        .unwrap();
        let sampler = game.deal_sampler().unwrap();
        let mut solver = MultiwaySolver::new(
            game,
            sampler,
            SolverConfig {
                seed: 9,
                max_memory_bytes: 1 << 24,
                max_traversal_depth: 64,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch: 1,
                traverser_vector: true,
            },
        )
        .unwrap();
        solver.run_sweeps_with_threads(15, 2).unwrap();

        let metrics = solver.metrics();
        assert!(metrics.hand_updates >= metrics.traversals);
        for &regret in &metrics.average_positive_regret {
            assert!(regret.is_finite());
        }
        let evaluation = solver.evaluate_average_profile(64, 5).unwrap();
        for seat in &evaluation.seats {
            assert!(seat.mean.is_finite());
        }
    }
}
