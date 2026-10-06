//! Mode A: exact postflop HU-NLHE solving. No card abstraction — full
//! seat-specific range supports with card-removal handled exactly.
//!
//! Multi-street slice: subgames can start on the flop, turn, or river. The
//! terminal kernels in [`kernel`] (sorted-rank O(n+m) showdown sweep, O(n)
//! inclusion-exclusion fold kernel) are shared by every showdown/fold
//! terminal in the tree; [`postflop`] adds chance nodes between streets
//! with suit-isomorphism merging on top. [`river`] is a thin shim over
//! [`postflop`] kept for its narrower, river-only config shape.
//!
//! [`aggregate`] and [`equity`] are reporting helpers: mapping combos to
//! their 13x13 preflop class and computing range-vs-range showdown equity
//! averaged over remaining runouts, respectively. Neither sits on the solve
//! path. [`viewer`] is the read-only query layer for an already-built tree:
//! street tagging, history replay, and river-subgame reconstruction for a
//! reach-weighted re-solve — see its module doc for the design contract.

pub mod game;
pub mod input;

mod aggregate;
mod equity;
mod hands;
pub use hands::PostflopHands;
mod kernel;
mod postflop;
mod river;
mod viewer;

pub use aggregate::{class_average, class_of_combo, class_weights};
pub use equity::range_equity;
pub use postflop::{
    MemoryEstimate, PerStreet, PostflopConfig, PostflopEvaluator, PostflopGame, PostflopNodeInfo,
    RuleHits, StreetTree, TreeBuildError, build_postflop_game, memory_usage,
    try_build_postflop_game, try_memory_usage,
};
pub use river::{RiverConfig, RiverGame, RiverNodeInfo, build_river_game};
pub use viewer::{
    ReplayError, RiverEntryState, node_streets, river_entry_state, river_resolve_config,
};

pub mod checkpoint;
pub mod sol;

pub mod artifact;
pub mod prepare;
pub mod queries;
pub mod report;
pub mod run;
pub mod views;
pub use hu_engine::{CompiledGame, NodeId, NodeKind, PublicTree, SolverState};
pub use nlh::betting::Action as BettingAction;
pub use nlh::{Card, PerPlayer, Player, Street};
