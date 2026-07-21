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
//!
//! Enumeration measures: T1 runs over the 1,755 canonical flops, T2 over
//! the 63,193 canonical turns, T3 over the *lazy product* (canonical turn x
//! 48 river cards) — the same joint counts as enumerating street-structured
//! canonical rivers (each raw river appears exactly once, weighted by its
//! turn's multiplicity) without materializing that ~2.17M-board list — and
//! `river_equity` over the 134,459 canonical unordered 5-card sets (bucket
//! assignment on a complete board is split-invariant; the river street
//! table is keyed by that same quotient, see `buckets.rs`).
//!
//! NOTE: CI never runs the full-street [`BlueprintArtifacts::build`] — the
//! turn-street EHS² scoring behind it alone takes minutes and the full
//! tables run to hundreds of MB, past the CI budget. The ignored release
//! test builds abstraction + artifacts from a moderate board *subset*
//! instead and checks every invariant there; the full build is the
//! documented user-invoked [`BlueprintArtifacts::load_or_build`] path.
//!
//! `kappa(h)` is defined as the full-range-weighted average compat mass
//! (see [`kappa_per_class`]). Deviation from the original design sketch,
//! confirmed by direct enumeration (`kappa_is_a_universal_constant` below):
//! this quantity is *exactly* `1225 / 1326` for every one of the 169
//! classes, not smallest for the most-overlapping hands as one might guess.
//! This falls out of deck combinatorics: any single fixed two-card combo is
//! disjoint from exactly `C(50, 2) = 1225` of the other 1325 combos,
//! independent of its own ranks/suits, so averaging `compat(h, o)` over the
//! *entire* full-range opponent mass (weighted by combo count, i.e.
//! uniformly over all 1326 combos) reproduces that same constant for every
//! `h`. The card-removal effect is real for any *specific* opponent class
//! `o` but washes out exactly when averaged over the full range. kappa is
//! kept as a computed (not hard-coded) quantity because the derivation is
//! easy to get subtly wrong and the design explicitly asks for it to be
//! derived from the `N(h, o)` table rather than assumed.

use std::io::Write;
use std::path::Path;

use cards::{
    ALL_CARDS, Card, CardSet, HandRank, NUM_CLASSES, NUM_COMBOS, class_index, combo_cards, rank_of,
};
use hand_index::{
    Board, SuitPerm, all_suit_perms, canonical_flops, canonicalize_board, permute_combo,
};
use rayon::prelude::*;

use cards::Street;

use crate::CardAbstraction;
use crate::buckets::{Ehs2Abstraction, Ehs2Params, canonical_river_sets, canonical_turns};

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

// --- 169-class combinatorics (reimplemented locally: `abstraction` cannot
// depend on `preflop`, which owns the canonical versions of these — see
// `preflop::classes`) --------------------------------------------------

/// Class of a combo: 3-line reimplementation of `preflop::classes::class_of_combo`.
fn class_of_combo(combo: usize) -> usize {
    let (hi, lo) = combo_cards(combo);
    class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit())
}

/// Number of combos in each of the 169 classes (6 pairs, 4 suited, 12
/// offsuit). Reimplementation of `preflop::classes::class_combo_counts`.
fn class_combo_counts() -> [u32; NUM_CLASSES] {
    let mut counts = [0u32; NUM_CLASSES];
    for combo in 0..NUM_COMBOS {
        counts[class_of_combo(combo)] += 1;
    }
    counts
}

/// `N(h, o)`: number of ordered card-disjoint combo pairs per class pair,
/// row-major `h * NUM_CLASSES + o`. `O(NUM_COMBOS^2)` reimplementation of
/// `preflop::classes::compat_counts`.
fn compat_counts() -> Vec<u32> {
    let classes: Vec<usize> = (0..NUM_COMBOS).map(class_of_combo).collect();
    let cardsets: Vec<CardSet> = (0..NUM_COMBOS)
        .map(|combo| {
            let (a, b) = combo_cards(combo);
            [a, b].into_iter().collect()
        })
        .collect();
    let mut counts = vec![0u32; NUM_CLASSES * NUM_CLASSES];
    for c_h in 0..NUM_COMBOS {
        for c_o in 0..NUM_COMBOS {
            if c_h != c_o && cardsets[c_h].is_disjoint(cardsets[c_o]) {
                counts[classes[c_h] * NUM_CLASSES + classes[c_o]] += 1;
            }
        }
    }
    counts
}

/// `kappa(h) = Sum_o N(h, o) / (n_h * 1326)` — the full-range-weighted
/// average compat mass a combo of class `h` retains against a uniformly
/// random other combo. See the module docs for why this is a constant.
fn kappa_per_class() -> [f64; NUM_CLASSES] {
    let n = compat_counts();
    let counts = class_combo_counts();
    let mut kappa = [0f64; NUM_CLASSES];
    for h in 0..NUM_CLASSES {
        let sum_n: u64 = (0..NUM_CLASSES)
            .map(|o| n[h * NUM_CLASSES + o] as u64)
            .sum();
        kappa[h] = sum_n as f64 / (counts[h] as f64 * 1326.0);
    }
    kappa
}

// --- hot-loop lookup tables ---------------------------------------------

/// Precomputed combo-index images of all 24 suit permutations, flat
/// `[perm_index * NUM_COMBOS + combo]`. The T2/T3/river-equity inner loops
/// map billions of combo indices through suit permutations; calling
/// `permute_combo` there (two `combo_cards` decodes + re-encode per call)
/// dominates the runtime, while this table turns each mapping into one
/// array read. 24 x 1,326 u16 = 64KB, built once per counts pass.
struct ComboPermTables {
    perms: [SuitPerm; 24],
    tables: Vec<u16>,
}

impl ComboPermTables {
    fn new() -> Self {
        let perms = all_suit_perms();
        let mut tables = vec![0u16; 24 * NUM_COMBOS];
        for (pi, perm) in perms.iter().enumerate() {
            for combo in 0..NUM_COMBOS {
                tables[pi * NUM_COMBOS + combo] = permute_combo(perm, combo) as u16;
            }
        }
        ComboPermTables { perms, tables }
    }

    /// The combo -> permuted-combo map of `perm`, indexed by combo.
    fn map(&self, perm: &SuitPerm) -> &[u16] {
        let pi = self
            .perms
            .iter()
            .position(|p| p == perm)
            .expect("not one of the 24 suit permutations");
        &self.tables[pi * NUM_COMBOS..(pi + 1) * NUM_COMBOS]
    }
}

/// Every combo's two cards as a `CardSet`, indexed by combo — one bitwise
/// AND per liveness check instead of a `combo_cards` decode.
fn combo_card_sets() -> Vec<CardSet> {
    (0..NUM_COMBOS)
        .map(|combo| {
            let (a, b) = combo_cards(combo);
            [a, b].into_iter().collect()
        })
        .collect()
}

// --- dense counts -> sparse TransitionTable -----------------------------

/// Converts row-major raw weighted counts into a [`TransitionTable`],
/// normalizing each nonzero row to sum to `row_target(row)` (kappa(h) for
/// T1, 1.0 for T2/T3). Zero-count entries are omitted; entries come out
/// already sorted by `(in, out)` since rows/columns are visited ascending.
fn counts_to_table(
    counts: &[u64],
    in_dim: usize,
    out_dim: usize,
    row_target: impl Fn(usize) -> f64,
) -> TransitionTable {
    let mut entries = Vec::new();
    for i in 0..in_dim {
        let row = &counts[i * out_dim..(i + 1) * out_dim];
        let row_sum: u64 = row.iter().sum();
        if row_sum == 0 {
            continue;
        }
        let target = row_target(i);
        for (o, &c) in row.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let weight = target * c as f64 / row_sum as f64;
            entries.push((i as u32, o as u32, weight as f32));
        }
    }
    TransitionTable {
        in_dim: in_dim as u32,
        out_dim: out_dim as u32,
        entries,
    }
}

// --- T1: class_to_flop ---------------------------------------------------

/// Raw weighted `(class, flop bucket)` counts (row-major `h * kf + b`) plus
/// `kf`. `flops` must already be canonical (as from `canonical_flops`) —
/// each is looked up directly, no re-canonicalization needed since T1 has
/// only one street.
fn class_to_flop_counts(abs: &Ehs2Abstraction, flops: &[(Board, u32)]) -> (Vec<u64>, usize) {
    let kf = abs.num_buckets(Street::Flop) as usize;
    let raw = flops
        .par_iter()
        .fold(
            || vec![0u64; NUM_CLASSES * kf],
            |mut acc, (flop_board, weight)| {
                let board_set: CardSet = flop_board.cards().collect();
                let row = abs.flop_row(flop_board);
                for (combo, &b) in row.iter().enumerate() {
                    let (c1, c2) = combo_cards(combo);
                    if board_set.contains(c1) || board_set.contains(c2) {
                        continue;
                    }
                    debug_assert!(b != u16::MAX, "live combo must have a bucket");
                    let h = class_of_combo(combo);
                    acc[h * kf + b as usize] += *weight as u64;
                }
                acc
            },
        )
        .reduce(
            || vec![0u64; NUM_CLASSES * kf],
            |mut a, b| {
                for i in 0..a.len() {
                    a[i] += b[i];
                }
                a
            },
        );
    (raw, kf)
}

fn class_to_flop_table(abs: &Ehs2Abstraction, flops: &[(Board, u32)]) -> TransitionTable {
    let (raw, kf) = class_to_flop_counts(abs, flops);
    let kappa = kappa_per_class();
    counts_to_table(&raw, NUM_CLASSES, kf, |h| kappa[h])
}

// --- T2: flop_to_turn ----------------------------------------------------

/// Canonicalizes `turn_board`'s 3-card flop prefix — NOT necessarily
/// canonical on its own even though `turn_board` is canonical as a whole —
/// and returns the flop bucket row plus the suit permutation mapping
/// literal combos on `turn_board` into that row's combo-index frame. This
/// is the "compose permutations" subtlety the PERFORMANCE section calls
/// out: the full board needs no permutation (it's already the table key),
/// but its prefix generally does.
fn flop_row_for_turn_prefix<'a>(
    abs: &'a Ehs2Abstraction,
    turn_board: &Board,
) -> (&'a [u16], SuitPerm) {
    let prefix = Board::new(&turn_board.flop, &[]);
    let (canon, perm) = canonicalize_board(&prefix);
    (abs.flop_row(&canon), perm)
}

/// Raw weighted `(flop bucket, turn bucket)` joint counts (row-major
/// `b_f * kt + b_t`) plus `(kf, kt)`. `turns` must already be canonical (as
/// from `canonical_turns`).
fn flop_to_turn_counts(abs: &Ehs2Abstraction, turns: &[(Board, u32)]) -> (Vec<u64>, usize, usize) {
    let kf = abs.num_buckets(Street::Flop) as usize;
    let kt = abs.num_buckets(Street::Turn) as usize;
    let perm_tables = ComboPermTables::new();
    let combo_sets = combo_card_sets();
    let counts = turns
        .par_iter()
        .fold(
            || vec![0u64; kf * kt],
            |mut acc, (turn_board, weight)| {
                let board_set: CardSet = turn_board.cards().collect();
                // Full board is already canonical: direct row, no permute.
                let turn_row = abs.turn_row(turn_board);
                let (flop_row, perm) = flop_row_for_turn_prefix(abs, turn_board);
                let pmap = perm_tables.map(&perm);
                for (combo, &b_t) in turn_row.iter().enumerate() {
                    if !board_set.is_disjoint(combo_sets[combo]) {
                        continue;
                    }
                    debug_assert!(b_t != u16::MAX);
                    let b_f = flop_row[pmap[combo] as usize];
                    debug_assert!(b_f != u16::MAX);
                    acc[b_f as usize * kt + b_t as usize] += *weight as u64;
                }
                acc
            },
        )
        .reduce(
            || vec![0u64; kf * kt],
            |mut a, b| {
                for i in 0..a.len() {
                    a[i] += b[i];
                }
                a
            },
        );
    (counts, kf, kt)
}

fn flop_to_turn_table(abs: &Ehs2Abstraction, turns: &[(Board, u32)]) -> TransitionTable {
    let (counts, kf, kt) = flop_to_turn_counts(abs, turns);
    counts_to_table(&counts, kf, kt, |_| 1.0)
}

// --- T3: turn_to_river ---------------------------------------------------

/// Raw weighted `(turn bucket, river bucket)` joint counts (row-major
/// `b_t * kr + b_r`) plus `(kt, kr)`. `turns` must already be canonical (as
/// from `canonical_turns`).
///
/// Enumerates the lazy product (canonical turn x 48 live river cards)
/// instead of materialized canonical river boards: extending each canonical
/// turn representative by every live card, weighted by the turn's own
/// multiplicity, covers each raw river exactly once by the same orbit
/// argument `canonical_turns` itself rests on — without the ~2.17M-entry
/// street-structured river board list (see the module docs). The turn row
/// needs no permutation (the turn board IS the table key); the river row is
/// fetched through the unordered-set quotient, whose canonicalizing
/// permutation maps combo indices into that row's frame.
fn turn_to_river_counts(abs: &Ehs2Abstraction, turns: &[(Board, u32)]) -> (Vec<u64>, usize, usize) {
    let kt = abs.num_buckets(Street::Turn) as usize;
    let kr = abs.num_buckets(Street::River) as usize;
    let perm_tables = ComboPermTables::new();
    let combo_sets = combo_card_sets();
    let counts = turns
        .par_iter()
        .fold(
            || vec![0u64; kt * kr],
            |mut acc, (turn_board, weight)| {
                let turn_set: CardSet = turn_board.cards().collect();
                let turn_row = abs.turn_row(turn_board);
                let four: Vec<Card> = turn_board.cards().collect();
                for r in ALL_CARDS {
                    if turn_set.contains(r) {
                        continue;
                    }
                    let five = [four[0], four[1], four[2], four[3], r];
                    let mut board_set = turn_set;
                    board_set.insert(r);
                    let (river_row, perm) = abs.river_row_for_set(five);
                    let pmap = perm_tables.map(&perm);
                    for (combo, &b_t) in turn_row.iter().enumerate() {
                        if !board_set.is_disjoint(combo_sets[combo]) {
                            continue; // dead on the turn or holding the river card
                        }
                        debug_assert!(b_t != u16::MAX);
                        let b_r = river_row[pmap[combo] as usize];
                        debug_assert!(b_r != u16::MAX);
                        acc[b_t as usize * kr + b_r as usize] += *weight as u64;
                    }
                }
                acc
            },
        )
        .reduce(
            || vec![0u64; kt * kr],
            |mut a, b| {
                for i in 0..a.len() {
                    a[i] += b[i];
                }
                a
            },
        );
    (counts, kt, kr)
}

fn turn_to_river_table(abs: &Ehs2Abstraction, turns: &[(Board, u32)]) -> TransitionTable {
    let (counts, kt, kr) = turn_to_river_counts(abs, turns);
    counts_to_table(&counts, kt, kr, |_| 1.0)
}

// --- river_equity: bucket-vs-bucket showdown ----------------------------

/// Reusable per-board scratch for [`accumulate_river_board`], sized to the
/// river bucket count once and reused across boards via rayon's
/// `map_init`/`fold` (mirrors `preflop::equity::Scratch`, generalized from
/// a fixed `NUM_CLASSES` to a runtime bucket count).
struct RiverScratch {
    /// `(rank, combo, bucket)` for every combo not touching the board.
    live: Vec<(HandRank, usize, u16)>,
    /// Total live combos per bucket on the current board.
    live_cnt: Vec<u32>,
    /// Total live combos per bucket containing a given card, flat
    /// `[card * kr + bucket]`.
    live_card: Vec<u32>,
    /// Running count of already-swept (strictly lower-ranked) live combos
    /// per bucket.
    lower: Vec<u32>,
    /// Running count of already-swept live combos per bucket containing a
    /// given card, flat `[card * kr + bucket]`.
    lower_card: Vec<u32>,
}

impl RiverScratch {
    fn new(kr: usize) -> Self {
        RiverScratch {
            live: Vec::with_capacity(1_100),
            live_cnt: vec![0; kr],
            live_card: vec![0; 52 * kr],
            lower: vec![0; kr],
            lower_card: vec![0; 52 * kr],
        }
    }
}

/// Accumulates one board's contribution into `win_acc`/`tie_acc`/`pair_acc`
/// (row-major `bucket_hero * kr + bucket_opp`, length `kr * kr`).
///
/// Mirrors `preflop::equity::accumulate_board`'s sorted-rank sweep with
/// `lower`/`lower_card` inclusion-exclusion, aggregated by bucket instead of
/// class, plus an extra `pair_cnt` pass: unlike class membership (fixed,
/// board-independent, so `preflop::equity` normalizes once via a global
/// `N(h, o)` table), bucket membership is board-dependent, so the
/// card-disjoint pairing count per ordered bucket pair has to be
/// accumulated per board here.
///
/// `pair_cnt[bc][bo] = live[bo] - live_card[c1][bo] - live_card[c2][bo] +
/// [bo == bc]`: inclusion-exclusion over "combos containing c1 or c2"
/// double-subtracts hero `c` itself exactly when `bo == bc` (the only combo
/// containing *both* c1 and c2 is `c`), so the indicator adds it back —
/// mirrors the `+1` self/tie correction in `preflop::equity`'s tie count,
/// applied here to the disjoint-pairing total rather than a tie group.
#[allow(clippy::too_many_arguments)] // mirrors preflop::equity::accumulate_board plus
// the extra `kr`/`pair_acc` a board-dependent bucket space needs; splitting
// the three (kr-sized) accumulators + scratch into a struct would obscure
// more than it clarifies here.
fn accumulate_river_board(
    abs: &Ehs2Abstraction,
    perm_tables: &ComboPermTables,
    board: [Card; 5],
    weight: u64,
    kr: usize,
    win_acc: &mut [u64],
    tie_acc: &mut [u64],
    pair_acc: &mut [u64],
    scratch: &mut RiverScratch,
) {
    let board_set: CardSet = board.into_iter().collect();
    let (river_row, perm) = abs.river_row_for_set(board);
    let pmap = perm_tables.map(&perm);

    scratch.live.clear();
    for combo in 0..NUM_COMBOS {
        let (c1, c2) = combo_cards(combo);
        if board_set.contains(c1) || board_set.contains(c2) {
            continue;
        }
        let bucket = river_row[pmap[combo] as usize];
        debug_assert!(bucket != u16::MAX);
        let rank = rank_of(board.into_iter().chain([c1, c2]));
        scratch.live.push((rank, combo, bucket));
    }
    scratch.live.sort_unstable_by_key(|&(r, _, _)| r);

    // Pass 1: board-wide per-bucket / per-card-per-bucket totals, for the
    // pair_cnt normalization (rank-order-independent).
    scratch.live_cnt[..kr].fill(0);
    scratch.live_card[..52 * kr].fill(0);
    for &(_, combo, bucket) in &scratch.live {
        let (c1, c2) = combo_cards(combo);
        scratch.live_cnt[bucket as usize] += 1;
        scratch.live_card[c1.index() * kr + bucket as usize] += 1;
        scratch.live_card[c2.index() * kr + bucket as usize] += 1;
    }
    for &(_, combo, bucket_c) in &scratch.live {
        let (c1, c2) = combo_cards(combo);
        let bc = bucket_c as usize;
        for bo in 0..kr {
            let mut pairs = scratch.live_cnt[bo] as i64
                - scratch.live_card[c1.index() * kr + bo] as i64
                - scratch.live_card[c2.index() * kr + bo] as i64;
            if bo == bc {
                pairs += 1;
            }
            debug_assert!(pairs >= 0, "pair_cnt underflow");
            pair_acc[bc * kr + bo] += weight * pairs as u64;
        }
    }

    // Pass 2: sorted-rank sweep for win/tie, same inclusion-exclusion shape
    // as preflop::equity::accumulate_board.
    scratch.lower[..kr].fill(0);
    scratch.lower_card[..52 * kr].fill(0);
    let n = scratch.live.len();
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n && scratch.live[j].0 == scratch.live[i].0 {
            j += 1;
        }
        let group = &scratch.live[i..j];

        for &(_, combo, bucket_c) in group {
            let (c1, c2) = combo_cards(combo);
            let bc = bucket_c as usize;
            for bo in 0..kr {
                let win_pairs = scratch.lower[bo] as i64
                    - scratch.lower_card[c1.index() * kr + bo] as i64
                    - scratch.lower_card[c2.index() * kr + bo] as i64;
                debug_assert!(win_pairs >= 0, "win_pairs underflow");
                win_acc[bc * kr + bo] += weight * win_pairs as u64;
            }
            for &(_, other, bucket_o) in group {
                if other == combo {
                    continue;
                }
                let (d1, d2) = combo_cards(other);
                if d1 != c1 && d1 != c2 && d2 != c1 && d2 != c2 {
                    tie_acc[bc * kr + bucket_o as usize] += weight;
                }
            }
        }

        for &(_, combo, bucket_c) in group {
            let (c1, c2) = combo_cards(combo);
            let bc = bucket_c as usize;
            scratch.lower[bc] += 1;
            scratch.lower_card[c1.index() * kr + bc] += 1;
            scratch.lower_card[c2.index() * kr + bc] += 1;
        }

        i = j;
    }
}

/// Raw weighted win/tie/pair counts over `boards` (each an unordered 5-card
/// set with a multiplicity weight), row-major `bucket_hero * kr +
/// bucket_opp`, plus `kr`. Split out from [`river_equity_over`] so tests can
/// check the exactness invariant on integer counts before normalization.
fn river_equity_counts(
    abs: &Ehs2Abstraction,
    boards: &[([Card; 5], u64)],
) -> (Vec<u64>, Vec<u64>, Vec<u64>, usize) {
    let kr = abs.num_buckets(Street::River) as usize;
    let perm_tables = ComboPermTables::new();
    let zeros = || vec![0u64; kr * kr];
    let (win, tie, pair) = boards
        .par_iter()
        .fold(
            || (zeros(), zeros(), zeros(), RiverScratch::new(kr)),
            |(mut win, mut tie, mut pair, mut scratch), &(board, weight)| {
                accumulate_river_board(
                    abs,
                    &perm_tables,
                    board,
                    weight,
                    kr,
                    &mut win,
                    &mut tie,
                    &mut pair,
                    &mut scratch,
                );
                (win, tie, pair, scratch)
            },
        )
        .map(|(win, tie, pair, _scratch)| (win, tie, pair))
        .reduce(
            || (zeros(), zeros(), zeros()),
            |mut a, b| {
                for i in 0..a.0.len() {
                    a.0[i] += b.0[i];
                    a.1[i] += b.1[i];
                    a.2[i] += b.2[i];
                }
                a
            },
        );
    (win, tie, pair, kr)
}

/// Normalizes raw counts into probabilities: `win/pair`, `tie/pair`, `0`
/// where `pair == 0` (no observed co-occurrence for that bucket pair among
/// the enumerated boards — documented in the module docs rather than
/// asserted, since a reduced test abstraction may legitimately not cover
/// every bucket pair).
fn river_equity_from_counts(win: &[u64], tie: &[u64], pair: &[u64], kr: usize) -> BucketEquity {
    let mut win_p = vec![0f64; kr * kr];
    let mut tie_p = vec![0f64; kr * kr];
    for idx in 0..kr * kr {
        if pair[idx] > 0 {
            win_p[idx] = win[idx] as f64 / pair[idx] as f64;
            tie_p[idx] = tie[idx] as f64 / pair[idx] as f64;
        }
    }
    BucketEquity {
        dim: kr as u32,
        win: win_p,
        tie: tie_p,
    }
}

fn river_equity_over(abs: &Ehs2Abstraction, boards: &[([Card; 5], u64)]) -> BucketEquity {
    let (win, tie, pair, kr) = river_equity_counts(abs, boards);
    river_equity_from_counts(&win, &tie, &pair, kr)
}

// --- cache codec ----------------------------------------------------------

const CACHE_MAGIC: &[u8; 8] = b"SLVRBLUP";
const CACHE_VERSION: u16 = 1;
const CACHE_HEADER_LEN: usize = 8 + 2;

/// `Ehs2Params` implied by `abs` (via the always-available `num_buckets`
/// accessor, which reads straight from params regardless of which streets
/// were actually built).
fn abs_params(abs: &Ehs2Abstraction) -> Ehs2Params {
    Ehs2Params {
        flop_buckets: abs.num_buckets(Street::Flop),
        turn_buckets: abs.num_buckets(Street::Turn),
        river_buckets: abs.num_buckets(Street::River),
    }
}

/// `Ehs2Params` implied by `artifacts`' own table dimensions — `save` has no
/// `&Ehs2Abstraction` to read params from (frozen signature), but the
/// dimensions it built from are exactly those params by construction.
fn artifacts_params(artifacts: &BlueprintArtifacts) -> Ehs2Params {
    Ehs2Params {
        flop_buckets: artifacts.class_to_flop.out_dim,
        turn_buckets: artifacts.flop_to_turn.out_dim,
        river_buckets: artifacts.turn_to_river.out_dim,
    }
}

impl BlueprintArtifacts {
    /// Builds all four tables by exact enumeration (rayon-parallel; minutes
    /// in release for full streets — see the module docs' CI note). The
    /// abstraction must have been built for flop, turn, and river.
    pub fn build(abs: &Ehs2Abstraction) -> Self {
        let flops = canonical_flops();
        let turns = canonical_turns();
        let river5: Vec<([Card; 5], u64)> = canonical_river_sets()
            .into_iter()
            .map(|(board, w)| (board, w as u64))
            .collect();
        Self::build_over(abs, &flops, &turns, &river5)
    }

    /// Builds artifacts over caller-supplied board lists instead of the
    /// full canonical enumerations: T1 over `flops`, T2 *and* T3 over
    /// `turns` (T3 extends each by all 48 live river cards), river equity
    /// over `river5`. The abstraction must cover every board involved —
    /// including every river completion of every entry in `turns`.
    /// [`Self::build`] passes the full streets; tests pass a subset
    /// matching an `Ehs2Abstraction::build_for_boards` abstraction, so the
    /// test suite doesn't pay full-street enumeration cost.
    pub(crate) fn build_over(
        abs: &Ehs2Abstraction,
        flops: &[(Board, u32)],
        turns: &[(Board, u32)],
        river5: &[([Card; 5], u64)],
    ) -> Self {
        BlueprintArtifacts {
            class_to_flop: class_to_flop_table(abs, flops),
            flop_to_turn: flop_to_turn_table(abs, turns),
            turn_to_river: turn_to_river_table(abs, turns),
            river_equity: river_equity_over(abs, river5),
        }
    }

    /// Reads a cached artifact set, verifying magic/version and that it was
    /// built for the same abstraction params.
    pub fn load(path: &Path, abs: &Ehs2Abstraction) -> Result<Self, BlueprintCacheError> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < CACHE_HEADER_LEN || &bytes[0..8] != CACHE_MAGIC {
            return Err(BlueprintCacheError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != CACHE_VERSION {
            return Err(BlueprintCacheError::BadVersion {
                found: version,
                expected: CACHE_VERSION,
            });
        }
        let (params, artifacts): (Ehs2Params, BlueprintArtifacts) =
            postcard::from_bytes(&bytes[CACHE_HEADER_LEN..])?;
        if params != abs_params(abs) {
            return Err(BlueprintCacheError::ParamsMismatch);
        }
        Ok(artifacts)
    }

    /// Writes atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<(), BlueprintCacheError> {
        let params = artifacts_params(self);
        let payload = postcard::to_allocvec(&(params, self))?;
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
                .unwrap_or("blueprint.postcard")
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

    /// Loads from `cache` when present and valid; otherwise builds and
    /// best-effort saves.
    pub fn load_or_build(abs: &Ehs2Abstraction, cache: Option<&Path>) -> Self {
        if let Some(path) = cache
            && let Ok(artifacts) = Self::load(path, abs)
        {
            return artifacts;
        }
        let artifacts = Self::build(abs);
        if let Some(path) = cache {
            let _ = artifacts.save(path);
        }
        artifacts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buckets::canonicalize_board_subset;
    use std::cmp::Ordering;

    fn parse(s: &str) -> Vec<Card> {
        s.split_whitespace().map(|c| c.parse().unwrap()).collect()
    }

    /// A small abstraction plus matching (flop, turn, river) canonical
    /// board lists built from the *same* literal boards, so the artifact
    /// builders exercise exactly the boards the abstraction has data for.
    /// Every turn board's 48 river completions are included — the T3
    /// builder extends each turn by every live card, so the river street
    /// must cover all of them.
    struct SmallFixture {
        abs: Ehs2Abstraction,
        flops: Vec<(Board, u32)>,
        turns: Vec<(Board, u32)>,
        rivers: Vec<(Board, u32)>,
    }

    /// Literal (non-canonicalized) boards: `flops`, plus each flop extended
    /// by every card in `turns_per_flop`, plus each such turn's 48 river
    /// completions.
    fn closed_literal_boards(flops: &[Vec<Card>], turns_per_flop: &[Card]) -> Vec<Vec<Card>> {
        let mut literal: Vec<Vec<Card>> = Vec::new();
        for flop in flops {
            literal.push(flop.clone());
            for &t in turns_per_flop {
                let mut turn = flop.clone();
                turn.push(t);
                literal.push(turn.clone());
                let used: CardSet = turn.iter().copied().collect();
                for r in ALL_CARDS {
                    if used.contains(r) {
                        continue;
                    }
                    let mut river = turn.clone();
                    river.push(r);
                    literal.push(river);
                }
            }
        }
        literal
    }

    fn small_fixture(k: u32) -> SmallFixture {
        let flops: Vec<Vec<Card>> = ["2c 7d Kh", "As Ks Qs", "9c 9d 2h"]
            .iter()
            .map(|s| parse(s))
            .collect();
        let turns: Vec<Card> = ["3h", "4s", "5d"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        let literal_boards = closed_literal_boards(&flops, &turns);
        let params = Ehs2Params {
            flop_buckets: k,
            turn_buckets: k,
            river_buckets: k,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, &literal_boards);
        let (flops, turns, rivers) = canonicalize_board_subset(&literal_boards);
        SmallFixture {
            abs,
            flops,
            turns,
            rivers,
        }
    }

    fn river5_from(rivers: &[(Board, u32)]) -> Vec<([Card; 5], u64)> {
        rivers
            .iter()
            .map(|(b, w)| {
                let cards: Vec<Card> = b.cards().collect();
                let arr: [Card; 5] = cards.try_into().unwrap();
                (arr, *w as u64)
            })
            .collect()
    }

    // --- kappa / N-table -------------------------------------------------

    #[test]
    fn compat_count_goldens() {
        let n = compat_counts();
        let aa = class_index(12, 12, false);
        let kk = class_index(11, 11, false);
        assert_eq!(n[aa * NUM_CLASSES + aa], 6, "N(AA, AA)");
        assert_eq!(n[aa * NUM_CLASSES + kk], 36, "N(AA, KK)");
        let total: u64 = n.iter().map(|&c| c as u64).sum();
        assert_eq!(total, 1_326 * 1_225);
        assert_eq!(total, 1_624_350);
    }

    #[test]
    fn kappa_closed_form_in_range() {
        let kappa = kappa_per_class();
        for (h, &k) in kappa.iter().enumerate() {
            assert!(
                (0.85..1.0).contains(&k),
                "kappa[{h}] = {k} not in (0.85, 1.0)"
            );
        }
    }

    #[test]
    fn kappa_is_a_universal_constant() {
        // See the module docs: full-range-weighted averaging of compat(h,o)
        // over ALL o exactly cancels the card-overlap effect, for every h,
        // because any single fixed combo is disjoint from exactly
        // C(50, 2) = 1225 of the other 1325 combos regardless of its own
        // identity. This contradicts the "smallest for classes with the
        // most card overlap" intuition in the original design sketch; this
        // test pins the (surprising but exact) reality instead.
        let kappa = kappa_per_class();
        let expected = 1225.0 / 1326.0;
        for (h, &k) in kappa.iter().enumerate() {
            assert!(
                (k - expected).abs() < 1e-12,
                "kappa[{h}] = {k}, expected constant {expected}"
            );
        }
    }

    // --- T1/T2/T3 row sums + mass accounting ------------------------------

    #[test]
    fn t1_total_raw_mass_matches_expected() {
        let SmallFixture { abs, flops, .. } = small_fixture(4);
        let (raw, _kf) = class_to_flop_counts(&abs, &flops);
        let total: u64 = raw.iter().sum();
        // Every 3-card board has exactly C(49, 2) = 1176 live combos.
        let expected: u64 = flops.iter().map(|&(_, w)| w as u64 * 1_176).sum();
        assert_eq!(total, expected);
    }

    #[test]
    fn t1_rows_sum_to_kappa() {
        let SmallFixture { abs, flops, .. } = small_fixture(4);
        let (raw, kf) = class_to_flop_counts(&abs, &flops);
        let kappa = kappa_per_class();
        for h in 0..NUM_CLASSES {
            let row = &raw[h * kf..(h + 1) * kf];
            let row_sum: u64 = row.iter().sum();
            if row_sum == 0 {
                continue; // no live combo of this class on any test board
            }
            let reconstructed: f64 = row
                .iter()
                .map(|&c| kappa[h] * c as f64 / row_sum as f64)
                .sum();
            assert!(
                (reconstructed - kappa[h]).abs() < 1e-9,
                "class {h}: row sum {reconstructed} != kappa {}",
                kappa[h]
            );
        }
        // Also check the actual public (f32) TransitionTable, looser
        // tolerance for the lossy cast.
        let table = class_to_flop_table(&abs, &flops);
        let mut sums = vec![0f64; NUM_CLASSES];
        for &(h, _, w) in &table.entries {
            sums[h as usize] += w as f64;
        }
        for h in 0..NUM_CLASSES {
            if sums[h] == 0.0 {
                continue;
            }
            assert!(
                (sums[h] - kappa[h]).abs() < 1e-4,
                "public row {h} sum {} vs kappa {}",
                sums[h],
                kappa[h]
            );
        }
    }

    #[test]
    fn t2_and_t3_total_raw_mass_and_row_sums() {
        let SmallFixture { abs, turns, .. } = small_fixture(4);

        let (t2_counts, kf, kt) = flop_to_turn_counts(&abs, &turns);
        let t2_total: u64 = t2_counts.iter().sum();
        // Every 4-card board has C(48, 2) = 1128 live combos.
        let t2_expected: u64 = turns.iter().map(|&(_, w)| w as u64 * 1_128).sum();
        assert_eq!(t2_total, t2_expected);
        for bf in 0..kf {
            let row = &t2_counts[bf * kt..(bf + 1) * kt];
            let row_sum: u64 = row.iter().sum();
            if row_sum == 0 {
                continue;
            }
            let reconstructed: f64 = row.iter().map(|&c| c as f64 / row_sum as f64).sum();
            assert!((reconstructed - 1.0).abs() < 1e-9);
        }

        let (t3_counts, kt2, kr) = turn_to_river_counts(&abs, &turns);
        assert_eq!(kt2, kt);
        let t3_total: u64 = t3_counts.iter().sum();
        // Per turn board: 48 river cards x C(47, 2) = 1081 live combos each.
        let t3_expected: u64 = turns.iter().map(|&(_, w)| w as u64 * 48 * 1_081).sum();
        assert_eq!(t3_total, t3_expected);
        for bt in 0..kt {
            let row = &t3_counts[bt * kr..(bt + 1) * kr];
            let row_sum: u64 = row.iter().sum();
            if row_sum == 0 {
                continue;
            }
            let reconstructed: f64 = row.iter().map(|&c| c as f64 / row_sum as f64).sum();
            assert!((reconstructed - 1.0).abs() < 1e-9);
        }
    }

    // --- batched-lookup vs naive bucket() agreement ------------------------

    #[test]
    fn t2_prefix_matches_naive_bucket_lookup() {
        let SmallFixture { abs, turns, .. } = small_fixture(4);
        let mut checked = 0;
        for (turn_board, _w) in &turns {
            let board_set: CardSet = turn_board.cards().collect();
            let (flop_row, perm) = flop_row_for_turn_prefix(&abs, turn_board);
            // Stride through combos so every turn board contributes samples
            // instead of the first board exhausting the check budget.
            for combo in (0..NUM_COMBOS).step_by(29) {
                let (c1, c2) = combo_cards(combo);
                if board_set.contains(c1) || board_set.contains(c2) {
                    continue;
                }
                let b_f = flop_row[permute_combo(&perm, combo)];
                let expected = abs.bucket(&turn_board.flop, combo);
                assert_eq!(
                    b_f as u32, expected,
                    "turn board {turn_board:?} combo {combo}"
                );
                checked += 1;
            }
        }
        assert!(checked >= 300, "test exercised too few samples: {checked}");
    }

    #[test]
    fn t3_river_set_lookup_matches_naive_bucket() {
        // The batched unordered-set row lookup T3 and river equity rely on
        // must agree with the public per-query bucket() path, across the
        // exact (turn x river card) product T3 iterates. Stride through
        // boards and combos so samples spread over many (board, combo)
        // pairs rather than exhausting the budget on the first board.
        let SmallFixture { abs, turns, .. } = small_fixture(4);
        let mut checked = 0;
        for (turn_board, _w) in &turns {
            let four: Vec<Card> = turn_board.cards().collect();
            let turn_set: CardSet = four.iter().copied().collect();
            for r in ALL_CARDS.into_iter().step_by(7) {
                if turn_set.contains(r) {
                    continue;
                }
                let five = [four[0], four[1], four[2], four[3], r];
                let mut board_set = turn_set;
                board_set.insert(r);
                let (river_row, perm) = abs.river_row_for_set(five);
                for combo in (0..NUM_COMBOS).step_by(151) {
                    let (c1, c2) = combo_cards(combo);
                    if board_set.contains(c1) || board_set.contains(c2) {
                        continue;
                    }
                    let b_r = river_row[permute_combo(&perm, combo)];
                    let expected = abs.bucket(&five, combo);
                    assert_eq!(b_r as u32, expected, "board {five:?} combo {combo}");
                    checked += 1;
                }
            }
        }
        assert!(checked >= 300, "test exercised too few samples: {checked}");
    }

    // --- river bucket invariance to street-role split ---------------------

    #[test]
    fn river_bucket_invariant_to_street_split() {
        // Regression guard on the lookup path: with the river street keyed
        // by the unordered 5-set quotient this is nearly tautological, but
        // it pins the contract that made that quotient sound in the first
        // place (river scores depend only on the unordered set + hole), so
        // a future re-keying that breaks it fails here.
        let five = parse("2c 7d Kh 3h 6c");
        // Three different (flop-set, later-order) role assignments of the
        // same underlying 5 physical cards.
        let splits: Vec<Vec<Card>> = vec![
            vec![five[0], five[1], five[2], five[3], five[4]],
            vec![five[0], five[3], five[4], five[1], five[2]],
            vec![five[1], five[2], five[3], five[0], five[4]],
        ];
        let params = Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, &splits);

        let dead: CardSet = five.iter().copied().collect();
        let combo = (0..NUM_COMBOS)
            .find(|&c| {
                let (a, b) = combo_cards(c);
                !dead.contains(a) && !dead.contains(b)
            })
            .unwrap();

        let buckets: Vec<u32> = splits.iter().map(|s| abs.bucket(s, combo)).collect();
        assert!(
            buckets.windows(2).all(|w| w[0] == w[1]),
            "bucket differs across street splits of the same 5-card board: {buckets:?}"
        );
    }

    // --- river sweep vs brute force + exactness invariant -----------------

    fn accumulate_river_board_brute_force(
        abs: &Ehs2Abstraction,
        board: [Card; 5],
        weight: u64,
        kr: usize,
        win_acc: &mut [u64],
        tie_acc: &mut [u64],
        pair_acc: &mut [u64],
    ) {
        let board_set: CardSet = board.into_iter().collect();
        let (river_row, perm) = abs.river_row_for_set(board);
        let live: Vec<(usize, u16, HandRank)> = (0..NUM_COMBOS)
            .filter_map(|combo| {
                let (c1, c2) = combo_cards(combo);
                if board_set.contains(c1) || board_set.contains(c2) {
                    return None;
                }
                let bucket = river_row[permute_combo(&perm, combo)];
                let rank = rank_of(board.into_iter().chain([c1, c2]));
                Some((combo, bucket, rank))
            })
            .collect();
        for &(c, bc, rc) in &live {
            let (c1, c2) = combo_cards(c);
            for &(d, bd, rd) in &live {
                if c == d {
                    continue;
                }
                let (d1, d2) = combo_cards(d);
                if d1 == c1 || d1 == c2 || d2 == c1 || d2 == c2 {
                    continue;
                }
                pair_acc[bc as usize * kr + bd as usize] += weight;
                match rc.cmp(&rd) {
                    Ordering::Greater => win_acc[bc as usize * kr + bd as usize] += weight,
                    Ordering::Equal => tie_acc[bc as usize * kr + bd as usize] += weight,
                    Ordering::Less => {}
                }
            }
        }
    }

    #[test]
    fn river_sweep_matches_brute_force_on_fixed_boards() {
        let SmallFixture { abs, rivers, .. } = small_fixture(4);
        let kr = 4usize;
        assert!(
            rivers.len() >= 2,
            "fixture should have several river boards"
        );
        for (river_board, _w) in rivers.iter().take(2) {
            let cards: Vec<Card> = river_board.cards().collect();
            let five: [Card; 5] = cards.try_into().unwrap();

            let zeros = || vec![0u64; kr * kr];
            let (mut win_fast, mut tie_fast, mut pair_fast) = (zeros(), zeros(), zeros());
            let mut scratch = RiverScratch::new(kr);
            let perm_tables = ComboPermTables::new();
            accumulate_river_board(
                &abs,
                &perm_tables,
                five,
                1,
                kr,
                &mut win_fast,
                &mut tie_fast,
                &mut pair_fast,
                &mut scratch,
            );

            let (mut win_brute, mut tie_brute, mut pair_brute) = (zeros(), zeros(), zeros());
            accumulate_river_board_brute_force(
                &abs,
                five,
                1,
                kr,
                &mut win_brute,
                &mut tie_brute,
                &mut pair_brute,
            );

            assert_eq!(win_fast, win_brute, "win mismatch for board {five:?}");
            assert_eq!(tie_fast, tie_brute, "tie mismatch for board {five:?}");
            assert_eq!(pair_fast, pair_brute, "pair mismatch for board {five:?}");
        }
    }

    #[test]
    fn river_equity_counts_satisfy_exactness_invariant() {
        let SmallFixture { abs, rivers, .. } = small_fixture(4);
        let river5 = river5_from(&rivers);
        let (win, tie, pair, kr) = river_equity_counts(&abs, &river5);
        for b1 in 0..kr {
            for b2 in 0..kr {
                let (i, j) = (b1 * kr + b2, b2 * kr + b1);
                assert_eq!(
                    win[i] + tie[i] + win[j],
                    pair[i],
                    "exactness failed for ({b1}, {b2})"
                );
                assert_eq!(pair[i], pair[j], "pair_cnt not symmetric for ({b1}, {b2})");
                assert_eq!(tie[i], tie[j], "tie_cnt not symmetric for ({b1}, {b2})");
            }
        }
    }

    // --- cache round trip ---------------------------------------------------

    fn tiny_artifacts() -> (Ehs2Abstraction, BlueprintArtifacts) {
        let SmallFixture {
            abs,
            flops,
            turns,
            rivers,
        } = small_fixture(4);
        let river5 = river5_from(&rivers);
        let artifacts = BlueprintArtifacts::build_over(&abs, &flops, &turns, &river5);
        (abs, artifacts)
    }

    /// The cheapest possible abstraction (one river board = one rank
    /// sweep) for cache tests that only need params to compare against.
    fn one_board_abs(params: Ehs2Params) -> Ehs2Abstraction {
        Ehs2Abstraction::build_for_boards(params, &[parse("2c 7d Kh 3h 6c")])
    }

    #[test]
    fn cache_round_trip() {
        let (abs, artifacts) = tiny_artifacts();
        let path = std::env::temp_dir().join(format!(
            "blueprint-cache-test-{}-{}.postcard",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        artifacts.save(&path).unwrap();
        let loaded = BlueprintArtifacts::load(&path, &abs).unwrap();
        assert_eq!(loaded, artifacts);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_bad_magic() {
        let abs = one_board_abs(Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        });
        let path = std::env::temp_dir().join(format!(
            "blueprint-cache-badmagic-{}.postcard",
            std::process::id()
        ));
        std::fs::write(&path, b"NOTMAGIC").unwrap();
        assert!(matches!(
            BlueprintArtifacts::load(&path, &abs),
            Err(BlueprintCacheError::BadMagic)
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_bad_version() {
        let abs = one_board_abs(Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        });
        let path = std::env::temp_dir().join(format!(
            "blueprint-cache-badversion-{}.postcard",
            std::process::id()
        ));
        let mut buf = Vec::new();
        buf.extend_from_slice(CACHE_MAGIC);
        buf.extend_from_slice(&99u16.to_le_bytes());
        std::fs::write(&path, &buf).unwrap();
        match BlueprintArtifacts::load(&path, &abs) {
            Err(BlueprintCacheError::BadVersion { found, expected }) => {
                assert_eq!(found, 99);
                assert_eq!(expected, CACHE_VERSION);
            }
            Ok(_) => panic!("expected BadVersion, got Ok"),
            Err(other) => panic!("expected BadVersion, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_mismatched_params() {
        let (_abs, artifacts) = tiny_artifacts();
        let path = std::env::temp_dir().join(format!(
            "blueprint-cache-mismatch-{}.postcard",
            std::process::id()
        ));
        artifacts.save(&path).unwrap();

        // A different abstraction (different K) should be rejected.
        let other_abs = one_board_abs(Ehs2Params {
            flop_buckets: 8,
            turn_buckets: 4,
            river_buckets: 4,
        });
        assert!(matches!(
            BlueprintArtifacts::load(&path, &other_abs),
            Err(BlueprintCacheError::ParamsMismatch)
        ));
        let _ = std::fs::remove_file(&path);
    }

    // --- board-subset build at scale (ignored: release CI) -----------------

    #[test]
    #[ignore = "board-subset abstraction + artifact build (~1-2 min release, well under 2GB); \
                CI runs it in release with --include-ignored; the FULL-street build stays \
                user-invoked via load_or_build (turn-street scoring alone is minutes and the \
                tables run to hundreds of MB — past the CI budget)"]
    fn subset_artifact_build_invariants() {
        // A spread of canonical flops (every 50th of the 1,755, mixing
        // textures), closed under: all 49 turn extensions per flop, all 48
        // river completions per turn — the closure the T3 (turn x river
        // card) product needs. ~86k literal boards; ~36 flops / ~1.7k
        // canonical turns / ~35k canonical river sets after dedup.
        let sampled: Vec<Vec<Card>> = canonical_flops()
            .into_iter()
            .step_by(50)
            .map(|(b, _)| b.flop)
            .collect();
        // closed_literal_boards assumes its turn cards don't collide with
        // the flop, so filter the 52 candidates per flop.
        let mut literal: Vec<Vec<Card>> = Vec::new();
        for flop in &sampled {
            let flop_set: CardSet = flop.iter().copied().collect();
            let live_turns: Vec<Card> = ALL_CARDS
                .into_iter()
                .filter(|&t| !flop_set.contains(t))
                .collect();
            literal.extend(closed_literal_boards(
                std::slice::from_ref(flop),
                &live_turns,
            ));
        }

        let params = Ehs2Params {
            flop_buckets: 8,
            turn_buckets: 8,
            river_buckets: 8,
        };
        let abs_start = std::time::Instant::now();
        let abs = Ehs2Abstraction::build_for_boards(params, &literal);
        eprintln!(
            "subset Ehs2Abstraction::build_for_boards ({} literals) took {:?}",
            literal.len(),
            abs_start.elapsed()
        );

        let (flops, turns, rivers) = canonicalize_board_subset(&literal);
        eprintln!(
            "subset canonical boards: {} flops, {} turns, {} river sets",
            flops.len(),
            turns.len(),
            rivers.len()
        );
        let river5 = river5_from(&rivers);

        let artifacts_start = std::time::Instant::now();
        let artifacts = BlueprintArtifacts::build_over(&abs, &flops, &turns, &river5);
        eprintln!(
            "subset BlueprintArtifacts::build_over took {:?}",
            artifacts_start.elapsed()
        );

        // T1 rows sum to kappa(h).
        let kappa = kappa_per_class();
        let mut class_sums = vec![0f64; NUM_CLASSES];
        for &(h, _, w) in &artifacts.class_to_flop.entries {
            class_sums[h as usize] += w as f64;
        }
        for h in 0..NUM_CLASSES {
            if class_sums[h] == 0.0 {
                continue;
            }
            assert!(
                (class_sums[h] - kappa[h]).abs() < 1e-4,
                "class {h} row sum {} vs kappa {}",
                class_sums[h],
                kappa[h]
            );
        }

        // T2/T3 rows sum to 1.
        let kf = params.flop_buckets as usize;
        let kt = params.turn_buckets as usize;
        let mut t2_sums = vec![0f64; kf];
        for &(bf, _, w) in &artifacts.flop_to_turn.entries {
            t2_sums[bf as usize] += w as f64;
        }
        for &s in &t2_sums {
            if s > 0.0 {
                assert!((s - 1.0).abs() < 1e-4, "flop_to_turn row sum {s}");
            }
        }
        let mut t3_sums = vec![0f64; kt];
        for &(bt, _, w) in &artifacts.turn_to_river.entries {
            t3_sums[bt as usize] += w as f64;
        }
        for &s in &t3_sums {
            if s > 0.0 {
                assert!((s - 1.0).abs() < 1e-4, "turn_to_river row sum {s}");
            }
        }

        // Crown-jewel exactness invariant on raw river counts, plus
        // symmetry, plus the normalized probability identity.
        let (win, tie, pair, kr) = river_equity_counts(&abs, &river5);
        for b1 in 0..kr {
            for b2 in 0..kr {
                let (i, j) = (b1 * kr + b2, b2 * kr + b1);
                assert_eq!(
                    win[i] + tie[i] + win[j],
                    pair[i],
                    "exactness failed for ({b1}, {b2})"
                );
                assert_eq!(pair[i], pair[j], "pair_cnt not symmetric ({b1}, {b2})");
                assert_eq!(tie[i], tie[j], "tie_cnt not symmetric ({b1}, {b2})");
                assert!(pair[i] > 0, "bucket pair ({b1}, {b2}) never co-occurs");
                let sum = artifacts.river_equity.win[i]
                    + artifacts.river_equity.tie[i]
                    + artifacts.river_equity.win[j];
                assert!(
                    (sum - 1.0).abs() < 1e-9,
                    "river_equity ({b1}, {b2}): win + tie + win^T = {sum}"
                );
            }
        }

        // Cache round trip at scale.
        let path = std::env::temp_dir().join(format!(
            "blueprint-cache-subset-{}.postcard",
            std::process::id()
        ));
        artifacts.save(&path).unwrap();
        let loaded = BlueprintArtifacts::load(&path, &abs).unwrap();
        assert_eq!(loaded, artifacts);
        let _ = std::fs::remove_file(&path);
    }
}
