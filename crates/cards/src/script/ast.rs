//! The tree script's output types (`Rule`, `Script`, ...), plus the private
//! parse tree [`parse`](super::parse) builds and [`lower`](super::lower)
//! flattens into a `Vec<Rule>`.
//!
//! Splitting "what a script compiles to" (this file) from "how the source
//! text gets there" (`parse.rs`, `lower.rs`) keeps the public shape in one
//! place: `Rule`, `Script`, and friends are read here without wading
//! through tokenizing or substitution.

use super::cond::Condition;
use crate::{SizeSpec, Street};

/// Which kind of aggressive action a rule's `add` / `remove` / `replace` /
/// `force` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Bet,
    Raise,
}

/// How a rule changes the action list already built for a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Adds the given sizes as candidates.
    Add,
    /// Removes every candidate of `action`'s kind.
    Remove,
    /// Removes every candidate of `action`'s kind, then adds the given
    /// sizes.
    Replace,
    /// Replaces the whole action list with just the given sizes -- fold and
    /// check are removed too.
    Force,
    /// Removes every action except check. Takes neither `action` nor
    /// `sizes`.
    Checkdown,
}

/// One flattened tree-script rule: applied to the base action list at every
/// decision node on `street` whose `condition` evaluates true. Rules from
/// one compiled [`Script`] apply in source order -- see the module's
/// "flattening" doc on [`super::lower`] -- so this carries no separate
/// priority field.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub street: Street,
    pub condition: Condition,
    pub effect: Effect,
    /// `None` only for `Effect::Checkdown`.
    pub action: Option<ActionKind>,
    pub sizes: Vec<SizeSpec>,
}

/// The type a `param`'s effective value was inferred to hold, from the
/// literal token as written (or as overridden).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    Number,
    Bool,
    Token,
}

/// One `param` a script declares, describing the variable it exposes to a
/// GUI or `validate` diagnostic -- see the module docs on `param` schemas.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSchema {
    pub name: String,
    pub kind: ParamKind,
    /// The literal token as written / overridden, kept verbatim so a caller
    /// can render it back without reformatting.
    pub default: String,
    pub description: Option<String>,
}

/// A compiled tree script: the `param` schema it exposes, and the flat rule
/// list the tree builder replays at every decision node.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub params: Vec<ParamSchema>,
    pub rules: Vec<Rule>,
}

// ---- private parse tree, produced by `parse.rs` and consumed by `lower.rs` ----

/// One statement or nested block inside a street block's body, with its
/// condition (if any) already parsed and type-checked -- lowering only
/// needs to combine conditions and assign streets, not parse anything.
#[derive(Clone, Debug)]
pub(crate) enum StmtAst {
    /// A bare statement: `<effect> <action> [sizes...]` or `checkdown`.
    Action {
        effect: Effect,
        action: Option<ActionKind>,
        sizes: Vec<SizeSpec>,
    },
    /// `when <condition> { <body> }`.
    When {
        condition: Condition,
        body: Vec<StmtAst>,
    },
    /// `if <cond> { } else if <cond> { } else { }`, with `else` optional.
    If {
        arms: Vec<(Condition, Vec<StmtAst>)>,
        else_body: Option<Vec<StmtAst>>,
    },
}

/// One `<street list> [when <condition>] { <body> }` block, with its street
/// list resolved to [`Street`]s and its shorthand `when` (if any) already
/// parsed.
#[derive(Clone, Debug)]
pub(crate) struct StreetBlockAst {
    pub streets: Vec<Street>,
    pub condition: Option<Condition>,
    pub body: Vec<StmtAst>,
}
