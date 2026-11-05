//! Sampled public boards and bucket strategies for two-player flop leaves.
mod cards;
mod eval;
mod own;
mod pass;
mod qre;
mod refine;
mod solve;
mod spec;
mod state;
mod tree;

pub use cards::{Board, BucketSource, FixedFlops, Sampling, WeightedFlopSubset};
pub use eval::{
    Evaluation, LeafDiagnostic, LeafOverride, LeafStrategyDump, LeafStrategyNode, SeatEvaluation,
    evaluate, evaluate_with_overrides, leaf_diagnostic, leaf_informed_best_response, leaf_strategy,
};
pub use own::{FlopFeatures, OWN_FLOP_GROUPS, leaf_weights, own_flop_groups};
pub use pass::{LeafStrategy, Strategies};
pub use qre::Qre;
pub use refine::{
    FlopClass, FlopClusters, HandSplit, Refined, Refinement, Relative, RelativeReach, Runout,
    canonical_flop, parse_streets,
};
pub use solve::{
    Checkpoint, LateBoards, Options, Progress, Solution, Timings, evaluation_boards, solve,
    solve_postflop_fixed, solve_postflop_fixed_with_state, solve_with_state,
};
pub use spec::{
    CardSpec, LeafCardSpec, LeafCards, LeafFlopGroups, LowSprCards, LowSprLevels, LowSprSpec,
    RECOMMENDED_FLOP_GROUPS, leaf_of_line, parse_leaf_flop_groups, parse_low_spr_levels,
};
pub use state::{FixedState, State, read_state};
pub use tree::{Node, Subtree};

#[cfg(test)]
mod tests;
