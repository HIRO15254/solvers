//! Multiway-aware private-card abstraction.
//!
//! Buckets are queried from a *physical* sampled world, after the public
//! board for the current street has been revealed.  The active-opponent
//! count is part of every query so an abstraction can distinguish, for
//! example, heads-up river strength from strength against five ranges.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;

use abstraction::{CACHE_FORMAT_VERSION, CardAbstraction, Ehs2Abstraction, Ehs2Params};
use cards::{
    ALL_CARDS, Card, CardSet, HandRank, NUM_CLASSES, NUM_COMBOS, class_index, combo_cards,
    combo_index, rank_of,
};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha20Rng;
use rustc_hash::FxHashMap;

use crate::types::Street;

pub type BucketId = u32;

const ROLLOUT_ARTIFACT_MAGIC: &[u8; 8] = b"SLVRMWAB";
/// Bumped from version 2 because [`RolloutKey`]'s field order (now
/// board-major, see the type doc comment) and the rollout-feature
/// computation itself (the v2 board-shared sample stream, see
/// `rollout_features_for_key`) both changed. The payload schema is
/// otherwise unchanged from version 2's
/// (params/training/bucket_counts/sets/fingerprint/assignment_cache).
pub const ROLLOUT_ARTIFACT_VERSION: u16 = 3;
const ROLLOUT_ARTIFACT_HEADER_LEN: usize = 8 + 2 + 8 + 32;
const MAX_ROLLOUT_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct BucketContext<'a> {
    pub street: Street,
    pub board: &'a [Card],
    pub combo: usize,
    /// Number of non-folded opponents, excluding the player being bucketed.
    pub active_opponents: u8,
}

/// Adds (relative to a centroids-only artifact) the memoized assignment
/// cache, serialized as a key-sorted `Vec` so the artifact bytes stay
/// deterministic. Assignments are a pure function of `(centroids, key)`, so
/// the cache can never change a result -- it is only ever a warm start for
/// [`RolloutKMeansAbstraction::bucket`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct RolloutArtifactPayload {
    params: RolloutKMeansParams,
    training: RolloutTrainingParams,
    bucket_counts: [StreetBucketCounts; 8],
    sets: Vec<CentroidSet>,
    fingerprint: [u8; 32],
    assignment_cache: Vec<(RolloutKey, BucketId)>,
}

/// Serializes `artifact`, dropping the tail of its (already key-sorted)
/// `assignment_cache` deterministically until the result fits `cap` if it
/// does not already. Centroids/params are never dropped: only cache
/// entries, which are pure memoization and therefore safe to truncate.
/// Returns `TooLarge` only when even the empty-cache payload exceeds `cap`.
fn build_artifact_payload(
    mut artifact: RolloutArtifactPayload,
    cap: u64,
) -> Result<Vec<u8>, RolloutArtifactError> {
    let payload = postcard::to_allocvec(&artifact)?;
    if payload.len() as u64 <= cap {
        return Ok(payload);
    }
    let original_count = artifact.assignment_cache.len();
    let mut base_artifact = artifact.clone();
    base_artifact.assignment_cache.clear();
    let base_payload = postcard::to_allocvec(&base_artifact)?;
    let base_len = base_payload.len() as u64;
    if base_len > cap {
        return Err(RolloutArtifactError::TooLarge {
            declared: base_len,
            limit: cap,
        });
    }
    if original_count == 0 {
        return Ok(base_payload);
    }
    let per_entry = (payload.len() as u64 - base_len) as f64 / original_count as f64;
    let mut estimated_count = if per_entry > 0.0 {
        (((cap - base_len) as f64) / per_entry).floor() as usize
    } else {
        original_count
    };
    estimated_count = estimated_count.min(original_count);
    loop {
        artifact.assignment_cache.truncate(estimated_count);
        let candidate = postcard::to_allocvec(&artifact)?;
        if candidate.len() as u64 <= cap || estimated_count == 0 {
            return Ok(candidate);
        }
        estimated_count = ((estimated_count as f64) * 0.9).floor() as usize;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketPath {
    pub preflop: BucketId,
    pub flop: BucketId,
    pub turn: BucketId,
    pub river: BucketId,
}

fn validate_centroid_sets(
    bucket_counts: &[StreetBucketCounts; 8],
    sets: &[CentroidSet],
) -> Result<(), RolloutArtifactError> {
    if sets.len() != 24 {
        return Err(RolloutArtifactError::InvalidStructure(
            "artifact must contain 24 street/opponent centroid sets",
        ));
    }
    let mut index = 0;
    for street in [Street::Flop, Street::Turn, Street::River] {
        for active_opponents in 1..=8 {
            let set = &sets[index];
            index += 1;
            if set.street != street || set.active_opponents != active_opponents {
                return Err(RolloutArtifactError::InvalidStructure(
                    "centroid sets are missing or out of canonical order",
                ));
            }
            if set.centroids.len()
                != bucket_counts[usize::from(active_opponents - 1)].get(street) as usize
            {
                return Err(RolloutArtifactError::InvalidStructure(
                    "centroid count does not match street bucket count",
                ));
            }
            for centroid in &set.centroids {
                let values = centroid.as_array();
                if values
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                    || centroid.expected_share_squared > centroid.expected_pot_share
                    || centroid.scoop_probability + centroid.tie_probability > 1.0 + 1.0e-12
                {
                    return Err(RolloutArtifactError::InvalidStructure(
                        "centroid contains invalid rollout features",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_street_bucket_counts(
    counts: StreetBucketCounts,
) -> Result<(), RolloutAbstractionError> {
    for (street, buckets) in [
        (Street::Flop, counts.flop),
        (Street::Turn, counts.turn),
        (Street::River, counts.river),
    ] {
        if buckets == 0 {
            return Err(RolloutAbstractionError::ZeroBuckets { street });
        }
    }
    Ok(())
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

impl From<RolloutKMeansParams> for StreetBucketCounts {
    fn from(params: RolloutKMeansParams) -> Self {
        Self {
            flop: params.flop_buckets,
            turn: params.turn_buckets,
            river: params.river_buckets,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RolloutArtifactError {
    #[error("rollout artifact is shorter than its header")]
    Truncated,
    #[error("rollout artifact has bad magic bytes")]
    BadMagic,
    #[error("rollout artifact version {found} is unsupported (expected {expected})")]
    UnsupportedVersion { found: u16, expected: u16 },
    #[error("rollout artifact length is invalid")]
    Length,
    #[error("rollout artifact payload {declared} exceeds limit {limit}")]
    TooLarge { declared: u64, limit: u64 },
    #[error("rollout artifact checksum mismatch")]
    ChecksumMismatch,
    #[error("rollout artifact fingerprint mismatch")]
    FingerprintMismatch,
    #[error("rollout artifact parameters do not match the builder")]
    ParameterMismatch,
    #[error("invalid rollout artifact: {0}")]
    InvalidStructure(&'static str),
    #[error(transparent)]
    Parameters(#[from] RolloutAbstractionError),
    #[error("rollout artifact codec error: {0}")]
    Codec(#[from] postcard::Error),
    #[error("rollout artifact io error: {0}")]
    Io(#[from] std::io::Error),
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

/// Four rollout statistics used for multiway clustering.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RolloutFeatures {
    /// Expected fraction of the pot won, including split pots.
    pub expected_pot_share: f64,
    pub expected_share_squared: f64,
    /// Probability that hero is the unique winner.
    pub scoop_probability: f64,
    /// Probability that hero ties for the best hand.
    pub tie_probability: f64,
}

impl RolloutFeatures {
    pub fn as_array(self) -> [f64; 4] {
        [
            self.expected_pot_share,
            self.expected_share_squared,
            self.scoop_probability,
            self.tie_probability,
        ]
    }

    fn from_array(values: [f64; 4]) -> Self {
        Self {
            expected_pot_share: values[0],
            expected_share_squared: values[1],
            scoop_probability: values[2],
            tie_probability: values[3],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RolloutKMeansParams {
    pub flop_buckets: u32,
    pub turn_buckets: u32,
    pub river_buckets: u32,
    pub rollout_samples: u32,
    pub seed: u64,
}

impl Default for RolloutKMeansParams {
    fn default() -> Self {
        Self {
            flop_buckets: 32,
            turn_buckets: 32,
            river_buckets: 32,
            rollout_samples: 10_000,
            seed: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RolloutTrainingParams {
    pub points_per_bucket: u32,
    pub kmeans_iterations: u32,
}

impl Default for RolloutTrainingParams {
    fn default() -> Self {
        Self {
            points_per_bucket: 8,
            kmeans_iterations: 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct CentroidSet {
    street: Street,
    active_opponents: u8,
    centroids: Vec<RolloutFeatures>,
}

/// Explicit builder for the expensive rollout/training phase.
#[derive(Clone, Copy, Debug)]
pub struct RolloutKMeansBuilder {
    params: RolloutKMeansParams,
    training: RolloutTrainingParams,
    bucket_overrides: [Option<StreetBucketCounts>; 8],
}

impl RolloutKMeansBuilder {
    pub fn new(params: RolloutKMeansParams) -> Self {
        Self {
            params,
            training: RolloutTrainingParams::default(),
            bucket_overrides: [None; 8],
        }
    }

    pub fn points_per_bucket(mut self, points: u32) -> Self {
        self.training.points_per_bucket = points;
        self
    }

    pub fn active_opponent_buckets(
        mut self,
        active_opponents: u8,
        counts: StreetBucketCounts,
    ) -> Result<Self, RolloutAbstractionError> {
        if !(1..=8).contains(&active_opponents) {
            return Err(RolloutAbstractionError::InvalidActiveOpponents(
                active_opponents,
            ));
        }
        validate_street_bucket_counts(counts)?;
        self.bucket_overrides[usize::from(active_opponents - 1)] = Some(counts);
        Ok(self)
    }

    pub fn effective_bucket_counts(self) -> [StreetBucketCounts; 8] {
        let fallback = StreetBucketCounts::from(self.params);
        std::array::from_fn(|index| self.bucket_overrides[index].unwrap_or(fallback))
    }

    pub fn kmeans_iterations(mut self, iterations: u32) -> Self {
        self.training.kmeans_iterations = iterations;
        self
    }

    /// Loads a previously trained artifact only when its construction
    /// parameters exactly match this builder.
    pub fn load_artifact(
        self,
        path: &Path,
    ) -> Result<RolloutKMeansAbstraction, RolloutArtifactError> {
        let abstraction = RolloutKMeansAbstraction::read_artifact(path)?;
        if abstraction.params != self.params
            || abstraction.training != self.training
            || abstraction.bucket_counts != self.effective_bucket_counts()
        {
            return Err(RolloutArtifactError::ParameterMismatch);
        }
        Ok(abstraction)
    }

    /// Trains separate centroids for every (postflop street, 1..=8 active
    /// opponents) group. No training is performed by constructors or bucket
    /// lookup; callers opt into this heavy operation explicitly.
    pub fn build(self) -> Result<RolloutKMeansAbstraction, RolloutAbstractionError> {
        validate_rollout_params(self.params, self.training)?;
        let bucket_counts = self.effective_bucket_counts();
        for counts in bucket_counts {
            validate_street_bucket_counts(counts)?;
        }
        let mut sets = Vec::with_capacity(24);
        for street in [Street::Flop, Street::Turn, Street::River] {
            for active_opponents in 1..=8 {
                let bucket_count = bucket_counts[usize::from(active_opponents - 1)].get(street);
                let training_points = bucket_count
                    .checked_mul(self.training.points_per_bucket)
                    .ok_or(RolloutAbstractionError::TrainingSizeOverflow)?;
                let training_points = usize::try_from(training_points)
                    .map_err(|_| RolloutAbstractionError::TrainingSizeOverflow)?;
                let bucket_count = usize::try_from(bucket_count)
                    .map_err(|_| RolloutAbstractionError::TrainingSizeOverflow)?;
                let mut rng = training_rng(self.params.seed, street, active_opponents);
                let mut points = Vec::with_capacity(training_points);
                for _ in 0..training_points {
                    let mut deck: Vec<Card> = ALL_CARDS.into_iter().collect();
                    deck.shuffle(&mut rng);
                    let combo = combo_index(deck[0], deck[1]);
                    let board_len = board_len(street);
                    let board = &deck[2..2 + board_len];
                    let key = canonical_rollout_key(BucketContext {
                        street,
                        board,
                        combo,
                        active_opponents,
                    });
                    points.push(rollout_features_for_key(self.params, key));
                }
                let centroids =
                    deterministic_kmeans(&points, bucket_count, self.training.kmeans_iterations);
                sets.push(CentroidSet {
                    street,
                    active_opponents,
                    centroids,
                });
            }
        }
        let fingerprint = rollout_fingerprint(self.params, self.training, &bucket_counts, &sets);
        Ok(RolloutKMeansAbstraction {
            params: self.params,
            training: self.training,
            bucket_counts,
            sets,
            fingerprint,
            assignment_cache: Mutex::new(FxHashMap::default()),
        })
    }
}

/// Active-opponent-aware rollout abstraction with deterministic k-means
/// centroids and a lazy concrete-situation assignment cache.
#[derive(Debug)]
pub struct RolloutKMeansAbstraction {
    params: RolloutKMeansParams,
    training: RolloutTrainingParams,
    bucket_counts: [StreetBucketCounts; 8],
    sets: Vec<CentroidSet>,
    fingerprint: [u8; 32],
    assignment_cache: Mutex<FxHashMap<RolloutKey, BucketId>>,
}

impl RolloutKMeansAbstraction {
    pub fn params(&self) -> RolloutKMeansParams {
        self.params
    }

    pub fn training_params(&self) -> RolloutTrainingParams {
        self.training
    }

    pub fn bucket_counts(&self, active_opponents: u8) -> Option<StreetBucketCounts> {
        (1..=8)
            .contains(&active_opponents)
            .then(|| self.bucket_counts[usize::from(active_opponents - 1)])
    }

    pub fn centroids(&self, street: Street, active_opponents: u8) -> Option<&[RolloutFeatures]> {
        self.set(street, active_opponents)
            .map(|set| set.centroids.as_slice())
    }

    pub fn rollout_features(
        &self,
        context: BucketContext<'_>,
    ) -> Result<RolloutFeatures, RolloutAbstractionError> {
        validate_rollout_context(context)?;
        Ok(rollout_features_for_key(
            self.params,
            canonical_rollout_key(context),
        ))
    }

    pub fn assignment_cache_len(&self) -> usize {
        self.assignment_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub fn clear_assignment_cache(&self) {
        self.assignment_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// Atomically writes the trained centroid sets, plus every concrete
    /// `(key -> bucket)` assignment memoized so far. Assignments are a pure
    /// function of the validated centroids and rollout parameters, so
    /// including the cache can never change what a load-then-solve produces
    /// -- it only lets a later process skip cache misses this run already
    /// paid for. If the cache has grown enough that the serialized artifact
    /// would exceed [`MAX_ROLLOUT_ARTIFACT_BYTES`], its tail is dropped
    /// deterministically (see [`build_artifact_payload`]) rather than
    /// failing the write; centroids and params are never affected.
    pub fn write_artifact(&self, path: &Path) -> Result<(), RolloutArtifactError> {
        self.write_artifact_capped(path, MAX_ROLLOUT_ARTIFACT_BYTES)
    }

    /// Implements [`Self::write_artifact`] against an explicit cap rather
    /// than always [`MAX_ROLLOUT_ARTIFACT_BYTES`], so tests can exercise the
    /// deterministic truncation path (see [`build_artifact_payload`])
    /// without a gigabyte-scale cache.
    fn write_artifact_capped(&self, path: &Path, cap: u64) -> Result<(), RolloutArtifactError> {
        let mut assignment_cache: Vec<(RolloutKey, BucketId)> = self
            .assignment_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(&key, &bucket)| (key, bucket))
            .collect();
        assignment_cache.sort_unstable_by_key(|&(key, _)| key);
        let artifact = RolloutArtifactPayload {
            params: self.params,
            training: self.training,
            bucket_counts: self.bucket_counts,
            sets: self.sets.clone(),
            fingerprint: self.fingerprint,
            assignment_cache,
        };
        let payload = build_artifact_payload(artifact, cap)?;
        let payload_len = u64::try_from(payload.len()).map_err(|_| RolloutArtifactError::Length)?;
        let checksum = *blake3::hash(&payload).as_bytes();
        let mut header = [0u8; ROLLOUT_ARTIFACT_HEADER_LEN];
        header[..8].copy_from_slice(ROLLOUT_ARTIFACT_MAGIC);
        header[8..10].copy_from_slice(&ROLLOUT_ARTIFACT_VERSION.to_le_bytes());
        header[10..18].copy_from_slice(&payload_len.to_le_bytes());
        header[18..50].copy_from_slice(&checksum);

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let directory = parent.unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(&header)?;
        temporary.write_all(&payload)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(path)
            .map_err(|error| RolloutArtifactError::Io(error.error))?;
        Ok(())
    }

    /// Re-writes the artifact at `path` with the assignment cache as it
    /// stands right now. Intended to be called after a solve so a later
    /// process (same config, same artifact path) starts with every
    /// assignment this run already paid for. Since assignments are pure
    /// memoization, this is purely a warm-start optimization and never
    /// changes the artifact's fingerprint or any downstream solve result.
    pub fn persist_assignment_cache(&self, path: &Path) -> Result<(), RolloutArtifactError> {
        self.write_artifact(path)
    }

    /// Reads and fully validates a trained centroid artifact before making
    /// it available for bucket lookup. Only [`ROLLOUT_ARTIFACT_VERSION`] is
    /// accepted; older formats are rejected with `UnsupportedVersion` rather
    /// than silently reinterpreted, since both `RolloutKey`'s layout and the
    /// rollout-feature computation changed underneath it.
    pub fn read_artifact(path: &Path) -> Result<Self, RolloutArtifactError> {
        let mut file = std::fs::File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len < ROLLOUT_ARTIFACT_HEADER_LEN as u64 {
            return Err(RolloutArtifactError::Truncated);
        }
        let mut header = [0u8; ROLLOUT_ARTIFACT_HEADER_LEN];
        file.read_exact(&mut header)?;
        if &header[..8] != ROLLOUT_ARTIFACT_MAGIC {
            return Err(RolloutArtifactError::BadMagic);
        }
        let version = u16::from_le_bytes(header[8..10].try_into().expect("two bytes"));
        if version != ROLLOUT_ARTIFACT_VERSION {
            return Err(RolloutArtifactError::UnsupportedVersion {
                found: version,
                expected: ROLLOUT_ARTIFACT_VERSION,
            });
        }
        let payload_len = u64::from_le_bytes(header[10..18].try_into().expect("eight bytes"));
        if payload_len > MAX_ROLLOUT_ARTIFACT_BYTES {
            return Err(RolloutArtifactError::TooLarge {
                declared: payload_len,
                limit: MAX_ROLLOUT_ARTIFACT_BYTES,
            });
        }
        let expected_len = (ROLLOUT_ARTIFACT_HEADER_LEN as u64)
            .checked_add(payload_len)
            .ok_or(RolloutArtifactError::Length)?;
        if file_len != expected_len {
            return Err(RolloutArtifactError::Length);
        }
        let payload_len = usize::try_from(payload_len).map_err(|_| RolloutArtifactError::Length)?;
        let mut payload = vec![0u8; payload_len];
        file.read_exact(&mut payload)?;
        let expected_checksum: [u8; 32] = header[18..50].try_into().expect("32 bytes");
        if *blake3::hash(&payload).as_bytes() != expected_checksum {
            return Err(RolloutArtifactError::ChecksumMismatch);
        }

        let artifact: RolloutArtifactPayload = postcard::from_bytes(&payload)?;
        let (params, training, bucket_counts, sets, declared_fingerprint, assignment_cache) = (
            artifact.params,
            artifact.training,
            artifact.bucket_counts,
            artifact.sets,
            artifact.fingerprint,
            artifact.assignment_cache,
        );
        validate_rollout_params(params, training)?;
        for counts in bucket_counts {
            validate_street_bucket_counts(counts)?;
        }
        validate_centroid_sets(&bucket_counts, &sets)?;
        let fingerprint = rollout_fingerprint(params, training, &bucket_counts, &sets);
        if fingerprint != declared_fingerprint {
            return Err(RolloutArtifactError::FingerprintMismatch);
        }
        Ok(Self {
            params,
            training,
            bucket_counts,
            sets,
            fingerprint,
            assignment_cache: Mutex::new(assignment_cache.into_iter().collect()),
        })
    }

    fn set(&self, street: Street, active_opponents: u8) -> Option<&CentroidSet> {
        self.sets
            .iter()
            .find(|set| set.street == street && set.active_opponents == active_opponents)
    }
}

impl MultiwayAbstraction for RolloutKMeansAbstraction {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
        assert!((1..=8).contains(&active_opponents));
        self.bucket_counts[usize::from(active_opponents - 1)].get(street)
    }

    fn bucket(&self, context: BucketContext<'_>) -> BucketId {
        validate_context(context);
        if context.street == Street::Preflop {
            assert!((1..=8).contains(&context.active_opponents));
            let (hi, lo) = combo_cards(context.combo);
            return class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit()) as u32;
        }
        validate_rollout_context(context).expect("valid multiway rollout context");
        let key = canonical_rollout_key(context);
        if let Some(&bucket) = self
            .assignment_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
        {
            return bucket;
        }

        let features = rollout_features_for_key(self.params, key);
        let set = self
            .set(context.street, context.active_opponents)
            .expect("builder creates every street/opponent centroid set");
        let bucket = nearest_centroid(features, &set.centroids) as BucketId;
        *self
            .assignment_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(key)
            .or_insert(bucket)
    }

    /// Buckets many combos against one shared `(street, board,
    /// active_opponents)` context with a single pair of cache locks and,
    /// on a miss, one shared [`RolloutStream`] instead of one per combo --
    /// see the module doc comment on [`RolloutStream`] for why every combo
    /// against the same board can share one Monte Carlo sample stream.
    fn bucket_batch(
        &self,
        street: Street,
        board: &[Card],
        active_opponents: u8,
        combos: &[usize],
    ) -> Vec<BucketId> {
        if street == Street::Preflop {
            return combos
                .iter()
                .map(|&combo| {
                    let (hi, lo) = combo_cards(combo);
                    class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit()) as u32
                })
                .collect();
        }
        validate_rollout_shared_context(street, board, active_opponents)
            .expect("valid multiway rollout context");
        let keys: Vec<RolloutKey> = combos
            .iter()
            .map(|&combo| {
                let context = BucketContext {
                    street,
                    board,
                    combo,
                    active_opponents,
                };
                validate_context(context);
                canonical_rollout_key(context)
            })
            .collect();

        let mut buckets = vec![0 as BucketId; combos.len()];
        let mut misses = Vec::new();
        {
            let cache = self
                .assignment_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for (index, &key) in keys.iter().enumerate() {
                match cache.get(&key) {
                    Some(&bucket) => buckets[index] = bucket,
                    None => misses.push(index),
                }
            }
        }

        if let Some(&first_miss) = misses.first() {
            let board_key = keys[first_miss].board;
            debug_assert!(
                misses.iter().all(|&index| keys[index].board == board_key),
                "batch combos queried against one board must canonicalize to one board key"
            );
            let set = self
                .set(street, active_opponents)
                .expect("builder creates every street/opponent centroid set");
            let mut stream = RolloutStream::new(
                self.params,
                keys[first_miss].street,
                active_opponents,
                board_key,
            );
            // Compute every miss's bucket before taking the cache lock again:
            // the Monte Carlo work happens above, not while holding the
            // mutex, so a batch never blocks unrelated cache readers/writers
            // for the duration of a rollout.
            let mut computed = Vec::with_capacity(misses.len());
            let mut batch_buckets = FxHashMap::default();
            batch_buckets.reserve(misses.len());
            for &index in &misses {
                let key = keys[index];
                let bucket = if let Some(&bucket) = batch_buckets.get(&key) {
                    bucket
                } else {
                    let hole = [Card::from_index(key.hole[0]), Card::from_index(key.hole[1])];
                    let features = stream.features_for_hole(hole);
                    let bucket = nearest_centroid(features, &set.centroids) as BucketId;
                    batch_buckets.insert(key, bucket);
                    bucket
                };
                computed.push((index, key, bucket));
            }
            let mut cache = self
                .assignment_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for (index, key, bucket) in computed {
                buckets[index] = *cache.entry(key).or_insert(bucket);
            }
        }
        buckets
    }

    fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
}

/// Canonical suit-isomorphic rollout key: board-major so that `board` is a
/// function of the physical board alone (the minimum over all 24 suit
/// permutations), independent of which hole is being queried; `hole` is
/// then the hero's hole mapped into that same board-canonical suit space,
/// minimized only among the permutations that already minimize `board`. The
/// derived `Ord` therefore sorts by `(street, active_opponents, board,
/// hole)`, which is exactly what [`RolloutKMeansAbstraction::bucket_batch`]
/// relies on to group many combos against one physical board onto one
/// shared [`RolloutStream`].
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
struct RolloutKey {
    street: u8,
    active_opponents: u8,
    board: [u8; 5],
    hole: [u8; 2],
}

fn validate_rollout_params(
    params: RolloutKMeansParams,
    training: RolloutTrainingParams,
) -> Result<(), RolloutAbstractionError> {
    for (street, buckets) in [
        (Street::Flop, params.flop_buckets),
        (Street::Turn, params.turn_buckets),
        (Street::River, params.river_buckets),
    ] {
        if buckets == 0 {
            return Err(RolloutAbstractionError::ZeroBuckets { street });
        }
    }
    if params.rollout_samples == 0 {
        return Err(RolloutAbstractionError::ZeroRolloutSamples);
    }
    if training.points_per_bucket == 0 {
        return Err(RolloutAbstractionError::ZeroTrainingPoints);
    }
    if training.kmeans_iterations == 0 {
        return Err(RolloutAbstractionError::ZeroKMeansIterations);
    }
    Ok(())
}

/// Part of [`validate_rollout_context`] that does not depend on a specific
/// combo: shared by every combo in one [`RolloutKMeansAbstraction::bucket_batch`]
/// call so that work is paid once per batch rather than once per combo.
fn validate_rollout_shared_context(
    street: Street,
    board: &[Card],
    active_opponents: u8,
) -> Result<(), RolloutAbstractionError> {
    if street == Street::Preflop {
        return Err(RolloutAbstractionError::PostflopOnly);
    }
    if board.len() != board_len(street) {
        return Err(RolloutAbstractionError::InvalidContext(
            "wrong board length",
        ));
    }
    if !(1..=8).contains(&active_opponents) {
        return Err(RolloutAbstractionError::InvalidContext(
            "active opponents must be 1..=8",
        ));
    }
    let mut board_set = CardSet::EMPTY;
    for &card in board {
        if board_set.contains(card) {
            return Err(RolloutAbstractionError::InvalidContext(
                "board contains duplicate card",
            ));
        }
        board_set.insert(card);
    }
    Ok(())
}

fn validate_rollout_context(context: BucketContext<'_>) -> Result<(), RolloutAbstractionError> {
    validate_rollout_shared_context(context.street, context.board, context.active_opponents)?;
    if context.combo >= NUM_COMBOS {
        return Err(RolloutAbstractionError::InvalidContext(
            "combo out of range",
        ));
    }
    let (a, b) = combo_cards(context.combo);
    if context.board.iter().any(|&card| card == a || card == b) {
        return Err(RolloutAbstractionError::InvalidContext(
            "hole cards collide with board",
        ));
    }
    Ok(())
}

fn canonical_rollout_key(context: BucketContext<'_>) -> RolloutKey {
    let (a, b) = combo_cards(context.combo);
    let mut best: Option<RolloutKey> = None;
    for s0 in 0..4 {
        for s1 in 0..4 {
            if s1 == s0 {
                continue;
            }
            for s2 in 0..4 {
                if s2 == s0 || s2 == s1 {
                    continue;
                }
                for s3 in 0..4 {
                    if s3 == s0 || s3 == s1 || s3 == s2 {
                        continue;
                    }
                    let permutation = [s0, s1, s2, s3];
                    let mut hole = [
                        permute_suit(a, permutation).index() as u8,
                        permute_suit(b, permutation).index() as u8,
                    ];
                    hole.sort_unstable();
                    // Fill the mapped board into a fixed `[u8; 5]`, padding
                    // unused trailing slots with `u8::MAX`. Because every
                    // real card index is < 52, sorting the whole 5-slot
                    // array yields the same result as sorting only the
                    // valid prefix and leaving the padding in place, with
                    // no per-permutation `Vec` allocation.
                    let mut board = [u8::MAX; 5];
                    for (slot, &card) in board.iter_mut().zip(context.board) {
                        *slot = permute_suit(card, permutation).index() as u8;
                    }
                    board.sort_unstable();
                    let candidate = RolloutKey {
                        street: context.street.index() as u8,
                        active_opponents: context.active_opponents,
                        board,
                        hole,
                    };
                    if best.is_none_or(|current| candidate < current) {
                        best = Some(candidate);
                    }
                }
            }
        }
    }
    best.expect("24 suit permutations")
}

fn permute_suit(card: Card, permutation: [u8; 4]) -> Card {
    card.with_suit(permutation[card.suit() as usize])
}

/// One drawn Monte Carlo sample within a [`RolloutStream`]: the runout plus
/// opponent hands actually dealt (as a [`CardSet`], to test hero collisions
/// cheaply) and the completed 5-card board. `opponent_best` -- the
/// `(best_opponent_rank, opponents_at_best)` pair used by every hero that
/// does not collide with this sample -- is filled at most once, on first
/// use, since a sample near the tail of an over-extended stream may end up
/// never queried by any hero.
struct RolloutSample {
    dealt: CardSet,
    board_five: [Card; 5],
    /// Opponent hole cards in dealt order, `2 * active_opponents` of them
    /// meaningful (bounded by the 9-max `MAX_OPPONENT_CARDS`); unused
    /// trailing slots are never read.
    opponents: [Card; MAX_OPPONENT_CARDS],
    opponent_best: Option<(HandRank, u32)>,
}

const MAX_OPPONENT_CARDS: usize = 16;

/// A board-keyed Monte Carlo sample stream shared by every hero hole
/// queried against the same canonical `(street, active_opponents, board)`.
/// Samples are generated lazily, in a fixed order (`0, 1, 2, ...`), by one
/// continuing RNG seeded from the board alone (never the hole -- see
/// [`rollout_stream_rng`]); [`Self::ensure`] only ever appends, so which
/// heroes have been queried so far, and in what order, can never change a
/// later sample's value. [`Self::features_for_hole`] rejects (skips) any
/// sample whose dealt cards collide with the queried hero: since a
/// hero-conditioned deal is exactly "deal from a hero-excluded deck", this
/// per-key conditional distribution is identical to v1's per-key-seeded
/// rollout, and estimator quality is unaffected. Sharing the stream across
/// holes only introduces cross-key correlation between different heroes'
/// estimates on the same board, never bias in any single hero's estimate.
struct RolloutStream {
    params: RolloutKMeansParams,
    active_opponents: u8,
    rng: ChaCha20Rng,
    base_deck: Vec<Card>,
    deck: Vec<Card>,
    board_prefix: [Card; 5],
    board_len: usize,
    samples: Vec<RolloutSample>,
}

impl RolloutStream {
    fn new(params: RolloutKMeansParams, street: u8, active_opponents: u8, board: [u8; 5]) -> Self {
        let board_len = board.iter().take_while(|&&index| index != u8::MAX).count();
        let dead: CardSet = board[..board_len]
            .iter()
            .map(|&index| Card::from_index(index))
            .collect();
        let base_deck: Vec<Card> = ALL_CARDS
            .into_iter()
            .filter(|&card| !dead.contains(card))
            .collect();
        let mut board_prefix = [Card::from_index(0); 5];
        for (slot, &index) in board_prefix.iter_mut().zip(&board[..board_len]) {
            *slot = Card::from_index(index);
        }
        let deck = base_deck.clone();
        Self {
            rng: rollout_stream_rng(params, street, active_opponents, board),
            params,
            active_opponents,
            base_deck,
            deck,
            board_prefix,
            board_len,
            samples: Vec::new(),
        }
    }

    /// Generates samples up to index `len - 1` if they do not exist yet.
    /// Each sample resets the deck from `base_deck` and draws exactly the
    /// dealt cards (`missing_board + 2 * active_opponents` of them, runout
    /// first, then opponent hands) with one `partial_shuffle`: a Fisher-
    /// Yates pass cut off after the dealt prefix, which selects an
    /// unbiased ordered sample while consuming ~3x fewer RNG draws per
    /// sample than shuffling the whole remaining deck.
    fn ensure(&mut self, len: usize) {
        let missing_board = 5 - self.board_len;
        let opponent_cards = 2 * usize::from(self.active_opponents);
        while self.samples.len() < len {
            self.deck.copy_from_slice(&self.base_deck);
            let (drawn, _) = self
                .deck
                .partial_shuffle(&mut self.rng, missing_board + opponent_cards);
            let mut board_five = self.board_prefix;
            board_five[self.board_len..].copy_from_slice(&drawn[..missing_board]);
            let mut dealt = CardSet::EMPTY;
            for &card in drawn.iter() {
                dealt.insert(card);
            }
            let mut opponents = [Card::from_index(0); MAX_OPPONENT_CARDS];
            opponents[..opponent_cards]
                .copy_from_slice(&drawn[missing_board..missing_board + opponent_cards]);
            self.samples.push(RolloutSample {
                dealt,
                board_five,
                opponents,
                opponent_best: None,
            });
        }
    }

    /// Lazily computes and memoizes sample `index`'s
    /// `(best_opponent_rank, opponents_at_best)` pair. Independent of any
    /// hero hole, so every non-colliding hero shares the one computation.
    fn opponent_best(&mut self, index: usize) -> (HandRank, u32) {
        if let Some(best) = self.samples[index].opponent_best {
            return best;
        }
        let opponent_cards = 2 * usize::from(self.active_opponents);
        let board_five = self.samples[index].board_five;
        let mut best_rank: Option<HandRank> = None;
        let mut opponents_at_best = 0u32;
        for pair in self.samples[index].opponents[..opponent_cards].chunks_exact(2) {
            let rank = rank_of(board_five.iter().copied().chain([pair[0], pair[1]]));
            match best_rank {
                Some(current) if rank < current => {}
                Some(current) if rank == current => opponents_at_best += 1,
                _ => {
                    best_rank = Some(rank);
                    opponents_at_best = 1;
                }
            }
        }
        let best = (
            best_rank.expect("active_opponents is validated to be at least 1"),
            opponents_at_best,
        );
        self.samples[index].opponent_best = Some(best);
        best
    }

    /// Accumulates exactly `params.rollout_samples` non-hero-colliding
    /// samples, walking the stream from index 0 and extending it on demand.
    fn features_for_hole(&mut self, hole: [Card; 2]) -> RolloutFeatures {
        let mut sums = [0.0; 4];
        let mut collected = 0u32;
        let mut index = 0usize;
        while collected < self.params.rollout_samples {
            self.ensure(index + 1);
            if self.samples[index].dealt.contains(hole[0])
                || self.samples[index].dealt.contains(hole[1])
            {
                index += 1;
                continue;
            }
            let board_five = self.samples[index].board_five;
            let hero_rank = rank_of(board_five.iter().copied().chain(hole));
            let (best_opponent_rank, opponents_at_best) = self.opponent_best(index);
            let (share, scoop, tie) = match hero_rank.cmp(&best_opponent_rank) {
                std::cmp::Ordering::Greater => (1.0, 1.0, 0.0),
                std::cmp::Ordering::Equal => {
                    let winner_count = 1 + opponents_at_best;
                    (1.0 / f64::from(winner_count), 0.0, 1.0)
                }
                std::cmp::Ordering::Less => (0.0, 0.0, 0.0),
            };
            sums[0] += share;
            sums[1] += share * share;
            sums[2] += scoop;
            sums[3] += tie;
            collected += 1;
            index += 1;
        }
        let denominator = f64::from(self.params.rollout_samples);
        RolloutFeatures::from_array(sums.map(|sum| sum / denominator))
    }
}

/// Computes the four v2 rollout statistics for one canonical `(board,
/// hole)` key by building a fresh [`RolloutStream`] for `key.board` and
/// reading `key.hole`'s features from it. Callers that need more than one
/// hole on the same board (training and
/// [`RolloutKMeansAbstraction::bucket_batch`]) build one `RolloutStream`
/// directly instead of calling this once per hole.
fn rollout_features_for_key(params: RolloutKMeansParams, key: RolloutKey) -> RolloutFeatures {
    let hole = [Card::from_index(key.hole[0]), Card::from_index(key.hole[1])];
    let mut stream = RolloutStream::new(params, key.street, key.active_opponents, key.board);
    stream.features_for_hole(hole)
}

/// Seeds the v2 rollout stream from the board alone -- the hole is
/// deliberately never hashed, so every hole queried against the same board
/// shares one sample stream (see [`RolloutStream`]).
fn rollout_stream_rng(
    params: RolloutKMeansParams,
    street: u8,
    active_opponents: u8,
    board: [u8; 5],
) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.rollout.v2");
    hasher.update(&params.seed.to_le_bytes());
    hasher.update(&params.rollout_samples.to_le_bytes());
    hasher.update(&[street, active_opponents]);
    hasher.update(&board);
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

fn training_rng(seed: u64, street: Street, active_opponents: u8) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.rollout-training.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&[street.index() as u8, active_opponents]);
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

fn deterministic_kmeans(
    points: &[RolloutFeatures],
    bucket_count: usize,
    iterations: u32,
) -> Vec<RolloutFeatures> {
    debug_assert!(points.len() >= bucket_count && bucket_count > 0);
    let mut centroids = vec![points[0]];
    while centroids.len() < bucket_count {
        let mut farthest_index = 0;
        let mut farthest_distance = -1.0;
        for (index, &point) in points.iter().enumerate() {
            let distance = centroids
                .iter()
                .map(|&centroid| feature_distance(point, centroid))
                .fold(f64::INFINITY, f64::min);
            if distance > farthest_distance {
                farthest_index = index;
                farthest_distance = distance;
            }
        }
        centroids.push(points[farthest_index]);
    }

    for _ in 0..iterations {
        let mut sums = vec![[0.0; 4]; bucket_count];
        let mut counts = vec![0u32; bucket_count];
        for &point in points {
            let cluster = nearest_centroid(point, &centroids);
            counts[cluster] += 1;
            for (sum, value) in sums[cluster].iter_mut().zip(point.as_array()) {
                *sum += value;
            }
        }
        let mut next = centroids.clone();
        for cluster in 0..bucket_count {
            if counts[cluster] > 0 {
                let divisor = f64::from(counts[cluster]);
                next[cluster] = RolloutFeatures::from_array(sums[cluster].map(|sum| sum / divisor));
            }
        }
        if next == centroids {
            break;
        }
        centroids = next;
    }
    centroids
}

fn nearest_centroid(point: RolloutFeatures, centroids: &[RolloutFeatures]) -> usize {
    let mut best_index = 0;
    let mut best_distance = f64::INFINITY;
    for (index, &centroid) in centroids.iter().enumerate() {
        let distance = feature_distance(point, centroid);
        if distance < best_distance {
            best_index = index;
            best_distance = distance;
        }
    }
    best_index
}

fn feature_distance(a: RolloutFeatures, b: RolloutFeatures) -> f64 {
    a.as_array()
        .into_iter()
        .zip(b.as_array())
        .map(|(left, right)| (left - right).powi(2))
        .sum()
}

fn rollout_fingerprint(
    params: RolloutKMeansParams,
    training: RolloutTrainingParams,
    bucket_counts: &[StreetBucketCounts; 8],
    sets: &[CentroidSet],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.rollout-kmeans.v2");
    for value in [
        params.flop_buckets,
        params.turn_buckets,
        params.river_buckets,
        params.rollout_samples,
        training.points_per_bucket,
        training.kmeans_iterations,
    ] {
        hasher.update(&value.to_le_bytes());
    }
    hasher.update(&params.seed.to_le_bytes());
    for counts in bucket_counts {
        hasher.update(&counts.flop.to_le_bytes());
        hasher.update(&counts.turn.to_le_bytes());
        hasher.update(&counts.river.to_le_bytes());
    }
    for set in sets {
        hasher.update(&[set.street.index() as u8, set.active_opponents]);
        hasher.update(&(set.centroids.len() as u64).to_le_bytes());
        for centroid in &set.centroids {
            for value in centroid.as_array() {
                hasher.update(&value.to_bits().to_le_bytes());
            }
        }
    }
    *hasher.finalize().as_bytes()
}

fn board_len(street: Street) -> usize {
    match street {
        Street::Preflop => 0,
        Street::Flop => 3,
        Street::Turn => 4,
        Street::River => 5,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RolloutAbstractionError {
    #[error("{street:?} bucket count must be positive")]
    ZeroBuckets { street: Street },
    #[error("rollout sample count must be positive")]
    ZeroRolloutSamples,
    #[error("training points per bucket must be positive")]
    ZeroTrainingPoints,
    #[error("k-means iteration count must be positive")]
    ZeroKMeansIterations,
    #[error("training set size overflow")]
    TrainingSizeOverflow,
    #[error("rollout features are postflop only")]
    PostflopOnly,
    #[error("invalid rollout context: {0}")]
    InvalidContext(&'static str),
    #[error("active opponent count must be 1..=8, found {0}")]
    InvalidActiveOpponents(u8),
}

#[cfg(test)]
mod rollout_tests {
    use super::*;
    use std::io::{Seek, SeekFrom};

    fn card(text: &str) -> Card {
        text.parse().unwrap()
    }

    fn tiny_builder() -> RolloutKMeansBuilder {
        RolloutKMeansBuilder::new(RolloutKMeansParams {
            flop_buckets: 2,
            turn_buckets: 2,
            river_buckets: 2,
            rollout_samples: 16,
            seed: 2026,
        })
        .points_per_bucket(1)
        .kmeans_iterations(3)
    }

    #[test]
    fn rollout_training_and_assignment_are_reproducible() {
        let a = tiny_builder().build().unwrap();
        let b = tiny_builder().build().unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.centroids(Street::Turn, 6), b.centroids(Street::Turn, 6));

        let combo = combo_index(card("Ah"), card("Kh"));
        let board = [card("Qh"), card("Jc"), card("2d")];
        let context = BucketContext {
            street: Street::Flop,
            board: &board,
            combo,
            active_opponents: 3,
        };
        assert_eq!(a.rollout_features(context), b.rollout_features(context));
        assert_eq!(a.bucket(context), b.bucket(context));
    }

    #[test]
    fn rollout_assignment_is_suit_invariant_and_opponent_specific() {
        let abstraction = tiny_builder().build().unwrap();
        let combo = combo_index(card("Ah"), card("Kh"));
        let board = [card("Qh"), card("Jc"), card("2d")];
        let permute = |card: Card| card.with_suit((card.suit() + 1) % 4);
        let permuted_combo = combo_index(permute(card("Ah")), permute(card("Kh")));
        let permuted_board = board.map(permute);
        let original = BucketContext {
            street: Street::Flop,
            board: &board,
            combo,
            active_opponents: 3,
        };
        let permuted = BucketContext {
            street: Street::Flop,
            board: &permuted_board,
            combo: permuted_combo,
            active_opponents: 3,
        };
        assert_eq!(
            abstraction.rollout_features(original),
            abstraction.rollout_features(permuted)
        );
        assert_eq!(abstraction.bucket(original), abstraction.bucket(permuted));

        abstraction.clear_assignment_cache();
        let one = BucketContext {
            active_opponents: 1,
            ..original
        };
        let eight = BucketContext {
            active_opponents: 8,
            ..original
        };
        let one_features = abstraction.rollout_features(one).unwrap();
        let eight_features = abstraction.rollout_features(eight).unwrap();
        assert_ne!(one_features, eight_features);
        abstraction.bucket(one);
        abstraction.bucket(eight);
        assert_eq!(abstraction.assignment_cache_len(), 2);
        assert_eq!(abstraction.centroids(Street::Flop, 1).unwrap().len(), 2);
        assert_eq!(abstraction.centroids(Street::Flop, 8).unwrap().len(), 2);
        assert_eq!(one_features, abstraction.rollout_features(one).unwrap());
        assert!(one_features.expected_share_squared <= one_features.expected_pot_share);
    }

    /// A fixed pool of hole-card pairs mixing suited and offsuit hands
    /// across every suit, for tests that need many non-colliding combos
    /// against a fixed board.
    fn candidate_holes() -> Vec<(Card, Card)> {
        [
            ("Ah", "Kh"),
            ("Ad", "Kd"),
            ("As", "Ks"),
            ("Ac", "Kc"),
            ("Ah", "Kd"),
            ("As", "Kc"),
            ("Qd", "Qc"),
            ("Js", "Jc"),
            ("Ts", "9s"),
            ("8d", "8c"),
            ("7h", "6h"),
            ("6s", "5d"),
            ("5h", "4h"),
            ("4c", "3c"),
            ("3d", "2d"),
            ("Kc", "Qs"),
            ("Jd", "Th"),
            ("9c", "8h"),
            ("Tc", "9d"),
            ("6c", "5s"),
            ("2s", "2h"),
            ("Qh", "Jd"),
        ]
        .into_iter()
        .map(|(a, b)| (card(a), card(b)))
        .collect()
    }

    #[test]
    fn bucket_batch_matches_solo_bucket_regardless_of_cache_state_or_order() {
        let abstraction = tiny_builder().build().unwrap();
        let street = Street::Flop;
        let active_opponents = 3;
        // A suit-symmetric (monotone) board and a non-symmetric one.
        let monotone = [card("Qh"), card("Jh"), card("2h")];
        let textured = [card("9s"), card("7d"), card("2c")];

        for board in [monotone, textured] {
            let combos: Vec<usize> = candidate_holes()
                .into_iter()
                .filter(|&(a, b)| !board.contains(&a) && !board.contains(&b))
                .map(|(a, b)| combo_index(a, b))
                .take(20)
                .collect();
            assert!(combos.len() >= 15, "expected a wide non-colliding pool");

            abstraction.clear_assignment_cache();
            let batch = abstraction.bucket_batch(street, &board, active_opponents, &combos);

            abstraction.clear_assignment_cache();
            let solo: Vec<BucketId> = combos
                .iter()
                .map(|&combo| {
                    abstraction.bucket(BucketContext {
                        street,
                        board: &board,
                        combo,
                        active_opponents,
                    })
                })
                .collect();
            assert_eq!(batch, solo, "batch must match solo bucket() per combo");

            abstraction.clear_assignment_cache();
            let mut reversed = combos.clone();
            reversed.reverse();
            let mut solo_reversed: Vec<BucketId> = reversed
                .iter()
                .map(|&combo| {
                    abstraction.bucket(BucketContext {
                        street,
                        board: &board,
                        combo,
                        active_opponents,
                    })
                })
                .collect();
            solo_reversed.reverse();
            assert_eq!(
                batch, solo_reversed,
                "solo query order must not change results"
            );
        }
    }

    #[test]
    fn rollout_artifact_is_deterministic_and_integrity_checked() {
        let abstraction = tiny_builder().build().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.mwab");
        let second = directory.path().join("second.mwab");
        abstraction.write_artifact(&first).unwrap();
        abstraction.write_artifact(&second).unwrap();
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap()
        );

        let loaded = tiny_builder().load_artifact(&first).unwrap();
        assert_eq!(loaded.fingerprint(), abstraction.fingerprint());
        assert_eq!(
            loaded.centroids(Street::River, 8),
            abstraction.centroids(Street::River, 8)
        );
        assert_eq!(loaded.assignment_cache_len(), 0);

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&first)
            .unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        file.write_all(&[byte[0] ^ 0x40]).unwrap();
        file.sync_all().unwrap();
        assert!(matches!(
            RolloutKMeansAbstraction::read_artifact(&first),
            Err(RolloutArtifactError::ChecksumMismatch)
        ));
    }

    #[test]
    fn read_artifact_rejects_a_stale_version() {
        let abstraction = tiny_builder().build().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("stale.mwab");
        abstraction.write_artifact(&path).unwrap();

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        // Header bytes 8..10 are the little-endian version; the payload
        // itself is still version 3's, but only the version field is under
        // test here -- the read must fail on the version check before it
        // ever gets to decoding the payload or checking the checksum.
        file.seek(SeekFrom::Start(8)).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);

        assert!(matches!(
            RolloutKMeansAbstraction::read_artifact(&path),
            Err(RolloutArtifactError::UnsupportedVersion {
                found: 2,
                expected: 3,
            })
        ));
    }

    #[test]
    fn write_artifact_truncates_the_assignment_cache_deterministically_under_a_tiny_cap() {
        let abstraction = tiny_builder().build().unwrap();
        // Populate several hundred distinct assignment-cache entries across
        // many boards/opponent counts/combos.
        let boards: Vec<[Card; 3]> = (0u8..40)
            .map(|offset| {
                [
                    Card::from_index(offset),
                    Card::from_index((offset + 13) % 52),
                    Card::from_index((offset + 27) % 52),
                ]
            })
            .collect();
        for board in &boards {
            let dead: CardSet = board.iter().copied().collect();
            let mut queried = 0;
            for &(a, b) in &candidate_holes() {
                if dead.contains(a) || dead.contains(b) {
                    continue;
                }
                for active_opponents in 1..=3u8 {
                    abstraction.bucket(BucketContext {
                        street: Street::Flop,
                        board,
                        combo: combo_index(a, b),
                        active_opponents,
                    });
                }
                queried += 1;
                if queried >= 8 {
                    break;
                }
            }
        }
        let original_len = abstraction.assignment_cache_len();
        assert!(
            original_len > 100,
            "expected a few hundred cache entries, found {original_len}"
        );

        let mut original_sorted: Vec<(RolloutKey, BucketId)> = abstraction
            .assignment_cache
            .lock()
            .unwrap()
            .iter()
            .map(|(&key, &bucket)| (key, bucket))
            .collect();
        original_sorted.sort_unstable_by_key(|&(key, _)| key);

        let directory = tempfile::tempdir().unwrap();
        let full_path = directory.path().join("full.mwab");
        abstraction.write_artifact(&full_path).unwrap();
        let full_len = std::fs::metadata(&full_path).unwrap().len();

        let path = directory.path().join("capped.mwab");
        // Half the untruncated size: comfortably below the full cache but
        // (since centroids/params/training dominate the empty-cache base
        // for a `tiny_builder` abstraction) still well above the
        // empty-cache floor, so truncation -- not `TooLarge` -- is what
        // makes the write succeed.
        let cap = full_len / 2;
        abstraction.write_artifact_capped(&path, cap).unwrap();
        let capped_file_len = std::fs::metadata(&path).unwrap().len();
        assert!(capped_file_len <= cap + ROLLOUT_ARTIFACT_HEADER_LEN as u64);
        assert!(capped_file_len < full_len);

        let loaded = RolloutKMeansAbstraction::read_artifact(&path).unwrap();
        assert_eq!(loaded.fingerprint(), abstraction.fingerprint());
        let loaded_len = loaded.assignment_cache_len();
        assert!(loaded_len < original_len);

        let mut loaded_sorted: Vec<(RolloutKey, BucketId)> = loaded
            .assignment_cache
            .lock()
            .unwrap()
            .iter()
            .map(|(&key, &bucket)| (key, bucket))
            .collect();
        loaded_sorted.sort_unstable_by_key(|&(key, _)| key);
        assert_eq!(loaded_sorted, original_sorted[..loaded_len]);
    }

    #[test]
    fn active_opponent_bucket_overrides_are_trained_and_fingerprinted() {
        let baseline = tiny_builder().build().unwrap();
        let overridden = tiny_builder()
            .active_opponent_buckets(
                3,
                StreetBucketCounts {
                    flop: 3,
                    turn: 4,
                    river: 5,
                },
            )
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(overridden.num_buckets(Street::Flop, 2), 2);
        assert_eq!(overridden.num_buckets(Street::Flop, 3), 3);
        assert_eq!(overridden.num_buckets(Street::Turn, 3), 4);
        assert_eq!(overridden.num_buckets(Street::River, 3), 5);
        assert_ne!(baseline.fingerprint(), overridden.fingerprint());
    }

    #[test]
    fn backend_enum_delegates_to_the_wrapped_rollout_abstraction() {
        let bare = tiny_builder().build().unwrap();
        let wrapped = MultiwayAbstractionBackend::RolloutKMeans(tiny_builder().build().unwrap());

        assert_eq!(bare.fingerprint(), wrapped.fingerprint());
        assert_eq!(wrapped.rollout().unwrap().fingerprint(), bare.fingerprint());
        for active_opponents in [1u8, 4, 8] {
            for street in [Street::Flop, Street::Turn, Street::River] {
                assert_eq!(
                    bare.num_buckets(street, active_opponents),
                    wrapped.num_buckets(street, active_opponents)
                );
            }
        }

        let board = [card("Qh"), card("Jc"), card("2d")];
        let combo = combo_index(card("Ah"), card("Kh"));
        let context = BucketContext {
            street: Street::Flop,
            board: &board,
            combo,
            active_opponents: 3,
        };
        assert_eq!(bare.bucket(context), wrapped.bucket(context));

        let combos: Vec<usize> = candidate_holes()
            .into_iter()
            .filter(|&(a, b)| !board.contains(&a) && !board.contains(&b))
            .map(|(a, b)| combo_index(a, b))
            .collect();
        assert_eq!(
            bare.bucket_batch(Street::Flop, &board, 3, &combos),
            wrapped.bucket_batch(Street::Flop, &board, 3, &combos)
        );
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

/// Statically-dispatched union of the multiway abstraction backends, so
/// game/session types stay concrete and the rollout-only assignment-cache
/// persistence can be reached by callers that hold the backend.
pub enum MultiwayAbstractionBackend {
    #[cfg(any(feature = "research-abstractions", test))]
    RolloutKMeans(RolloutKMeansAbstraction),
    Ehs2Table(TableAbstractionAdapter<Ehs2Abstraction>),
}

impl MultiwayAbstractionBackend {
    /// The rollout backend, when this is one -- used to reach
    /// [`RolloutKMeansAbstraction::persist_assignment_cache`] after a solve.
    /// The ehs2-table backend has no assignment cache of its own: its
    /// content is already fully determined (and disk-cached) at build time.
    #[cfg(any(feature = "research-abstractions", test))]
    pub fn rollout(&self) -> Option<&RolloutKMeansAbstraction> {
        match self {
            MultiwayAbstractionBackend::RolloutKMeans(inner) => Some(inner),
            MultiwayAbstractionBackend::Ehs2Table(_) => None,
        }
    }
}

impl MultiwayAbstraction for MultiwayAbstractionBackend {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
        match self {
            #[cfg(any(feature = "research-abstractions", test))]
            MultiwayAbstractionBackend::RolloutKMeans(inner) => {
                inner.num_buckets(street, active_opponents)
            }
            MultiwayAbstractionBackend::Ehs2Table(inner) => {
                inner.num_buckets(street, active_opponents)
            }
        }
    }

    fn bucket(&self, context: BucketContext<'_>) -> BucketId {
        match self {
            #[cfg(any(feature = "research-abstractions", test))]
            MultiwayAbstractionBackend::RolloutKMeans(inner) => inner.bucket(context),
            MultiwayAbstractionBackend::Ehs2Table(inner) => inner.bucket(context),
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
            #[cfg(any(feature = "research-abstractions", test))]
            MultiwayAbstractionBackend::RolloutKMeans(inner) => {
                inner.bucket_batch(street, board, active_opponents, combos)
            }
            MultiwayAbstractionBackend::Ehs2Table(inner) => {
                inner.bucket_batch(street, board, active_opponents, combos)
            }
        }
    }

    fn fingerprint(&self) -> [u8; 32] {
        match self {
            #[cfg(any(feature = "research-abstractions", test))]
            MultiwayAbstractionBackend::RolloutKMeans(inner) => inner.fingerprint(),
            MultiwayAbstractionBackend::Ehs2Table(inner) => inner.fingerprint(),
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
