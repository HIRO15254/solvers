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

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Index;

use cards::{
    ALL_CARDS, Card, CardSet, Chips, HandRank, NUM_COMBOS, PerPlayer, Player, Range, Street,
    combo_cards, combo_index, rank_of,
};
use engine::{
    CompiledGame, NodeId, PublicTree, ReachMap, SparseTransition, TempNode, TerminalEvaluator,
    TreeSpec,
};
use game::{BakedPayoffs, PayoffPipeline, TerminalDescriptor, TerminalKind};
use hand_index::{
    Board, DealGroup, SuitPerm, all_suit_perms, deal_groups_with, orbit_perms, permute_combo,
    stabilizer,
};

use crate::kernel;

/// A value per postflop street. `Preflop` is out of scope for this crate
/// (postflop trees start no earlier than the flop) and indexing with it
/// panics.
#[derive(Clone, Debug, Default)]
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

/// A postflop subgame: starting board (3, 4, or 5 cards fixes the starting
/// street), both ranges, the pot already built, remaining effective stacks,
/// and a bet grammar per street.
#[derive(Clone)]
pub struct PostflopConfig {
    /// 3 (flop), 4 (turn), or 5 (river) distinct cards.
    pub board: Vec<Card>,
    pub ranges: PerPlayer<Range>,
    /// Pot at the start of the subgame; must be even (equal contributions).
    pub pot: Chips,
    /// Chips behind for each player, at the start of the subgame.
    pub effective_stack: Chips,
    /// Bet/raise sizes as fractions of the current pot, per street per
    /// player.
    pub bet_fractions: PerStreet<PerPlayer<Vec<f64>>>,
    /// Maximum number of bets+raises per street.
    pub max_raises: PerStreet<u32>,
    /// Merge turn/river deals into suit-isomorphism classes. The default
    /// constructor sets this `true`; river-only subgames (via the
    /// [`crate::river`] shim) never deal, so it has no effect there.
    pub iso_merging: bool,
    /// Record per-node history/action-label metadata for
    /// [`PostflopGame::node_by_history`]. Costs a `String` + `Vec<String>`
    /// per action node, so large flop trees may want this off.
    pub track_node_info: bool,
}

impl Default for PostflopConfig {
    fn default() -> Self {
        PostflopConfig {
            board: Vec::new(),
            ranges: PerPlayer::new(Range::default(), Range::default()),
            pot: Chips::ZERO,
            effective_stack: Chips::ZERO,
            bet_fractions: PerStreet::default(),
            max_raises: PerStreet::default(),
            iso_merging: true,
            track_node_info: true,
        }
    }
}

/// Node metadata mirroring the toy games' and river slice's scheme.
#[derive(Clone, Debug, Default)]
pub struct PostflopNodeInfo {
    pub history: String,
    pub actions: Vec<String>,
}

pub struct PostflopGame {
    pub game: CompiledGame<PostflopEvaluator>,
    pub node_info: Vec<PostflopNodeInfo>,
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
    kind: TerminalKind,
    payoffs: BakedPayoffs,
    /// Index into `PostflopEvaluator::rank_tables` for `Showdown`
    /// terminals; `u32::MAX` sentinel for `Fold` terminals, which use the
    /// evaluator's single `fold_combos` list instead (see its doc comment).
    table: u32,
}

/// Exact terminal evaluation over 1,326-combo reach vectors, generalized
/// from the river-only evaluator to runouts at any street.
pub struct PostflopEvaluator {
    terminals: Vec<PostflopTerminal>,
    /// Showdown rank tables, deduped by completed 5-card board (see
    /// [`Builder::rank_table_id`]).
    rank_tables: Vec<Vec<(HandRank, u32)>>,
    /// Live combos disjoint from the subgame's *starting* board, used by
    /// every fold terminal regardless of street or runout — see the
    /// invariant documented on [`kernel::fold_kernel`].
    fold_combos: Vec<(HandRank, u32)>,
}

impl TerminalEvaluator for PostflopEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        debug_assert_eq!(opp_reach.len(), NUM_COMBOS);
        debug_assert_eq!(out.len(), NUM_COMBOS);
        out.fill(0.0);
        let term = &self.terminals[terminal as usize];
        // Orient payoff constants to the traversing player: `u_win` is p's
        // utility when p's hand wins, etc.
        let (u_win, u_tie, u_lose) = match p {
            Player::P0 => (
                term.payoffs.win_p0[Player::P0],
                term.payoffs.tie[Player::P0],
                term.payoffs.win_p1[Player::P0],
            ),
            Player::P1 => (
                term.payoffs.win_p1[Player::P1],
                term.payoffs.tie[Player::P1],
                term.payoffs.win_p0[Player::P1],
            ),
        };
        match term.kind {
            TerminalKind::Fold { .. } => {
                kernel::fold_kernel(&self.fold_combos, u_win, opp_reach, out);
            }
            TerminalKind::Showdown => {
                kernel::showdown_kernel(
                    &self.rank_tables[term.table as usize],
                    u_win,
                    u_tie,
                    u_lose,
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
#[derive(Clone)]
struct LineState {
    street: Street,
    board: Vec<Card>,
    to_act: Player,
    /// Total chips committed this subgame (all streets), per player —
    /// *not* reset between streets, unlike `outstanding`/`raises_used`.
    contrib: PerPlayer<Chips>,
    outstanding: Chips,
    raises_used: u32,
    first_checked: bool,
    all_in: bool,
    history: String,
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

/// Distinct total-contribution amounts a bet/raise can reach for the acting
/// player at `state` (post pot-fraction sizing, all-in clamping, and
/// same-amount dedup). Shared between the real builder (which turns each
/// into a child node) and the memory-usage dry run (which only needs the
/// count), so the two can never produce a different action count for the
/// same config.
fn raise_targets(state: &LineState, config: &PostflopConfig) -> Vec<Chips> {
    let actor = state.to_act;
    if state.raises_used >= config.max_raises[state.street] {
        return Vec::new();
    }
    let pot_now = config.pot + state.contrib[Player::P0] + state.contrib[Player::P1];
    let behind = config.effective_stack - state.contrib[actor];
    if behind <= state.outstanding {
        return Vec::new();
    }
    let mut seen: Vec<Chips> = Vec::new();
    for &fraction in &config.bet_fractions[state.street][actor] {
        let pot_after_call = pot_now + state.outstanding;
        let raw = (fraction * pot_after_call.as_f64()).round() as u32;
        let extra = Chips(raw.max(1)).min(behind - state.outstanding);
        let additional = state.outstanding + extra;
        if !seen.contains(&additional) {
            seen.push(additional);
        }
    }
    seen
}

struct Builder<'a> {
    config: &'a PostflopConfig,
    pipeline: PayoffPipeline<'a>,
    /// Suit permutations preserving both ranges (see
    /// [`range_preserving_perms`]).
    sym: Vec<SuitPerm>,
    terminals: Vec<PostflopTerminal>,
    rank_tables: Vec<Vec<(HandRank, u32)>>,
    rank_table_ids: BTreeMap<[Card; 5], u32>,
    masks: Vec<Vec<f32>>,
    /// Per-card reach mask, built lazily and cached by card index (at most
    /// 52 masks total regardless of how many chance nodes share a card).
    card_masks: [Option<u32>; 52],
    /// Quotient transitions for merged deal classes, interned by
    /// (board, members).
    transitions: Vec<SparseTransition>,
    transition_ids: BTreeMap<(Vec<Card>, Vec<Card>), u32>,
    node_info: Vec<PostflopNodeInfo>,
}

/// Builds a postflop subgame through the payoff pipeline.
pub fn build_postflop_game(config: &PostflopConfig, pipeline: PayoffPipeline<'_>) -> PostflopGame {
    assert!(config.pot.0.is_multiple_of(2), "pot must be even");
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
        config,
        pipeline,
        sym: range_preserving_perms(&config.ranges),
        terminals: Vec::new(),
        rank_tables: Vec::new(),
        rank_table_ids: BTreeMap::new(),
        masks: Vec::new(),
        card_masks: [None; 52],
        transitions: Vec::new(),
        transition_ids: BTreeMap::new(),
        node_info: vec![PostflopNodeInfo {
            history: "<untagged>".into(),
            actions: Vec::new(),
        }],
    };
    let root = builder.betting(LineState {
        street: start_street,
        board: config.board.clone(),
        to_act: Player::P0,
        contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
        outstanding: Chips::ZERO,
        raises_used: 0,
        first_checked: false,
        all_in: false,
        history: String::new(),
    });

    // Root ranges as 1,326-weight vectors with board conflicts zeroed.
    let range_vec = |p: Player| -> Vec<f32> {
        (0..NUM_COMBOS)
            .map(|combo| {
                let (c1, c2) = combo_cards(combo);
                if board_set.contains(c1) || board_set.contains(c2) {
                    0.0
                } else {
                    config.ranges[p].weight(combo)
                }
            })
            .collect()
    };
    let ranges = PerPlayer::new(range_vec(Player::P0), range_vec(Player::P1));

    // Live combos disjoint from the *starting* board — the universal fold
    // list every fold terminal in the tree shares (see
    // `PostflopEvaluator::fold_combos`).
    let fold_combos: Vec<(HandRank, u32)> = (0..NUM_COMBOS)
        .filter_map(|combo| {
            let (c1, c2) = combo_cards(combo);
            if board_set.contains(c1) || board_set.contains(c2) {
                None
            } else {
                Some((HandRank(0), combo as u32))
            }
        })
        .collect();

    let Builder {
        terminals,
        rank_tables,
        masks,
        transitions,
        node_info,
        ..
    } = builder;

    let evaluator = PostflopEvaluator {
        terminals,
        rank_tables,
        fold_combos,
    };

    // Joint compatible weight, via the same inclusion-exclusion the fold
    // kernel uses. Only the pair-compat sum — no live-count denominators
    // (the 1/45, 1/44 deal weights already live on the chance branches).
    let (all_total, all_card) = kernel::compat_sums(&evaluator.fold_combos, &ranges[Player::P1]);
    let normalizer: f64 = evaluator
        .fold_combos
        .iter()
        .map(|&(_, combo)| {
            let idx = combo as usize;
            let (c1, c2) = combo_cards(idx);
            ranges[Player::P0][idx] as f64
                * (all_total - all_card[c1.index()] - all_card[c2.index()]
                    + ranges[Player::P1][idx] as f64)
        })
        .sum();
    assert!(normalizer > 0.0, "ranges share no compatible combos");

    let tree = PublicTree::compile(TreeSpec {
        root,
        masks,
        transitions,
        root_dims: PerPlayer::new(NUM_COMBOS as u32, NUM_COMBOS as u32),
    });

    PostflopGame {
        game: CompiledGame {
            tree,
            evaluator,
            root_ranges: ranges,
            normalizer,
            zero_sum,
        },
        node_info,
    }
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

    fn betting(&mut self, state: LineState) -> TempNode {
        let actor = state.to_act;
        let mut actions: Vec<(String, TempNode)> = Vec::new();

        if state.outstanding == Chips::ZERO {
            if state.first_checked {
                let next = LineState {
                    history: self.extend_history(&state.history, || "x".into()),
                    ..state.clone()
                };
                actions.push((self.action_label(|| "check".into()), self.street_end(next)));
            } else {
                let next = LineState {
                    to_act: actor.opponent(),
                    first_checked: true,
                    history: self.extend_history(&state.history, || "x".into()),
                    ..state.clone()
                };
                actions.push((self.action_label(|| "check".into()), self.betting(next)));
            }
        } else {
            let fold_state = LineState {
                history: self.extend_history(&state.history, || "f".into()),
                ..state.clone()
            };
            actions.push((
                self.action_label(|| "fold".into()),
                self.terminal(&fold_state, TerminalKind::Fold { folder: actor }),
            ));
            let mut contrib = state.contrib;
            contrib[actor] += state.outstanding;
            let call_state = LineState {
                contrib,
                outstanding: Chips::ZERO,
                history: self.extend_history(&state.history, || "c".into()),
                ..state.clone()
            };
            actions.push((
                self.action_label(|| "call".into()),
                self.street_end(call_state),
            ));
        }

        // Bets and raises share sizing logic: `f * pot after call`.
        for additional in raise_targets(&state, self.config) {
            let mut contrib = state.contrib;
            contrib[actor] += additional;
            let to = contrib[actor];
            let verb = self.action_label(|| {
                if state.outstanding == Chips::ZERO {
                    format!("bet {to}")
                } else {
                    format!("raise to {to}")
                }
            });
            let next = LineState {
                to_act: actor.opponent(),
                contrib,
                outstanding: additional - state.outstanding,
                raises_used: state.raises_used + 1,
                history: self.extend_history(&state.history, || format!("b{to}")),
                ..state.clone()
            };
            actions.push((verb, self.betting(next)));
        }

        self.finish_action_node(actor, state.history, actions)
    }

    fn finish_action_node(
        &mut self,
        actor: Player,
        history: String,
        actions: Vec<(String, TempNode)>,
    ) -> TempNode {
        let tag = if self.config.track_node_info {
            let tag = self.node_info.len() as u32;
            self.node_info.push(PostflopNodeInfo {
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

    /// After a call or check-check: showdown at the river, otherwise deal
    /// the next street's chance node. Either way, first latch `all_in` if
    /// either player has committed their whole starting stack.
    fn street_end(&mut self, mut state: LineState) -> TempNode {
        if state.contrib[Player::P0] == self.config.effective_stack
            || state.contrib[Player::P1] == self.config.effective_stack
        {
            state.all_in = true;
        }
        if state.street == Street::River {
            self.terminal(&state, TerminalKind::Showdown)
        } else {
            self.deal_chance(state)
        }
    }

    fn deal_chance(&mut self, state: LineState) -> TempNode {
        let next = next_street(state.street);
        let denom = deal_denom(state.street);
        let groups = chance_groups(&state.board, self.config.iso_merging, &self.sym);

        let mut deals: Vec<(f32, PerPlayer<ReachMap>, TempNode)> = Vec::with_capacity(groups.len());
        for group in &groups {
            let weight = group.members.len() as f32 / denom;
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
                PerPlayer::new(ReachMap::Mask(mask_id), ReachMap::Mask(mask_id))
            } else {
                let id = self.quotient_transition(&state.board, group);
                PerPlayer::new(ReachMap::Transition(id), ReachMap::Transition(id))
            };

            let mut board = state.board.clone();
            board.push(group.representative);
            let history =
                self.extend_history(&state.history, || format!("[{}]", group.representative));

            let child_state = LineState {
                street: next,
                board,
                to_act: Player::P0,
                contrib: state.contrib,
                outstanding: Chips::ZERO,
                raises_used: 0,
                first_checked: false,
                all_in: state.all_in,
                history,
            };

            let child = if state.all_in {
                self.after_deal(child_state)
            } else {
                self.betting(child_state)
            };
            deals.push((weight, maps, child));
        }

        TempNode::Chance { deals, tag: 0 }
    }

    /// After a chance deal on an all-in line: no more betting, just the
    /// next chance node, or the showdown once the river is reached.
    fn after_deal(&mut self, state: LineState) -> TempNode {
        if state.street == Street::River {
            self.terminal(&state, TerminalKind::Showdown)
        } else {
            self.deal_chance(state)
        }
    }

    /// Quotient transition for a merged deal class (see the comment at the
    /// use site in [`Self::deal_chance`]). Interned by (board, members):
    /// identical classes recur across betting lines of the same street.
    fn quotient_transition(&mut self, board: &[Card], group: &DealGroup) -> u32 {
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
        let mut entries: Vec<(u32, u32, f32)> = Vec::with_capacity(perms.len() * (NUM_COMBOS - 51));
        for perm in &perms {
            for h in 0..NUM_COMBOS {
                let (c1, c2) = combo_cards(h);
                if c1 == rep || c2 == rep {
                    continue; // dead in rep coordinates
                }
                entries.push((permute_combo(perm, h) as u32, h as u32, member_avg));
            }
        }
        let id = self.transitions.len() as u32;
        self.transitions.push(SparseTransition {
            in_dim: NUM_COMBOS as u32,
            out_dim: NUM_COMBOS as u32,
            entries,
        });
        self.transition_ids.insert(key, id);
        id
    }

    fn card_mask(&mut self, card: Card) -> u32 {
        if let Some(id) = self.card_masks[card.index()] {
            return id;
        }
        let id = self.masks.len() as u32;
        let mask: Vec<f32> = (0..NUM_COMBOS)
            .map(|combo| {
                let (c1, c2) = combo_cards(combo);
                if c1 == card || c2 == card { 0.0 } else { 1.0 }
            })
            .collect();
        self.masks.push(mask);
        self.card_masks[card.index()] = Some(id);
        id
    }

    fn terminal(&mut self, state: &LineState, kind: TerminalKind) -> TempNode {
        let half_pot = Chips(self.config.pot.0 / 2);
        let contrib = PerPlayer::new(
            half_pot + state.contrib[Player::P0],
            half_pot + state.contrib[Player::P1],
        );
        let descriptor = TerminalDescriptor {
            kind,
            street: state.street,
            pot: contrib[Player::P0] + contrib[Player::P1],
            contrib,
            stacks_before: PerPlayer::new(
                self.config.effective_stack + half_pot,
                self.config.effective_stack + half_pot,
            ),
        };
        let payoffs = self.pipeline.bake(&descriptor);
        let table = match kind {
            TerminalKind::Fold { .. } => u32::MAX,
            TerminalKind::Showdown => self.rank_table_id(&state.board),
        };
        let id = self.terminals.len() as u32;
        self.terminals.push(PostflopTerminal {
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
        let board_set: CardSet = board5.iter().copied().collect();
        let mut sorted: Vec<(HandRank, u32)> = Vec::new();
        for combo in 0..NUM_COMBOS {
            let (c1, c2) = combo_cards(combo);
            if board_set.contains(c1) || board_set.contains(c2) {
                continue;
            }
            let rank = rank_of(board5.iter().copied().chain([c1, c2]));
            sorted.push((rank, combo as u32));
        }
        sorted.sort_unstable();
        let id = self.rank_tables.len() as u32;
        self.rank_tables.push(sorted);
        self.rank_table_ids.insert(key, id);
        id
    }
}

/// Storage/node/terminal/rank-table footprint of a [`PostflopConfig`],
/// without actually building the tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryEstimate {
    /// Bytes for an `F32Storage` backend (two `f32` arenas).
    pub f32_bytes: u64,
    /// Bytes for a hypothetical quantized `i16` backend: two `i16` arenas
    /// plus a per-action-node `f32` scale pair (regrets, strategy sum).
    pub i16_bytes: u64,
    pub nodes: u64,
    pub terminals: u64,
    pub rank_tables: u64,
}

/// Dry-run of [`build_postflop_game`]'s recursion that counts storage
/// elements, nodes, terminals, and distinct showdown boards without
/// constructing any `TempNode`s, chance masks, or rank tables — cheap
/// enough to run as a preflight check before committing to a full build of
/// a large flop tree.
pub fn memory_usage(config: &PostflopConfig) -> MemoryEstimate {
    let board_len = config.board.len();
    let start_street = match board_len {
        3 => Street::Flop,
        4 => Street::Turn,
        5 => Street::River,
        _ => panic!("board must have 3 (flop), 4 (turn), or 5 (river) cards"),
    };

    let mut counting = Counting {
        config,
        sym: range_preserving_perms(&config.ranges),
        elements: 0,
        nodes: 0,
        terminals: 0,
        action_nodes: 0,
        rank_table_keys: BTreeSet::new(),
    };
    counting.betting(LineState {
        street: start_street,
        board: config.board.clone(),
        to_act: Player::P0,
        contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
        outstanding: Chips::ZERO,
        raises_used: 0,
        first_checked: false,
        all_in: false,
        history: String::new(),
    });

    MemoryEstimate {
        f32_bytes: counting.elements * 2 * 4,
        i16_bytes: counting.elements * 2 * 2 + counting.action_nodes * 2 * 4,
        nodes: counting.nodes,
        terminals: counting.terminals,
        rank_tables: counting.rank_table_keys.len() as u64,
    }
}

/// Counting-only mirror of [`Builder`]'s recursion. Reuses the same
/// `raise_targets`/`chance_groups` shape helpers as the real builder so the
/// two can never disagree about how many children a node has.
struct Counting<'a> {
    config: &'a PostflopConfig,
    sym: Vec<SuitPerm>,
    elements: u64,
    nodes: u64,
    terminals: u64,
    action_nodes: u64,
    rank_table_keys: BTreeSet<[Card; 5]>,
}

impl Counting<'_> {
    fn betting(&mut self, state: LineState) {
        self.nodes += 1;
        self.action_nodes += 1;
        let mut num_actions: u64 = 0;

        if state.outstanding == Chips::ZERO {
            num_actions += 1;
            if state.first_checked {
                self.street_end(state.clone());
            } else {
                let next = LineState {
                    to_act: state.to_act.opponent(),
                    first_checked: true,
                    ..state.clone()
                };
                self.betting(next);
            }
        } else {
            num_actions += 2;
            self.terminal_fold();
            let mut contrib = state.contrib;
            contrib[state.to_act] += state.outstanding;
            let call_state = LineState {
                contrib,
                outstanding: Chips::ZERO,
                ..state.clone()
            };
            self.street_end(call_state);
        }

        let raises = raise_targets(&state, self.config);
        num_actions += raises.len() as u64;
        for additional in raises {
            let actor = state.to_act;
            let mut contrib = state.contrib;
            contrib[actor] += additional;
            let next = LineState {
                to_act: actor.opponent(),
                contrib,
                outstanding: additional - state.outstanding,
                raises_used: state.raises_used + 1,
                ..state.clone()
            };
            self.betting(next);
        }

        self.elements += num_actions * NUM_COMBOS as u64;
    }

    fn street_end(&mut self, mut state: LineState) {
        if state.contrib[Player::P0] == self.config.effective_stack
            || state.contrib[Player::P1] == self.config.effective_stack
        {
            state.all_in = true;
        }
        if state.street == Street::River {
            self.terminal_showdown(&state.board);
        } else {
            self.deal_chance(state);
        }
    }

    fn deal_chance(&mut self, state: LineState) {
        self.nodes += 1;
        let next = next_street(state.street);
        let groups = chance_groups(&state.board, self.config.iso_merging, &self.sym);
        for group in &groups {
            let mut board = state.board.clone();
            board.push(group.representative);
            let child_state = LineState {
                street: next,
                board,
                to_act: Player::P0,
                contrib: state.contrib,
                outstanding: Chips::ZERO,
                raises_used: 0,
                first_checked: false,
                all_in: state.all_in,
                history: String::new(),
            };
            if state.all_in {
                self.after_deal(child_state);
            } else {
                self.betting(child_state);
            }
        }
    }

    fn after_deal(&mut self, state: LineState) {
        if state.street == Street::River {
            self.terminal_showdown(&state.board);
        } else {
            self.deal_chance(state);
        }
    }

    fn terminal_fold(&mut self) {
        self.nodes += 1;
        self.terminals += 1;
    }

    fn terminal_showdown(&mut self, board: &[Card]) {
        self.nodes += 1;
        self.terminals += 1;
        let board5: [Card; 5] = board
            .try_into()
            .expect("showdown requires a completed 5-card board");
        let mut key = board5;
        key.sort_unstable();
        self.rank_table_keys.insert(key);
    }
}
