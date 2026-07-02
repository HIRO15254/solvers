//! Hot solver core: public game tree, regret/strategy storage, discount
//! schedules, vector-form CFR, and accelerated best response.
//!
//! The engine knows nothing about poker. It walks a prebuilt [`PublicTree`]
//! carrying per-hand `f32` reach vectors and returning per-hand
//! counterfactual-value vectors. Everything variant-specific arrives through
//! the tree structure, the [`TerminalEvaluator`] implementation, and the
//! reach masks / transitions baked in at build time.
//!
//! The engine is intentionally heads-up only (`PerPlayer<T>` is a pair);
//! generalizing to N players is an explicit non-goal.

mod schedule;
mod scratch;
mod solver;
mod storage;
mod tree;

pub use schedule::{CfrPlus, Dcfr, DiscountSchedule, Discounts, HsDcfr, Vanilla, linear_cfr};
pub use scratch::Scratch;
pub use solver::{CompiledGame, ParConfig, Solver, TerminalEvaluator};
pub use storage::{F32Storage, F32View, Storage, StorageOps, StorageRef, StorageSpan, StorageView};
pub use tree::{
    Deal, Node, NodeId, NodeKind, PublicTree, ReachMap, SparseTransition, TempNode, TreeSpec,
};
