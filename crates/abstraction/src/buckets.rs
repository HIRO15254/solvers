//! E[HS²] percentile bucketing with per-street canonical-board tables and a
//! postcard disk cache.
//!
//! Internal representation avoids `hand_index::Board`/`cards::Card` in the
//! serialized form (neither implements `serde`): canonical boards are keyed
//! by raw card-index arrays (`[u8; 3]` flop, `[u8; 4]` turn, `[u8; 5]`
//! river), each mapping to a `Vec<u16>` indexed by `cards::combo_index`
//! (dead combos, i.e. sharing a card with the board, are `u16::MAX`).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use cards::{ALL_CARDS, Card, CardSet, HandRank, NUM_COMBOS, combo_cards, rank_of};
use hand_index::{Board, SuitPerm, canonical_flops, canonicalize_board, permute_combo};
use rayon::prelude::*;

use cards::Street;

use crate::CardAbstraction;

/// Bucket counts per postflop street.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ehs2Params {
    pub flop_buckets: u32,
    pub turn_buckets: u32,
    pub river_buckets: u32,
}

/// Errors from the bucket-table disk cache codec.
#[derive(Debug, thiserror::Error)]
pub enum BucketCacheError {
    #[error("bad magic bytes (not a bucket cache file)")]
    BadMagic,
    #[error("unsupported cache version {found} (expected {expected})")]
    BadVersion { found: u16, expected: u16 },
    #[error("cache was built with different params or streets")]
    ParamsMismatch,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("codec error: {0}")]
    Codec(#[from] postcard::Error),
}

const CACHE_MAGIC: &[u8; 8] = b"SLVRBKTS";
const CACHE_VERSION: u16 = 1;
const CACHE_HEADER_LEN: usize = 8 + 2;

/// One street's percentile table: global cut thresholds (ascending, length
/// `num_buckets - 1`) plus per-canonical-board bucket assignments.
///
/// Board keys are `Vec<u8>` (card indices: 3 for flop, 4 for turn, 5 for
/// river) rather than a fixed-size array — `serde`'s array impls aren't
/// generic over a const `N`, so a `[u8; N]` key can't be derived once for
/// all three streets; a small `Vec` avoids that without triplicating the
/// table type.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct StreetTable {
    thresholds: Vec<f64>,
    boards: BTreeMap<Vec<u8>, Vec<u16>>,
}

/// E[HS²] percentile abstraction over one or more postflop streets.
///
/// Percentile thresholds are global per street across every scored
/// situation, weighted by the canonical board's suit-class multiplicity, so
/// buckets have (approximately) equal total mass. Lookup canonicalizes the
/// board, permutes the combo with the same suit permutation, and reads the
/// per-canonical-board table.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ehs2Abstraction {
    params: Ehs2Params,
    flop: Option<StreetTable>,
    turn: Option<StreetTable>,
    river: Option<StreetTable>,
}

// --- canonical-board key helpers -------------------------------------------

fn flop_key(board: &Board) -> Vec<u8> {
    vec![
        board.flop[0].index() as u8,
        board.flop[1].index() as u8,
        board.flop[2].index() as u8,
    ]
}

fn turn_key(board: &Board) -> Vec<u8> {
    vec![
        board.flop[0].index() as u8,
        board.flop[1].index() as u8,
        board.flop[2].index() as u8,
        board.later[0].index() as u8,
    ]
}

fn river_key(board: &Board) -> Vec<u8> {
    vec![
        board.flop[0].index() as u8,
        board.flop[1].index() as u8,
        board.flop[2].index() as u8,
        board.later[0].index() as u8,
        board.later[1].index() as u8,
    ]
}

// --- canonical board enumeration for turn/river ----------------------------

/// All canonical turn boards (flop as a set + one later card) with
/// multiplicities, derived from [`canonical_flops`] extended by every
/// non-flop card. Correct by the same orbit argument `canonical_flops` uses:
/// extending a canonical flop's representative by all 49 candidate cards and
/// weighting the result by the flop's own multiplicity reproduces exactly
/// what summing over every raw flop in its orbit would give, since suit
/// permutations act bijectively on "cards not on the flop" and
/// canonicalization is invariant under them.
///
/// `pub(crate)`: the `blueprint` module enumerates the same 63,193 canonical
/// turn boards to build `T2`.
pub(crate) fn canonical_turns() -> Vec<(Board, u32)> {
    let flops = canonical_flops();
    let mut counts: BTreeMap<Board, u32> = BTreeMap::new();
    for (flop_board, weight) in &flops {
        let flop_set: CardSet = flop_board.flop.iter().copied().collect();
        for t in ALL_CARDS {
            if flop_set.contains(t) {
                continue;
            }
            let raw = Board::new(&flop_board.flop, &[t]);
            let (canon, _) = canonicalize_board(&raw);
            *counts.entry(canon).or_insert(0) += weight;
        }
    }
    counts.into_iter().collect()
}

/// All canonical river boards (flop as a set + turn, river later cards) with
/// multiplicities, derived from [`canonical_turns`] by the same argument.
///
/// `pub(crate)`: the `blueprint` module enumerates the same 134,459
/// street-structured canonical river boards to build `T3`.
pub(crate) fn canonical_rivers() -> Vec<(Board, u32)> {
    let turns = canonical_turns();
    let mut counts: BTreeMap<Board, u32> = BTreeMap::new();
    for (turn_board, weight) in &turns {
        let used: CardSet = turn_board
            .flop
            .iter()
            .chain(turn_board.later.iter())
            .copied()
            .collect();
        for r in ALL_CARDS {
            if used.contains(r) {
                continue;
            }
            let raw = Board::new(&turn_board.flop, &[turn_board.later[0], r]);
            let (canon, _) = canonicalize_board(&raw);
            *counts.entry(canon).or_insert(0) += weight;
        }
    }
    counts.into_iter().collect()
}

// --- the amortized per-board rank sweep ------------------------------------

/// Reusable scratch for [`Sweep::sweep`], sized so building thousands of
/// boards allocates only once per rayon worker (via `map_init`).
struct Sweep {
    /// (rank, combo) for every combo not touching the current board.
    live: Vec<(HandRank, usize)>,
    /// Running count of already-swept (strictly lower-ranked) live combos.
    lower: u32,
    /// Running count of already-swept live combos containing a given card.
    lower_card: [u32; 52],
    /// Per-card count of combos within the tied group currently being
    /// processed; reset lazily via `touched` so groups don't cost O(52).
    group_cards: [u32; 52],
    touched: Vec<usize>,
}

impl Sweep {
    fn new() -> Self {
        Sweep {
            live: Vec::with_capacity(1_100),
            lower: 0,
            lower_card: [0; 52],
            group_cards: [0; 52],
            touched: Vec::with_capacity(64),
        }
    }

    /// Ranks every combo disjoint from the complete 5-card `board`, then
    /// sweeps sorted-by-rank to fill `hs_out[combo]` for each live combo
    /// with its hand strength against the uniform live-opponent pool
    /// (exactly `cards::rank_of`-comparisons, no sampling). Ties within a
    /// group are resolved in O(1) per combo via a per-card tally of the
    /// group's own cards (mirrors the `lower`/`lower_card` inclusion-
    /// exclusion trick, applied to the group instead of everything below
    /// it) — this keeps the whole sweep O(live) beyond the initial sort,
    /// which matters because build paths call this millions of times.
    ///
    /// After the call, `self.live` holds every live `(rank, combo)` pair
    /// (sorted); callers read it directly instead of receiving it back, to
    /// avoid an awkward self-borrowing return type.
    fn sweep(&mut self, board_set: CardSet, board_cards: [Card; 5], hs_out: &mut [f64]) {
        self.live.clear();
        for combo in 0..NUM_COMBOS {
            let (c1, c2) = combo_cards(combo);
            if board_set.contains(c1) || board_set.contains(c2) {
                continue;
            }
            let rank = rank_of(board_cards.into_iter().chain([c1, c2]));
            self.live.push((rank, combo));
        }
        self.live.sort_unstable_by_key(|&(r, _)| r);

        self.lower = 0;
        self.lower_card = [0; 52];

        let n = self.live.len();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n && self.live[j].0 == self.live[i].0 {
                j += 1;
            }
            let group_len = (j - i) as u32;

            self.touched.clear();
            for &(_, combo) in &self.live[i..j] {
                let (c1, c2) = combo_cards(combo);
                for c in [c1, c2] {
                    if self.group_cards[c.index()] == 0 {
                        self.touched.push(c.index());
                    }
                    self.group_cards[c.index()] += 1;
                }
            }

            for &(_, combo) in &self.live[i..j] {
                let (c1, c2) = combo_cards(combo);
                let win_pairs = self.lower as i64
                    - self.lower_card[c1.index()] as i64
                    - self.lower_card[c2.index()] as i64;
                // Every other group member ties, except those sharing a
                // card with `combo` (which can't be its opponent).
                let tie_pairs = group_len as i64 + 1
                    - self.group_cards[c1.index()] as i64
                    - self.group_cards[c2.index()] as i64;
                debug_assert!(win_pairs >= 0 && tie_pairs >= 0);
                hs_out[combo] = (win_pairs as f64 + 0.5 * tie_pairs as f64) / 990.0;
            }

            for &c in &self.touched {
                self.group_cards[c] = 0;
            }
            for &(_, combo) in &self.live[i..j] {
                let (c1, c2) = combo_cards(combo);
                self.lower_card[c1.index()] += 1;
                self.lower_card[c2.index()] += 1;
            }
            self.lower += group_len;

            i = j;
        }
    }
}

// --- per-street scoring (E[HS²], amortized over completions) --------------

/// Flop score: E[HS²] over all C(49, 2) = 1,176 (turn, river) completions,
/// swept once per completion and folded into every live hero's
/// accumulators — never re-ranked per hero. Each live combo naturally
/// accumulates over exactly C(47, 2) = 1,081 of those completions (the ones
/// not touching its own two cards), matching `ehs::ehs2`'s definition.
fn score_flop_board(flop: [Card; 3], sweep: &mut Sweep, hs_scratch: &mut [f64]) -> Vec<f32> {
    let board_set: CardSet = flop.into_iter().collect();
    let remaining: Vec<Card> = ALL_CARDS
        .into_iter()
        .filter(|&c| !board_set.contains(c))
        .collect();
    debug_assert_eq!(remaining.len(), 49);

    let mut sum_hs2 = vec![0f64; NUM_COMBOS];
    let mut count = vec![0u32; NUM_COMBOS];

    for i in 0..remaining.len() {
        for j in (i + 1)..remaining.len() {
            let (t, r) = (remaining[i], remaining[j]);
            let mut board5_set = board_set;
            board5_set.insert(t);
            board5_set.insert(r);
            let board5 = [flop[0], flop[1], flop[2], t, r];
            sweep.sweep(board5_set, board5, hs_scratch);
            for &(_, combo) in &sweep.live {
                let hs = hs_scratch[combo];
                sum_hs2[combo] += hs * hs;
                count[combo] += 1;
            }
        }
    }

    let mut scores = vec![f32::NAN; NUM_COMBOS];
    for combo in 0..NUM_COMBOS {
        let (c1, c2) = combo_cards(combo);
        if board_set.contains(c1) || board_set.contains(c2) {
            continue; // dead: shares a card with the board itself
        }
        debug_assert_eq!(count[combo], 1_081, "C(47, 2) flop completions per hero");
        scores[combo] = (sum_hs2[combo] / count[combo] as f64) as f32;
    }
    scores
}

/// Turn score: E[HS²] over all 48 river completions, same amortization as
/// [`score_flop_board`]; each live combo accumulates over exactly 46 of
/// them (52 - 4 board - 2 hole).
fn score_turn_board(turn: [Card; 4], sweep: &mut Sweep, hs_scratch: &mut [f64]) -> Vec<f32> {
    let board_set: CardSet = turn.into_iter().collect();

    let mut sum_hs2 = vec![0f64; NUM_COMBOS];
    let mut count = vec![0u32; NUM_COMBOS];

    for river in ALL_CARDS {
        if board_set.contains(river) {
            continue;
        }
        let mut board5_set = board_set;
        board5_set.insert(river);
        let board5 = [turn[0], turn[1], turn[2], turn[3], river];
        sweep.sweep(board5_set, board5, hs_scratch);
        for &(_, combo) in &sweep.live {
            let hs = hs_scratch[combo];
            sum_hs2[combo] += hs * hs;
            count[combo] += 1;
        }
    }

    let mut scores = vec![f32::NAN; NUM_COMBOS];
    for combo in 0..NUM_COMBOS {
        let (c1, c2) = combo_cards(combo);
        if board_set.contains(c1) || board_set.contains(c2) {
            continue;
        }
        debug_assert_eq!(count[combo], 46, "52 - 4 board - 2 hole");
        scores[combo] = (sum_hs2[combo] / count[combo] as f64) as f32;
    }
    scores
}

/// River score: HS² directly from a single sweep (no further completions —
/// the board is already complete). Percentile ranking of HS² equals that of
/// HS since squaring is monotone on `[0, 1]`.
fn score_river_board(river: [Card; 5], sweep: &mut Sweep, hs_scratch: &mut [f64]) -> Vec<f32> {
    let board_set: CardSet = river.into_iter().collect();
    sweep.sweep(board_set, river, hs_scratch);
    let mut scores = vec![f32::NAN; NUM_COMBOS];
    for &(_, combo) in &sweep.live {
        let hs = hs_scratch[combo];
        scores[combo] = (hs * hs) as f32;
    }
    scores
}

fn flop_score_fn(board: &Board, sweep: &mut Sweep, hs: &mut [f64]) -> Vec<f32> {
    score_flop_board([board.flop[0], board.flop[1], board.flop[2]], sweep, hs)
}

fn turn_score_fn(board: &Board, sweep: &mut Sweep, hs: &mut [f64]) -> Vec<f32> {
    score_turn_board(
        [board.flop[0], board.flop[1], board.flop[2], board.later[0]],
        sweep,
        hs,
    )
}

fn river_score_fn(board: &Board, sweep: &mut Sweep, hs: &mut [f64]) -> Vec<f32> {
    score_river_board(
        [
            board.flop[0],
            board.flop[1],
            board.flop[2],
            board.later[0],
            board.later[1],
        ],
        sweep,
        hs,
    )
}

// --- percentile binning ------------------------------------------------

/// Cuts weighted scores into `k` equal-total-weight bins, returning the
/// `k - 1` ascending cut thresholds (bin `i`'s upper edge).
fn percentile_thresholds(mut scored: Vec<(f32, u32)>, k: u32) -> Vec<f64> {
    assert!(k >= 1, "num_buckets must be at least 1");
    if k == 1 || scored.is_empty() {
        return Vec::new();
    }
    scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let total_weight: u64 = scored.iter().map(|&(_, w)| w as u64).sum();

    let mut thresholds = Vec::with_capacity((k - 1) as usize);
    let mut cum: u64 = 0;
    let mut next_cut = 1u64;
    for &(score, w) in &scored {
        cum += w as u64;
        while next_cut < k as u64 && cum * k as u64 >= total_weight * next_cut {
            thresholds.push(score as f64);
            next_cut += 1;
        }
    }
    while thresholds.len() < (k - 1) as usize {
        thresholds.push(f64::INFINITY);
    }
    thresholds
}

/// Bin index of `score`: number of thresholds it strictly exceeds.
fn bucket_of(thresholds: &[f64], score: f32) -> u32 {
    thresholds.partition_point(|&t| (score as f64) > t) as u32
}

/// Builds one street's table: parallel per-board scoring (the expensive
/// part, amortized across every hero), then a global percentile cut, then a
/// cheap second pass mapping scores to bucket indices.
fn build_table(
    boards: &[(Board, u32)],
    k: u32,
    key_fn: fn(&Board) -> Vec<u8>,
    score_fn: fn(&Board, &mut Sweep, &mut [f64]) -> Vec<f32>,
) -> StreetTable {
    let scored: Vec<(Vec<u8>, Vec<f32>, u32)> = boards
        .par_iter()
        .map_init(
            || (Sweep::new(), vec![0f64; NUM_COMBOS]),
            |(sweep, hs_scratch), (board, weight)| {
                let scores = score_fn(board, sweep, hs_scratch);
                (key_fn(board), scores, *weight)
            },
        )
        .collect();

    let mut global: Vec<(f32, u32)> = Vec::new();
    for (_, scores, weight) in &scored {
        global.extend(
            scores
                .iter()
                .copied()
                .filter(|s| !s.is_nan())
                .map(|s| (s, *weight)),
        );
    }
    let thresholds = percentile_thresholds(global, k);

    let mut boards_out = BTreeMap::new();
    for (key, scores, _weight) in scored {
        let mut buckets = vec![u16::MAX; NUM_COMBOS];
        for (combo, &s) in scores.iter().enumerate() {
            if !s.is_nan() {
                buckets[combo] = bucket_of(&thresholds, s) as u16;
            }
        }
        boards_out.insert(key, buckets);
    }

    StreetTable {
        thresholds,
        boards: boards_out,
    }
}

fn push_dedup(v: &mut Vec<(Board, u32)>, canon: Board) {
    for entry in v.iter_mut() {
        if entry.0 == canon {
            entry.1 += 1;
            return;
        }
    }
    v.push((canon, 1));
}

/// A canonical board with its raw enumeration multiplicity.
pub(crate) type WeightedBoards = Vec<(Board, u32)>;

/// Canonicalizes a list of literal boards (3, 4, or 5 cards each) into
/// deduplicated per-street `(Board, weight)` lists, weight being how many
/// input boards canonicalized to the same representative.
///
/// Factored out of [`Ehs2Abstraction::build_for_boards`] (which just builds
/// tables from the result) so the `blueprint` module's tests can construct
/// a small artifact set over exactly the same board subset a small test
/// abstraction was built for — `build_for_boards` doesn't otherwise expose
/// which canonical boards it ended up covering.
pub(crate) fn canonicalize_board_subset(
    boards: &[Vec<Card>],
) -> (WeightedBoards, WeightedBoards, WeightedBoards) {
    let mut flop_boards: WeightedBoards = Vec::new();
    let mut turn_boards: WeightedBoards = Vec::new();
    let mut river_boards: WeightedBoards = Vec::new();

    for board_cards in boards {
        let (flop, later) = match board_cards.len() {
            3 => (&board_cards[..3], &board_cards[3..3]),
            4 => (&board_cards[..3], &board_cards[3..4]),
            5 => (&board_cards[..3], &board_cards[3..5]),
            n => panic!("canonicalize_board_subset: board must be 3, 4, or 5 cards, got {n}"),
        };
        let query = Board::new(flop, later);
        let (canon, _) = canonicalize_board(&query);
        match board_cards.len() {
            3 => push_dedup(&mut flop_boards, canon),
            4 => push_dedup(&mut turn_boards, canon),
            5 => push_dedup(&mut river_boards, canon),
            _ => unreachable!(),
        }
    }
    (flop_boards, turn_boards, river_boards)
}

impl Ehs2Abstraction {
    /// Builds the abstraction for the given streets over ALL canonical
    /// boards of each street. Expensive (minutes in release for the full
    /// flop set) — production callers should go through
    /// [`Ehs2Abstraction::load_or_build`].
    pub fn build(params: Ehs2Params, streets: &[Street]) -> Self {
        let mut out = Ehs2Abstraction {
            params,
            flop: None,
            turn: None,
            river: None,
        };
        for &street in streets {
            match street {
                Street::Preflop => {
                    panic!("Ehs2Abstraction::build: preflop is never abstracted")
                }
                Street::Flop => {
                    let boards = canonical_flops();
                    out.flop = Some(build_table(
                        &boards,
                        params.flop_buckets,
                        flop_key,
                        flop_score_fn,
                    ));
                }
                Street::Turn => {
                    let boards = canonical_turns();
                    out.turn = Some(build_table(
                        &boards,
                        params.turn_buckets,
                        turn_key,
                        turn_score_fn,
                    ));
                }
                Street::River => {
                    let boards = canonical_rivers();
                    out.river = Some(build_table(
                        &boards,
                        params.river_buckets,
                        river_key,
                        river_score_fn,
                    ));
                }
            }
        }
        out
    }

    /// Builds only for the given boards (any subset, mixed streets).
    /// Percentile thresholds then cover just these boards — intended for
    /// tests and experiments, not production tables.
    pub fn build_for_boards(params: Ehs2Params, boards: &[Vec<Card>]) -> Self {
        let (flop_boards, turn_boards, river_boards) = canonicalize_board_subset(boards);

        let mut out = Ehs2Abstraction {
            params,
            flop: None,
            turn: None,
            river: None,
        };
        if !flop_boards.is_empty() {
            out.flop = Some(build_table(
                &flop_boards,
                params.flop_buckets,
                flop_key,
                flop_score_fn,
            ));
        }
        if !turn_boards.is_empty() {
            out.turn = Some(build_table(
                &turn_boards,
                params.turn_buckets,
                turn_key,
                turn_score_fn,
            ));
        }
        if !river_boards.is_empty() {
            out.river = Some(build_table(
                &river_boards,
                params.river_buckets,
                river_key,
                river_score_fn,
            ));
        }
        out
    }

    /// Streets this abstraction was built for.
    pub fn streets(&self) -> Vec<Street> {
        let mut v = Vec::new();
        if self.flop.is_some() {
            v.push(Street::Flop);
        }
        if self.turn.is_some() {
            v.push(Street::Turn);
        }
        if self.river.is_some() {
            v.push(Street::River);
        }
        v
    }

    /// Reads a cached table, verifying magic, version, and params.
    pub fn load(path: &Path, params: Ehs2Params) -> Result<Self, BucketCacheError> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < CACHE_HEADER_LEN || &bytes[0..8] != CACHE_MAGIC {
            return Err(BucketCacheError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != CACHE_VERSION {
            return Err(BucketCacheError::BadVersion {
                found: version,
                expected: CACHE_VERSION,
            });
        }
        let table: Ehs2Abstraction = postcard::from_bytes(&bytes[CACHE_HEADER_LEN..])?;
        if table.params != params {
            return Err(BucketCacheError::ParamsMismatch);
        }
        Ok(table)
    }

    /// Writes the table atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<(), BucketCacheError> {
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
                .unwrap_or("ehs2buckets.postcard")
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

    /// Loads from `cache` when present and valid for `params`/`streets`;
    /// otherwise builds and best-effort saves.
    pub fn load_or_build(params: Ehs2Params, streets: &[Street], cache: Option<&Path>) -> Self {
        if let Some(path) = cache
            && let Ok(table) = Self::load(path, params)
        {
            let mut wanted: Vec<Street> = streets.to_vec();
            wanted.sort();
            let mut have = table.streets();
            have.sort();
            if wanted == have {
                return table;
            }
        }
        let table = Self::build(params, streets);
        if let Some(path) = cache {
            let _ = table.save(path);
        }
        table
    }

    /// Combo-bucket row for a board already known to be canonical for its
    /// street (e.g. a member of `canonical_flops()`/[`canonical_turns`]/
    /// [`canonical_rivers`], or the output of `canonicalize_board`).
    ///
    /// This is the batched counterpart of the public per-query `bucket()`:
    /// no canonicalization happens here, so callers doing millions of
    /// lookups over the same small set of boards (`blueprint`'s T1/T2/T3/
    /// river-equity builders) canonicalize once per board and then read the
    /// whole row via combo-index slicing, instead of paying `bucket()`'s
    /// per-call canonicalization cost per combo.
    pub(crate) fn flop_row(&self, canon_flop: &Board) -> &[u16] {
        let table = self
            .flop
            .as_ref()
            .unwrap_or_else(|| panic!("Ehs2Abstraction::flop_row: flop street not built"));
        table.boards.get(&flop_key(canon_flop)).unwrap_or_else(|| {
            panic!("Ehs2Abstraction::flop_row: canonical board {canon_flop:?} not present")
        })
    }

    /// Turn counterpart of [`Ehs2Abstraction::flop_row`].
    pub(crate) fn turn_row(&self, canon_turn: &Board) -> &[u16] {
        let table = self
            .turn
            .as_ref()
            .unwrap_or_else(|| panic!("Ehs2Abstraction::turn_row: turn street not built"));
        table.boards.get(&turn_key(canon_turn)).unwrap_or_else(|| {
            panic!("Ehs2Abstraction::turn_row: canonical board {canon_turn:?} not present")
        })
    }

    /// River counterpart of [`Ehs2Abstraction::flop_row`].
    pub(crate) fn river_row(&self, canon_river: &Board) -> &[u16] {
        let table = self
            .river
            .as_ref()
            .unwrap_or_else(|| panic!("Ehs2Abstraction::river_row: river street not built"));
        table
            .boards
            .get(&river_key(canon_river))
            .unwrap_or_else(|| {
                panic!("Ehs2Abstraction::river_row: canonical board {canon_river:?} not present")
            })
    }
}

/// Canonicalizes a query board (3/4/5 cards, first three treated as the
/// unordered flop per crate convention) and returns the street it belongs
/// to along with the canonical board and the suit permutation used.
fn canonicalize_query(board: &[Card]) -> (Street, Board, SuitPerm) {
    let street = match board.len() {
        3 => Street::Flop,
        4 => Street::Turn,
        5 => Street::River,
        n => panic!("Ehs2Abstraction::bucket: board must be 3, 4, or 5 cards, got {n}"),
    };
    let query = Board::new(&board[..3], &board[3..]);
    let (canon, perm) = canonicalize_board(&query);
    (street, canon, perm)
}

fn lookup(table: &StreetTable, key: &[u8], combo: usize, board: &[Card], orig_combo: usize) -> u32 {
    let buckets = table.boards.get(key).unwrap_or_else(|| {
        panic!(
            "Ehs2Abstraction::bucket: board {board:?} not present in the built table \
             (was it included in build/build_for_boards?)"
        )
    });
    let b = buckets[combo];
    assert!(
        b != u16::MAX,
        "Ehs2Abstraction::bucket: combo {orig_combo} overlaps board {board:?}"
    );
    b as u32
}

impl CardAbstraction for Ehs2Abstraction {
    fn num_buckets(&self, street: Street) -> u32 {
        match street {
            Street::Preflop => {
                panic!("Ehs2Abstraction::num_buckets: preflop is never abstracted")
            }
            Street::Flop => self.params.flop_buckets,
            Street::Turn => self.params.turn_buckets,
            Street::River => self.params.river_buckets,
        }
    }

    fn bucket(&self, board: &[Card], combo: usize) -> u32 {
        let (street, canon, perm) = canonicalize_query(board);
        let permuted_combo = permute_combo(&perm, combo);
        match street {
            Street::Flop => {
                let table = self
                    .flop
                    .as_ref()
                    .unwrap_or_else(|| panic!("Ehs2Abstraction::bucket: flop street not built"));
                lookup(table, &flop_key(&canon), permuted_combo, board, combo)
            }
            Street::Turn => {
                let table = self
                    .turn
                    .as_ref()
                    .unwrap_or_else(|| panic!("Ehs2Abstraction::bucket: turn street not built"));
                lookup(table, &turn_key(&canon), permuted_combo, board, combo)
            }
            Street::River => {
                let table = self
                    .river
                    .as_ref()
                    .unwrap_or_else(|| panic!("Ehs2Abstraction::bucket: river street not built"));
                lookup(table, &river_key(&canon), permuted_combo, board, combo)
            }
            Street::Preflop => unreachable!("canonicalize_query never returns Preflop"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cards::combo_index;
    use hand_index::{all_suit_perms, permute_card};

    fn parse(s: &str) -> Vec<Card> {
        s.split_whitespace().map(|c| c.parse().unwrap()).collect()
    }

    #[test]
    fn canonical_turn_count_pin() {
        // NOTE: 63,193, not 16,432. `hand_index::canonical_unordered_board_count(4)`
        // (16,432) quotients raw *unordered 4-card sets* (raw space C(52,4)=270,725)
        // by suit permutation, treating all four cards as interchangeable. This
        // crate's `canonicalize_board` preserves the flop/turn *role* distinction
        // (raw space is C(52,3)*49=1,082,900 flop-set+turn-card pairs, per the
        // module docs' own accounting) — a suit permutation can recolor cards but
        // never reassigns which physical card holds the "turn" role, so it is a
        // strictly finer quotient. 16,432 is below the theoretical minimum orbit
        // count for a 1,082,900-element space under a 24-element group
        // (1,082,900 / 24 ≈ 45,121), so it cannot be the answer for this
        // structured quotient. 63,193 is confirmed against an independent
        // brute-force canonicalization of every raw (flop, turn) pair (not just
        // canonical-flop extensions) — see the deviations note in the PR
        // description / final report.
        let turns = canonical_turns();
        assert_eq!(turns.len(), 63_193);
        let total: u64 = turns.iter().map(|&(_, w)| w as u64).sum();
        // Raw (flop-set, turn) pairs: C(52, 3) * 49.
        assert_eq!(total, 22_100 * 49);
    }

    #[test]
    fn build_for_boards_suit_perm_invariance() {
        let flops = ["2c 7d Kh", "As Ks Qs", "9c 9d 2h", "Th 9h 8h", "3c 3d 3h"];
        let boards: Vec<Vec<Card>> = flops.iter().map(|s| parse(s)).collect();
        let params = Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, &boards);

        let board = parse("2c 7d Kh");
        let combo = combo_index("Ah".parse().unwrap(), "Ad".parse().unwrap());
        let base_bucket = abs.bucket(&board, combo);

        // Permute the whole situation (board + hole) by a nontrivial suit
        // permutation and confirm the bucket is unchanged — the trait's
        // core contract.
        for perm in all_suit_perms() {
            if perm == [0, 1, 2, 3] {
                continue;
            }
            let permuted_board: Vec<Card> = board.iter().map(|&c| permute_card(&perm, c)).collect();
            let permuted_combo = permute_combo(&perm, combo);
            let bucket = abs.bucket(&permuted_board, permuted_combo);
            assert_eq!(
                bucket, base_bucket,
                "bucket changed under suit perm {perm:?}"
            );
        }
    }

    #[test]
    fn monotonicity_smoke_on_a_fixed_flop() {
        let board = parse("2c 7d Kh");
        let params = Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, std::slice::from_ref(&board));

        let nutty = combo_index("Kd".parse().unwrap(), "Kc".parse().unwrap()); // trip kings
        let trash = combo_index("2h".parse().unwrap(), "3s".parse().unwrap()); // weak pair-ish
        assert!(abs.bucket(&board, nutty) >= abs.bucket(&board, trash));
    }

    #[test]
    fn every_live_combo_gets_a_bucket_below_k() {
        let board = parse("2c 7d Kh");
        let k = 4;
        let params = Ehs2Params {
            flop_buckets: k,
            turn_buckets: k,
            river_buckets: k,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, std::slice::from_ref(&board));
        let dead: CardSet = board.iter().copied().collect();
        for combo in 0..NUM_COMBOS {
            let (c1, c2) = combo_cards(combo);
            if dead.contains(c1) || dead.contains(c2) {
                continue;
            }
            let b = abs.bucket(&board, combo);
            assert!(b < k, "bucket {b} not below k={k}");
        }
    }

    #[test]
    #[should_panic(expected = "overlaps board")]
    fn dead_combo_panics() {
        let board = parse("2c 7d Kh");
        let params = Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, std::slice::from_ref(&board));
        let dead_combo = combo_index("2c".parse().unwrap(), "9s".parse().unwrap());
        abs.bucket(&board, dead_combo);
    }

    #[test]
    fn cache_round_trip() {
        let boards: Vec<Vec<Card>> = ["2c 7d Kh", "As Ks Qs"].iter().map(|s| parse(s)).collect();
        let params = Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, &boards);

        let path = std::env::temp_dir().join(format!(
            "abstraction-cache-test-{}-{}.postcard",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        abs.save(&path).unwrap();
        let loaded = Ehs2Abstraction::load(&path, params).unwrap();

        for board in &boards {
            let dead: CardSet = board.iter().copied().collect();
            for combo in 0..NUM_COMBOS {
                let (c1, c2) = combo_cards(combo);
                if dead.contains(c1) || dead.contains(c2) {
                    continue;
                }
                assert_eq!(abs.bucket(board, combo), loaded.bucket(board, combo));
            }
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_bad_magic() {
        let path = std::env::temp_dir().join(format!(
            "abstraction-cache-badmagic-{}.postcard",
            std::process::id()
        ));
        std::fs::write(&path, b"NOTMAGIC").unwrap();
        let params = Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        };
        assert!(matches!(
            Ehs2Abstraction::load(&path, params),
            Err(BucketCacheError::BadMagic)
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_bad_version() {
        let path = std::env::temp_dir().join(format!(
            "abstraction-cache-badversion-{}.postcard",
            std::process::id()
        ));
        let mut buf = Vec::new();
        buf.extend_from_slice(CACHE_MAGIC);
        buf.extend_from_slice(&99u16.to_le_bytes());
        std::fs::write(&path, &buf).unwrap();
        let params = Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        };
        match Ehs2Abstraction::load(&path, params) {
            Err(BucketCacheError::BadVersion { found, expected }) => {
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
        let board = parse("2c 7d Kh");
        let params = Ehs2Params {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, &[board]);
        let path = std::env::temp_dir().join(format!(
            "abstraction-cache-mismatch-{}.postcard",
            std::process::id()
        ));
        abs.save(&path).unwrap();
        let other_params = Ehs2Params {
            flop_buckets: 8,
            turn_buckets: 4,
            river_buckets: 4,
        };
        assert!(matches!(
            Ehs2Abstraction::load(&path, other_params),
            Err(BucketCacheError::ParamsMismatch)
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    #[ignore = "full-street EHS² build; CI runs it in release with --include-ignored"]
    fn flop_street_full_build_small_k() {
        let params = Ehs2Params {
            flop_buckets: 8,
            turn_buckets: 8,
            river_buckets: 8,
        };
        let start = std::time::Instant::now();
        let abs = Ehs2Abstraction::build(params, &[Street::Flop]);
        let elapsed = start.elapsed();
        eprintln!("Ehs2Abstraction::build(Flop, k=8) took {elapsed:?}");

        let flops = canonical_flops();
        assert_eq!(flops.len(), 1_755);

        let table = abs.flop.as_ref().unwrap();
        assert_eq!(
            table.boards.len(),
            flops.len(),
            "every canonical flop has a table"
        );

        // Thresholds nondecreasing.
        for w in table.thresholds.windows(2) {
            assert!(
                w[0] <= w[1],
                "thresholds not nondecreasing: {:?}",
                table.thresholds
            );
        }

        // Bucket masses balanced within 20% of each other.
        let mut mass = vec![0u64; params.flop_buckets as usize];
        for (flop_board, weight) in &flops {
            let key = flop_key(flop_board);
            let buckets = &table.boards[&key];
            for &b in buckets {
                if b != u16::MAX {
                    mass[b as usize] += *weight as u64;
                }
            }
        }
        let avg = mass.iter().sum::<u64>() as f64 / mass.len() as f64;
        for (b, &m) in mass.iter().enumerate() {
            let ratio = m as f64 / avg;
            assert!(
                (0.8..=1.2).contains(&ratio),
                "bucket {b} mass {m} deviates >20% from average {avg}"
            );
        }
    }
}
