//! Multiway-aware private-card abstraction.
//!
//! Buckets are queried from a *physical* sampled world, after the public
//! board for the current street has been revealed.  The active-opponent
//! count is part of every query so an abstraction can distinguish, for
//! example, heads-up river strength from strength against five ranges.

use abstraction::{CACHE_FORMAT_VERSION, CardAbstraction, Ehs2Abstraction, Ehs2Params};
use cards::{Card, NUM_CLASSES, NUM_COMBOS, class_index, combo_cards, rank_of};

use crate::types::Street;

pub type BucketId = u32;

#[derive(Clone, Copy, Debug)]
pub struct BucketContext<'a> {
    pub street: Street,
    pub board: &'a [Card],
    pub combo: usize,
    /// Number of non-folded opponents, excluding the player being bucketed.
    pub active_opponents: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketPath {
    pub preflop: BucketId,
    pub flop: BucketId,
    pub turn: BucketId,
    pub river: BucketId,
}

impl BucketPath {
    pub fn get(self, street: Street) -> BucketId {
        match street {
            Street::Preflop => self.preflop,
            Street::Flop => self.flop,
            Street::Turn => self.turn,
            Street::River => self.river,
        }
    }

    pub fn as_array(self) -> [BucketId; 4] {
        [self.preflop, self.flop, self.turn, self.river]
    }
}

/// Effective postflop bucket budgets for one active-opponent count. Builder
/// overrides use this type while [`RolloutKMeansParams`] remains the global
/// fallback for concise configurations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StreetBucketCounts {
    pub flop: u32,
    pub turn: u32,
    pub river: u32,
}

impl StreetBucketCounts {
    pub fn get(self, street: Street) -> u32 {
        match street {
            Street::Preflop => NUM_CLASSES as u32,
            Street::Flop => self.flop,
            Street::Turn => self.turn,
            Street::River => self.river,
        }
    }
}

/// Street-by-street abstraction suitable for correlated multiway samples.
pub trait MultiwayAbstraction: Send + Sync {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32;

    fn bucket(&self, context: BucketContext<'_>) -> BucketId;

    /// Buckets many hole combos that share one (street, board, active-opponent)
    /// context. Semantically identical to calling [`Self::bucket`] per combo;
    /// implementations may share work across the batch.
    fn bucket_batch(
        &self,
        street: Street,
        board: &[Card],
        active_opponents: u8,
        combos: &[usize],
    ) -> Vec<BucketId> {
        combos
            .iter()
            .map(|&combo| {
                self.bucket(BucketContext {
                    street,
                    board,
                    combo,
                    active_opponents,
                })
            })
            .collect()
    }

    /// Content identity used to reject incompatible checkpoints.
    fn fingerprint(&self) -> [u8; 32];

    /// Computes all four buckets from the same hole cards and shared runout.
    fn full_path(&self, combo: usize, runout: &[Card; 5], active_opponents: [u8; 4]) -> BucketPath {
        BucketPath {
            preflop: self.bucket(BucketContext {
                street: Street::Preflop,
                board: &[],
                combo,
                active_opponents: active_opponents[0],
            }),
            flop: self.bucket(BucketContext {
                street: Street::Flop,
                board: &runout[..3],
                combo,
                active_opponents: active_opponents[1],
            }),
            turn: self.bucket(BucketContext {
                street: Street::Turn,
                board: &runout[..4],
                combo,
                active_opponents: active_opponents[2],
            }),
            river: self.bucket(BucketContext {
                street: Street::River,
                board: runout,
                combo,
                active_opponents: active_opponents[3],
            }),
        }
    }
}

/// Bucket counts for the deterministic feature-hash baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureHashParams {
    pub flop_buckets: u32,
    pub turn_buckets: u32,
    pub river_buckets: u32,
}

impl Default for FeatureHashParams {
    fn default() -> Self {
        Self {
            flop_buckets: 1_024,
            turn_buckets: 1_024,
            river_buckets: 2_048,
        }
    }
}

/// A cheap, deterministic, suit-invariant baseline abstraction.
///
/// This is intentionally a baseline rather than an equity claim.  It hashes
/// poker-relevant features (made-hand rank, rank texture, canonical suit
/// texture, hole structure, and opponent count) into a fixed street budget.
/// A rollout- or clustering-based abstraction can replace it through
/// [`MultiwayAbstraction`] without changing the solver.
#[derive(Clone, Debug)]
pub struct FeatureHashAbstraction {
    params: FeatureHashParams,
    fingerprint: [u8; 32],
}

impl FeatureHashAbstraction {
    pub fn new(params: FeatureHashParams) -> Result<Self, AbstractionError> {
        for (street, count) in [
            (Street::Flop, params.flop_buckets),
            (Street::Turn, params.turn_buckets),
            (Street::River, params.river_buckets),
        ] {
            if count == 0 {
                return Err(AbstractionError::ZeroBuckets { street });
            }
        }

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.feature-hash.v1");
        hasher.update(&params.flop_buckets.to_le_bytes());
        hasher.update(&params.turn_buckets.to_le_bytes());
        hasher.update(&params.river_buckets.to_le_bytes());
        let fingerprint = *hasher.finalize().as_bytes();
        Ok(Self {
            params,
            fingerprint,
        })
    }

    pub fn params(&self) -> FeatureHashParams {
        self.params
    }

    fn bucket_count(&self, street: Street) -> u32 {
        match street {
            Street::Preflop => NUM_CLASSES as u32,
            Street::Flop => self.params.flop_buckets,
            Street::Turn => self.params.turn_buckets,
            Street::River => self.params.river_buckets,
        }
    }
}

impl Default for FeatureHashAbstraction {
    fn default() -> Self {
        Self::new(FeatureHashParams::default()).expect("default bucket counts are non-zero")
    }
}

impl MultiwayAbstraction for FeatureHashAbstraction {
    fn num_buckets(&self, street: Street, _active_opponents: u8) -> u32 {
        self.bucket_count(street)
    }

    fn bucket(&self, context: BucketContext<'_>) -> BucketId {
        validate_context(context);
        let (hi, lo) = combo_cards(context.combo);
        if context.street == Street::Preflop {
            return class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit()) as u32;
        }

        let features = invariant_features(context, hi, lo);
        let digest = blake3::hash(&features);
        let value = u64::from_le_bytes(digest.as_bytes()[..8].try_into().expect("eight bytes"));
        (value % u64::from(self.bucket_count(context.street))) as u32
    }

    fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
}

/// Adapts an existing board/combo table to the multiway interface.
///
/// Existing tables do not model opponent count, so this adapter ignores that
/// field.  The caller supplies a content fingerprint (normally the hash of
/// the table cache) to keep checkpoint compatibility explicit.
#[derive(Clone, Debug)]
pub struct TableAbstractionAdapter<A> {
    inner: A,
    fingerprint: [u8; 32],
}

impl<A> TableAbstractionAdapter<A> {
    pub fn new(inner: A, fingerprint: [u8; 32]) -> Self {
        Self { inner, fingerprint }
    }

    pub fn inner(&self) -> &A {
        &self.inner
    }

    pub fn into_inner(self) -> A {
        self.inner
    }
}

impl<A: CardAbstraction> MultiwayAbstraction for TableAbstractionAdapter<A> {
    fn num_buckets(&self, street: Street, _active_opponents: u8) -> u32 {
        match street {
            Street::Preflop => NUM_CLASSES as u32,
            _ => self.inner.num_buckets(to_cards_street(street)),
        }
    }

    fn bucket(&self, context: BucketContext<'_>) -> BucketId {
        validate_context(context);
        if context.street == Street::Preflop {
            let (hi, lo) = combo_cards(context.combo);
            class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit()) as u32
        } else {
            self.inner.bucket(context.board, context.combo)
        }
    }

    fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
}

/// Statically-dispatched union of the multiway card abstractions, so game
/// and session types stay concrete.
///
/// [`Self::Ehs2Table`] is the only backend a production config can select.
/// [`Self::FeatureHash`] is the cheap deterministic baseline: building an
/// EHS² table costs minutes, so tests that need a real session -- rather
/// than a real abstraction -- use the baseline instead.
pub enum MultiwayAbstractionBackend {
    Ehs2Table(TableAbstractionAdapter<Ehs2Abstraction>),
    FeatureHash(FeatureHashAbstraction),
}

impl MultiwayAbstraction for MultiwayAbstractionBackend {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
        match self {
            Self::Ehs2Table(inner) => inner.num_buckets(street, active_opponents),
            Self::FeatureHash(inner) => inner.num_buckets(street, active_opponents),
        }
    }

    fn bucket(&self, context: BucketContext<'_>) -> BucketId {
        match self {
            Self::Ehs2Table(inner) => inner.bucket(context),
            Self::FeatureHash(inner) => inner.bucket(context),
        }
    }

    fn bucket_batch(
        &self,
        street: Street,
        board: &[Card],
        active_opponents: u8,
        combos: &[usize],
    ) -> Vec<BucketId> {
        match self {
            Self::Ehs2Table(inner) => inner.bucket_batch(street, board, active_opponents, combos),
            Self::FeatureHash(inner) => inner.bucket_batch(street, board, active_opponents, combos),
        }
    }

    fn fingerprint(&self) -> [u8; 32] {
        match self {
            Self::Ehs2Table(inner) => inner.fingerprint(),
            Self::FeatureHash(inner) => inner.fingerprint(),
        }
    }
}

/// Content fingerprint for the ehs2-table backend
/// ([`TableAbstractionAdapter<Ehs2Abstraction>`]): a domain separator, the
/// three bucket counts, and the abstraction crate's on-disk table-format
/// version. `Ehs2Abstraction::build` is a pure function of `params` alone,
/// so `params` plus the format version fully identify content -- but
/// folding in the version means the fingerprint still changes if the table
/// semantics ever bump, the same guarantee `rollout_fingerprint` gives the
/// rollout backend by hashing the trained centroids directly.
pub fn ehs2_table_fingerprint(params: Ehs2Params) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.ehs2-table.v1");
    hasher.update(&params.flop_buckets.to_le_bytes());
    hasher.update(&params.turn_buckets.to_le_bytes());
    hasher.update(&params.river_buckets.to_le_bytes());
    hasher.update(&CACHE_FORMAT_VERSION.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn validate_context(context: BucketContext<'_>) {
    assert!(context.combo < NUM_COMBOS, "combo index out of range");
    assert!(
        context.active_opponents <= 8,
        "active opponent count exceeds 9-max"
    );
    let expected_board_len = match context.street {
        Street::Preflop => 0,
        Street::Flop => 3,
        Street::Turn => 4,
        Street::River => 5,
    };
    assert_eq!(
        context.board.len(),
        expected_board_len,
        "board length does not match street"
    );
    let (a, b) = combo_cards(context.combo);
    assert!(
        context.board.iter().all(|&card| card != a && card != b),
        "hole cards collide with board"
    );
}

fn invariant_features(context: BucketContext<'_>, hi: Card, lo: Card) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(64);
    bytes.extend_from_slice(b"mwfh1");
    bytes.push(street_index(context.street));
    bytes.push(context.active_opponents);
    bytes.extend_from_slice(
        &rank_of(context.board.iter().copied().chain([hi, lo]))
            .0
            .to_le_bytes(),
    );

    // Hole-card structure is encoded without absolute suit labels.
    bytes.push(hi.rank());
    bytes.push(lo.rank());
    bytes.push(u8::from(hi.suit() == lo.suit()));
    bytes.push(hi.rank().abs_diff(lo.rank()));

    let mut board_rank_counts = [0u8; 13];
    let mut all_rank_counts = [0u8; 13];
    let mut rank_mask = 0u16;
    for &card in context.board {
        board_rank_counts[card.rank() as usize] += 1;
        all_rank_counts[card.rank() as usize] += 1;
        rank_mask |= 1 << card.rank();
    }
    for card in [hi, lo] {
        all_rank_counts[card.rank() as usize] += 1;
        rank_mask |= 1 << card.rank();
    }
    bytes.extend_from_slice(&board_rank_counts);
    bytes.extend_from_slice(&all_rank_counts);
    bytes.extend_from_slice(&rank_mask.to_le_bytes());

    // Sort per-suit (board count, hole count) pairs.  The resulting texture
    // is invariant under every global permutation of the four suits while
    // retaining flush and backdoor-flush structure.
    let mut suit_texture = [[0u8; 2]; 4];
    for &card in context.board {
        suit_texture[card.suit() as usize][0] += 1;
    }
    for card in [hi, lo] {
        suit_texture[card.suit() as usize][1] += 1;
    }
    suit_texture.sort_unstable();
    for texture in suit_texture {
        bytes.extend_from_slice(&texture);
    }
    bytes
}

fn street_index(street: Street) -> u8 {
    match street {
        Street::Preflop => 0,
        Street::Flop => 1,
        Street::Turn => 2,
        Street::River => 3,
    }
}

fn to_cards_street(street: Street) -> cards::Street {
    match street {
        Street::Preflop => cards::Street::Preflop,
        Street::Flop => cards::Street::Flop,
        Street::Turn => cards::Street::Turn,
        Street::River => cards::Street::River,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AbstractionError {
    #[error("{street:?} bucket count must be positive")]
    ZeroBuckets { street: Street },
}

#[cfg(test)]
mod tests {
    use super::*;
    use cards::combo_index;

    fn card(text: &str) -> Card {
        text.parse().unwrap()
    }

    fn permute(card: Card) -> Card {
        card.with_suit((card.suit() + 1) % 4)
    }

    #[test]
    fn feature_hash_is_suit_isomorphic() {
        let abstraction = FeatureHashAbstraction::default();
        let combo = combo_index(card("Ah"), card("Kh"));
        let board = [card("Qh"), card("Jc"), card("2d"), card("9s")];
        let permuted_combo = combo_index(permute(card("Ah")), permute(card("Kh")));
        let permuted_board = board.map(permute);

        let a = abstraction.bucket(BucketContext {
            street: Street::Turn,
            board: &board,
            combo,
            active_opponents: 5,
        });
        let b = abstraction.bucket(BucketContext {
            street: Street::Turn,
            board: &permuted_board,
            combo: permuted_combo,
            active_opponents: 5,
        });
        assert_eq!(a, b);
    }

    #[test]
    fn full_path_uses_every_shared_board_prefix() {
        let abstraction = FeatureHashAbstraction::default();
        let combo = combo_index(card("As"), card("Kd"));
        let runout = [card("2c"), card("3d"), card("4h"), card("5s"), card("6c")];
        let path = abstraction.full_path(combo, &runout, [8, 7, 4, 1]);
        assert!(path.preflop < NUM_CLASSES as u32);
        assert!(path.flop < abstraction.params().flop_buckets);
        assert!(path.turn < abstraction.params().turn_buckets);
        assert!(path.river < abstraction.params().river_buckets);
        assert_eq!(path.as_array()[2], path.get(Street::Turn));
    }

    #[test]
    fn opponent_count_participates_in_postflop_features() {
        let abstraction = FeatureHashAbstraction::new(FeatureHashParams {
            flop_buckets: u32::MAX,
            turn_buckets: u32::MAX,
            river_buckets: u32::MAX,
        })
        .unwrap();
        let combo = combo_index(card("As"), card("Kd"));
        let board = [card("2c"), card("3d"), card("4h")];
        let heads_up = abstraction.bucket(BucketContext {
            street: Street::Flop,
            board: &board,
            combo,
            active_opponents: 1,
        });
        let six_way = abstraction.bucket(BucketContext {
            street: Street::Flop,
            board: &board,
            combo,
            active_opponents: 5,
        });
        assert_ne!(heads_up, six_way);
    }

    #[test]
    fn fingerprint_depends_on_parameters() {
        let a = FeatureHashAbstraction::default();
        let b = FeatureHashAbstraction::new(FeatureHashParams {
            river_buckets: 4_096,
            ..FeatureHashParams::default()
        })
        .unwrap();
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_eq!(
            a.fingerprint(),
            FeatureHashAbstraction::default().fingerprint()
        );
    }
}
