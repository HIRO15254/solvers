use std::sync::Arc;

use super::support::*;
use super::*;

#[derive(Clone, Debug)]
pub(super) enum TraversalEvent {
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
pub(super) struct TraversalDelta {
    pub(super) sample_id: u64,
    pub(super) traverser: usize,
    pub(super) deal_attempts: u64,
    pub(super) terminal_evaluations: u64,
    pub(super) hand_updates: u64,
    pub(super) events: Vec<TraversalEvent>,
}

/// Dense-mode traversal event: an add into one arena column's regret or
/// strategy-sum slots, addressed by the wire-format `column_id` from
/// [`DenseArena::column_id`].
#[derive(Clone, Debug)]
pub(super) enum DenseEvent {
    AddRegret { column: u32, values: Vec<f64> },
    AddStrategy { column: u32, values: Vec<f64> },
}

#[derive(Clone, Debug)]
pub(super) struct DenseTraversalDelta {
    pub(super) sample_id: u64,
    pub(super) traverser: usize,
    pub(super) deal_attempts: u64,
    pub(super) terminal_evaluations: u64,
    pub(super) hand_updates: u64,
    pub(super) events: Vec<DenseEvent>,
}

/// One traversal's delta, produced by whichever worker
/// [`MultiwaySolver::generate_traversal_delta`] dispatched to. Every delta in
/// a solver's lifetime carries the same variant (decided once at
/// construction by [`ExternalSamplingGame::recall_mode`]); the mismatched
/// case in [`MultiwaySolver::merge_sweep`] is defensive only.
pub(super) enum AnyTraversalDelta {
    Sparse(TraversalDelta),
    Dense(DenseTraversalDelta),
}

pub(super) struct TraversalWorker<'a, G: ExternalSamplingGame> {
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
pub(super) struct DenseStorage {
    pub(super) tree: PublicTree,
    pub(super) arena: DenseArena,
}

/// Borrowed view of one dense-arena column, mirroring [`PolicyColumn`]'s
/// fields without cloning until a caller actually needs an owned copy.
pub(super) struct DenseColumnView<'a> {
    pub(super) action_labels: &'a [String],
    pub(super) regrets: &'a [f32],
    pub(super) strategy_sum: &'a [f32],
}

impl DenseStorage {
    pub(super) fn build<G: ExternalSamplingGame>(
        game: &G,
        max_memory_bytes: u64,
        max_traversal_depth: u32,
        commit_pages: bool,
    ) -> Result<Self, SolverError> {
        tree::preflight_arena_with_limits_and_depth(
            game,
            tree::MAX_TREE_NODES,
            max_memory_bytes,
            max_traversal_depth,
        )?;
        let tree =
            tree::enumerate_tree_with_limits(game, tree::MAX_TREE_NODES, max_traversal_depth)?;
        let mut arena = tree::build_arena(game, &tree, max_memory_bytes)?;
        if commit_pages {
            arena.commit_pages();
        }
        Ok(Self { tree, arena })
    }

    /// Resolves `key` to its `(node, bucket)` dense-arena slot, independent
    /// of whether that column has been touched yet. `None` iff `key` cannot
    /// possibly correspond to any node in the enumerated tree.
    pub(super) fn target(&self, key: InfoKey) -> Option<(NodeId, BucketId)> {
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
    pub(super) fn column_view(&self, key: InfoKey) -> Option<DenseColumnView<'_>> {
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

    pub(super) fn info_key_for(&self, node_id: NodeId, bucket: BucketId) -> InfoKey {
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
pub(super) fn for_each_touched_column<'a>(
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

pub(super) fn label_probabilities(
    labels: &[String],
    probabilities: Vec<f32>,
) -> Vec<ActionProbability> {
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

impl<'a, G: ExternalSamplingGame> TraversalWorker<'a, G> {
    pub(super) fn new(solver: &'a MultiwaySolver<G>, linear_weight: f64) -> Self {
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

    pub(super) fn finish(
        self,
        sample_id: u64,
        traverser: usize,
        deal_attempts: u64,
    ) -> TraversalDelta {
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
    pub(super) fn traverse(
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
pub(super) struct DenseTraversalWorker<'a, G: ExternalSamplingGame> {
    game: &'a G,
    tree: &'a PublicTree,
    arena: &'a DenseArena,
    config: SolverConfig,
    linear_weight: f64,
    events: Vec<DenseEvent>,
    terminal_evaluations: u64,
}

impl<'a, G: ExternalSamplingGame> DenseTraversalWorker<'a, G> {
    pub(super) fn new(
        game: &'a G,
        dense: &'a DenseStorage,
        config: SolverConfig,
        linear_weight: f64,
    ) -> Self {
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

    pub(super) fn finish(
        self,
        sample_id: u64,
        traverser: usize,
        deal_attempts: u64,
    ) -> DenseTraversalDelta {
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
    pub(super) fn traverse(
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
struct BucketPolicy {
    strategy: Vec<f64>,
    prunable_actions: u64,
}

struct BucketUpdate {
    weight_sum: f64,
    reach_weight_sum: f64,
    action_diffs: Vec<f64>,
}
pub(super) struct VectorTraversalWorker<'a, G: ExternalSamplingGame> {
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
    bucket_cache: [Option<Arc<[BucketId]>>; 4],
}

impl<'a, G: ExternalSamplingGame> VectorTraversalWorker<'a, G> {
    pub(super) fn new(
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

    pub(super) fn finish(
        self,
        sample_id: u64,
        traverser: usize,
        deal_attempts: u64,
    ) -> DenseTraversalDelta {
        DenseTraversalDelta {
            sample_id,
            traverser,
            deal_attempts,
            terminal_evaluations: self.terminal_evaluations,
            hand_updates: self.combos.len() as u64,
            events: self.events,
        }
    }

    /// Evaluates the traverser's terminal utility for the subset of the
    /// master combo list named by `active` (indices into `self.combos`).
    /// `active` need not be the full feasible set: pruned subtrees (see
    /// [`Self::traverse`]) descend with a smaller subset, and this method
    /// only ever pays for the combos actually named by it.
    fn terminal_vector(
        &mut self,
        state: &G::State,
        world: &SampledWorld,
        traverser: usize,
        active: &[usize],
    ) -> Result<Vec<f64>, SolverError> {
        let mut values = Vec::new();
        let subset: Vec<usize> = active.iter().map(|&idx| self.combos[idx]).collect();
        self.game
            .terminal_utilities_for_combos(state, world, traverser, &subset, &mut values);
        if values.len() != active.len() {
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

    /// `active` names the traversal's currently active combo subset as
    /// indices into the fixed master `self.combos`/`self.weights` (the root
    /// call passes `0..self.combos.len()`); `own_reach` is aligned to
    /// `active` the same way. When [`SolverConfig::prune`] is disabled,
    /// `active` is always the full master range at every node (pruning is
    /// the only thing that ever shrinks it), so this is byte-identical to
    /// the pre-pruning algorithm.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn traverse(
        &mut self,
        state: G::State,
        node_id: NodeId,
        world: &SampledWorld,
        traverser: usize,
        active: &[usize],
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
            return self.terminal_vector(&state, world, traverser, active);
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
            let active_len = active.len();
            let street_index = node.street.index();
            if self.bucket_cache[street_index].is_none() {
                // Always built from the full master combo list, even when
                // `active` is a pruned-down subset: the cache is reused by
                // every later traverser node on this street within the
                // traversal, some of which may see a wider active set.
                let table = self
                    .game
                    .buckets_for_combos(&state, world, traverser, &self.combos);
                self.bucket_cache[street_index] = Some(Arc::from(table));
            }
            // Keep an owned, constant-time handle across recursive calls instead of
            // cloning the full combo-to-bucket table at every traverser node.
            let bucket_table = Arc::clone(
                self.bucket_cache[street_index]
                    .as_ref()
                    .expect("populated above"),
            );

            // Per-bucket regret-matched strategy for every bucket present
            // among the currently active combos (buckets absent from
            // `active` were pruned out of this subtree by an ancestor
            // action and never need a strategy here). When pruning is
            // enabled, the same arena read also snapshots which (bucket,
            // action) pairs are pruning candidates -- zero regret-matched
            // probability and regret below `prune_threshold` -- so no
            // second arena lookup is needed later.
            let prune_threshold = self.config.prune_threshold as f32;
            let mut bucket_policies: FxHashMap<BucketId, BucketPolicy> = FxHashMap::default();
            // Per-bucket prunable actions are stored beside the strategy,
            // avoiding a second hash table and lookup throughout this node.
            // Actions past index 63 are never prunable; real trees have far
            // fewer actions per node.
            let mut any_prunable = 0u64;
            for &idx in active {
                let bucket = bucket_table[idx];
                if let Entry::Vacant(entry) = bucket_policies.entry(bucket) {
                    let range = self.arena.slot_range(node_id, bucket)?;
                    let regrets = &self.arena.regrets[range];
                    let strategy = regret_matching(regrets);
                    let mut prunable_actions = 0u64;
                    if self.config.prune {
                        for (action, (&probability, &regret)) in
                            strategy.iter().zip(regrets.iter()).enumerate().take(64)
                        {
                            if probability == 0.0 && regret < prune_threshold {
                                prunable_actions |= 1u64 << action;
                            }
                        }
                    }
                    any_prunable |= prunable_actions;
                    entry.insert(BucketPolicy {
                        strategy,
                        prunable_actions,
                    });
                }
            }

            // Pluribus-style regret-based pruning: for each action with at
            // least one prunable bucket among the currently active combos,
            // draw one coin deciding whether this visit actually skips
            // those buckets' subtrees. The coin is drawn lazily -- only
            // when pruning is enabled and something is prunable for this
            // action -- so a `prune = false` (or nothing-prunable)
            // traversal never advances `rng` any differently than before
            // this feature existed. A single bitmask also avoids allocating
            // one `Vec<bool>` at every traverser node.
            let mut skipped_mask = 0u64;
            for action in 0..num_actions.min(64) {
                let action_bit = 1u64 << action;
                if any_prunable & action_bit != 0 {
                    let needle = rng.gen_range(0.0..1.0);
                    if needle < self.config.prune_skip_probability {
                        skipped_mask |= action_bit;
                    }
                }
            }

            let mut action_values: Vec<Vec<f64>> = Vec::with_capacity(num_actions);
            for action in 0..num_actions {
                let next_state = self.game.next_state_with(&state, &actions, action);
                let skips_pruned = action < 64 && skipped_mask & (1u64 << action) != 0;
                let child_value = if skips_pruned {
                    // Combo-subset descent: drop combos whose bucket is a
                    // pruning candidate for this action. Their
                    // regret-matched probability is exactly zero, so they
                    // contribute zero to `node_value` and get no regret
                    // update below regardless of what value they'd have
                    // received -- skipping them here only saves work.
                    let action_bit = 1u64 << action;
                    let mut child_active = Vec::with_capacity(active_len);
                    let mut child_positions = Vec::with_capacity(active_len);
                    for (pos, &idx) in active.iter().enumerate() {
                        let pruned =
                            bucket_policies[&bucket_table[idx]].prunable_actions & action_bit != 0;
                        if !pruned {
                            child_active.push(idx);
                            child_positions.push(pos);
                        }
                    }
                    let sub_values = match node.children[action] {
                        Child::Terminal => {
                            self.terminal_vector(&next_state, world, traverser, &child_active)?
                        }
                        Child::Decision(child_id) => {
                            let child_own_reach: Vec<f64> = child_active
                                .iter()
                                .zip(child_positions.iter())
                                .map(|(&idx, &pos)| {
                                    let strategy = &bucket_policies[&bucket_table[idx]].strategy;
                                    own_reach[pos] * strategy[action]
                                })
                                .collect();
                            self.traverse(
                                next_state,
                                child_id,
                                world,
                                traverser,
                                &child_active,
                                &child_own_reach,
                                sample_importance,
                                rng,
                                depth + 1,
                            )?
                        }
                    };
                    let mut scattered = vec![0.0; active_len];
                    for (sub_pos, &pos) in child_positions.iter().enumerate() {
                        scattered[pos] = sub_values[sub_pos];
                    }
                    scattered
                } else {
                    match node.children[action] {
                        Child::Terminal => {
                            self.terminal_vector(&next_state, world, traverser, active)?
                        }
                        Child::Decision(child_id) => {
                            // Per-combo reach for this action's subtree:
                            // each feasible combo's own bucket picks its own
                            // regret-matched probability of taking
                            // `action`.
                            let mut child_own_reach = vec![0.0; active_len];
                            for (pos, &idx) in active.iter().enumerate() {
                                let strategy = &bucket_policies[&bucket_table[idx]].strategy;
                                child_own_reach[pos] = own_reach[pos] * strategy[action];
                            }
                            self.traverse(
                                next_state,
                                child_id,
                                world,
                                traverser,
                                active,
                                &child_own_reach,
                                sample_importance,
                                rng,
                                depth + 1,
                            )?
                        }
                    }
                };
                if child_value.len() != active_len {
                    return Err(SolverError::InvalidState(
                        "vector traversal child value width changed mid-traversal",
                    ));
                }
                action_values.push(child_value);
            }

            let mut node_value = vec![0.0; active_len];
            for (pos, value) in node_value.iter_mut().enumerate() {
                let idx = active[pos];
                let strategy = &bucket_policies[&bucket_table[idx]].strategy;
                let mut total = 0.0;
                for (action, values) in action_values.iter().enumerate() {
                    total += strategy[action] * values[pos];
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
            // average-strategy mass for bucket b at this node. Keeping all
            // three values in one record reduces three hash-table probes per
            // combo to one without changing floating-point accumulation order.
            let mut bucket_updates: FxHashMap<BucketId, BucketUpdate> = FxHashMap::default();
            for (pos, &idx) in active.iter().enumerate() {
                let bucket = bucket_table[idx];
                let weight = self.weights[idx];
                let update = bucket_updates
                    .entry(bucket)
                    .or_insert_with(|| BucketUpdate {
                        weight_sum: 0.0,
                        reach_weight_sum: 0.0,
                        action_diffs: vec![0.0; num_actions],
                    });
                update.weight_sum += weight;
                update.reach_weight_sum += weight * own_reach[pos];

                // A (bucket, action) pair that this visit actually pruned
                // gets exactly no regret update -- not a `weight * (0.0 -
                // node_value)` update -- because the action was never
                // sampled for this bucket this visit. Zero when nothing was
                // skipped at this node.
                let pruned_mask = if skipped_mask != 0 {
                    skipped_mask & bucket_policies[&bucket].prunable_actions
                } else {
                    0
                };
                for (action, values) in action_values.iter().enumerate() {
                    if action < 64 && pruned_mask & (1u64 << action) != 0 {
                        continue;
                    }
                    update.action_diffs[action] += weight * (values[pos] - node_value[pos]);
                }
            }
            for (bucket, mut update) in bucket_updates {
                // Every bucket present in `bucket_table` has at least one
                // feasible member with positive range weight, so this is
                // always strictly positive; the guard is defensive only.
                if update.weight_sum <= 0.0 {
                    continue;
                }
                for value in &mut update.action_diffs {
                    let scaled = sample_importance * (*value / update.weight_sum);
                    if !scaled.is_finite() {
                        return Err(SolverError::NumericOverflow);
                    }
                    *value = scaled;
                }
                let column = self.arena.column_id(node_id, bucket)?;
                self.events.push(DenseEvent::AddRegret {
                    column,
                    values: update.action_diffs,
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
                if update.reach_weight_sum > 0.0 {
                    let sigma = &bucket_policies[&bucket].strategy;
                    let values: Vec<f64> = sigma
                        .iter()
                        .map(|&probability| {
                            self.linear_weight * update.reach_weight_sum * probability
                        })
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
                Child::Terminal => self.terminal_vector(&next_state, world, traverser, active),
                Child::Decision(child_id) => self.traverse(
                    next_state,
                    child_id,
                    world,
                    traverser,
                    active,
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
