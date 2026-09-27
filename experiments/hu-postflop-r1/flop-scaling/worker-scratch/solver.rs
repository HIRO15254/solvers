use std::ops::{Deref, DerefMut};
use std::sync::Mutex;
/// Invocation-owned idle pools. An index is only a cache hint, never an
/// exclusivity proof: nested Rayon work and other pools may reuse that index.
/// At most one idle Scratch is retained per bin; active leases and their byte
/// capacities are not bounded by that count. Dropping this bank frees its bins.
struct ScratchBank {
    bins: Vec<Mutex<Option<Scratch>>>,
}

impl ScratchBank {
    fn new() -> Self {
        let workers = rayon::current_num_threads();
        Self::with_bins(if workers > 1 { workers } else { 0 })
    }

    fn with_bins(count: usize) -> Self {
        Self {
            bins: (0..count).map(|_| Mutex::new(None)).collect(),
        }
    }

    fn checkout(&self) -> ScratchLease<'_> {
        self.checkout_for(rayon::current_thread_index())
    }

    fn checkout_for(&self, index: Option<usize>) -> ScratchLease<'_> {
        let bin = index.and_then(|i| self.bins.get(i));
        if let Some(bin) = bin
            && let Ok(mut idle) = bin.try_lock()
        {
            let scratch = idle.take().unwrap_or_default();
            // No guard or RefCell borrow survives into the recursive walk.
            return ScratchLease {
                scratch,
                bin: Some(bin),
            };
        }
        // Pool-external, out-of-range, contended or poisoned: never wait.
        ScratchLease {
            scratch: Scratch::new(),
            bin: None,
        }
    }
}

struct ScratchLease<'a> {
    scratch: Scratch,
    bin: Option<&'a Mutex<Option<Scratch>>>,
}

impl Deref for ScratchLease<'_> {
    type Target = Scratch;

    fn deref(&self) -> &Self::Target {
        &self.scratch
    }
}

impl DerefMut for ScratchLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.scratch
    }
}

impl Drop for ScratchLease<'_> {
    fn drop(&mut self) {
        // Preserve an evaluator/recorder panic; discard incomplete scratch.
        // No user code runs under the lock, and poisoned locks are not unwrapped.
        if std::thread::panicking() {
            return;
        }
        if let Some(bin) = self.bin
            && let Ok(mut idle) = bin.try_lock()
            && idle.is_none()
        {
            *idle = Some(std::mem::take(&mut self.scratch));
        }
        // If a nested lease returned first, or the bin is busy, discard this
        // owned scratch instead of growing retention or waiting on other work.
    }
}

use cards::{PerPlayer, Player};
use rayon::prelude::*;

use crate::schedule::{DiscountSchedule, Discounts};
use crate::scratch::Scratch;
use crate::storage::{
    StateMismatch, Storage, StorageRef, StorageSpan, StorageState, StorageStateRef, StorageView,
};
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

// Scheduling only: this does not change chance-depth or arithmetic order.
// Storage elements are a generic, range-dimension-aware work proxy, not a
// measurement of terminal evaluator work (terminal spans contain no storage).
// Keep this candidate's grain rule fixed across trees and worker counts.
const ACTION_PAR_MIN_GRAIN: usize = 4_096;
const ACTION_PAR_MAX_GRAIN: usize = 65_536;
const ACTION_PAR_TASKS_PER_WORKER: usize = 4;

fn chance_budget(configured: u32) -> u32 {
    if rayon::current_num_threads() > 1 {
        configured
    } else {
        0
    }
}

fn subtree_elements(tree: &PublicTree, node: NodeId) -> usize {
    let span = tree.storage_spans[node as usize];
    span.end - span.start
}

#[derive(Clone, Copy, Default)]
struct ActionSplit {
    here: bool,
    // Includes this node. Sequential ancestors also need independent
    // storage views whenever any descendant can consume its view by split.
    below: bool,
}

struct ActionPlan {
    nodes: Vec<ActionSplit>,
}

impl ActionPlan {
    fn new(tree: &PublicTree) -> Option<Self> {
        let workers = rayon::current_num_threads();
        if workers == 1
            || tree.subtree_has_chance[0]
            || subtree_elements(tree, 0) < 2 * ACTION_PAR_MIN_GRAIN
        {
            return None;
        }
        let grain = subtree_elements(tree, 0)
            .div_ceil(workers.saturating_mul(ACTION_PAR_TASKS_PER_WORKER))
            .clamp(ACTION_PAR_MIN_GRAIN, ACTION_PAR_MAX_GRAIN);
        let mut nodes = vec![ActionSplit::default(); tree.nodes.len()];
        // Compiled child ids always follow their parent. One reverse pass
        // identifies both the forks and every ancestor that must protect
        // its view; no repeated subtree search is needed during traversal.
        for (id, node) in tree.nodes.iter().enumerate().rev() {
            if node.kind != NodeKind::Action {
                continue;
            }
            let here = tree
                .children(id as NodeId)
                .filter(|&child| subtree_elements(tree, child) >= grain)
                .take(2)
                .count()
                == 2;
            let below = here
                || tree
                    .children(id as NodeId)
                    .any(|child| nodes[child as usize].below);
            nodes[id] = ActionSplit { here, below };
        }
        nodes[0].below.then_some(Self { nodes })
    }
}

fn parallel_actions(node: NodeId, plan: Option<&ActionPlan>) -> bool {
    plan.is_some_and(|plan| plan.nodes[node as usize].here)
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

/// Serialization view of the current iteration and borrowed storage arenas.
/// Its field order and wire representation match [`SolverState`], which remains
/// the owned representation used when reading and restoring a checkpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SolverStateRef<'a> {
    pub iteration: u64,
    pub storage: StorageStateRef<'a>,
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

    /// Copies the iteration count and storage backend contents into an owned
    /// snapshot. Use [`Self::state_ref`] when writing a checkpoint immediately.
    pub fn state(&self) -> SolverState {
        SolverState {
            iteration: self.iteration,
            storage: self.storage.state(),
        }
    }

    /// Borrows the complete checkpoint state without cloning storage. The
    /// immutable borrow prevents stepping or restoring while it is serialized.
    pub fn state_ref(&self) -> SolverStateRef<'_> {
        SolverStateRef {
            iteration: self.iteration,
            storage: self.storage.state_ref(),
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
        let action_plan = ActionPlan::new(&self.game.tree);
        let bank = ScratchBank::new();
        self.step_with_plan(action_plan.as_ref(), &bank);
    }

    fn step_with_plan(&mut self, action_plan: Option<&ActionPlan>, bank: &ScratchBank) {
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
                bank,
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
                chance_budget(self.par.chance_depth),
                action_plan,
            );
            self.scratch.put(out);
        }
        self.iteration = t;
    }

    pub fn run(&mut self, iterations: u64) {
        if iterations == 0 {
            return;
        }
        // Construct inside the caller's current pool, not in Solver::new:
        // callers may build a solver outside the pool that later runs it.
        // Reuse the immutable plan across both player passes and iterations.
        let action_plan = ActionPlan::new(&self.game.tree);
        let bank = ScratchBank::new();
        for _ in 0..iterations {
            self.step_with_plan(action_plan.as_ref(), &bank);
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
        self.expected_values_where(p, |_| true)
    }

    /// Per-hand values at selected action nodes, indexed by `StorageRef::index`.
    ///
    /// Every downstream value is still computed: `include` controls only which
    /// completed action-node vectors are copied into the result. Unselected
    /// entries stay `None`, saving their allocation without changing values at
    /// selected ancestors. The predicate may run concurrently across branches.
    pub fn expected_values_where(
        &self,
        p: Player,
        include: impl Fn(NodeId) -> bool + Sync,
    ) -> Vec<Option<Vec<f32>>> {
        let recorded = Mutex::new(vec![None; self.game.tree.storage_refs.len()]);
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
            par: self.par,
        };
        let combine =
            |sref: StorageRef, children_flat: &[f32], out: &mut [f32], scratch: &mut Scratch| {
                let num_hands = sref.num_hands as usize;
                let mut sigma = scratch.take(sref.len());
                ctx.storage.average_strategy(sref, sref.index, &mut sigma);
                for a in 0..sref.num_actions as usize {
                    let row = &sigma[a * num_hands..(a + 1) * num_hands];
                    let child = &children_flat[a * num_hands..(a + 1) * num_hands];
                    for h in 0..out.len() {
                        out[h] += row[h] * child[h];
                    }
                }
                scratch.put(sigma);
            };
        let record = |node: NodeId, values: &[f32]| {
            if include(node) {
                let sref = ctx.tree.storage_ref(ctx.tree.node(node));
                recorded.lock().expect("value recorder mutex")[sref.index as usize] =
                    Some(values.to_vec());
            }
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        let action_plan = ActionPlan::new(&self.game.tree);
        let bank = ScratchBank::new();
        value_pass(
            &ctx,
            &bank,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
            &combine,
            &record,
            chance_budget(self.par.chance_depth),
            action_plan.as_ref(),
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
    bank: &'w ScratchBank,
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
/// The chance budget and action plan identify nodes that can avoid worrying
/// about this: `Ambient` is the original, allocation-free
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
        action_plan: Option<&ActionPlan>,
    ) -> Self {
        if (par_budget == 0 || !tree.subtree_has_chance[node_id as usize])
            && !action_plan.is_some_and(|plan| plan.nodes[node_id as usize].below)
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

    fn children_mut(&mut self) -> &mut [V] {
        match self {
            ActionViews::Split { views, has_own } => &mut views[*has_own as usize..],
            ActionViews::Ambient(_) => unreachable!("parallel actions require disjoint views"),
        }
    }
}

/// CFR update pass for player `p`, writing p's counterfactual values (one
/// per hand in p's current private-state space) into `out`.
///
/// All temporaries come from `scratch`, taken and released in recursion
/// (stack) order on the sequential path. Parallel tasks have local scratch
/// leases and disjoint outputs. Idle scratch is retained by the invocation
/// bank across its passes, with at most one idle Scratch per worker-index bin.
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
    action_plan: Option<&ActionPlan>,
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
                    .map_init(
                        || ctx.bank.checkout(),
                        |scratch, (pos, (child, mut view))| {
                            let scratch = scratch.deref_mut();
                            let deal = *ctx.tree.deal(&node, pos);
                            // Each child's own deal maps decide its dimensions:
                            // a `Transition` may change them, and different
                            // deals off the same chance node may map into
                            // different dimensions (see `PublicTree::mapped_dim`
                            // and `ReachMap`'s doc comment), so these must come
                            // from this child's `deal`, not from `my_reach`/
                            // `opp_reach`'s own (parent) lengths.
                            let my_dim =
                                ctx.tree.mapped_dim(deal.maps[ctx.p], my_reach.len() as u32)
                                    as usize;
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
                                None,
                            );
                            scratch.put(opp_next);
                            scratch.put(my_next);
                            child_out
                        },
                    )
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
                    ActionViews::split_for(storage, ctx.tree, node_id, None, child_budget, None);
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
                        None,
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
            let mut views = ActionViews::split_for(
                storage,
                ctx.tree,
                node_id,
                Some(own_span),
                par_budget,
                action_plan,
            );

            let mut sigma = scratch.take(sref.len());
            views.own().regret_matching(sref, sref.index, &mut sigma);

            // One flat action-major buffer: action `a`'s row is that
            // action's own `out` parameter, so its recursion writes
            // straight into place instead of returning a fresh `Vec`.
            let mut cfvs = scratch.take(sref.len());
            let mut node_cfv = scratch.take(num_hands);
            if num_hands > 0 && parallel_actions(node_id, action_plan) {
                cfr_action_children(
                    ctx,
                    &mut views,
                    node_id,
                    my_reach,
                    opp_reach,
                    &sigma,
                    &mut cfvs,
                    par_budget,
                    action_plan,
                );
            } else {
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
                        action_plan,
                    );
                }
                scratch.put(my_next);
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

            let mut views =
                ActionViews::split_for(storage, ctx.tree, node_id, None, par_budget, action_plan);

            if !out.is_empty() && parallel_actions(node_id, action_plan) {
                let mut children = scratch.take(node.num_children as usize * out.len());
                cfr_action_children(
                    ctx,
                    &mut views,
                    node_id,
                    my_reach,
                    opp_reach,
                    &sigma,
                    &mut children,
                    par_budget,
                    action_plan,
                );
                // Preserve the sequential action order, never a rayon reduction.
                for child_out in children.chunks_exact(out.len()) {
                    for h in 0..out.len() {
                        out[h] += child_out[h];
                    }
                }
                scratch.put(children);
            } else {
                let mut opp_next = scratch.take(num_hands);
                let mut child_out = scratch.take(my_reach.len());
                for (a, child) in ctx.tree.children(node_id).enumerate() {
                    let row = &sigma[a * num_hands..(a + 1) * num_hands];
                    for h in 0..num_hands {
                        opp_next[h] = opp_reach[h] * row[h];
                    }
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
                        action_plan,
                    );
                    for h in 0..out.len() {
                        out[h] += child_out[h];
                    }
                }
                scratch.put(child_out);
                scratch.put(opp_next);
            }
            scratch.put(sigma);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cfr_action_children<E: TerminalEvaluator, V: StorageView>(
    ctx: &PassCtx<'_, E>,
    views: &mut ActionViews<'_, V>,
    node_id: NodeId,
    my_reach: &[f32],
    opp_reach: &[f32],
    sigma: &[f32],
    children: &mut [f32],
    par_budget: u32,
    action_plan: Option<&ActionPlan>,
) {
    let node = ctx.tree.node(node_id);
    let own_action = node.player == ctx.p;
    let reach = if own_action { my_reach } else { opp_reach };
    views
        .children_mut()
        .par_iter_mut()
        .zip(children.par_chunks_mut(my_reach.len()))
        .enumerate()
        .for_each_init(
            || ctx.bank.checkout(),
            |scratch, (a, (view, child_out))| {
                let scratch = scratch.deref_mut();
                let mut next = scratch.take(reach.len());
                let row = &sigma[a * reach.len()..(a + 1) * reach.len()];
                for h in 0..reach.len() {
                    next[h] = reach[h] * row[h];
                }
                let (my_next, opp_next) = if own_action {
                    (next.as_slice(), opp_reach)
                } else {
                    (my_reach, next.as_slice())
                };
                cfr_pass(
                    ctx,
                    view,
                    scratch,
                    node.first_child + a as NodeId,
                    my_next,
                    opp_next,
                    child_out,
                    par_budget,
                    action_plan,
                );
                scratch.put(next);
            },
        );
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
    bank: &ScratchBank,
    scratch: &mut Scratch,
    node_id: NodeId,
    opp_reach: &[f32],
    out: &mut [f32],
    combine: &C,
    record: &R,
    par_budget: u32,
    action_plan: Option<&ActionPlan>,
) where
    C: Fn(StorageRef, &[f32], &mut [f32], &mut Scratch) + Sync,
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
                    .map_init(
                        || bank.checkout(),
                        |scratch, (pos, child)| {
                            let scratch = scratch.deref_mut();
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
                                bank,
                                scratch,
                                child,
                                &opp_next,
                                &mut child_out,
                                combine,
                                record,
                                child_budget,
                                None,
                            );
                            scratch.put(opp_next);
                            child_out
                        },
                    )
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
                        bank,
                        scratch,
                        child,
                        &opp_next,
                        &mut child_out,
                        combine,
                        record,
                        child_budget,
                        None,
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
            if my_dim > 0 && parallel_actions(node_id, action_plan) {
                children_flat
                    .par_chunks_mut(my_dim)
                    .enumerate()
                    .for_each_init(
                        || bank.checkout(),
                        |scratch, (a, row)| {
                            let scratch = scratch.deref_mut();
                            value_pass(
                                ctx,
                                bank,
                                scratch,
                                node.first_child + a as NodeId,
                                opp_reach,
                                row,
                                combine,
                                record,
                                par_budget,
                                action_plan,
                            );
                        },
                    );
            } else {
                for (a, child) in ctx.tree.children(node_id).enumerate() {
                    let row = &mut children_flat[a * my_dim..(a + 1) * my_dim];
                    value_pass(
                        ctx,
                        bank,
                        scratch,
                        child,
                        opp_reach,
                        row,
                        combine,
                        record,
                        par_budget,
                        action_plan,
                    );
                }
            }
            combine(sref, &children_flat, out, scratch);
            scratch.put(children_flat);
        }
        NodeKind::Action => {
            let sref = ctx.tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            let mut sigma = scratch.take(sref.len());
            ctx.storage.average_strategy(sref, sref.index, &mut sigma);
            let my_dim = out.len();
            if my_dim > 0 && parallel_actions(node_id, action_plan) {
                let mut children = scratch.take(node.num_children as usize * my_dim);
                children.par_chunks_mut(my_dim).enumerate().for_each_init(
                    || bank.checkout(),
                    |scratch, (a, child_out)| {
                        let scratch = scratch.deref_mut();
                        let mut opp_next = scratch.take(num_hands);
                        let row = &sigma[a * num_hands..(a + 1) * num_hands];
                        for h in 0..num_hands {
                            opp_next[h] = opp_reach[h] * row[h];
                        }
                        value_pass(
                            ctx,
                            bank,
                            scratch,
                            node.first_child + a as NodeId,
                            &opp_next,
                            child_out,
                            combine,
                            record,
                            par_budget,
                            action_plan,
                        );
                        scratch.put(opp_next);
                    },
                );
                for child_out in children.chunks_exact(my_dim) {
                    for h in 0..my_dim {
                        out[h] += child_out[h];
                    }
                }
                scratch.put(children);
            } else {
                let mut opp_next = scratch.take(num_hands);
                let mut child_out = scratch.take(my_dim);
                for (a, child) in ctx.tree.children(node_id).enumerate() {
                    let row = &sigma[a * num_hands..(a + 1) * num_hands];
                    for h in 0..num_hands {
                        opp_next[h] = opp_reach[h] * row[h];
                    }
                    child_out.fill(0.0);
                    value_pass(
                        ctx,
                        bank,
                        scratch,
                        child,
                        &opp_next,
                        &mut child_out,
                        combine,
                        record,
                        par_budget,
                        action_plan,
                    );
                    for h in 0..my_dim {
                        out[h] += child_out[h];
                    }
                }
                scratch.put(child_out);
                scratch.put(opp_next);
            }
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
    let combine =
        |sref: StorageRef, children_flat: &[f32], out: &mut [f32], scratch: &mut Scratch| {
            let num_hands = sref.num_hands as usize;
            let mut sigma = scratch.take(sref.len());
            ctx.storage.average_strategy(sref, sref.index, &mut sigma);
            for a in 0..sref.num_actions as usize {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                let child = &children_flat[a * num_hands..(a + 1) * num_hands];
                for h in 0..out.len() {
                    out[h] += row[h] * child[h];
                }
            }
            scratch.put(sigma);
        };
    let action_plan = ActionPlan::new(ctx.tree);
    let bank = ScratchBank::new();
    value_pass(
        ctx,
        &bank,
        scratch,
        node_id,
        opp_reach,
        out,
        &combine,
        &no_record,
        chance_budget(par_budget),
        action_plan.as_ref(),
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
    let combine =
        |sref: StorageRef, children_flat: &[f32], out: &mut [f32], _scratch: &mut Scratch| {
            let num_hands = sref.num_hands as usize;
            for h in 0..out.len() {
                out[h] = (0..sref.num_actions as usize)
                    .map(|a| children_flat[a * num_hands + h])
                    .fold(f32::NEG_INFINITY, f32::max);
            }
        };
    let action_plan = ActionPlan::new(ctx.tree);
    let bank = ScratchBank::new();
    value_pass(
        ctx,
        &bank,
        scratch,
        node_id,
        opp_reach,
        out,
        &combine,
        &no_record,
        chance_budget(par_budget),
        action_plan.as_ref(),
    );
}

#[cfg(test)]
mod scratch_bank_tests {
    use super::*;
    use crate::schedule::Dcfr;
    use crate::storage::{F32Storage, I16Storage};
    use crate::tree::{ReachMap, TempNode, TreeSpec};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;

    #[test]
    fn reentrant_leases_are_disjoint_and_only_one_returns() {
        let bank = ScratchBank::with_bins(1);
        let mut outer = bank.checkout_for(Some(0));
        let mut a = outer.take(8);
        a.fill(11.0);
        let mut inner = bank.checkout_for(Some(0));
        let mut b = inner.take(8);
        assert_ne!(a.as_ptr(), b.as_ptr());
        b.fill(29.0);
        let retained = b.as_ptr();
        inner.put(b);
        drop(inner);
        assert_eq!(a, vec![11.0; 8]);
        outer.put(a);
        drop(outer);
        let mut next = bank.checkout_for(Some(0));
        let b = next.take(8);
        assert_eq!(b.as_ptr(), retained);
        assert!(b.iter().all(|x| x.to_bits() == 0));
    }

    #[test]
    fn reused_buffers_zero_empty_smaller_and_larger_dimensions() {
        let bank = ScratchBank::with_bins(1);
        let mut first = bank.checkout_for(Some(0));
        let mut buffer = first.take(32);
        buffer.fill(f32::NAN);
        let allocation = buffer.as_ptr();
        first.put(buffer);
        drop(first);
        for size in [0, 3, 32, 65, 1] {
            let mut lease = bank.checkout_for(Some(0));
            let mut buffer = lease.take(size);
            assert_eq!(buffer.len(), size);
            assert!(buffer.iter().all(|x| x.to_bits() == 0));
            if size <= 32 && size != 1 {
                assert_eq!(buffer.as_ptr(), allocation);
            }
            buffer.fill(-7.0);
            lease.put(buffer);
        }
    }

    #[test]
    fn contention_and_invalid_indices_use_uncached_owned_scratch() {
        let bank = ScratchBank::with_bins(1);
        let guard = bank.bins[0].lock().unwrap();
        // Holding the same mutex here would deadlock a blocking implementation.
        for index in [Some(0), Some(1), None] {
            let mut lease = bank.checkout_for(index);
            assert!(lease.bin.is_none());
            let buffer = lease.take(4);
            assert_eq!(buffer, vec![0.0; 4]);
            lease.put(buffer);
        }
        drop(guard);
        assert!(bank.bins[0].lock().unwrap().is_none());
    }

    #[test]
    fn panic_is_propagated_and_poison_does_not_cause_another_panic() {
        let bank = ScratchBank::with_bins(1);
        let failure = catch_unwind(AssertUnwindSafe(|| {
            let mut lease = bank.checkout_for(Some(0));
            lease.put(vec![9.0; 16]);
            panic!("original evaluator panic");
        }));
        assert_eq!(
            failure.unwrap_err().downcast_ref::<&str>(),
            Some(&"original evaluator panic")
        );
        assert!(bank.bins[0].lock().unwrap().is_none());
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = bank.bins[0].lock().unwrap();
            panic!("intentional test poison");
        }));
        let mut lease = bank.checkout_for(Some(0));
        assert!(lease.bin.is_none());
        let buffer = lease.take(2);
        lease.put(buffer);
        drop(lease);
    }

    #[test]
    fn invocation_drop_has_no_global_owner_of_cached_buffers() {
        let bank = Arc::new(ScratchBank::with_bins(2));
        let weak = Arc::downgrade(&bank);
        for index in 0..2 {
            let mut lease = bank.checkout_for(Some(index));
            lease.put(vec![1.0; 64]);
        }
        assert!(bank.bins.iter().all(|bin| bin.lock().unwrap().is_some()));
        assert_eq!(Arc::strong_count(&bank), 1);
        drop(bank);
        // ScratchBank has only owned Vec/Mutex/Option/Scratch fields, whose
        // normal field destruction releases the cached Vec allocations.
        assert!(weak.upgrade().is_none());
    }

    struct Payoff;

    impl TerminalEvaluator for Payoff {
        fn eval(&self, terminal: u32, p: Player, opp: &[f32], out: &mut [f32]) {
            let payoff = if p == Player::P0 {
                terminal as f32
            } else {
                -(terminal as f32)
            };
            out.fill(opp.iter().sum::<f32>() * payoff);
        }
    }

    fn game() -> CompiledGame<Payoff> {
        let action = || TempNode::Action {
            player: Player::P0,
            children: vec![
                TempNode::Terminal { id: 1, tag: 0 },
                TempNode::Terminal { id: 3, tag: 0 },
            ],
            tag: 0,
        };
        let inner = || TempNode::Chance {
            deals: (0..4)
                .map(|_| {
                    (
                        0.25,
                        PerPlayer::new(ReachMap::Identity, ReachMap::Identity),
                        action(),
                    )
                })
                .collect(),
            tag: 0,
        };
        let tree = PublicTree::compile(TreeSpec {
            root: TempNode::Chance {
                deals: (0..4)
                    .map(|_| {
                        (
                            0.25,
                            PerPlayer::new(ReachMap::Identity, ReachMap::Identity),
                            inner(),
                        )
                    })
                    .collect(),
                tag: 0,
            },
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(1, 1),
        });
        CompiledGame {
            tree,
            evaluator: Payoff,
            root_ranges: PerPlayer::new(vec![1.0], vec![1.0]),
            normalizer: 1.0,
            zero_sum: true,
        }
    }

    fn compare_run_and_step<S: Storage>() {
        let mut reference = None;
        for workers in [1, 2, 4] {
            rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap()
                .install(|| {
                    let make = || {
                        let mut solver =
                            Solver::<_, S>::new(game(), Box::<Dcfr>::default(), Some(3));
                        solver.set_par(ParConfig {
                            chance_depth: 2,
                            min_children: 2,
                        });
                        solver
                    };
                    let mut run = make();
                    assert_eq!(run.expected_value(Player::P0), 2.0);
                    assert_eq!(run.best_response_value(Player::P0), 3.0);
                    run.run(3);
                    let mut steps = make();
                    for _ in 0..3 {
                        steps.step();
                    }
                    assert_eq!(run.state(), steps.state());
                    for p in Player::BOTH {
                        assert_eq!(
                            run.expected_value(p).to_bits(),
                            steps.expected_value(p).to_bits()
                        );
                        assert_eq!(
                            run.best_response_value(p).to_bits(),
                            steps.best_response_value(p).to_bits()
                        );
                        assert_eq!(
                            run.expected_values_everywhere(p),
                            steps.expected_values_everywhere(p)
                        );
                    }
                    let state = run.state();
                    if let Some(reference) = &reference {
                        assert_eq!(&state, reference);
                    } else {
                        reference = Some(state);
                    }
                });
        }
    }

    #[test]
    fn invocation_lifetimes_preserve_nested_chance_f32() {
        compare_run_and_step::<F32Storage>();
    }

    #[test]
    fn invocation_lifetimes_preserve_nested_chance_i16() {
        compare_run_and_step::<I16Storage>();
    }
}

#[cfg(test)]
mod action_plan_tests {
    use super::*;
    use crate::tree::{ReachMap, TempNode, TreeSpec};

    fn leaf_action(player: Player, children: usize, tag: u32) -> TempNode {
        TempNode::Action {
            player,
            children: (0..children)
                .map(|_| TempNode::Terminal { id: 0, tag: 0 })
                .collect(),
            tag,
        }
    }

    fn compile(root: TempNode, dims: PerPlayer<u32>) -> PublicTree {
        PublicTree::compile(TreeSpec {
            root,
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: dims,
        })
    }

    #[test]
    fn deep_action_fork_protects_every_sequential_ancestor() {
        // The fork is five actions below the root; each ancestor has just
        // one child with storage. Its two children have exactly 4,096
        // elements each, and the entire tree is below the former 65,536 gate.
        let mut root = TempNode::Action {
            player: Player::P1,
            children: vec![
                leaf_action(Player::P0, 2, 100),
                leaf_action(Player::P0, 2, 101),
            ],
            tag: 99,
        };
        for depth in 0..5 {
            let player = if depth % 2 == 0 {
                Player::P0
            } else {
                Player::P1
            };
            root = TempNode::Action {
                player,
                children: vec![TempNode::Terminal { id: 0, tag: 0 }, root],
                tag: 10 + depth,
            };
        }
        let tree = compile(root, PerPlayer::new(2_048, 1_024));
        assert_eq!(tree.storage_len, 26_624);
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap()
            .install(|| {
                let plan = ActionPlan::new(&tree).expect("deep fork must remain eligible");
                let mut forks = 0;
                let mut protected_ancestors = 0;
                for (id, &tag) in tree.tags.iter().enumerate() {
                    if tag == 99 {
                        assert!(plan.nodes[id].here && plan.nodes[id].below);
                        forks += 1;
                    } else if (10..15).contains(&tag) {
                        assert!(!plan.nodes[id].here && plan.nodes[id].below);
                        protected_ancestors += 1;
                    } else {
                        assert!(!plan.nodes[id].here && !plan.nodes[id].below);
                    }
                }
                assert_eq!((forks, protected_ancestors), (1, 5));
            });
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| assert!(ActionPlan::new(&tree).is_none()));
    }

    #[test]
    fn action_fork_requires_two_children_at_or_above_grain() {
        let make_tree = |second_at_grain| {
            let second = if second_at_grain {
                leaf_action(Player::P1, 2, 0) // 4,096 elements
            } else {
                leaf_action(Player::P0, 1, 0) // 4,095 elements
            };
            compile(
                TempNode::Action {
                    player: Player::P0,
                    children: vec![leaf_action(Player::P1, 2, 0), second],
                    tag: 0,
                },
                PerPlayer::new(4_095, 2_048),
            )
        };
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap()
            .install(|| {
                assert!(ActionPlan::new(&make_tree(false)).is_none());
                let plan = ActionPlan::new(&make_tree(true)).unwrap();
                assert!(parallel_actions(0, Some(&plan)));
            });
    }

    #[test]
    fn chance_tree_keeps_action_plan_disabled() {
        let action = TempNode::Action {
            player: Player::P0,
            children: vec![leaf_action(Player::P0, 2, 0), leaf_action(Player::P0, 2, 0)],
            tag: 0,
        };
        let tree = compile(
            TempNode::Chance {
                deals: vec![(
                    1.0,
                    PerPlayer::new(ReachMap::Identity, ReachMap::Identity),
                    action,
                )],
                tag: 0,
            },
            PerPlayer::new(4_096, 4_096),
        );
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap()
            .install(|| assert!(ActionPlan::new(&tree).is_none()));
    }
}
