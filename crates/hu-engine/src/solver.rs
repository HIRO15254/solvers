use std::sync::Mutex;

use nlh::{PerPlayer, Player};
use rayon::prelude::*;

use crate::CfrPrecision;
use crate::schedule::{DiscountSchedule, Discounts};
use crate::scratch::{Scratch, with_worker_scratch};
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
    /// CFR-only terminal hook; evaluation and saved EVs always call `eval`.
    fn eval_cfr(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        self.eval(terminal, p, opp_reach, out);
    }
    /// Select the arithmetic of `eval_cfr`. Evaluators without a relaxed
    /// kernel ignore it; `Solver::set_cfr_precision` forwards here.
    fn set_cfr_precision(&mut self, _precision: CfrPrecision) {}
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
    /// expected value as -P0's during exploitability accounting — an optimization,
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
/// stay sequential even inside the budgeted depth). Large action subtrees
/// use an internal size threshold independently of these chance settings.
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

// Require at least two substantial child subtrees. Storage is only a work
// proxy (terminal kernels may dominate), so keep this conservative and fixed.
const ACTION_PAR_MIN_ELEMENTS: usize = 16_384;

fn subtree_elements(tree: &PublicTree, node: NodeId) -> usize {
    let span = tree.storage_spans[node as usize];
    span.end - span.start
}

fn parallel_actions(tree: &PublicTree, node: NodeId) -> bool {
    rayon::current_num_threads() > 1
        && tree
            .children(node)
            .filter(|&child| subtree_elements(tree, child) >= ACTION_PAR_MIN_ELEMENTS)
            .take(2)
            .count()
            == 2
}

// Mutable slices are metadata only; all child values occupy one flat buffer.
// Deal dimensions can differ, so ordinary par_chunks_mut cannot be used.
fn chance_rows<'a>(
    tree: &PublicTree,
    node: NodeId,
    p: Player,
    dim: usize,
    channels: usize,
    flat: &'a mut [f32],
) -> Vec<&'a mut [f32]> {
    let n = tree.node(node);
    let mut rest = flat;
    (0..n.num_children as usize)
        .map(|pos| {
            let len = tree.mapped_dim(tree.deal(n, pos).maps[p], dim as u32) as usize * channels;
            let (row, tail) = std::mem::take(&mut rest).split_at_mut(len);
            rest = tail;
            row
        })
        .collect()
}

fn chance_len(tree: &PublicTree, node: NodeId, p: Player, dim: usize) -> usize {
    let n = tree.node(node);
    (0..n.num_children as usize)
        .map(|pos| tree.mapped_dim(tree.deal(n, pos).maps[p], dim as u32) as usize)
        .sum()
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
    cfr_precision: CfrPrecision,
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
            cfr_precision: CfrPrecision::F64,
        }
    }

    /// Overrides the chance-node parallelism thresholds (default:
    /// [`ParConfig::default`]). Additive on top of [`Solver::new`] so every
    /// existing call site keeps working unchanged.
    pub fn set_par(&mut self, par: ParConfig) {
        self.par = par;
    }

    /// Select CFR terminal and current-strategy arithmetic for both this
    /// solver and its evaluator; evaluation and averages stay exact.
    pub fn set_cfr_precision(&mut self, precision: CfrPrecision) {
        self.cfr_precision = precision;
        self.game.evaluator.set_cfr_precision(precision);
    }

    pub fn iteration(&self) -> u64 {
        self.iteration
    }

    pub fn game(&self) -> &CompiledGame<E> {
        &self.game
    }

    /// Permanently frees regrets after the final checkpoint. Average strategy,
    /// evaluation and EV passes remain available. Running iterations, reading
    /// current strategy, snapshotting/checkpointing or restoring then panics.
    /// Create a new solver to resume from a previously saved checkpoint.
    pub fn release_regrets(&mut self) {
        self.storage.release_regrets();
    }

    /// Read-only access to the raw storage backend, e.g. to snapshot
    /// regrets/strategy sums for a determinism check.
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Restore directly into the existing arenas. Iteration is committed only
    /// after the reader succeeds; discard this solver if the reader fails.
    pub fn restore_stream<T, Err>(
        &mut self,
        iteration: u64,
        read: impl FnOnce(&mut S) -> Result<T, Err>,
    ) -> Result<T, Err> {
        self.storage.assert_regrets_available();
        let result = read(&mut self.storage)?;
        self.iteration = iteration;
        Ok(result)
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
        self.step_impl::<true>();
    }

    // The unpruned instantiation is used only by differential unit tests.
    fn step_impl<const PRUNE: bool>(&mut self) {
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
                cfr_precision: self.cfr_precision,
                par: self.par,
            };
            let mut out = self.scratch.take(self.game.tree.root_dims[p] as usize);
            cfr_pass::<_, _, PRUNE>(
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
        self.storage.assert_regrets_available();
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
        let record = |node: NodeId, values: &[f32]| {
            let sref = ctx.tree.storage_ref(ctx.tree.node(node));
            recorded.lock().expect("value recorder mutex")[sref.index as usize] =
                Some(values.to_vec());
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        value_pass::<_, _, _, true, false, true>(
            &ctx,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
            &mut [],
            &record,
            self.par.chance_depth,
        );
        recorded.into_inner().expect("value recorder mutex")
    }

    /// Visit action nodes as both seats' values are produced, carrying both
    /// reaches from the root. Only path/worker scratch is retained. The
    /// callback may run concurrently and must copy or pack its borrowed data
    /// before returning. Arithmetic for each seat follows `ev_pass` exactly.
    pub fn visit_expected_values<R>(&self, record: R)
    where
        R: Fn(NodeId, PerPlayer<&[f32]>, PerPlayer<&[f32]>, &[f32]) + Sync,
    {
        let mut scratch = Scratch::new();
        let dims = self.game.tree.root_dims;
        let mut flat = scratch.take((dims[Player::P0] + dims[Player::P1]) as usize);
        let (a, b) = flat.split_at_mut(dims[Player::P0] as usize);
        profile_pass(
            &self.game,
            &self.storage,
            self.par,
            &mut scratch,
            0,
            PerPlayer::new(
                &self.game.root_ranges[Player::P0],
                &self.game.root_ranges[Player::P1],
            ),
            PerPlayer::new(a, b),
            &record,
            self.par.chance_depth,
        );
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
        let (ev0, br0) = self.ev_and_br::<true>(Player::P0);
        let (ev1, br1) = if self.game.zero_sum {
            let (_, br1) = self.ev_and_br::<false>(Player::P1);
            (-ev0, br1)
        } else {
            self.ev_and_br::<true>(Player::P1)
        };
        PerPlayer::new(br0 - ev0, br1 - ev1)
    }

    /// Root EV and exploitability at the same evaluation boundary. The BR
    /// convention (including zero-sum accounting) is identical to
    /// `exploitability`; actual EVs are returned for both seats.
    pub fn evaluate(&self) -> (PerPlayer<f64>, PerPlayer<f64>) {
        let (ev0, br0) = self.ev_and_br::<true>(Player::P0);
        let (ev1, br1) = self.ev_and_br::<true>(Player::P1);
        (
            PerPlayer::new(ev0, ev1),
            PerPlayer::new(br0 - ev0, br1 - if self.game.zero_sum { -ev0 } else { ev1 }),
        )
    }

    fn ev_and_br<const EV: bool>(&self, p: Player) -> (f64, f64) {
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
            par: self.par,
        };
        with_worker_scratch(|scratch| {
            let dim = self.game.tree.root_dims[p] as usize;
            let mut ev = scratch.take(if EV { dim } else { 0 });
            let mut br = scratch.take(dim);
            value_pass::<_, _, _, EV, true, false>(
                &ctx,
                scratch,
                0,
                &self.game.root_ranges[p.opponent()],
                &mut ev,
                &mut br,
                &no_record,
                self.par.chance_depth,
            );
            let result = (
                if EV { self.root_aggregate(p, &ev) } else { 0.0 },
                self.root_aggregate(p, &br),
            );
            scratch.put(br);
            scratch.put(ev);
            result
        })
    }

    /// Normalized average strategy at an action node (`A*H`, action-major).
    pub fn average_strategy_at(&self, node: NodeId) -> Vec<f32> {
        self.node_strategy(node, |sref, out| {
            self.storage.average_strategy(sref, sref.index, out)
        })
    }

    /// Current (regret-matching) strategy at an action node.
    pub fn current_strategy_at(&self, node: NodeId) -> Vec<f32> {
        self.storage.assert_regrets_available();
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
    cfr_precision: CfrPrecision,
    par: ParConfig,
}

/// Storage access for an action node's own storage ref (when split with
/// one) and each of its children, while it processes them.
///
/// An eligible chance or action node may run its children in parallel,
/// which calls `StorageView::split` on whatever view it's handed. Ordinary
/// Rust reborrowing means that, left unprotected, that view is the *same
/// object* every action-node ancestor up to the root is also holding —
/// `split` permanently empties it (see its doc comment), so an ancestor
/// that reuses its ambient view afterward (every "my player" node, for its
/// own regret/strategy update; every sibling after the first, for its own
/// subtree) would find it gone.
///
/// Chance eligibility and the conservative action size bound identify
/// subtrees that cannot split: `Ambient` is the original, allocation-free
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
        if rayon::current_num_threads() == 1
            || ((par_budget == 0 || !tree.subtree_has_chance[node_id as usize])
                && subtree_elements(tree, node_id) < 2 * ACTION_PAR_MIN_ELEMENTS)
        {
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
/// (stack) order to reuse their allocations: every buffer a
/// call takes is released (in reverse order) before that call returns,
/// which is exactly the LIFO discipline [`Scratch`] relies on.
///
/// Every caller must pass a zeroed `out`, including terminals: `step` takes
/// the root buffer from scratch, chance/action parallel rows come from fresh
/// scratch buffers, each own-action CFV row is visited once, and the reused
/// sequential opponent-action buffer is cleared before every child. This
/// lets zero-opponent-reach terminals leave `out` untouched.
#[allow(clippy::too_many_arguments)]
fn cfr_pass<E: TerminalEvaluator, V: StorageView, const PRUNE: bool>(
    ctx: &PassCtx<'_, E>,
    storage: &mut V,
    scratch: &mut Scratch,
    node_id: NodeId,
    my_reach: &[f32],
    opp_reach: &[f32],
    out: &mut [f32],
    par_budget: u32,
) {
    debug_assert!(
        out.iter().all(|&x| x == 0.0),
        "CFR output must start at zero"
    );
    let node = *ctx.tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {
            if !PRUNE || !opp_reach.iter().all(|&x| x == 0.0) {
                ctx.evaluator.eval_cfr(node.aux, ctx.p, opp_reach, out);
            }
        }
        NodeKind::Chance => {
            // `chance_depth` bounds how many chance-node crossings remain
            // eligible for fan-out, so it decrements here regardless of
            // whether this particular node ends up parallelizing.
            let child_budget = par_budget.saturating_sub(1);
            if rayon::current_num_threads() > 1
                && par_budget > 0
                && node.num_children as usize >= ctx.par.min_children
            {
                let spans: Vec<_> = ctx
                    .tree
                    .children(node_id)
                    .map(|id| ctx.tree.storage_spans[id as usize])
                    .collect();
                let views = storage.split(&spans);
                let mut flat = scratch.take(chance_len(ctx.tree, node_id, ctx.p, my_reach.len()));
                chance_rows(ctx.tree, node_id, ctx.p, my_reach.len(), 1, &mut flat)
                    .into_par_iter()
                    .zip(views)
                    .enumerate()
                    .for_each(|(pos, (row, mut view))| {
                        with_worker_scratch(|scratch| {
                            let deal = *ctx.tree.deal(&node, pos);
                            let opp_dim = ctx
                                .tree
                                .mapped_dim(deal.maps[ctx.p.opponent()], opp_reach.len() as u32)
                                as usize;
                            let mut my_next = scratch.take(row.len());
                            let mut opp_next = scratch.take(opp_dim);
                            ctx.tree
                                .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
                            ctx.tree.map_reach_into(
                                deal.maps[ctx.p.opponent()],
                                opp_reach,
                                &mut opp_next,
                            );
                            cfr_pass::<_, _, PRUNE>(
                                ctx,
                                &mut view,
                                scratch,
                                node.first_child + pos as u32,
                                &my_next,
                                &opp_next,
                                row,
                                child_budget,
                            );
                            scratch.put(opp_next);
                            scratch.put(my_next);
                        });
                    });
                let mut offset = 0;
                for pos in 0..node.num_children as usize {
                    let deal = *ctx.tree.deal(&node, pos);
                    let len = ctx.tree.mapped_dim(deal.maps[ctx.p], my_reach.len() as u32) as usize;
                    ctx.tree.accumulate_values_with_scratch(
                        deal.maps[ctx.p],
                        deal.weight,
                        &flat[offset..offset + len],
                        out,
                        scratch,
                    );
                    offset += len;
                }
                scratch.put(flat);
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
                    cfr_pass::<_, _, PRUNE>(
                        ctx,
                        views.child(pos),
                        scratch,
                        child,
                        &my_next,
                        &opp_next,
                        &mut child_out,
                        child_budget,
                    );
                    ctx.tree.accumulate_values_with_scratch(
                        deal.maps[ctx.p],
                        deal.weight,
                        &child_out,
                        out,
                        scratch,
                    );
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
            views
                .own()
                .regret_matching_cfr(sref, sref.index, &mut sigma, ctx.cfr_precision);

            // One flat action-major buffer: action `a`'s row is that
            // action's own `out` parameter, so its recursion writes
            // straight into place instead of returning a fresh `Vec`.
            let mut cfvs = scratch.take(sref.len());
            let mut node_cfv = scratch.take(num_hands);
            // Reused across actions (not re-taken per action): its
            // recursive use always returns before the next action starts,
            // so overwriting it in place is safe.
            let mut my_next = scratch.take(num_hands);

            if !out.is_empty() && parallel_actions(ctx.tree, node_id) {
                let ActionViews::Split { views, has_own } = &mut views else {
                    unreachable!()
                };
                views[*has_own as usize..]
                    .par_iter_mut()
                    .zip(cfvs.par_chunks_mut(num_hands))
                    .enumerate()
                    .for_each(|(a, (view, row))| {
                        with_worker_scratch(|scratch| {
                            let mut reach = scratch.take(num_hands);
                            for h in 0..num_hands {
                                reach[h] = my_reach[h] * sigma[a * num_hands + h];
                            }
                            cfr_pass::<_, _, PRUNE>(
                                ctx,
                                view,
                                scratch,
                                node.first_child + a as u32,
                                &reach,
                                opp_reach,
                                row,
                                par_budget,
                            );
                            scratch.put(reach);
                        });
                    });
            } else {
                for (a, child) in ctx.tree.children(node_id).enumerate() {
                    let row = &sigma[a * num_hands..(a + 1) * num_hands];
                    for h in 0..num_hands {
                        my_next[h] = my_reach[h] * row[h];
                    }
                    let out_row = &mut cfvs[a * num_hands..(a + 1) * num_hands];
                    cfr_pass::<_, _, PRUNE>(
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
            let zero_opp = PRUNE && opp_reach.iter().all(|&x| x == 0.0);
            let mut sigma = scratch.take(if zero_opp { 0 } else { sref.len() });
            if !zero_opp {
                storage.regret_matching_cfr(sref, sref.index, &mut sigma, ctx.cfr_precision);
            }

            let mut views = ActionViews::split_for(storage, ctx.tree, node_id, None, par_budget);

            // `take` initializes this reusable buffer to zero. In the
            // zero-reach case it is shared immutably by parallel children.
            let mut opp_next = scratch.take(num_hands);
            let mut child_out = scratch.take(my_reach.len());
            if !out.is_empty() && parallel_actions(ctx.tree, node_id) {
                let mut flat = scratch.take(node.num_children as usize * out.len());
                let ActionViews::Split { views, has_own } = &mut views else {
                    unreachable!()
                };
                views[*has_own as usize..]
                    .par_iter_mut()
                    .zip(flat.par_chunks_mut(out.len()))
                    .enumerate()
                    .for_each(|(a, (view, row))| {
                        with_worker_scratch(|scratch| {
                            if zero_opp {
                                cfr_pass::<_, _, PRUNE>(
                                    ctx,
                                    view,
                                    scratch,
                                    node.first_child + a as u32,
                                    my_reach,
                                    &opp_next,
                                    row,
                                    par_budget,
                                );
                            } else {
                                let mut reach = scratch.take(num_hands);
                                for h in 0..num_hands {
                                    reach[h] = opp_reach[h] * sigma[a * num_hands + h];
                                }
                                cfr_pass::<_, _, PRUNE>(
                                    ctx,
                                    view,
                                    scratch,
                                    node.first_child + a as u32,
                                    my_reach,
                                    &reach,
                                    row,
                                    par_budget,
                                );
                                scratch.put(reach);
                            }
                        });
                    });
                if !zero_opp {
                    for row in flat.chunks(out.len()) {
                        for (dst, &v) in out.iter_mut().zip(row) {
                            *dst += v;
                        }
                    }
                }
                scratch.put(flat);
            } else {
                for (a, child) in ctx.tree.children(node_id).enumerate() {
                    if !zero_opp {
                        let row = &sigma[a * num_hands..(a + 1) * num_hands];
                        for h in 0..num_hands {
                            opp_next[h] = opp_reach[h] * row[h];
                        }
                    }
                    // See the Chance-node comment: reused accumulator targets
                    // must be reset before every use, not just the initial take.
                    child_out.fill(0.0);
                    cfr_pass::<_, _, PRUNE>(
                        ctx,
                        views.child(a),
                        scratch,
                        child,
                        my_reach,
                        &opp_next,
                        &mut child_out,
                        par_budget,
                    );
                    if !zero_opp {
                        for h in 0..out.len() {
                            out[h] += child_out[h];
                        }
                    }
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

/// EV and BR share opponent reach and terminal evaluation. Each channel keeps
/// the original child-order arithmetic; no parallel reduction is used.
#[allow(clippy::too_many_arguments)]
fn value_pass<
    E: TerminalEvaluator,
    S: Storage,
    R,
    const EV: bool,
    const BR: bool,
    const RECORD: bool,
>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    ev: &mut [f32],
    br: &mut [f32],
    record: &R,
    par_budget: u32,
) where
    R: Fn(NodeId, &[f32]) + Sync,
{
    // Recording walks retain the original arithmetic and visit every node.
    // This also preserves signed zeros in saved per-node values.
    if !RECORD && opp_reach.iter().all(|&x| x == 0.0) {
        return;
    }
    let node = *ctx.tree.node(node_id);
    let dim = if EV { ev.len() } else { br.len() };
    let channels = EV as usize + BR as usize;
    match node.kind {
        NodeKind::Terminal => {
            if EV {
                ctx.evaluator.eval(node.aux, ctx.p, opp_reach, ev);
                if BR {
                    br.copy_from_slice(ev);
                }
            } else {
                ctx.evaluator.eval(node.aux, ctx.p, opp_reach, br);
            }
        }
        NodeKind::Chance => {
            let child_budget = par_budget.saturating_sub(1);
            if rayon::current_num_threads() > 1
                && par_budget > 0
                && node.num_children as usize >= ctx.par.min_children
            {
                let mut flat = scratch.take(chance_len(ctx.tree, node_id, ctx.p, dim) * channels);
                chance_rows(ctx.tree, node_id, ctx.p, dim, channels, &mut flat)
                    .into_par_iter()
                    .enumerate()
                    .for_each(|(pos, row)| {
                        with_worker_scratch(|scratch| {
                            let deal = *ctx.tree.deal(&node, pos);
                            let opp_dim = ctx
                                .tree
                                .mapped_dim(deal.maps[ctx.p.opponent()], opp_reach.len() as u32)
                                as usize;
                            let mut opp_next = scratch.take(opp_dim);
                            ctx.tree.map_reach_into(
                                deal.maps[ctx.p.opponent()],
                                opp_reach,
                                &mut opp_next,
                            );
                            let child_dim = row.len() / channels;
                            let (child_ev, child_br) = value_row::<EV, BR>(row, child_dim);
                            value_pass::<_, _, _, EV, BR, RECORD>(
                                ctx,
                                scratch,
                                node.first_child + pos as u32,
                                &opp_next,
                                child_ev,
                                child_br,
                                record,
                                child_budget,
                            );
                            scratch.put(opp_next);
                        });
                    });
                let mut offset = 0;
                for pos in 0..node.num_children as usize {
                    let deal = *ctx.tree.deal(&node, pos);
                    let len = ctx.tree.mapped_dim(deal.maps[ctx.p], dim as u32) as usize;
                    if EV {
                        ctx.tree.accumulate_values_with_scratch(
                            deal.maps[ctx.p],
                            deal.weight,
                            &flat[offset..offset + len],
                            ev,
                            scratch,
                        );
                        offset += len;
                    }
                    if BR {
                        ctx.tree.accumulate_values_with_scratch(
                            deal.maps[ctx.p],
                            deal.weight,
                            &flat[offset..offset + len],
                            br,
                            scratch,
                        );
                        offset += len;
                    }
                }
                scratch.put(flat);
            } else {
                for (pos, child) in ctx.tree.children(node_id).enumerate() {
                    let deal = *ctx.tree.deal(&node, pos);
                    let opp_dim = ctx
                        .tree
                        .mapped_dim(deal.maps[ctx.p.opponent()], opp_reach.len() as u32)
                        as usize;
                    let child_dim = ctx.tree.mapped_dim(deal.maps[ctx.p], dim as u32) as usize;
                    let mut opp_next = scratch.take(opp_dim);
                    ctx.tree
                        .map_reach_into(deal.maps[ctx.p.opponent()], opp_reach, &mut opp_next);
                    let mut flat = scratch.take(child_dim * channels);
                    let (child_ev, child_br) = value_row::<EV, BR>(&mut flat, child_dim);
                    value_pass::<_, _, _, EV, BR, RECORD>(
                        ctx,
                        scratch,
                        child,
                        &opp_next,
                        child_ev,
                        child_br,
                        record,
                        child_budget,
                    );
                    if EV {
                        ctx.tree.accumulate_values_with_scratch(
                            deal.maps[ctx.p],
                            deal.weight,
                            child_ev,
                            ev,
                            scratch,
                        );
                    }
                    if BR {
                        ctx.tree.accumulate_values_with_scratch(
                            deal.maps[ctx.p],
                            deal.weight,
                            child_br,
                            br,
                            scratch,
                        );
                    }
                    scratch.put(flat);
                    scratch.put(opp_next);
                }
            }
        }
        NodeKind::Action => {
            let sref = ctx.tree.storage_ref(&node);
            let hands = sref.num_hands as usize;
            let actions = sref.num_actions as usize;
            let own = node.player == ctx.p;
            let mut sigma = scratch.take(if !own || EV { sref.len() } else { 0 });
            if !own || EV {
                ctx.storage.average_strategy(sref, sref.index, &mut sigma);
            }
            let parallel = dim != 0 && parallel_actions(ctx.tree, node_id);
            if own || parallel {
                let mut flat = scratch.take(actions * dim * channels);
                let visit = |scratch: &mut Scratch, a: usize, row: &mut [f32]| {
                    let (child_ev, child_br) = value_row::<EV, BR>(row, dim);
                    if own {
                        value_pass::<_, _, _, EV, BR, RECORD>(
                            ctx,
                            scratch,
                            node.first_child + a as u32,
                            opp_reach,
                            child_ev,
                            child_br,
                            record,
                            par_budget,
                        );
                    } else {
                        let mut reach = scratch.take(hands);
                        for h in 0..hands {
                            reach[h] = opp_reach[h] * sigma[a * hands + h];
                        }
                        value_pass::<_, _, _, EV, BR, RECORD>(
                            ctx,
                            scratch,
                            node.first_child + a as u32,
                            &reach,
                            child_ev,
                            child_br,
                            record,
                            par_budget,
                        );
                        scratch.put(reach);
                    }
                };
                if parallel {
                    flat.par_chunks_mut(dim * channels)
                        .enumerate()
                        .for_each(|(a, row)| {
                            with_worker_scratch(|scratch| visit(scratch, a, row));
                        });
                } else {
                    for a in 0..actions {
                        let row = &mut flat[a * dim * channels..(a + 1) * dim * channels];
                        visit(scratch, a, row);
                    }
                }
                if own && BR {
                    br.fill(f32::NEG_INFINITY);
                }
                for a in 0..actions {
                    let row = &mut flat[a * dim * channels..(a + 1) * dim * channels];
                    let (child_ev, child_br) = value_row::<EV, BR>(row, dim);
                    for h in 0..dim {
                        if EV {
                            if own {
                                ev[h] += sigma[a * hands + h] * child_ev[h];
                            } else {
                                ev[h] += child_ev[h];
                            }
                        }
                        if BR {
                            if own {
                                br[h] = br[h].max(child_br[h]);
                            } else {
                                br[h] += child_br[h];
                            }
                        }
                    }
                }
                scratch.put(flat);
            } else {
                let mut reach = scratch.take(hands);
                let mut flat = scratch.take(dim * channels);
                for (a, child) in ctx.tree.children(node_id).enumerate() {
                    for h in 0..hands {
                        reach[h] = opp_reach[h] * sigma[a * hands + h];
                    }
                    flat.fill(0.0);
                    let (child_ev, child_br) = value_row::<EV, BR>(&mut flat, dim);
                    value_pass::<_, _, _, EV, BR, RECORD>(
                        ctx, scratch, child, &reach, child_ev, child_br, record, par_budget,
                    );
                    for h in 0..dim {
                        if EV {
                            ev[h] += child_ev[h];
                        }
                        if BR {
                            br[h] += child_br[h];
                        }
                    }
                }
                scratch.put(flat);
                scratch.put(reach);
            }
            scratch.put(sigma);
            record(node_id, if EV { ev } else { br });
        }
    }
}

// Both-seat EV pass with path-local reach and deterministic child reductions.
#[allow(clippy::too_many_arguments)]
fn profile_pass<E: TerminalEvaluator, S: Storage, R>(
    game: &CompiledGame<E>,
    storage: &S,
    par: ParConfig,
    scratch: &mut Scratch,
    id: NodeId,
    reach: PerPlayer<&[f32]>,
    mut ev: PerPlayer<&mut [f32]>,
    record: &R,
    budget: u32,
) where
    R: Fn(NodeId, PerPlayer<&[f32]>, PerPlayer<&[f32]>, &[f32]) + Sync,
{
    let tree = &game.tree;
    let node = *tree.node(id);
    match node.kind {
        NodeKind::Terminal => {
            for p in Player::BOTH {
                game.evaluator.eval(node.aux, p, reach[p.opponent()], ev[p]);
            }
        }
        NodeKind::Chance => {
            let mut flat = scratch.take(
                chance_len(tree, id, Player::P0, ev[Player::P0].len())
                    + chance_len(tree, id, Player::P1, ev[Player::P1].len()),
            );
            let mut rest = flat.as_mut_slice();
            let mut rows = Vec::with_capacity(node.num_children as usize);
            for pos in 0..node.num_children as usize {
                let deal = *tree.deal(&node, pos);
                let a =
                    tree.mapped_dim(deal.maps[Player::P0], ev[Player::P0].len() as u32) as usize;
                let b =
                    tree.mapped_dim(deal.maps[Player::P1], ev[Player::P1].len() as u32) as usize;
                let (row, tail) = rest.split_at_mut(a + b);
                rest = tail;
                let (a, b) = row.split_at_mut(a);
                rows.push((a, b));
            }
            let visit = |scratch: &mut Scratch, pos: usize, a: &mut [f32], b: &mut [f32]| {
                let deal = *tree.deal(&node, pos);
                let mut r0 = scratch.take(a.len());
                let mut r1 = scratch.take(b.len());
                tree.map_reach_into(deal.maps[Player::P0], reach[Player::P0], &mut r0);
                tree.map_reach_into(deal.maps[Player::P1], reach[Player::P1], &mut r1);
                profile_pass(
                    game,
                    storage,
                    par,
                    scratch,
                    node.first_child + pos as u32,
                    PerPlayer::new(&r0, &r1),
                    PerPlayer::new(a, b),
                    record,
                    budget.saturating_sub(1),
                );
                scratch.put(r1);
                scratch.put(r0);
            };
            if rayon::current_num_threads() > 1
                && budget > 0
                && node.num_children as usize >= par.min_children
            {
                rows.into_par_iter().enumerate().for_each(|(pos, (a, b))| {
                    with_worker_scratch(|scratch| visit(scratch, pos, a, b))
                });
            } else {
                for (pos, (a, b)) in rows.into_iter().enumerate() {
                    visit(scratch, pos, a, b);
                }
            }
            let mut offset = 0;
            for pos in 0..node.num_children as usize {
                let deal = *tree.deal(&node, pos);
                for p in Player::BOTH {
                    let len = tree.mapped_dim(deal.maps[p], ev[p].len() as u32) as usize;
                    tree.accumulate_values_with_scratch(
                        deal.maps[p],
                        deal.weight,
                        &flat[offset..offset + len],
                        ev[p],
                        scratch,
                    );
                    offset += len;
                }
            }
            scratch.put(flat);
        }
        NodeKind::Action => {
            let sref = tree.storage_ref(&node);
            let hands = sref.num_hands as usize;
            let mut sigma = scratch.take(sref.len());
            storage.average_strategy(sref, sref.index, &mut sigma);
            let dim0 = ev[Player::P0].len();
            let dim = dim0 + ev[Player::P1].len();
            let mut flat = scratch.take(dim * node.num_children as usize);
            let visit = |scratch: &mut Scratch, a: usize, row: &mut [f32]| {
                let mut next = scratch.take(hands);
                for h in 0..hands {
                    next[h] = reach[node.player][h] * sigma[a * hands + h];
                }
                let r = if node.player == Player::P0 {
                    PerPlayer::new(next.as_slice(), reach[Player::P1])
                } else {
                    PerPlayer::new(reach[Player::P0], next.as_slice())
                };
                let (a_ev, b_ev) = row.split_at_mut(dim0);
                profile_pass(
                    game,
                    storage,
                    par,
                    scratch,
                    node.first_child + a as u32,
                    r,
                    PerPlayer::new(a_ev, b_ev),
                    record,
                    budget,
                );
                scratch.put(next);
            };
            if dim != 0 && parallel_actions(tree, id) {
                flat.par_chunks_mut(dim)
                    .enumerate()
                    .for_each(|(a, row)| with_worker_scratch(|scratch| visit(scratch, a, row)));
            } else {
                for a in 0..node.num_children as usize {
                    visit(scratch, a, &mut flat[a * dim..(a + 1) * dim]);
                }
            }
            for a in 0..node.num_children as usize {
                let row = &flat[a * dim..(a + 1) * dim];
                let child = PerPlayer::new(&row[..dim0], &row[dim0..]);
                for p in Player::BOTH {
                    for h in 0..ev[p].len() {
                        if node.player == p {
                            ev[p][h] += sigma[a * hands + h] * child[p][h];
                        } else {
                            ev[p][h] += child[p][h];
                        }
                    }
                }
            }
            record(
                id,
                reach,
                PerPlayer::new(ev[Player::P0], ev[Player::P1]),
                &sigma,
            );
            scratch.put(flat);
            scratch.put(sigma);
        }
    }
}

/// Splits one child row of a fused pass into its EV and BR channels (either
/// may be empty when that channel is off).
fn value_row<const EV: bool, const BR: bool>(
    row: &mut [f32],
    dim: usize,
) -> (&mut [f32], &mut [f32]) {
    debug_assert!(EV || BR);
    row.split_at_mut(if EV { dim } else { 0 })
}

/// The recorder [`value_pass`] uses when the caller only wants the root
/// aggregate. Monomorphization compiles it away.
fn no_record(_: NodeId, _: &[f32]) {}

/// One walk of the tree that fills per-hand values: [`ev_pass`] for the
/// average profile, [`br_pass`] for a best response. Named so
/// [`Solver::values_at`] can take either without spelling the signature out.
type ValuePass<E, S> = fn(&ValueCtx<'_, E, S>, &mut Scratch, NodeId, &[f32], &mut [f32], u32);

/// Expected values for `p` when both players play their average strategy.
pub(crate) fn ev_pass<E: TerminalEvaluator, S: Storage>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    out: &mut [f32],
    par_budget: u32,
) {
    value_pass::<_, _, _, true, false, false>(
        ctx,
        scratch,
        node_id,
        opp_reach,
        out,
        &mut [],
        &no_record,
        par_budget,
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
    value_pass::<_, _, _, false, true, false>(
        ctx,
        scratch,
        node_id,
        opp_reach,
        &mut [],
        out,
        &no_record,
        par_budget,
    );
}

#[cfg(test)]
#[path = "solver/dead_subtree_tests.rs"]
mod dead_subtree_tests;
