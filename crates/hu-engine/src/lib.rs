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

mod mccfr;
mod reach;
mod schedule;
mod scratch;
mod solver;
mod storage;
mod tree;

pub use mccfr::{McCfg, McSolver, McSolverState};
pub use reach::{
    MismatchReason, SubtreeMismatch, pair_subtrees, parent_array, path_from_root, reach_at,
};
pub use schedule::{CfrPlus, Dcfr, DiscountSchedule, Discounts, HsDcfr, Vanilla, linear_cfr};
pub use scratch::Scratch;
pub use solver::{CompiledGame, ParConfig, Solver, SolverState, TerminalEvaluator};
pub use storage::{
    F32Storage, F32View, I16Storage, I16View, MixedStorage, MixedView, StateMismatch, Storage,
    StorageArrays, StorageArraysMut, StorageOps, StorageRef, StorageSpan, StorageState,
    StorageView,
};
pub use tree::{
    Deal, Node, NodeId, NodeKind, PublicTree, ReachMap, SparseTransition, TempNode, TreeSpec,
};

/// Arithmetic precision of CFR terminals and current-strategy regret matching.
/// Evaluation and average strategy retain their f64 arithmetic in either mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum CfrPrecision {
    #[default]
    F64,
    F32,
}
impl CfrPrecision {
    pub fn name(self) -> &'static str {
        match self {
            Self::F64 => "f64",
            Self::F32 => "f32",
        }
    }
}
