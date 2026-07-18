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

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use cards::{Card, HandRank, NUM_CLASSES, NUM_COMBOS, combo_cards, rank_of};
#[cfg(test)]
use hand_index::SuitPerm;
use hand_index::{all_suit_perms, permute_card};
use rayon::prelude::*;

use crate::classes::{class_of_combo, compat_counts};

/// Boards disjoint from a fixed disjoint combo pair: `C(48, 5)`.
pub const EQUITY_BOARDS_PER_PAIR: u64 = 1_712_304;

const CACHE_MAGIC: &[u8; 8] = b"SLVRPFEQ";
const CACHE_VERSION: u16 = 1;
const CACHE_HEADER_LEN: usize = 8 + 2;

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
    #[error(
        "cache payload has wrong table length (win {win_len}, tie {tie_len}), expected {expected}"
    )]
    BadLength {
        win_len: usize,
        tie_len: usize,
        expected: usize,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("codec error: {0}")]
    Codec(#[from] postcard::Error),
}

/// Reusable per-board scratch buffers, kept out of the rayon hot loop.
///
/// `lower_card` is heap-backed (flat `Vec`, indexed `card * NUM_CLASSES +
/// class`) rather than a `[[u32; 169]; 52]` field: that fixed-size array is
/// 35KB, and giving it stack storage blows the (comparatively small) rayon
/// worker thread stacks once `accumulate_board` gets inlined into the
/// recursive `join`-based work-stealing split/reduce tree.
pub(crate) struct Scratch {
    /// (rank, combo) of every combo not colliding with the current board.
    live: Vec<(HandRank, usize)>,
    /// Count of already-swept combos per class.
    lower: [u32; NUM_CLASSES],
    /// Count of already-swept combos per class that contain a given card,
    /// flat `[card * NUM_CLASSES + class]`.
    lower_card: Vec<u32>,
}

impl Scratch {
    pub(crate) fn new() -> Self {
        Scratch {
            live: Vec::with_capacity(1_081),
            lower: [0; NUM_CLASSES],
            lower_card: vec![0u32; 52 * NUM_CLASSES],
        }
    }
}

/// The canonical (suit-minimal) representative of an unordered 5-card set:
/// the min, over all 24 suit permutations, of the sorted permuted card
/// indices. Distinct from `hand_index::canonicalize_board`, which is
/// street-structured (flop vs later streets); here the board is a single
/// unordered 5-set.
pub(crate) fn canonical_key(cards: [Card; 5]) -> [u8; 5] {
    let perms = all_suit_perms();
    let mut best = [u8::MAX; 5];
    for perm in &perms {
        let mut mapped = [0u8; 5];
        for (i, &c) in cards.iter().enumerate() {
            mapped[i] = permute_card(perm, c).index() as u8;
        }
        mapped.sort_unstable();
        if mapped < best {
            best = mapped;
        }
    }
    best
}

/// Applies a suit permutation to a 5-card board (used by tests).
#[cfg(test)]
fn permute_board(perm: &SuitPerm, board: [Card; 5]) -> [Card; 5] {
    let mut out = board;
    for c in out.iter_mut() {
        *c = permute_card(perm, *c);
    }
    out
}

/// All 134,459 canonical 5-card boards with their raw multiplicities
/// (summing to `C(52, 5) = 2,598,960`).
pub(crate) fn canonical_boards() -> Vec<([Card; 5], u32)> {
    let all: Vec<Card> = cards::ALL_CARDS.into_iter().collect();
    let mut counts: HashMap<[u8; 5], u32> = HashMap::with_capacity(140_000);
    for i in 0..52 {
        for j in (i + 1)..52 {
            for k in (j + 1)..52 {
                for l in (k + 1)..52 {
                    for m in (l + 1)..52 {
                        let board = [all[i], all[j], all[k], all[l], all[m]];
                        let key = canonical_key(board);
                        *counts.entry(key).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    counts
        .into_iter()
        .map(|(key, weight)| {
            let board = [
                Card::from_index(key[0]),
                Card::from_index(key[1]),
                Card::from_index(key[2]),
                Card::from_index(key[3]),
                Card::from_index(key[4]),
            ];
            (board, weight)
        })
        .collect()
}

/// Sweeps a single board, accumulating weighted win/tie counts into
/// `win_acc`/`tie_acc` (row-major `h * NUM_CLASSES + o`, length
/// `NUM_CLASSES * NUM_CLASSES`). Factored out of [`compute_counts`] so it is
/// unit-testable against a brute-force double loop on a fixed board.
pub(crate) fn accumulate_board(
    board: [Card; 5],
    weight: u64,
    win_acc: &mut [u64],
    tie_acc: &mut [u64],
    scratch: &mut Scratch,
) {
    let board_set: cards::CardSet = board.into_iter().collect();

    scratch.live.clear();
    for combo in 0..NUM_COMBOS {
        let (c1, c2) = combo_cards(combo);
        if board_set.contains(c1) || board_set.contains(c2) {
            continue;
        }
        let rank = rank_of(board.into_iter().chain([c1, c2]));
        scratch.live.push((rank, combo));
    }
    scratch.live.sort_unstable_by_key(|&(rank, _)| rank);

    scratch.lower = [0; NUM_CLASSES];
    scratch.lower_card.iter_mut().for_each(|x| *x = 0);

    let n = scratch.live.len();
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n && scratch.live[j].0 == scratch.live[i].0 {
            j += 1;
        }
        let group = &scratch.live[i..j];

        // Before folding the group into the running tallies: wins against
        // strictly-lower-ranked (already-swept) opponent combos, and ties
        // against the group's other members.
        for &(_, combo) in group {
            let (c1, c2) = combo_cards(combo);
            let h = class_of_combo(combo);
            for o in 0..NUM_CLASSES {
                let win_pairs = scratch.lower[o] as i64
                    - scratch.lower_card[c1.index() * NUM_CLASSES + o] as i64
                    - scratch.lower_card[c2.index() * NUM_CLASSES + o] as i64;
                debug_assert!(win_pairs >= 0, "win_pairs underflow");
                win_acc[h * NUM_CLASSES + o] += weight * win_pairs as u64;
            }
            for &(_, other) in group {
                if other == combo {
                    continue;
                }
                let (d1, d2) = combo_cards(other);
                if d1 != c1 && d1 != c2 && d2 != c1 && d2 != c2 {
                    let o = class_of_combo(other);
                    tie_acc[h * NUM_CLASSES + o] += weight;
                }
            }
        }

        // Now fold the group into lower/lower_card for subsequent groups.
        for &(_, combo) in group {
            let h = class_of_combo(combo);
            let (c1, c2) = combo_cards(combo);
            scratch.lower[h] += 1;
            scratch.lower_card[c1.index() * NUM_CLASSES + h] += 1;
            scratch.lower_card[c2.index() * NUM_CLASSES + h] += 1;
        }

        i = j;
    }
}

/// Brute-force reference used only by tests: an `O(live^2)` double loop
/// over live combo pairs, no inclusion-exclusion.
#[cfg(test)]
fn accumulate_board_brute_force(
    board: [Card; 5],
    weight: u64,
    win_acc: &mut [u64],
    tie_acc: &mut [u64],
) {
    let board_set: cards::CardSet = board.into_iter().collect();
    let live: Vec<usize> = (0..NUM_COMBOS)
        .filter(|&combo| {
            let (c1, c2) = combo_cards(combo);
            !board_set.contains(c1) && !board_set.contains(c2)
        })
        .collect();
    let ranks: Vec<HandRank> = live
        .iter()
        .map(|&combo| {
            let (c1, c2) = combo_cards(combo);
            rank_of(board.into_iter().chain([c1, c2]))
        })
        .collect();
    for (ix, &c) in live.iter().enumerate() {
        let (c1, c2) = combo_cards(c);
        let h = class_of_combo(c);
        for (iy, &d) in live.iter().enumerate() {
            if c == d {
                continue;
            }
            let (d1, d2) = combo_cards(d);
            if d1 == c1 || d1 == c2 || d2 == c1 || d2 == c2 {
                continue; // not card-disjoint
            }
            let o = class_of_combo(d);
            match ranks[ix].cmp(&ranks[iy]) {
                std::cmp::Ordering::Greater => win_acc[h * NUM_CLASSES + o] += weight,
                std::cmp::Ordering::Equal => tie_acc[h * NUM_CLASSES + o] += weight,
                std::cmp::Ordering::Less => {}
            }
        }
    }
}

/// Raw weighted win/tie counts (row-major `h * NUM_CLASSES + o`), summed
/// over all canonical boards. Split out from [`EquityTable::compute`] so
/// tests can check exact integer invariants before normalization.
pub(crate) fn compute_counts() -> (Vec<u64>, Vec<u64>) {
    let boards = canonical_boards();
    let zeros = || vec![0u64; NUM_CLASSES * NUM_CLASSES];
    boards
        .par_iter()
        .fold(
            || (zeros(), zeros(), Scratch::new()),
            |(mut win, mut tie, mut scratch), &(board, weight)| {
                accumulate_board(board, weight as u64, &mut win, &mut tie, &mut scratch);
                (win, tie, scratch)
            },
        )
        .map(|(win, tie, _scratch)| (win, tie))
        .reduce(
            || (zeros(), zeros()),
            |mut a, b| {
                for i in 0..a.0.len() {
                    a.0[i] += b.0[i];
                    a.1[i] += b.1[i];
                }
                a
            },
        )
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
        let (win_counts, tie_counts) = compute_counts();
        Self::from_counts(&win_counts, &tie_counts)
    }

    /// Normalizes raw weighted counts (from [`compute_counts`]) into
    /// probabilities. Split out so tests can normalize the same counts they
    /// checked exactness invariants on, instead of paying for a second
    /// `compute_counts` pass.
    pub(crate) fn from_counts(win_counts: &[u64], tie_counts: &[u64]) -> Self {
        let n = compat_counts();
        let mut win = vec![0f64; NUM_CLASSES * NUM_CLASSES];
        let mut tie = vec![0f64; NUM_CLASSES * NUM_CLASSES];
        for idx in 0..NUM_CLASSES * NUM_CLASSES {
            let denom = n[idx] as f64 * EQUITY_BOARDS_PER_PAIR as f64;
            win[idx] = win_counts[idx] as f64 / denom;
            tie[idx] = tie_counts[idx] as f64 / denom;
        }
        EquityTable { win, tie }
    }

    /// Reads a cached table, verifying magic, version, and vector lengths.
    pub fn load(path: &Path) -> Result<Self, EquityCacheError> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < CACHE_HEADER_LEN || &bytes[0..8] != CACHE_MAGIC {
            return Err(EquityCacheError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != CACHE_VERSION {
            return Err(EquityCacheError::BadVersion {
                found: version,
                expected: CACHE_VERSION,
            });
        }
        let table: EquityTable = postcard::from_bytes(&bytes[CACHE_HEADER_LEN..])?;
        let expected = NUM_CLASSES * NUM_CLASSES;
        if table.win.len() != expected || table.tie.len() != expected {
            return Err(EquityCacheError::BadLength {
                win_len: table.win.len(),
                tie_len: table.tie.len(),
                expected,
            });
        }
        Ok(table)
    }

    /// Writes the table (atomically: temp file + rename).
    pub fn save(&self, path: &Path) -> Result<(), EquityCacheError> {
        let payload = postcard::to_allocvec(self)?;
        let mut buf = Vec::with_capacity(CACHE_HEADER_LEN + payload.len());
        buf.extend_from_slice(CACHE_MAGIC);
        buf.extend_from_slice(&CACHE_VERSION.to_le_bytes());
        buf.extend_from_slice(&payload);

        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let tmp_name = format!(
            ".{}.tmp",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("equity.postcard")
        );
        let tmp_path = dir.join(tmp_name);
        {
            let mut file = std::fs::File::create(&tmp_path)?;
            file.write_all(&buf)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Loads from `cache` when present and valid; otherwise computes and
    /// best-effort saves (a failed save is not an error).
    pub fn load_or_compute(cache: Option<&Path>) -> Self {
        if let Some(path) = cache
            && let Ok(table) = Self::load(path)
        {
            return table;
        }
        let table = Self::compute();
        if let Some(path) = cache {
            let _ = table.save(path);
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::seq::SliceRandom;
    use rand_chacha::ChaCha8Rng;

    fn board(s: &str) -> [Card; 5] {
        let cards: Vec<Card> = s.split_whitespace().map(|c| c.parse().unwrap()).collect();
        cards.try_into().unwrap()
    }

    fn diff_against_brute_force(b: [Card; 5]) {
        let zeros = || vec![0u64; NUM_CLASSES * NUM_CLASSES];
        let (mut win_fast, mut tie_fast) = (zeros(), zeros());
        let mut scratch = Scratch::new();
        accumulate_board(b, 1, &mut win_fast, &mut tie_fast, &mut scratch);

        let (mut win_brute, mut tie_brute) = (zeros(), zeros());
        accumulate_board_brute_force(b, 1, &mut win_brute, &mut tie_brute);

        assert_eq!(win_fast, win_brute, "win mismatch for board {b:?}");
        assert_eq!(tie_fast, tie_brute, "tie mismatch for board {b:?}");
    }

    #[test]
    fn single_board_matches_brute_force() {
        diff_against_brute_force(board("2c 7d Jh Js Kd"));
    }

    #[test]
    fn paired_board_matches_brute_force_with_big_tie_groups() {
        diff_against_brute_force(board("2c 2d 2h 7c 7h"));
    }

    #[test]
    fn canonical_key_is_suit_perm_invariant() {
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let boards = [
            board("2c 7d Jh Js Kd"),
            board("2c 2d 2h 7c 7h"),
            board("As Ks Qs Js Ts"),
            board("2c 3d 4h 5s 6c"),
        ];
        for b in boards {
            let key = canonical_key(b);
            for _ in 0..10 {
                let mut perm: SuitPerm = [0, 1, 2, 3];
                perm.shuffle(&mut rng);
                let permuted = permute_board(&perm, b);
                assert_eq!(
                    canonical_key(permuted),
                    key,
                    "canonical key changed under suit perm {perm:?} for board {b:?}"
                );
            }
            // The key itself must be one of the 24 permuted forms.
            let found = all_suit_perms().iter().any(|perm| {
                let mut mapped: Vec<u8> = permute_board(perm, b)
                    .iter()
                    .map(|c| c.index() as u8)
                    .collect();
                mapped.sort_unstable();
                mapped == key
            });
            assert!(found, "canonical key is not a permuted form of {b:?}");
        }
    }

    #[test]
    #[ignore = "full 2.6M-board enumeration; CI runs it in release with --include-ignored"]
    fn full_table_invariants_and_anchors() {
        // Canonical board count/multiplicity check first: cheap relative to
        // the sweep below (no HandRank evaluation), so this doesn't
        // meaningfully affect the reported compute() time.
        let boards = canonical_boards();
        assert_eq!(boards.len(), 134_459, "canonical board count");
        assert_eq!(
            boards.iter().map(|&(_, w)| w as u64).sum::<u64>(),
            2_598_960,
            "canonical board multiplicities"
        );

        // Compute the raw counts exactly once (the expensive part) and
        // derive both the invariant checks and the normalized table from
        // them, rather than recomputing.
        let start = std::time::Instant::now();
        let (win_counts, tie_counts) = compute_counts();
        eprintln!("EquityTable::compute() took {:?}", start.elapsed());
        let table = EquityTable::from_counts(&win_counts, &tie_counts);

        let n = compat_counts();

        for h in 0..NUM_CLASSES {
            for o in 0..NUM_CLASSES {
                let ho = h * NUM_CLASSES + o;
                let oh = o * NUM_CLASSES + h;
                assert_eq!(
                    win_counts[ho] + tie_counts[ho] + win_counts[oh],
                    n[ho] as u64 * EQUITY_BOARDS_PER_PAIR,
                    "exactness invariant failed for (h={h}, o={o})"
                );
                assert_eq!(
                    tie_counts[ho], tie_counts[oh],
                    "tie symmetry failed for (h={h}, o={o})"
                );
            }
        }

        // Class indices per cards::class_index's 13x13 grid (see equity.rs
        // module docs / classes::class_label goldens): AA=0, AKs=1, KK=14,
        // QQ=28.
        let aa = 0;
        let kk = 14;
        let qq = 28;
        let aks = 1;
        let seven_two_o = cards::class_index(5, 0, false);
        let win_aa_kk = table.win(aa, kk);
        assert!(
            (0.80..0.83).contains(&win_aa_kk),
            "AA vs KK win = {win_aa_kk}"
        );
        let win_aa_72o = table.win(aa, seven_two_o);
        assert!(
            (0.86..0.90).contains(&win_aa_72o),
            "AA vs 72o win = {win_aa_72o}"
        );
        let win_aks_qq = table.win(aks, qq);
        assert!(
            (0.44..0.48).contains(&win_aks_qq),
            "AKs vs QQ win = {win_aks_qq}"
        );

        // Save + load round-trip.
        let path = std::env::temp_dir().join(format!(
            "preflop-equity-test-{}-{}.postcard",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        table.save(&path).unwrap();
        let loaded = EquityTable::load(&path).unwrap();
        assert!(loaded == table);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_bad_magic() {
        let path = std::env::temp_dir().join(format!(
            "preflop-equity-badmagic-{}.postcard",
            std::process::id()
        ));
        std::fs::write(&path, b"NOTMAGIC").unwrap();
        assert!(matches!(
            EquityTable::load(&path),
            Err(EquityCacheError::BadMagic)
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_bad_version() {
        let path = std::env::temp_dir().join(format!(
            "preflop-equity-badversion-{}.postcard",
            std::process::id()
        ));
        let mut buf = Vec::new();
        buf.extend_from_slice(CACHE_MAGIC);
        buf.extend_from_slice(&99u16.to_le_bytes());
        std::fs::write(&path, &buf).unwrap();
        match EquityTable::load(&path) {
            Err(EquityCacheError::BadVersion { found, expected }) => {
                assert_eq!(found, 99);
                assert_eq!(expected, CACHE_VERSION);
            }
            Ok(_) => panic!("expected BadVersion, got Ok"),
            Err(other) => panic!("expected BadVersion, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_or_compute_without_cache_computes_directly() {
        // Small smoke test (not the full table): from_probabilities table
        // saved/loaded through load_or_compute's cache path.
        let win = vec![0.5f64; NUM_CLASSES * NUM_CLASSES];
        let tie = vec![0.1f64; NUM_CLASSES * NUM_CLASSES];
        let table = EquityTable::from_probabilities(win, tie);
        let path = std::env::temp_dir().join(format!(
            "preflop-equity-loadorcompute-{}.postcard",
            std::process::id()
        ));
        table.save(&path).unwrap();
        let loaded = EquityTable::load_or_compute(Some(&path));
        assert!(loaded == table);
        let _ = std::fs::remove_file(&path);
    }
}
