use std::sync::Arc;

use rand::Rng;
use rand_chacha::ChaCha20Rng;
use rustc_hash::FxHashMap;

use super::workers::{DenseEvent, DenseStorage};
use super::*;

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
}

impl<'a, G: ExternalSamplingGame> DenseAverageStrategyWorker<'a, G> {
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
            max_depth: config.max_traversal_depth,
            linear_weight,
            events: Vec::new(),
            bucket_cache: FxHashMap::default(),
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
                self.traverse_scalar(next, child_id, world, averager, child_reach, rng, depth + 1)?;
            }
        } else {
            let action = rng.gen_range(0..num_actions);
            let Child::Decision(child_id) = node.children[action] else {
                return Ok(());
            };
            let next = self.game.next_state_with(&state, &actions, action);
            self.traverse_scalar(next, child_id, world, averager, own_reach, rng, depth + 1)?;
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
                self.traverse_vector(
                    next,
                    child_id,
                    world,
                    averager,
                    combos,
                    weights,
                    &child_reach,
                    rng,
                    depth + 1,
                )?;
            }
        } else {
            let action = rng.gen_range(0..num_actions);
            let Child::Decision(child_id) = node.children[action] else {
                return Ok(());
            };
            let next = self.game.next_state_with(&state, &actions, action);
            self.traverse_vector(
                next,
                child_id,
                world,
                averager,
                combos,
                weights,
                own_reach,
                rng,
                depth + 1,
            )?;
        }
        Ok(())
    }
}
