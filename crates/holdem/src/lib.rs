//! Mode A: exact postflop HU-NLHE solving. No card abstraction — full
//! 1,326-combo ranges with card-removal handled exactly.
//!
//! Current slice: river-only subgames. The terminal kernels here (sorted-
//! rank O(n+m) showdown sweep, O(n) inclusion-exclusion fold kernel) are
//! the exact algorithms the full flop solver reuses per canonical river;
//! turn/flop trees add chance nodes with suit-isomorphism merging on top.

mod river;

pub use river::{RiverConfig, RiverEvaluator, RiverGame, RiverNodeInfo, build_river_game};
