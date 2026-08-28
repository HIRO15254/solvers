use std::sync::Mutex;

use cards::{PerPlayer, Player};
use rayon::prelude::*;

use crate::schedule::{DiscountSchedule, Discounts};
use crate::scratch::Scratch;
use crate::storage::{StateMismatch, Storage, StorageRef, StorageSpan, StorageState, StorageView};
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
    /// True when baked terminal utilities are exactly zero-sum (set by
    /// builders from `PayoffPipeline::is_zero_sum`). Enables deriving P1's
    /// expected value as -P0's instead of a second walk — an optimization,
    /// never an assumption: raked games leave it false and get the full
    /// general-sum accounting.
    pub zero_sum: bool,
}

/// Thresholds controlling rayon fan-out over chance-node children.
///
/// `chance_depth` bounds how many chance-node crossings (from the root)
/// remain eligible for parallel fan-out; it decrements at every chance node
/// regardless of whether that node actually parallelizes, so it reads as
/// "top N chance levels". `min_children` is the per-node fan-out threshold
/// (games with few chance children per node, like Kuhn's none or Leduc's six,
/// stay sequential even inside the budgeted depth).
#[derive(Clone, Copy, Debug)]
pub struct ParConfig {
    pub chance_depth: u32,
    pub min_children: usize,
}

impl Default for ParConfig {
    /// Turn+river levels; Kuhn (no chance node) and Leduc (6-way deal) stay
    /// sequential via `min_children`.
    fn default() -> Self {
        ParConfig {
            chance_depth: 2,
            min_children: 12,
        }
    }
}

/// Checkpointable solver state: the iteration count plus the storage
/// backend's contents, sufficient to resume a solve bit-for-bit (the
/// solver has no RNG or other hidden state).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolverState {
    pub iteration: u64,
    pub storage: StorageState,
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
    par: ParConfig,
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
        let storage = S::new(game.tree.storage_len, game.tree.storage_refs.len());
        Solver {
            game,
            storage,
            scratch: Scratch::new(),
            schedule,
            planned_iters,
            iteration: 0,
            par: ParConfig::default(),
        }
    }

    /// Overrides the chance-node parallelism thresholds (default:
    /// [`ParConfig::default`]). Additive on top of [`Solver::new`] so every
    /// existing call site keeps working unchanged.
    pub fn set_par(&mut self, par: ParConfig) {
        self.par = par;
    }

    pub fn iteration(&self) -> u64 {
        self.iteration
    }

    pub fn game(&self) -> &CompiledGame<E> {
        &self.game
    }

    /// Read-only access to the raw storage backend, e.g. to snapshot
    /// regrets/strategy sums for a determinism check.
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Snapshots the iteration count and storage backend contents for a
    /// checkpoint.
    pub fn state(&self) -> SolverState {
        SolverState {
            iteration: self.iteration,
            storage: self.storage.state(),
        }
    }

    /// Restores a previously-snapshotted iteration count and storage
    /// backend contents. Fails (leaving `self` unchanged) if `state.storage`
    /// doesn't match this solver's storage backend.
    pub fn restore_state(&mut self, state: SolverState) -> Result<(), StateMismatch> {
        self.storage.restore_state(state.storage)?;
        self.iteration = state.iteration;
        Ok(())
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
                par: self.par,
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
                self.par.chance_depth,
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
            par: self.par,
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        ev_pass(
            &ctx,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
            self.par.chance_depth,
        );
        self.root_aggregate(p, &out)
    }

    /// Per-hand counterfactual values of the average profile for `p` at
    /// `node`, given the reach vectors *at that node*.
    ///
    /// [`Self::expected_value`] is this at the root, aggregated against the
    /// root range. Deeper nodes need reaches the caller computes with
    /// [`crate::reach_at`], because a node's values are only meaningful
    /// against the range that actually arrives there — the root range would
    /// answer a different question. Passing both players' reaches also
    /// gives the hand count on each side, which the tree does not store per
    /// node.
    ///
    /// The result is one value per hand of `p`, on the same basis as the
    /// solve's payoffs. Callers that report EV re-base it themselves (for
    /// postflop, onto the subgame-start basis).
    pub fn expected_values_at(
        &self,
        node: NodeId,
        p: Player,
        reach: PerPlayer<&[f32]>,
    ) -> Vec<f32> {
        self.values_at(node, p, reach, ev_pass)
    }

    /// Per-hand best-response values for `p` at `node` against the
    /// opponent's average strategy. Subtracting
    /// [`Self::expected_values_at`] gives per-hand regret at that node.
    pub fn best_response_values_at(
        &self,
        node: NodeId,
        p: Player,
        reach: PerPlayer<&[f32]>,
    ) -> Vec<f32> {
        self.values_at(node, p, reach, br_pass)
    }

    fn values_at(
        &self,
        node: NodeId,
        p: Player,
        reach: PerPlayer<&[f32]>,
        pass: ValuePass<E, S>,
    ) -> Vec<f32> {
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
            par: self.par,
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(reach[p].len());
        pass(
            &ctx,
            &mut scratch,
            node,
            reach[p.opponent()],
            &mut out,
            self.par.chance_depth,
        );
        out.to_vec()
    }

    /// Per-hand values of the average profile for `p` at **every** action
    /// node, indexed by `StorageRef::index`, in one pass.
    ///
    /// [`Self::expected_values_at`] answers one node and costs a walk of
    /// that node's whole subtree, so asking it for every node would be
    /// quadratic. An artifact that stores values for every node needs this
    /// instead: the value pass already computes each node's vector on its
    /// way back up, and this records them as it goes.
    ///
    /// Entries are `None` for storage refs the pass never reached.
    pub fn expected_values_everywhere(&self, p: Player) -> Vec<Option<Vec<f32>>> {
        let recorded = Mutex::new(vec![None; self.game.tree.storage_refs.len()]);
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
            par: self.par,
        };
        let combine = |sref: StorageRef, children_flat: &[f32], out: &mut [f32]| {
            let num_hands = sref.num_hands as usize;
            let mut sigma = vec![0.0f32; sref.len()];
            ctx.storage.average_strategy(sref, sref.index, &mut sigma);
            for a in 0..sref.num_actions as usize {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                let child = &children_flat[a * num_hands..(a + 1) * num_hands];
                for h in 0..out.len() {
                    out[h] += row[h] * child[h];
                }
            }
        };
        let record = |node: NodeId, values: &[f32]| {
            let sref = ctx.tree.storage_ref(ctx.tree.node(node));
            recorded.lock().expect("value recorder mutex")[sref.index as usize] =
                Some(values.to_vec());
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        value_pass(
            &ctx,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
            &combine,
            &record,
            self.par.chance_depth,
        );
        recorded.into_inner().expect("value recorder mutex")
    }

    /// Best-response value against the opponent's average strategy, per deal.
    pub fn best_response_value(&self, p: Player) -> f64 {
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
            par: self.par,
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        br_pass(
            &ctx,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
            self.par.chance_depth,
        );
        self.root_aggregate(p, &out)
    }

    /// Per-player exploitability `BR_p(avg_{-p}) - u_p(avg)`. Reported
    /// separately per player because raked (general-sum) games are
    /// asymmetric; for zero-sum games the sum is NashConv and half the sum
    /// is the conventional exploitability.
    pub fn exploitability(&self) -> PerPlayer<f64> {
        let ev0 = self.expected_value(Player::P0);
        let ev1 = if self.game.zero_sum {
            -ev0
        } else {
            self.expected_value(Player::P1)
        };
        PerPlayer::new(
            self.best_response_value(Player::P0) - ev0,
            self.best_response_value(Player::P1) - ev1,
        )
    }

    /// Normalized average strategy at an action node (`A*H`, action-major).
    pub fn average_strategy_at(&self, node: NodeId) -> Vec<f32> {
        self.node_strategy(node, |sref, out| {
            self.storage.average_strategy(sref, sref.index, out)
        })
    }

    /// Current (regret-matching) strategy at an action node.
    pub fn current_strategy_at(&self, node: NodeId) -> Vec<f32> {
        self.node_strategy(node, |sref, out| {
            self.storage.regret_matching(sref, sref.index, out)
        })
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
    par: ParConfig,
}

/// Storage access for an action node's own storage ref (when split with
/// one) and each of its children, while it processes them.
///
/// A chance node with enough children may run its children in parallel,
/// which calls `StorageView::split` on whatever view it's handed. Ordinary
/// Rust reborrowing means that, left unprotected, that view is the *same
/// object* every action-node ancestor up to the root is also holding —
/// `split` permanently empties it (see its doc comment), so an ancestor
/// that reuses its ambient view afterward (every "my player" node, for its
/// own regret/strategy update; every sibling after the first, for its own
/// subtree) would find it gone.
///
/// [`PublicTree::subtree_has_chance`] tells us exactly which nodes can
/// avoid worrying about this: `Ambient` is the original, allocation-free
/// path (one view reborrowed across every access), taken whenever nothing
/// below can possibly split; `Split` carves out one independent view per
/// child (plus, when constructed with an own span, this node's own
/// storage-ref slice as `views[0]`) so a deep split can only ever consume
/// a child's own piece.
enum ActionViews<'v, V> {
    Ambient(&'v mut V),
    Split { views: Vec<V>, has_own: bool },
}

impl<'v, V: StorageView> ActionViews<'v, V> {
    fn split_for(
        storage: &'v mut V,
        tree: &PublicTree,
        node_id: NodeId,
        own_span: Option<StorageSpan>,
        par_budget: u32,
    ) -> Self {
        if par_budget == 0 || !tree.subtree_has_chance[node_id as usize] {
            return ActionViews::Ambient(storage);
        }
        let has_own = own_span.is_some();
        let num_children = tree.node(node_id).num_children as usize;
        let mut spans: Vec<StorageSpan> = Vec::with_capacity(has_own as usize + num_children);
        spans.extend(own_span);
        spans.extend(
            tree.children(node_id)
                .map(|id| tree.storage_spans[id as usize]),
        );
        ActionViews::Split {
            views: storage.split(&spans),
            has_own,
        }
    }

    /// This node's own view. Only valid when constructed with `own_span:
    /// Some(_)`.
    fn own(&mut self) -> &mut V {
        match self {
            ActionViews::Ambient(v) => v,
            ActionViews::Split { views, .. } => &mut views[0],
        }
    }

    /// The `index`-th child's view.
    fn child(&mut self, index: usize) -> &mut V {
        match self {
            ActionViews::Ambient(v) => v,
            ActionViews::Split { views, has_own } => &mut views[index + *has_own as usize],
        }
    }
}

/// CFR update pass for player `p`, writing p's counterfactual values (one
/// per hand in p's current private-state space) into `out`.
///
/// All temporaries come from `scratch`, taken and released in recursion
/// (stack) order so steady-state solving allocates nothing: every buffer a
/// call takes is released (in reverse order) before that call returns,
/// which is exactly the LIFO discipline [`Scratch`] relies on.
#[allow(clippy::too_many_arguments)]
fn cfr_pass<E: TerminalEvaluator, V: StorageView>(
    ctx: &PassCtx<'_, E>,
    storage: &mut V,
    scratch: &mut Scratch,
    node_id: NodeId,
    my_reach: &[f32],
    opp_reach: &[f32],
    out: &mut [f32],
    par_budget: u32,
) {
    let node = *ctx.tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {
            ctx.evaluator.eval(node.aux, ctx.p, opp_reach, out);
        }
        NodeKind::Chance => {
            // `chance_depth` bounds how many chance-node crossings remain
            // eligible for fan-out, so it decrements here regardless of
            // whether this particular node ends up parallelizing.
            let child_budget = par_budget.saturating_sub(1);
            if par_budget > 0 && node.num_children as usize >= ctx.par.min_children {
                // Chance nodes own no storage themselves: split `storage`
                // into one disjoint view per child up front and fan out.
                // After the split the parent `storage` view is empty (see
                // `StorageView::split`) and must not be touched again in
                // this stack frame — every remaining access below goes
                // through the per-child views instead.
                let child_ids: Vec<NodeId> = ctx.tree.children(node_id).collect();
                let spans: Vec<StorageSpan> = child_ids
                    .iter()
                    .map(|&id| ctx.tree.storage_spans[id as usize])
                    .collect();
                let views = storage.split(&spans);
                // Indexed (ordered) collect, then in-child-order fold below:
                // together with the disjoint per-child storage views and an
                // unchanged per-child op sequence, this keeps the parallel
                // pass bitwise identical to the sequential one. Do not
                // switch this to `reduce`, which does not guarantee order.
                let results: Vec<Vec<f32>> = child_ids
                    .into_par_iter()
                    .zip(views)
                    .enumerate()
                    .map_init(Scratch::new, |scratch, (pos, (child, mut view))| {
                        let deal = *ctx.tree.deal(&node, pos);
                        // Each child's own deal maps decide its dimensions:
                        // a `Transition` may change them, and different
                        // deals off the same chance node may map into
                        // different dimensions (see `PublicTree::mapped_dim`
                        // and `ReachMap`'s doc comment), so these must come
                        // from this child's `deal`, not from `my_reach`/
                        // `opp_reach`'s own (parent) lengths.
                        let my_dim =
                            ctx.tree.mapped_dim(deal.maps[ctx.p], my_reach.len() as u32) as usize;
                        let opp_dim = ctx
                            .tree
                            .mapped_dim(deal.maps[ctx.p.opponent()], opp_reach.len() as u32)
                            as usize;
                        let mut my_next = scratch.take(my_dim);
                        let mut opp_next = scratch.take(opp_dim);
                        ctx.tree
                            .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
                        ctx.tree.map_reach_into(
                            deal.maps[ctx.p.opponent()],
                            opp_reach,
                            &mut opp_next,
                        );
                        // Child values live in the mapped my-space: same
                        // length as `my_next`.
                        let mut child_out = scratch.take(my_dim);
                        cfr_pass(
                            ctx,
                            &mut view,
                            scratch,
                            child,
                            &my_next,
                            &opp_next,
                            &mut child_out,
                            child_budget,
                        );
                        scratch.put(opp_next);
                        scratch.put(my_next);
                        child_out
                    })
                    .collect();
                for (pos, child_out) in results.into_iter().enumerate() {
                    let deal = *ctx.tree.deal(&node, pos);
                    ctx.tree
                        .accumulate_values(deal.maps[ctx.p], deal.weight, &child_out, out);
                }
            } else {
                // Different deals off the same chance node may map into
                // different per-player dimensions (a `Transition` need not
                // preserve dimension, and each deal carries its own maps —
                // see `PublicTree::mapped_dim`), so `my_next`/`opp_next`/
                // `child_out` are sized per deal and taken/put inside the
                // loop instead of hoisted above it.
                //
                // See `ActionViews`: a deeper chance node still eligible to
                // parallelize (`child_budget > 0`) could split whatever
                // view a sibling deal is holding, so siblings need
                // independent views too, not just the parallel branch's
                // own children.
                let mut views =
                    ActionViews::split_for(storage, ctx.tree, node_id, None, child_budget);
                for (pos, child) in ctx.tree.children(node_id).enumerate() {
                    let deal = *ctx.tree.deal(&node, pos);
                    let my_dim =
                        ctx.tree.mapped_dim(deal.maps[ctx.p], my_reach.len() as u32) as usize;
                    let opp_dim = ctx
                        .tree
                        .mapped_dim(deal.maps[ctx.p.opponent()], opp_reach.len() as u32)
                        as usize;
                    let mut my_next = scratch.take(my_dim);
                    let mut opp_next = scratch.take(opp_dim);
                    ctx.tree
                        .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
                    ctx.tree
                        .map_reach_into(deal.maps[ctx.p.opponent()], opp_reach, &mut opp_next);
                    // `child_out` is a fresh (already-zeroed by `take`)
                    // out-param accumulator target for the recursive call:
                    // Chance/opponent-Action children only add into it (they
                    // rely on the caller starting them at zero). Same length
                    // as `my_next`: child values live in the mapped my-space.
                    let mut child_out = scratch.take(my_dim);
                    cfr_pass(
                        ctx,
                        views.child(pos),
                        scratch,
                        child,
                        &my_next,
                        &opp_next,
                        &mut child_out,
                        child_budget,
                    );
                    ctx.tree
                        .accumulate_values(deal.maps[ctx.p], deal.weight, &child_out, out);
                    scratch.put(child_out);
                    scratch.put(opp_next);
                    scratch.put(my_next);
                }
            }
        }
        NodeKind::Action if node.player == ctx.p => {
            let sref = ctx.tree.storage_ref(&node);
            let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
            debug_assert_eq!(num_hands, my_reach.len());

            // See `ActionViews`: this node's own regret/strategy update
            // happens after every child returns, so it needs a span
            // protected from any descendant chance node's parallel split.
            // `sref_start`/`sref_end` cover exactly this node's own ref
            // (`sref.index`, equal to `node.aux`) so a quantized backend's
            // `StorageView::split` can carve out this node's single scale
            // slot along with its element range.
            let own_span = StorageSpan {
                start: sref.offset,
                end: sref.offset + sref.len(),
                sref_start: sref.index,
                sref_end: sref.index + 1,
            };
            let mut views =
                ActionViews::split_for(storage, ctx.tree, node_id, Some(own_span), par_budget);

            let mut sigma = scratch.take(sref.len());
            views.own().regret_matching(sref, sref.index, &mut sigma);

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
                cfr_pass(
                    ctx,
                    views.child(a),
                    scratch,
                    child,
                    &my_next,
                    opp_reach,
                    out_row,
                    par_budget,
                );
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
            views
                .own()
                .update_regrets(sref, sref.index, &cfvs, ctx.discounts);

            // Overwrite `cfvs` again as the reach-weighted strategy buffer.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] = my_reach[h] * sigma[a * num_hands + h];
                }
            }
            views
                .own()
                .accumulate_strategy(sref, sref.index, &cfvs, ctx.discounts);

            out.copy_from_slice(&node_cfv);

            scratch.put(my_next);
            scratch.put(node_cfv);
            scratch.put(cfvs);
            scratch.put(sigma);
        }
        NodeKind::Action => {
            // Opponent's node: current strategy scales the opponent reach;
            // counterfactual values sum over their actions. `storage` is
            // read (never updated) before any child is touched, so the
            // read itself never needs protecting from a descendant's
            // split — only the per-child recursion below does, against a
            // *sibling's* descendant split (see `ActionViews`).
            let sref = ctx.tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            debug_assert_eq!(num_hands, opp_reach.len());
            let mut sigma = scratch.take(sref.len());
            storage.regret_matching(sref, sref.index, &mut sigma);

            let mut views = ActionViews::split_for(storage, ctx.tree, node_id, None, par_budget);

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
                    views.child(a),
                    scratch,
                    child,
                    my_reach,
                    &opp_next,
                    &mut child_out,
                    par_budget,
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
///
/// `pub(crate)` so [`crate::mccfr::McSolver`] can reuse `ev_pass`/`br_pass`
/// for exact evaluation of its average strategy instead of duplicating the
/// walk.
pub(crate) struct ValueCtx<'w, E, S> {
    pub(crate) tree: &'w PublicTree,
    pub(crate) evaluator: &'w E,
    pub(crate) storage: &'w S,
    pub(crate) p: Player,
    pub(crate) par: ParConfig,
}

/// Shared walk for expected-value and best-response computation: `p`'s own
/// nodes combine child values with `combine`; opponent nodes always follow
/// the opponent's average strategy. Writes `p`'s values at this node into
/// `out` (dimension implied by `out.len()`).
#[allow(clippy::too_many_arguments)]
fn value_pass<E: TerminalEvaluator, S: Storage, C, R>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    out: &mut [f32],
    combine: &C,
    record: &R,
    par_budget: u32,
) where
    C: Fn(StorageRef, &[f32], &mut [f32]) + Sync,
    R: Fn(NodeId, &[f32]) + Sync,
{
    let node = *ctx.tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {
            ctx.evaluator.eval(node.aux, ctx.p, opp_reach, out);
        }
        NodeKind::Chance => {
            // `out`'s dimension is this node's own (parent) p-space; each
            // deal's own maps decide the *child* dimensions below (a
            // `Transition` may change them, and different deals off the
            // same chance node may map into different dimensions — see
            // `PublicTree::mapped_dim` and `ReachMap`'s doc comment), so
            // `out.len()` must not be reused to size `child_out`.
            let parent_dim = out.len() as u32;
            // See the matching comment in `cfr_pass`: the budget decrements
            // at every chance node regardless of whether it parallelizes.
            let child_budget = par_budget.saturating_sub(1);
            if par_budget > 0 && node.num_children as usize >= ctx.par.min_children {
                // Storage here is a shared `&S` (Sync), so unlike `cfr_pass`
                // there's nothing to split — every task just reads through
                // the same reference. Ordered collect + in-order fold below
                // keeps this bitwise identical to the sequential fold.
                let child_ids: Vec<NodeId> = ctx.tree.children(node_id).collect();
                let results: Vec<Vec<f32>> = child_ids
                    .into_par_iter()
                    .enumerate()
                    .map_init(Scratch::new, |scratch, (pos, child)| {
                        let deal = *ctx.tree.deal(&node, pos);
                        let opp_dim = ctx
                            .tree
                            .mapped_dim(deal.maps[ctx.p.opponent()], opp_reach.len() as u32)
                            as usize;
                        let my_dim = ctx.tree.mapped_dim(deal.maps[ctx.p], parent_dim) as usize;
                        let mut opp_next = scratch.take(opp_dim);
                        ctx.tree.map_reach_into(
                            deal.maps[ctx.p.opponent()],
                            opp_reach,
                            &mut opp_next,
                        );
                        let mut child_out = scratch.take(my_dim);
                        value_pass(
                            ctx,
                            scratch,
                            child,
                            &opp_next,
                            &mut child_out,
                            combine,
                            record,
                            child_budget,
                        );
                        scratch.put(opp_next);
                        child_out
                    })
                    .collect();
                for (pos, child_out) in results.into_iter().enumerate() {
                    let deal = *ctx.tree.deal(&node, pos);
                    ctx.tree
                        .accumulate_values(deal.maps[ctx.p], deal.weight, &child_out, out);
                }
            } else {
                // Sized per deal (see the comment above `parent_dim`), not
                // hoisted, so `my_next`/`child_out` are taken and put inside
                // the loop instead of once for the whole node.
                for (pos, child) in ctx.tree.children(node_id).enumerate() {
                    let deal = *ctx.tree.deal(&node, pos);
                    let opp_dim = ctx
                        .tree
                        .mapped_dim(deal.maps[ctx.p.opponent()], opp_reach.len() as u32)
                        as usize;
                    let my_dim = ctx.tree.mapped_dim(deal.maps[ctx.p], parent_dim) as usize;
                    let mut opp_next = scratch.take(opp_dim);
                    ctx.tree
                        .map_reach_into(deal.maps[ctx.p.opponent()], opp_reach, &mut opp_next);
                    // Freshly taken (already zeroed by `take`) each deal.
                    let mut child_out = scratch.take(my_dim);
                    value_pass(
                        ctx,
                        scratch,
                        child,
                        &opp_next,
                        &mut child_out,
                        combine,
                        record,
                        child_budget,
                    );
                    ctx.tree
                        .accumulate_values(deal.maps[ctx.p], deal.weight, &child_out, out);
                    scratch.put(child_out);
                    scratch.put(opp_next);
                }
            }
        }
        NodeKind::Action if node.player == ctx.p => {
            let sref = ctx.tree.storage_ref(&node);
            let my_dim = out.len();
            let num_actions = sref.num_actions as usize;
            let mut children_flat = scratch.take(num_actions * my_dim);
            for (a, child) in ctx.tree.children(node_id).enumerate() {
                let row = &mut children_flat[a * my_dim..(a + 1) * my_dim];
                value_pass(
                    ctx, scratch, child, opp_reach, row, combine, record, par_budget,
                );
            }
            combine(sref, &children_flat, out);
            scratch.put(children_flat);
        }
        NodeKind::Action => {
            let sref = ctx.tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            let mut sigma = scratch.take(sref.len());
            ctx.storage.average_strategy(sref, sref.index, &mut sigma);
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
                value_pass(
                    ctx,
                    scratch,
                    child,
                    &opp_next,
                    &mut child_out,
                    combine,
                    record,
                    par_budget,
                );
                for h in 0..my_dim {
                    out[h] += child_out[h];
                }
            }
            scratch.put(child_out);
            scratch.put(opp_next);
            scratch.put(sigma);
        }
    }
    // Both action branches have `out` finished by here, whoever is to act,
    // so a caller recording per-node values sees every action node exactly
    // once. `combine` cannot do this: it only fires on `ctx.p`'s own nodes.
    if node.kind == NodeKind::Action {
        record(node_id, out);
    }
}

/// The recorder [`value_pass`] uses when the caller only wants the root
/// aggregate. Monomorphization compiles it away.
fn no_record(_: NodeId, _: &[f32]) {}

/// Expected values for `p` when both players play their average strategy.
/// One walk of the tree that fills per-hand values: [`ev_pass`] for the
/// average profile, [`br_pass`] for a best response. Named so
/// [`Solver::values_at`] can take either without spelling the signature out.
type ValuePass<E, S> = fn(&ValueCtx<'_, E, S>, &mut Scratch, NodeId, &[f32], &mut [f32], u32);

pub(crate) fn ev_pass<E: TerminalEvaluator, S: Storage>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    out: &mut [f32],
    par_budget: u32,
) {
    let combine = |sref: StorageRef, children_flat: &[f32], out: &mut [f32]| {
        let num_hands = sref.num_hands as usize;
        let mut sigma = vec![0.0f32; sref.len()];
        ctx.storage.average_strategy(sref, sref.index, &mut sigma);
        for a in 0..sref.num_actions as usize {
            let row = &sigma[a * num_hands..(a + 1) * num_hands];
            let child = &children_flat[a * num_hands..(a + 1) * num_hands];
            for h in 0..out.len() {
                out[h] += row[h] * child[h];
            }
        }
    };
    value_pass(
        ctx, scratch, node_id, opp_reach, out, &combine, &no_record, par_budget,
    );
}

/// Best-response values for `p` against the opponent's average strategy:
/// per-hand max over actions (Johanson-style accelerated best response —
/// every hero hand is maximized simultaneously in one walk).
pub(crate) fn br_pass<E: TerminalEvaluator, S: Storage>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    out: &mut [f32],
    par_budget: u32,
) {
    let combine = |sref: StorageRef, children_flat: &[f32], out: &mut [f32]| {
        let num_hands = sref.num_hands as usize;
        for h in 0..out.len() {
            out[h] = (0..sref.num_actions as usize)
                .map(|a| children_flat[a * num_hands + h])
                .fold(f32::NEG_INFINITY, f32::max);
        }
    };
    value_pass(
        ctx, scratch, node_id, opp_reach, out, &combine, &no_record, par_budget,
    );
}
