use std::sync::Arc;

use rand::Rng;
use rand_chacha::ChaCha20Rng;
use rustc_hash::FxHashMap;

use super::workers::{DenseEvent, DenseStorage, TraversalEvent};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AverageOpponentSampling {
    UniformOne,
    EnumerateFirst,
}

/// Independent average-policy traversal for sparse, full-recall storage.
///
/// At `averager` nodes every action is followed and `own_reach` is multiplied
/// by that hand's current strategy. At every other seat only one legal action
/// is sampled uniformly, irrespective of that seat's current strategy. Thus
/// every public history has full support, including histories behind a
/// zero-probability opponent action. The omitted inverse sampling probability
/// is a fixed scalar for one exact public-history column and cancels when its
/// `strategy_sum` is normalized.
pub(super) struct SparseAverageStrategyWorker<'a, G: ExternalSamplingGame> {
    game: &'a G,
    policies: &'a FxHashMap<InfoKey, PolicyColumn>,
    histories: &'a FxHashMap<HistoryKey, HistoryEntry>,
    max_depth: u32,
    linear_weight: f64,
    events: Vec<TraversalEvent>,
    local_policies: FxHashMap<InfoKey, Vec<String>>,
    local_histories: FxHashMap<HistoryKey, HistoryEntry>,
    opponent_sampling: AverageOpponentSampling,
}

impl<'a, G: ExternalSamplingGame> SparseAverageStrategyWorker<'a, G> {
    #[cfg(test)]
    pub(super) fn new(solver: &'a MultiwaySolver<G>, linear_weight: f64) -> Self {
        Self::with_sampling(solver, linear_weight, AverageOpponentSampling::UniformOne)
    }

    pub(super) fn with_sampling(
        solver: &'a MultiwaySolver<G>,
        linear_weight: f64,
        opponent_sampling: AverageOpponentSampling,
    ) -> Self {
        Self {
            game: &solver.game,
            policies: &solver.policies,
            histories: &solver.histories,
            max_depth: solver.config.max_traversal_depth,
            linear_weight,
            events: Vec::new(),
            local_policies: FxHashMap::default(),
            local_histories: FxHashMap::default(),
            opponent_sampling,
        }
    }

    pub(super) fn finish(self) -> Vec<TraversalEvent> {
        self.events
    }

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
                return Err(SolverError::ActionCountChanged {
                    key,
                    stored: labels.len(),
                    current: num_actions,
                });
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
        let existing = self
            .histories
            .get(&key)
            .or_else(|| self.local_histories.get(&key));
        if let Some(existing) = existing {
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
        averager: usize,
        history: HistoryKey,
        own_reach: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
    ) -> Result<(), SolverError> {
        self.traverse_inner(
            state, world, averager, history, own_reach, rng, depth, false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse_inner(
        &mut self,
        state: G::State,
        world: &SampledWorld,
        averager: usize,
        history: HistoryKey,
        own_reach: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
        first_opponent_seen: bool,
    ) -> Result<(), SolverError> {
        if depth > self.max_depth {
            return Err(SolverError::DepthLimit {
                limit: self.max_depth,
            });
        }
        let Some(actor) = self.game.actor(&state) else {
            return Ok(());
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

        if actor == averager {
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
            let values = strategy
                .iter()
                .map(|&probability| self.linear_weight * own_reach * probability)
                .collect();
            self.events
                .push(TraversalEvent::AddStrategy { key, values });

            let mut label = String::new();
            for (action, &probability) in strategy.iter().enumerate() {
                let child_reach = own_reach * probability;
                if child_reach == 0.0 {
                    continue;
                }
                if !child_reach.is_finite() {
                    return Err(SolverError::NumericOverflow);
                }
                label.clear();
                self.game.write_action_label(&actions, action, &mut label);
                let child_history = self.record_history(history, actor, action, &label)?;
                let next = self.game.next_state_with(&state, &actions, action);
                self.traverse_inner(
                    next,
                    world,
                    averager,
                    child_history,
                    child_reach,
                    rng,
                    depth + 1,
                    first_opponent_seen,
                )?;
            }
        } else if self.opponent_sampling == AverageOpponentSampling::EnumerateFirst
            && !first_opponent_seen
        {
            // Consume the draw used by UniformOne, then give every child the
            // same post-draw stream. The child selected by the discarded draw
            // is therefore paired exactly with the baseline walk.
            let discarded_action = rng.gen_range(0..num_actions);
            let child_base_rng = rng.clone();
            let mut selected_final_rng = None;
            let mut label = String::new();
            for action in 0..num_actions {
                label.clear();
                self.game.write_action_label(&actions, action, &mut label);
                let child_history = self.record_history(history, actor, action, &label)?;
                let next = self.game.next_state_with(&state, &actions, action);
                let mut child_rng = child_base_rng.clone();
                self.traverse_inner(
                    next,
                    world,
                    averager,
                    child_history,
                    own_reach,
                    &mut child_rng,
                    depth + 1,
                    true,
                )?;
                if action == discarded_action {
                    selected_final_rng = Some(child_rng);
                }
            }
            *rng = selected_final_rng.expect("discarded action is in the legal action range");
        } else {
            // Uniform public-action sampling is deliberately independent of
            // this opponent's policy, so zero-probability actions retain full
            // support and no importance ratio can divide by zero.
            let action = rng.gen_range(0..num_actions);
            let mut label = String::new();
            self.game.write_action_label(&actions, action, &mut label);
            let child_history = self.record_history(history, actor, action, &label)?;
            let next = self.game.next_state_with(&state, &actions, action);
            self.traverse_inner(
                next,
                world,
                averager,
                child_history,
                own_reach,
                rng,
                depth + 1,
                first_opponent_seen,
            )?;
        }
        Ok(())
    }
}

/// Independent average-policy traversal for the preallocated Street-recall
/// arena. Scalar mode follows one dealt hand; vector mode Rao-Blackwellizes
/// over every feasible own combo using conditional range weights.
pub(super) struct DenseAverageStrategyWorker<'a, G: ExternalSamplingGame> {
    game: &'a G,
    tree: &'a PublicTree,
    arena: &'a DenseArena,
    max_depth: u32,
    linear_weight: f64,
    events: Vec<DenseEvent>,
    /// Full feasible-combo bucket tables keyed by `(street, opponents at
    /// street start)`. Counterfactual branches can reach the same street with
    /// different player counts, so street alone is not a valid cache key.
    bucket_cache: FxHashMap<(usize, u8), Arc<[BucketId]>>,
    opponent_sampling: AverageOpponentSampling,
}

impl<'a, G: ExternalSamplingGame> DenseAverageStrategyWorker<'a, G> {
    #[cfg(test)]
    pub(super) fn new(
        game: &'a G,
        dense: &'a DenseStorage,
        config: SolverConfig,
        linear_weight: f64,
    ) -> Self {
        Self::with_sampling(
            game,
            dense,
            config,
            linear_weight,
            AverageOpponentSampling::UniformOne,
        )
    }

    pub(super) fn with_sampling(
        game: &'a G,
        dense: &'a DenseStorage,
        config: SolverConfig,
        linear_weight: f64,
        opponent_sampling: AverageOpponentSampling,
    ) -> Self {
        Self {
            game,
            tree: &dense.tree,
            arena: &dense.arena,
            max_depth: config.max_traversal_depth,
            linear_weight,
            events: Vec::new(),
            bucket_cache: FxHashMap::default(),
            opponent_sampling,
        }
    }

    pub(super) fn finish(self) -> Vec<DenseEvent> {
        self.events
    }

    fn node<'b>(
        &self,
        node_id: NodeId,
        actor: usize,
        actions: &'b G::Actions,
    ) -> Result<&'a tree::TreeNode, SolverError> {
        let node = self
            .tree
            .nodes
            .get(node_id as usize)
            .ok_or(SolverError::InvalidState(
                "average traversal referenced an unknown dense node",
            ))?;
        let num_actions = self.game.num_actions_of(actions);
        if node.actor as usize != actor || node.action_labels.len() != num_actions {
            return Err(SolverError::TreeNodeMismatch {
                node: node_id,
                expected: node.action_labels.len(),
                found: num_actions,
            });
        }
        Ok(node)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn traverse_scalar(
        &mut self,
        state: G::State,
        node_id: NodeId,
        world: &SampledWorld,
        averager: usize,
        own_reach: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
    ) -> Result<(), SolverError> {
        self.traverse_scalar_inner(
            state, node_id, world, averager, own_reach, rng, depth, false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse_scalar_inner(
        &mut self,
        state: G::State,
        node_id: NodeId,
        world: &SampledWorld,
        averager: usize,
        own_reach: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
        first_opponent_seen: bool,
    ) -> Result<(), SolverError> {
        if depth > self.max_depth {
            return Err(SolverError::DepthLimit {
                limit: self.max_depth,
            });
        }
        let Some(actor) = self.game.actor(&state) else {
            return Ok(());
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
        let node = self.node(node_id, actor, &actions)?;

        if actor == averager {
            let private = self.game.bucket(&state, world, actor);
            validate_private_info(private, num_players, RecallMode::Street)?;
            let column = self.arena.column_id(node_id, private.current_bucket())?;
            let range = self.arena.slot_range(node_id, private.current_bucket())?;
            let strategy = regret_matching(&self.arena.regrets[range]);
            let values = strategy
                .iter()
                .map(|&probability| self.linear_weight * own_reach * probability)
                .collect();
            self.events.push(DenseEvent::AddStrategy { column, values });
            for (action, &probability) in strategy.iter().enumerate() {
                let Child::Decision(child_id) = node.children[action] else {
                    continue;
                };
                let child_reach = own_reach * probability;
                if child_reach == 0.0 {
                    continue;
                }
                if !child_reach.is_finite() {
                    return Err(SolverError::NumericOverflow);
                }
                let next = self.game.next_state_with(&state, &actions, action);
                self.traverse_scalar_inner(
                    next,
                    child_id,
                    world,
                    averager,
                    child_reach,
                    rng,
                    depth + 1,
                    first_opponent_seen,
                )?;
            }
        } else if self.opponent_sampling == AverageOpponentSampling::EnumerateFirst
            && !first_opponent_seen
        {
            let discarded_action = rng.gen_range(0..num_actions);
            let child_base_rng = rng.clone();
            let mut selected_final_rng =
                if matches!(node.children[discarded_action], Child::Terminal) {
                    Some(child_base_rng.clone())
                } else {
                    None
                };
            for (action, child) in node.children.iter().copied().enumerate() {
                let Child::Decision(child_id) = child else {
                    continue;
                };
                let next = self.game.next_state_with(&state, &actions, action);
                let mut child_rng = child_base_rng.clone();
                self.traverse_scalar_inner(
                    next,
                    child_id,
                    world,
                    averager,
                    own_reach,
                    &mut child_rng,
                    depth + 1,
                    true,
                )?;
                if action == discarded_action {
                    selected_final_rng = Some(child_rng);
                }
            }
            *rng = selected_final_rng.expect("discarded action has a known dense child");
        } else {
            let action = rng.gen_range(0..num_actions);
            let Child::Decision(child_id) = node.children[action] else {
                return Ok(());
            };
            let next = self.game.next_state_with(&state, &actions, action);
            self.traverse_scalar_inner(
                next,
                child_id,
                world,
                averager,
                own_reach,
                rng,
                depth + 1,
                first_opponent_seen,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn traverse_vector(
        &mut self,
        state: G::State,
        node_id: NodeId,
        world: &SampledWorld,
        averager: usize,
        combos: &[usize],
        weights: &[f64],
        own_reach: &[f64],
        rng: &mut ChaCha20Rng,
        depth: u32,
    ) -> Result<(), SolverError> {
        self.traverse_vector_inner(
            state, node_id, world, averager, combos, weights, own_reach, rng, depth, false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse_vector_inner(
        &mut self,
        state: G::State,
        node_id: NodeId,
        world: &SampledWorld,
        averager: usize,
        combos: &[usize],
        weights: &[f64],
        own_reach: &[f64],
        rng: &mut ChaCha20Rng,
        depth: u32,
        first_opponent_seen: bool,
    ) -> Result<(), SolverError> {
        if depth > self.max_depth {
            return Err(SolverError::DepthLimit {
                limit: self.max_depth,
            });
        }
        if combos.len() != weights.len() || combos.len() != own_reach.len() {
            return Err(SolverError::InvalidState(
                "average vector widths changed mid-traversal",
            ));
        }
        let Some(actor) = self.game.actor(&state) else {
            return Ok(());
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
        let node = self.node(node_id, actor, &actions)?;

        if actor == averager {
            let cache_key = (node.street.index(), node.bucket_active_opponents);
            if !self.bucket_cache.contains_key(&cache_key) {
                let buckets = self
                    .game
                    .buckets_for_combos(&state, world, averager, combos);
                self.bucket_cache.insert(cache_key, Arc::from(buckets));
            }
            let buckets = Arc::clone(self.bucket_cache.get(&cache_key).expect("populated above"));
            if buckets.len() != combos.len() {
                return Err(SolverError::InvalidState(
                    "buckets_for_combos returned the wrong number of buckets",
                ));
            }
            let mut bucket_strategies: FxHashMap<BucketId, Vec<f64>> = FxHashMap::default();
            let mut bucket_mass: FxHashMap<BucketId, f64> = FxHashMap::default();
            for (index, &bucket) in buckets.iter().enumerate() {
                if let Entry::Vacant(entry) = bucket_strategies.entry(bucket) {
                    let range = self.arena.slot_range(node_id, bucket)?;
                    entry.insert(regret_matching(&self.arena.regrets[range]));
                }
                *bucket_mass.entry(bucket).or_default() += weights[index] * own_reach[index];
            }
            for (&bucket, strategy) in &bucket_strategies {
                let mass = bucket_mass[&bucket];
                if mass <= 0.0 {
                    continue;
                }
                let values = strategy
                    .iter()
                    .map(|&probability| self.linear_weight * mass * probability)
                    .collect();
                let column = self.arena.column_id(node_id, bucket)?;
                self.events.push(DenseEvent::AddStrategy { column, values });
            }

            for (action, child) in node.children.iter().copied().enumerate() {
                let Child::Decision(child_id) = child else {
                    continue;
                };
                let mut child_reach = Vec::with_capacity(own_reach.len());
                let mut any_positive = false;
                for (index, &reach) in own_reach.iter().enumerate() {
                    let value = reach * bucket_strategies[&buckets[index]][action];
                    any_positive |= value > 0.0;
                    if !value.is_finite() {
                        return Err(SolverError::NumericOverflow);
                    }
                    child_reach.push(value);
                }
                if !any_positive {
                    continue;
                }
                let next = self.game.next_state_with(&state, &actions, action);
                self.traverse_vector_inner(
                    next,
                    child_id,
                    world,
                    averager,
                    combos,
                    weights,
                    &child_reach,
                    rng,
                    depth + 1,
                    first_opponent_seen,
                )?;
            }
        } else if self.opponent_sampling == AverageOpponentSampling::EnumerateFirst
            && !first_opponent_seen
        {
            let discarded_action = rng.gen_range(0..num_actions);
            let child_base_rng = rng.clone();
            let mut selected_final_rng =
                if matches!(node.children[discarded_action], Child::Terminal) {
                    Some(child_base_rng.clone())
                } else {
                    None
                };
            for (action, child) in node.children.iter().copied().enumerate() {
                let Child::Decision(child_id) = child else {
                    continue;
                };
                let next = self.game.next_state_with(&state, &actions, action);
                let mut child_rng = child_base_rng.clone();
                self.traverse_vector_inner(
                    next,
                    child_id,
                    world,
                    averager,
                    combos,
                    weights,
                    own_reach,
                    &mut child_rng,
                    depth + 1,
                    true,
                )?;
                if action == discarded_action {
                    selected_final_rng = Some(child_rng);
                }
            }
            *rng = selected_final_rng.expect("discarded action has a known dense child");
        } else {
            let action = rng.gen_range(0..num_actions);
            let Child::Decision(child_id) = node.children[action] else {
                return Ok(());
            };
            let next = self.game.next_state_with(&state, &actions, action);
            self.traverse_vector_inner(
                next,
                child_id,
                world,
                averager,
                combos,
                weights,
                own_reach,
                rng,
                depth + 1,
                first_opponent_seen,
            )?;
        }
        Ok(())
    }
}
