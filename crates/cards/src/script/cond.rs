//! Dialect, variables, and the compiled boolean condition tree the tree
//! script's `when` / `if` conditions lower to.
//!
//! A [`Condition<V>`] is compiled once, at parse time, against a concrete
//! [`Dialect<V>`] -- so a bad identifier or a type mismatch is a
//! [`ScriptError`] the caller sees immediately, and [`Condition::eval`] can
//! be infallible: there is nothing left for it to get wrong at tree-build
//! time, on the tree builder's hot path.
//!
//! `V` is what makes this reusable across families: postflop's [`PostflopVar`]
//! and `multiway`'s own variable enum are both just implementations of
//! [`Vars`], read through [`VarSource<V>`] -- postflop's [`RuleContext`]
//! implements `VarSource<PostflopVar>` right here, since board/card concepts
//! belong in `cards`; `multiway` implements `VarSource<MultiwayVar>` for its
//! own betting-state types, in its own crate, with no orphan-rule problem
//! (multiway already depends on `cards`).

use std::fmt;

use super::ScriptError;
use super::token::{Token, TokenKind, expect_punct, is_punct, is_word, line_at, tokenize};
use crate::{BoardFacts, SizeUnit, Street, rank_name};

/// The type a variable reads as, used to type-check conditions at parse
/// time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Bool,
    Number,
    Text,
}

/// One family's vocabulary of named condition variables. A dialect's
/// [`Dialect::vars`] lists the concrete `V` values a script may name; a name
/// outside that list is an unknown identifier rather than a silently-false
/// condition (see `is_reserved` and the parser's identifier lookup).
///
/// `'static` and `Copy` because a [`Condition<V>`] stores `V` by value at
/// every leaf and is expected to be cheap to clone and to live as long as
/// the compiled tree that holds it.
pub trait Vars: Copy + Eq + fmt::Debug + 'static {
    /// The identifier a script author types for this variable.
    fn name(self) -> &'static str;
    /// The type this variable's value has, which constrains which
    /// comparisons a condition may use against it.
    fn kind(self) -> VarKind;
}

/// One named variable postflop's tree script condition can read. The full
/// set below is [`POSTFLOP`]'s; `multiway` exposes a different, narrower set
/// through its own [`Vars`] implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostflopVar {
    Aggressions,
    Raises,
    Unopened,
    InPosition,
    Position,
    Players,
    Spr,
    Pot,
    ToCall,
    FacingPct,
    Cbet,
    Donk,
    BoardCards,
    BoardSuits,
    BoardRanks,
    StraightRanks,
    Paired,
    Monotone,
    TwoTone,
    Rainbow,
    FlushPossible,
    StraightPossible,
    HighCard,
    LowCard,
}

impl Vars for PostflopVar {
    fn name(self) -> &'static str {
        use PostflopVar::*;
        match self {
            Aggressions => "aggressions",
            Raises => "raises",
            Unopened => "unopened",
            InPosition => "in_position",
            Position => "position",
            Players => "players",
            Spr => "spr",
            Pot => "pot",
            ToCall => "to_call",
            FacingPct => "facing_pct",
            Cbet => "cbet",
            Donk => "donk",
            BoardCards => "board_cards",
            BoardSuits => "board_suits",
            BoardRanks => "board_ranks",
            StraightRanks => "straight_ranks",
            Paired => "paired",
            Monotone => "monotone",
            TwoTone => "two_tone",
            Rainbow => "rainbow",
            FlushPossible => "flush_possible",
            StraightPossible => "straight_possible",
            HighCard => "high_card",
            LowCard => "low_card",
        }
    }

    fn kind(self) -> VarKind {
        use PostflopVar::*;
        match self {
            Aggressions | Raises | Players | Spr | Pot | ToCall | FacingPct | BoardCards
            | BoardSuits | BoardRanks | StraightRanks => VarKind::Number,
            Unopened | InPosition | Cbet | Donk | Paired | Monotone | TwoTone | Rainbow
            | FlushPossible | StraightPossible => VarKind::Bool,
            Position | HighCard | LowCard => VarKind::Text,
        }
    }
}

impl PostflopVar {
    /// True for the twelve board-texture variables, i.e. the ones
    /// [`Condition::specialize`] can fold once a board is known. Not part of
    /// [`Vars`]: "board variable" is a postflop/`BoardFacts` concept with no
    /// equivalent in another family's dialect.
    fn is_board_var(self) -> bool {
        use PostflopVar::*;
        matches!(
            self,
            BoardCards
                | BoardSuits
                | BoardRanks
                | StraightRanks
                | Paired
                | Monotone
                | TwoTone
                | Rainbow
                | FlushPossible
                | StraightPossible
                | HighCard
                | LowCard
        )
    }
}

/// The full set of [`PostflopVar`]s, in the order [`POSTFLOP`] exposes them
/// -- used only to build that dialect table; iterate `Dialect::vars` for
/// anything that should respect a narrower dialect.
const ALL_POSTFLOP_VARS: &[PostflopVar] = &[
    PostflopVar::Aggressions,
    PostflopVar::Raises,
    PostflopVar::Unopened,
    PostflopVar::InPosition,
    PostflopVar::Position,
    PostflopVar::Players,
    PostflopVar::Spr,
    PostflopVar::Pot,
    PostflopVar::ToCall,
    PostflopVar::FacingPct,
    PostflopVar::Cbet,
    PostflopVar::Donk,
    PostflopVar::BoardCards,
    PostflopVar::BoardSuits,
    PostflopVar::BoardRanks,
    PostflopVar::StraightRanks,
    PostflopVar::Paired,
    PostflopVar::Monotone,
    PostflopVar::TwoTone,
    PostflopVar::Rainbow,
    PostflopVar::FlushPossible,
    PostflopVar::StraightPossible,
    PostflopVar::HighCard,
    PostflopVar::LowCard,
];

/// Which kind of aggressive-or-not action a rule's `add` / `remove` /
/// `replace` / `force` names. All five variants live in one enum regardless
/// of dialect; [`Dialect::actions`] is what actually narrows which names a
/// given family's grammar accepts (postflop lists only `Bet/Raise` -- its
/// `node_actions` already skips any rule whose action is not the node's own
/// wager kind, so `Fold`/`Check`/`Call` simply never match there even if a
/// caller constructed one directly). `Serialize`/`Deserialize` are here so a
/// family whose typed rule surface names an action directly in TOML (as
/// `multiway::config::RuleAction`, a type alias for this enum) needs no
/// second copy of the same five spellings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    Fold,
    Check,
    Call,
    Bet,
    Raise,
}

impl ActionKind {
    /// The identifier a script author types for this action.
    pub fn name(self) -> &'static str {
        match self {
            ActionKind::Fold => "fold",
            ActionKind::Check => "check",
            ActionKind::Call => "call",
            ActionKind::Bet => "bet",
            ActionKind::Raise => "raise",
        }
    }
}

/// Which names and street vocabulary one config family's tree script
/// exposes. `cards` has no notion of a family (that lives in `holdem` /
/// `multiway`), so a family builds its own `Dialect<V>` naming the `V`
/// values, action words, and street keywords it wants; [`POSTFLOP`] is the
/// one the postflop family uses today.
#[derive(Clone, Copy, Debug)]
pub struct Dialect<V: Vars> {
    pub vars: &'static [V],
    pub streets: &'static [(&'static str, Street)],
    pub actions: &'static [ActionKind],
    pub unit: SizeUnit,
}

/// The postflop tree script's dialect: every [`PostflopVar`], the three
/// postflop streets, `bet`/`raise` (postflop's tree never adds or removes a
/// fold/check/call candidate through a rule), and chip-denominated sizes.
pub static POSTFLOP: Dialect<PostflopVar> = Dialect {
    vars: ALL_POSTFLOP_VARS,
    streets: &[
        ("flop", Street::Flop),
        ("turn", Street::Turn),
        ("river", Street::River),
    ],
    actions: &[ActionKind::Bet, ActionKind::Raise],
    unit: SizeUnit::Chips,
};

/// Reserved names a `param` or `define` may never shadow: every dialect
/// variable name, the dialect's street keywords, every action word (not just
/// the ones this dialect's grammar accepts -- a name is either safe in every
/// dialect or reserved in every dialect, so it stays safe if a future
/// dialect widens its `actions`), the size literals with no numeric suffix,
/// the boolean literals, and every script keyword. A substitution must never
/// make a literal or a variable mean something else.
pub(crate) fn is_reserved<V: Vars>(name: &str, dialect: &Dialect<V>) -> bool {
    const KEYWORDS: &[&str] = &[
        "param",
        "define",
        "when",
        "if",
        "else",
        "in",
        "add",
        "remove",
        "replace",
        "force",
        "checkdown",
        "fold",
        "check",
        "call",
        "bet",
        "raise",
        "true",
        "false",
        "a",
        "e",
        "min",
    ];
    KEYWORDS.contains(&name)
        || dialect.vars.iter().any(|v| v.name() == name)
        || dialect
            .streets
            .iter()
            .any(|(street_name, _)| *street_name == name)
}

/// Which side, if any, made the last bet or raise before the current
/// street started. Read by the `cbet` / `donk` derived variables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviousAggressor {
    None,
    Actor,
    Opponent,
}

/// Everything a condition may read about one postflop decision node.
/// Deliberately a concrete struct of primitives, not a trait: it keeps
/// evaluation allocation-free and dispatch-free on the tree builder's hot
/// path. Implements [`VarSource<PostflopVar>`] below.
#[derive(Clone, Copy, Debug)]
pub struct RuleContext {
    pub aggressions: u32,
    pub in_position: bool,
    pub spr: f64,
    pub pot: f64,
    pub to_call: f64,
    pub previous_aggressor: PreviousAggressor,
    pub board: BoardFacts,
}

/// The part of a [`RuleContext`] that is already fixed once a chance node
/// has dealt the board -- everything [`Condition::specialize`] can use to
/// fold a condition ahead of time.
#[derive(Clone, Copy, Debug, Default)]
pub struct PartialContext {
    pub board: Option<BoardFacts>,
}

/// A comparison operator between a variable and a literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Eq,
    Ne,
    Ge,
    Gt,
}

/// A literal value written in a condition or `in [...]` list.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// A compiled tree-script condition, generic over one family's variable
/// vocabulary `V`. Every leaf is type-checked against a [`Dialect<V>`] at
/// parse time (see [`Condition::parse`]), so [`Condition::eval`] never fails
/// and never needs to allocate.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition<V> {
    Const(bool),
    Truth(V),
    Not(Box<Condition<V>>),
    And(Box<Condition<V>>, Box<Condition<V>>),
    Or(Box<Condition<V>>, Box<Condition<V>>),
    Compare { var: V, op: CmpOp, value: Literal },
    Member { var: V, values: Vec<Literal> },
}

/// A variable's value at one decision node, resolved just long enough to
/// compare against a [`Literal`]. The text variant is `&'static str`, not
/// `String`: every family's text-valued variables (postflop's
/// `position`/`high_card`/`low_card`, multiway's `position`) come from
/// static tables, so producing one never needs to allocate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(&'static str),
}

/// One family's answer to "what is this variable worth here". Implemented
/// once per (family, node-representation) pair -- postflop's [`RuleContext`]
/// below, `multiway`'s `(&BettingState, SeatId)` in its own crate -- so
/// [`Condition::eval`] stays generic over both without ever boxing or
/// dispatching dynamically.
pub trait VarSource<V> {
    fn value(&self, var: V) -> Value;
}

impl VarSource<PostflopVar> for RuleContext {
    fn value(&self, var: PostflopVar) -> Value {
        use PostflopVar::*;
        match var {
            Aggressions | Raises => Value::Number(f64::from(self.aggressions)),
            Unopened => Value::Bool(self.aggressions == 0),
            InPosition => Value::Bool(self.in_position),
            Position => Value::Text(if self.in_position { "IP" } else { "OOP" }),
            Players => Value::Number(2.0),
            Spr => Value::Number(self.spr),
            Pot => Value::Number(self.pot),
            ToCall => Value::Number(self.to_call),
            FacingPct => Value::Number(if self.pot <= 0.0 {
                0.0
            } else {
                self.to_call / self.pot * 100.0
            }),
            Cbet => Value::Bool(
                self.aggressions == 0
                    && matches!(self.previous_aggressor, PreviousAggressor::Actor),
            ),
            Donk => Value::Bool(
                self.aggressions == 0
                    && matches!(self.previous_aggressor, PreviousAggressor::Opponent),
            ),
            BoardCards | BoardSuits | BoardRanks | StraightRanks | Paired | Monotone | TwoTone
            | Rainbow | FlushPossible | StraightPossible | HighCard | LowCard => {
                board_value(var, self.board)
            }
        }
    }
}

fn board_value(var: PostflopVar, board: BoardFacts) -> Value {
    use PostflopVar::*;
    match var {
        BoardCards => Value::Number(f64::from(board.cards)),
        BoardSuits => Value::Number(f64::from(board.suits)),
        BoardRanks => Value::Number(f64::from(board.ranks)),
        StraightRanks => Value::Number(f64::from(board.straight_ranks)),
        Paired => Value::Bool(board.paired()),
        Monotone => Value::Bool(board.monotone()),
        TwoTone => Value::Bool(board.two_tone()),
        Rainbow => Value::Bool(board.rainbow()),
        FlushPossible => Value::Bool(board.flush_possible()),
        StraightPossible => Value::Bool(board.straight_possible()),
        HighCard => Value::Text(rank_name(board.high_rank)),
        LowCard => Value::Text(rank_name(board.low_rank)),
        _ => unreachable!("board_value called with a non-board var"),
    }
}

impl<V: Vars> Condition<V> {
    /// Compiles one standalone condition expression -- not a whole script,
    /// just the grammar `parse_condition` accepts -- against `dialect`,
    /// rejecting any trailing tokens after it. This is the entry point a
    /// family reaches for when it wants a single compiled condition rather
    /// than a full [`super::Script`] (multiway's `when = "..."` tree-rule
    /// strings, and this module's own tests).
    pub fn parse(source: &str, dialect: &Dialect<V>) -> Result<Condition<V>, ScriptError> {
        let tokens = tokenize(source)?;
        let significant: Vec<Token> = tokens
            .into_iter()
            .filter(|t| !matches!(t.kind, TokenKind::Comment(_)))
            .collect();
        let mut pos = 0;
        let condition = parse_condition(&significant, &mut pos, dialect)?;
        if pos != significant.len() {
            return Err(ScriptError {
                line: line_at(&significant, pos),
                message: "unexpected trailing tokens after condition".to_string(),
            });
        }
        Ok(condition)
    }

    /// Evaluates this condition against a concrete decision node. Returns
    /// `bool`, not `Result`: every leaf was checked against the dialect at
    /// parse time, so there is nothing left to fail here.
    pub fn eval<S: VarSource<V>>(&self, source: &S) -> bool {
        match self {
            Condition::Const(value) => *value,
            Condition::Truth(var) => match source.value(*var) {
                Value::Bool(value) => value,
                _ => unreachable!("Truth is only constructed for boolean vars"),
            },
            Condition::Not(inner) => !inner.eval(source),
            Condition::And(left, right) => left.eval(source) && right.eval(source),
            Condition::Or(left, right) => left.eval(source) || right.eval(source),
            Condition::Compare { var, op, value } => compare(source.value(*var), *op, value),
            Condition::Member { var, values } => {
                let actual = source.value(*var);
                values.iter().any(|literal| equals(actual, literal))
            }
        }
    }
}

impl Condition<PostflopVar> {
    /// Replaces every board-derived variable with a constant once the board
    /// is known, then folds `Not` / `And` / `Or` so a fully-determined
    /// subtree collapses to `Const`. This lets the tree builder specialize
    /// the rule list once per dealt board (a per-chance-node cost) instead
    /// of re-testing board predicates at every decision node under it (a
    /// per-node cost). Not wired into the builder yet. Postflop-only: "board
    /// variable" and [`PartialContext`] are `BoardFacts` concepts with no
    /// equivalent in another family's dialect.
    pub fn specialize(&self, known: &PartialContext) -> Condition<PostflopVar> {
        match self {
            Condition::Const(value) => Condition::Const(*value),
            Condition::Truth(var) => {
                if var.is_board_var()
                    && let Some(board) = known.board
                {
                    let Value::Bool(value) = board_value(*var, board) else {
                        unreachable!("Truth is only constructed for boolean vars")
                    };
                    return Condition::Const(value);
                }
                Condition::Truth(*var)
            }
            Condition::Not(inner) => match inner.specialize(known) {
                Condition::Const(value) => Condition::Const(!value),
                other => Condition::Not(Box::new(other)),
            },
            Condition::And(left, right) => and(left.specialize(known), right.specialize(known)),
            Condition::Or(left, right) => or(left.specialize(known), right.specialize(known)),
            Condition::Compare { var, op, value } => {
                if var.is_board_var()
                    && let Some(board) = known.board
                {
                    return Condition::Const(compare(board_value(*var, board), *op, value));
                }
                Condition::Compare {
                    var: *var,
                    op: *op,
                    value: value.clone(),
                }
            }
            Condition::Member { var, values } => {
                if var.is_board_var()
                    && let Some(board) = known.board
                {
                    let actual = board_value(*var, board);
                    return Condition::Const(values.iter().any(|literal| equals(actual, literal)));
                }
                Condition::Member {
                    var: *var,
                    values: values.clone(),
                }
            }
        }
    }
}

fn and<V>(left: Condition<V>, right: Condition<V>) -> Condition<V> {
    match (&left, &right) {
        (Condition::Const(false), _) | (_, Condition::Const(false)) => Condition::Const(false),
        (Condition::Const(true), _) => right,
        (_, Condition::Const(true)) => left,
        _ => Condition::And(Box::new(left), Box::new(right)),
    }
}

fn or<V>(left: Condition<V>, right: Condition<V>) -> Condition<V> {
    match (&left, &right) {
        (Condition::Const(true), _) | (_, Condition::Const(true)) => Condition::Const(true),
        (Condition::Const(false), _) => right,
        (_, Condition::Const(false)) => left,
        _ => Condition::Or(Box::new(left), Box::new(right)),
    }
}

/// AND-combines two conditions, collapsing away a literal `true` on either
/// side so an unconditioned statement's `Rule::condition` is exactly
/// `Condition::Const(true)` rather than a chain of vacuous `And`s.
pub(crate) fn and_simplify<V>(left: Condition<V>, right: Condition<V>) -> Condition<V> {
    and(left, right)
}

fn compare(actual: Value, op: CmpOp, literal: &Literal) -> bool {
    match (actual, literal) {
        (Value::Bool(actual), Literal::Bool(expected)) => match op {
            CmpOp::Eq => actual == *expected,
            CmpOp::Ne => actual != *expected,
            _ => unreachable!("booleans only type-check against == and !="),
        },
        (Value::Number(actual), Literal::Number(expected)) => match op {
            CmpOp::Lt => actual < *expected,
            CmpOp::Le => actual <= *expected,
            CmpOp::Eq => actual == *expected,
            CmpOp::Ne => actual != *expected,
            CmpOp::Ge => actual >= *expected,
            CmpOp::Gt => actual > *expected,
        },
        (Value::Text(actual), Literal::Text(expected)) => match op {
            CmpOp::Eq => actual == expected.as_str(),
            CmpOp::Ne => actual != expected.as_str(),
            _ => unreachable!("text only type-checks against == and !="),
        },
        _ => unreachable!("type mismatch should have been rejected at parse time"),
    }
}

fn equals(actual: Value, literal: &Literal) -> bool {
    match (actual, literal) {
        (Value::Bool(actual), Literal::Bool(expected)) => actual == *expected,
        (Value::Number(actual), Literal::Number(expected)) => actual == *expected,
        (Value::Text(actual), Literal::Text(expected)) => actual == expected.as_str(),
        _ => unreachable!("type mismatch should have been rejected at parse time"),
    }
}

// ---- condition grammar: `||` < `&&` < unary `!` < primary ----------------

/// Parses one condition starting at `*pos`, type-checking every leaf
/// against `dialect`. Stops as soon as the grammar can extend no further
/// (in particular, `{` is never part of a condition), so the caller can
/// parse `when <condition> {` by calling this and then expecting `{`.
pub(crate) fn parse_condition<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<Condition<V>, ScriptError> {
    parse_or(tokens, pos, dialect)
}

fn parse_or<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<Condition<V>, ScriptError> {
    let mut left = parse_and(tokens, pos, dialect)?;
    while is_punct(tokens, *pos, "||") {
        *pos += 1;
        let right = parse_and(tokens, pos, dialect)?;
        left = Condition::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<Condition<V>, ScriptError> {
    let mut left = parse_unary(tokens, pos, dialect)?;
    while is_punct(tokens, *pos, "&&") {
        *pos += 1;
        let right = parse_unary(tokens, pos, dialect)?;
        left = Condition::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_unary<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<Condition<V>, ScriptError> {
    if is_punct(tokens, *pos, "!") {
        *pos += 1;
        let inner = parse_unary(tokens, pos, dialect)?;
        return Ok(Condition::Not(Box::new(inner)));
    }
    if is_punct(tokens, *pos, "(") {
        *pos += 1;
        let inner = parse_or(tokens, pos, dialect)?;
        expect_punct(tokens, pos, ")")?;
        return Ok(inner);
    }
    parse_predicate(tokens, pos, dialect)
}

fn parse_predicate<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<Condition<V>, ScriptError> {
    let line = line_at(tokens, *pos);
    let name = expect_identifier(tokens, pos)?;
    let var = dialect
        .vars
        .iter()
        .copied()
        .find(|v| v.name() == name)
        .ok_or_else(|| ScriptError {
            line,
            message: format!("unknown identifier {name:?}"),
        })?;

    if is_word(tokens, *pos, "in") {
        *pos += 1;
        return parse_membership(tokens, pos, var, line);
    }
    if let Some(op) = peek_cmp_op(tokens, *pos) {
        *pos += 1;
        let (value_line, literal) = parse_literal(tokens, pos)?;
        check_compare_types(var, op, &literal, line, value_line)?;
        return Ok(Condition::Compare {
            var,
            op,
            value: literal,
        });
    }
    match var.kind() {
        VarKind::Bool => Ok(Condition::Truth(var)),
        VarKind::Number => Err(ScriptError {
            line,
            message: format!(
                "{:?} is a number and requires a comparison or 'in'",
                var.name()
            ),
        }),
        VarKind::Text => Err(ScriptError {
            line,
            message: format!("{:?} is text and requires a comparison or 'in'", var.name()),
        }),
    }
}

fn parse_membership<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    var: V,
    var_line: usize,
) -> Result<Condition<V>, ScriptError> {
    if var.kind() == VarKind::Bool {
        return Err(ScriptError {
            line: var_line,
            message: format!("{:?} is boolean and does not support 'in'", var.name()),
        });
    }
    expect_punct(tokens, pos, "[")?;
    let mut values = Vec::new();
    if !is_punct(tokens, *pos, "]") {
        loop {
            let (value_line, literal) = parse_literal(tokens, pos)?;
            check_member_type(var, &literal, value_line)?;
            values.push(literal);
            if is_punct(tokens, *pos, ",") {
                *pos += 1;
                continue;
            }
            break;
        }
    }
    expect_punct(tokens, pos, "]")?;
    Ok(Condition::Member { var, values })
}

fn peek_cmp_op(tokens: &[Token], pos: usize) -> Option<CmpOp> {
    match tokens.get(pos).map(|t| &t.kind) {
        Some(TokenKind::Punct("<")) => Some(CmpOp::Lt),
        Some(TokenKind::Punct("<=")) => Some(CmpOp::Le),
        Some(TokenKind::Punct("==")) => Some(CmpOp::Eq),
        Some(TokenKind::Punct("!=")) => Some(CmpOp::Ne),
        Some(TokenKind::Punct(">=")) => Some(CmpOp::Ge),
        Some(TokenKind::Punct(">")) => Some(CmpOp::Gt),
        _ => None,
    }
}

/// Parses one literal: a quoted string, `true` / `false`, a number, or --
/// matching multiway -- a bare word that is none of those, which is a text
/// literal (`high_card == A` works the same as `high_card == "A"`).
fn parse_literal(tokens: &[Token], pos: &mut usize) -> Result<(usize, Literal), ScriptError> {
    let line = line_at(tokens, *pos);
    match tokens.get(*pos).map(|t| &t.kind) {
        Some(TokenKind::Str(text)) => {
            let literal = Literal::Text(text.clone());
            *pos += 1;
            Ok((line, literal))
        }
        Some(TokenKind::Word(word)) => {
            let literal = if word == "true" {
                Literal::Bool(true)
            } else if word == "false" {
                Literal::Bool(false)
            } else if let Ok(number) = word.parse::<f64>() {
                Literal::Number(number)
            } else {
                Literal::Text(word.clone())
            };
            *pos += 1;
            Ok((line, literal))
        }
        _ => Err(ScriptError {
            line,
            message: "expected a literal".to_string(),
        }),
    }
}

fn expect_identifier(tokens: &[Token], pos: &mut usize) -> Result<String, ScriptError> {
    let line = line_at(tokens, *pos);
    match tokens.get(*pos).map(|t| &t.kind) {
        Some(TokenKind::Word(word)) => {
            let word = word.clone();
            *pos += 1;
            Ok(word)
        }
        _ => Err(ScriptError {
            line,
            message: "expected an identifier".to_string(),
        }),
    }
}

fn check_compare_types<V: Vars>(
    var: V,
    op: CmpOp,
    literal: &Literal,
    var_line: usize,
    value_line: usize,
) -> Result<(), ScriptError> {
    match var.kind() {
        VarKind::Bool => match (op, literal) {
            (CmpOp::Eq | CmpOp::Ne, Literal::Bool(_)) => Ok(()),
            _ => Err(ScriptError {
                line: var_line,
                message: format!(
                    "{:?} is boolean and supports only == or != against true/false",
                    var.name()
                ),
            }),
        },
        VarKind::Number => match literal {
            Literal::Number(_) => Ok(()),
            _ => Err(ScriptError {
                line: value_line,
                message: format!("{:?} is a number and requires a number literal", var.name()),
            }),
        },
        VarKind::Text => match (op, literal) {
            (CmpOp::Eq | CmpOp::Ne, Literal::Text(_)) => Ok(()),
            (CmpOp::Eq | CmpOp::Ne, _) => Err(ScriptError {
                line: value_line,
                message: format!("{:?} is text and requires a text literal", var.name()),
            }),
            _ => Err(ScriptError {
                line: var_line,
                message: format!(
                    "ordering operators are not valid on text or boolean variable {:?}",
                    var.name()
                ),
            }),
        },
    }
}

fn check_member_type<V: Vars>(
    var: V,
    literal: &Literal,
    value_line: usize,
) -> Result<(), ScriptError> {
    match (var.kind(), literal) {
        (VarKind::Number, Literal::Number(_)) | (VarKind::Text, Literal::Text(_)) => Ok(()),
        (VarKind::Number, _) => Err(ScriptError {
            line: value_line,
            message: format!(
                "{:?} is a number and requires number literals in 'in [...]'",
                var.name()
            ),
        }),
        (VarKind::Text, _) => Err(ScriptError {
            line: value_line,
            message: format!(
                "{:?} is text and requires text literals in 'in [...]'",
                var.name()
            ),
        }),
        (VarKind::Bool, _) => Err(ScriptError {
            line: value_line,
            message: format!("{:?} is boolean and does not support 'in'", var.name()),
        }),
    }
}

impl fmt::Display for PostflopVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Renders a condition back to source-like text, using the tree script's own
/// precedence (`||` lowest, `&&` next, unary `!` tightest, comparisons /
/// membership / truth tightest of all): a subexpression is parenthesized
/// only when it sits under an operator that binds tighter than it does.
/// Reused by every dialect's diagnostics (postflop's `tree` diagnostic) and
/// by `multiway`'s `.mwtree` frontend, which has no other source text for a
/// rule whose condition is a nested `when`/`if` composition that never
/// existed as one literal string.
///
/// `Not` is the one deliberate exception -- its operand is always
/// parenthesized, matching this contract's own normalization example
/// (`!(A) && !(B) && !(C) && D`, in the "正規化" section of
/// `docs/solver-config-v1.jp.md`): a bare `!paired` reads fine on its own,
/// but scanning a long `&&` chain for which term is negated does not.
impl<V: Vars> fmt::Display for Condition<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", render_condition_prec(self, 0))
    }
}

/// `min_prec` is the precedence of the operator `condition` sits under (0 at
/// the top level, `||`'s own precedence); `condition` is parenthesized if
/// its own precedence is lower. Precedence levels: `||` = 0, `&&` = 1,
/// everything else (unary `!`, comparisons, membership, `Truth`, `Const`) =
/// 2, matching this module's own grammar (`parse_or` < `parse_and` <
/// `parse_unary` < `parse_predicate`).
fn render_condition_prec<V: Vars>(condition: &Condition<V>, min_prec: u8) -> String {
    let (text, prec) = match condition {
        // `Const` has no script syntax of its own -- it only ever appears as
        // the base of an unconditioned statement (`Const(true)`) or a folded
        // board-only condition (`Condition::specialize`). Spelling it out
        // beats inventing pseudo-code.
        Condition::Const(true) => ("always".to_string(), 2),
        Condition::Const(false) => ("never".to_string(), 2),
        Condition::Truth(var) => (var.name().to_string(), 2),
        Condition::Not(inner) => (format!("!({})", render_condition_prec(inner, 0)), 2),
        Condition::And(left, right) => (
            format!(
                "{} && {}",
                render_condition_prec(left, 1),
                render_condition_prec(right, 1)
            ),
            1,
        ),
        Condition::Or(left, right) => (
            format!(
                "{} || {}",
                render_condition_prec(left, 0),
                render_condition_prec(right, 0)
            ),
            0,
        ),
        Condition::Compare { var, op, value } => (
            format!(
                "{} {} {}",
                var.name(),
                cmp_op_str(*op),
                render_literal(value)
            ),
            2,
        ),
        Condition::Member { var, values } => (
            format!(
                "{} in [{}]",
                var.name(),
                values
                    .iter()
                    .map(render_literal)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            2,
        ),
    };
    if prec < min_prec {
        format!("({text})")
    } else {
        text
    }
}

fn cmp_op_str(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Lt => "<",
        CmpOp::Le => "<=",
        CmpOp::Eq => "==",
        CmpOp::Ne => "!=",
        CmpOp::Ge => ">=",
        CmpOp::Gt => ">",
    }
}

fn render_literal(literal: &Literal) -> String {
    match literal {
        Literal::Bool(value) => value.to_string(),
        Literal::Number(value) => format!("{value}"),
        Literal::Text(value) => format!("{value:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Condition<PostflopVar>, ScriptError> {
        Condition::parse(source, &POSTFLOP)
    }

    fn ctx() -> RuleContext {
        RuleContext {
            aggressions: 0,
            in_position: true,
            spr: 5.0,
            pot: 100.0,
            to_call: 0.0,
            previous_aggressor: PreviousAggressor::None,
            board: BoardFacts::default(),
        }
    }

    #[test]
    fn precedence_matches_or_lowest_and_next_not_tightest() {
        // `false && false || true` must be `(false && false) || true`.
        let condition = parse("aggressions == 1 && aggressions == 2 || unopened").unwrap();
        assert!(condition.eval(&ctx()));
    }

    #[test]
    fn not_binds_tighter_than_and() {
        let condition = parse("!unopened && in_position").unwrap();
        assert!(!condition.eval(&ctx()));
    }

    #[test]
    fn parens_override_precedence() {
        let condition = parse("aggressions == 0 && (position == \"OOP\" || in_position)").unwrap();
        assert!(condition.eval(&ctx()));
    }

    #[test]
    fn bare_word_literal_matches_quoted_string() {
        let mut board_ctx = ctx();
        board_ctx.board = BoardFacts::new(&[
            "Ah".parse().unwrap(),
            "Kd".parse().unwrap(),
            "2c".parse().unwrap(),
        ]);
        assert!(parse("high_card == A").unwrap().eval(&board_ctx));
        assert!(parse("high_card == \"A\"").unwrap().eval(&board_ctx));
    }

    #[test]
    fn unknown_identifier_is_rejected_and_named() {
        for name in [
            "limpers",
            "flats",
            "squeeze",
            "open_cold_calls",
            "preflop_participant",
            "in_position_to_last_aggressor",
        ] {
            let error = parse(name).unwrap_err();
            assert!(error.message.contains(name), "{}", error.message);
        }
    }

    #[test]
    fn number_var_bare_is_rejected() {
        let error = parse("spr").unwrap_err();
        assert!(error.message.contains("spr"));
    }

    #[test]
    fn ordering_on_bool_is_rejected() {
        let error = parse("paired > 1").unwrap_err();
        assert!(error.message.contains("paired"));
    }

    #[test]
    fn ordering_on_text_is_rejected() {
        let error = parse("high_card >= \"A\"").unwrap_err();
        assert!(error.message.contains("high_card"));
    }

    #[test]
    fn bool_compared_to_number_is_rejected() {
        let error = parse("cbet == 3").unwrap_err();
        assert!(error.message.contains("cbet"));
    }

    #[test]
    fn every_error_has_a_line_number() {
        let error = parse("spr").unwrap_err();
        assert_eq!(error.line, 1);
        let source = "aggressions == 0 &&\nspr";
        let error = parse(source).unwrap_err();
        assert_eq!(error.line, 2);
    }

    #[test]
    fn cbet_and_donk_and_facing_pct_at_pot_zero() {
        let mut context = ctx();
        context.pot = 0.0;
        context.to_call = 10.0;
        context.previous_aggressor = PreviousAggressor::Actor;
        assert!(parse("facing_pct == 0").unwrap().eval(&context));
        assert!(parse("cbet").unwrap().eval(&context));
        assert!(!parse("donk").unwrap().eval(&context));

        context.previous_aggressor = PreviousAggressor::Opponent;
        assert!(parse("donk").unwrap().eval(&context));
        assert!(!parse("cbet").unwrap().eval(&context));

        context.previous_aggressor = PreviousAggressor::None;
        assert!(!parse("cbet").unwrap().eval(&context));
        assert!(!parse("donk").unwrap().eval(&context));

        context.aggressions = 1;
        context.previous_aggressor = PreviousAggressor::Actor;
        assert!(!parse("cbet").unwrap().eval(&context));
    }

    #[test]
    fn facing_pct_is_to_call_over_pot() {
        let mut context = ctx();
        context.pot = 200.0;
        context.to_call = 50.0;
        assert!(parse("facing_pct == 25").unwrap().eval(&context));
    }

    /// [`Condition`]'s [`fmt::Display`] impl exists so a family with no
    /// other source text for a rule (a nested `when`/`if` composition, in
    /// multiway's `.mwtree` frontend) can still render one -- and that
    /// rendering must itself be valid input to [`Condition::parse`], or the
    /// round trip a compiled `.mwtree` rule depends on breaks.
    #[test]
    fn display_output_reparses_to_an_equal_condition() {
        for source in [
            "aggressions == 1 && aggressions == 2 || unopened",
            "!unopened && in_position",
            "aggressions == 0 && (position == \"OOP\" || in_position)",
            "high_card == \"A\"",
            "paired && spr <= 3",
        ] {
            let condition = parse(source).unwrap();
            let rendered = condition.to_string();
            let reparsed = parse(&rendered).unwrap_or_else(|error| {
                panic!("{rendered:?} (rendered from {source:?}) failed to reparse: {error}")
            });
            assert_eq!(condition, reparsed, "rendered {rendered:?} from {source:?}");
        }
    }

    #[test]
    fn specialize_folds_board_only_conditions() {
        let unpaired = BoardFacts::new(&[
            "Ah".parse().unwrap(),
            "Kd".parse().unwrap(),
            "2c".parse().unwrap(),
        ]);
        let paired_board = BoardFacts::new(&[
            "Ah".parse().unwrap(),
            "Ad".parse().unwrap(),
            "2c".parse().unwrap(),
        ]);
        let condition = parse("paired && spr <= 3").unwrap();

        let specialized = condition.specialize(&PartialContext {
            board: Some(unpaired),
        });
        assert_eq!(specialized, Condition::Const(false));

        let specialized = condition.specialize(&PartialContext {
            board: Some(paired_board),
        });
        assert_eq!(
            specialized,
            Condition::Compare {
                var: PostflopVar::Spr,
                op: CmpOp::Le,
                value: Literal::Number(3.0),
            }
        );

        let unresolved = condition.specialize(&PartialContext { board: None });
        assert_eq!(unresolved, condition);
    }
}
