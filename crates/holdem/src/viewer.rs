//! Read-only "viewer" query layer for the postflop builder: street tagging
//! for an already-built [`PublicTree`], history replay against a
//! [`PostflopConfig`] to reconstruct the state at a river-entry node, and a
//! fresh from-scratch river-start config for that state.
//!
//! # Design contract
//!
//! The trunk's river subtree rooted at a river-entry `Action` node (the
//! first action node reached right after a chance deal completes the board
//! to 5 cards) is **structurally identical** — same node kinds, same acting
//! players, same branching factor at every node, in the same DFS order — to
//! a fresh river-start [`PostflopConfig`] built with `board` = the completed
//! board, `pot' = pot + 2c`, and `eff' = eff - c`, where `c` is the
//! per-player chip contribution at the moment the river card lands (both
//! players have contributed exactly `c` at that point: a chance node is
//! only ever reached right after a call or a check-check, both of which
//! leave contributions equal — see [`river_entry_state`]'s doc comment).
//! `crates/holdem/tests/viewer.rs`'s guard test checks this against
//! `build_postflop_game` for *every* river-entry node of a real trunk,
//! forever: if a future change to the builder's betting/sizing logic ever
//! breaks this equivalence, that test fails immediately rather than
//! silently shipping a viewer that mis-renders river strategies.
//!
//! This equivalence licenses the standard "small save" viewer-artifact
//! trick (see `docs/research/solver-survey.jp.md` on PioSOLVER's small/very-small
//! saves): a solved trunk's checkpoint can omit river regrets/strategies
//! entirely and re-solve the river on demand when a viewer navigates there,
//! using [`river_entry_state`] + [`river_resolve_config`] to reconstruct an
//! equivalent subgame and running a fresh (short) solve on it. That said, a
//! **reach-weighted fresh river re-solve is only an approximation of the
//! trunk's actual river strategy** at that node, not a bit-identical
//! replay: the trunk solved the whole game jointly, so its river strategy
//! reflects incentives correlated across *every* river-entry node through
//! the shared regret-matching/averaging dynamics, whereas a re-solve seeds
//! each river subgame independently from a snapshotted reach and solves it
//! in isolation. The two agree in the limit of exact convergence at every
//! node (both compute a best response to the same fixed opponent range) but
//! are not bit-identical at any finite iteration count — this is standard
//! viewer-artifact practice (PioSOLVER, TexasSolver), not a bug.

use std::fmt;

use cards::{Card, Chips, NUM_COMBOS, PerPlayer, Player, Range, Street};
use engine::{NodeId, NodeKind, PublicTree};

use crate::postflop::{PerStreet, PostflopConfig};

/// Street of every node in `tree`, given the tree's starting street (derived
/// by the caller from the starting board length: 3 cards = Flop, 4 = Turn,
/// 5 = River). Pure structural walk, no history-string parsing: every child
/// of a [`NodeKind::Chance`] node is one street later than its parent; every
/// other node shares its parent's street. The root itself keeps `start` (it
/// has no parent to inherit from).
pub fn node_streets(tree: &PublicTree, start: Street) -> Vec<Street> {
    let mut streets = vec![start; tree.nodes.len()];
    // Explicit stack (not recursion): large flop trees can nest deep enough
    // that a naive recursive walk risks the stack, and this is meant to be
    // cheap to call from viewer tooling on real trunks.
    let mut stack: Vec<NodeId> = vec![0];
    while let Some(id) = stack.pop() {
        let node = tree.node(id);
        let child_street = match node.kind {
            NodeKind::Chance => next_street(streets[id as usize]),
            _ => streets[id as usize],
        };
        for child in tree.children(id) {
            streets[child as usize] = child_street;
            stack.push(child);
        }
    }
    streets
}

/// One street later. Duplicated (four lines) from `crate::postflop`'s
/// private helper of the same name: that module's internals are not part of
/// this crate's shared surface, and `viewer` intentionally has no other
/// dependency on it.
fn next_street(street: Street) -> Street {
    match street {
        Street::Preflop => unreachable!("postflop trees never start before the flop"),
        Street::Flop => Street::Turn,
        Street::Turn => Street::River,
        Street::River => unreachable!("no chance node follows the river"),
    }
}

/// State at the moment a river-entry node is reached: the completed 5-card
/// board, the pot already built (both players' contributions, from every
/// earlier street, folded in — see the module doc's design contract), and
/// the effective stack remaining behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiverEntryState {
    pub board: [Card; 5],
    pub pot: Chips,
    pub effective_stack: Chips,
}

/// Failure replaying a history string in [`river_entry_state`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayError {
    /// An unrecognized character, or a malformed `b{amount}`/`[Xy]` token.
    BadToken(String),
    /// The history reached a fold before completing the board to 5 cards —
    /// there is no river subgame to reconstruct after a fold.
    FoldedBeforeRiver,
    /// Every token was consumed but the board does not have exactly 5
    /// cards: the history didn't actually reach a river-entry node.
    IncompleteBoard { cards: usize },
    /// A `[Xy]` chance token arrived after the board already had 5 cards.
    BoardOverflow,
    /// Right at the point of river entry the two players' total
    /// contributions this subgame were not equal. Every chance node is only
    /// ever reached immediately after a call or a check-check, both of
    /// which leave contributions equal (see [`river_entry_state`]'s doc
    /// comment), so this indicates the supplied history does not correspond
    /// to a real path through `config`'s builder grammar.
    UnequalContribution { p0: Chips, p1: Chips },
    /// `pot' = trunk.pot + 2c` was odd. Structurally impossible given the
    /// builder's even-starting-pot invariant (see `PostflopConfig::pot`),
    /// kept as a defensive check rather than a silent wraparound.
    OddPot(Chips),
    /// `eff' = trunk.effective_stack - c` would be zero or negative: no
    /// chips remain behind at the river, so there is no river subgame with
    /// any action left to resolve.
    StackUnderflow {
        effective_stack: Chips,
        contrib: Chips,
    },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::BadToken(tok) => write!(f, "unrecognized history token: {tok:?}"),
            ReplayError::FoldedBeforeRiver => {
                write!(f, "history folded before reaching a river-entry node")
            }
            ReplayError::IncompleteBoard { cards } => write!(
                f,
                "history left the board at {cards} card(s), expected exactly 5 at river entry"
            ),
            ReplayError::BoardOverflow => write!(f, "history dealt a card past a 5-card board"),
            ReplayError::UnequalContribution { p0, p1 } => write!(
                f,
                "unequal contributions at river entry: p0={p0} p1={p1} (not a valid history)"
            ),
            ReplayError::OddPot(pot) => write!(f, "river-entry pot {pot} is odd"),
            ReplayError::StackUnderflow {
                effective_stack,
                contrib,
            } => write!(
                f,
                "effective stack {effective_stack} does not cover river-entry contribution {contrib}"
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

/// One history token, per `crate::postflop::Builder::extend_history`'s
/// grammar: `x` check, `f` fold, `c` call, `b{to}` bet/raise, `[Xy]` a
/// chance card extending the board.
enum Token {
    Check,
    Fold,
    Call,
    /// `to`: the acting player's new total contribution this subgame,
    /// *cumulative from the subgame's start* (not a per-street increment —
    /// see [`river_entry_state`]'s doc comment on why this reading is the
    /// one that makes the replay arithmetic work: `postflop::Builder::betting`
    /// sets this history token from `contrib[actor]` *after* adding the
    /// action's chip amount, and `contrib` is explicitly never reset between
    /// streets).
    Bet(u32),
    Chance(Card),
}

/// Splits a history string into tokens. `[Xy]` is always exactly a
/// bracketed 2-character card (rank + suit), so there is no ambiguity with
/// the bare `c` (call) token despite clubs also being spelled `c`.
fn tokenize(history: &str) -> Result<Vec<Token>, ReplayError> {
    let mut tokens = Vec::new();
    let mut chars = history.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'x' => tokens.push(Token::Check),
            'f' => tokens.push(Token::Fold),
            'c' => tokens.push(Token::Call),
            'b' => {
                let mut digits = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        digits.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let to: u32 = digits
                    .parse()
                    .map_err(|_| ReplayError::BadToken(format!("b{digits}")))?;
                tokens.push(Token::Bet(to));
            }
            '[' => {
                let card_str: String = (&mut chars).take(2).collect();
                if card_str.chars().count() != 2 || chars.next() != Some(']') {
                    return Err(ReplayError::BadToken(format!("[{card_str}")));
                }
                let card: Card = card_str
                    .parse()
                    .map_err(|_| ReplayError::BadToken(format!("[{card_str}]")))?;
                tokens.push(Token::Chance(card));
            }
            other => return Err(ReplayError::BadToken(other.to_string())),
        }
    }
    Ok(tokens)
}

/// Replays a history string of tokens `x` / `c` / `f` / `b{to}` / `[Xy]`
/// against `config` (the trunk [`PostflopConfig`]) up to a river-entry node,
/// returning that node's board/pot/effective-stack state.
///
/// The replay does not need to track "outstanding" (the amount owed to
/// call) as a separate quantity, because of an invariant in
/// `crate::postflop::Builder::betting`/`raise_targets`: `b{to}` already
/// encodes the acting player's new *absolute* contribution `to`, and every
/// bet/raise leaves `outstanding == |contrib[P0] - contrib[P1]|` (the raise
/// size is layered on top of whatever the actor already owed). So a `c`
/// (call) token can simply set the caller's contribution equal to the
/// opponent's, and a chance token is only ever reached once contributions
/// are already equal (the invariant this function's `UnequalContribution`
/// error defends).
pub fn river_entry_state(
    config: &PostflopConfig,
    history: &str,
) -> Result<RiverEntryState, ReplayError> {
    let tokens = tokenize(history)?;

    let mut board = config.board.clone();
    let mut contrib = PerPlayer::new(Chips::ZERO, Chips::ZERO);
    let mut to_act = Player::P0;
    let mut first_checked = false;

    for token in tokens {
        match token {
            Token::Fold => return Err(ReplayError::FoldedBeforeRiver),
            Token::Check => {
                if first_checked {
                    // Check-check: the street is over, so the very next
                    // token must be a chance card. `to_act` resets exactly
                    // like `Builder::deal_chance`'s `child_state`.
                    to_act = Player::P0;
                    first_checked = false;
                } else {
                    first_checked = true;
                    to_act = to_act.opponent();
                }
            }
            Token::Call => {
                let opponent = to_act.opponent();
                contrib[to_act] = contrib[opponent];
                to_act = Player::P0;
                first_checked = false;
            }
            Token::Bet(to) => {
                contrib[to_act] = Chips(to);
                to_act = to_act.opponent();
            }
            Token::Chance(card) => {
                if board.len() >= 5 {
                    return Err(ReplayError::BoardOverflow);
                }
                board.push(card);
                to_act = Player::P0;
                first_checked = false;
            }
        }
    }

    if board.len() != 5 {
        return Err(ReplayError::IncompleteBoard { cards: board.len() });
    }
    let (p0, p1) = (contrib[Player::P0], contrib[Player::P1]);
    if p0 != p1 {
        return Err(ReplayError::UnequalContribution { p0, p1 });
    }
    let c = p0;
    let pot = config.pot + c + c;
    if !pot.0.is_multiple_of(2) {
        return Err(ReplayError::OddPot(pot));
    }
    if c >= config.effective_stack {
        return Err(ReplayError::StackUnderflow {
            effective_stack: config.effective_stack,
            contrib: c,
        });
    }
    let effective_stack = config.effective_stack - c;

    let board: [Card; 5] = board
        .try_into()
        .expect("board length checked to be exactly 5 above");
    Ok(RiverEntryState {
        board,
        pot,
        effective_stack,
    })
}

/// Fresh river-start [`PostflopConfig`] for the subgame rooted at a river
/// entry: board and pot/effective-stack from `entry` (see
/// [`RiverEntryState`] — `entry.pot` already includes both players' `c`),
/// ranges built from `reach` (each combo's reach becomes that combo's
/// weight — always a valid weight since reach values are products of `[0,
/// 1]` factors, per [`engine::reach_at`]'s contract: strategy-column
/// probabilities and `Mask`/`Transition` weights, none of which ever push a
/// product outside `[0, 1]`; clamped defensively against float rounding at
/// the boundary regardless), river bet/raise menus and raise cap copied from
/// `trunk` (the only street the fresh subgame ever plays), flop/turn menus
/// left empty (moot: a river-start board has no chance nodes), `iso_merging:
/// false` (also moot, for the same reason — nothing left to merge with no
/// chance nodes), and `track_node_info: true` (a re-solved subgame is
/// exactly the thing a viewer wants node histories for).
pub fn river_resolve_config(
    trunk: &PostflopConfig,
    entry: &RiverEntryState,
    reach: &PerPlayer<Vec<f32>>,
) -> PostflopConfig {
    let build_range = |p: Player| -> Range {
        let mut range = Range::default();
        for combo in 0..NUM_COMBOS {
            range.set_weight(combo, reach[p][combo].clamp(0.0, 1.0));
        }
        range
    };

    PostflopConfig {
        board: entry.board.to_vec(),
        ranges: PerPlayer::new(build_range(Player::P0), build_range(Player::P1)),
        pot: entry.pot,
        effective_stack: entry.effective_stack,
        bet_fractions: PerStreet {
            flop: PerPlayer::new(Vec::new(), Vec::new()),
            turn: PerPlayer::new(Vec::new(), Vec::new()),
            river: trunk.bet_fractions.river.clone(),
        },
        raise_fractions: PerStreet {
            flop: PerPlayer::new(Vec::new(), Vec::new()),
            turn: PerPlayer::new(Vec::new(), Vec::new()),
            river: trunk.raise_fractions.river.clone(),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 0,
            river: trunk.max_raises.river,
        },
        iso_merging: false,
        track_node_info: true,
    }
}
