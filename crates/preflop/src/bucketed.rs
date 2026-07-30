//! The bucketed blueprint game: the 169-class preflop trunk extended
//! through aggregated bucket-space postflop streets (see
//! `docs/research/blueprint-design.md`).
//!
//! Board cards never branch the public tree: each street boundary is one
//! chance node with a single aggregated deal whose `ReachMap::Transition`
//! carries P(next-street bucket | current bucket) for both players (T1 rows
//! additionally carry the kappa measure scale). Preflop fold and all-in
//! terminals keep the trunk's exact 169x169 compat/equity treatment; only
//! terminals inside the bucketed streets drop hand-vs-hand blockers
//! (compat = 1, the standard blueprint approximation).
//!
//! Postflop conventions: the big blind ([`Player::P1`]) acts first on every
//! postflop street. Bet and raise sizes are pot fractions (of the pot after
//! a call, mirroring `holdem`'s sizing), clamped to the stack and deduped
//! against the all-in. All-in showdowns before the river use transition-
//! composed equity (`W_turn = T3 * W_river * T3^T` and likewise through
//! T2), which is exact *within* the game's own independence approximation
//! and preserves zero-sum (`W + T + W^T == 1` survives row-stochastic
//! composition).

use std::fmt;

use abstraction::{BlueprintArtifacts, TransitionTable};
use cards::{Chips, NUM_CLASSES, PerPlayer, Player, Street};
use engine::{
    CompiledGame, F32Storage, I16Storage, NodeId, PublicTree, ReachMap, SparseTransition, Storage,
    TempNode, TerminalEvaluator, TreeSpec,
};
use game::{PayoffPipeline, TerminalDescriptor, TerminalKind};

use crate::classes;
use crate::equity::EquityTable;
use crate::model::{TermCoef, fold_coef, showdown_coef};
use crate::trunk::{
    BaseActions, LineState, MemoryEstimate, PreflopConfig, PreflopNodeInfo, base_actions,
    normalizer, raise_targets, root_line_state,
};

/// Postflop bet sizing: pot-fraction bet/raise sizes per street per player
/// (P0 = SB, P1 = BB), plus a per-street raise cap. Empty size lists mean
/// check/call-only streets.
#[derive(Clone, Debug)]
pub struct PostflopBets {
    pub flop: PerPlayer<Vec<f64>>,
    pub turn: PerPlayer<Vec<f64>>,
    pub river: PerPlayer<Vec<f64>>,
    pub max_raises: u32,
    /// Offer the all-in as a bet/raise at every postflop decision.
    pub include_allin: bool,
}

impl PostflopBets {
    /// The pot-fraction bet/raise sizes for `actor` on `street`.
    fn sizes(&self, street: Street, actor: Player) -> &[f64] {
        match street {
            Street::Flop => &self.flop[actor],
            Street::Turn => &self.turn[actor],
            Street::River => &self.river[actor],
            Street::Preflop => unreachable!("postflop sizing is never queried preflop"),
        }
    }
}

/// Which private-state space a terminal's coefficients live in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TermSpace {
    Preflop,
    Flop,
    Turn,
    River,
}

impl fmt::Display for TermSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// The bucket space a postflop street's terminals live in (`Preflop` never
/// occurs here — every postflop terminal belongs to whichever street it
/// ended on).
fn term_space_of(street: Street) -> TermSpace {
    match street {
        Street::Flop => TermSpace::Flop,
        Street::Turn => TermSpace::Turn,
        Street::River => TermSpace::River,
        Street::Preflop => unreachable!("postflop terminals never occur preflop"),
    }
}

// --- transition-table plumbing ------------------------------------------

/// Index into `TreeSpec::transitions` for `class_to_flop` (`T1`).
const T1: u32 = 0;
/// Index for `flop_to_turn` (`T2`).
const T2: u32 = 1;
/// Index for `turn_to_river` (`T3`).
const T3: u32 = 2;

/// 1:1 conversion: both `abstraction::TransitionTable` and
/// `engine::SparseTransition` are `(in_dim, out_dim, Vec<(u32, u32, f32)>)`.
fn sparse_transition(table: &TransitionTable) -> SparseTransition {
    SparseTransition {
        in_dim: table.in_dim,
        out_dim: table.out_dim,
        entries: table.entries.clone(),
    }
}

/// Dense row-major `in_dim x out_dim` matrix from a sparse
/// [`TransitionTable`], widened to f64 for the composition math below.
fn dense_transition(table: &TransitionTable) -> Vec<f64> {
    let (in_dim, out_dim) = (table.in_dim as usize, table.out_dim as usize);
    let mut dense = vec![0f64; in_dim * out_dim];
    for &(i, o, w) in &table.entries {
        dense[i as usize * out_dim + o as usize] = w as f64;
    }
    dense
}

/// `parent[a][b] = Sum_{i,j} trans[a][i] * child[i][j] * trans[b][j]`, f64
/// throughout (`trans` is `p_dim x c_dim` row-major, `child` is `c_dim x
/// c_dim`) — the transition-composed equity/tie matrix one street earlier
/// (module docs: `W_turn = T3 * W_river * T3^T`, etc). Computed as two
/// dense matmuls (`mid = trans * child`, `parent = mid * trans^T`) rather
/// than a naive quadruple loop, since a full-size blueprint (Kt/Kr in the
/// hundreds) makes the quadruple loop's `O(p^2 * c^2)` infeasible.
fn compose_equity(trans: &[f64], p_dim: usize, c_dim: usize, child: &[f64]) -> Vec<f64> {
    let mut mid = vec![0f64; p_dim * c_dim];
    for a in 0..p_dim {
        let mid_row = &mut mid[a * c_dim..(a + 1) * c_dim];
        for i in 0..c_dim {
            let t_ai = trans[a * c_dim + i];
            if t_ai == 0.0 {
                continue;
            }
            let child_row = &child[i * c_dim..(i + 1) * c_dim];
            for (m, &c) in mid_row.iter_mut().zip(child_row) {
                *m += t_ai * c;
            }
        }
    }
    let mut parent = vec![0f64; p_dim * p_dim];
    for a in 0..p_dim {
        let mid_row = &mid[a * c_dim..(a + 1) * c_dim];
        for b in 0..p_dim {
            let trans_row = &trans[b * c_dim..(b + 1) * c_dim];
            let mut acc = 0f64;
            for (&m, &t) in mid_row.iter().zip(trans_row) {
                acc += m * t;
            }
            parent[a * p_dim + b] = acc;
        }
    }
    parent
}

// --- evaluator -----------------------------------------------------------

/// Terminal evaluator over four private-state spaces: the exact preflop
/// 169-class space (compat / compat*e_win / compat*e_tie shared tables,
/// identical math to [`crate::PreflopEvaluator`]) and the three bucket
/// spaces (dense win/tie matrices, compat = 1). Every terminal stores
/// `(space, per-player TermCoef)`; `eval` dispatches on the space.
pub struct BlueprintEvaluator {
    // Preflop tables: `compat[h][o]`, `compat[h][o] * e_win(h, o)`,
    // `compat[h][o] * e_tie(h, o)`, row-major over the 169 classes —
    // identical construction to `PreflopEvaluator::new`.
    pf_compat: Vec<f32>,
    pf_win: Vec<f32>,
    pf_tie: Vec<f32>,
    // Bucket-space dense win/tie matrices (compat == 1 everywhere). River
    // is `artifacts.river_equity` directly; flop/turn are transition-
    // composed (see `compose_equity`).
    flop_dim: usize,
    flop_win: Vec<f32>,
    flop_tie: Vec<f32>,
    turn_dim: usize,
    turn_win: Vec<f32>,
    turn_tie: Vec<f32>,
    river_dim: usize,
    river_win: Vec<f32>,
    river_tie: Vec<f32>,
    terminals: Vec<(TermSpace, PerPlayer<[f64; 3]>)>,
}

impl BlueprintEvaluator {
    fn new(table: &EquityTable, artifacts: &BlueprintArtifacts) -> Self {
        assert_eq!(
            artifacts.class_to_flop.in_dim as usize, NUM_CLASSES,
            "class_to_flop must map from the 169 preflop classes"
        );
        assert_eq!(
            artifacts.flop_to_turn.in_dim, artifacts.class_to_flop.out_dim,
            "flop_to_turn.in_dim must match class_to_flop.out_dim"
        );
        assert_eq!(
            artifacts.turn_to_river.in_dim, artifacts.flop_to_turn.out_dim,
            "turn_to_river.in_dim must match flop_to_turn.out_dim"
        );
        assert_eq!(
            artifacts.river_equity.dim, artifacts.turn_to_river.out_dim,
            "river_equity.dim must match turn_to_river.out_dim"
        );

        let counts = crate::classes::compat_counts();
        let combo_counts = crate::classes::class_combo_counts();
        let len = NUM_CLASSES * NUM_CLASSES;
        let mut pf_compat = vec![0f32; len];
        let mut pf_win = vec![0f32; len];
        let mut pf_tie = vec![0f32; len];
        for h in 0..NUM_CLASSES {
            for o in 0..NUM_CLASSES {
                let idx = h * NUM_CLASSES + o;
                let c = counts[idx] as f64 / (combo_counts[h] as f64 * combo_counts[o] as f64);
                pf_compat[idx] = c as f32;
                pf_win[idx] = (c * table.win(h, o)) as f32;
                pf_tie[idx] = (c * table.tie(h, o)) as f32;
            }
        }

        let kf = artifacts.class_to_flop.out_dim as usize;
        let kt = artifacts.flop_to_turn.out_dim as usize;
        let kr = artifacts.turn_to_river.out_dim as usize;

        let river_win_f64 = artifacts.river_equity.win.clone();
        let river_tie_f64 = artifacts.river_equity.tie.clone();

        let t3 = dense_transition(&artifacts.turn_to_river);
        let turn_win_f64 = compose_equity(&t3, kt, kr, &river_win_f64);
        let turn_tie_f64 = compose_equity(&t3, kt, kr, &river_tie_f64);

        let t2 = dense_transition(&artifacts.flop_to_turn);
        let flop_win_f64 = compose_equity(&t2, kf, kt, &turn_win_f64);
        let flop_tie_f64 = compose_equity(&t2, kf, kt, &turn_tie_f64);

        let to_f32 = |v: Vec<f64>| -> Vec<f32> { v.into_iter().map(|x| x as f32).collect() };

        BlueprintEvaluator {
            pf_compat,
            pf_win,
            pf_tie,
            flop_dim: kf,
            flop_win: to_f32(flop_win_f64),
            flop_tie: to_f32(flop_tie_f64),
            turn_dim: kt,
            turn_win: to_f32(turn_win_f64),
            turn_tie: to_f32(turn_tie_f64),
            river_dim: kr,
            river_win: to_f32(river_win_f64),
            river_tie: to_f32(river_tie_f64),
            terminals: Vec::new(),
        }
    }

    /// Registers a terminal's per-player coefficients in `space`, returning
    /// its id (the `TempNode::Terminal { id, .. }` the builder must use).
    fn push_terminal(&mut self, space: TermSpace, coef: PerPlayer<TermCoef>) -> u32 {
        let id = self.terminals.len() as u32;
        self.terminals.push((space, coef.map(|c| [c.a, c.b, c.c])));
        id
    }
}

/// `out[h] = Sum_o opp_reach[o] * (a * compat[h][o] + b * win[h][o] + c *
/// tie[h][o])`, f64 accumulator — the preflop space's exact row math,
/// identical to `PreflopEvaluator::eval`.
fn eval_with_compat(
    compat: &[f32],
    win: &[f32],
    tie: &[f32],
    dim: usize,
    coef: [f64; 3],
    opp_reach: &[f32],
    out: &mut [f32],
) {
    debug_assert_eq!(opp_reach.len(), dim);
    debug_assert_eq!(out.len(), dim);
    let [a, b, c] = coef;
    for h in 0..dim {
        let row_c = &compat[h * dim..(h + 1) * dim];
        let row_w = &win[h * dim..(h + 1) * dim];
        let row_t = &tie[h * dim..(h + 1) * dim];
        let mut acc = 0f64;
        for o in 0..dim {
            acc += opp_reach[o] as f64
                * (a * row_c[o] as f64 + b * row_w[o] as f64 + c * row_t[o] as f64);
        }
        out[h] = acc as f32;
    }
}

/// Same row math with compat folded to the implicit constant 1 (bucket
/// spaces never carry hand-vs-hand blockers): `out[h] = Sum_o opp_reach[o]
/// * (a + b * win[h][o] + c * tie[h][o])`.
fn eval_bucket(
    win: &[f32],
    tie: &[f32],
    dim: usize,
    coef: [f64; 3],
    opp_reach: &[f32],
    out: &mut [f32],
) {
    debug_assert_eq!(opp_reach.len(), dim);
    debug_assert_eq!(out.len(), dim);
    let [a, b, c] = coef;
    for h in 0..dim {
        let row_w = &win[h * dim..(h + 1) * dim];
        let row_t = &tie[h * dim..(h + 1) * dim];
        let mut acc = 0f64;
        for o in 0..dim {
            acc += opp_reach[o] as f64 * (a + b * row_w[o] as f64 + c * row_t[o] as f64);
        }
        out[h] = acc as f32;
    }
}

impl TerminalEvaluator for BlueprintEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        let (space, coef) = &self.terminals[terminal as usize];
        let coef = coef[p];
        match space {
            TermSpace::Preflop => {
                eval_with_compat(
                    &self.pf_compat,
                    &self.pf_win,
                    &self.pf_tie,
                    NUM_CLASSES,
                    coef,
                    opp_reach,
                    out,
                );
            }
            TermSpace::Flop => {
                eval_bucket(
                    &self.flop_win,
                    &self.flop_tie,
                    self.flop_dim,
                    coef,
                    opp_reach,
                    out,
                );
            }
            TermSpace::Turn => {
                eval_bucket(
                    &self.turn_win,
                    &self.turn_tie,
                    self.turn_dim,
                    coef,
                    opp_reach,
                    out,
                );
            }
            TermSpace::River => {
                eval_bucket(
                    &self.river_win,
                    &self.river_tie,
                    self.river_dim,
                    coef,
                    opp_reach,
                    out,
                );
            }
        }
    }
}

/// A compiled blueprint game plus node metadata (same conventions as
/// [`crate::PreflopGame`]: tag-0 sentinel, history tokens — postflop
/// streets are separated by `/`, with `x`/`c`/`f`/`b{to}` action tokens
/// where `to` is the actor's total street contribution in chips).
pub struct BlueprintGame {
    pub game: CompiledGame<BlueprintEvaluator>,
    pub node_info: Vec<PreflopNodeInfo>,
}

impl BlueprintGame {
    /// Node lookup by exact history string (`""` is the root).
    pub fn node_by_history(&self, history: &str) -> Option<NodeId> {
        let tag = self
            .node_info
            .iter()
            .position(|info| info.history == history)? as u32;
        self.game
            .tree
            .tags
            .iter()
            .position(|&t| t == tag)
            .map(|id| id as u32)
    }

    /// The node's info record.
    pub fn info(&self, node: NodeId) -> &PreflopNodeInfo {
        &self.node_info[self.game.tree.tags[node as usize] as usize]
    }
}

// --- postflop betting-line state ------------------------------------------

/// Betting-line state for the postflop streets, threaded through
/// [`Builder::post_betting`]. `street_contrib` resets to `(0, 0)` at the
/// start of each street; `total_before_street` is the (equal) per-player
/// total committed before this street began, so `total_contrib` gives the
/// whole-hand total the stack-behind computation needs.
#[derive(Clone)]
struct PostState {
    street: Street,
    to_act: Player,
    total_before_street: PerPlayer<Chips>,
    street_contrib: PerPlayer<Chips>,
    raises_used: u32,
    /// Whether this street has already seen one check (mirrors
    /// `holdem::postflop::LineState::first_checked`): a second check ends
    /// the street.
    checked: bool,
    history: String,
}

impl PostState {
    fn total_contrib(&self, p: Player) -> Chips {
        self.total_before_street[p] + self.street_contrib[p]
    }

    fn pot_now(&self) -> Chips {
        self.total_contrib(Player::P0) + self.total_contrib(Player::P1)
    }
}

/// Distinct total-street-contribution targets a bet/raise can reach for the
/// acting player at `state` (pot-fraction sizing, all-in clamping,
/// same-amount dedup) — mirrors `holdem::postflop::raise_targets`'s math
/// (see its doc comment), adapted to return the absolute street-contribution
/// `to` (this crate's `b{to}` token convention) rather than a delta, and
/// with an explicit `include_allin` branch (`holdem`'s bet-fraction lists
/// reach the stack directly instead). Shared between the real builder and
/// the memory-usage dry run.
fn post_raise_targets(
    state: &PostState,
    post: &PostflopBets,
    effective_stack: Chips,
) -> Vec<Chips> {
    if state.raises_used >= post.max_raises {
        return Vec::new();
    }
    let actor = state.to_act;
    let opp = actor.opponent();
    let outstanding = state.street_contrib[opp] - state.street_contrib[actor];
    let stack_behind = effective_stack - state.total_contrib(actor);
    if stack_behind <= outstanding {
        return Vec::new();
    }
    let pot_now = state.pot_now();
    let mut seen: Vec<Chips> = Vec::new();
    for &fraction in post.sizes(state.street, actor) {
        let pot_after_call = pot_now + outstanding;
        let raw = (fraction * pot_after_call.as_f64()).round() as u32;
        let extra = Chips(raw.max(1)).min(stack_behind - outstanding);
        let additional = outstanding + extra;
        let to = state.street_contrib[actor] + additional;
        if !seen.contains(&to) {
            seen.push(to);
        }
    }
    if post.include_allin {
        let to = state.street_contrib[actor] + stack_behind;
        if !seen.contains(&to) {
            seen.push(to);
        }
    }
    seen
}

fn next_street(street: Street) -> Street {
    match street {
        Street::Flop => Street::Turn,
        Street::Turn => Street::River,
        Street::River => unreachable!("no chance node follows the river"),
        Street::Preflop => unreachable!("postflop streets never start before the flop"),
    }
}

fn bucket_dim_for_street(street: Street, kf: usize, kt: usize, kr: usize) -> usize {
    match street {
        Street::Flop => kf,
        Street::Turn => kt,
        Street::River => kr,
        Street::Preflop => unreachable!("postflop streets never start before the flop"),
    }
}

// --- builder ---------------------------------------------------------------

struct Builder<'a> {
    trunk: &'a PreflopConfig,
    post: &'a PostflopBets,
    pipeline: PayoffPipeline<'a>,
    evaluator: BlueprintEvaluator,
    node_info: Vec<PreflopNodeInfo>,
}

impl Builder<'_> {
    fn extend_history(&self, base: &str, token: &str) -> String {
        if self.trunk.track_node_info {
            format!("{base}{token}")
        } else {
            String::new()
        }
    }

    fn action_label(&self, make: impl FnOnce() -> String) -> String {
        if self.trunk.track_node_info {
            make()
        } else {
            String::new()
        }
    }

    fn finish_action_node(
        &mut self,
        actor: Player,
        history: String,
        actions: Vec<(String, TempNode)>,
    ) -> TempNode {
        let tag = if self.trunk.track_node_info {
            let tag = self.node_info.len() as u32;
            self.node_info.push(PreflopNodeInfo {
                history,
                actions: actions.iter().map(|(name, _)| name.clone()).collect(),
            });
            tag
        } else {
            0
        };
        TempNode::Action {
            player: actor,
            children: actions.into_iter().map(|(_, child)| child).collect(),
            tag,
        }
    }

    // --- preflop trunk (verbatim reuse of `crate::trunk`'s recursion shape,
    // see the module docs: the only change is `showdown_or_continuation`) --

    fn betting(&mut self, state: LineState) -> TempNode {
        let actor = state.to_act;
        let opp = actor.opponent();
        let mut actions: Vec<(String, TempNode)> = Vec::new();

        match base_actions(&state, self.trunk) {
            BaseActions::Check => {
                let history = self.extend_history(&state.history, "x");
                let label = self.action_label(|| "Check".into());
                let next = LineState {
                    history,
                    ..state.clone()
                };
                actions.push((label, self.showdown_or_continuation(&next)));
            }
            BaseActions::FoldThenLimp { allow_limp } => {
                actions.push((
                    self.action_label(|| "Fold".into()),
                    self.fold_terminal(&state, actor),
                ));
                if allow_limp {
                    let history = self.extend_history(&state.history, "c");
                    let label = self.action_label(|| "Limp".into());
                    let mut contrib = state.contrib;
                    contrib[actor] = self.trunk.bb;
                    let next = LineState {
                        contrib,
                        to_act: opp,
                        bb_option: true,
                        history,
                        ..state.clone()
                    };
                    actions.push((label, self.betting(next)));
                }
            }
            BaseActions::FoldThenCall => {
                actions.push((
                    self.action_label(|| "Fold".into()),
                    self.fold_terminal(&state, actor),
                ));
                let history = self.extend_history(&state.history, "c");
                let label = self.action_label(|| "Call".into());
                let mut contrib = state.contrib;
                contrib[actor] = state.contrib[opp];
                let next = LineState {
                    contrib,
                    history,
                    ..state.clone()
                };
                actions.push((label, self.showdown_or_continuation(&next)));
            }
        }

        for t in raise_targets(&state, self.trunk) {
            let history = self.extend_history(&state.history, &format!("r{}", t.0));
            let label = self.action_label(|| {
                let x = t.0 as f64 / self.trunk.bb.0 as f64;
                if t == self.trunk.effective_stack {
                    format!("All-in {x}bb")
                } else {
                    format!("Raise {x}bb")
                }
            });
            let mut contrib = state.contrib;
            contrib[actor] = t;
            let next = LineState {
                contrib,
                raises_used: state.raises_used + 1,
                prev_raise_to: state.last_raise_to,
                last_raise_to: t,
                to_act: opp,
                bb_option: false,
                history,
            };
            actions.push((label, self.betting(next)));
        }

        self.finish_action_node(actor, state.history, actions)
    }

    fn fold_terminal(&mut self, state: &LineState, folder: Player) -> TempNode {
        let descriptor = TerminalDescriptor {
            kind: TerminalKind::Fold { folder },
            street: Street::Preflop,
            pot: state.contrib[Player::P0] + state.contrib[Player::P1],
            contrib: state.contrib,
            stacks_before: PerPlayer::new(self.trunk.effective_stack, self.trunk.effective_stack),
        };
        let baked = self.pipeline.bake(&descriptor);
        let coef = PerPlayer::new(fold_coef(&baked, Player::P0), fold_coef(&baked, Player::P1));
        let id = self.evaluator.push_terminal(TermSpace::Preflop, coef);
        TempNode::Terminal { id, tag: 0 }
    }

    /// Preflop all-in showdown (exact 169x169 equity, unchanged from the
    /// trunk) when both contributions already equal the effective stack;
    /// otherwise a `Chance(T1)` node into the flop's bucket-space betting
    /// (the one change from the trunk's own `showdown_or_continuation`).
    fn showdown_or_continuation(&mut self, state: &LineState) -> TempNode {
        let contrib = state.contrib;
        let all_in = contrib[Player::P0] == self.trunk.effective_stack
            || contrib[Player::P1] == self.trunk.effective_stack;
        if all_in {
            let descriptor = TerminalDescriptor {
                kind: TerminalKind::Showdown,
                street: Street::Flop,
                pot: contrib[Player::P0] + contrib[Player::P1],
                contrib,
                stacks_before: PerPlayer::new(
                    self.trunk.effective_stack,
                    self.trunk.effective_stack,
                ),
            };
            let baked = self.pipeline.bake(&descriptor);
            let coef = PerPlayer::new(
                showdown_coef(&baked, Player::P0),
                showdown_coef(&baked, Player::P1),
            );
            let id = self.evaluator.push_terminal(TermSpace::Preflop, coef);
            TempNode::Terminal { id, tag: 0 }
        } else {
            let history = self.extend_history(&state.history, "/");
            let flop_state = PostState {
                street: Street::Flop,
                to_act: Player::P1,
                total_before_street: contrib,
                street_contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
                raises_used: 0,
                checked: false,
                history,
            };
            let child = self.post_betting(flop_state);
            TempNode::Chance {
                deals: vec![(
                    1.0,
                    PerPlayer::new(ReachMap::Transition(T1), ReachMap::Transition(T1)),
                    child,
                )],
                tag: 0,
            }
        }
    }

    // --- postflop bucket-space betting --------------------------------

    fn post_betting(&mut self, state: PostState) -> TempNode {
        let actor = state.to_act;
        let opp = actor.opponent();
        let mut actions: Vec<(String, TempNode)> = Vec::new();
        let outstanding = state.street_contrib[opp] - state.street_contrib[actor];

        if outstanding == Chips::ZERO {
            let history = self.extend_history(&state.history, "x");
            let label = self.action_label(|| "Check".into());
            if state.checked {
                let next = PostState {
                    history,
                    ..state.clone()
                };
                actions.push((label, self.street_end(next)));
            } else {
                let next = PostState {
                    to_act: opp,
                    checked: true,
                    history,
                    ..state.clone()
                };
                actions.push((label, self.post_betting(next)));
            }
        } else {
            // Fold terminals are untagged (no node_info entry), so — like
            // the trunk's own `fold_terminal` call — there's no need to
            // extend `history` with an "f" token that would never be read.
            actions.push((
                self.action_label(|| "Fold".into()),
                self.post_fold_terminal(&state, actor),
            ));

            let mut street_contrib = state.street_contrib;
            street_contrib[actor] = street_contrib[opp];
            let call_history = self.extend_history(&state.history, "c");
            let call_state = PostState {
                street_contrib,
                history: call_history,
                ..state.clone()
            };
            actions.push((
                self.action_label(|| "Call".into()),
                self.street_end(call_state),
            ));
        }

        for to in post_raise_targets(&state, self.post, self.trunk.effective_stack) {
            let mut street_contrib = state.street_contrib;
            street_contrib[actor] = to;
            let history = self.extend_history(&state.history, &format!("b{}", to.0));
            let label = self.action_label(|| {
                if outstanding == Chips::ZERO {
                    format!("Bet {to}")
                } else {
                    format!("Raise to {to}")
                }
            });
            let next = PostState {
                street_contrib,
                to_act: opp,
                raises_used: state.raises_used + 1,
                history,
                ..state.clone()
            };
            actions.push((label, self.post_betting(next)));
        }

        self.finish_action_node(actor, state.history, actions)
    }

    fn post_fold_terminal(&mut self, state: &PostState, folder: Player) -> TempNode {
        let contrib = PerPlayer::new(
            state.total_contrib(Player::P0),
            state.total_contrib(Player::P1),
        );
        let descriptor = TerminalDescriptor {
            kind: TerminalKind::Fold { folder },
            street: state.street,
            pot: contrib[Player::P0] + contrib[Player::P1],
            contrib,
            stacks_before: PerPlayer::new(self.trunk.effective_stack, self.trunk.effective_stack),
        };
        let baked = self.pipeline.bake(&descriptor);
        let coef = PerPlayer::new(fold_coef(&baked, Player::P0), fold_coef(&baked, Player::P1));
        let id = self
            .evaluator
            .push_terminal(term_space_of(state.street), coef);
        TempNode::Terminal { id, tag: 0 }
    }

    /// Showdown terminal in the current street's bucket space: reached
    /// either at the river (forced showdown) or earlier once a player is
    /// all-in (composed equity — see the module docs).
    fn showdown_terminal_post(&mut self, state: &PostState) -> TempNode {
        let contrib = PerPlayer::new(
            state.total_contrib(Player::P0),
            state.total_contrib(Player::P1),
        );
        let descriptor = TerminalDescriptor {
            kind: TerminalKind::Showdown,
            street: state.street,
            pot: contrib[Player::P0] + contrib[Player::P1],
            contrib,
            stacks_before: PerPlayer::new(self.trunk.effective_stack, self.trunk.effective_stack),
        };
        let baked = self.pipeline.bake(&descriptor);
        let coef = PerPlayer::new(
            showdown_coef(&baked, Player::P0),
            showdown_coef(&baked, Player::P1),
        );
        let id = self
            .evaluator
            .push_terminal(term_space_of(state.street), coef);
        TempNode::Terminal { id, tag: 0 }
    }

    /// After a call or check-check: showdown (river, or earlier if either
    /// player is now all-in — both use the *current* street's bucket
    /// space), otherwise deal the next street's chance node.
    fn street_end(&mut self, state: PostState) -> TempNode {
        let all_in = state.total_contrib(Player::P0) == self.trunk.effective_stack
            || state.total_contrib(Player::P1) == self.trunk.effective_stack;
        if state.street == Street::River || all_in {
            self.showdown_terminal_post(&state)
        } else {
            self.deal_chance(state)
        }
    }

    fn deal_chance(&mut self, state: PostState) -> TempNode {
        let transition = match state.street {
            Street::Flop => T2,
            Street::Turn => T3,
            _ => unreachable!("deal_chance is only called before the river"),
        };
        let next = next_street(state.street);
        let history = self.extend_history(&state.history, "/");
        let total_before_street = PerPlayer::new(
            state.total_contrib(Player::P0),
            state.total_contrib(Player::P1),
        );
        let next_state = PostState {
            street: next,
            to_act: Player::P1,
            total_before_street,
            street_contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
            raises_used: 0,
            checked: false,
            history,
        };
        let child = self.post_betting(next_state);
        TempNode::Chance {
            deals: vec![(
                1.0,
                PerPlayer::new(
                    ReachMap::Transition(transition),
                    ReachMap::Transition(transition),
                ),
                child,
            )],
            tag: 0,
        }
    }
}

// --- memory usage dry run --------------------------------------------------

/// Counting-only mirror of [`Builder`]'s recursion (both preflop and
/// postflop), sharing [`base_actions`]/[`raise_targets`]/[`post_raise_targets`]
/// so node/terminal/storage counts can never disagree with the real
/// builder (trunk convention, see `crate::trunk::Counting`).
struct Counting<'a> {
    trunk: &'a PreflopConfig,
    post: &'a PostflopBets,
    kf: usize,
    kt: usize,
    kr: usize,
    nodes: u64,
    terminals: u64,
    elements: u64,
    action_nodes: u64,
}

impl Counting<'_> {
    fn betting(&mut self, state: LineState) {
        self.nodes += 1;
        self.action_nodes += 1;
        let actor = state.to_act;
        let opp = actor.opponent();
        let mut num_actions: u64 = 0;

        match base_actions(&state, self.trunk) {
            BaseActions::Check => {
                num_actions += 1;
                self.showdown_or_continuation(&state);
            }
            BaseActions::FoldThenLimp { allow_limp } => {
                num_actions += 1;
                self.terminal();
                if allow_limp {
                    num_actions += 1;
                    let mut contrib = state.contrib;
                    contrib[actor] = self.trunk.bb;
                    let next = LineState {
                        contrib,
                        to_act: opp,
                        bb_option: true,
                        history: String::new(),
                        ..state.clone()
                    };
                    self.betting(next);
                }
            }
            BaseActions::FoldThenCall => {
                num_actions += 2;
                self.terminal();
                let mut contrib = state.contrib;
                contrib[actor] = state.contrib[opp];
                let next = LineState {
                    contrib,
                    history: String::new(),
                    ..state.clone()
                };
                self.showdown_or_continuation(&next);
            }
        }

        let raises = raise_targets(&state, self.trunk);
        num_actions += raises.len() as u64;
        for t in raises {
            let mut contrib = state.contrib;
            contrib[actor] = t;
            let next = LineState {
                contrib,
                raises_used: state.raises_used + 1,
                prev_raise_to: state.last_raise_to,
                last_raise_to: t,
                to_act: opp,
                bb_option: false,
                history: String::new(),
            };
            self.betting(next);
        }

        self.elements += num_actions * NUM_CLASSES as u64;
    }

    fn showdown_or_continuation(&mut self, state: &LineState) {
        let contrib = state.contrib;
        let all_in = contrib[Player::P0] == self.trunk.effective_stack
            || contrib[Player::P1] == self.trunk.effective_stack;
        if all_in {
            self.terminal();
        } else {
            self.nodes += 1; // Chance(T1)
            self.post_betting(PostState {
                street: Street::Flop,
                to_act: Player::P1,
                total_before_street: contrib,
                street_contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
                raises_used: 0,
                checked: false,
                history: String::new(),
            });
        }
    }

    fn post_betting(&mut self, state: PostState) {
        self.nodes += 1;
        self.action_nodes += 1;
        let actor = state.to_act;
        let opp = actor.opponent();
        let mut num_actions: u64 = 0;
        let dim = bucket_dim_for_street(state.street, self.kf, self.kt, self.kr);

        let outstanding = state.street_contrib[opp] - state.street_contrib[actor];
        if outstanding == Chips::ZERO {
            num_actions += 1;
            if state.checked {
                self.street_end(state.clone());
            } else {
                let next = PostState {
                    to_act: opp,
                    checked: true,
                    history: String::new(),
                    ..state
                };
                self.post_betting(next);
            }
        } else {
            num_actions += 2;
            self.terminal();
            let mut street_contrib = state.street_contrib;
            street_contrib[actor] = street_contrib[opp];
            let call_state = PostState {
                street_contrib,
                history: String::new(),
                ..state
            };
            self.street_end(call_state);
        }

        let raises = post_raise_targets(&state, self.post, self.trunk.effective_stack);
        num_actions += raises.len() as u64;
        for to in raises {
            let mut street_contrib = state.street_contrib;
            street_contrib[actor] = to;
            let next = PostState {
                street_contrib,
                to_act: opp,
                raises_used: state.raises_used + 1,
                history: String::new(),
                ..state.clone()
            };
            self.post_betting(next);
        }

        self.elements += num_actions * dim as u64;
    }

    fn street_end(&mut self, state: PostState) {
        let all_in = state.total_contrib(Player::P0) == self.trunk.effective_stack
            || state.total_contrib(Player::P1) == self.trunk.effective_stack;
        if state.street == Street::River || all_in {
            self.terminal();
        } else {
            self.nodes += 1; // chance node into the next street
            let next = next_street(state.street);
            let total_before_street = PerPlayer::new(
                state.total_contrib(Player::P0),
                state.total_contrib(Player::P1),
            );
            self.post_betting(PostState {
                street: next,
                to_act: Player::P1,
                total_before_street,
                street_contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
                raises_used: 0,
                checked: false,
                history: String::new(),
            });
        }
    }

    fn terminal(&mut self) {
        self.nodes += 1;
        self.terminals += 1;
    }
}

/// Sizes the blueprint tree without building it (dry run sharing the
/// action-enumeration code with the real builder, trunk convention).
pub fn memory_usage(
    trunk: &PreflopConfig,
    post: &PostflopBets,
    artifacts: &BlueprintArtifacts,
) -> MemoryEstimate {
    let mut counting = Counting {
        trunk,
        post,
        kf: artifacts.class_to_flop.out_dim as usize,
        kt: artifacts.flop_to_turn.out_dim as usize,
        kr: artifacts.turn_to_river.out_dim as usize,
        nodes: 0,
        terminals: 0,
        elements: 0,
        action_nodes: 0,
    };
    counting.betting(root_line_state(trunk));
    MemoryEstimate {
        f32_bytes: F32Storage::bytes_for(
            counting.elements as usize,
            counting.action_nodes as usize,
        ),
        i16_bytes: I16Storage::bytes_for(
            counting.elements as usize,
            counting.action_nodes as usize,
        ),
        nodes: counting.nodes,
        terminals: counting.terminals,
    }
}

/// Builds the blueprint game.
///
/// The trunk's betting recursion is reused verbatim; its continuation
/// terminals become `Chance(T1)` nodes into the flop street. `table` feeds
/// the exact preflop fold/all-in terminals exactly as in
/// [`crate::build_preflop_game`]; `artifacts` feeds the transitions and
/// bucket showdowns. `zero_sum` is `pipeline.is_zero_sum()` (bucket
/// showdown blends preserve it — see module docs).
///
/// Panics on inconsistent configs (trunk asserts, artifact dimension
/// mismatches, or a postflop street with zero legal actions).
pub fn build_blueprint_game(
    trunk: &PreflopConfig,
    post: &PostflopBets,
    table: &EquityTable,
    artifacts: &BlueprintArtifacts,
    pipeline: PayoffPipeline<'_>,
) -> BlueprintGame {
    assert!(
        trunk.sb < trunk.bb,
        "small blind ({:?}) must be less than the big blind ({:?})",
        trunk.sb,
        trunk.bb
    );
    assert!(
        trunk.effective_stack > trunk.bb,
        "effective stack ({:?}) must exceed the big blind ({:?})",
        trunk.effective_stack,
        trunk.bb
    );

    let root_ranges = PerPlayer::new(
        classes::class_mass(&trunk.ranges[Player::P0]),
        classes::class_mass(&trunk.ranges[Player::P1]),
    );
    let mass = root_ranges.as_ref().map(|r| r.iter().sum::<f32>());
    assert!(
        mass[Player::P0] > 0.0,
        "P0 (SB) range must have positive mass"
    );
    assert!(
        mass[Player::P1] > 0.0,
        "P1 (BB) range must have positive mass"
    );

    let evaluator = BlueprintEvaluator::new(table, artifacts);
    let transitions = vec![
        sparse_transition(&artifacts.class_to_flop),
        sparse_transition(&artifacts.flop_to_turn),
        sparse_transition(&artifacts.turn_to_river),
    ];

    let mut builder = Builder {
        trunk,
        post,
        pipeline,
        evaluator,
        node_info: vec![PreflopNodeInfo {
            history: "<untagged>".into(),
            actions: Vec::new(),
        }],
    };

    let root = builder.betting(root_line_state(trunk));

    let tree = PublicTree::compile(TreeSpec {
        root,
        masks: Vec::new(),
        transitions,
        root_dims: PerPlayer::new(NUM_CLASSES as u32, NUM_CLASSES as u32),
    });
    assert!(
        tree.node(0).num_children >= 2,
        "root must have at least two legal actions"
    );

    let normalizer_value = normalizer(&root_ranges);
    let zero_sum = builder.pipeline.is_zero_sum();

    let Builder {
        evaluator,
        node_info,
        ..
    } = builder;

    BlueprintGame {
        game: CompiledGame {
            tree,
            evaluator,
            root_ranges: root_ranges.map(|r| r.to_vec()),
            normalizer: normalizer_value,
            zero_sum,
        },
        node_info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abstraction::BucketEquity;
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    /// Small synthetic artifacts (Kf=3, Kt=2, Kr=2): T1 rows sum to kappa
    /// (1225/1326), T2/T3 rows sum to 1, river equity satisfies `win + tie +
    /// win^T == 1` (tie symmetric). Deterministic (seeded), not meant to
    /// resemble a real abstraction — only the invariants matter.
    fn synthetic_artifacts() -> BlueprintArtifacts {
        let kappa = 1225.0 / 1326.0;
        let kf = 3usize;
        let kt = 2usize;
        let kr = 2usize;

        // T1: 169 -> 3, row sums = kappa. Cycle buckets 0..kf so every
        // class has a nonzero row.
        let mut t1 = Vec::new();
        for h in 0..NUM_CLASSES {
            let b = h % kf;
            t1.push((h as u32, b as u32, kappa as f32));
        }
        let class_to_flop = TransitionTable {
            in_dim: NUM_CLASSES as u32,
            out_dim: kf as u32,
            entries: t1,
        };

        // T2: 3 -> 2, row-stochastic (two entries per row, 0.5/0.5 unless
        // the split undershoots dim 2, still summing to 1).
        let flop_to_turn = TransitionTable {
            in_dim: kf as u32,
            out_dim: kt as u32,
            entries: vec![
                (0, 0, 0.7),
                (0, 1, 0.3),
                (1, 0, 0.4),
                (1, 1, 0.6),
                (2, 0, 0.5),
                (2, 1, 0.5),
            ],
        };

        // T3: 2 -> 2, row-stochastic.
        let turn_to_river = TransitionTable {
            in_dim: kt as u32,
            out_dim: kr as u32,
            entries: vec![(0, 0, 0.8), (0, 1, 0.2), (1, 0, 0.35), (1, 1, 0.65)],
        };

        // River equity: win + tie + win^T == 1, tie symmetric. Diagonal
        // pairs are their own reciprocal, so they need an exact 50/50 win
        // split around the tie mass (`2 * win[a][a] + tie[a][a] == 1`);
        // off-diagonal pairs just need `win[a][b] + tie[a][b] + win[b][a]
        // == 1` (picked win[0][1] = 0.6 and solved win[1][0] from it).
        let win = vec![0.45, 0.6, 0.35, 0.4];
        let tie = vec![0.1, 0.05, 0.05, 0.2];
        let river_equity = BucketEquity {
            dim: kr as u32,
            win,
            tie,
        };

        BlueprintArtifacts {
            class_to_flop,
            flop_to_turn,
            turn_to_river,
            river_equity,
        }
    }

    /// Deterministic asymmetric tie-free preflop table (reused pattern from
    /// `trunk.rs`'s tests).
    fn synthetic_table() -> EquityTable {
        let s = |x: usize| (x + 1) as f64;
        let mut win = vec![0.0f64; NUM_CLASSES * NUM_CLASSES];
        for h in 0..NUM_CLASSES {
            for o in 0..NUM_CLASSES {
                win[h * NUM_CLASSES + o] = 0.25 + 0.5 * s(h) / (s(h) + s(o));
            }
        }
        EquityTable::from_probabilities(win, vec![0.0; NUM_CLASSES * NUM_CLASSES])
    }

    // --- composition invariants -------------------------------------------

    #[test]
    fn composed_matrices_satisfy_probability_split_identity() {
        let artifacts = synthetic_artifacts();
        let table = synthetic_table();
        let evaluator = BlueprintEvaluator::new(&table, &artifacts);

        for (win, tie, dim) in [
            (
                &evaluator.river_win,
                &evaluator.river_tie,
                evaluator.river_dim,
            ),
            (&evaluator.turn_win, &evaluator.turn_tie, evaluator.turn_dim),
            (&evaluator.flop_win, &evaluator.flop_tie, evaluator.flop_dim),
        ] {
            for a in 0..dim {
                for b in 0..dim {
                    let sum =
                        win[a * dim + b] as f64 + tie[a * dim + b] as f64 + win[b * dim + a] as f64;
                    assert!(
                        (sum - 1.0).abs() < 1e-6,
                        "dim {dim}: ({a},{b}) win+tie+win^T = {sum}"
                    );
                }
            }
        }
    }

    #[test]
    fn hand_computed_2x2_composition_matches() {
        // A tiny hand-computed check of `compose_equity`: T (2x2, row-
        // stochastic) composed with a known 2x2 child.
        let trans = vec![0.8, 0.2, 0.3, 0.7]; // 2x2, rows sum to 1
        let child = vec![0.6, 0.1, 0.1, 0.6]; // 2x2 symmetric-ish
        let got = compose_equity(&trans, 2, 2, &child);

        // parent[a][b] = sum_ij trans[a][i] child[i][j] trans[b][j], by hand.
        let expected = |a: usize, b: usize| -> f64 {
            let mut acc = 0.0;
            for i in 0..2 {
                for j in 0..2 {
                    acc += trans[a * 2 + i] * child[i * 2 + j] * trans[b * 2 + j];
                }
            }
            acc
        };
        for a in 0..2 {
            for b in 0..2 {
                assert!(
                    (got[a * 2 + b] - expected(a, b)).abs() < 1e-12,
                    "({a},{b}): got {} expected {}",
                    got[a * 2 + b],
                    expected(a, b)
                );
            }
        }
    }

    // --- evaluator differentials -------------------------------------------

    fn naive_bucket_eval(
        win: &[f32],
        tie: &[f32],
        dim: usize,
        coef: TermCoef,
        opp_reach: &[f32],
    ) -> Vec<f32> {
        let mut out = vec![0f32; dim];
        for h in 0..dim {
            let mut acc = 0f64;
            for o in 0..dim {
                let u =
                    coef.a + coef.b * win[h * dim + o] as f64 + coef.c * tie[h * dim + o] as f64;
                acc += opp_reach[o] as f64 * u;
            }
            out[h] = acc as f32;
        }
        out
    }

    #[test]
    fn eval_matches_naive_triple_loop_per_space() {
        let artifacts = synthetic_artifacts();
        let table = synthetic_table();
        let mut rng = ChaCha8Rng::seed_from_u64(11);

        let random_coef = |rng: &mut ChaCha8Rng| TermCoef {
            a: rng.gen_range(-3.0..3.0),
            b: rng.gen_range(-3.0..3.0),
            c: rng.gen_range(-3.0..3.0),
        };

        // Preflop space: same differential as `PreflopEvaluator`'s own test.
        {
            let mut evaluator = BlueprintEvaluator::new(&table, &artifacts);
            let coef0 = random_coef(&mut rng);
            let coef1 = random_coef(&mut rng);
            let id = evaluator.push_terminal(TermSpace::Preflop, PerPlayer::new(coef0, coef1));
            let opp_reach: Vec<f32> = (0..NUM_CLASSES).map(|_| rng.gen_range(0.0..1.0)).collect();
            let counts = crate::classes::compat_counts();
            let combo_counts = crate::classes::class_combo_counts();
            for (p, coef) in [(Player::P0, coef0), (Player::P1, coef1)] {
                let mut out = vec![0f32; NUM_CLASSES];
                evaluator.eval(id, p, &opp_reach, &mut out);
                let mut expected = vec![0f32; NUM_CLASSES];
                for h in 0..NUM_CLASSES {
                    let mut acc = 0f64;
                    for o in 0..NUM_CLASSES {
                        let idx = h * NUM_CLASSES + o;
                        let compat =
                            counts[idx] as f64 / (combo_counts[h] as f64 * combo_counts[o] as f64);
                        let u = coef.a + coef.b * table.win(h, o) + coef.c * table.tie(h, o);
                        acc += opp_reach[o] as f64 * compat * u;
                    }
                    expected[h] = acc as f32;
                }
                for h in 0..NUM_CLASSES {
                    let tol = 1e-4 * (expected[h] as f64).abs().max(1.0);
                    assert!(
                        (expected[h] as f64 - out[h] as f64).abs() <= tol,
                        "preflop p={p:?} h={h}: expected {} got {}",
                        expected[h],
                        out[h]
                    );
                }
            }
        }

        // Bucket spaces.
        let mut evaluator = BlueprintEvaluator::new(&table, &artifacts);
        for space in [TermSpace::Flop, TermSpace::Turn, TermSpace::River] {
            let (win, tie, dim) = match space {
                TermSpace::Flop => (
                    evaluator.flop_win.clone(),
                    evaluator.flop_tie.clone(),
                    evaluator.flop_dim,
                ),
                TermSpace::Turn => (
                    evaluator.turn_win.clone(),
                    evaluator.turn_tie.clone(),
                    evaluator.turn_dim,
                ),
                TermSpace::River => (
                    evaluator.river_win.clone(),
                    evaluator.river_tie.clone(),
                    evaluator.river_dim,
                ),
                TermSpace::Preflop => unreachable!(),
            };
            let coef0 = random_coef(&mut rng);
            let coef1 = random_coef(&mut rng);
            let id = evaluator.push_terminal(space, PerPlayer::new(coef0, coef1));
            let opp_reach: Vec<f32> = (0..dim).map(|_| rng.gen_range(0.0..1.0)).collect();
            for (p, coef) in [(Player::P0, coef0), (Player::P1, coef1)] {
                let mut out = vec![0f32; dim];
                evaluator.eval(id, p, &opp_reach, &mut out);
                let expected = naive_bucket_eval(&win, &tie, dim, coef, &opp_reach);
                for h in 0..dim {
                    let tol = 1e-4 * (expected[h] as f64).abs().max(1.0);
                    assert!(
                        (expected[h] as f64 - out[h] as f64).abs() <= tol,
                        "{space:?} p={p:?} h={h}: expected {} got {}",
                        expected[h],
                        out[h]
                    );
                }
            }
        }
    }

    // --- zero-sum terminal identity ---------------------------------------

    fn push_fold_trunk(stack: Chips) -> PreflopConfig {
        PreflopConfig {
            effective_stack: stack,
            sb: Chips(5),
            bb: Chips(10),
            ranges: PerPlayer::new(cards::Range::full(), cards::Range::full()),
            open_sizes_bb: Vec::new(),
            raise_factors: Vec::new(),
            max_raises: 1,
            include_allin: true,
            allow_limp: false,
            track_node_info: true,
        }
    }

    fn small_postflop_bets() -> PostflopBets {
        PostflopBets {
            flop: PerPlayer::new(vec![0.5], vec![0.5]),
            turn: PerPlayer::new(vec![0.5], vec![0.5]),
            river: PerPlayer::new(vec![0.5], vec![0.5]),
            max_raises: 1,
            include_allin: true,
        }
    }

    #[test]
    fn zero_sum_terminal_identity() {
        use game::{ChipEv, NoRake};

        let trunk = push_fold_trunk(Chips(1000)); // deep enough that push/fold isn't all-in
        let post = small_postflop_bets();
        let table = synthetic_table();
        let artifacts = synthetic_artifacts();
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        };
        let game = build_blueprint_game(&trunk, &post, &table, &artifacts, pipeline);
        assert!(game.game.zero_sum);

        let mut rng = ChaCha8Rng::seed_from_u64(99);
        let dims = (
            NUM_CLASSES,
            artifacts.class_to_flop.out_dim as usize,
            artifacts.flop_to_turn.out_dim as usize,
            artifacts.turn_to_river.out_dim as usize,
        );

        let evaluator = &game.game.evaluator;
        for t in 0..evaluator.terminals.len() as u32 {
            let (space, _) = &evaluator.terminals[t as usize];
            let dim = match space {
                TermSpace::Preflop => dims.0,
                TermSpace::Flop => dims.1,
                TermSpace::Turn => dims.2,
                TermSpace::River => dims.3,
            };
            let r0: Vec<f32> = (0..dim).map(|_| rng.gen_range(0.0..1.0)).collect();
            let r1: Vec<f32> = (0..dim).map(|_| rng.gen_range(0.0..1.0)).collect();
            let mut out0 = vec![0f32; dim];
            let mut out1 = vec![0f32; dim];
            evaluator.eval(t, Player::P0, &r1, &mut out0);
            evaluator.eval(t, Player::P1, &r0, &mut out1);
            let sum0: f64 = out0
                .iter()
                .zip(&r0)
                .map(|(&v, &r)| v as f64 * r as f64)
                .sum();
            let sum1: f64 = out1
                .iter()
                .zip(&r1)
                .map(|(&v, &r)| v as f64 * r as f64)
                .sum();
            let tol = 1e-3 * (1.0 + sum0.abs().max(sum1.abs()));
            assert!(
                (sum0 + sum1).abs() < tol,
                "terminal {t} ({space:?}): p0 {sum0} + p1 {sum1} != 0 (tol {tol})"
            );
        }
    }
}
