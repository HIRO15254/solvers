//! Card abstraction: build-time bucketing of (board, hole-combo) situations.
//!
//! Buckets compress the postflop private-state space so a Mode B blueprint
//! game stays tabular (`docs/architecture.md` §6). Everything in this crate
//! runs at tree-build time only — the engine's hot loop sees buckets purely
//! as reach-vector dimensions.
//!
//! This slice ships the E[HS²] percentile baseline
//! ([`Ehs2Abstraction`]): per street, every live (canonical board, combo)
//! situation is scored by the expectation of its squared river hand
//! strength — squaring rewards hands whose strength distribution has
//! variance (draws) over made-but-static hands of equal mean — and bucketed
//! into equal-mass percentile bins. Preflop is never abstracted (the 169
//! classes are lossless there); rivers use plain HS (squaring a
//! deterministic value changes nothing but keeps the scale uniform).
//!
//! Distribution-aware abstractions (IR-KE-KO: k-means + EMD histograms,
//! OCHS rivers) land in M7 behind the same [`CardAbstraction`] trait, as
//! does the Waugh perfect-hash index for compact cache keys — until then,
//! canonical boards (suit-isomorphism representatives) key everything.

mod blueprint;
mod buckets;
mod ehs;

pub use blueprint::{BlueprintArtifacts, BlueprintCacheError, BucketEquity, TransitionTable};
pub use buckets::{BucketCacheError, CACHE_FORMAT_VERSION, Ehs2Abstraction, Ehs2Params};
pub use ehs::{ehs2, hand_strength};

use cards::{Card, Street};

/// Build-time mapping from a concrete (board, hole-combo) situation to its
/// abstract bucket.
///
/// Implementations canonicalize internally: callers pass real cards, and
/// two situations that are suit-isomorphic must land in the same bucket.
pub trait CardAbstraction: Send + Sync {
    /// Number of buckets on `street` (`Street::Preflop` is not abstracted
    /// and must not be queried).
    fn num_buckets(&self, street: Street) -> u32;

    /// Bucket of `combo` (a `cards::combo_index`) on `board` (3, 4, or 5
    /// cards — the length selects the street). Panics if the combo shares a
    /// card with the board or the street was not built.
    fn bucket(&self, board: &[Card], combo: usize) -> u32;
}
