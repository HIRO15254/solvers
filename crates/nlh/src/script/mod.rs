//! Postflop tree-script front end: `.tree` source text in, a compiled
//! [`Script`] (a `param` schema plus a flat [`Rule`] list) out.
//!
//! This is the frontend only -- tokenizing, substitution, the condition
//! and block grammar, type-checking, and lowering to rules. It has no
//! knowledge of a betting tree; `crates/hu-postflop`'s tree builder is the
//! consumer that walks decision nodes and replays [`Rule::condition`]
//! against a [`RuleContext`] it fills in per node.
//!
//! ```text
//! .tree source ──tokenize──► tokens ──substitute──► tokens ──parse──► AST ──lower──► Vec<Rule>
//! ```
//!
//! Public syntax and vocabulary are defined in `docs/nlh-input-v1.jp.md`
//! section 9. `spot` owns the common-input dialect; product tree builders
//! evaluate its compiled conditions. The internal PostflopVar dialect is
//! retained for HU game construction and independent oracle tests.
//! Parsing produces a typed [`Condition`] tree once; [`Condition::eval`]
//! never re-parses or fails.

mod ast;
mod cond;
mod lower;
mod parse;
mod token;

pub use ast::{ActionKind, Effect, ParamKind, ParamSchema, Rule, Script};
pub use cond::{
    CmpOp, Condition, Dialect, Literal, POSTFLOP, PartialContext, PostflopVar, PreviousAggressor,
    RuleContext, SizeParser, Value, VarKind, VarSource, Vars,
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
