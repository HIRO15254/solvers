//! Multi-street (flop/turn/river) exact postflop builder.
//!
//! Generalizes the river-only slice: a subgame now starts on any street (a
//! 3, 4, or 5-card board), runs its own bet grammar per street, and deals
//! chance nodes between streets. Chance branches are merged into
//! suit-isomorphism classes (`iso_merging`): structurally identical
//! turn/river cards share one tree branch as an exact quotient — a merged
//! class becomes a [`ReachMap::Transition`] averaging the members'
//! relabeled reaches, with the class multiplicity in the deal weight —
//! so per-hand reaches, CFVs, and therefore strategies match the unmerged
//! tree exactly (not just range-aggregate values). Merging only happens
//! under suit permutations that fix both players' ranges.
//!
//! The terminal kernels (sorted-rank showdown sweep, inclusion-exclusion
//! fold) live in [`crate::kernel`] and are shared verbatim with the
//! river-only shim in [`crate::river`].

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Index, IndexMut};

use crate::PostflopHands;
use crate::game::{BakedPayoffs, PayoffPipeline, TerminalDescriptor, TerminalKind};
use crate::kernel::{Hand, RankedHands};
use hu_engine::{
    CompiledGame, NodeId, PublicTree, ReachMap, SparseTransition, TempNode, TerminalEvaluator,
    TreeSpec,
};
use nlh::iso::{
    Board, DealGroup, SuitPerm, all_suit_perms, deal_groups_with, orbit_perms, permute_combo,
    stabilizer,
};
use nlh::script::{ActionKind, Effect, JamBound, PreviousAggressor, RuleContext};
use nlh::{
    ALL_CARDS, BoardFacts, Card, CardSet, Chips, NUM_COMBOS, PerPlayer, Player, Range, SizeSpec,
    Street, combo_cards, combo_index, rank_of,
};

use crate::kernel;

/// A value per postflop street. `Preflop` is out of scope for this crate
/// (postflop trees start no earlier than the flop) and indexing with it
/// panics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PerStreet<T> {
    pub flop: T,
    pub turn: T,
    pub river: T,
}

impl<T> Index<Street> for PerStreet<T> {
    type Output = T;
    fn index(&self, street: Street) -> &T {
        match street {
            Street::Preflop => unreachable!("postflop trees never index Preflop"),
            Street::Flop => &self.flop,
            Street::Turn => &self.turn,
            Street::River => &self.river,
        }
    }
}

impl<T> IndexMut<Street> for PerStreet<T> {
    fn index_mut(&mut self, street: Street) -> &mut T {
        match street {
            Street::Preflop => unreachable!("postflop trees never index Preflop"),
            Street::Flop => &mut self.flop,
            Street::Turn => &mut self.turn,
            Street::River => &mut self.river,
        }
    }
}

/// One street's common-input betting rules.
#[derive(Clone, Debug, Default)]
pub struct StreetTree {
    /// Common-input rules in source order and actor history.
    pub nlh_rules: Box<crate::input::NlhStreetRules>,
}

/// A postflop subgame: starting board (3, 4, or 5 cards fixes the starting
/// street), both ranges, the pot already built, remaining effective stacks,
/// and a bet grammar per street.
#[derive(Clone)]
pub struct PostflopConfig {
    /// 3 (flop), 4 (turn), or 5 (river) distinct cards.
    pub board: Vec<Card>,
    pub ranges: PerPlayer<Range>,
    /// Pot at the start of the subgame. Need not be even: an odd pot splits
    /// as OOP = `pot / 2` (floor), IP = `pot - pot / 2` at every terminal, so
    /// `contrib[P0] + contrib[P1]` always equals `pot` exactly.
    ///
    /// That split is an internal bookkeeping convention with no modelling
    /// content, and a config never states it. `PayoffPipeline::bake`
    /// cancels it out of `stacks_after` algebraically, so it reaches the
    /// baked payoffs only as a per-player constant — the same constant at
    /// every terminal, which no strategy, best response, or exploitability
    /// can see. It survives in exactly one place: the level of the reported
    /// root EV, which [`PostflopConfig::starting_share`] adds back so the
    /// reported number is measured from the start of the subgame (see that
    /// method).
    pub pot: Chips,
    /// Chips behind for each player, at the start of the subgame.
    pub effective_stack: Chips,
    /// Betting grammar per street: the tree-script rules that build each
    /// node's action menu, plus the optional all-in default.
    pub streets: PerStreet<StreetTree>,
    /// Smallest legal opening bet and smallest legal raise increment — the
    /// big blind's role in a game that has no blinds. `SizeSpec::MinRaise`
    /// and the minimum-full-raise bump (see `sized_targets`) are both
    /// anchored on this value.
    pub min_bet: Chips,
    /// Rounding used when resolving computed bet sizes to chips.
    pub chip_rounding: nlh::Rounding,
    /// Number of chips per big blind.
    pub scale: nlh::ChipScale,
    /// Merge turn/river deals into suit-isomorphism classes. River-only
    /// subgames never deal, so this has no effect there.
    pub iso_merging: bool,
    /// Optional suit permutations preserved by profile selectors.
    pub profile_suit_perms: Option<Vec<SuitPerm>>,
    /// Record per-node history/action-label metadata for
    /// [`PostflopGame::node_by_history`]. Costs a `String` + `Vec<String>`
    /// per action node, so large flop trees may want this off.
    pub track_node_info: bool,
    /// The last player to bet or raise before this subgame began. Exists
    /// only to define the `cbet`/`donk` tree-script variables on the
    /// starting street (see `rule_context`); it affects nothing else, and in
    /// particular is unrelated to `StreetTree`'s own `previous_aggressor`
    /// bookkeeping across streets *within* the subgame.
    pub preflop_aggressor: Option<Player>,
}

impl PostflopConfig {
    /// This player's slice of the starting pot, under the internal
    /// floor/ceil split (see [`PostflopConfig::pot`]).
    ///
    /// Solve payoffs are measured from before the pot was built, which is
    /// what keeps an unraked chip-EV game exactly zero-sum. Adding this
    /// share back to a player's root EV re-bases it on the start of the
    /// subgame: "chips this player takes out of the pot, minus the chips
    /// they put in from here". The two re-based EVs then sum to
    /// `pot - E[rake]`, which is the convention PioSOLVER and GTO Wizard
    /// report and what the validation workflow compares against. Because
    /// the solver value already carries `-starting_share`, adding it back
    /// cancels the split exactly: the reported EV is the same whichever way
    /// an odd chip was assigned.
    pub fn starting_share(&self, player: Player) -> Chips {
        let oop = Chips(self.pot.0 / 2);
        match player {
            Player::P0 => oop,
            Player::P1 => self.pot - oop,
        }
    }
}

/// A blank subgame to fill in field by field — an empty board and a zero
/// stack are not a runnable config, so this is a starting point for test
/// fixtures rather than something to hand to `build_postflop_game`.
impl Default for PostflopConfig {
    fn default() -> Self {
        PostflopConfig {
            board: Vec::new(),
            ranges: PerPlayer::new(Range::default(), Range::default()),
            pot: Chips::ZERO,
            effective_stack: Chips::ZERO,
            streets: PerStreet::default(),
            min_bet: Chips(1),
            chip_rounding: nlh::Rounding::Nearest,
            scale: nlh::ChipScale::DEFAULT,
            iso_merging: true,
            profile_suit_perms: None,
            track_node_info: true,
            preflop_aggressor: None,
        }
    }
}

/// Node metadata mirroring the toy games' and river slice's scheme.
#[derive(Clone, Debug)]
pub struct PostflopNodeInfo {
    pub history: String,
    pub actions: Vec<String>,
    /// Street this node belongs to.
    pub street: Street,
    /// Each player's total contribution to the pot at this node, the
    /// starting-pot share included. Used to report the current pot. These
    /// chip amounts must not be added to utility-valued EVs: all nodes use
    /// the original subgame's fixed utility baseline.
    pub contrib: PerPlayer<Chips>,
}

impl Default for PostflopNodeInfo {
    fn default() -> Self {
        PostflopNodeInfo {
            history: String::new(),
            actions: Vec::new(),
            street: Street::Flop,
            contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
        }
    }
}

pub struct PostflopGame {
    pub game: CompiledGame<PostflopEvaluator>,
    pub node_info: Vec<PostflopNodeInfo>,
    /// Which tree-script rules' conditions were ever true during this
    /// build -- see [`RuleHits`]. A caller (the CLI) diffs this against
    /// `config.streets` to name every rule that matched nothing and warn
    /// about it; `memory_usage`'s dry run reports the identical set for the
    /// same config (see that function's doc comment).
    pub rule_hits: RuleHits,
}

impl PostflopGame {
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
}

struct PostflopTerminal {
    board_mask: u64,
    kind: TerminalKind,
    payoffs: BakedPayoffs,
    /// Index into `PostflopEvaluator::rank_tables` for `Showdown`
    /// terminals; `u32::MAX` sentinel for `Fold` terminals, which use the
    /// evaluator's single `fold_combos` list instead (see its doc comment).
    table: u32,
}

struct CompactTerminal {
    board_mask: u64,
    kind: TerminalKind,
    payoff: u32,
    table: u32,
}

/// Exact terminal evaluation over fixed seat-specific compact vectors, generalized
/// from the river-only evaluator to runouts at any street.
pub struct PostflopEvaluator {
    terminals: Vec<CompactTerminal>,
    payoffs: Vec<BakedPayoffs>,
    cfr_precision: hu_engine::CfrPrecision,
    /// Showdown rank tables, deduped by completed 5-card board (see
    /// [`Builder::rank_table_id`]).
    rank_tables: Vec<PerPlayer<RankedHands>>,
    /// Batch-only table lookup. River folds share their board's showdown
    /// table; earlier folds (or boards without a table) keep the MAX sentinel.
    /// The existing terminal table/dispatch and all existing kernels are unchanged.
    batch_tables: Vec<u32>,
    /// Live combos disjoint from the subgame's *starting* board, used by
    /// every fold terminal regardless of street or runout — see the
    /// invariant documented on [`kernel::fold_kernel`].
    fold_combos: PerPlayer<Vec<Hand>>,
    pub hands: PostflopHands,
    pub(crate) mask_cards: Vec<Card>,
    /// Board masks indexed by profile incentive space.
    profile_boards: Vec<u64>,
}

#[cfg(test)]
#[path = "precision_tests.rs"]
mod precision_tests;

thread_local! {
    // Only caller indices, no reaches/outputs or evaluator-owned references.
    // Allocates on first use/growth, then reuses capacity on each worker.
    static TERMINAL_BATCH_ORDER: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

impl TerminalEvaluator for PostflopEvaluator {
    fn supports_compat_mass(&self) -> bool {
        true
    }

    fn compat_mass(&self, space: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        kernel::board_compat_mass(
            &self.fold_combos[p],
            &self.fold_combos[p.opponent()],
            &self.hands.same[p],
            self.profile_boards[space as usize],
            opp_reach,
            out,
        );
    }

    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        self.eval_with_kernel(terminal, p, opp_reach, out, hu_engine::CfrPrecision::F64);
    }
    fn eval_batch(
        &self,
        terminals: &[u32],
        p: Player,
        reaches: &[&[f32]],
        outs: &mut [&mut [f32]],
    ) {
        assert_eq!(terminals.len(), reaches.len());
        assert_eq!(terminals.len(), outs.len());
        // Fold sums retain global-combo order, showdown sums retain rank order.
        let key = |i: usize| {
            let term = &self.terminals[terminals[i] as usize];
            match term.kind {
                TerminalKind::Fold { .. } => (true, term.board_mask),
                TerminalKind::Showdown => (false, term.table as u64),
            }
        };
        TERMINAL_BATCH_ORDER.with_borrow_mut(|order| {
            order.clear();
            order.extend(0..terminals.len());
            order.sort_unstable_by_key(|&i| (key(i), i));
            let mut remaining = order.as_slice();
            while let Some(&first) = remaining.first() {
                let end = remaining.partition_point(|&i| key(i) == key(first));
                let (group, tail) = remaining.split_at(end);
                for indices in group.chunks(4) {
                    if indices.len() == 1 {
                        let i = indices[0];
                        self.eval(terminals[i], p, reaches[i], outs[i]);
                        continue;
                    }
                    let term = &self.terminals[terminals[first] as usize];
                    let fold = if key(first).0 {
                        Some((
                            self.fold_combos[p].as_slice(),
                            self.fold_combos[p.opponent()].as_slice(),
                            term.board_mask,
                        ))
                    } else {
                        None
                    };
                    // Fold batches need no rank data, including fold-only games.
                    let empty = PerPlayer::new(
                        kernel::RankedHands::default(),
                        kernel::RankedHands::default(),
                    );
                    let table = if fold.is_some() {
                        &empty
                    } else {
                        &self.rank_tables[term.table as usize]
                    };
                    macro_rules! run {
                        ($n:literal) => {
                            kernel::exact_batch_kernel::<$n>(
                                &table[p],
                                &table[p.opponent()],
                                fold,
                                &self.hands.same[p],
                                std::array::from_fn(|lane| {
                                    self.cfr_utilities(
                                        &self.terminals[terminals[indices[lane]] as usize],
                                        p,
                                    )
                                }),
                                std::array::from_fn(|lane| reaches[indices[lane]]),
                                outs,
                                indices,
                            )
                        };
                    }
                    match indices.len() {
                        2 => run!(2),
                        3 => run!(3),
                        4 => run!(4),
                        _ => unreachable!(),
                    }
                }
                remaining = tail;
            }
        });
    }
    fn eval_cfr(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        self.eval_with_kernel(terminal, p, opp_reach, out, self.cfr_precision);
    }
    fn eval_cfr_batch(
        &self,
        terminals: &[u32],
        p: Player,
        opp_reaches: &[&[f32]],
        outs: &mut [&mut [f32]],
    ) {
        assert_eq!(terminals.len(), opp_reaches.len());
        assert_eq!(terminals.len(), outs.len());
        if self.cfr_precision != hu_engine::CfrPrecision::F32 {
            for ((&id, &reach), out) in terminals.iter().zip(opp_reaches).zip(outs) {
                self.eval_cfr(id, p, reach, out);
            }
            return;
        }
        TERMINAL_BATCH_ORDER.with_borrow_mut(|order| {
            order.clear();
            for (i, ((&id, &reach), out)) in terminals
                .iter()
                .zip(opp_reaches)
                .zip(outs.iter_mut())
                .enumerate()
            {
                assert_eq!(reach.len(), self.hands.len(p.opponent()));
                assert_eq!(out.len(), self.hands.len(p));
                if self.batch_tables[id as usize] == u32::MAX {
                    self.eval_cfr(id, p, reach, out);
                } else {
                    order.push(i);
                }
            }
            // Preserve increasing caller indices within each table so output
            // slices can be borrowed together once per lane-kernel call.
            let is_fold = |i: usize| {
                matches!(
                    self.terminals[terminals[i] as usize].kind,
                    TerminalKind::Fold { .. }
                )
            };
            // Separate kinds for every call: a fold's arithmetic cannot depend
            // on whether it happens to share the caller's batch with a showdown.
            // CFR subtrees normally contain one table, so this is already an
            // ascending run. Filtering each kind below avoids a kind sort and
            // its repeated terminal/table lookups on these small hot calls.
            order.sort_unstable_by_key(|&i| (self.batch_tables[terminals[i] as usize], i));
            let mut remaining = order.as_slice();
            while let Some(&first) = remaining.first() {
                let table_id = self.batch_tables[terminals[first] as usize];
                let end = remaining
                    .partition_point(|&i| self.batch_tables[terminals[i] as usize] == table_id);
                let (group, tail) = remaining.split_at(end);
                let table = &self.rank_tables[table_id as usize];
                for fold in [false, true] {
                    let mut selected = group.iter().copied().filter(|&i| is_fold(i) == fold);
                    loop {
                        let mut indices = [0; kernel::TERMINAL_LANES];
                        let mut count = 0;
                        for (i, dst) in selected
                            .by_ref()
                            .take(kernel::TERMINAL_LANES)
                            .zip(&mut indices)
                        {
                            *dst = i;
                            count += 1;
                        }
                        if count == 0 {
                            break;
                        }
                        let indices = &indices[..count];
                        let mut reaches = [&[][..]; kernel::TERMINAL_LANES];
                        let mut utilities = [[0.0; 3]; kernel::TERMINAL_LANES];
                        for ((reach, utility), &i) in
                            reaches.iter_mut().zip(&mut utilities).zip(indices)
                        {
                            *reach = opp_reaches[i];
                            let term = &self.terminals[terminals[i] as usize];
                            let u = self.cfr_utilities(term, p);
                            *utility = if fold {
                                [u[0] as f32; 3]
                            } else {
                                u.map(|v| v as f32)
                            };
                        }
                        if fold {
                            kernel::fold_batch_kernel_f32(
                                &table[p],
                                &table[p.opponent()],
                                &self.hands.same[p],
                                &utilities[..indices.len()],
                                &reaches[..indices.len()],
                                outs,
                                indices,
                            );
                        } else {
                            kernel::terminal_batch_kernel_f32(
                                &table[p],
                                &table[p.opponent()],
                                &self.hands.same[p],
                                &utilities[..indices.len()],
                                &reaches[..indices.len()],
                                outs,
                                indices,
                            );
                        }
                    }
                }
                remaining = tail;
            }
        });
    }
    fn eval_cfr_siblings(
        &self,
        terminals: &[u32],
        p: Player,
        opp_reach: &[f32],
        outs: &mut [&mut [f32]],
    ) {
        assert_eq!(terminals.len(), outs.len());
        let pair = if self.cfr_precision == hu_engine::CfrPrecision::F32 && terminals.len() >= 2 {
            terminals.iter().enumerate().find_map(|(f, &id)| {
                let fold = &self.terminals[id as usize];
                if !matches!(fold.kind, TerminalKind::Fold { .. }) {
                    return None;
                }
                terminals.iter().enumerate().find_map(|(s, &id)| {
                    let showdown = &self.terminals[id as usize];
                    (matches!(showdown.kind, TerminalKind::Showdown)
                        && fold.board_mask == showdown.board_mask)
                        .then_some((f, s))
                })
            })
        } else {
            None
        };
        if let Some((f, s)) = pair {
            let fold = &self.terminals[terminals[f] as usize];
            let show = &self.terminals[terminals[s] as usize];
            let show_payoff = &self.payoffs[show.payoff as usize];
            let fold_payoff = &self.payoffs[fold.payoff as usize];
            let utilities = match p {
                Player::P0 => [
                    show_payoff.win_p0[p],
                    show_payoff.tie[p],
                    show_payoff.win_p1[p],
                    fold_payoff.win_p0[p],
                ],
                Player::P1 => [
                    show_payoff.win_p1[p],
                    show_payoff.tie[p],
                    show_payoff.win_p0[p],
                    fold_payoff.win_p1[p],
                ],
            };
            let (fold_out, show_out) = if f < s {
                let (before, after) = outs.split_at_mut(s);
                (&mut *before[f], &mut *after[0])
            } else {
                let (before, after) = outs.split_at_mut(f);
                (&mut *after[0], &mut *before[s])
            };
            debug_assert_eq!(opp_reach.len(), self.hands.len(p.opponent()));
            debug_assert_eq!(fold_out.len(), self.hands.len(p));
            debug_assert_eq!(show_out.len(), self.hands.len(p));
            let table = &self.rank_tables[show.table as usize];
            for &local in &table[p].dead {
                show_out[local as usize] = 0.0;
            }
            kernel::showdown_fold_kernel_relaxed_f32(
                &table[p],
                &table[p.opponent()],
                &self.hands.same[p],
                utilities,
                opp_reach,
                show_out,
                fold_out,
            );
        }
        for (i, (&terminal, out)) in terminals.iter().zip(outs).enumerate() {
            if pair.is_none_or(|(f, s)| i != f && i != s) {
                self.eval_cfr(terminal, p, opp_reach, out);
            }
        }
    }
    fn add_cfr_opponent_terminals(
        &self,
        terminals: &[u32],
        p: Player,
        reaches: &[&[f32]],
        out: &mut [f32],
        tmp: &mut [f32],
    ) {
        assert_eq!(terminals.len(), reaches.len());
        assert_eq!(out.len(), tmp.len());
        debug_assert_eq!(out.len(), self.hands.len(p));
        if self.cfr_precision != hu_engine::CfrPrecision::F32 {
            for (&terminal, &reach) in terminals.iter().zip(reaches) {
                tmp.fill(0.0);
                self.eval_cfr(terminal, p, reach, tmp);
                for (dst, &v) in out.iter_mut().zip(tmp.iter()) {
                    *dst += v;
                }
            }
            return;
        }
        let mut i = 0;
        while i < terminals.len() {
            // Fuse adjacent contributions only, preserving batch order even
            // if a caller supplies more than the engine's two terminals.
            if i + 1 < terminals.len() {
                let a = &self.terminals[terminals[i] as usize];
                let b = &self.terminals[terminals[i + 1] as usize];
                let pair = match (a.kind, b.kind) {
                    (TerminalKind::Fold { .. }, TerminalKind::Showdown) => Some((i, i + 1)),
                    (TerminalKind::Showdown, TerminalKind::Fold { .. }) => Some((i + 1, i)),
                    _ => None,
                };
                if let Some((f, s)) = pair.filter(|_| a.board_mask == b.board_mask) {
                    let fold = &self.terminals[terminals[f] as usize];
                    let show = &self.terminals[terminals[s] as usize];
                    let [win, tie, lose] = self.cfr_utilities(show, p);
                    let [u_fold, _, _] = self.cfr_utilities(fold, p);
                    let table = &self.rank_tables[show.table as usize];
                    debug_assert_eq!(reaches[f].len(), self.hands.len(p.opponent()));
                    debug_assert_eq!(reaches[s].len(), self.hands.len(p.opponent()));
                    kernel::add_showdown_fold_kernel_relaxed_f32(
                        &table[p],
                        &table[p.opponent()],
                        &self.hands.same[p],
                        [win, tie, lose, u_fold],
                        reaches[s],
                        reaches[f],
                        out,
                    );
                    i += 2;
                    continue;
                }
            }
            let term = &self.terminals[terminals[i] as usize];
            let utilities = self.cfr_utilities(term, p);
            debug_assert_eq!(reaches[i].len(), self.hands.len(p.opponent()));
            match term.kind {
                TerminalKind::Fold { .. } => kernel::add_fold_kernel_relaxed_f32(
                    &self.fold_combos[p],
                    &self.fold_combos[p.opponent()],
                    &self.hands.same[p],
                    utilities[0],
                    term.board_mask,
                    reaches[i],
                    out,
                ),
                TerminalKind::Showdown => {
                    let table = &self.rank_tables[term.table as usize];
                    kernel::add_showdown_kernel_relaxed_f32(
                        &table[p],
                        &table[p.opponent()],
                        &self.hands.same[p],
                        utilities,
                        reaches[i],
                        out,
                    );
                }
            }
            i += 1;
        }
    }
    /// Set CFR terminal arithmetic; all evaluation calls remain f64.
    fn set_cfr_precision(&mut self, precision: hu_engine::CfrPrecision) {
        self.cfr_precision = precision;
    }
}
impl PostflopEvaluator {
    /// Heap bytes allocated by the evaluator's tables, by capacity.
    pub fn heap_bytes(&self) -> u64 {
        vec_bytes(&self.terminals)
            + vec_bytes(&self.payoffs)
            + vec_bytes(&self.rank_tables)
            + self
                .rank_tables
                .iter()
                .flat_map(|table| [&table[Player::P0], &table[Player::P1]])
                .map(|ranked| {
                    vec_bytes(&ranked.hands) + vec_bytes(&ranked.groups) + vec_bytes(&ranked.dead)
                })
                .sum::<u64>()
            + vec_bytes(&self.batch_tables)
            + vec_bytes(&self.fold_combos[Player::P0])
            + vec_bytes(&self.fold_combos[Player::P1])
            + self.hands.heap_bytes()
            + vec_bytes(&self.mask_cards)
            + vec_bytes(&self.profile_boards)
    }

    /// Register the board of an incentive node and return its compatibility
    /// space key. Keys remain valid for the lifetime of this evaluator.
    pub fn register_profile_board(&mut self, board: &[Card]) -> u32 {
        let mask = board
            .iter()
            .fold(0u64, |mask, card| mask | (1u64 << card.index()));
        if let Some(index) = self
            .profile_boards
            .iter()
            .position(|&existing| existing == mask)
        {
            return index as u32;
        }
        let index = self.profile_boards.len() as u32;
        self.profile_boards.push(mask);
        index
    }

    // Fold win payoffs are baked identically for both rank orientations.
    fn cfr_utilities(&self, term: &CompactTerminal, p: Player) -> [f64; 3] {
        let payoffs = &self.payoffs[term.payoff as usize];
        match p {
            Player::P0 => [payoffs.win_p0[p], payoffs.tie[p], payoffs.win_p1[p]],
            Player::P1 => [payoffs.win_p1[p], payoffs.tie[p], payoffs.win_p0[p]],
        }
    }
    fn eval_with_kernel(
        &self,
        terminal: u32,
        p: Player,
        opp_reach: &[f32],
        out: &mut [f32],
        variant: hu_engine::CfrPrecision,
    ) {
        debug_assert_eq!(opp_reach.len(), self.hands.len(p.opponent()));
        debug_assert_eq!(out.len(), self.hands.len(p));
        let term = &self.terminals[terminal as usize];
        // Orient payoff constants to the traversing player: `u_win` is p's
        // utility when p's hand wins, etc.
        let [u_win, u_tie, u_lose] = self.cfr_utilities(term, p);
        match term.kind {
            TerminalKind::Fold { .. } => {
                let kernel = match variant {
                    hu_engine::CfrPrecision::F64 => kernel::fold_kernel,
                    hu_engine::CfrPrecision::F32 => kernel::fold_kernel_relaxed_f32,
                };
                kernel(
                    &self.fold_combos[p],
                    &self.fold_combos[p.opponent()],
                    &self.hands.same[p],
                    u_win,
                    term.board_mask,
                    opp_reach,
                    out,
                );
            }
            TerminalKind::Showdown => {
                let table = &self.rank_tables[term.table as usize];
                for &local in &table[p].dead {
                    out[local as usize] = 0.0;
                }
                let kernel = match variant {
                    hu_engine::CfrPrecision::F64 => kernel::showdown_kernel,
                    hu_engine::CfrPrecision::F32 => kernel::showdown_kernel_relaxed_f32,
                };
                kernel(
                    &table[p],
                    &table[p.opponent()],
                    &self.hands.same[p],
                    [u_win, u_tie, u_lose],
                    opp_reach,
                    out,
                );
            }
        }
    }
}

/// Everything about one betting line needed to keep building: which street,
/// the board so far, whose turn, chips already committed this subgame
/// (persists across streets), what's owed this street, and street-local
/// bookkeeping (raise count, whether the street has seen a check, whether
/// either player is already all-in).
#[derive(Clone, Copy)]
struct LineBoard {
    cards: [Card; 5],
    len: u8,
}

impl LineBoard {
    fn new(board: &[Card]) -> Self {
        let mut cards = [Card::from_index(0); 5];
        cards[..board.len()].copy_from_slice(board);
        Self {
            cards,
            len: board.len() as u8,
        }
    }

    fn push(&mut self, card: Card) {
        self.cards[self.len as usize] = card;
        self.len += 1;
    }
}

impl std::ops::Deref for LineBoard {
    type Target = [Card];

    fn deref(&self) -> &[Card] {
        &self.cards[..self.len as usize]
    }
}

#[derive(Clone)]
struct LineState {
    street: Street,
    board: LineBoard,
    /// Suit-permutation-invariant summary of `board`, for the tree-script
    /// board predicates (`paired`, `two_tone`, ...). Computed once per
    /// chance branch -- at the root, and again in `deal_chance` each time a
    /// card is dealt -- and never recomputed per decision node, since it
    /// only ever changes when `board` does.
    board_facts: BoardFacts,
    to_act: Player,
    /// Total chips committed this subgame (all streets), per player —
    /// *not* reset between streets, unlike `outstanding`/`raises_used`.
    contrib: PerPlayer<Chips>,
    outstanding: Chips,
    raises_used: u32,
    first_checked: bool,
    all_in: bool,
    /// Both players' equal cumulative contribution at the moment this
    /// street began. A street always starts right after a call or a
    /// check-check, both of which leave `contrib[P0] == contrib[P1]`, so one
    /// shared value suffices for both players' street-relative wagers (see
    /// `sized_targets`).
    street_start: Chips,
    /// Size of the last full bet/raise increment on this street; `ZERO`
    /// before any bet/raise has happened this street. Reset at each street
    /// start, alongside `street_start`.
    last_full_raise: Chips,
    /// The last player to bet or raise on the PREVIOUS street; `None` when
    /// that street checked through, or when this is the subgame's first
    /// street. Read by the tree-script `donk` variable via `rule_context` --
    /// see `docs/nlh-input-v1.jp.md`'s effectと合法性 chapter.
    previous_aggressor: Option<Player>,
    /// The last player to bet or raise on THIS street so far; `None` until
    /// someone does. Carried forward into the next street's
    /// `previous_aggressor` by `street_end`/`deal_chance`.
    street_aggressor: Option<Player>,
}

impl LineState {
    fn root(config: &PostflopConfig) -> Self {
        let street = match config.board.len() {
            3 => Street::Flop,
            4 => Street::Turn,
            5 => Street::River,
            _ => panic!("board must have 3 (flop), 4 (turn), or 5 (river) cards"),
        };
        Self {
            street,
            board: LineBoard::new(&config.board),
            board_facts: BoardFacts::new(&config.board),
            to_act: Player::P0,
            contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
            outstanding: Chips::ZERO,
            raises_used: 0,
            first_checked: false,
            all_in: false,
            street_start: Chips::ZERO,
            last_full_raise: Chips::ZERO,
            previous_aggressor: config.preflop_aggressor,
            street_aggressor: None,
        }
    }

    fn dealt(&self, card: Card) -> Self {
        let mut board = self.board;
        board.push(card);
        Self {
            street: next_street(self.street),
            board_facts: BoardFacts::new(&board),
            board,
            to_act: Player::P0,
            contrib: self.contrib,
            outstanding: Chips::ZERO,
            raises_used: 0,
            first_checked: false,
            all_in: self.all_in,
            street_start: self.contrib[Player::P0],
            last_full_raise: Chips::ZERO,
            previous_aggressor: self.street_aggressor,
            street_aggressor: None,
        }
    }
}

/// Static dispatch shares street transitions without adding builder work
/// (temporary nodes, labels, histories, or ranks) to the counting walk.
trait TreeWalker {
    type Output;
    fn config(&self) -> &PostflopConfig;
    fn showdown(&mut self, state: &LineState) -> Self::Output;
    fn chance(&mut self, state: LineState, history: &str) -> Self::Output;
    fn decision(&mut self, state: LineState, history: &str) -> Self::Output;
}

fn walk_after_deal<W: TreeWalker>(walker: &mut W, state: LineState, history: &str) -> W::Output {
    if state.street == Street::River {
        walker.showdown(&state)
    } else {
        walker.chance(state, history)
    }
}

fn walk_street_end<W: TreeWalker>(
    walker: &mut W,
    mut state: LineState,
    history: &str,
) -> W::Output {
    if state.contrib[Player::P0] == walker.config().effective_stack
        || state.contrib[Player::P1] == walker.config().effective_stack
    {
        state.all_in = true;
    }
    walk_after_deal(walker, state, history)
}

fn walk_dealt<W: TreeWalker>(
    walker: &mut W,
    state: &LineState,
    card: Card,
    history: &str,
) -> W::Output {
    let child = state.dealt(card);
    if state.all_in {
        walk_after_deal(walker, child, history)
    } else {
        walker.decision(child, history)
    }
}

impl TreeWalker for Builder<'_> {
    type Output = TempNode;
    fn config(&self) -> &PostflopConfig {
        self.config
    }
    fn showdown(&mut self, state: &LineState) -> TempNode {
        self.terminal(state, TerminalKind::Showdown)
    }
    fn chance(&mut self, state: LineState, history: &str) -> TempNode {
        self.deal_chance(state, history)
    }
    fn decision(&mut self, state: LineState, history: &str) -> TempNode {
        self.betting(state, history)
    }
}

impl TreeWalker for Counting<'_> {
    type Output = ();
    fn config(&self) -> &PostflopConfig {
        self.config
    }
    fn showdown(&mut self, state: &LineState) {
        self.terminal(state, TerminalKind::Showdown);
    }
    fn chance(&mut self, state: LineState, _: &str) {
        self.deal_chance(state);
    }
    fn decision(&mut self, state: LineState, _: &str) {
        self.betting(state);
    }
}

/// Betting streets left to play from `street`, inclusive — what Pio's bare
/// `e` geometric size divides the remaining stack across.
fn streets_remaining(street: Street) -> u8 {
    match street {
        Street::Flop => 3,
        Street::Turn => 2,
        Street::River => 1,
        Street::Preflop => unreachable!("postflop trees start no earlier than the flop"),
    }
}

fn next_street(street: Street) -> Street {
    match street {
        Street::Flop => Street::Turn,
        Street::Turn => Street::River,
        Street::River => unreachable!("no chance node follows the river"),
        Street::Preflop => unreachable!("postflop trees start no earlier than the flop"),
    }
}

/// Conditional weight denominator for the deal leaving `street`: cards
/// unseen given *both* hole pairs are already fixed (52 - board - 2 - 2),
/// not the simpler "cards unseen given the board" (49/48) a per-card
/// marginal would suggest. Getting this wrong scales flop EVs by 45/49.
fn deal_denom(street: Street) -> f32 {
    match street {
        Street::Flop => 45.0,
        Street::Turn => 44.0,
        Street::River | Street::Preflop => {
            unreachable!("chance nodes only follow the flop or the turn")
        }
    }
}

/// Suit permutations that leave both players' range weight vectors
/// unchanged. Merging two chance deals is only sound under a permutation
/// that fixes the whole game — board *and* ranges — so this is computed
/// once per build and intersected with each board stabilizer. Exact f32
/// equality is deliberate: ranges are user input, not computed data.
fn range_preserving_perms(ranges: &PerPlayer<Range>) -> Vec<SuitPerm> {
    all_suit_perms()
        .into_iter()
        .filter(|perm| {
            (0..NUM_COMBOS).all(|combo| {
                let (a, b) = combo_cards(combo);
                let mapped = combo_index(
                    a.with_suit(perm[a.suit() as usize]),
                    b.with_suit(perm[b.suit() as usize]),
                );
                Player::BOTH
                    .into_iter()
                    .all(|p| ranges[p].weight(combo) == ranges[p].weight(mapped))
            })
        })
        .collect()
}

fn game_preserving_perms(config: &PostflopConfig) -> Vec<SuitPerm> {
    let mut perms = range_preserving_perms(&config.ranges);
    if let Some(allowed) = &config.profile_suit_perms {
        perms.retain(|p| allowed.contains(p));
    }
    perms
}

/// The next street's candidate cards, grouped into suit-isomorphism classes
/// under `board`'s stabilizer intersected with the range-preserving
/// permutations (or singleton groups when `iso_merging` is off). Shared
/// between the real builder and the memory-usage dry run so the two can
/// never disagree about a chance node's fan-out.
fn chance_groups(board: &[Card], iso_merging: bool, sym: &[SuitPerm]) -> Vec<DealGroup> {
    if iso_merging {
        deal_groups_with(&Board::new(&board[..3], &board[3..]), &[], sym)
    } else {
        let board_set: CardSet = board.iter().copied().collect();
        ALL_CARDS
            .into_iter()
            .filter(|c| !board_set.contains(*c))
            .map(|c| DealGroup {
                representative: c,
                members: vec![c],
            })
            .collect()
    }
}

/// Resolves an explicit size menu at `state` into additional-contribution
/// wagers: every entry of `sizes` resolved to a raise-to target
/// (street-relative, i.e. the actor's total contribution *this street*),
/// bumped up to the minimum full raise, clamped to all-in, sorted, deduped,
/// and filtered down to targets that actually raise the bet faced — the resolution order fixed by
/// `docs/nlh-input-v1.jp.md`'s `[tree]` size-literal table
/// (identical to `mw_preflop::betting::BettingState::legal_actions`'s size
/// resolution, `ToChips`/chip-unit literals instead of `ToBb`/bb). Returned
/// as *additional* contribution over the actor's current street wager — the
/// shape both `Builder::betting` and the counting mirror `Counting::betting`
/// consume — so the two can never disagree about a node's action count.
///
/// Menu selection belongs to `node_actions`. Only stack and wager legality
/// constrain the resolved targets here.
fn sized_targets(state: &LineState, config: &PostflopConfig, sizes: &[SizeSpec]) -> Vec<Chips> {
    let Some(sizing) = sizing(state, config) else {
        return Vec::new();
    };
    let mut targets: Vec<Chips> = Vec::with_capacity(sizes.len());
    for &size in sizes {
        let target = Chips(sizing.context.resolve_u32(size));
        targets.push(target);
    }
    targets.sort_unstable();
    targets.dedup();
    targets.retain(|&target| target > sizing.bet_to_match);
    targets
        .into_iter()
        .map(|target| target - sizing.actor_wager)
        .collect()
}

/// Street-relative inputs for resolving sizes at one decision node.
struct Sizing {
    context: nlh::SizeContext,
    actor_wager: Chips,
    bet_to_match: Chips,
    maximum: Chips,
}

/// The sizing inputs at `state`, or `None` when the actor cannot wager
/// beyond a call.
fn sizing(state: &LineState, config: &PostflopConfig) -> Option<Sizing> {
    let actor = state.to_act;
    let opponent = actor.opponent();
    // Street-relative wagers: `contrib` is cumulative across the whole
    // subgame, but every size literal (and the minimum-full-raise rule) is
    // defined in terms of this street's own wagers.
    let actor_wager = state.contrib[actor] - state.street_start;
    let bet_to_match = state.contrib[opponent] - state.street_start;
    let maximum = config.effective_stack - state.street_start;
    let behind = maximum - actor_wager;
    if behind <= state.outstanding {
        return None;
    }

    let to_call = state.outstanding.min(behind);
    let pot_now = config.pot + state.contrib[Player::P0] + state.contrib[Player::P1];
    let pot_after_call = pot_now + to_call;
    let called_to = actor_wager + to_call;

    // `mw_preflop::betting::BettingState::minimum_full_target`, with
    // `min_bet` playing the big blind's role: the first bet of a street must
    // reach `min_bet`, and every later raise must reach the previous full
    // raise's own increment on top of the bet it faces.
    let minimum = if state.last_full_raise > Chips::ZERO {
        (bet_to_match + state.last_full_raise).min(maximum)
    } else {
        (bet_to_match + config.min_bet).min(maximum)
    };

    Some(Sizing {
        context: nlh::SizeContext {
            big_blind: config.scale.chips_per_bb(),
            called_to: u64::from(called_to.0),
            pot_after_call: u64::from(pot_after_call.0),
            previous_bet: u64::from(bet_to_match.0),
            minimum: u64::from(minimum.0),
            maximum: u64::from(maximum.0),
            effective_stack: u64::from(maximum.0),
            streets_remaining: streets_remaining(state.street),
            rounding: config.chip_rounding,
        },
        actor_wager,
        bet_to_match,
        maximum,
    })
}

/// The `jam` effect: every wager in `actions` whose street target passes
/// `bound` against `threshold`'s street target becomes the all-in wager.
fn jam(
    state: &LineState,
    config: &PostflopConfig,
    actions: &mut [NodeAction],
    bound: JamBound,
    threshold: SizeSpec,
) {
    let Some(sizing) = sizing(state, config) else {
        return;
    };
    let threshold = Chips(sizing.context.threshold_u32(threshold));
    let all_in = sizing.maximum - sizing.actor_wager;
    for action in actions {
        if let NodeAction::Wager(add) = *action
            && bound.holds(sizing.actor_wager + add, threshold)
        {
            *action = NodeAction::Wager(all_in);
        }
    }
}

/// The `merge` effect: walking the wagers in `actions` from the largest
/// down, drop each one within `width` of the last kept wager. Every wager
/// shares the actor's street wager, so comparing the added chips compares
/// the street targets.
fn merge(
    state: &LineState,
    config: &PostflopConfig,
    actions: &mut Vec<NodeAction>,
    width: SizeSpec,
) {
    let Some(sizing) = sizing(state, config) else {
        return;
    };
    let width = u64::from(sizing.context.merge_width_u32(width));
    let mut kept: Vec<u64> = actions
        .iter()
        .filter_map(|action| match *action {
            NodeAction::Wager(add) => Some(u64::from(add.0)),
            _ => None,
        })
        .collect();
    nlh::merge_targets(&mut kept, width);
    actions.retain(|action| match *action {
        NodeAction::Wager(add) => kept.contains(&u64::from(add.0)),
        _ => true,
    });
}

/// One legal action at a decision node. `Wager` carries the *additional*
/// chips the actor puts in over their current contribution — the same
/// quantity `sized_targets` already returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeAction {
    Fold,
    Check,
    Call,
    Wager(Chips),
}

/// The non-aggressive action(s) every node starts from before any wager is
/// considered: `check` with no outstanding bet, `fold`+`call` facing one.
fn base_actions(state: &LineState) -> Vec<NodeAction> {
    if state.outstanding == Chips::ZERO {
        vec![NodeAction::Check]
    } else {
        vec![NodeAction::Fold, NodeAction::Call]
    }
}

/// Sort key reproducing the fixed push order `Builder`/`Counting` have
/// always used: check-or-fold-then-call first, then wagers ascending by
/// size. Rules re-sort by this key after every edit (see `node_actions`), so
/// a script-driven tree still yields the exact same child order, history
/// strings, and `node_info.actions` list as the old fixed-menu grammar would
/// for the same resolved sizes.
fn action_sort_key(action: NodeAction) -> (u8, u32) {
    match action {
        NodeAction::Fold => (0, 0),
        NodeAction::Check => (1, 0),
        NodeAction::Call => (2, 0),
        NodeAction::Wager(chips) => (3, chips.0),
    }
}

/// This node's common postflop values, filled in from `state`/`config`.
/// `state.to_act`: `in_position` and `previous_aggressor` are both read
/// relative to whoever is on the move at this node, not either player fixed.
fn rule_context(state: &LineState, config: &PostflopConfig) -> RuleContext {
    let actor = state.to_act;
    let pot_now = config.pot + state.contrib[Player::P0] + state.contrib[Player::P1];
    let behind = config.effective_stack - state.contrib[actor];
    RuleContext {
        aggressions: state.raises_used,
        in_position: actor == Player::P1,
        spr: if pot_now == Chips::ZERO {
            0.0
        } else {
            behind.as_f64() / pot_now.as_f64()
        },
        pot: pot_now.as_f64(),
        to_call: state.outstanding.min(behind).as_f64(),
        previous_aggressor: match state.previous_aggressor {
            None => PreviousAggressor::None,
            Some(p) if p == actor => PreviousAggressor::Actor,
            Some(_) => PreviousAggressor::Opponent,
        },
        board: state.board_facts,
    }
}

/// Whether each tree-script rule's *condition* ever evaluated true at some
/// decision node during a build walk (`node_actions`) -- one `bool` per
/// rule, indexed by that rule's position within its own street's
/// `StreetTree::nlh_rules`, not the whole script's flat rule list. This is
/// the empirical answer to "did any node satisfy this rule?" --
/// deliberately *not* "did the rule change the action list", since a rule
/// whose condition is true but whose `action` names the wager kind the node
/// doesn't have (see `node_actions`'s inert-rule branch) still had its
/// guard checked and is not what a script author needs warned about; only
/// a condition that is never true anywhere is the silent-authoring mistake
/// `docs/nlh-input-v1.jp.md`'s `[tree]` chapter's dead-rule
/// warning exists for.
///
/// A `Vec<bool>` per street, not a `HashMap`: `node_actions` runs at every
/// decision node of trees with hundreds of thousands of nodes, so recording
/// a hit must be a plain indexed write, not a hashed insert.
pub type RuleHits = PerStreet<Vec<bool>>;

/// A fresh, all-`false` [`RuleHits`] sized to `config`'s three
/// common-input rule lists -- the shape both [`Builder`] and [`Counting`]
/// start their walk from, so they can never disagree about how many rules
/// exist per street.
fn rule_hits_for(config: &PostflopConfig) -> RuleHits {
    let len = |tree: &StreetTree| tree.nlh_rules.rules.len();
    PerStreet {
        flop: vec![false; len(&config.streets.flop)],
        turn: vec![false; len(&config.streets.turn)],
        river: vec![false; len(&config.streets.river)],
    }
}

/// Every legal action at `state`: the base non-aggressive action(s), then
/// this street's tree-script rules applied in source order with per-kind
/// candidates (`docs/nlh-input-v1.jp.md` section 9). Shared by the real
/// builder and the memory-usage dry run; an empty menu is rejected by both
/// checked APIs.
///
/// `hits` records, per rule, whether its condition was ever seen true (see
/// [`RuleHits`]) -- threaded in as an accumulator rather than returned, so
/// this function's return type (and therefore both call sites) stays
/// exactly as it was before the dead-rule diagnostic existed.
fn node_actions(
    state: &LineState,
    config: &PostflopConfig,
    hits: &mut RuleHits,
) -> Vec<NodeAction> {
    let street = &config.streets[state.street];
    let rules = &street.nlh_rules;
    let mut actions = base_actions(state);
    let mut common = rule_context(state, config);
    let remaining = (config.effective_stack - state.contrib[Player::P0])
        .min(config.effective_stack - state.contrib[Player::P1]);
    common.spr = if common.pot == 0.0 {
        f64::INFINITY
    } else {
        remaining.as_f64() / common.pot
    };
    common.pot /= config.scale.chips_per_bb() as f64;
    common.to_call /= config.scale.chips_per_bb() as f64;
    let ctx = crate::input::NlhContext {
        common,
        player: &rules.players[state.to_act],
    };
    let wager_kind = if state.outstanding == Chips::ZERO {
        ActionKind::Bet
    } else {
        ActionKind::Raise
    };
    let matches = |a: &NodeAction, kind| match a {
        NodeAction::Fold => kind == ActionKind::Fold,
        NodeAction::Check => kind == ActionKind::Check,
        NodeAction::Call => kind == ActionKind::Call,
        NodeAction::Wager(_) => kind == wager_kind,
    };
    for (index, rule) in rules.rules.iter().enumerate() {
        if !rule.condition.eval(&ctx) {
            continue;
        }
        hits[state.street][index] = true;
        if rule.effect == Effect::Checkdown {
            actions = vec![if state.outstanding == Chips::ZERO {
                NodeAction::Check
            } else {
                NodeAction::Fold
            }];
        } else {
            let kind = rule
                .action
                .expect("compiled non-checkdown rule has an action");
            if let Effect::Jam(bound) = rule.effect {
                if kind == wager_kind {
                    jam(state, config, &mut actions, bound, rule.sizes[0]);
                }
            } else if rule.effect == Effect::Merge {
                if kind == wager_kind {
                    merge(state, config, &mut actions, rule.sizes[0]);
                }
            } else {
                let candidates = if matches!(kind, ActionKind::Bet | ActionKind::Raise) {
                    if kind == wager_kind {
                        sized_targets(state, config, &rule.sizes)
                            .into_iter()
                            .map(NodeAction::Wager)
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    base_actions(state)
                        .into_iter()
                        .filter(|a| matches(a, kind))
                        .collect()
                };
                match rule.effect {
                    Effect::Add => actions.extend(candidates),
                    Effect::Remove => actions.retain(|a| !matches(a, kind)),
                    Effect::Replace => {
                        actions.retain(|a| !matches(a, kind));
                        actions.extend(candidates);
                    }
                    Effect::Force => actions = candidates,
                    Effect::Checkdown | Effect::Jam(_) | Effect::Merge => unreachable!(),
                }
            }
        }
        actions.sort_by_key(|&a| action_sort_key(a));
        actions.dedup();
    }
    actions
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TreeBuildError {
    #[error("tree rules leave an empty action menu on {street:?} for {player:?}")]
    EmptyMenu { street: Street, player: Player },
    #[error(
        "internal invariant: iso permutation maps {player:?} combo {combo} outside root support"
    )]
    SupportNotClosed { player: Player, combo: usize },
}

/// Builds a compiled game, rejecting any action menu emptied by tree rules.
pub fn try_build_postflop_game(
    config: &PostflopConfig,
    pipeline: PayoffPipeline<'_>,
) -> Result<PostflopGame, TreeBuildError> {
    build_checked(config, pipeline, true)
}

/// Where one [`NodeAction`] leads from `state`. Carries the full state
/// transition (`contrib`, `outstanding`, `raises_used`, `last_full_raise`,
/// `street_aggressor`, `to_act`, `first_checked`) that used to be duplicated
/// between `Builder::betting` and `Counting::betting`. Does NOT touch
/// `history` — building history strings and node info stays with `Builder`,
/// the only walker that has them; callers extend `history` themselves from
/// the returned state's other fields.
enum ChildStep {
    /// Another decision node on this street.
    Betting(LineState),
    /// The street is over: showdown at the river, otherwise a chance node.
    StreetEnd(LineState),
    /// `actor` folded.
    Fold,
}

/// The state-arithmetic core of a betting-tree edge, shared by both walkers.
/// The check action can lead to two different [`ChildStep`]s depending on
/// `state.first_checked`: the first check on a street reopens the betting
/// (`Betting`), while the second check (check-check) ends the street
/// (`StreetEnd`).
fn child_step(state: &LineState, action: NodeAction) -> ChildStep {
    let actor = state.to_act;
    match action {
        NodeAction::Fold => ChildStep::Fold,
        NodeAction::Check => {
            if state.first_checked {
                ChildStep::StreetEnd(state.clone())
            } else {
                ChildStep::Betting(LineState {
                    to_act: actor.opponent(),
                    first_checked: true,
                    ..state.clone()
                })
            }
        }
        NodeAction::Call => {
            let mut contrib = state.contrib;
            contrib[actor] += state.outstanding;
            ChildStep::StreetEnd(LineState {
                contrib,
                outstanding: Chips::ZERO,
                ..state.clone()
            })
        }
        NodeAction::Wager(additional) => {
            let mut contrib = state.contrib;
            contrib[actor] += additional;
            // This raise's increment over the bet it faces (see
            // `sized_targets`'s doc comment for why `additional -
            // state.outstanding` equals `target - bet_to_match` in
            // street-relative terms), which becomes both the new
            // `outstanding` owed and the street's `last_full_raise` for the
            // next minimum-raise computation.
            let increment = additional - state.outstanding;
            ChildStep::Betting(LineState {
                to_act: actor.opponent(),
                contrib,
                outstanding: increment,
                raises_used: state.raises_used + 1,
                last_full_raise: increment,
                street_aggressor: Some(actor),
                ..state.clone()
            })
        }
    }
}

type SharedRankTables = std::sync::Arc<
    parking_lot::Mutex<
        BTreeMap<
            [Card; 5],
            std::sync::Arc<std::sync::OnceLock<std::sync::Arc<PerPlayer<RankedHands>>>>,
        >,
    >,
>;

struct Builder<'a> {
    hands: PostflopHands,
    error: Option<TreeBuildError>,
    config: &'a PostflopConfig,
    pipeline: PayoffPipeline<'a>,
    /// Suit permutations preserving both ranges (see
    /// [`range_preserving_perms`]).
    sym: Vec<SuitPerm>,
    terminals: Vec<PostflopTerminal>,
    rank_tables: Vec<std::sync::Arc<PerPlayer<RankedHands>>>,
    rank_table_ids: BTreeMap<[Card; 5], u32>,
    /// Immutable board tables shared across parallel forks; IDs remain local
    /// until absorb assigns them in serial encounter order.
    shared_rank_tables: SharedRankTables,
    masks: Vec<Vec<f32>>,
    mask_cards: Vec<Card>,
    /// Per-card reach mask, built lazily and cached by card index (at most
    /// 52 masks total regardless of how many chance nodes share a card).
    card_masks: [Option<PerPlayer<u32>>; 52],
    /// Quotient transitions for merged deal classes, interned by
    /// (board, members).
    transitions: Vec<SparseTransition>,
    transition_ids: BTreeMap<(Vec<Card>, Vec<Card>), PerPlayer<u32>>,
    node_info: Vec<PostflopNodeInfo>,
    /// Accumulated by every [`node_actions`] call this walk makes -- see
    /// [`RuleHits`].
    hits: RuleHits,
    /// Build the deals of the first chance node level in parallel (see
    /// [`Self::deal_chance`]); forked builders walk their subtree serially.
    parallel: bool,
}

/// Builds a postflop subgame through the payoff pipeline.
pub fn build_postflop_game(config: &PostflopConfig, pipeline: PayoffPipeline<'_>) -> PostflopGame {
    build_checked(config, pipeline, true).expect("postflop builder invariants")
}
/// `parallel` builds the first chance level's deals concurrently; the
/// result is identical either way.
fn build_checked(
    config: &PostflopConfig,
    pipeline: PayoffPipeline<'_>,
    parallel: bool,
) -> Result<PostflopGame, TreeBuildError> {
    let board_len = config.board.len();
    assert!(
        (3..=5).contains(&board_len),
        "board must have 3 (flop), 4 (turn), or 5 (river) cards"
    );
    let board_set: CardSet = config.board.iter().copied().collect();
    assert_eq!(board_set.len(), board_len, "board must have distinct cards");
    let start_street = match board_len {
        3 => Street::Flop,
        4 => Street::Turn,
        5 => Street::River,
        _ => unreachable!(),
    };

    let zero_sum = pipeline.is_zero_sum();
    let mut builder = Builder {
        hands: PostflopHands::new(&config.board, &config.ranges),
        error: None,
        config,
        pipeline,
        sym: game_preserving_perms(config),
        terminals: Vec::new(),
        rank_tables: Vec::new(),
        rank_table_ids: BTreeMap::new(),
        shared_rank_tables: Default::default(),
        masks: Vec::new(),
        mask_cards: Vec::new(),
        card_masks: [None; 52],
        transitions: Vec::new(),
        transition_ids: BTreeMap::new(),
        node_info: vec![PostflopNodeInfo {
            history: "<untagged>".into(),
            street: start_street,
            ..PostflopNodeInfo::default()
        }],
        hits: rule_hits_for(config),
        parallel,
    };
    let root = builder.betting(LineState::root(config), "");

    if let Some(error) = builder.error.take() {
        return Err(error);
    }
    let hands = builder.hands.clone();
    let ranges = root_ranges(config, &hands);
    let fold = |p: Player| {
        hands
            .combos(p)
            .iter()
            .enumerate()
            .map(|(i, &h)| {
                let (a, b) = combo_cards(h as usize);
                Hand {
                    local: i as u16,
                    cards: [a.index() as u8, b.index() as u8],
                }
            })
            .collect()
    };
    let fold_combos = PerPlayer::new(fold(Player::P0), fold(Player::P1));

    let Builder {
        terminals,
        rank_tables,
        shared_rank_tables,
        masks,
        mask_cards,
        transitions,
        node_info,
        hits,
        ..
    } = builder;
    drop(shared_rank_tables);
    let rank_tables = rank_tables
        .into_iter()
        .map(|table| {
            std::sync::Arc::try_unwrap(table).unwrap_or_else(|_| {
                panic!("rank table must have one owner after builder cache release")
            })
        })
        .collect();

    // Precompute only the new batch dispatch. Fold terminal metadata remains
    // unchanged so every existing CFR/evaluation path retains its behavior.
    let board_tables: BTreeMap<_, _> = terminals
        .iter()
        .filter(|t| matches!(t.kind, TerminalKind::Showdown) && t.board_mask.count_ones() == 5)
        .map(|t| (t.board_mask, t.table))
        .collect();
    let batch_tables = terminals
        .iter()
        .map(|t| {
            if matches!(t.kind, TerminalKind::Showdown) {
                t.table
            } else {
                board_tables.get(&t.board_mask).copied().unwrap_or(u32::MAX)
            }
        })
        .collect();
    let mut payoff_ids = BTreeMap::new();
    let mut payoffs = Vec::new();
    let mut terminals: Vec<CompactTerminal> = terminals
        .into_iter()
        .map(|term| {
            let p = term.payoffs;
            let key = [
                p.win_p0[Player::P0].to_bits(),
                p.win_p0[Player::P1].to_bits(),
                p.tie[Player::P0].to_bits(),
                p.tie[Player::P1].to_bits(),
                p.win_p1[Player::P0].to_bits(),
                p.win_p1[Player::P1].to_bits(),
            ];
            let payoff = *payoff_ids.entry(key).or_insert_with(|| {
                let id = payoffs.len() as u32;
                payoffs.push(p);
                id
            });
            CompactTerminal {
                board_mask: term.board_mask,
                kind: term.kind,
                payoff,
                table: term.table,
            }
        })
        .collect();
    // `collect` can reuse the larger builder allocation in place. Every
    // evaluator table is then held at its exact length: the solve keeps it
    // resident and the memory estimate counts its elements.
    terminals.shrink_to_fit();
    payoffs.shrink_to_fit();
    let mut mask_cards = mask_cards;
    mask_cards.shrink_to_fit();
    let evaluator = PostflopEvaluator {
        cfr_precision: hu_engine::CfrPrecision::F64,
        terminals,
        payoffs,
        rank_tables,
        batch_tables,
        fold_combos,
        hands,
        mask_cards,
        profile_boards: Vec::new(),
    };

    // Joint compatible weight, via the same inclusion-exclusion the fold
    // kernel uses. Only the pair-compat sum — no live-count denominators
    // (the 1/45, 1/44 deal weights already live on the chance branches).
    let (all_total, all_card) =
        kernel::compat_sums(&evaluator.fold_combos[Player::P1], &ranges[Player::P1]);
    let normalizer: f64 = evaluator.fold_combos[Player::P0]
        .iter()
        .map(|h| {
            let idx = h.local as usize;
            let same = evaluator.hands.same[Player::P0][idx];
            let r = if same == crate::hands::ABSENT {
                0.0
            } else {
                ranges[Player::P1][same as usize] as f64
            };
            ranges[Player::P0][idx] as f64
                * (all_total - all_card[h.cards[0] as usize] - all_card[h.cards[1] as usize] + r)
        })
        .sum();
    assert!(normalizer > 0.0, "ranges share no compatible combos");

    let tree = PublicTree::compile(TreeSpec {
        root,
        masks,
        transitions,
        root_dims: PerPlayer::new(
            evaluator.hands.len(Player::P0) as u32,
            evaluator.hands.len(Player::P1) as u32,
        ),
    });

    Ok(PostflopGame {
        game: CompiledGame {
            tree,
            evaluator,
            root_ranges: ranges,
            normalizer,
            zero_sum,
        },
        node_info,
        rule_hits: hits,
    })
}

/// Each player's starting weight of every combo in their support.
pub(crate) fn root_ranges(config: &PostflopConfig, hands: &PostflopHands) -> PerPlayer<Vec<f32>> {
    let range = |p: Player| {
        hands
            .combos(p)
            .iter()
            .map(|&h| config.ranges[p].weight(h as usize))
            .collect()
    };
    PerPlayer::new(range(Player::P0), range(Player::P1))
}

impl Builder<'_> {
    /// Appends a history token, formatted lazily: when `track_node_info` is
    /// off `token` is never called, so callers passing a `format!`-based
    /// closure (bet sizes, chance-deal card labels) pay no allocation or
    /// `Display` work for it — the flag must skip building the strings
    /// themselves, not just their storage in `node_info`.
    fn extend_history(&self, history: &str, token: impl FnOnce() -> String) -> String {
        if self.config.track_node_info {
            format!("{history}{}", token())
        } else {
            String::new()
        }
    }

    /// Same laziness as [`Self::extend_history`] for a node's per-action
    /// labels (only ever read back out of `node_info`, never off `history`).
    fn action_label(&self, make: impl FnOnce() -> String) -> String {
        if self.config.track_node_info {
            make()
        } else {
            String::new()
        }
    }

    fn betting(&mut self, state: LineState, history: &str) -> TempNode {
        let actor = state.to_act;
        let mut actions: Vec<(String, TempNode)> = Vec::new();
        let menu = node_actions(&state, self.config, &mut self.hits);
        if menu.is_empty() && self.error.is_none() {
            self.error = Some(TreeBuildError::EmptyMenu {
                street: state.street,
                player: actor,
            });
        }
        for action in menu {
            // Labels and histories name the actor's street-relative target.
            // Keep both computations lazy when node information is disabled.
            let wager_to = |additional| state.contrib[actor] + additional - state.street_start;
            let label = self.action_label(|| match action {
                NodeAction::Fold => "fold".into(),
                NodeAction::Check => "check".into(),
                NodeAction::Call => "call".into(),
                NodeAction::Wager(additional) => {
                    let to = wager_to(additional);
                    if state.outstanding == Chips::ZERO {
                        format!("bet {to}")
                    } else {
                        format!("raise to {to}")
                    }
                }
            });
            let child_history = self.extend_history(history, || match action {
                NodeAction::Fold => "f".into(),
                NodeAction::Check => "x".into(),
                NodeAction::Call => "c".into(),
                NodeAction::Wager(additional) => format!("r{}", wager_to(additional)),
            });
            let child = match child_step(&state, action) {
                ChildStep::Fold => self.terminal(&state, TerminalKind::Fold { folder: actor }),
                ChildStep::StreetEnd(next) => self.street_end(next, &child_history),
                ChildStep::Betting(next) => self.betting(next, &child_history),
            };
            actions.push((label, child));
        }

        self.finish_action_node(actor, &state, history.to_owned(), actions)
    }

    fn finish_action_node(
        &mut self,
        actor: Player,
        state: &LineState,
        history: String,
        actions: Vec<(String, TempNode)>,
    ) -> TempNode {
        let tag = if self.config.track_node_info {
            let tag = self.node_info.len() as u32;
            self.node_info.push(PostflopNodeInfo {
                history,
                actions: actions.iter().map(|(name, _)| name.clone()).collect(),
                street: state.street,
                contrib: PerPlayer::new(
                    self.config.starting_share(Player::P0) + state.contrib[Player::P0],
                    self.config.starting_share(Player::P1) + state.contrib[Player::P1],
                ),
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

    /// After a call or check-check: showdown at the river, otherwise deal
    /// the next street's chance node. Either way, first latch `all_in` if
    /// either player has committed their whole starting stack.
    fn street_end(&mut self, state: LineState, history: &str) -> TempNode {
        walk_street_end(self, state, history)
    }

    /// Deals the next street. Sibling deals are independent subtrees: on the
    /// first chance level each is built by a forked builder in parallel and
    /// absorbed in deal order. Every interned id (mask, transition, rank
    /// table, terminal, node tag) is assigned in the order a serial walk
    /// first meets it, so the result is identical to the serial build.
    fn deal_chance(&mut self, state: LineState, history: &str) -> TempNode {
        debug_assert_eq!(
            state.contrib[Player::P0],
            state.contrib[Player::P1],
            "a chance node is only ever reached right after a call or a check-check, \
             both of which leave contributions equal"
        );
        let groups = chance_groups(&state.board, self.config.iso_merging, &self.sym);
        let deals = if self.parallel {
            use rayon::prelude::*;
            let built: Vec<_> = groups
                .par_iter()
                .map(|group| {
                    let mut fork = self.fork();
                    let deal = fork.deal(&state, group, history);
                    (fork, deal)
                })
                .collect();
            built
                .into_iter()
                .map(|(fork, deal)| self.absorb(fork, deal))
                .collect()
        } else {
            groups
                .iter()
                .map(|group| self.deal(&state, group, history))
                .collect()
        };
        TempNode::Chance { deals, tag: 0 }
    }

    fn deal(
        &mut self,
        state: &LineState,
        group: &DealGroup,
        history: &str,
    ) -> (f32, PerPlayer<ReachMap>, TempNode) {
        let weight = group.members.len() as f32 / deal_denom(state.street);
        // Singleton classes stay on the cheap shared card-removal mask.
        // Merged classes must NOT be approximated as `k x rep-masked
        // branch`: each member removes different combos and values
        // relabel across members, so per-hand reaches and CFVs would be
        // distorted (only range-aggregate values survive by symmetry).
        // Instead a quotient transition averages the members' relabeled
        // reaches (entry weight 1/k) — the rep branch then sees exactly
        // one member's reach and solves identically to any original
        // member branch — while the class multiplicity k stays in the
        // deal weight, so the backward map reproduces `sum_i v_ci`
        // exactly, per hand.
        let maps = if group.members.len() == 1 {
            let mask_id = self.card_mask(group.representative);
            PerPlayer::new(
                ReachMap::Mask(mask_id[Player::P0]),
                ReachMap::Mask(mask_id[Player::P1]),
            )
        } else {
            let id = self.quotient_transition(&state.board, group);
            PerPlayer::new(
                ReachMap::Transition(id[Player::P0]),
                ReachMap::Transition(id[Player::P1]),
            )
        };

        let child_history = self.extend_history(history, || format!("[{}]", group.representative));
        let child = walk_dealt(self, state, group.representative, &child_history);
        (weight, maps, child)
    }

    /// An empty serial builder for one subtree, sharing this one's inputs.
    fn fork(&self) -> Self {
        Builder {
            hands: self.hands.clone(),
            error: None,
            config: self.config,
            pipeline: PayoffPipeline {
                rake: self.pipeline.rake,
                utility: self.pipeline.utility,
            },
            sym: self.sym.clone(),
            terminals: Vec::new(),
            rank_tables: Vec::new(),
            rank_table_ids: BTreeMap::new(),
            shared_rank_tables: self.shared_rank_tables.clone(),
            masks: Vec::new(),
            mask_cards: Vec::new(),
            card_masks: [None; 52],
            transitions: Vec::new(),
            transition_ids: BTreeMap::new(),
            node_info: Vec::new(),
            hits: rule_hits_for(self.config),
            parallel: false,
        }
    }

    /// Appends a forked builder's output after everything built so far,
    /// re-interning its masks, transitions and rank tables in the order the
    /// fork created them and shifting its terminal ids and node tags.
    fn absorb(
        &mut self,
        fork: Builder<'_>,
        (weight, mut maps, mut node): (f32, PerPlayer<ReachMap>, TempNode),
    ) -> (f32, PerPlayer<ReachMap>, TempNode) {
        let Builder {
            error,
            terminals,
            rank_tables,
            rank_table_ids,
            masks,
            mask_cards,
            transitions,
            transition_ids,
            node_info,
            hits,
            ..
        } = fork;
        if self.error.is_none() {
            self.error = error;
        }
        crate::prepare::merge_rule_hits(&mut self.hits, &hits);

        // Masks come in (P0, P1) pairs per card.
        let mut mask_map = Vec::with_capacity(masks.len());
        let mut masks = masks.into_iter();
        for card in mask_cards.into_iter().step_by(2) {
            let pair = [
                masks.next().expect("mask pair"),
                masks.next().expect("mask pair"),
            ];
            let ids = match self.card_masks[card.index()] {
                Some(ids) => ids,
                None => {
                    let ids = PerPlayer::new(self.masks.len() as u32, self.masks.len() as u32 + 1);
                    for mask in pair {
                        self.masks.push(mask);
                        self.mask_cards.push(card);
                    }
                    self.card_masks[card.index()] = Some(ids);
                    ids
                }
            };
            mask_map.extend([ids[Player::P0], ids[Player::P1]]);
        }

        // Transitions also come in (P0, P1) pairs, one per interned key.
        let mut keys: Vec<_> = transition_ids.into_iter().collect();
        keys.sort_unstable_by_key(|(_, ids)| ids[Player::P0]);
        let mut transition_map = Vec::with_capacity(transitions.len());
        let mut transitions = transitions.into_iter();
        for (key, _) in keys {
            let pair = [
                transitions.next().expect("transition pair"),
                transitions.next().expect("transition pair"),
            ];
            let ids = match self.transition_ids.get(&key) {
                Some(&ids) => ids,
                None => {
                    let base = self.transitions.len() as u32;
                    self.transitions.extend(pair);
                    let ids = PerPlayer::new(base, base + 1);
                    self.transition_ids.insert(key, ids);
                    ids
                }
            };
            transition_map.extend([ids[Player::P0], ids[Player::P1]]);
        }

        let mut boards: Vec<_> = rank_table_ids.into_iter().collect();
        boards.sort_unstable_by_key(|&(_, id)| id);
        let mut table_map = Vec::with_capacity(rank_tables.len());
        for ((key, _), table) in boards.into_iter().zip(rank_tables) {
            let id = *self.rank_table_ids.entry(key).or_insert_with(|| {
                self.rank_tables.push(table);
                self.rank_tables.len() as u32 - 1
            });
            table_map.push(id);
        }

        let terminal_offset = self.terminals.len() as u32;
        self.terminals
            .extend(terminals.into_iter().map(|mut terminal| {
                if terminal.table != u32::MAX {
                    terminal.table = table_map[terminal.table as usize];
                }
                terminal
            }));
        let tag_offset = if self.config.track_node_info {
            self.node_info.len() as u32
        } else {
            0
        };
        self.node_info.extend(node_info);

        let remap = |map: &mut ReachMap| match map {
            ReachMap::Mask(id) => *id = mask_map[*id as usize],
            ReachMap::Transition(id) => *id = transition_map[*id as usize],
            ReachMap::Identity => {}
        };
        for p in Player::BOTH {
            remap(&mut maps[p]);
        }
        let mut stack = vec![&mut node];
        while let Some(node) = stack.pop() {
            match node {
                TempNode::Action { children, tag, .. } => {
                    *tag += tag_offset;
                    stack.extend(children.iter_mut());
                }
                TempNode::Chance { deals, .. } => {
                    for (_, maps, child) in deals.iter_mut() {
                        for p in Player::BOTH {
                            remap(&mut maps[p]);
                        }
                        stack.push(child);
                    }
                }
                TempNode::Terminal { id, .. } => *id += terminal_offset,
            }
        }
        (weight, maps, node)
    }

    /// Quotient transition for a merged deal class (see the comment at the
    /// use site in [`Self::deal_chance`]). Interned by (board, members):
    /// identical classes recur across betting lines of the same street.
    fn quotient_transition(&mut self, board: &[Card], group: &DealGroup) -> PerPlayer<u32> {
        let key = (board.to_vec(), group.members.clone());
        if let Some(&id) = self.transition_ids.get(&key) {
            return id;
        }
        let stab: Vec<SuitPerm> = stabilizer(&Board::new(&board[..3], &board[3..]))
            .into_iter()
            .filter(|perm| self.sym.contains(perm))
            .collect();
        let rep = group.representative;
        let perms = orbit_perms(&stab, rep, &group.members);
        let member_avg = 1.0 / perms.len() as f32;
        let mut ids = PerPlayer::new(0, 0);
        for p in Player::BOTH {
            let mut entries = Vec::with_capacity(perms.len() * self.hands.len(p));
            for perm in &perms {
                for (local, &global) in self.hands.combos(p).iter().enumerate() {
                    let (a, b) = combo_cards(global as usize);
                    if a == rep || b == rep {
                        continue;
                    }
                    let mapped = self.hands.local(p, permute_combo(perm, global as usize));
                    if let Some(mapped) = mapped {
                        debug_assert_eq!(
                            self.hands.combos(p)[mapped] as usize,
                            permute_combo(perm, global as usize)
                        );
                        entries.push((mapped as u32, local as u32, member_avg));
                    } else {
                        self.error = Some(TreeBuildError::SupportNotClosed {
                            player: p,
                            combo: global as usize,
                        });
                    }
                }
            }
            ids[p] = self.transitions.len() as u32;
            self.transitions.push(SparseTransition {
                in_dim: self.hands.len(p) as u32,
                out_dim: self.hands.len(p) as u32,
                entries,
            });
        }
        self.transition_ids.insert(key, ids);
        ids
    }
    fn card_mask(&mut self, card: Card) -> PerPlayer<u32> {
        if let Some(ids) = self.card_masks[card.index()] {
            return ids;
        }
        let mut ids = PerPlayer::new(0, 0);
        for p in Player::BOTH {
            ids[p] = self.masks.len() as u32;
            let mask = self
                .hands
                .combos(p)
                .iter()
                .map(|&h| {
                    let (a, b) = combo_cards(h as usize);
                    if a == card || b == card { 0.0 } else { 1.0 }
                })
                .collect();
            self.masks.push(mask);
            self.mask_cards.push(card);
        }
        self.card_masks[card.index()] = Some(ids);
        ids
    }

    fn terminal(&mut self, state: &LineState, kind: TerminalKind) -> TempNode {
        // Starting-pot split for an odd `pot` (see `PostflopConfig::pot`):
        // internal bookkeeping only, and it cancels out of every reported
        // number. What it buys is the zero-sum property — payoffs measured
        // from before the pot was built.
        let oop_share = self.config.starting_share(Player::P0);
        let ip_share = self.config.starting_share(Player::P1);
        let contrib = PerPlayer::new(
            oop_share + state.contrib[Player::P0],
            ip_share + state.contrib[Player::P1],
        );
        let descriptor = TerminalDescriptor {
            kind,
            street: state.street,
            pot: contrib[Player::P0] + contrib[Player::P1],
            contrib,
            stacks_before: PerPlayer::new(
                self.config.effective_stack + oop_share,
                self.config.effective_stack + ip_share,
            ),
        };
        let payoffs = self.pipeline.bake(&descriptor);
        let table = match kind {
            TerminalKind::Fold { .. } => u32::MAX,
            TerminalKind::Showdown => self.rank_table_id(&state.board),
        };
        let id = self.terminals.len() as u32;
        self.terminals.push(PostflopTerminal {
            board_mask: state
                .board
                .iter()
                .fold(0, |mask, c| mask | (1u64 << c.index())),
            kind,
            payoffs,
            table,
        });
        TempNode::Terminal { id, tag: 0 }
    }

    /// Showdown rank table for a completed 5-card board, deduped by the
    /// board's sorted card set (different runout orders — e.g. turn/river
    /// swapped by a different betting line reaching the same five cards —
    /// share one table).
    fn rank_table_id(&mut self, board: &[Card]) -> u32 {
        let board5: [Card; 5] = board
            .try_into()
            .expect("showdown requires a completed 5-card board");
        let mut key = board5;
        key.sort_unstable();
        if let Some(&id) = self.rank_table_ids.get(&key) {
            return id;
        }
        let cell = self
            .shared_rank_tables
            .lock()
            .entry(key)
            .or_default()
            .clone();
        let table = cell
            .get_or_init(|| {
                let board_set: CardSet = board5.iter().copied().collect();
                // Rank every live combo in the union of both seats only once.
                let mut ranks = [None; NUM_COMBOS];
                for p in Player::BOTH {
                    for &global in self.hands.combos(p) {
                        let global = global as usize;
                        if ranks[global].is_some() {
                            continue;
                        }
                        let (a, b) = combo_cards(global);
                        if !board_set.contains(a) && !board_set.contains(b) {
                            ranks[global] = Some(rank_of(board5.iter().copied().chain([a, b])));
                        }
                    }
                }
                let ranked = |p: Player| {
                    let mut sorted = Vec::new();
                    let mut dead = Vec::new();
                    for (local, &global) in self.hands.combos(p).iter().enumerate() {
                        if let Some(rank) = ranks[global as usize] {
                            sorted.push((rank, global, local as u16));
                        } else {
                            dead.push(local as u16);
                        }
                    }
                    sorted.sort_unstable();
                    // Exact capacities: every table stays resident through the
                    // solve and the memory estimate counts its elements.
                    dead.shrink_to_fit();
                    let mut table = RankedHands {
                        hands: Vec::with_capacity(sorted.len()),
                        groups: Vec::new(),
                        dead,
                    };
                    for (rank, global, local) in sorted {
                        if table.groups.last().is_none_or(|&(r, _)| r != rank) {
                            table.groups.push((rank, table.hands.len()));
                        }
                        let (a, b) = combo_cards(global as usize);
                        table.hands.push(Hand {
                            local,
                            cards: [a.index() as u8, b.index() as u8],
                        });
                        table.groups.last_mut().unwrap().1 = table.hands.len();
                    }
                    table.groups.shrink_to_fit();
                    table
                };
                std::sync::Arc::new(PerPlayer::new(ranked(Player::P0), ranked(Player::P1)))
            })
            .clone();
        let id = self.rank_tables.len() as u32;
        self.rank_tables.push(table);
        self.rank_table_ids.insert(key, id);
        id
    }
}

/// Process image (binary, libraries, runtime and allocator state) resident
/// beside the counted structures; the CLI measured 7-9 MiB.
pub(crate) const PROCESS_BYTES: u64 = 32 << 20;
/// The allocator keeps part of the build's freed temporaries resident. With
/// [`PROCESS_BYTES`], this share of the game covers what a one-thread build's
/// RSS holds beyond the game (5-17 MiB in the calibrated trees).
const BUILD_RESIDUE_DIVISOR: u64 = 16;
/// Engine scratch one worker thread retains once it has run CFR and
/// evaluation passes: a fixed part plus buffers per tree level and hand of
/// the wider range. At least 1.5x the per-worker need measured on flop, turn
/// and river trees (experiments/p1-efficiency-2026-10/memory-estimate-20261029).
const THREAD_FIXED_BYTES: u64 = 1 << 20;
const THREAD_LEVEL_HAND_BYTES: u64 = 1792;
/// Viewer-index action labels and history tokens, interned per distinct size.
const LABEL_BYTES: u64 = 256 << 10;
/// Per action node, an installed profile holds a lock and an incentive slot,
/// two subtree prefix sums and the frequency controller's base incentive
/// slot. Lock and incentive values are not counted. The slots are counted
/// twice: resolution's freed temporaries stay in the allocator, and a
/// one-thread lock run measured 1.0-1.5x the slots.
const PROFILE_SLOT_BYTES: u64 = 2
    * (std::mem::size_of::<Option<hu_engine::NodeLock>>()
        + std::mem::size_of::<Option<hu_engine::NodeIncentive>>()
        + 2 * std::mem::size_of::<usize>()
        + std::mem::size_of::<Option<Vec<f32>>>()) as u64;

/// Heap bytes of a vector's allocation.
pub(crate) fn vec_bytes<T>(v: &Vec<T>) -> u64 {
    (v.capacity() * std::mem::size_of::<T>()) as u64
}

// Each wildcard can expand at most one chance node's maximum fan-out.
fn selector_match_bound(selector: &str, max_deals: u64, action_nodes: u64) -> u64 {
    (0..selector.bytes().filter(|&b| b == b'?').count())
        .fold(1u64, |n, _| n.saturating_mul(max_deals))
        .min(action_nodes)
}

/// Storage/node/terminal/rank-table footprint of a [`PostflopConfig`],
/// without actually building the tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEstimate {
    /// Bytes for an `F32Storage` backend (two `f32` arenas).
    pub f32_bytes: u64,
    /// Bytes for a hypothetical quantized `i16` backend: two `i16` arenas
    /// plus a per-action-node `f32` scale pair (regrets, strategy sum).
    pub i16_bytes: u64,
    /// Bytes for i16 regrets, f32 strategy sums, and one regret scale per node.
    pub i16_f32avg_bytes: u64,
    /// Releasable f32 regret arena (4L bytes).
    pub f32_regret_bytes: u64,
    /// Releasable i16 regret arena and node scales (2L + 4N bytes),
    /// shared by i16 and i16-f32avg backends.
    pub i16_regret_bytes: u64,
    /// Packed value slots, mode/street metadata and one bounded parallel
    /// strategy batch for a full `.sol` export.
    pub save_bytes: u64,
    /// The `.sol` writer's bounded streaming codec budget, including the
    /// run's config frames. The checkpoint encoder is [`Self::checkpoint_bytes`].
    pub compression_bytes: u64,
    /// Compiled game resident from build through save: public tree, terminal
    /// evaluator (rank tables, payoffs, masks, transitions) and viewer index.
    pub game_bytes: u64,
    /// Engine scratch one worker thread retains.
    pub thread_bytes: u64,
    /// Worker threads: one from [`memory_usage`]; `prepare` sets the run's count.
    pub threads: u64,
    /// Installed solve profile with its resolution residue: zero from
    /// [`memory_usage`]; `prepare` sets it when the input has a profile.
    pub profile_bytes: u64,
    pub nodes: u64,
    pub action_nodes: u64,
    /// Largest action-node strategy, in f32 elements.
    pub max_strategy_elements: u64,
    /// Most action and chance nodes on one root-to-leaf path.
    pub max_depth: u64,
    /// Most deals of one chance node.
    pub max_deals: u64,
    pub terminals: u64,
    pub rank_tables: u64,
    /// Which tree-script rules' conditions were ever true during this dry
    /// run -- see [`RuleHits`]. Reports the identical set
    /// [`build_postflop_game`] would for the same config (`Counting` and
    /// `Builder` share `node_actions`), which is what lets the CLI print
    /// the dead-rule warning from the cheap preflight, before committing to
    /// a possibly very large real build.
    pub rule_hits: RuleHits,
}

/// Dry-run of [`build_postflop_game`]'s recursion that counts storage
/// elements, nodes, terminals, and distinct showdown boards without
/// constructing any `TempNode`s, chance masks, or rank tables — cheap
/// enough to run as a preflight check before committing to a full build of
/// a large flop tree.
pub fn memory_usage(config: &PostflopConfig) -> MemoryEstimate {
    try_memory_usage(config).expect("postflop tree must have nonempty action menus")
}

impl MemoryEstimate {
    /// Memory resident beside storage from build through save: process
    /// image, compiled game and its build residue, profile slots and every
    /// worker's scratch.
    pub fn resident_bytes(&self) -> u64 {
        PROCESS_BYTES
            .saturating_add(self.game_bytes)
            .saturating_add(self.game_bytes / BUILD_RESIDUE_DIVISOR)
            .saturating_add(self.profile_bytes)
            .saturating_add(self.thread_bytes.saturating_mul(self.threads))
    }

    /// Profile slots and resolution residue for this tree's action nodes.
    pub fn profile_slot_bytes(&self) -> u64 {
        self.action_nodes.saturating_mul(PROFILE_SLOT_BYTES)
    }

    /// Workspace for the river-constraint stream in a no-rivers profile save.
    pub(crate) fn river_constraint_workspace(&self, config: &PostflopConfig) -> u64 {
        let hands = PostflopHands::new(&config.board, &config.ranges);
        let widest = hands.len(Player::P0).max(hands.len(Player::P1)) as u64;
        crate::sol::river_constraint_workspace(
            self.max_strategy_elements,
            self.f32_regret_bytes / 4,
            widest,
            self.action_nodes,
        )
    }

    /// Compact-subtree peak for disjoint selected storage spans, including
    /// both serial and parallel save peaks and the full resident budget.
    pub fn subtree_required_bytes(
        &self,
        backend: crate::input::Storage,
        spans: &[hu_engine::StorageSpan],
    ) -> u64 {
        let (elements, refs) = spans.iter().fold((0u64, 0u64), |(elements, refs), span| {
            (
                elements.saturating_add(span.end.saturating_sub(span.start) as u64),
                refs.saturating_add(span.sref_end.saturating_sub(span.sref_start) as u64),
            )
        });
        let mut compact = self.clone();
        compact.f32_bytes = elements.saturating_mul(8);
        compact.i16_bytes = elements
            .saturating_mul(4)
            .saturating_add(refs.saturating_mul(8));
        compact.i16_f32avg_bytes = elements
            .saturating_mul(6)
            .saturating_add(refs.saturating_mul(4));
        compact.f32_regret_bytes = elements.saturating_mul(4);
        compact.i16_regret_bytes = elements
            .saturating_mul(2)
            .saturating_add(refs.saturating_mul(4));
        compact
            .required_bytes(backend)
            .max(compact.parallel_save_bytes(backend).unwrap_or(u64::MAX))
    }

    /// Freeze-specific upper bound, including both compact and whole-tree
    /// fallback allocations. Selectors are bounded by their possible chance
    /// expansions; their actual matches and fallback reason are known only
    /// after the game and base solution have been loaded.
    pub(crate) fn freeze_bytes(&self, config: &PostflopConfig, profile: &spot::ProfileIr) -> u64 {
        let matches =
            |selector: &str| selector_match_bound(selector, self.max_deals, self.action_nodes);
        let target_bound = profile
            .locks
            .iter()
            .map(|x| matches(&x.node))
            .chain(profile.incentives.iter().map(|x| matches(&x.node)))
            .chain(profile.frequencies.iter().map(|x| matches(&x.node)))
            .fold(0u64, u64::saturating_add)
            .min(self.action_nodes);
        let frequency_bound = profile
            .frequencies
            .iter()
            .map(|x| matches(&x.node))
            .fold(0u64, u64::saturating_add);
        let hands = PostflopHands::new(&config.board, &config.ranges);
        let total_hands = (hands.len(Player::P0) + hands.len(Player::P1)) as u64;
        let widest = hands.len(Player::P0).max(hands.len(Player::P1)) as u64;
        // Frozen ancestors and explicitly locked targets are disjoint action
        // nodes. Each target has at most max_depth ancestors.
        let lock_nodes = target_bound
            .saturating_mul(self.max_depth.saturating_add(1))
            .min(self.action_nodes);
        let lock_values = lock_nodes
            .saturating_mul(self.max_strategy_elements)
            .saturating_mul(std::mem::size_of::<f32>() as u64)
            .min(self.f32_regret_bytes);
        let root_bytes = target_bound.saturating_mul(
            std::mem::size_of::<hu_engine::SubtreeRoot>() as u64
                + total_hands * std::mem::size_of::<f32>() as u64,
        );
        let frequency_bytes = frequency_bound.saturating_mul(
            std::mem::size_of::<crate::profile::FrequencyTarget>() as u64
                + widest * std::mem::size_of::<u32>() as u64
                + 2 * std::mem::size_of::<f64>() as u64,
        );
        self.game_bytes
            // Pinned u16 strategy chunks and their headers/indexes during
            // whole-tree passes; locks cover targets and frozen ancestors.
            .saturating_add(self.f32_regret_bytes / 2)
            .saturating_add(self.action_nodes.saturating_mul(80))
            .saturating_add(4 * 1024 * 1024)
            .saturating_add(lock_values)
            .saturating_add(lock_nodes.saturating_mul(widest * 4))
            .saturating_add(self.max_strategy_elements.saturating_mul(8))
            // Incentives and the controller's copy of their base values.
            .saturating_add(
                if profile.incentives.is_empty() && profile.frequencies.is_empty() {
                    0
                } else {
                    target_bound
                        .saturating_mul(self.max_strategy_elements)
                        .saturating_mul(std::mem::size_of::<f32>() as u64)
                        .min(self.f32_regret_bytes)
                        .saturating_mul(2)
                },
            )
            .saturating_add(frequency_bytes.saturating_mul(2))
            .saturating_add(root_bytes.saturating_mul(2))
            // Compact ref map; non-identity base node and suit maps.
            .saturating_add(self.action_nodes.saturating_mul(16))
            .saturating_add(self.nodes.saturating_mul(16))
            .saturating_add(24 * 2 * nlh::NUM_COMBOS as u64 * 2)
    }

    /// The checkpoint's zstd encoder with the run's worker count, needed
    /// beside all storage by every periodic and final checkpoint.
    pub fn checkpoint_bytes(&self) -> u64 {
        crate::checkpoint::encoder_workspace(self.threads)
    }

    /// Final checkpoint and solution written concurrently, retaining all
    /// storage: S + W + C + E plus the resident memory, where C is the `.sol`
    /// writer's budget and E the checkpoint encoder's. W already includes the
    /// bounded Rayon strategy batch. No state snapshot is allocated. Overflow
    /// means the parallel peak cannot fit a u64 memory limit.
    pub fn parallel_save_bytes(&self, backend: crate::input::Storage) -> Option<u64> {
        let storage = match backend {
            crate::input::Storage::F32 => self.f32_bytes,
            crate::input::Storage::I16 => self.i16_bytes,
            crate::input::Storage::I16F32Avg => self.i16_f32avg_bytes,
        };
        storage
            .checked_add(self.save_bytes)?
            .checked_add(self.compression_bytes)?
            .checked_add(self.checkpoint_bytes())?
            .checked_add(self.resident_bytes())
    }

    /// Peak of solving and checkpointing with all storage (S + E) or saving
    /// after regret release (S - R + W + C), plus the resident memory.
    /// Arithmetic saturates.
    pub fn required_bytes(&self, backend: crate::input::Storage) -> u64 {
        let (storage, regrets) = match backend {
            crate::input::Storage::F32 => (self.f32_bytes, self.f32_regret_bytes),
            crate::input::Storage::I16 => (self.i16_bytes, self.i16_regret_bytes),
            crate::input::Storage::I16F32Avg => (self.i16_f32avg_bytes, self.i16_regret_bytes),
        };
        storage
            .saturating_add(self.checkpoint_bytes())
            .max(
                storage
                    .saturating_sub(regrets)
                    .saturating_add(self.save_bytes)
                    .saturating_add(self.compression_bytes),
            )
            .saturating_add(self.resident_bytes())
    }
}

/// Counting preflight with a typed error for a menu emptied by common rules.
pub fn try_memory_usage(config: &PostflopConfig) -> Result<MemoryEstimate, TreeBuildError> {
    count_tree(config, true)
}
/// A selector walk retains only its frontier, never tree nodes or storage.
#[derive(Clone)]
enum SelectorNode {
    Action(LineState),
    Chance(LineState),
    Terminal,
}

#[derive(Clone)]
struct SelectorTarget {
    path: Vec<usize>,
    node: SelectorNode,
}

fn selector_walk(
    config: &PostflopConfig,
    steps: &[&str],
    history: bool,
    lookup: bool,
) -> Option<Vec<SelectorTarget>> {
    let sym = game_preserving_perms(config);
    let mut current = vec![SelectorTarget {
        path: Vec::new(),
        node: SelectorNode::Action(LineState::root(config)),
    }];
    let mut hits = rule_hits_for(config);
    for &step in steps {
        let mut next = Vec::new();
        for target in current {
            match target.node {
                SelectorNode::Action(state) => {
                    let actions = node_actions(&state, config, &mut hits);
                    let labels: Vec<String> = actions
                        .iter()
                        .map(|action| match action {
                            NodeAction::Fold => "fold".into(),
                            NodeAction::Check => "check".into(),
                            NodeAction::Call => "call".into(),
                            NodeAction::Wager(additional) => {
                                let to =
                                    state.contrib[state.to_act] + *additional - state.street_start;
                                let to = config.scale.amount_text(nlh::MwChips(to.0 as u64));
                                if state.outstanding == Chips::ZERO {
                                    format!("bet {to}")
                                } else {
                                    format!("raise to {to}")
                                }
                            }
                        })
                        .collect();
                    let selected = if history {
                        labels.iter().position(|label| {
                            crate::queries::history_token_matches(
                                &crate::queries::history_token(label),
                                step,
                                config.scale,
                            )
                        })
                    } else {
                        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                        crate::queries::resolve_action(&refs, step).ok()
                    };
                    if let Some(pos) = selected {
                        let node = match child_step(&state, actions[pos]) {
                            ChildStep::Fold => SelectorNode::Terminal,
                            ChildStep::Betting(child) => SelectorNode::Action(child),
                            ChildStep::StreetEnd(mut child) => {
                                if child.contrib[Player::P0] == config.effective_stack
                                    || child.contrib[Player::P1] == config.effective_stack
                                {
                                    child.all_in = true;
                                }
                                if child.street == Street::River {
                                    SelectorNode::Terminal
                                } else {
                                    SelectorNode::Chance(child)
                                }
                            }
                        };
                        let mut path = target.path;
                        path.push(pos);
                        next.push(SelectorTarget { path, node });
                    }
                }
                SelectorNode::Chance(state) => {
                    let wanted = if history {
                        step.strip_prefix('[')
                            .and_then(|s| s.strip_suffix(']'))
                            .unwrap_or(step)
                    } else {
                        step
                    };
                    for (pos, group) in chance_groups(&state.board, config.iso_merging, &sym)
                        .iter()
                        .enumerate()
                    {
                        let cardless = state.all_in && group.members.len() > 1;
                        let label = if cardless {
                            format!("deal{pos}")
                        } else {
                            group.representative.to_string()
                        };
                        let matches = if lookup {
                            if cardless {
                                wanted == label
                            } else {
                                wanted.parse::<Card>().ok() == Some(group.representative)
                            }
                        } else {
                            wanted == "?"
                                || label.eq_ignore_ascii_case(wanted.trim_end_matches('*'))
                        };
                        if !matches {
                            continue;
                        }
                        let child = state.dealt(group.representative);
                        let node = if state.all_in {
                            if child.street == Street::River {
                                SelectorNode::Terminal
                            } else {
                                SelectorNode::Chance(child)
                            }
                        } else {
                            SelectorNode::Action(child)
                        };
                        let mut path = target.path.clone();
                        path.push(pos);
                        next.push(SelectorTarget { path, node });
                    }
                }
                SelectorNode::Terminal => {}
            }
        }
        if next.is_empty() {
            return None;
        }
        current = next;
    }
    Some(current)
}

fn selector_targets(config: &PostflopConfig, selector: &str) -> Option<Vec<SelectorTarget>> {
    if selector.is_empty() || selector == "root" {
        return selector_walk(config, &[], false, false);
    }
    let history_steps = crate::queries::selector_history_steps(selector, config.scale);
    // Try canonical tokenization first, then other equivalent spellings.
    let history = history_steps.iter().find_map(|steps| {
        let targets = selector_walk(config, steps, true, !selector.contains("[?]"))?;
        targets
            .iter()
            .all(|t| matches!(t.node, SelectorNode::Action(_)))
            .then_some(targets)
    });
    let targets = if selector.contains("[?]") {
        history?
    } else if let Some(targets) = history {
        targets
    } else {
        let steps: Vec<_> = selector
            .split('/')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        selector_walk(config, &steps, false, false)?
    };
    targets
        .iter()
        .all(|t| matches!(t.node, SelectorNode::Action(_)))
        .then_some(targets)
}

fn selector_has_hands(
    config: &PostflopConfig,
    state: &LineState,
    selector: &spot::HandSelector,
) -> Option<bool> {
    let range = match selector {
        spot::HandSelector::Range(text) => Some(text.parse::<Range>().ok()?),
        _ => None,
    };
    let category = match selector {
        spot::HandSelector::Category(text) => Some(nlh::category::CategoryExpr::parse(text).ok()?),
        _ => None,
    };
    Some((0..nlh::NUM_COMBOS).any(|combo| {
        let (a, b) = nlh::combo_cards(combo);
        !state.board.contains(&a)
            && !state.board.contains(&b)
            && config.ranges[state.to_act].weight(combo) > 0.0
            && range.as_ref().is_none_or(|r| r.weight(combo) > 0.0)
            && category
                .as_ref()
                .is_none_or(|c| c.matches(&state.board, [a, b]))
    }))
}

/// Count maximal matched target subtrees. Spans are synthetic disjoint
/// intervals: the budget consumes only their lengths, not tree offsets.
pub(crate) fn selector_subtree_spans(
    config: &PostflopConfig,
    profile: &spot::ProfileIr,
) -> Option<Vec<hu_engine::StorageSpan>> {
    let mut selected = Vec::new();
    for lock in &profile.locks {
        for target in selector_targets(config, &lock.node)? {
            let SelectorNode::Action(state) = &target.node else {
                return None;
            };
            let has_hands = match &lock.strategy {
                spot::LockStrategy::Uniform(_) => selector_has_hands(config, state, &lock.hands)?,
                // The per-combo strategy ignores the entry's hand selector.
                spot::LockStrategy::PerCombo(entries) => {
                    let mut found = false;
                    for (text, _) in entries {
                        found |= selector_has_hands(
                            config,
                            state,
                            &spot::HandSelector::Range(text.clone()),
                        )?;
                    }
                    found
                }
            };
            if has_hands {
                selected.push(target);
            }
        }
    }
    for incentive in &profile.incentives {
        for target in selector_targets(config, &incentive.node)? {
            let SelectorNode::Action(state) = &target.node else {
                return None;
            };
            if selector_has_hands(config, state, &incentive.hands)? {
                selected.push(target);
            }
        }
    }
    selected.sort_by(|a, b| a.path.cmp(&b.path));
    let mut maximal: Vec<SelectorTarget> = Vec::new();
    for target in selected {
        if !maximal
            .last()
            .is_some_and(|parent| target.path.starts_with(&parent.path))
        {
            maximal.push(target);
        }
    }
    if maximal.is_empty() || maximal[0].path.is_empty() {
        return None;
    }
    let mut spans = Vec::new();
    let mut end = 0usize;
    let mut sref_end = 0u32;
    for target in maximal {
        let SelectorNode::Action(state) = target.node else {
            return None;
        };
        let estimate = count_tree_from(config, false, state).ok()?;
        let next = end.checked_add(usize::try_from(estimate.f32_regret_bytes / 4).ok()?)?;
        let next_ref = sref_end.checked_add(u32::try_from(estimate.action_nodes).ok()?)?;
        spans.push(hu_engine::StorageSpan {
            start: end,
            end: next,
            sref_start: sref_end,
            sref_end: next_ref,
        });
        end = next;
        sref_end = next_ref;
    }
    Some(spans)
}

/// `parallel` counts the first chance level's deals concurrently; the
/// estimate is identical either way.
fn count_tree(config: &PostflopConfig, parallel: bool) -> Result<MemoryEstimate, TreeBuildError> {
    count_tree_from(config, parallel, LineState::root(config))
}

fn count_tree_from(
    config: &PostflopConfig,
    parallel: bool,
    start: LineState,
) -> Result<MemoryEstimate, TreeBuildError> {
    let mut counting = Counting {
        config,
        sym: game_preserving_perms(config),
        hands: PostflopHands::new(&config.board, &config.ranges),
        elements: 0,
        max_strategy_elements: 0,
        min_strategy_elements: u64::MAX,
        nodes: 0,
        terminals: 0,
        action_nodes: 0,
        deals: 0,
        depth: 0,
        max_depth: 0,
        max_deals: 0,
        mask_cards: 0,
        transition_keys: BTreeSet::new(),
        payoff_keys: BTreeSet::new(),
        rank_table_keys: BTreeSet::new(),
        hits: rule_hits_for(config),
        error: None,
        parallel,
    };
    counting.betting(start);

    if let Some(error) = counting.error {
        return Err(error);
    }
    let hands = (counting.hands.len(Player::P0) + counting.hands.len(Player::P1)) as u64;
    let widest = counting
        .hands
        .len(Player::P0)
        .max(counting.hands.len(Player::P1)) as u64;
    Ok(MemoryEstimate {
        f32_bytes: counting.elements * 2 * 4,
        i16_bytes: counting.elements * 2 * 2 + counting.action_nodes * 2 * 4,
        i16_f32avg_bytes: counting.elements * 6 + counting.action_nodes * 4,
        f32_regret_bytes: counting.elements * 4,
        i16_regret_bytes: counting.elements * 2 + counting.action_nodes * 4,
        save_bytes: counting.action_nodes * 2 * hands
            + counting.action_nodes
                * (std::mem::size_of::<parking_lot::Mutex<Option<(f32, Vec<u8>)>>>()
                    + std::mem::size_of::<bool>()) as u64
            + counting.nodes * std::mem::size_of::<Street>() as u64
            // Conservatively add the batch to the value pass's retained slots.
            + crate::sol::strategy_batch_workspace(
                counting.elements, counting.min_strategy_elements, counting.max_strategy_elements, counting.action_nodes,
            ),
        compression_bytes: crate::sol::compression_workspace(
            (counting.max_strategy_elements * 2).max(2 * hands),
            counting.elements * 2 + counting.action_nodes * 2 * hands,
            counting.action_nodes,
        ),
        game_bytes: counting.game_bytes(),
        thread_bytes: THREAD_FIXED_BYTES + THREAD_LEVEL_HAND_BYTES * widest * counting.max_depth,
        threads: 1,
        profile_bytes: 0,
        nodes: counting.nodes,
        action_nodes: counting.action_nodes,
        max_strategy_elements: counting.max_strategy_elements,
        max_depth: counting.max_depth,
        max_deals: counting.max_deals,
        terminals: counting.terminals,
        rank_tables: counting.rank_table_keys.len() as u64,
        rule_hits: counting.hits,
    })
}

/// Counting-only mirror of [`Builder`]'s recursion. Reuses the same
/// `node_actions`/`chance_groups` shape helpers as the real builder so the
/// two can never disagree about how many children a node has.
struct Counting<'a> {
    hands: PostflopHands,
    error: Option<TreeBuildError>,
    config: &'a PostflopConfig,
    sym: Vec<SuitPerm>,
    elements: u64,
    max_strategy_elements: u64,
    min_strategy_elements: u64,
    nodes: u64,
    terminals: u64,
    action_nodes: u64,
    deals: u64,
    /// Action and chance nodes above the node being counted, inclusive.
    depth: u64,
    max_depth: u64,
    max_deals: u64,
    /// Cards dealt as singleton classes; each has one card-removal mask per seat.
    mask_cards: u64,
    /// Merged deal classes by (board, members); each has one quotient
    /// transition per seat (see [`Builder::quotient_transition`]).
    transition_keys: BTreeSet<(Vec<Card>, Vec<Card>)>,
    /// Everything [`Builder::terminal`] bakes payoffs from; the builder
    /// interns the baked payoffs, so this bounds their count.
    payoff_keys: BTreeSet<(u8, Street, Chips, Chips)>,
    rank_table_keys: BTreeSet<[Card; 5]>,
    /// Accumulated by every [`node_actions`] call this dry run makes -- see
    /// [`RuleHits`].
    hits: RuleHits,
    /// Count the first chance level's deals in parallel, like [`Builder`].
    parallel: bool,
}

impl Counting<'_> {
    fn betting(&mut self, state: LineState) {
        if self.error.is_some() {
            return;
        }
        self.nodes += 1;
        self.action_nodes += 1;
        self.depth += 1;
        self.max_depth = self.max_depth.max(self.depth);

        let actions = node_actions(&state, self.config, &mut self.hits);
        if actions.is_empty() {
            self.error = Some(TreeBuildError::EmptyMenu {
                street: state.street,
                player: state.to_act,
            });
            return;
        }
        let num_actions = actions.len() as u64;
        for action in actions {
            match child_step(&state, action) {
                ChildStep::Fold => self.terminal(
                    &state,
                    TerminalKind::Fold {
                        folder: state.to_act,
                    },
                ),
                ChildStep::StreetEnd(next) => self.street_end(next),
                ChildStep::Betting(next) => self.betting(next),
            }
        }

        let elements = num_actions * self.hands.len(state.to_act) as u64;
        self.elements += elements;
        self.max_strategy_elements = self.max_strategy_elements.max(elements);
        self.min_strategy_elements = self.min_strategy_elements.min(elements);
        self.depth -= 1;
    }

    fn street_end(&mut self, state: LineState) {
        walk_street_end(self, state, "");
    }

    fn deal_chance(&mut self, state: LineState) {
        self.nodes += 1;
        self.depth += 1;
        self.max_depth = self.max_depth.max(self.depth);
        let groups = chance_groups(&state.board, self.config.iso_merging, &self.sym);
        self.deals += groups.len() as u64;
        self.max_deals = self.max_deals.max(groups.len() as u64);
        if self.parallel {
            use rayon::prelude::*;
            let forks: Vec<_> = groups
                .par_iter()
                .map(|group| {
                    let mut fork = self.fork();
                    fork.deal(&state, group);
                    fork
                })
                .collect();
            for fork in forks {
                self.absorb(fork);
            }
        } else {
            for group in &groups {
                self.deal(&state, group);
            }
        }
        self.depth -= 1;
    }

    fn deal(&mut self, state: &LineState, group: &DealGroup) {
        // The same reach-map choice as `Builder::deal`.
        if group.members.len() == 1 {
            self.mask_cards |= 1 << group.representative.index();
        } else {
            self.transition_keys
                .insert((state.board.to_vec(), group.members.clone()));
        }
        walk_dealt(self, state, group.representative, "");
    }

    fn fork(&self) -> Self {
        Counting {
            hands: self.hands.clone(),
            error: None,
            config: self.config,
            sym: self.sym.clone(),
            elements: 0,
            max_strategy_elements: 0,
            min_strategy_elements: u64::MAX,
            nodes: 0,
            terminals: 0,
            action_nodes: 0,
            deals: 0,
            depth: self.depth,
            max_depth: 0,
            max_deals: 0,
            mask_cards: 0,
            transition_keys: BTreeSet::new(),
            payoff_keys: BTreeSet::new(),
            rank_table_keys: BTreeSet::new(),
            hits: rule_hits_for(self.config),
            parallel: false,
        }
    }

    /// Adds a fork's counts. The serial walk stops at its first error, so
    /// the earliest fork's error wins and the counts no longer matter.
    fn absorb(&mut self, fork: Counting<'_>) {
        if self.error.is_none() {
            self.error = fork.error;
        }
        self.elements += fork.elements;
        self.max_strategy_elements = self.max_strategy_elements.max(fork.max_strategy_elements);
        self.min_strategy_elements = self.min_strategy_elements.min(fork.min_strategy_elements);
        self.nodes += fork.nodes;
        self.terminals += fork.terminals;
        self.action_nodes += fork.action_nodes;
        self.deals += fork.deals;
        self.max_depth = self.max_depth.max(fork.max_depth);
        self.max_deals = self.max_deals.max(fork.max_deals);
        self.mask_cards |= fork.mask_cards;
        self.transition_keys.extend(fork.transition_keys);
        self.payoff_keys.extend(fork.payoff_keys);
        self.rank_table_keys.extend(fork.rank_table_keys);
        crate::prepare::merge_rule_hits(&mut self.hits, &fork.hits);
    }

    fn terminal(&mut self, state: &LineState, kind: TerminalKind) {
        self.nodes += 1;
        self.terminals += 1;
        let kind_key = match kind {
            TerminalKind::Fold { folder: Player::P0 } => 0,
            TerminalKind::Fold { folder: Player::P1 } => 1,
            TerminalKind::Showdown => 2,
        };
        self.payoff_keys.insert((
            kind_key,
            state.street,
            state.contrib[Player::P0],
            state.contrib[Player::P1],
        ));
        if kind == TerminalKind::Showdown {
            let board5: [Card; 5] = state.board[..]
                .try_into()
                .expect("showdown requires a completed 5-card board");
            let mut key = board5;
            key.sort_unstable();
            self.rank_table_keys.insert(key);
        }
    }

    /// Heap bytes of the compiled game at the builder's exact capacities:
    /// every per-node, per-action-node, per-terminal and per-deal array of
    /// the tree, evaluator and viewer index, the masks, quotient transitions,
    /// interned payoffs and rank tables (at most one rank group per live
    /// hand), the hand maps and ranges, and an allowance for labels.
    fn game_bytes(&self) -> u64 {
        use hu_engine::{Deal, Node, StorageRef, StorageSpan};
        use std::mem::size_of;
        let seat = |p: Player| self.hands.len(p) as u64;
        let hands = seat(Player::P0) + seat(Player::P1);
        // Tree nodes, tags, spans and chance flags; index edges, streets and parents.
        let per_node = size_of::<Node>()
            + size_of::<u32>()
            + size_of::<StorageSpan>()
            + size_of::<bool>()
            + size_of::<u32>()
            + size_of::<Street>()
            + size_of::<NodeId>();
        // Storage refs and their node ids; index pots.
        let per_action = size_of::<StorageRef>() + size_of::<NodeId>() + size_of::<u32>();
        // Compact terminals and batch tables.
        let per_terminal = size_of::<CompactTerminal>() + size_of::<u32>();
        // Tree deals; index deal cards.
        let per_deal = size_of::<Deal>() + size_of::<u8>();
        // Per card: one mask per seat and both `mask_cards` entries.
        let masks = self.mask_cards.count_ones() as u64
            * (2 * (size_of::<Vec<f32>>() + size_of::<Card>()) as u64
                + hands * size_of::<f32>() as u64);
        let transitions = self
            .transition_keys
            .iter()
            .map(|(_, members)| {
                (2 * size_of::<SparseTransition>()) as u64
                    + members.len() as u64 * hands * size_of::<(u32, u32, f32)>() as u64
            })
            .sum::<u64>();
        let payoffs = self.payoff_keys.len() as u64 * size_of::<BakedPayoffs>() as u64;
        let rank_tables = self
            .rank_table_keys
            .iter()
            .map(|board| {
                let board: CardSet = board.iter().copied().collect();
                let ranked = |p: Player| {
                    let live = self
                        .hands
                        .combos(p)
                        .iter()
                        .filter(|&&global| {
                            let (a, b) = combo_cards(global as usize);
                            !board.contains(a) && !board.contains(b)
                        })
                        .count() as u64;
                    live * (size_of::<Hand>() + size_of::<(nlh::HandRank, usize)>()) as u64
                        + (seat(p) - live) * size_of::<u16>() as u64
                };
                size_of::<PerPlayer<RankedHands>>() as u64 + ranked(Player::P0) + ranked(Player::P1)
            })
            .sum::<u64>();
        // Support, same-combo and inverse maps in the evaluator and the index.
        let hand_maps = 2 * (2 * hands + 2 * NUM_COMBOS as u64) * size_of::<u16>() as u64;
        // Fold combos; root ranges in the game and the index.
        let per_hand = size_of::<Hand>() + 2 * size_of::<f32>();
        self.nodes * per_node as u64
            + self.action_nodes * per_action as u64
            + self.terminals * per_terminal as u64
            + self.deals * per_deal as u64
            + masks
            + transitions
            + payoffs
            + rank_tables
            + hand_maps
            + hands * per_hand as u64
            + LABEL_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_count_excludes_runouts_blocking_selected_hands() {
        use crate::game::{ChipEv, NoRake};
        use crate::index::{GameIndex, TreeView};

        let mut config = PostflopConfig {
            board: cards("As Ks Qs 2c"),
            ranges: PerPlayer::new("AhAd".parse().unwrap(), "JTs,99".parse().unwrap()),
            pot: Chips(6),
            effective_stack: Chips(9),
            ..Default::default()
        };
        let hands = spot::HandSelector::Range("AhAd".into());
        let profile = spot::ProfileIr {
            locks: vec![spot::LockIr {
                node: "xx[?]".into(),
                hands: hands.clone(),
                strategy: spot::LockStrategy::Uniform(vec![("check".into(), 1.0)]),
                confidence: 1.0,
            }],
            incentives: Vec::new(),
            frequencies: Vec::new(),
            freeze_upstream: None,
        };
        crate::profile::restrict_iso(&mut config, &profile).unwrap();
        let counted = selector_subtree_spans(&config, &profile).unwrap();
        let game = build_postflop_game(
            &config,
            PayoffPipeline {
                rake: &NoRake,
                utility: &ChipEv,
            },
        );
        let index = GameIndex::from_game(&game.game, &game.node_info, Street::Turn, config.scale);
        let tree = &game.game.tree;
        let view = TreeView {
            tree,
            index: &index,
        };
        let nodes = view.profile_nodes("xx[?]").unwrap();
        let mut actual = Vec::new();
        let mut blocked = 0;
        for id in nodes {
            if view
                .profile_hand_mask(id, &config.board, &hands)
                .unwrap()
                .iter()
                .any(|&selected| selected)
            {
                let span = tree.storage_spans[id as usize];
                actual.push((span.end - span.start, span.sref_end - span.sref_start));
            } else {
                blocked += 1;
            }
        }
        assert!(blocked > 0);
        let mut measured: Vec<_> = counted
            .iter()
            .map(|s| (s.end - s.start, s.sref_end - s.sref_start))
            .collect();
        actual.sort_unstable();
        measured.sort_unstable();
        assert_eq!(measured, actual);
    }

    #[test]
    fn selector_preflight_accepts_equivalent_history_amounts() {
        let input = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
bb_chips = 6
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 4h"
[ranges]
BB = "AK,44"
BTN = "AK,44"
[tree]
script = "turn { replace bet [2bb] } river { checkdown } turn when aggressions >= 1 { remove raise }"
[run]
memory = "1GiB"
"#;
        let p = crate::prepare::prepare(input, std::path::Path::new("test.toml")).unwrap();
        for (bb, chips) in [("r2", "r12c"), ("r2c[?]", "r12cc[?]")] {
            let paths = |selector| {
                selector_targets(&p.config, selector)
                    .unwrap()
                    .into_iter()
                    .map(|target| target.path)
                    .collect::<Vec<_>>()
            };
            assert_eq!(paths(bb), paths(chips));
        }
    }

    #[test]
    fn selector_count_matches_compiled_maximal_subtrees() {
        use crate::game::{ChipEv, NoRake};
        use crate::index::{GameIndex, TreeView};

        for selectors in [
            vec!["check/check/2d"],
            vec!["xx[?]"],
            vec!["x", "xx[?]", "check/check/?/check"],
            vec!["check/check/2d", "xx[?]xx[?]"],
        ] {
            let mut config = PostflopConfig {
                board: cards("As Ks Qs"),
                ranges: PerPlayer::new(
                    "AA,KQs,76s".parse().unwrap(),
                    "JTs,99,AKo".parse().unwrap(),
                ),
                pot: Chips(6),
                effective_stack: Chips(9),
                ..Default::default()
            };
            let profile = spot::ProfileIr {
                locks: selectors
                    .iter()
                    .map(|node| spot::LockIr {
                        node: (*node).into(),
                        hands: spot::HandSelector::All,
                        strategy: spot::LockStrategy::Uniform(vec![("check".into(), 1.0)]),
                        confidence: 1.0,
                    })
                    .collect(),
                incentives: Vec::new(),
                frequencies: Vec::new(),
                freeze_upstream: None,
            };
            crate::profile::restrict_iso(&mut config, &profile).unwrap();
            let counted = selector_subtree_spans(&config, &profile).unwrap();
            let game = build_postflop_game(
                &config,
                PayoffPipeline {
                    rake: &NoRake,
                    utility: &ChipEv,
                },
            );
            let index =
                GameIndex::from_game(&game.game, &game.node_info, Street::Flop, config.scale);
            let tree = &game.game.tree;
            let view = TreeView {
                tree,
                index: &index,
            };
            let targets: BTreeSet<_> = selectors
                .iter()
                .flat_map(|selector| view.profile_nodes(selector).unwrap())
                .collect();
            let mut actual = Vec::new();
            for &id in &targets {
                let mut parent = index.parents[id as usize];
                let mut nested = false;
                while parent != NodeId::MAX {
                    nested |= targets.contains(&parent);
                    parent = index.parents[parent as usize];
                }
                if !nested {
                    let span = tree.storage_spans[id as usize];
                    actual.push((span.end - span.start, span.sref_end - span.sref_start));
                }
            }
            let mut measured: Vec<_> = counted
                .iter()
                .map(|s| (s.end - s.start, s.sref_end - s.sref_start))
                .collect();
            actual.sort_unstable();
            measured.sort_unstable();
            assert_eq!(measured, actual, "{selectors:?}");
            assert_eq!(
                measured
                    .iter()
                    .fold((0usize, 0u32), |(e, r), &(a, b)| (e + a, r + b)),
                actual
                    .iter()
                    .fold((0usize, 0u32), |(e, r), &(a, b)| (e + a, r + b)),
                "{selectors:?}"
            );
        }
    }

    #[test]
    fn selector_bounds_cover_wildcards_and_saturate() {
        assert_eq!(selector_match_bound("x", 45, 1000), 1);
        assert_eq!(selector_match_bound("xx[?]", 45, 1000), 45);
        assert_eq!(
            selector_match_bound("check/check/?/check/check/?", 45, 1000),
            1000
        );
        assert_eq!(selector_match_bound("[?][?][?]", u64::MAX, 99), 99);
        assert_eq!(selector_match_bound("root", 0, 0), 0);
    }

    #[test]
    fn compact_peak_uses_selected_spans_for_each_backend() {
        let estimate = MemoryEstimate {
            f32_bytes: 8000,
            i16_bytes: 4800,
            i16_f32avg_bytes: 6400,
            f32_regret_bytes: 4000,
            i16_regret_bytes: 2400,
            ..Default::default()
        };
        let spans = [
            hu_engine::StorageSpan {
                start: 0,
                end: 100,
                sref_start: 0,
                sref_end: 2,
            },
            hu_engine::StorageSpan {
                start: 500,
                end: 600,
                sref_start: 10,
                sref_end: 13,
            },
        ];
        for (backend, bytes) in [
            (crate::input::Storage::F32, 1600),
            (crate::input::Storage::I16, 840),
            (crate::input::Storage::I16F32Avg, 1220),
        ] {
            assert_eq!(
                estimate.subtree_required_bytes(backend, &spans),
                estimate.resident_bytes() + estimate.checkpoint_bytes() + bytes,
            );
            assert!(
                estimate.subtree_required_bytes(backend, &spans) < estimate.required_bytes(backend)
            );
        }
    }

    #[test]
    fn freeze_budget_covers_fallback_frequency_and_auto_selection() {
        let config = PostflopConfig {
            board: cards("As Kd Qh"),
            ..Default::default()
        };
        let mut profile = spot::ProfileIr {
            locks: Vec::new(),
            incentives: Vec::new(),
            frequencies: Vec::new(),
            freeze_upstream: Some(spot::FreezeIr {
                solution: "base.sol".into(),
                sha256: None,
            }),
        };
        let mut estimate = MemoryEstimate {
            game_bytes: 1000,
            nodes: 100,
            action_nodes: 40,
            max_deals: 20,
            max_depth: 5,
            max_strategy_elements: 100,
            f32_bytes: 8000,
            i16_f32avg_bytes: 6000,
            f32_regret_bytes: 4000,
            i16_regret_bytes: 2000,
            ..Default::default()
        };
        let fallback = estimate.freeze_bytes(&config, &profile);
        profile.locks.push(spot::LockIr {
            node: "x".into(),
            hands: spot::HandSelector::All,
            strategy: spot::LockStrategy::Uniform(vec![("check".into(), 1.0)]),
            confidence: 1.0,
        });
        let locked = estimate.freeze_bytes(&config, &profile);
        assert!(locked >= fallback + 6 * 100 * 4);
        profile.locks.clear();
        profile.frequencies.push(spot::FrequencyIr {
            node: "xx[?]".into(),
            hands: spot::HandSelector::All,
            action: "check".into(),
            target: 0.5,
        });
        let frequency = estimate.freeze_bytes(&config, &profile);
        assert!(frequency > fallback);
        estimate.profile_bytes = frequency;
        let f32 = estimate.required_bytes(crate::input::Storage::F32);
        let mixed = estimate.required_bytes(crate::input::Storage::I16F32Avg);
        assert!(f32 > mixed);
        assert_eq!(
            crate::prepare::resolve_storage(crate::input::StorageChoice::Auto, &estimate, f32),
            crate::input::Storage::F32
        );
        assert_eq!(
            crate::prepare::resolve_storage(crate::input::StorageChoice::Auto, &estimate, f32 - 1),
            crate::input::Storage::I16F32Avg
        );
    }

    fn cards(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|card| card.parse().expect("test board card"))
            .collect()
    }

    /// Every member of a suit-isomorphism class must produce the same
    /// [`BoardFacts`], on every street.
    ///
    /// `deal_chance` pushes only the class *representative* onto the child
    /// board, so a board predicate that could tell two members of a class
    /// apart would be evaluated on the representative and then applied to
    /// the whole class. That builds a wrong tree with no panic and no
    /// failing assertion: `iso_merging` is an exact quotient only as long
    /// as nothing downstream can observe which member was dealt. Builder /
    /// Counting parity would not catch it either, since both walkers would
    /// be equally wrong.
    ///
    /// `BoardFacts::new` is proved suit-permutation invariant in
    /// `nlh::board`, and the classes here come from permutations that fix
    /// both the board and the ranges — so this holds by construction. It is
    /// pinned end to end anyway, through this crate's own private
    /// `chance_groups`, because it is the one property whose violation is
    /// silent.
    #[test]
    fn every_member_of_a_merged_class_has_the_representative_board_facts() {
        // Suit-symmetric ranges, so `range_preserving_perms` is the full
        // group and the board's own stabilizer decides what merges. A
        // suit-pinned range (`AhKh`) would shrink the stabilizer to the
        // identity and this sweep would merge nothing at all -- which the
        // `merged_classes` assertion below exists to catch.
        let ranges = PerPlayer::new(
            "AA,KQs,76s".parse::<Range>().expect("oop range"),
            "JTs,99,AKo".parse::<Range>().expect("ip range"),
        );
        let sym = range_preserving_perms(&ranges);

        let mut merged_classes = 0;
        for board_text in [
            // Monotone: the three unused suits are freely permutable, so
            // every off-suit turn card merges into one class.
            "As Ks Qs",
            // Two unused suits: the turn's `d` and `s` cards merge pairwise.
            "Th 9h 8h 2c",
            // Paired and two-tone, a different stabilizer again.
            "2c 2d 9d",
            // All four suits present: the stabilizer is trivial and every
            // class is a singleton. Included so the sweep also covers the
            // un-merged path.
            "2c 7d 9h Ks",
        ] {
            let board = cards(board_text);
            for group in chance_groups(&board, true, &sym) {
                if group.members.len() > 1 {
                    merged_classes += 1;
                }
                let facts_of = |card: Card| {
                    let mut extended = board.clone();
                    extended.push(card);
                    BoardFacts::new(&extended)
                };
                let representative = facts_of(group.representative);
                for &member in &group.members {
                    assert_eq!(
                        facts_of(member),
                        representative,
                        "board {board_text}: member {member} of the class dealt as \
                         {} disagrees about the board",
                        group.representative,
                    );
                }
            }
        }
        // Without this the sweep could pass by merging nothing at all.
        assert!(
            merged_classes > 0,
            "the sweep saw no merged class, so it proved nothing about merging"
        );
    }

    /// The parallel build forks a builder per first-level deal and re-interns
    /// every mask, transition, rank table, terminal and node tag. Any
    /// ordering slip would renumber storage refs, so saved `.sol` blocks
    /// would land on the wrong nodes; the serial walk is the reference.
    #[test]
    fn parallel_build_matches_serial_build() {
        use crate::game::{ChipEv, NoRake};
        // Every menu starts with all-in, so short stacks reach all-in runouts.
        let street = |street| StreetTree {
            nlh_rules: crate::test_support::street_rules(crate::test_support::allin_rules(street)),
        };
        let mut merged = false;
        for board_text in ["As Ks Qs", "2c 2d 9d", "Th 9h 8h 2c"] {
            let config = PostflopConfig {
                board: cards(board_text),
                // Suit-symmetric ranges so deals merge into transitions.
                ranges: PerPlayer::new(
                    "AA,KQs,76s,T9s".parse::<Range>().unwrap(),
                    "JTs,99,AKo,98s".parse::<Range>().unwrap(),
                ),
                pot: Chips(6),
                effective_stack: Chips(9),
                streets: PerStreet {
                    flop: street(Street::Flop),
                    turn: street(Street::Turn),
                    river: street(Street::River),
                },
                ..Default::default()
            };
            let build = |parallel| {
                let pipeline = PayoffPipeline {
                    rake: &NoRake,
                    utility: &ChipEv,
                };
                build_checked(&config, pipeline, parallel).unwrap()
            };
            assert_eq!(
                count_tree(&config, false).unwrap(),
                count_tree(&config, true).unwrap(),
                "{board_text}"
            );
            let (serial, parallel) = (build(false), build(true));
            let (a, b) = (&serial.game.tree, &parallel.game.tree);
            merged |= !a.transitions.is_empty();
            assert_eq!(
                format!("{:?}", a.nodes),
                format!("{:?}", b.nodes),
                "{board_text}"
            );
            assert_eq!(
                format!("{:?}", a.deals),
                format!("{:?}", b.deals),
                "{board_text}"
            );
            assert_eq!(a.tags, b.tags, "{board_text}");
            assert_eq!(a.masks, b.masks, "{board_text}");
            assert_eq!(
                format!("{:?}", a.transitions),
                format!("{:?}", b.transitions),
                "{board_text}"
            );
            assert_eq!(
                format!("{:?}", serial.node_info),
                format!("{:?}", parallel.node_info),
                "{board_text}"
            );
            assert_eq!(serial.rule_hits, parallel.rule_hits, "{board_text}");
            let (x, y) = (&serial.game.evaluator, &parallel.game.evaluator);
            assert_eq!(x.mask_cards, y.mask_cards, "{board_text}");
            assert_eq!(x.batch_tables, y.batch_tables, "{board_text}");
            assert_eq!(x.terminals.len(), y.terminals.len(), "{board_text}");
            for (s, p) in x.terminals.iter().zip(&y.terminals) {
                assert_eq!(
                    (
                        s.board_mask,
                        s.table,
                        format!("{:?}{:?}", s.kind, x.payoffs[s.payoff as usize])
                    ),
                    (
                        p.board_mask,
                        p.table,
                        format!("{:?}{:?}", p.kind, y.payoffs[p.payoff as usize])
                    ),
                    "{board_text}"
                );
            }
            assert_eq!(
                format!("{:?}", x.rank_tables),
                format!("{:?}", y.rank_tables),
                "{board_text}"
            );
        }
        assert!(
            merged,
            "no board merged a deal class, so transitions went untested"
        );
    }
}
