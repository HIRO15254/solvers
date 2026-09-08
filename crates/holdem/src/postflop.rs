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
use std::ops::{Index, IndexMut};

use cards::script::{
    ActionKind, CmpOp, Condition, Effect, Literal, PostflopVar, PreviousAggressor, Rule,
    RuleContext,
};
use cards::{
    ALL_CARDS, BoardFacts, Card, CardSet, Chips, HandRank, NUM_COMBOS, PerPlayer, Player, Range,
    SizeSpec, Street, combo_cards, combo_index, geometric_allin_target, rank_of,
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

/// One street's betting grammar: the tree-script rules that get replayed at
/// every decision node on this street (see [`node_actions`]), plus the
/// structural limits a script cannot express (`docs/solver-config-v1.jp.md`'s
/// 適用モデル chapter: `max_aggressive_actions` is a memory-preflight input,
/// and `allin_threshold` is a size-resolution rule, not an action-list
/// add/remove). `rules` used to come only from the five fixed-menu fields
/// (`oop_bet`/`ip_bet`/`oop_raise`/`ip_raise`/`oop_donk`) a pre-script
/// grammar had; [`StreetTree::from_script`] is what a compiled tree script
/// (`cards::script::Script`) builds this from now, and
/// [`StreetTree::pot_fractions`] is the short constructor test/bench
/// fixtures use for the common single-level pot-fraction case.
#[derive(Clone, Debug)]
pub struct StreetTree {
    /// This street's tree-script rules, in source order. Source order is
    /// the entire priority model -- there is no priority field.
    pub rules: Vec<Rule<PostflopVar>>,
    /// Maximum bets plus raises on this street (multiway's name for what
    /// this used to call `max_raises`).
    pub max_aggressive_actions: u32,
    /// Always offer the all-in target in addition to whatever the rules
    /// produce. Applied before any rule runs (see [`node_actions`]).
    pub include_allin: bool,
    /// Targets at or above this fraction of the actor's maximum target
    /// collapse into the all-in target. Finite and in `(0.0, 1.0]`.
    pub allin_threshold: Option<f64>,
}

impl Default for StreetTree {
    fn default() -> Self {
        StreetTree {
            rules: Vec::new(),
            max_aggressive_actions: 2,
            include_allin: false,
            allin_threshold: None,
        }
    }
}

/// AND-combines two conditions without ever nesting `Const(true)`, matching
/// `cards::script::cond`'s own simplification convention -- not load-bearing
/// here (the translated rules never actually combine two `Const`s), just
/// keeping every hand-built `Condition` in the same normal form the script
/// compiler would produce.
fn and(left: Condition<PostflopVar>, right: Condition<PostflopVar>) -> Condition<PostflopVar> {
    Condition::And(Box::new(left), Box::new(right))
}

fn not(inner: Condition<PostflopVar>) -> Condition<PostflopVar> {
    Condition::Not(Box::new(inner))
}

/// One raise menu's levels as `Add Raise` rules gated on `aggressions`: level
/// `i` (`0`-based) applies when `i < levels.len() - 1` and
/// `aggressions == i + 1`, and the last level applies when
/// `aggressions >= levels.len()` -- reproducing
/// `raise_targets`'s old `level = (raises_used - 1).min(len - 1)` selection.
/// An empty `levels` yields no rules at all ("never raise"). Used by
/// [`StreetTree::pot_fractions`], the short constructor for the common
/// single-level case.
fn raise_level_rules(
    street: Street,
    actor: Condition<PostflopVar>,
    levels: &[Vec<SizeSpec>],
) -> Vec<Rule<PostflopVar>> {
    let len = levels.len();
    levels
        .iter()
        .enumerate()
        .map(|(i, sizes)| {
            let aggression = if i + 1 < len {
                Condition::Compare {
                    var: PostflopVar::Aggressions,
                    op: CmpOp::Eq,
                    value: Literal::Number((i + 1) as f64),
                }
            } else {
                Condition::Compare {
                    var: PostflopVar::Aggressions,
                    op: CmpOp::Ge,
                    value: Literal::Number(len as f64),
                }
            };
            Rule {
                street,
                condition: and(actor.clone(), aggression),
                effect: Effect::Add,
                action: Some(ActionKind::Raise),
                sizes: sizes.clone(),
            }
        })
        .collect()
}

impl StreetTree {
    /// A street whose bet and raise menus are the same pot fractions for
    /// both players — the shape every config had before size literals, kept
    /// as a short constructor for the test/bench fixtures that just want a
    /// classic single-level pot-fraction tree. `oop`/`ip` become the opening
    /// bet menus, and the raise menus reuse them as a single level (an
    /// unraised street's `oop_raise`/`ip_raise` used to fall back to the
    /// player's own bet menu; this constructor reproduces exactly that).
    ///
    /// Has no `street` parameter (63 call sites across tests, benches, and
    /// `src/river.rs` depend on this exact signature), so the rules built
    /// below are tagged with an arbitrary placeholder street (`Street::Flop`).
    /// That's sound: `StreetTree`'s only consumer (`node_actions`) selects
    /// which street's rules to run structurally, by indexing
    /// `PerStreet<StreetTree>` with the node's own street -- it never reads
    /// `Rule::street` back out of the rules it runs.
    pub fn pot_fractions(oop: &[f64], ip: &[f64], max_aggressive_actions: u32) -> StreetTree {
        let sizes = |fractions: &[f64]| -> Vec<SizeSpec> {
            fractions
                .iter()
                .map(|&fraction| SizeSpec::PotAfterCall { fraction })
                .collect()
        };
        let street = Street::Flop;
        let not_in_position = not(Condition::Truth(PostflopVar::InPosition));
        let in_position = Condition::Truth(PostflopVar::InPosition);
        let oop_sizes = sizes(oop);
        let ip_sizes = sizes(ip);

        let mut rules = vec![
            Rule {
                street,
                condition: not_in_position.clone(),
                effect: Effect::Add,
                action: Some(ActionKind::Bet),
                sizes: oop_sizes.clone(),
            },
            Rule {
                street,
                condition: in_position.clone(),
                effect: Effect::Add,
                action: Some(ActionKind::Bet),
                sizes: ip_sizes.clone(),
            },
        ];
        rules.extend(raise_level_rules(
            street,
            not_in_position,
            std::slice::from_ref(&oop_sizes),
        ));
        rules.extend(raise_level_rules(
            street,
            in_position,
            std::slice::from_ref(&ip_sizes),
        ));

        StreetTree {
            rules,
            max_aggressive_actions,
            include_allin: false,
            allin_threshold: None,
        }
    }

    /// Collects one street's rules out of a compiled tree script's flat rule
    /// list (`cards::script::Script::rules`), preserving source order --
    /// source order is the script's entire priority model (see
    /// `docs/solver-config-v1.jp.md`'s script の構造 chapter), so this
    /// must not reorder or resort what it filters. `rules` is the *whole*
    /// script's rule list (every street's statements interleaved in source
    /// order); this keeps only the ones tagged for `street`, exactly as
    /// `node_actions` expects to run them.
    pub fn from_script(
        street: Street,
        rules: &[Rule<PostflopVar>],
        max_aggressive_actions: u32,
        include_allin: bool,
        allin_threshold: Option<f64>,
    ) -> StreetTree {
        StreetTree {
            rules: rules
                .iter()
                .filter(|rule| rule.street == street)
                .cloned()
                .collect(),
            max_aggressive_actions,
            include_allin,
            allin_threshold,
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
    /// node's action menu, plus the structural limits a script cannot
    /// override (the aggressive-action cap and all-in handling).
    pub streets: PerStreet<StreetTree>,
    /// Smallest legal opening bet and smallest legal raise increment — the
    /// big blind's role in a game that has no blinds. `SizeSpec::MinRaise`
    /// and the minimum-full-raise bump (see `sized_targets`) are both
    /// anchored on this value.
    pub min_bet: Chips,
    /// Merge turn/river deals into suit-isomorphism classes. The default
    /// constructor sets this `true`; river-only subgames (via the
    /// [`crate::river`] shim) never deal, so it has no effect there.
    pub iso_merging: bool,
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
            iso_merging: true,
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
    history: String,
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
    /// see `docs/solver-config-v1.jp.md`'s 文 — action list の書き換え chapter.
    previous_aggressor: Option<Player>,
    /// The last player to bet or raise on THIS street so far; `None` until
    /// someone does. Carried forward into the next street's
    /// `previous_aggressor` by `street_end`/`deal_chance`.
    street_aggressor: Option<Player>,
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

/// `(amount * fraction).round()`, clamped into `Chips`' `u32` range. Matches
/// `multiway::betting::scale`'s rounding convention (same size-literal
/// grammar, chip unit instead of bb).
fn scale(amount: Chips, fraction: f64) -> Chips {
    let scaled = (amount.as_f64() * fraction).round();
    Chips(scaled.clamp(0.0, u32::MAX as f64) as u32)
}

/// Resolves an explicit size menu at `state` into additional-contribution
/// wagers: every entry of `sizes` resolved to a raise-to target
/// (street-relative, i.e. the actor's total contribution *this street*),
/// bumped up to the minimum full raise, clamped to all-in, merged into
/// all-in past `allin_threshold`, sorted, deduped, and filtered down to
/// targets that actually raise the bet faced — the resolution order fixed by
/// `docs/solver-config-v1.jp.md`'s `[game.tree]` size-literal table
/// (identical to `multiway::betting::BettingState::legal_actions`'s size
/// resolution, `ToChips`/chip-unit literals instead of `ToBb`/bb). Returned
/// as *additional* contribution over the actor's current street wager — the
/// shape both `Builder::betting` and the counting mirror `Counting::betting`
/// consume — so the two can never disagree about a node's action count.
///
/// This is everything the old (pre-tree-script) `raise_targets` did *except*
/// choosing which menu to resolve and appending the `include_allin` target —
/// both now the caller's job (`node_actions`), since which menu applies is a
/// tree-script rule's concern, not this function's. The two early returns
/// below stay here rather than moving to the caller: they are structural
/// limits (`max_aggressive_actions`, "no chips behind") that no script rule
/// is allowed to override by naming its own sizes.
fn sized_targets(state: &LineState, config: &PostflopConfig, sizes: &[SizeSpec]) -> Vec<Chips> {
    let street = &config.streets[state.street];
    if state.raises_used >= street.max_aggressive_actions {
        return Vec::new();
    }
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
        return Vec::new();
    }

    let to_call = state.outstanding.min(behind);
    let pot_now = config.pot + state.contrib[Player::P0] + state.contrib[Player::P1];
    let pot_after_call = pot_now + to_call;
    let called_to = actor_wager + to_call;

    // `multiway::betting::BettingState::minimum_full_target`, with
    // `min_bet` playing the big blind's role: the first bet of a street must
    // reach `min_bet`, and every later raise must reach the previous full
    // raise's own increment on top of the bet it faces.
    let minimum = if state.last_full_raise > Chips::ZERO {
        (bet_to_match + state.last_full_raise).min(maximum)
    } else {
        (bet_to_match + config.min_bet).min(maximum)
    };

    let mut targets: Vec<Chips> = Vec::with_capacity(sizes.len());
    for &size in sizes {
        let mut target = match size {
            SizeSpec::ToBb { .. } => unreachable!(
                "the bb size literal belongs to the Multiway Preflop family; \
                 postflop configs are chip-denominated and never produce ToBb"
            ),
            SizeSpec::PotAfterCall { fraction } => called_to + scale(pot_after_call, fraction),
            SizeSpec::PreviousBetMultiple { factor } => scale(bet_to_match, factor),
            SizeSpec::MinRaise => minimum,
            SizeSpec::AllIn => maximum,
            // Postflop has a single shared `effective_stack`, so the
            // effective stack IS the actor's maximum target — the two
            // variants resolve identically here, unlike multiway's
            // per-opponent `EffectiveStackFraction`.
            SizeSpec::StackFraction { fraction }
            | SizeSpec::EffectiveStackFraction { fraction } => scale(maximum, fraction),
            // Pio's bare `e` splits the stack over the streets that are
            // left, so a flop `e` is three bets and a river `e` is one.
            SizeSpec::GeometricAllIn { streets: _ } | SizeSpec::GeometricAllInRemaining => {
                let streets = match size {
                    SizeSpec::GeometricAllIn { streets } => streets,
                    _ => streets_remaining(state.street),
                };
                Chips(geometric_allin_target(
                    called_to.0 as u64,
                    pot_after_call.0 as u64,
                    maximum.0 as u64,
                    streets,
                ) as u32)
            }
            SizeSpec::ToChips { value } => Chips(value.round().clamp(0.0, u32::MAX as f64) as u32),
        };
        if target < minimum && maximum >= minimum {
            target = minimum;
        }
        target = target.min(maximum);
        if let Some(threshold) = street.allin_threshold
            && target >= scale(maximum, threshold)
        {
            target = maximum;
        }
        targets.push(target);
    }
    targets.sort_unstable();
    targets.dedup();
    targets.retain(|&target| target > bet_to_match);
    targets
        .into_iter()
        .map(|target| target - actor_wager)
        .collect()
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
/// Also the fallback `node_actions` returns when rules empty the list out
/// entirely (see that function) — the only sound reading of `force` /
/// `checkdown` / `remove` clearing every candidate, and what keeps
/// `engine::tree`'s `assert!(num_actions >= 1)` satisfied.
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

/// This node's `RuleContext`, filled in from `state`/`config` per
/// `docs/solver-config-v1.jp.md`'s condition variable table. `actor` is
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
/// `StreetTree::rules` (i.e. `StreetTree::from_script`'s per-street filter
/// order), not by position in the whole script's flat rule list. This is
/// the empirical answer to "did any node satisfy this rule?" --
/// deliberately *not* "did the rule change the action list", since a rule
/// whose condition is true but whose `action` names the wager kind the node
/// doesn't have (see `node_actions`'s inert-rule branch) still had its
/// guard checked and is not what a script author needs warned about; only
/// a condition that is never true anywhere is the silent-authoring mistake
/// `docs/solver-config-v1.jp.md`'s `[game.tree]` chapter's dead-rule
/// warning exists for.
///
/// A `Vec<bool>` per street, not a `HashMap`: `node_actions` runs at every
/// decision node of trees with hundreds of thousands of nodes, so recording
/// a hit must be a plain indexed write, not a hashed insert.
pub type RuleHits = PerStreet<Vec<bool>>;

/// A fresh, all-`false` [`RuleHits`] sized to `config`'s three
/// `StreetTree::rules` lists -- the shape both [`Builder`] and [`Counting`]
/// start their walk from, so they can never disagree about how many rules
/// exist per street.
fn rule_hits_for(config: &PostflopConfig) -> RuleHits {
    PerStreet {
        flop: vec![false; config.streets.flop.rules.len()],
        turn: vec![false; config.streets.turn.rules.len()],
        river: vec![false; config.streets.river.rules.len()],
    }
}

/// Every legal action at `state`: the base non-aggressive action(s), the
/// `include_allin` default (added before any rule runs, per
/// `docs/solver-config-v1.jp.md`'s 解決順序 chapter -- a script can
/// then edit it away), then this street's tree-script rules applied in
/// source order. A rule whose `action` doesn't match the node's own wager
/// kind (`Bet` with no outstanding bet, `Raise` facing one) is inert and
/// skipped entirely. Shared between the real builder and the memory-usage
/// dry run so the two can never disagree about a node's action count or
/// order.
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
    let mut actions = base_actions(state);

    if street.include_allin {
        actions.extend(
            sized_targets(state, config, &[SizeSpec::AllIn])
                .into_iter()
                .map(NodeAction::Wager),
        );
    }

    let ctx = rule_context(state, config);
    let node_kind = if state.outstanding == Chips::ZERO {
        ActionKind::Bet
    } else {
        ActionKind::Raise
    };

    for (index, rule) in street.rules.iter().enumerate() {
        if !rule.condition.eval(&ctx) {
            continue;
        }
        hits[state.street][index] = true;
        if rule.effect == Effect::Checkdown {
            actions.retain(|a| matches!(a, NodeAction::Check));
        } else if rule.action == Some(node_kind) {
            let sized = || {
                sized_targets(state, config, &rule.sizes)
                    .into_iter()
                    .map(NodeAction::Wager)
            };
            match rule.effect {
                Effect::Remove => actions.retain(|a| !matches!(a, NodeAction::Wager(_))),
                Effect::Replace => {
                    actions.retain(|a| !matches!(a, NodeAction::Wager(_)));
                    actions.extend(sized());
                }
                Effect::Add => actions.extend(sized()),
                Effect::Force => actions = sized().collect(),
                Effect::Checkdown => unreachable!("checked above"),
            }
        } else {
            // The rule names the other wager kind (`remove bet` facing a
            // bet, `force raise [..]` with nothing to raise): inert here.
            continue;
        }
        actions.sort_by_key(|&a| action_sort_key(a));
        actions.dedup();
    }

    if actions.is_empty() {
        base_actions(state)
    } else {
        actions
    }
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
    /// Accumulated by every [`node_actions`] call this walk makes -- see
    /// [`RuleHits`].
    hits: RuleHits,
}

/// Builds a postflop subgame through the payoff pipeline.
pub fn build_postflop_game(config: &PostflopConfig, pipeline: PayoffPipeline<'_>) -> PostflopGame {
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
            street: start_street,
            ..PostflopNodeInfo::default()
        }],
        hits: rule_hits_for(config),
    };
    let root = builder.betting(LineState {
        street: start_street,
        board_facts: BoardFacts::new(&config.board),
        board: config.board.clone(),
        to_act: Player::P0,
        contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
        outstanding: Chips::ZERO,
        raises_used: 0,
        first_checked: false,
        all_in: false,
        history: String::new(),
        street_start: Chips::ZERO,
        last_full_raise: Chips::ZERO,
        previous_aggressor: config.preflop_aggressor,
        street_aggressor: None,
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
        hits,
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
        rule_hits: hits,
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

        for action in node_actions(&state, self.config, &mut self.hits) {
            // A wager's label and history token both name `to`, the
            // actor's new cumulative contribution. Recomputing it inside
            // each closure keeps both of them lazy: with
            // `track_node_info = false` neither closure runs at all.
            let wager_to = |additional| state.contrib[actor] + additional;
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
            let history = self.extend_history(&state.history, || match action {
                NodeAction::Fold => "f".into(),
                NodeAction::Check => "x".into(),
                NodeAction::Call => "c".into(),
                NodeAction::Wager(additional) => format!("r{}", wager_to(additional)),
            });
            let child = match child_step(&state, action) {
                ChildStep::Fold => {
                    let fold_state = LineState {
                        history,
                        ..state.clone()
                    };
                    self.terminal(&fold_state, TerminalKind::Fold { folder: actor })
                }
                ChildStep::StreetEnd(next) => self.street_end(LineState { history, ..next }),
                ChildStep::Betting(next) => self.betting(LineState { history, ..next }),
            };
            actions.push((label, child));
        }

        self.finish_action_node(actor, &state, state.history.clone(), actions)
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
        debug_assert_eq!(
            state.contrib[Player::P0],
            state.contrib[Player::P1],
            "a chance node is only ever reached right after a call or a check-check, \
             both of which leave contributions equal"
        );
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
            let board_facts = BoardFacts::new(&board);
            let history =
                self.extend_history(&state.history, || format!("[{}]", group.representative));

            let child_state = LineState {
                street: next,
                board_facts,
                board,
                to_act: Player::P0,
                contrib: state.contrib,
                outstanding: Chips::ZERO,
                raises_used: 0,
                first_checked: false,
                all_in: state.all_in,
                history,
                // A chance node is only ever reached right after a call or a
                // check-check, both of which leave `contrib[P0] ==
                // contrib[P1]` — that shared value is the new street's
                // baseline. `state.street_aggressor` (this street's bettor,
                // or `None` on a check-check) becomes the next street's
                // `previous_aggressor`, the donk-menu signal.
                street_start: state.contrib[Player::P0],
                last_full_raise: Chips::ZERO,
                previous_aggressor: state.street_aggressor,
                street_aggressor: None,
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
///
/// No longer `Copy` once [`RuleHits`] joined the struct (a `Vec<bool>` per
/// street isn't); every caller already used this by move or by field access,
/// never relying on implicit duplication, so dropping `Copy` is not a
/// breaking change in practice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEstimate {
    /// Bytes for an `F32Storage` backend (two `f32` arenas).
    pub f32_bytes: u64,
    /// Bytes for a hypothetical quantized `i16` backend: two `i16` arenas
    /// plus a per-action-node `f32` scale pair (regrets, strategy sum).
    pub i16_bytes: u64,
    pub nodes: u64,
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
        hits: rule_hits_for(config),
    };
    counting.betting(LineState {
        street: start_street,
        board_facts: BoardFacts::new(&config.board),
        board: config.board.clone(),
        to_act: Player::P0,
        contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
        outstanding: Chips::ZERO,
        raises_used: 0,
        first_checked: false,
        all_in: false,
        history: String::new(),
        street_start: Chips::ZERO,
        last_full_raise: Chips::ZERO,
        previous_aggressor: config.preflop_aggressor,
        street_aggressor: None,
    });

    MemoryEstimate {
        f32_bytes: counting.elements * 2 * 4,
        i16_bytes: counting.elements * 2 * 2 + counting.action_nodes * 2 * 4,
        nodes: counting.nodes,
        terminals: counting.terminals,
        rank_tables: counting.rank_table_keys.len() as u64,
        rule_hits: counting.hits,
    }
}

/// Counting-only mirror of [`Builder`]'s recursion. Reuses the same
/// `node_actions`/`chance_groups` shape helpers as the real builder so the
/// two can never disagree about how many children a node has.
struct Counting<'a> {
    config: &'a PostflopConfig,
    sym: Vec<SuitPerm>,
    elements: u64,
    nodes: u64,
    terminals: u64,
    action_nodes: u64,
    rank_table_keys: BTreeSet<[Card; 5]>,
    /// Accumulated by every [`node_actions`] call this dry run makes -- see
    /// [`RuleHits`].
    hits: RuleHits,
}

impl Counting<'_> {
    fn betting(&mut self, state: LineState) {
        self.nodes += 1;
        self.action_nodes += 1;

        let actions = node_actions(&state, self.config, &mut self.hits);
        let num_actions = actions.len() as u64;
        for action in actions {
            match child_step(&state, action) {
                ChildStep::Fold => self.terminal_fold(),
                ChildStep::StreetEnd(next) => self.street_end(next),
                ChildStep::Betting(next) => self.betting(next),
            }
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
            let board_facts = BoardFacts::new(&board);
            let child_state = LineState {
                street: next,
                board_facts,
                board,
                to_act: Player::P0,
                contrib: state.contrib,
                outstanding: Chips::ZERO,
                raises_used: 0,
                first_checked: false,
                all_in: state.all_in,
                history: String::new(),
                street_start: state.contrib[Player::P0],
                last_full_raise: Chips::ZERO,
                previous_aggressor: state.street_aggressor,
                street_aggressor: None,
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

#[cfg(test)]
mod tests {
    use super::*;

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
    /// `cards::board`, and the classes here come from permutations that fix
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
}
