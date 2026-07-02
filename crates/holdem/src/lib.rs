//! Mode A: exact postflop HU-NLHE solving. No card abstraction — full
//! 1,326-combo ranges with card-removal handled exactly.
//!
//! Multi-street slice: subgames can start on the flop, turn, or river. The
//! terminal kernels in [`kernel`] (sorted-rank O(n+m) showdown sweep, O(n)
//! inclusion-exclusion fold kernel) are shared by every showdown/fold
//! terminal in the tree; [`postflop`] adds chance nodes between streets
//! with suit-isomorphism merging on top. [`river`] is a thin shim over
//! [`postflop`] kept for its narrower, river-only config shape.

mod kernel;
mod postflop;
mod river;

pub use postflop::{
    MemoryEstimate, PerStreet, PostflopConfig, PostflopEvaluator, PostflopGame, PostflopNodeInfo,
    build_postflop_game, memory_usage,
};
pub use river::{RiverConfig, RiverGame, RiverNodeInfo, build_river_game};
