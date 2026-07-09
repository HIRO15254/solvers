//! Exact 169x169 preflop all-in equity, computed by full enumeration.
//!
//! For every ordered class pair `(h, o)` the table stores the probability
//! that a hand of class `h` beats (or ties) a hand of class `o` when the
//! remaining five board cards run out, averaged uniformly over all
//! card-disjoint combo pairs of the two classes and all
//! `C(48, 5) = 1,712,304` boards disjoint from both.
//!
//! # Algorithm
//!
//! Enumeration is over the 134,459 canonical (suit-isomorphic) 5-card
//! boards with their multiplicities — a 19.3x reduction over the raw
//! `C(52, 5) = 2,598,960`. Per board:
//!
//! 1. rank every combo that doesn't collide with the board
//!    (`cards::rank_of`, 7 cards);
//! 2. sort combos by rank ascending and sweep equal-rank groups;
//! 3. for each hero combo `c = (c1, c2)` of class `h`, count opponent
//!    combos of each class `o` that are strictly lower and card-disjoint
//!    via inclusion-exclusion on prefix tallies:
//!    `lower[o] - lower_card[c1][o] - lower_card[c2][o]` (no third term: a
//!    distinct combo can't share both cards with `c`); ties are counted by
//!    scanning the hero's own rank group directly (`+1` correction for the
//!    hero itself, which shares both cards);
//! 4. accumulate counts weighted by the board's multiplicity into per-class
//!    win/tie totals (u64 — exact integers throughout).
//!
//! Exactness invariant (tested): for every ordered pair,
//! `win(h, o) + tie(h, o) + win(o, h)` in raw counts equals
//! `N(h, o) * 1,712,304` where `N` is [`crate::compat_counts`].
//!
//! The enumeration parallelizes over boards with rayon and takes seconds in
//! release mode; [`EquityTable::load_or_compute`] adds an optional disk
//! cache (postcard, magic + version header) so repeated CLI runs skip it.

use std::path::Path;

use cards::NUM_CLASSES;

/// Boards disjoint from a fixed disjoint combo pair: `C(48, 5)`.
pub const EQUITY_BOARDS_PER_PAIR: u64 = 1_712_304;

/// Exact preflop all-in equity between the 169 hand classes.
///
/// `win[h * 169 + o]` is the probability that class `h` beats class `o`
/// (conditioned on a card-disjoint deal), `tie` likewise; `lose(h, o)`
/// is `win(o, h)` by symmetry and is not stored.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EquityTable {
    win: Vec<f64>,
    tie: Vec<f64>,
}

/// Errors from the disk cache codec.
#[derive(Debug, thiserror::Error)]
pub enum EquityCacheError {
    #[error("bad magic bytes (not an equity cache file)")]
    BadMagic,
    #[error("unsupported cache version {found} (expected {expected})")]
    BadVersion { found: u16, expected: u16 },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("codec error: {0}")]
    Codec(#[from] postcard::Error),
}

impl EquityTable {
    /// Builds a table directly from probability vectors (row-major
    /// `h * 169 + o`, both of length `169 * 169`). Primarily for tests
    /// that need synthetic tables; panics on wrong lengths.
    pub fn from_probabilities(win: Vec<f64>, tie: Vec<f64>) -> Self {
        assert_eq!(win.len(), NUM_CLASSES * NUM_CLASSES);
        assert_eq!(tie.len(), NUM_CLASSES * NUM_CLASSES);
        EquityTable { win, tie }
    }

    /// P(class `h` beats class `o`).
    pub fn win(&self, h: usize, o: usize) -> f64 {
        self.win[h * NUM_CLASSES + o]
    }

    /// P(class `h` ties class `o`). Symmetric.
    pub fn tie(&self, h: usize, o: usize) -> f64 {
        self.tie[h * NUM_CLASSES + o]
    }

    /// P(class `h` loses to class `o`) — `win(o, h)`.
    pub fn lose(&self, h: usize, o: usize) -> f64 {
        self.win(o, h)
    }

    /// Full exact enumeration (rayon-parallel over canonical boards).
    pub fn compute() -> Self {
        todo!("implemented by equity/classes agent")
    }

    /// Reads a cached table, verifying magic and version.
    pub fn load(_path: &Path) -> Result<Self, EquityCacheError> {
        todo!("implemented by equity/classes agent")
    }

    /// Writes the table (atomically: temp file + rename).
    pub fn save(&self, _path: &Path) -> Result<(), EquityCacheError> {
        todo!("implemented by equity/classes agent")
    }

    /// Loads from `cache` when present and valid; otherwise computes and
    /// best-effort saves (a failed save is not an error).
    pub fn load_or_compute(_cache: Option<&Path>) -> Self {
        todo!("implemented by equity/classes agent")
    }
}
