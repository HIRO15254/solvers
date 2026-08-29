//! Dialect, variables, and the compiled boolean condition tree the tree
//! script's `when` / `if` conditions lower to.
//!
//! A [`Condition`] is compiled once, at parse time, against a concrete
//! [`Dialect`] -- so a bad identifier or a type mismatch is a
//! [`ScriptError`] the caller sees immediately, and [`Condition::eval`] can
//! be infallible: there is nothing left for it to get wrong at tree-build
//! time, on the tree builder's hot path.

use std::fmt;

use super::ScriptError;
use super::token::{Token, TokenKind, expect_punct, is_punct, is_word, line_at};
use crate::{BoardFacts, SizeUnit, Street, rank_name};

/// The type a [`Var`] reads as, used to type-check conditions at parse time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Bool,
    Number,
    Text,
}

/// One named variable a tree script condition can read. The full set below
/// is `POSTFLOP`'s; a narrower dialect (e.g. a future family) would expose
/// a subset via [`Dialect::vars`], and a name outside that subset is an
/// unknown identifier rather than a silently-false condition -- see
/// `is_reserved` and the parser's identifier lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Var {
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

impl Var {
    /// The identifier a script author types for this variable.
    pub fn name(self) -> &'static str {
        use Var::*;
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

    /// The type this variable's value has, which constrains which
    /// comparisons a condition may use against it.
    pub fn kind(self) -> VarKind {
        use Var::*;
        match self {
            Aggressions | Raises | Players | Spr | Pot | ToCall | FacingPct | BoardCards
            | BoardSuits | BoardRanks | StraightRanks => VarKind::Number,
            Unopened | InPosition | Cbet | Donk | Paired | Monotone | TwoTone | Rainbow
            | FlushPossible | StraightPossible => VarKind::Bool,
            Position | HighCard | LowCard => VarKind::Text,
        }
    }

    /// True for the twelve board-texture variables, i.e. the ones
    /// [`Condition::specialize`] can fold once a board is known.
    fn is_board_var(self) -> bool {
        use Var::*;
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

/// The full set of [`Var`]s, in the order `POSTFLOP` exposes them -- used
/// only to build that dialect table; iterate `Dialect::vars` for anything
/// that should respect a narrower dialect.
const ALL_VARS: &[Var] = &[
    Var::Aggressions,
    Var::Raises,
    Var::Unopened,
    Var::InPosition,
    Var::Position,
    Var::Players,
    Var::Spr,
    Var::Pot,
    Var::ToCall,
    Var::FacingPct,
    Var::Cbet,
    Var::Donk,
    Var::BoardCards,
    Var::BoardSuits,
    Var::BoardRanks,
    Var::StraightRanks,
    Var::Paired,
    Var::Monotone,
    Var::TwoTone,
    Var::Rainbow,
    Var::FlushPossible,
    Var::StraightPossible,
    Var::HighCard,
    Var::LowCard,
];

/// Which names and street vocabulary one config family's tree script
/// exposes. `cards` has no notion of a family (that lives in `holdem` /
/// `multiway`), so a family builds its own `Dialect` naming the [`Var`]s and
/// street keywords it wants; [`POSTFLOP`] is the one the postflop family
/// uses today, exposing every `Var`.
#[derive(Clone, Copy, Debug)]
pub struct Dialect {
    pub vars: &'static [Var],
    pub streets: &'static [(&'static str, Street)],
    pub unit: SizeUnit,
}

/// The postflop tree script's dialect: every [`Var`], the three postflop
/// streets, and chip-denominated sizes.
pub static POSTFLOP: Dialect = Dialect {
    vars: ALL_VARS,
    streets: &[
        ("flop", Street::Flop),
        ("turn", Street::Turn),
        ("river", Street::River),
    ],
    unit: SizeUnit::Chips,
};

/// Reserved names a `param` or `define` may never shadow: every dialect
/// `Var` name, the dialect's street keywords, the size literals with no
/// numeric suffix, the boolean literals, and every script keyword. A
/// substitution must never make a literal or a variable mean something
/// else.
pub(crate) fn is_reserved(name: &str, dialect: &Dialect) -> bool {
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

/// Everything a condition may read about one decision node. Deliberately a
/// concrete struct of primitives, not a trait: it keeps evaluation
/// allocation-free and dispatch-free on the tree builder's hot path.
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

/// A compiled tree-script condition. Every leaf is type-checked against a
/// [`Dialect`] at parse time (see `parse_condition`), so [`Condition::eval`]
/// never fails and never needs to allocate.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    Const(bool),
    Truth(Var),
    Not(Box<Condition>),
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
    Compare { var: Var, op: CmpOp, value: Literal },
    Member { var: Var, values: Vec<Literal> },
}

/// A variable's value at one decision node, resolved just long enough to
/// compare against a [`Literal`]. Not part of the public API: callers only
/// ever see the `bool` [`Condition::eval`] returns.
#[derive(Clone, Copy)]
enum RuntimeValue {
    Bool(bool),
    Number(f64),
    Text(&'static str),
}

impl Condition {
    /// Evaluates this condition against a concrete decision node. Returns
    /// `bool`, not `Result`: every leaf was checked against the dialect at
    /// parse time, so there is nothing left to fail here.
    pub fn eval(&self, ctx: &RuleContext) -> bool {
        match self {
            Condition::Const(value) => *value,
            Condition::Truth(var) => match value_of(*var, ctx) {
                RuntimeValue::Bool(value) => value,
                _ => unreachable!("Truth is only constructed for boolean vars"),
            },
            Condition::Not(inner) => !inner.eval(ctx),
            Condition::And(left, right) => left.eval(ctx) && right.eval(ctx),
            Condition::Or(left, right) => left.eval(ctx) || right.eval(ctx),
            Condition::Compare { var, op, value } => compare(value_of(*var, ctx), *op, value),
            Condition::Member { var, values } => {
                let actual = value_of(*var, ctx);
                values.iter().any(|literal| equals(actual, literal))
            }
        }
    }

    /// Replaces every board-derived variable with a constant once the board
    /// is known, then folds `Not` / `And` / `Or` so a fully-determined
    /// subtree collapses to `Const`. This lets the tree builder specialize
    /// the rule list once per dealt board (a per-chance-node cost) instead
    /// of re-testing board predicates at every decision node under it (a
    /// per-node cost). Not wired into the builder yet.
    pub fn specialize(&self, known: &PartialContext) -> Condition {
        match self {
            Condition::Const(value) => Condition::Const(*value),
            Condition::Truth(var) => {
                if var.is_board_var()
                    && let Some(board) = known.board
                {
                    let RuntimeValue::Bool(value) = board_value(*var, board) else {
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

fn and(left: Condition, right: Condition) -> Condition {
    match (&left, &right) {
        (Condition::Const(false), _) | (_, Condition::Const(false)) => Condition::Const(false),
        (Condition::Const(true), _) => right,
        (_, Condition::Const(true)) => left,
        _ => Condition::And(Box::new(left), Box::new(right)),
    }
}

fn or(left: Condition, right: Condition) -> Condition {
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
pub(crate) fn and_simplify(left: Condition, right: Condition) -> Condition {
    and(left, right)
}

fn value_of(var: Var, ctx: &RuleContext) -> RuntimeValue {
    use Var::*;
    match var {
        Aggressions | Raises => RuntimeValue::Number(f64::from(ctx.aggressions)),
        Unopened => RuntimeValue::Bool(ctx.aggressions == 0),
        InPosition => RuntimeValue::Bool(ctx.in_position),
        Position => RuntimeValue::Text(if ctx.in_position { "IP" } else { "OOP" }),
        Players => RuntimeValue::Number(2.0),
        Spr => RuntimeValue::Number(ctx.spr),
        Pot => RuntimeValue::Number(ctx.pot),
        ToCall => RuntimeValue::Number(ctx.to_call),
        FacingPct => RuntimeValue::Number(if ctx.pot <= 0.0 {
            0.0
        } else {
            ctx.to_call / ctx.pot * 100.0
        }),
        Cbet => RuntimeValue::Bool(
            ctx.aggressions == 0 && matches!(ctx.previous_aggressor, PreviousAggressor::Actor),
        ),
        Donk => RuntimeValue::Bool(
            ctx.aggressions == 0 && matches!(ctx.previous_aggressor, PreviousAggressor::Opponent),
        ),
        BoardCards | BoardSuits | BoardRanks | StraightRanks | Paired | Monotone | TwoTone
        | Rainbow | FlushPossible | StraightPossible | HighCard | LowCard => {
            board_value(var, ctx.board)
        }
    }
}

fn board_value(var: Var, board: BoardFacts) -> RuntimeValue {
    use Var::*;
    match var {
        BoardCards => RuntimeValue::Number(f64::from(board.cards)),
        BoardSuits => RuntimeValue::Number(f64::from(board.suits)),
        BoardRanks => RuntimeValue::Number(f64::from(board.ranks)),
        StraightRanks => RuntimeValue::Number(f64::from(board.straight_ranks)),
        Paired => RuntimeValue::Bool(board.paired()),
        Monotone => RuntimeValue::Bool(board.monotone()),
        TwoTone => RuntimeValue::Bool(board.two_tone()),
        Rainbow => RuntimeValue::Bool(board.rainbow()),
        FlushPossible => RuntimeValue::Bool(board.flush_possible()),
        StraightPossible => RuntimeValue::Bool(board.straight_possible()),
        HighCard => RuntimeValue::Text(rank_name(board.high_rank)),
        LowCard => RuntimeValue::Text(rank_name(board.low_rank)),
        _ => unreachable!("board_value called with a non-board var"),
    }
}

fn compare(actual: RuntimeValue, op: CmpOp, literal: &Literal) -> bool {
    match (actual, literal) {
        (RuntimeValue::Bool(actual), Literal::Bool(expected)) => match op {
            CmpOp::Eq => actual == *expected,
            CmpOp::Ne => actual != *expected,
            _ => unreachable!("booleans only type-check against == and !="),
        },
        (RuntimeValue::Number(actual), Literal::Number(expected)) => match op {
            CmpOp::Lt => actual < *expected,
            CmpOp::Le => actual <= *expected,
            CmpOp::Eq => actual == *expected,
            CmpOp::Ne => actual != *expected,
            CmpOp::Ge => actual >= *expected,
            CmpOp::Gt => actual > *expected,
        },
        (RuntimeValue::Text(actual), Literal::Text(expected)) => match op {
            CmpOp::Eq => actual == expected.as_str(),
            CmpOp::Ne => actual != expected.as_str(),
            _ => unreachable!("text only type-checks against == and !="),
        },
        _ => unreachable!("type mismatch should have been rejected at parse time"),
    }
}

fn equals(actual: RuntimeValue, literal: &Literal) -> bool {
    match (actual, literal) {
        (RuntimeValue::Bool(actual), Literal::Bool(expected)) => actual == *expected,
        (RuntimeValue::Number(actual), Literal::Number(expected)) => actual == *expected,
        (RuntimeValue::Text(actual), Literal::Text(expected)) => actual == expected.as_str(),
        _ => unreachable!("type mismatch should have been rejected at parse time"),
    }
}

// ---- condition grammar: `||` < `&&` < unary `!` < primary ----------------

/// Parses one condition starting at `*pos`, type-checking every leaf
/// against `dialect`. Stops as soon as the grammar can extend no further
/// (in particular, `{` is never part of a condition), so the caller can
/// parse `when <condition> {` by calling this and then expecting `{`.
pub(crate) fn parse_condition(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect,
) -> Result<Condition, ScriptError> {
    parse_or(tokens, pos, dialect)
}

fn parse_or(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect,
) -> Result<Condition, ScriptError> {
    let mut left = parse_and(tokens, pos, dialect)?;
    while is_punct(tokens, *pos, "||") {
        *pos += 1;
        let right = parse_and(tokens, pos, dialect)?;
        left = Condition::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect,
) -> Result<Condition, ScriptError> {
    let mut left = parse_unary(tokens, pos, dialect)?;
    while is_punct(tokens, *pos, "&&") {
        *pos += 1;
        let right = parse_unary(tokens, pos, dialect)?;
        left = Condition::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_unary(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect,
) -> Result<Condition, ScriptError> {
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

fn parse_predicate(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect,
) -> Result<Condition, ScriptError> {
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

fn parse_membership(
    tokens: &[Token],
    pos: &mut usize,
    var: Var,
    var_line: usize,
) -> Result<Condition, ScriptError> {
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

fn check_compare_types(
    var: Var,
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

fn check_member_type(var: Var, literal: &Literal, value_line: usize) -> Result<(), ScriptError> {
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

impl fmt::Display for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::super::token::tokenize;
    use super::*;

    fn parse(source: &str) -> Result<Condition, ScriptError> {
        let tokens = tokenize(source).unwrap();
        let significant: Vec<Token> = tokens
            .into_iter()
            .filter(|t| !matches!(t.kind, TokenKind::Comment(_)))
            .collect();
        let mut pos = 0;
        let condition = parse_condition(&significant, &mut pos, &POSTFLOP)?;
        if pos != significant.len() {
            return Err(ScriptError {
                line: line_at(&significant, pos),
                message: "trailing tokens".to_string(),
            });
        }
        Ok(condition)
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
                var: Var::Spr,
                op: CmpOp::Le,
                value: Literal::Number(3.0),
            }
        );

        let unresolved = condition.specialize(&PartialContext { board: None });
        assert_eq!(unresolved, condition);
    }
}
