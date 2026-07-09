//! Blueprint artifacts: aggregated bucket transitions and bucket-vs-bucket
//! river equity, built from an [`Ehs2Abstraction`] by exact enumeration.
//!
//! These are the data a bucketed Mode B game feeds into
//! `engine::SparseTransition` chance nodes and its showdown evaluator (see
//! `docs/blueprint-design.md`). Everything here is engine-agnostic raw
//! triples/matrices — the `preflop` crate converts them.
//!
//! Measure convention: `class_to_flop` rows sum to the class's full-range
//! average compat mass kappa(h) (hand-vs-hand blockers exist only preflop),
//! `flop_to_turn`/`turn_to_river` rows sum to 1, and `river_equity` is a
//! probability split per ordered bucket pair (`win + tie + win-transposed
//! == 1`). Board-vs-hand removal is exact everywhere (conflicting combos
//! are never counted); hand-vs-hand removal inside the bucketed streets is
//! deliberately dropped — the standard blueprint approximation.

use std::path::Path;

use crate::buckets::Ehs2Abstraction;

/// Sparse row-major transition: `entries` are `(in_index, out_index,
/// weight)` triples, at most one per pair, sorted by `(in, out)`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TransitionTable {
    pub in_dim: u32,
    pub out_dim: u32,
    pub entries: Vec<(u32, u32, f32)>,
}

/// Dense bucket-vs-bucket river equity, row-major `hero * dim + opp`:
/// `win[i]` is P(hero bucket beats opp bucket) conditioned on a card-legal
/// pairing, `tie` likewise (symmetric).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BucketEquity {
    pub dim: u32,
    pub win: Vec<f64>,
    pub tie: Vec<f64>,
}

/// Everything a bucketed blueprint game needs beyond the abstraction
/// itself. See the module docs for the exact semantics of each table.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlueprintArtifacts {
    /// 169 preflop classes -> flop buckets; rows scaled by kappa(h).
    pub class_to_flop: TransitionTable,
    /// Flop buckets -> turn buckets (row-stochastic).
    pub flop_to_turn: TransitionTable,
    /// Turn buckets -> river buckets (row-stochastic).
    pub turn_to_river: TransitionTable,
    /// River bucket-vs-bucket showdown probabilities.
    pub river_equity: BucketEquity,
}

/// Errors from the artifact disk-cache codec.
#[derive(Debug, thiserror::Error)]
pub enum BlueprintCacheError {
    #[error("bad magic bytes (not a blueprint artifact file)")]
    BadMagic,
    #[error("unsupported cache version {found} (expected {expected})")]
    BadVersion { found: u16, expected: u16 },
    #[error("cache was built with different abstraction params")]
    ParamsMismatch,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("codec error: {0}")]
    Codec(#[from] postcard::Error),
}

impl BlueprintArtifacts {
    /// Builds all four tables by exact enumeration (rayon-parallel; minutes
    /// in release for full streets). The abstraction must have been built
    /// for flop, turn, and river.
    pub fn build(_abs: &Ehs2Abstraction) -> Self {
        todo!("implemented by blueprint agent")
    }

    /// Reads a cached artifact set, verifying magic/version and that it was
    /// built for the same abstraction params.
    pub fn load(_path: &Path, _abs: &Ehs2Abstraction) -> Result<Self, BlueprintCacheError> {
        todo!("implemented by blueprint agent")
    }

    /// Writes atomically (temp file + rename).
    pub fn save(&self, _path: &Path) -> Result<(), BlueprintCacheError> {
        todo!("implemented by blueprint agent")
    }

    /// Loads from `cache` when present and valid; otherwise builds and
    /// best-effort saves.
    pub fn load_or_build(_abs: &Ehs2Abstraction, _cache: Option<&Path>) -> Self {
        todo!("implemented by blueprint agent")
    }
}
