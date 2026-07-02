use cards::{PerPlayer, Player};

use crate::schedule::{DiscountSchedule, Discounts};
use crate::storage::{Storage, StorageRef, StorageView};
use crate::tree::{NodeId, NodeKind, PublicTree};

/// Variant-owned terminal evaluation — the only variant code on the hot
/// path, called once per terminal per pass and amortized over all hands.
///
/// Writes, for each of player `p`'s hands, the unnormalized expected payoff
/// to `p`: the sum over opponent hands of `opp_reach[o] * compat(h, o) *
/// payoff_p(h, o)`. Hand-vs-hand card-removal (blocker) effects live
/// entirely inside implementations; chance-probability constants are baked
/// into deal weights and the game normalizer at build time.
pub trait TerminalEvaluator: Send + Sync {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]);
}

/// Everything the solver needs: the compiled tree, the terminal evaluator,
/// root ranges, and the normalizer converting unnormalized root aggregates
/// into per-deal expected values.
pub struct CompiledGame<E> {
    pub tree: PublicTree,
    pub evaluator: E,
    pub root_ranges: PerPlayer<Vec<f32>>,
    /// Total joint reach weight of compatible root hand pairs.
    pub normalizer: f64,
}

/// Vector-form CFR solver with alternating updates.
pub struct Solver<E, S> {
    game: CompiledGame<E>,
    storage: S,
    schedule: Box<dyn DiscountSchedule>,
    planned_iters: Option<u64>,
    iteration: u64,
}

impl<E: TerminalEvaluator, S: Storage> Solver<E, S> {
    pub fn new(
        game: CompiledGame<E>,
        schedule: Box<dyn DiscountSchedule>,
        planned_iters: Option<u64>,
    ) -> Self {
        assert!(
            !schedule.requires_planned_iters() || planned_iters.is_some(),
            "{} requires a planned iteration budget",
            schedule.name()
        );
        for (player, range) in [Player::P0, Player::P1]
            .into_iter()
            .zip(game.root_ranges.as_ref().0)
        {
            assert_eq!(
                range.len(),
                game.tree.root_dims[player] as usize,
                "root range length must match root dims"
            );
        }
        let storage = S::new(game.tree.storage_len);
        Solver {
            game,
            storage,
            schedule,
            planned_iters,
            iteration: 0,
        }
    }

    pub fn iteration(&self) -> u64 {
        self.iteration
    }

    pub fn game(&self) -> &CompiledGame<E> {
        &self.game
    }

    /// One alternating iteration: a regret/strategy update pass for each
    /// player in turn.
    pub fn step(&mut self) {
        let t = self.iteration + 1;
        let discounts = self.schedule.at(t, self.planned_iters);
        for p in Player::BOTH {
            let my_range = self.game.root_ranges[p].clone();
            let opp_range = self.game.root_ranges[p.opponent()].clone();
            let mut view = self.storage.view_mut();
            cfr_pass(
                &self.game.tree,
                &self.game.evaluator,
                &mut view,
                0,
                p,
                &my_range,
                &opp_range,
                &discounts,
            );
        }
        self.iteration = t;
    }

    pub fn run(&mut self, iterations: u64) {
        for _ in 0..iterations {
            self.step();
        }
    }

    /// Expected value of the average strategy profile for `p`, per deal.
    pub fn expected_value(&self, p: Player) -> f64 {
        let values = ev_pass(
            &self.game.tree,
            &self.game.evaluator,
            &self.storage,
            0,
            p,
            &self.game.root_ranges[p.opponent()],
        );
        self.root_aggregate(p, &values)
    }

    /// Best-response value against the opponent's average strategy, per deal.
    pub fn best_response_value(&self, p: Player) -> f64 {
        let values = br_pass(
            &self.game.tree,
            &self.game.evaluator,
            &self.storage,
            0,
            p,
            &self.game.root_ranges[p.opponent()],
        );
        self.root_aggregate(p, &values)
    }

    /// Per-player exploitability `BR_p(avg_{-p}) - u_p(avg)`. Reported
    /// separately per player because raked (general-sum) games are
    /// asymmetric; for zero-sum games the sum is NashConv and half the sum
    /// is the conventional exploitability.
    pub fn exploitability(&self) -> PerPlayer<f64> {
        PerPlayer::new(
            self.best_response_value(Player::P0) - self.expected_value(Player::P0),
            self.best_response_value(Player::P1) - self.expected_value(Player::P1),
        )
    }

    /// Normalized average strategy at an action node (`A*H`, action-major).
    pub fn average_strategy_at(&self, node: NodeId) -> Vec<f32> {
        self.node_strategy(node, |sref, out| self.storage.average_strategy(sref, out))
    }

    /// Current (regret-matching) strategy at an action node.
    pub fn current_strategy_at(&self, node: NodeId) -> Vec<f32> {
        self.node_strategy(node, |sref, out| self.storage.regret_matching(sref, out))
    }

    fn node_strategy(&self, node: NodeId, f: impl Fn(StorageRef, &mut [f32])) -> Vec<f32> {
        let n = self.game.tree.node(node);
        assert_eq!(n.kind, NodeKind::Action, "not an action node");
        let sref = self.game.tree.storage_ref(n);
        let mut out = vec![0.0; sref.len()];
        f(sref, &mut out);
        out
    }

    fn root_aggregate(&self, p: Player, values: &[f32]) -> f64 {
        let range = &self.game.root_ranges[p];
        let total: f64 = range
            .iter()
            .zip(values)
            .map(|(&r, &v)| r as f64 * v as f64)
            .sum();
        total / self.game.normalizer
    }
}

/// CFR update pass for player `p`. Returns p's counterfactual values, one
/// per hand in p's current private-state space.
///
/// M1 keeps per-call `Vec` scratch for clarity; arena reuse and rayon
/// parallelism over chance branches land with the holdem tree (M2), whose
/// storage layout they constrain.
#[allow(clippy::too_many_arguments)]
fn cfr_pass<E: TerminalEvaluator, V: StorageView>(
    tree: &PublicTree,
    evaluator: &E,
    storage: &mut V,
    node_id: NodeId,
    p: Player,
    my_reach: &[f32],
    opp_reach: &[f32],
    discounts: &Discounts,
) -> Vec<f32> {
    let node = *tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {
            let mut out = vec![0.0; my_reach.len()];
            evaluator.eval(node.aux, p, opp_reach, &mut out);
            out
        }
        NodeKind::Chance => {
            let mut out = vec![0.0; my_reach.len()];
            for (pos, child) in tree.children(node_id).enumerate() {
                let deal = *tree.deal(&node, pos);
                let my_next = tree.map_reach(deal.maps[p], my_reach);
                let opp_next = tree.map_reach(deal.maps[p.opponent()], opp_reach);
                let child_cfv = cfr_pass(
                    tree, evaluator, storage, child, p, &my_next, &opp_next, discounts,
                );
                tree.accumulate_values(deal.maps[p], deal.weight, &child_cfv, &mut out);
            }
            out
        }
        NodeKind::Action if node.player == p => {
            let sref = tree.storage_ref(&node);
            let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
            debug_assert_eq!(num_hands, my_reach.len());
            let mut sigma = vec![0.0f32; sref.len()];
            storage.regret_matching(sref, &mut sigma);

            let mut action_cfvs: Vec<Vec<f32>> = Vec::with_capacity(num_actions);
            for (a, child) in tree.children(node_id).enumerate() {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                let my_next: Vec<f32> = my_reach.iter().zip(row).map(|(&r, &s)| r * s).collect();
                action_cfvs.push(cfr_pass(
                    tree, evaluator, storage, child, p, &my_next, opp_reach, discounts,
                ));
            }

            let mut node_cfv = vec![0.0f32; num_hands];
            for (a, cfv) in action_cfvs.iter().enumerate() {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    node_cfv[h] += row[h] * cfv[h];
                }
            }

            let mut inst = vec![0.0f32; sref.len()];
            for (a, cfv) in action_cfvs.iter().enumerate() {
                for h in 0..num_hands {
                    inst[a * num_hands + h] = cfv[h] - node_cfv[h];
                }
            }
            storage.update_regrets(sref, &inst, discounts);

            // Reuse `inst` as the reach-weighted strategy buffer.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    inst[a * num_hands + h] = my_reach[h] * sigma[a * num_hands + h];
                }
            }
            storage.accumulate_strategy(sref, &inst, discounts);
            node_cfv
        }
        NodeKind::Action => {
            // Opponent's node: current strategy scales the opponent reach;
            // counterfactual values sum over their actions.
            let sref = tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            debug_assert_eq!(num_hands, opp_reach.len());
            let mut sigma = vec![0.0f32; sref.len()];
            storage.regret_matching(sref, &mut sigma);

            let mut out = vec![0.0f32; my_reach.len()];
            for (a, child) in tree.children(node_id).enumerate() {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                let opp_next: Vec<f32> = opp_reach.iter().zip(row).map(|(&r, &s)| r * s).collect();
                let child_cfv = cfr_pass(
                    tree, evaluator, storage, child, p, my_reach, &opp_next, discounts,
                );
                for h in 0..out.len() {
                    out[h] += child_cfv[h];
                }
            }
            out
        }
    }
}

/// Shared walk for expected-value and best-response computation: `p`'s own
/// nodes combine child values with `combine`; opponent nodes always follow
/// the opponent's average strategy.
#[allow(clippy::too_many_arguments)]
fn value_pass<E: TerminalEvaluator, S: Storage>(
    tree: &PublicTree,
    evaluator: &E,
    storage: &S,
    node_id: NodeId,
    p: Player,
    opp_reach: &[f32],
    my_dim: usize,
    combine: &impl Fn(StorageRef, &[Vec<f32>], &mut Vec<f32>),
) -> Vec<f32> {
    let node = *tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {
            let mut out = vec![0.0; my_dim];
            evaluator.eval(node.aux, p, opp_reach, &mut out);
            out
        }
        NodeKind::Chance => {
            let mut out = vec![0.0; my_dim];
            for (pos, child) in tree.children(node_id).enumerate() {
                let deal = *tree.deal(&node, pos);
                let opp_next = tree.map_reach(deal.maps[p.opponent()], opp_reach);
                let child_dim = match deal.maps[p] {
                    crate::tree::ReachMap::Transition(t) => {
                        tree.transitions[t as usize].out_dim as usize
                    }
                    _ => my_dim,
                };
                let child_v = value_pass(
                    tree, evaluator, storage, child, p, &opp_next, child_dim, combine,
                );
                tree.accumulate_values(deal.maps[p], deal.weight, &child_v, &mut out);
            }
            out
        }
        NodeKind::Action if node.player == p => {
            let sref = tree.storage_ref(&node);
            let children: Vec<Vec<f32>> = tree
                .children(node_id)
                .map(|child| {
                    value_pass(
                        tree, evaluator, storage, child, p, opp_reach, my_dim, combine,
                    )
                })
                .collect();
            let mut out = vec![0.0f32; my_dim];
            combine(sref, &children, &mut out);
            out
        }
        NodeKind::Action => {
            let sref = tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            let mut sigma = vec![0.0f32; sref.len()];
            storage.average_strategy(sref, &mut sigma);
            let mut out = vec![0.0f32; my_dim];
            for (a, child) in tree.children(node_id).enumerate() {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                let opp_next: Vec<f32> = opp_reach.iter().zip(row).map(|(&r, &s)| r * s).collect();
                let child_v = value_pass(
                    tree, evaluator, storage, child, p, &opp_next, my_dim, combine,
                );
                for h in 0..out.len() {
                    out[h] += child_v[h];
                }
            }
            out
        }
    }
}

/// Expected values for `p` when both players play their average strategy.
fn ev_pass<E: TerminalEvaluator, S: Storage>(
    tree: &PublicTree,
    evaluator: &E,
    storage: &S,
    node_id: NodeId,
    p: Player,
    opp_reach: &[f32],
) -> Vec<f32> {
    let my_dim = tree.root_dims[p] as usize;
    let combine = |sref: StorageRef, children: &[Vec<f32>], out: &mut Vec<f32>| {
        let num_hands = sref.num_hands as usize;
        let mut sigma = vec![0.0f32; sref.len()];
        storage.average_strategy(sref, &mut sigma);
        for (a, child) in children.iter().enumerate() {
            let row = &sigma[a * num_hands..(a + 1) * num_hands];
            for h in 0..out.len() {
                out[h] += row[h] * child[h];
            }
        }
    };
    value_pass(
        tree, evaluator, storage, node_id, p, opp_reach, my_dim, &combine,
    )
}

/// Best-response values for `p` against the opponent's average strategy:
/// per-hand max over actions (Johanson-style accelerated best response —
/// every hero hand is maximized simultaneously in one walk).
fn br_pass<E: TerminalEvaluator, S: Storage>(
    tree: &PublicTree,
    evaluator: &E,
    storage: &S,
    node_id: NodeId,
    p: Player,
    opp_reach: &[f32],
) -> Vec<f32> {
    let my_dim = tree.root_dims[p] as usize;
    let combine = |_sref: StorageRef, children: &[Vec<f32>], out: &mut Vec<f32>| {
        for h in 0..out.len() {
            out[h] = children
                .iter()
                .map(|c| c[h])
                .fold(f32::NEG_INFINITY, f32::max);
        }
    };
    value_pass(
        tree, evaluator, storage, node_id, p, opp_reach, my_dim, &combine,
    )
}
