use cards::{PerPlayer, Player};

use crate::schedule::{DiscountSchedule, Discounts};
use crate::scratch::Scratch;
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
    /// LIFO scratch pool for the CFR walk. A field separate from `storage`
    /// so `step` can hold `storage.view_mut()` and `&mut scratch`
    /// simultaneously without a borrow conflict.
    scratch: Scratch,
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
            scratch: Scratch::new(),
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
            let ctx = PassCtx {
                tree: &self.game.tree,
                evaluator: &self.game.evaluator,
                p,
                discounts: &discounts,
            };
            let mut out = self.scratch.take(self.game.tree.root_dims[p] as usize);
            cfr_pass(
                &ctx,
                &mut view,
                &mut self.scratch,
                0,
                &my_range,
                &opp_range,
                &mut out,
            );
            self.scratch.put(out);
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
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        ev_pass(
            &ctx,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
        );
        self.root_aggregate(p, &out)
    }

    /// Best-response value against the opponent's average strategy, per deal.
    pub fn best_response_value(&self, p: Player) -> f64 {
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        br_pass(
            &ctx,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
        );
        self.root_aggregate(p, &out)
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

/// Read-only context threaded through [`cfr_pass`]: everything about the
/// walk that doesn't change across a single pass.
struct PassCtx<'w, E> {
    tree: &'w PublicTree,
    evaluator: &'w E,
    p: Player,
    discounts: &'w Discounts,
}

/// CFR update pass for player `p`, writing p's counterfactual values (one
/// per hand in p's current private-state space) into `out`.
///
/// All temporaries come from `scratch`, taken and released in recursion
/// (stack) order so steady-state solving allocates nothing: every buffer a
/// call takes is released (in reverse order) before that call returns,
/// which is exactly the LIFO discipline [`Scratch`] relies on.
fn cfr_pass<E: TerminalEvaluator, V: StorageView>(
    ctx: &PassCtx<'_, E>,
    storage: &mut V,
    scratch: &mut Scratch,
    node_id: NodeId,
    my_reach: &[f32],
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let node = *ctx.tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {
            ctx.evaluator.eval(node.aux, ctx.p, opp_reach, out);
        }
        NodeKind::Chance => {
            // All deals off one chance node map into the same per-player
            // dimension (masks always preserve it; transitions are assumed
            // to by construction), so `my_next`/`opp_next`/`child_out` are
            // sized once and reused across deals instead of round-tripping
            // through the free list every iteration.
            let mut my_next = scratch.take(my_reach.len());
            let mut opp_next = scratch.take(opp_reach.len());
            let mut child_out = scratch.take(my_reach.len());
            for (pos, child) in ctx.tree.children(node_id).enumerate() {
                let deal = *ctx.tree.deal(&node, pos);
                ctx.tree
                    .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
                ctx.tree
                    .map_reach_into(deal.maps[ctx.p.opponent()], opp_reach, &mut opp_next);
                // `child_out` is an out-param accumulator target for the
                // recursive call: Chance/opponent-Action children only add
                // into it (they rely on the caller starting them at zero),
                // so a reused buffer must be reset every iteration, not
                // just at the initial `take`.
                child_out.fill(0.0);
                cfr_pass(
                    ctx,
                    storage,
                    scratch,
                    child,
                    &my_next,
                    &opp_next,
                    &mut child_out,
                );
                ctx.tree
                    .accumulate_values(deal.maps[ctx.p], deal.weight, &child_out, out);
            }
            scratch.put(child_out);
            scratch.put(opp_next);
            scratch.put(my_next);
        }
        NodeKind::Action if node.player == ctx.p => {
            let sref = ctx.tree.storage_ref(&node);
            let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
            debug_assert_eq!(num_hands, my_reach.len());

            let mut sigma = scratch.take(sref.len());
            storage.regret_matching(sref, &mut sigma);

            // One flat action-major buffer: action `a`'s row is that
            // action's own `out` parameter, so its recursion writes
            // straight into place instead of returning a fresh `Vec`.
            let mut cfvs = scratch.take(sref.len());
            let mut node_cfv = scratch.take(num_hands);
            // Reused across actions (not re-taken per action): its
            // recursive use always returns before the next action starts,
            // so overwriting it in place is safe.
            let mut my_next = scratch.take(num_hands);

            for (a, child) in ctx.tree.children(node_id).enumerate() {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    my_next[h] = my_reach[h] * row[h];
                }
                let out_row = &mut cfvs[a * num_hands..(a + 1) * num_hands];
                cfr_pass(ctx, storage, scratch, child, &my_next, opp_reach, out_row);
            }

            for a in 0..num_actions {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    node_cfv[h] += row[h] * cfvs[a * num_hands + h];
                }
            }

            // Instantaneous regret, computed in place.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] -= node_cfv[h];
                }
            }
            storage.update_regrets(sref, &cfvs, ctx.discounts);

            // Overwrite `cfvs` again as the reach-weighted strategy buffer.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] = my_reach[h] * sigma[a * num_hands + h];
                }
            }
            storage.accumulate_strategy(sref, &cfvs, ctx.discounts);

            out.copy_from_slice(&node_cfv);

            scratch.put(my_next);
            scratch.put(node_cfv);
            scratch.put(cfvs);
            scratch.put(sigma);
        }
        NodeKind::Action => {
            // Opponent's node: current strategy scales the opponent reach;
            // counterfactual values sum over their actions.
            let sref = ctx.tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            debug_assert_eq!(num_hands, opp_reach.len());
            let mut sigma = scratch.take(sref.len());
            storage.regret_matching(sref, &mut sigma);

            let mut opp_next = scratch.take(num_hands);
            let mut child_out = scratch.take(my_reach.len());
            for (a, child) in ctx.tree.children(node_id).enumerate() {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    opp_next[h] = opp_reach[h] * row[h];
                }
                // See the Chance-node comment: reused accumulator targets
                // must be reset before every use, not just the initial take.
                child_out.fill(0.0);
                cfr_pass(
                    ctx,
                    storage,
                    scratch,
                    child,
                    my_reach,
                    &opp_next,
                    &mut child_out,
                );
                for h in 0..out.len() {
                    out[h] += child_out[h];
                }
            }
            scratch.put(child_out);
            scratch.put(opp_next);
            scratch.put(sigma);
        }
    }
}

/// Read-only context threaded through [`value_pass`].
struct ValueCtx<'w, E, S> {
    tree: &'w PublicTree,
    evaluator: &'w E,
    storage: &'w S,
    p: Player,
}

/// Shared walk for expected-value and best-response computation: `p`'s own
/// nodes combine child values with `combine`; opponent nodes always follow
/// the opponent's average strategy. Writes `p`'s values at this node into
/// `out` (dimension implied by `out.len()`).
fn value_pass<E: TerminalEvaluator, S: Storage>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    out: &mut [f32],
    combine: &impl Fn(StorageRef, &[f32], &mut [f32]),
) {
    let node = *ctx.tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {
            ctx.evaluator.eval(node.aux, ctx.p, opp_reach, out);
        }
        NodeKind::Chance => {
            let my_dim = out.len();
            let mut opp_next = scratch.take(opp_reach.len());
            let mut child_out = scratch.take(my_dim);
            for (pos, child) in ctx.tree.children(node_id).enumerate() {
                let deal = *ctx.tree.deal(&node, pos);
                ctx.tree
                    .map_reach_into(deal.maps[ctx.p.opponent()], opp_reach, &mut opp_next);
                // Reused accumulator target: reset before every use (see
                // the matching comment in `cfr_pass`).
                child_out.fill(0.0);
                value_pass(ctx, scratch, child, &opp_next, &mut child_out, combine);
                ctx.tree
                    .accumulate_values(deal.maps[ctx.p], deal.weight, &child_out, out);
            }
            scratch.put(child_out);
            scratch.put(opp_next);
        }
        NodeKind::Action if node.player == ctx.p => {
            let sref = ctx.tree.storage_ref(&node);
            let my_dim = out.len();
            let num_actions = sref.num_actions as usize;
            let mut children_flat = scratch.take(num_actions * my_dim);
            for (a, child) in ctx.tree.children(node_id).enumerate() {
                let row = &mut children_flat[a * my_dim..(a + 1) * my_dim];
                value_pass(ctx, scratch, child, opp_reach, row, combine);
            }
            combine(sref, &children_flat, out);
            scratch.put(children_flat);
        }
        NodeKind::Action => {
            let sref = ctx.tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            let mut sigma = scratch.take(sref.len());
            ctx.storage.average_strategy(sref, &mut sigma);
            let my_dim = out.len();
            let mut opp_next = scratch.take(num_hands);
            let mut child_out = scratch.take(my_dim);
            for (a, child) in ctx.tree.children(node_id).enumerate() {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    opp_next[h] = opp_reach[h] * row[h];
                }
                // Reused accumulator target: reset before every use (see
                // the matching comment in `cfr_pass`).
                child_out.fill(0.0);
                value_pass(ctx, scratch, child, &opp_next, &mut child_out, combine);
                for h in 0..my_dim {
                    out[h] += child_out[h];
                }
            }
            scratch.put(child_out);
            scratch.put(opp_next);
            scratch.put(sigma);
        }
    }
}

/// Expected values for `p` when both players play their average strategy.
fn ev_pass<E: TerminalEvaluator, S: Storage>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let combine = |sref: StorageRef, children_flat: &[f32], out: &mut [f32]| {
        let num_hands = sref.num_hands as usize;
        let mut sigma = vec![0.0f32; sref.len()];
        ctx.storage.average_strategy(sref, &mut sigma);
        for a in 0..sref.num_actions as usize {
            let row = &sigma[a * num_hands..(a + 1) * num_hands];
            let child = &children_flat[a * num_hands..(a + 1) * num_hands];
            for h in 0..out.len() {
                out[h] += row[h] * child[h];
            }
        }
    };
    value_pass(ctx, scratch, node_id, opp_reach, out, &combine);
}

/// Best-response values for `p` against the opponent's average strategy:
/// per-hand max over actions (Johanson-style accelerated best response —
/// every hero hand is maximized simultaneously in one walk).
fn br_pass<E: TerminalEvaluator, S: Storage>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let combine = |sref: StorageRef, children_flat: &[f32], out: &mut [f32]| {
        let num_hands = sref.num_hands as usize;
        for h in 0..out.len() {
            out[h] = (0..sref.num_actions as usize)
                .map(|a| children_flat[a * num_hands + h])
                .fold(f32::NEG_INFINITY, f32::max);
        }
    };
    value_pass(ctx, scratch, node_id, opp_reach, out, &combine);
}
