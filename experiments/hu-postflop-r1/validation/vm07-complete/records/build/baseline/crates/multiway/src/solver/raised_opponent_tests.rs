//! Finite arithmetic oracles for the research-only raised-opponent walk.
//! These games have explicit utilities; they do not use poker settlement or
//! another CFR implementation to calculate the expected regret increments.

use std::collections::BTreeMap;
use std::fmt::Write;

use rand::RngCore;

use super::*;

#[derive(Clone)]
enum ToyNode {
    Decision { actor: usize, children: Vec<usize> },
    Terminal([f64; 2]),
}

struct ToyGame {
    nodes: Vec<ToyNode>,
    root: usize,
    combos: [usize; 2],
    shared_bucket: bool,
}

impl ToyGame {
    fn empty(shared_bucket: bool) -> Self {
        Self {
            nodes: vec![],
            root: 0,
            combos: [
                cards::combo_index("As".parse().unwrap(), "Ah".parse().unwrap()),
                cards::combo_index("Ks".parse().unwrap(), "Kh".parse().unwrap()),
            ],
            shared_bucket,
        }
    }

    fn terminal(&mut self, values: [f64; 2]) -> usize {
        let id = self.nodes.len();
        self.nodes.push(ToyNode::Terminal(values));
        id
    }

    fn decision(&mut self, actor: usize, children: Vec<usize>) -> usize {
        let id = self.nodes.len();
        self.nodes.push(ToyNode::Decision { actor, children });
        id
    }

    fn hand_index(&self, combo: usize) -> usize {
        self.combos.iter().position(|&c| c == combo).unwrap()
    }

    fn payoff(&self, state: usize, combo: usize) -> f64 {
        match self.nodes[state] {
            ToyNode::Terminal(values) => values[self.hand_index(combo)],
            ToyNode::Decision { .. } => panic!("nonterminal payoff requested"),
        }
    }

    fn world(&self) -> SampledWorld {
        let opponent = cards::combo_index("Qs".parse().unwrap(), "Jc".parse().unwrap());
        SampledWorld::new(
            vec![self.combos[0], opponent],
            ["2c", "3d", "4h", "5s", "6c"].map(|card| card.parse().unwrap()),
        )
        .unwrap()
    }

    fn dense(&self) -> DenseStorage {
        let tree = tree::enumerate_tree(self).unwrap();
        let arena = tree::build_arena(self, &tree, 1 << 20).unwrap();
        DenseStorage { tree, arena }
    }
}

impl ExternalSamplingGame for ToyGame {
    type State = usize;
    type Actions = usize;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> usize {
        self.root
    }

    fn actor(&self, state: &usize) -> Option<usize> {
        match self.nodes[*state] {
            ToyNode::Decision { actor, .. } => Some(actor),
            ToyNode::Terminal(_) => None,
        }
    }

    fn node_actions(&self, state: &usize) -> usize {
        *state
    }

    fn num_actions_of(&self, actions: &usize) -> usize {
        match &self.nodes[*actions] {
            ToyNode::Decision { children, .. } => children.len(),
            ToyNode::Terminal(_) => 0,
        }
    }

    fn next_state_with(&self, state: &usize, actions: &usize, action_index: usize) -> usize {
        assert_eq!(state, actions);
        match &self.nodes[*actions] {
            ToyNode::Decision { children, .. } => children[action_index],
            ToyNode::Terminal(_) => panic!("terminal expansion requested"),
        }
    }

    fn write_action_label(&self, _actions: &usize, action_index: usize, out: &mut String) {
        write!(out, "action-{action_index}").unwrap();
    }

    fn bucket(&self, state: &usize, world: &SampledWorld, actor: usize) -> PrivateInfo {
        assert_eq!(self.actor(state), Some(actor));
        // Opponent decisions use only the opponent's physical hand. This toy
        // deliberately puts all such hands in one class; they never inspect
        // the substituted traverser hand or its payoff.
        let bucket = if actor == 0 {
            self.bucket_for_combo(state, world, actor, world.hole_combo(actor))
        } else {
            0
        };
        PrivateInfo::from_current_bucket(Street::Preflop, 1, bucket)
    }

    fn terminal_utilities(&self, state: &usize, world: &SampledWorld, utilities: &mut [f64]) {
        let value = self.payoff(*state, world.hole_combo(0));
        utilities.copy_from_slice(&[value, -value]);
    }

    fn recall_mode(&self) -> RecallMode {
        RecallMode::Street
    }

    fn bucket_count(&self, _street: Street, _active_opponents: u8) -> u32 {
        if self.shared_bucket { 1 } else { 2 }
    }

    fn dense_node_context(&self, _state: &usize) -> DenseNodeContext {
        DenseNodeContext {
            street: Street::Preflop,
            active_opponents: 1,
            bucket_active_opponents: 1,
        }
    }

    fn bucket_for_combo(
        &self,
        _state: &usize,
        _world: &SampledWorld,
        actor: usize,
        combo: usize,
    ) -> BucketId {
        assert_eq!(actor, 0);
        let hand = self.hand_index(combo);
        if self.shared_bucket { 0 } else { hand as u32 }
    }

    fn terminal_utilities_for_combos(
        &self,
        state: &usize,
        _world: &SampledWorld,
        traverser: usize,
        combos: &[usize],
        out: &mut Vec<f64>,
    ) {
        assert_eq!(traverser, 0);
        out.extend(combos.iter().map(|&combo| self.payoff(*state, combo)));
    }
}

fn node_id(dense: &DenseStorage, path: &[usize]) -> NodeId {
    let mut id = 0;
    for &action in path {
        id = match dense.tree.nodes[id as usize].children[action] {
            Child::Decision(child) => child,
            Child::Terminal => panic!("path ends in a terminal"),
        };
    }
    id
}

fn set_regrets(dense: &mut DenseStorage, path: &[usize], bucket: BucketId, regrets: &[f32]) {
    let range = dense
        .arena
        .slot_range(node_id(dense, path), bucket)
        .unwrap();
    dense.arena.regrets[range].copy_from_slice(regrets);
}

struct TraversalResult {
    values: Vec<f64>,
    events: Vec<(u32, Vec<f64>)>,
    terminals: u64,
    rng: ChaCha20Rng,
}

fn run(
    game: &ToyGame,
    dense: &DenseStorage,
    eligible: &[bool],
    raised: bool,
    alpha: f64,
    seed: u64,
) -> TraversalResult {
    let config = SolverConfig {
        max_memory_bytes: 1 << 20,
        max_traversal_depth: 16,
        exploration_epsilon: 0.0,
        discount_until: 0,
        traverser_vector: true,
        prune: false,
        ..SolverConfig::default()
    };
    let mut worker =
        VectorTraversalWorker::new(game, dense, config, game.combos.to_vec(), vec![1.0, 3.0])
            .unwrap()
            .with_raised_preflop_nodes(eligible);
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let world = game.world();
    let values = if raised {
        worker.traverse_raised_preflop(game.root, 0, &world, 0, &[0, 1], alpha, &mut rng, 0)
    } else {
        worker.traverse(game.root, 0, &world, 0, &[0, 1], alpha, &mut rng, 0)
    }
    .unwrap();
    let delta = worker.finish(0, 0, 1);
    assert_eq!(delta.hand_updates, 2);
    let events = delta
        .events
        .into_iter()
        .map(|event| match event {
            DenseEvent::AddRegret { column, values } => (column, values),
            DenseEvent::AddStrategy { .. } => panic!("regret worker changed averaging state"),
        })
        .collect();
    TraversalResult {
        values,
        events,
        terminals: delta.terminal_evaluations,
        rng,
    }
}

fn regret_at(
    result: &TraversalResult,
    dense: &DenseStorage,
    path: &[usize],
    bucket: BucketId,
) -> Vec<f64> {
    let id = node_id(dense, path);
    let column = dense.arena.column_id(id, bucket).unwrap();
    let mut total = vec![0.0; dense.tree.nodes[id as usize].action_labels.len()];
    for (_, values) in result.events.iter().filter(|(c, _)| *c == column) {
        for (sum, value) in total.iter_mut().zip(values) {
            *sum += value;
        }
    }
    total
}

fn two_hand_fixture(shared_bucket: bool) -> (ToyGame, DenseStorage, Vec<bool>) {
    let mut game = ToyGame::empty(shared_bucket);
    let own_fold = game.terminal([0.0, 0.0]);
    let opponent_fold = game.terminal([2.0, 2.0]);
    let left = game.terminal([10.0, -4.0]);
    let right = game.terminal([-2.0, 6.0]);
    let own_later = game.decision(0, vec![left, right]);
    let opponent = game.decision(1, vec![opponent_fold, own_later]);
    game.root = game.decision(0, vec![own_fold, opponent]);
    let mut dense = game.dense();
    set_regrets(&mut dense, &[1], 0, &[3.0, 1.0]);
    let mut eligible = vec![false; dense.tree.nodes.len()];
    eligible[node_id(&dense, &[1]) as usize] = true;
    (game, dense, eligible)
}

#[test]
fn raised_opponent_exact_descendant_and_ancestor_weights_exclude_own_reach() {
    for own_raise_zero in [false, true] {
        for alpha in [0.5, 1.0, 2.0] {
            let (game, mut dense, eligible) = two_hand_fixture(false);
            if own_raise_zero {
                for bucket in 0..2 {
                    set_regrets(&mut dense, &[], bucket, &[1.0, 0.0]);
                }
            }
            let result = run(&game, &dense, &eligible, true, alpha, 17);
            // Hand utility differences are (6,-6), (-5,5). The only
            // counterfactual reach is opponent action 1's probability 1/4;
            // raw own weights (1,3) normalize once to (1/4,3/4).
            assert_eq!(
                regret_at(&result, &dense, &[1, 1], 0),
                vec![alpha * 0.375, alpha * -0.375]
            );
            assert_eq!(
                regret_at(&result, &dense, &[1, 1], 1),
                vec![alpha * -0.9375, alpha * 0.9375]
            );
            // Independently: raise values are 3/4*2 + 1/4*4 = 2.5 and
            // 3/4*2 + 1/4*1 = 1.75. Returned values do not contain alpha,
            // and do not apply the opponent probability a second time.
            if own_raise_zero {
                assert_eq!(result.values, vec![0.0, 0.0]);
                assert_eq!(regret_at(&result, &dense, &[], 0), vec![0.0, alpha * 0.625]);
                assert_eq!(
                    regret_at(&result, &dense, &[], 1),
                    vec![0.0, alpha * 1.3125]
                );
            } else {
                assert_eq!(result.values, vec![1.25, 0.875]);
                assert_eq!(
                    regret_at(&result, &dense, &[], 0),
                    vec![alpha * -0.3125, alpha * 0.3125]
                );
                assert_eq!(
                    regret_at(&result, &dense, &[], 1),
                    vec![alpha * -0.65625, alpha * 0.65625]
                );
            }
            assert_eq!(result.terminals, 4);
        }
    }
}

#[test]
fn raised_opponent_shared_bucket_adds_hand_mass_without_renormalizing() {
    let (game, dense, eligible) = two_hand_fixture(true);
    let result = run(&game, &dense, &eligible, true, 2.0, 5);
    assert_eq!(result.values, vec![1.25, 0.875]);
    // Sum the two independently derived hand contributions, including their
    // unequal probabilities. Do not average the two hands equally.
    assert_eq!(regret_at(&result, &dense, &[1, 1], 0), vec![-1.125, 1.125]);
    assert_eq!(regret_at(&result, &dense, &[], 0), vec![-1.9375, 1.9375]);
    assert_eq!(result.events.len(), 2);
}

#[test]
fn raised_opponent_zero_probability_branch_cannot_create_numeric_support() {
    let (game, mut dense, eligible) = two_hand_fixture(false);
    set_regrets(&mut dense, &[1], 0, &[1.0, 0.0]);
    for bucket in 0..2 {
        set_regrets(&mut dense, &[], bucket, &[1.0, 0.0]);
    }
    let result = run(&game, &dense, &eligible, true, 1.0, 7);
    // Skipping the zero opponent branch and emitting all-zero descendant
    // events are both numerically valid. Neither yields regret support.
    assert_eq!(regret_at(&result, &dense, &[1, 1], 0), vec![0.0, 0.0]);
    assert_eq!(regret_at(&result, &dense, &[1, 1], 1), vec![0.0, 0.0]);
    assert_eq!(regret_at(&result, &dense, &[], 0), vec![0.0, 0.5]);
    assert_eq!(regret_at(&result, &dense, &[], 1), vec![0.0, 1.5]);
    assert_eq!(result.values, vec![0.0, 0.0]);
}

fn sibling_fixture() -> (ToyGame, DenseStorage, Vec<bool>) {
    let mut game = ToyGame::empty(false);
    let mut root_children = vec![];
    for scale in [1.0, 2.0] {
        let mut first_children = vec![];
        for values in [[[1.0, 4.0], [5.0, -2.0]], [[9.0, 8.0], [-3.0, 6.0]]] {
            let leaves = values
                .into_iter()
                .map(|pair| game.terminal(pair.map(|value| scale * value)))
                .collect();
            first_children.push(game.decision(1, leaves));
        }
        root_children.push(game.decision(1, first_children));
    }
    game.root = game.decision(0, root_children);
    let mut dense = game.dense();
    for root_action in 0..2 {
        set_regrets(&mut dense, &[root_action], 0, &[3.0, 1.0]);
    }
    // Even marking the traverser's node eligible must not spend its path
    // budget. Each following opponent qualifies; only the first may expand.
    let eligible = vec![true; dense.tree.nodes.len()];
    (game, dense, eligible)
}

#[test]
fn raised_opponent_budget_is_per_path_and_later_opponent_still_samples() {
    let (game, dense, eligible) = sibling_fixture();
    for seed in 0..16 {
        let mut oracle_rng = ChaCha20Rng::seed_from_u64(seed);
        let mut root_action_values = vec![];
        for scale in [1.0, 2.0] {
            let _virtual_first_draw = oracle_rng.gen_range(0.0..1.0);
            let later_action_zero = oracle_rng.gen_range(0.0..1.0) < 0.5;
            // The two first-opponent children share this subsequent draw.
            // Their weighted utilities are (3,5) for later action 0 and
            // (3,0) for action 1; the second root subtree doubles utility.
            root_action_values.push([
                3.0 * scale,
                if later_action_zero { 5.0 * scale } else { 0.0 },
            ]);
        }
        let mut result = run(&game, &dense, &eligible, true, 1.0, seed);
        for (hand, weight) in [0.25, 0.75].into_iter().enumerate() {
            let left = root_action_values[0][hand];
            let right = root_action_values[1][hand];
            let expected = (left + right) * 0.5;
            assert_eq!(result.values[hand], expected);
            assert_eq!(
                regret_at(&result, &dense, &[], hand as u32),
                vec![weight * (left - expected), weight * (right - expected)]
            );
        }
        // Four leaves = two root branches * two first-opponent actions *
        // one sampled later action. A shared budget produces three; a second
        // enumeration on each path produces eight.
        assert_eq!(result.terminals, 4);
        assert_eq!(result.rng.next_u64(), oracle_rng.next_u64());
    }
}

fn uneven_tail_fixture() -> (ToyGame, DenseStorage, Vec<bool>) {
    let mut game = ToyGame::empty(false);
    let mut root_children = vec![];
    for offset in [0.0, 10.0] {
        let immediate = game.terminal([offset, -offset]);
        let middle = game.terminal([offset + 1.0, offset + 2.0]);
        let deep_left = game.terminal([offset + 3.0, offset + 4.0]);
        let deep_right = game.terminal([offset + 5.0, offset + 6.0]);
        let deep = game.decision(1, vec![deep_left, deep_right]);
        let late = game.decision(1, vec![middle, deep]);
        root_children.push(game.decision(1, vec![immediate, late]));
    }
    game.root = game.decision(0, root_children);
    let mut dense = game.dense();
    for root_action in 0..2 {
        set_regrets(&mut dense, &[root_action], 0, &[3.0, 1.0]);
        set_regrets(&mut dense, &[root_action, 1], 0, &[1.0, 3.0]);
    }
    let eligible = vec![true; dense.tree.nodes.len()];
    (game, dense, eligible)
}

#[test]
fn raised_opponent_restores_virtual_chosen_child_rng_with_unequal_tail_depths() {
    let (game, dense, eligible) = uneven_tail_fixture();
    let mut observed_continuations = BTreeMap::new();
    for seed in 0..64 {
        let mut original = run(&game, &dense, &eligible, false, 1.0, seed);
        let mut expanded = run(&game, &dense, &eligible, true, 1.0, seed);
        // Independently count the original sampled walk's two root siblings.
        // The selected first child consumes zero, one, or two further draws.
        let mut oracle = ChaCha20Rng::seed_from_u64(seed);
        let mut draws = 0;
        for _ in 0..2 {
            draws += 1;
            if oracle.gen_range(0.0..1.0) >= 0.75 {
                draws += 1;
                if oracle.gen_range(0.0..1.0) >= 0.25 {
                    draws += 1;
                    let _: f64 = oracle.gen_range(0.0..1.0);
                }
            }
        }
        *observed_continuations.entry(draws).or_insert(0usize) += 1;
        for _ in 0..4 {
            let expected = oracle.next_u64();
            assert_eq!(original.rng.next_u64(), expected);
            assert_eq!(expanded.rng.next_u64(), expected);
        }
    }
    // Ensure this finite set actually exercises immediate and deeper virtual
    // children; the assertions above compare streams, not sample frequencies.
    assert!(observed_continuations.contains_key(&2));
    assert!(observed_continuations.keys().any(|&draws| draws >= 4));
}

#[test]
fn raised_opponent_disabled_mask_preserves_original_events_values_and_rng() {
    let (game, dense, _) = uneven_tail_fixture();
    let disabled = vec![false; dense.tree.nodes.len()];
    for seed in 0..16 {
        let mut original = run(&game, &dense, &disabled, false, 0.5, seed);
        let mut disabled_research = run(&game, &dense, &disabled, true, 0.5, seed);
        assert_eq!(original.values, disabled_research.values);
        assert_eq!(original.events, disabled_research.events);
        assert_eq!(original.terminals, disabled_research.terminals);
        for _ in 0..4 {
            assert_eq!(original.rng.next_u64(), disabled_research.rng.next_u64());
        }
    }
}
