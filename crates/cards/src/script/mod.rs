//! Postflop tree-script front end: `.tree` source text in, a compiled
//! [`Script`] (a `param` schema plus a flat [`Rule`] list) out.
//!
//! This is the frontend only -- tokenizing, substitution, the condition
//! and block grammar, type-checking, and lowering to rules. It has no
//! knowledge of a betting tree; `crates/holdem`'s tree builder is the
//! consumer that walks decision nodes and replays [`Rule::condition`]
//! against a [`RuleContext`] it fills in per node.
//!
//! ```text
//! .tree source ──tokenize──► tokens ──substitute──► tokens ──parse──► AST ──lower──► Vec<Rule>
//! ```
//!
//! See `docs/postflop-tree-script-v1.jp.md` for the normative grammar this
//! module implements: 適用モデル, 条件式, 盤面述語, script frontend, error,
//! 具体例. The grammar is the same one
//! `crates/multiway/src/tree_rules.rs` evaluates today, ported to a
//! compile-once/evaluate-many shape: multiway re-parses its condition
//! string at every node, which is fine for a small preflop tree but not for
//! a postflop tree with hundreds of thousands of decision nodes, so here
//! parsing produces a typed [`Condition`] tree once and [`Condition::eval`]
//! never re-parses or fails.

mod ast;
mod cond;
mod lower;
mod parse;
mod token;

pub use ast::{ActionKind, Effect, ParamKind, ParamSchema, Rule, Script};
pub use cond::{
    CmpOp, Condition, Dialect, Literal, POSTFLOP, PartialContext, PreviousAggressor, RuleContext,
    Var, VarKind,
};

/// An error compiling a tree script. Every error -- tokenizing, `param` /
/// `define` resolution, or the block/condition/size grammar -- carries the
/// 1-based source line it was detected on.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ScriptError {
    pub line: usize,
    pub message: String,
}
