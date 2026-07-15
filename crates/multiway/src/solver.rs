//! Sparse, lazy external-sampling MCCFR for sampled multiway games.
//!
//! This module intentionally does not call its diagnostics exploitability or
//! Nash convergence: with more than two players the game is general-sum, and
//! bucket abstraction may also introduce imperfect recall.  The solver
//! exposes sampled regret diagnostics and average policies without claiming
//! a two-player zero-sum guarantee.

use std::collections::HashMap;
use std::mem::size_of;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};

use crate::abstraction::{BucketId, BucketPath};
use crate::sampler::{DealSampler, SampleError, SampledWorld};
use crate::types::{MAX_SEATS, MIN_SEATS, Street};

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

    fn num_players(&self) -> usize;
    fn root_state(&self) -> Self::State;

    /// Acting seat, or `None` iff the state is terminal.
    fn actor(&self, state: &Self::State) -> Option<usize>;
    fn num_actions(&self, state: &Self::State) -> usize;
    fn next_state(&self, state: &Self::State, action_index: usize) -> Self::State;
    /// Stable, user-facing label corresponding to `action_index`. Labels at
    /// an information set must be non-empty and unique.
    fn action_label(&self, state: &Self::State, action_index: usize) -> String;

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
    /// Sum of positive cumulative regrets divided by completed updates for
    /// each seat.  This is a sampled diagnostic, not exploitability.
    pub average_positive_regret: Vec<f64>,
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

/// Sparse external-sampling MCCFR state.  A policy column exists only after
/// `(public history, player, bucket)` is visited by a sampled traversal.
pub struct MultiwaySolver<G: ExternalSamplingGame> {
    game: G,
    sampler: DealSampler,
    config: SolverConfig,
    policies: HashMap<InfoKey, PolicyColumn>,
    histories: HashMap<HistoryKey, HistoryEntry>,
    approx_memory_bytes: u64,
    traversals: u64,
    completed_sweeps: u64,
    next_sample_id: u64,
    total_deal_attempts: u64,
    terminal_evaluations: u64,
}

impl<G: ExternalSamplingGame> MultiwaySolver<G> {
    pub fn new(game: G, sampler: DealSampler, config: SolverConfig) -> Result<Self, SolverError> {
        validate_setup(&game, &sampler, config)?;
        Ok(Self {
            game,
            sampler,
            config,
            policies: HashMap::new(),
            histories: HashMap::new(),
            approx_memory_bytes: 0,
            traversals: 0,
            completed_sweeps: 0,
            next_sample_id: 0,
            total_deal_attempts: 0,
            terminal_evaluations: 0,
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
        validate_setup(&game, &sampler, state.config)?;
        if state.schema_version != SOLVER_STATE_VERSION {
            return Err(SolverError::StateVersion {
                found: state.schema_version,
                expected: SOLVER_STATE_VERSION,
            });
        }
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

        let mut histories = HashMap::with_capacity(state.histories.len());
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

        let mut policies = HashMap::with_capacity(state.policies.len());
        for entry in state.policies {
            validate_column(entry.key, &entry.column, game.num_players())?;
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
        hasher.update(b"solvers.multiway.solver-config.v1");
        hasher.update(&self.config.seed.to_le_bytes());
        hasher.update(&self.config.max_memory_bytes.to_le_bytes());
        hasher.update(&self.config.max_traversal_depth.to_le_bytes());
        hasher.update(&self.config.exploration_epsilon.to_bits().to_le_bytes());
        hasher.update(&self.config.discount_every.to_le_bytes());
        hasher.update(&self.config.discount_until.to_le_bytes());
        hasher.update(&self.sampler.range_fingerprint());
        hasher.update(&self.game.game_fingerprint());
        *hasher.finalize().as_bytes()
    }

    pub fn abstraction_fingerprint(&self) -> [u8; 32] {
        self.game.abstraction_fingerprint()
    }

    pub fn run_sweeps(&mut self, sweeps: u64) -> Result<(), SolverError> {
        let traversals = sweeps
            .checked_mul(self.game.num_players() as u64)
            .ok_or(SolverError::TraversalCountOverflow)?;
        self.run_traversals(traversals)
    }

    /// Runs a resumable number of individual player traversals.  Traversers
    /// rotate in seat order; one complete rotation is a sweep.
    pub fn run_traversals(&mut self, count: u64) -> Result<(), SolverError> {
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
        self.policies.get(&key).map(PolicyColumn::current_strategy)
    }

    pub fn average_strategy(&self, key: InfoKey) -> Option<Vec<f32>> {
        self.policies.get(&key).map(PolicyColumn::average_strategy)
    }

    pub fn current_action_probabilities(&self, key: InfoKey) -> Option<Vec<ActionProbability>> {
        self.policies
            .get(&key)
            .map(PolicyColumn::current_action_probabilities)
    }

    pub fn average_action_probabilities(&self, key: InfoKey) -> Option<Vec<ActionProbability>> {
        self.policies
            .get(&key)
            .map(PolicyColumn::average_action_probabilities)
    }

    pub fn policy(&self, key: InfoKey) -> Option<&PolicyColumn> {
        self.policies.get(&key)
    }

    pub fn history_entry(&self, key: HistoryKey) -> Option<&HistoryEntry> {
        self.histories.get(&key)
    }

    /// Resolves a public-history hash to its root-to-node action-label path.
    pub fn resolve_history(&self, mut key: HistoryKey) -> Option<Vec<String>> {
        let mut reversed = Vec::new();
        while key != HistoryKey::ROOT {
            let entry = self.histories.get(&key)?;
            reversed.push(entry.action_label.clone());
            key = entry.parent;
        }
        reversed.reverse();
        Some(reversed)
    }

    pub fn snapshot_state(&self) -> SolverState {
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
        SolverState {
            schema_version: SOLVER_STATE_VERSION,
            config: self.config,
            traversals: self.traversals,
            completed_sweeps: self.completed_sweeps,
            next_sample_id: self.next_sample_id,
            total_deal_attempts: self.total_deal_attempts,
            terminal_evaluations: self.terminal_evaluations,
            histories,
            policies,
        }
    }

    pub fn metrics(&self) -> SolverMetrics {
        let num_players = self.game.num_players();
        let mut positive_regret = vec![0.0; num_players];
        for (key, column) in &self.policies {
            positive_regret[key.player as usize] += column
                .regrets
                .iter()
                .map(|&regret| f64::from(regret.max(0.0)))
                .sum::<f64>();
        }
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
            infosets: self.policies.len() as u64,
            memory_bytes: self.approx_memory_bytes,
            total_deal_attempts: self.total_deal_attempts,
            mean_deal_attempts: if self.traversals == 0 {
                0.0
            } else {
                self.total_deal_attempts as f64 / self.traversals as f64
            },
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
            let num_actions = self.game.num_actions(&state);
            if num_actions == 0 {
                return Err(SolverError::NoActions { actor });
            }
            let private = self.game.bucket(&state, world, actor);
            validate_private_info(private, num_players)?;
            let key = InfoKey {
                history,
                player: actor as u8,
                street: private.street,
                active_opponents: private.active_opponents,
                bucket_path: private.bucket_path,
            };
            let labels = (0..num_actions)
                .map(|action| self.game.action_label(&state, action))
                .collect::<Vec<_>>();
            validate_action_labels(&labels)?;
            let stored = self.policies.get(&key);
            let strategy = if let Some(column) = stored {
                if column.action_labels != labels {
                    return Err(SolverError::ActionLabelsChanged { key });
                }
                column.average_strategy()
            } else {
                vec![1.0 / num_actions as f32; num_actions]
            };
            let action = if deviator == Some(actor) {
                stored.map_or_else(
                    || sample_profile_action(&strategy, rng),
                    |column| regret_greedy_action(&column.regrets),
                )
            } else {
                sample_profile_action(&strategy, rng)
            };
            state = self.game.next_state(&state, action);
            history = history.child(actor, action);
            if depth == self.config.max_traversal_depth {
                return Err(SolverError::DepthLimit {
                    limit: self.config.max_traversal_depth,
                });
            }
        }
        unreachable!("depth loop returns at its upper bound")
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
        let num_actions = self.game.num_actions(&state);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        let private = self.game.bucket(&state, world, actor);
        validate_private_info(private, num_players)?;
        let key = InfoKey {
            history,
            player: actor as u8,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        };
        let action_labels = (0..num_actions)
            .map(|action| self.game.action_label(&state, action))
            .collect::<Vec<_>>();
        validate_action_labels(&action_labels)?;
        let strategy = self.strategy_for(key, action_labels.clone())?;

        if actor == traverser {
            let mut action_values = vec![0.0; num_actions];
            for (action, value) in action_values.iter_mut().enumerate() {
                let old_reach = reach[actor];
                reach[actor] *= strategy[action];
                let child_history =
                    self.record_history(history, actor, action, &action_labels[action])?;
                let next = self.game.next_state(&state, action);
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
            let child_history =
                self.record_history(history, actor, action, &action_labels[action])?;
            let next = self.game.next_state(&state, action);
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

    fn strategy_for(
        &mut self,
        key: InfoKey,
        action_labels: Vec<String>,
    ) -> Result<Vec<f64>, SolverError> {
        let num_actions = action_labels.len();
        if let Some(column) = self.policies.get(&key) {
            if column.num_actions() != num_actions {
                return Err(SolverError::ActionCountChanged {
                    key,
                    stored: column.num_actions(),
                    current: num_actions,
                });
            }
            if column.action_labels != action_labels {
                return Err(SolverError::ActionLabelsChanged { key });
            }
            return Ok(regret_matching(&column.regrets));
        }

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
        let mut keys: Vec<_> = self.policies.keys().copied().collect();
        keys.sort_unstable();
        for key in keys {
            let column = self.policies.get_mut(&key).expect("key came from map");
            for value in column.regrets.iter_mut().chain(&mut column.strategy_sum) {
                *value *= factor;
            }
        }
    }
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
    Ok(())
}

fn validate_private_info(info: PrivateInfo, num_players: usize) -> Result<(), SolverError> {
    if info.street > 3 {
        return Err(SolverError::InvalidPrivateInfo("street is outside 0..=3"));
    }
    if info.active_opponents == 0 || info.active_opponents as usize >= num_players {
        return Err(SolverError::InvalidPrivateInfo(
            "active opponents is outside 1..players",
        ));
    }
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
    Ok(())
}

fn validate_column(
    key: InfoKey,
    column: &PolicyColumn,
    num_players: usize,
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
    histories: &HashMap<HistoryKey, HistoryEntry>,
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

    impl ExternalSamplingGame for PrefixImportanceGame {
        type State = PrefixState;

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

        fn num_actions(&self, state: &Self::State) -> usize {
            usize::from(!matches!(state, PrefixState::Terminal(_))) * 2
        }

        fn next_state(&self, state: &Self::State, action_index: usize) -> Self::State {
            match state {
                PrefixState::Opponent => PrefixState::Hero,
                PrefixState::Hero => PrefixState::Terminal(action_index),
                PrefixState::Terminal(_) => panic!("terminal state has no child"),
            }
        }

        fn action_label(&self, state: &Self::State, action_index: usize) -> String {
            let labels = match state {
                PrefixState::Opponent => ["left", "right"],
                PrefixState::Hero => ["win", "pass"],
                PrefixState::Terminal(_) => panic!("terminal state has no actions"),
            };
            labels[action_index].to_string()
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

    impl ExternalSamplingGame for DominatedChoice {
        type State = ToyState;

        fn num_players(&self) -> usize {
            2
        }

        fn root_state(&self) -> Self::State {
            ToyState::Choose
        }

        fn actor(&self, state: &Self::State) -> Option<usize> {
            matches!(state, ToyState::Choose).then_some(0)
        }

        fn num_actions(&self, state: &Self::State) -> usize {
            usize::from(matches!(state, ToyState::Choose)) * 2
        }

        fn next_state(&self, state: &Self::State, action_index: usize) -> Self::State {
            assert_eq!(*state, ToyState::Choose);
            ToyState::Terminal(action_index)
        }

        fn action_label(&self, _state: &Self::State, action_index: usize) -> String {
            match action_index {
                0 => "best".to_string(),
                1 => "dominated".to_string(),
                _ => panic!("action out of range"),
            }
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
        assert_eq!(column.strategy_sum, vec![1.5, 0.0]);
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
    fn deterministic_substreams_make_fresh_and_resumed_runs_identical() {
        let mut uninterrupted = solver(777, 1 << 20);
        uninterrupted.run_sweeps(40).unwrap();

        let mut first_half = solver(777, 1 << 20);
        first_half.run_sweeps(17).unwrap();
        let state = first_half.snapshot_state();
        let mut resumed = MultiwaySolver::from_state(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state,
        )
        .unwrap();
        resumed.run_sweeps(23).unwrap();

        assert_eq!(uninterrupted.snapshot_state(), resumed.snapshot_state());
        assert_eq!(uninterrupted.metrics(), resumed.metrics());
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
        let mut solver = solver(0, 1);
        assert!(matches!(
            solver.run_traversals(1),
            Err(SolverError::MemoryLimit { limit: 1, .. })
        ));
        assert_eq!(solver.metrics().infosets, 0);
        assert_eq!(solver.metrics().traversals, 0);
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
            .strategy_for(opponent_key, vec!["left".to_string(), "right".to_string()])
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
}
