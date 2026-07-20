//! Indexed, read-only strategy artifact for sampled multiway solves.
//!
//! Unlike a `.mwckpt`, this format deliberately omits regrets and RNG state.
//! Blocks are sorted by their complete infoset key, allowing a viewer or the
//! bridge to binary-search one strategy without rebuilding a public tree.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Estimate, MULTIWAY_SCHEMA_VERSION, config_hash, config_hash_hex};

pub const MWSOL_HEADER_LEN: usize = 8 + 2 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 32 + 32;
pub const MWSOL_FORMAT_VERSION: u16 = 3;
/// Oldest on-disk version this reader still accepts. Version 2 frames are
/// plain postcard-encoded `MultiwayStrategyBlock`s (no quantization
/// support); version 3 wraps each frame in `FrameBlock` so it can carry
/// either an `F32` or `I16`-quantized payload.
pub const MWSOL_MIN_FORMAT_VERSION: u16 = 2;
pub const MWSOL_MAX_PAGE_LIMIT: usize = 4096;
/// Denominator used by i16 quantization (`i16::MAX`): quantized entries for
/// a block always sum to exactly this value.
const MWSOL_I16_DENOMINATOR: i16 = i16::MAX;
const MWSOL_INDEX_ENTRY_LEN: usize = 16 + 1 + 1 + 1 + 4 * 4 + 8 + 8 + 8 + 32;
const MAGIC: &[u8; 8] = b"SLVRMWSL";
const MAX_METADATA_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_STRATEGY_BLOCK_UNCOMPRESSED_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_STRATEGY_BLOCKS: u64 = 10_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MultiwayStrategyKey {
    pub history: [u8; 16],
    pub actor: u8,
    pub street: u8,
    pub active_opponents: u8,
    /// Full private recall. Entries after the current street hold the
    /// solver's unreached-bucket sentinel (`u32::MAX`), never zero.
    pub bucket_path: [u32; 4],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiwayHistoryAction {
    pub actor: u8,
    pub action_index: u32,
    pub action: String,
}

/// One edge in the compact public action-history trie. The root is implicit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiwayHistoryNode {
    pub key: [u8; 16],
    pub parent: [u8; 16],
    pub actor: u8,
    pub action_index: u32,
    pub action: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiwayStrategyBlock {
    pub key: MultiwayStrategyKey,
    /// Structured labels such as `fold`, `call`, and `raise-to:2500`.
    pub actions: Vec<String>,
    pub probabilities: Vec<f32>,
}

/// Per-frame on-disk representation, selected by [`write_mwsol_with`].
/// `MultiwayStrategyBlock` (the in-memory, always-`f32` type) never changes;
/// this is purely a codec detail of the `.mwsol` version-3 frame payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MwsolStorage {
    /// Store probabilities verbatim as `f32`.
    F32,
    /// Quantize probabilities to `i16` fixed point (denominator
    /// `i16::MAX`), roughly halving strategy payload size.
    I16,
}

/// Version-3 frame payload. Version-2 files instead store a plain
/// postcard-encoded `MultiwayStrategyBlock` per frame with no wrapping enum;
/// `MwSolReader` branches on `format_version()` to pick the right decode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum FrameBlock {
    F32 {
        key: MultiwayStrategyKey,
        actions: Vec<String>,
        probabilities: Vec<f32>,
    },
    I16 {
        key: MultiwayStrategyKey,
        actions: Vec<String>,
        quantized: Vec<i16>,
    },
}

impl FrameBlock {
    fn from_block(block: &MultiwayStrategyBlock, storage: MwsolStorage) -> Self {
        match storage {
            MwsolStorage::F32 => FrameBlock::F32 {
                key: block.key,
                actions: block.actions.clone(),
                probabilities: block.probabilities.clone(),
            },
            MwsolStorage::I16 => FrameBlock::I16 {
                key: block.key,
                actions: block.actions.clone(),
                quantized: quantize_i16(&block.probabilities),
            },
        }
    }

    fn into_block(self) -> MultiwayStrategyBlock {
        match self {
            FrameBlock::F32 {
                key,
                actions,
                probabilities,
            } => MultiwayStrategyBlock {
                key,
                actions,
                probabilities,
            },
            FrameBlock::I16 {
                key,
                actions,
                quantized,
            } => {
                let probabilities = quantized
                    .iter()
                    .map(|&q| f32::from(q) / f32::from(MWSOL_I16_DENOMINATOR))
                    .collect();
                MultiwayStrategyBlock {
                    key,
                    actions,
                    probabilities,
                }
            }
        }
    }
}

/// Quantizes probabilities to `i16` fixed point with denominator
/// `i16::MAX` (32767) using the largest-remainder method, so
/// `sum(quantized) == 32767` exactly and every entry is `>= 0`. Input need
/// not sum exactly to `1.0`; it is normalized by its own sum first, so this
/// tolerates the same `1e-4` slack `validate_strategy_block` allows.
fn quantize_i16(probabilities: &[f32]) -> Vec<i16> {
    let denominator = f64::from(MWSOL_I16_DENOMINATOR);
    let sum: f64 = probabilities.iter().map(|&p| f64::from(p)).sum();
    if probabilities.is_empty() || sum <= 0.0 || !sum.is_finite() {
        return vec![0; probabilities.len()];
    }

    let scaled: Vec<f64> = probabilities
        .iter()
        .map(|&p| f64::from(p) / sum * denominator)
        .collect();
    let mut floors: Vec<i64> = scaled.iter().map(|&value| value.floor() as i64).collect();
    let floor_sum: i64 = floors.iter().sum();
    let remainder = (MWSOL_I16_DENOMINATOR as i64 - floor_sum).clamp(0, floors.len() as i64);

    let mut order: Vec<usize> = (0..probabilities.len()).collect();
    order.sort_by(|&a, &b| {
        let fraction_a = scaled[a] - floors[a] as f64;
        let fraction_b = scaled[b] - floors[b] as f64;
        fraction_b
            .partial_cmp(&fraction_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });
    for &index in order.iter().take(remainder as usize) {
        floors[index] += 1;
    }

    floors
        .into_iter()
        .map(|value| value.clamp(0, i64::from(MWSOL_I16_DENOMINATOR)) as i16)
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiwaySeatResult {
    pub seat: u8,
    pub profile_ev: Option<Estimate>,
    pub average_positive_regret: f64,
    pub strategy_drift_l1: f64,
    pub deviation_gain_lower_bound: Option<Estimate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiwaySolutionMetadata {
    pub schema_version: u16,
    pub config_toml: String,
    pub abstraction_fingerprint: [u8; 32],
    pub sweeps: u64,
    pub approximate_profile: bool,
    pub seats: Vec<MultiwaySeatResult>,
    /// Strictly key-sorted compact trie; shared by all private buckets.
    pub histories: Vec<MultiwayHistoryNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiwaySolution {
    pub schema_version: u16,
    pub config_toml: String,
    pub abstraction_fingerprint: [u8; 32],
    pub sweeps: u64,
    pub approximate_profile: bool,
    pub seats: Vec<MultiwaySeatResult>,
    /// Strictly key-sorted compact trie; shared by all private buckets.
    pub histories: Vec<MultiwayHistoryNode>,
    /// Must be strictly sorted by `key`.
    pub strategies: Vec<MultiwayStrategyBlock>,
}

impl MultiwaySolutionMetadata {
    fn from_solution(solution: &MultiwaySolution) -> Self {
        Self {
            schema_version: solution.schema_version,
            config_toml: solution.config_toml.clone(),
            abstraction_fingerprint: solution.abstraction_fingerprint,
            sweeps: solution.sweeps,
            approximate_profile: solution.approximate_profile,
            seats: solution.seats.clone(),
            histories: solution.histories.clone(),
        }
    }

    pub fn resolve_history(&self, key: [u8; 16]) -> Option<Vec<MultiwayHistoryAction>> {
        resolve_history(&self.histories, key)
    }

    fn validate(&self) -> Result<(), MwSolError> {
        validate_metadata_fields(
            self.schema_version,
            self.approximate_profile,
            &self.histories,
        )
    }
}

impl MultiwaySolution {
    pub fn strategy(&self, key: MultiwayStrategyKey) -> Option<&MultiwayStrategyBlock> {
        self.strategies
            .binary_search_by_key(&key, |block| block.key)
            .ok()
            .map(|index| &self.strategies[index])
    }

    pub fn resolve_history(&self, key: [u8; 16]) -> Option<Vec<MultiwayHistoryAction>> {
        resolve_history(&self.histories, key)
    }

    fn validate(&self) -> Result<(), MwSolError> {
        validate_metadata_fields(
            self.schema_version,
            self.approximate_profile,
            &self.histories,
        )?;
        if self
            .strategies
            .windows(2)
            .any(|pair| pair[0].key >= pair[1].key)
        {
            return Err(MwSolError::UnsortedIndex);
        }
        for block in &self.strategies {
            validate_strategy_block(block, &self.histories)?;
        }
        Ok(())
    }
}

fn resolve_history(
    histories: &[MultiwayHistoryNode],
    mut key: [u8; 16],
) -> Option<Vec<MultiwayHistoryAction>> {
    let mut path = Vec::new();
    while key != [0; 16] {
        let index = histories.binary_search_by_key(&key, |node| node.key).ok()?;
        let node = &histories[index];
        path.push(MultiwayHistoryAction {
            actor: node.actor,
            action_index: node.action_index,
            action: node.action.clone(),
        });
        key = node.parent;
    }
    path.reverse();
    Some(path)
}

fn validate_metadata_fields(
    schema_version: u16,
    approximate_profile: bool,
    histories: &[MultiwayHistoryNode],
) -> Result<(), MwSolError> {
    if schema_version != MULTIWAY_SCHEMA_VERSION {
        return Err(MwSolError::SchemaVersion {
            found: schema_version,
            expected: MULTIWAY_SCHEMA_VERSION,
        });
    }
    if !approximate_profile {
        return Err(MwSolError::MissingApproximationMarker);
    }
    validate_histories(histories)
}

fn validate_histories(histories: &[MultiwayHistoryNode]) -> Result<(), MwSolError> {
    if histories.windows(2).any(|pair| pair[0].key >= pair[1].key) {
        return Err(MwSolError::InvalidHistoryTrie);
    }
    for node in histories {
        if node.key == [0; 16]
            || node.key != history_child(node.parent, node.actor, node.action_index)
            || node.action.is_empty()
            || (node.parent != [0; 16]
                && histories
                    .binary_search_by_key(&node.parent, |entry| entry.key)
                    .is_err())
        {
            return Err(MwSolError::InvalidHistoryTrie);
        }
        let mut key = node.key;
        for depth in 0..=histories.len() {
            if key == [0; 16] {
                break;
            }
            if depth == histories.len() {
                return Err(MwSolError::InvalidHistoryTrie);
            }
            let index = histories
                .binary_search_by_key(&key, |entry| entry.key)
                .map_err(|_| MwSolError::InvalidHistoryTrie)?;
            key = histories[index].parent;
        }
    }
    Ok(())
}

fn validate_strategy_key(
    key: MultiwayStrategyKey,
    histories: &[MultiwayHistoryNode],
) -> Result<(), MwSolError> {
    if key.history != [0; 16]
        && histories
            .binary_search_by_key(&key.history, |node| node.key)
            .is_err()
    {
        return Err(MwSolError::InvalidHistoryTrie);
    }
    Ok(())
}

fn validate_strategy_block(
    block: &MultiwayStrategyBlock,
    histories: &[MultiwayHistoryNode],
) -> Result<(), MwSolError> {
    validate_strategy_key(block.key, histories)?;
    if block.actions.is_empty() || block.actions.len() != block.probabilities.len() {
        return Err(MwSolError::InvalidStrategy(block.key));
    }
    if block
        .probabilities
        .iter()
        .any(|probability| !probability.is_finite() || *probability < 0.0)
    {
        return Err(MwSolError::InvalidStrategy(block.key));
    }
    let sum: f32 = block.probabilities.iter().sum();
    if (sum - 1.0).abs() > 1e-4 {
        return Err(MwSolError::InvalidStrategy(block.key));
    }
    Ok(())
}

fn history_child(parent: [u8; 16], actor: u8, action_index: u32) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.history.v1");
    hasher.update(&parent);
    hasher.update(&u64::from(actor).to_le_bytes());
    hasher.update(&u64::from(action_index).to_le_bytes());
    let mut key = [0; 16];
    key.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    key
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MwSolHeader {
    pub config_hash: [u8; 32],
    pub abstraction_fingerprint: [u8; 32],
    pub sweeps: u64,
    pub metadata_compressed_len: u64,
    pub metadata_uncompressed_len: u64,
    pub strategy_count: u64,
    pub strategy_payload_len: u64,
    pub metadata_checksum: [u8; 32],
    pub index_checksum: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MwSolStrategyIndexEntry {
    key: MultiwayStrategyKey,
    offset: u64,
    compressed_len: u64,
    uncompressed_len: u64,
    checksum: [u8; 32],
}

#[derive(Clone, Debug, PartialEq)]
pub struct MwSolStrategyPage {
    pub cursor: usize,
    pub next_cursor: Option<usize>,
    pub total: usize,
    pub strategies: Vec<MultiwayStrategyBlock>,
}

#[derive(Debug)]
pub struct MwSolReader {
    file: File,
    metadata: MultiwaySolutionMetadata,
    index_start: u64,
    frames_start: u64,
    strategy_count: usize,
    format_version: u16,
}

/// Writes an `.mwsol` file (version 3), encoding each strategy frame per
/// `storage`. `I16` quantization happens only at write time; the in-memory
/// `MultiwaySolution` stays `f32` throughout.
pub fn write_mwsol_with(
    path: &Path,
    solution: &MultiwaySolution,
    storage: MwsolStorage,
) -> Result<(), MwSolError> {
    write_mwsol_frames(path, solution, MWSOL_FORMAT_VERSION, |block| {
        let frame = FrameBlock::from_block(block, storage);
        Ok(postcard::to_allocvec(&frame)?)
    })
}

/// Shared writer body parameterized over the on-disk format version and a
/// per-block frame encoder, so tests can synthesize a version-2 file (plain
/// `MultiwayStrategyBlock` frames) through the exact same machinery that
/// production version-3 writes use.
fn write_mwsol_frames(
    path: &Path,
    solution: &MultiwaySolution,
    format_version: u16,
    encode_frame: impl Fn(&MultiwayStrategyBlock) -> Result<Vec<u8>, MwSolError>,
) -> Result<(), MwSolError> {
    solution.validate()?;

    let metadata = MultiwaySolutionMetadata::from_solution(solution);
    let metadata_raw = postcard::to_allocvec(&metadata)?;
    let metadata_uncompressed_len =
        u64::try_from(metadata_raw.len()).map_err(|_| MwSolError::LengthOverflow)?;
    if metadata_uncompressed_len > MAX_METADATA_UNCOMPRESSED_BYTES {
        return Err(MwSolError::MetadataTooLarge {
            declared: metadata_uncompressed_len,
            limit: MAX_METADATA_UNCOMPRESSED_BYTES,
        });
    }
    let metadata_compressed = zstd::stream::encode_all(metadata_raw.as_slice(), 3)?;
    let metadata_compressed_len =
        u64::try_from(metadata_compressed.len()).map_err(|_| MwSolError::LengthOverflow)?;
    let metadata_compressed_limit = compression_bound(metadata_uncompressed_len)?;
    if metadata_compressed_len == 0 || metadata_compressed_len > metadata_compressed_limit {
        return Err(MwSolError::MetadataCompressedLengthInvalid {
            declared: metadata_compressed_len,
            limit: metadata_compressed_limit,
        });
    }

    let strategy_count =
        u64::try_from(solution.strategies.len()).map_err(|_| MwSolError::LengthOverflow)?;
    if strategy_count > MAX_STRATEGY_BLOCKS {
        return Err(MwSolError::StrategyCountTooLarge {
            declared: strategy_count,
            limit: MAX_STRATEGY_BLOCKS,
        });
    }
    let index_len = strategy_count
        .checked_mul(MWSOL_INDEX_ENTRY_LEN as u64)
        .ok_or(MwSolError::LengthOverflow)?;
    let index_start = (MWSOL_HEADER_LEN as u64)
        .checked_add(metadata_compressed_len)
        .ok_or(MwSolError::LengthOverflow)?;
    let frames_start = index_start
        .checked_add(index_len)
        .ok_or(MwSolError::LengthOverflow)?;

    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.seek(SeekFrom::Start(frames_start))?;

    let mut strategy_payload_len = 0u64;
    let mut total_uncompressed_len = metadata_uncompressed_len;
    let mut index_hasher = blake3::Hasher::new();
    for (index, block) in solution.strategies.iter().enumerate() {
        let raw = encode_frame(block)?;
        let uncompressed_len = u64::try_from(raw.len()).map_err(|_| MwSolError::LengthOverflow)?;
        if uncompressed_len > MAX_STRATEGY_BLOCK_UNCOMPRESSED_BYTES {
            return Err(MwSolError::StrategyBlockTooLarge {
                index,
                declared: uncompressed_len,
                limit: MAX_STRATEGY_BLOCK_UNCOMPRESSED_BYTES,
            });
        }
        total_uncompressed_len = total_uncompressed_len
            .checked_add(uncompressed_len)
            .ok_or(MwSolError::LengthOverflow)?;
        if total_uncompressed_len > MAX_TOTAL_UNCOMPRESSED_BYTES {
            return Err(MwSolError::TotalUncompressedTooLarge {
                declared: total_uncompressed_len,
                limit: MAX_TOTAL_UNCOMPRESSED_BYTES,
            });
        }

        let compressed = zstd::stream::encode_all(raw.as_slice(), 3)?;
        let compressed_len =
            u64::try_from(compressed.len()).map_err(|_| MwSolError::LengthOverflow)?;
        let compressed_limit = compression_bound(uncompressed_len)?;
        if compressed_len == 0 || compressed_len > compressed_limit {
            return Err(MwSolError::StrategyCompressedLengthInvalid {
                index,
                declared: compressed_len,
                limit: compressed_limit,
            });
        }

        temporary.write_all(&compressed)?;
        let entry = MwSolStrategyIndexEntry {
            key: block.key,
            offset: strategy_payload_len,
            compressed_len,
            uncompressed_len,
            checksum: *blake3::hash(&compressed).as_bytes(),
        };
        let encoded_entry = encode_index_entry(entry);
        index_hasher.update(&encoded_entry);
        strategy_payload_len = strategy_payload_len
            .checked_add(compressed_len)
            .ok_or(MwSolError::LengthOverflow)?;

        let index = u64::try_from(index).map_err(|_| MwSolError::LengthOverflow)?;
        let index_position = index_start
            .checked_add(
                index
                    .checked_mul(MWSOL_INDEX_ENTRY_LEN as u64)
                    .ok_or(MwSolError::LengthOverflow)?,
            )
            .ok_or(MwSolError::LengthOverflow)?;
        temporary.seek(SeekFrom::Start(index_position))?;
        temporary.write_all(&encoded_entry)?;
        let payload_end = frames_start
            .checked_add(strategy_payload_len)
            .ok_or(MwSolError::LengthOverflow)?;
        temporary.seek(SeekFrom::Start(payload_end))?;
    }

    let header = MwSolHeader {
        config_hash: config_hash(solution.config_toml.as_bytes()),
        abstraction_fingerprint: solution.abstraction_fingerprint,
        sweeps: solution.sweeps,
        metadata_compressed_len,
        metadata_uncompressed_len,
        strategy_count,
        strategy_payload_len,
        metadata_checksum: *blake3::hash(&metadata_compressed).as_bytes(),
        index_checksum: *index_hasher.finalize().as_bytes(),
    };
    temporary.seek(SeekFrom::Start(0))?;
    temporary.write_all(&encode_header(header, format_version))?;
    temporary.write_all(&metadata_compressed)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| MwSolError::Io(error.error))?;
    Ok(())
}

impl MwSolReader {
    pub fn open(path: &Path) -> Result<Self, MwSolError> {
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len < MWSOL_HEADER_LEN as u64 {
            return Err(MwSolError::Truncated);
        }

        let mut encoded_header = [0u8; MWSOL_HEADER_LEN];
        file.read_exact(&mut encoded_header)?;
        let (header, format_version) = decode_header(&encoded_header)?;
        if header.metadata_uncompressed_len == 0
            || header.metadata_uncompressed_len > MAX_METADATA_UNCOMPRESSED_BYTES
        {
            return Err(MwSolError::MetadataTooLarge {
                declared: header.metadata_uncompressed_len,
                limit: MAX_METADATA_UNCOMPRESSED_BYTES,
            });
        }
        let metadata_compressed_limit = compression_bound(header.metadata_uncompressed_len)?;
        if header.metadata_compressed_len == 0
            || header.metadata_compressed_len > metadata_compressed_limit
        {
            return Err(MwSolError::MetadataCompressedLengthInvalid {
                declared: header.metadata_compressed_len,
                limit: metadata_compressed_limit,
            });
        }
        if header.strategy_count > MAX_STRATEGY_BLOCKS {
            return Err(MwSolError::StrategyCountTooLarge {
                declared: header.strategy_count,
                limit: MAX_STRATEGY_BLOCKS,
            });
        }
        let strategy_count =
            usize::try_from(header.strategy_count).map_err(|_| MwSolError::LengthOverflow)?;
        let index_len = header
            .strategy_count
            .checked_mul(MWSOL_INDEX_ENTRY_LEN as u64)
            .ok_or(MwSolError::LengthOverflow)?;
        let index_start = (MWSOL_HEADER_LEN as u64)
            .checked_add(header.metadata_compressed_len)
            .ok_or(MwSolError::LengthOverflow)?;
        let frames_start = index_start
            .checked_add(index_len)
            .ok_or(MwSolError::LengthOverflow)?;
        let expected_file_len = frames_start
            .checked_add(header.strategy_payload_len)
            .ok_or(MwSolError::LengthOverflow)?;
        if file_len < expected_file_len {
            return Err(MwSolError::Truncated);
        }
        if file_len > expected_file_len {
            return Err(MwSolError::LengthMismatch);
        }

        let metadata_compressed_len = usize::try_from(header.metadata_compressed_len)
            .map_err(|_| MwSolError::LengthOverflow)?;
        let mut metadata_compressed = vec![0u8; metadata_compressed_len];
        file.read_exact(&mut metadata_compressed)?;
        if *blake3::hash(&metadata_compressed).as_bytes() != header.metadata_checksum {
            return Err(MwSolError::MetadataChecksumMismatch);
        }
        let metadata_raw = decompress_exact(
            &metadata_compressed,
            header.metadata_uncompressed_len,
            MAX_METADATA_UNCOMPRESSED_BYTES,
        )?;
        let metadata: MultiwaySolutionMetadata = postcard::from_bytes(&metadata_raw)?;
        metadata.validate()?;

        let computed = config_hash(metadata.config_toml.as_bytes());
        if computed != header.config_hash {
            return Err(MwSolError::ConfigHashMismatch {
                header: config_hash_hex(&header.config_hash),
                computed: config_hash_hex(&computed),
            });
        }
        if metadata.abstraction_fingerprint != header.abstraction_fingerprint
            || metadata.sweeps != header.sweeps
        {
            return Err(MwSolError::HeaderMismatch);
        }

        let actual_index_checksum = hash_file_region(&mut file, index_start, index_len)?;
        if actual_index_checksum != header.index_checksum {
            return Err(MwSolError::IndexChecksumMismatch);
        }

        file.seek(SeekFrom::Start(index_start))?;
        let mut previous_key = None;
        let mut indexed_payload_len = 0u64;
        let mut total_uncompressed_len = header.metadata_uncompressed_len;
        for index in 0..strategy_count {
            let mut encoded_entry = [0u8; MWSOL_INDEX_ENTRY_LEN];
            file.read_exact(&mut encoded_entry)?;
            let entry = decode_index_entry(&encoded_entry);
            if previous_key.is_some_and(|previous| previous >= entry.key) {
                return Err(MwSolError::UnsortedIndex);
            }
            validate_strategy_key(entry.key, &metadata.histories)?;
            if entry.offset != indexed_payload_len {
                return Err(MwSolError::StrategyOffsetMismatch {
                    index,
                    declared: entry.offset,
                    expected: indexed_payload_len,
                });
            }
            if entry.uncompressed_len == 0
                || entry.uncompressed_len > MAX_STRATEGY_BLOCK_UNCOMPRESSED_BYTES
            {
                return Err(MwSolError::StrategyBlockTooLarge {
                    index,
                    declared: entry.uncompressed_len,
                    limit: MAX_STRATEGY_BLOCK_UNCOMPRESSED_BYTES,
                });
            }
            let compressed_limit = compression_bound(entry.uncompressed_len)?;
            if entry.compressed_len == 0 || entry.compressed_len > compressed_limit {
                return Err(MwSolError::StrategyCompressedLengthInvalid {
                    index,
                    declared: entry.compressed_len,
                    limit: compressed_limit,
                });
            }
            indexed_payload_len = indexed_payload_len
                .checked_add(entry.compressed_len)
                .ok_or(MwSolError::LengthOverflow)?;
            total_uncompressed_len = total_uncompressed_len
                .checked_add(entry.uncompressed_len)
                .ok_or(MwSolError::LengthOverflow)?;
            if total_uncompressed_len > MAX_TOTAL_UNCOMPRESSED_BYTES {
                return Err(MwSolError::TotalUncompressedTooLarge {
                    declared: total_uncompressed_len,
                    limit: MAX_TOTAL_UNCOMPRESSED_BYTES,
                });
            }
            previous_key = Some(entry.key);
        }
        if indexed_payload_len != header.strategy_payload_len {
            return Err(MwSolError::StrategyPayloadLengthMismatch {
                declared: header.strategy_payload_len,
                indexed: indexed_payload_len,
            });
        }

        Ok(Self {
            file,
            metadata,
            index_start,
            frames_start,
            strategy_count,
            format_version,
        })
    }

    /// The on-disk format version this file was read as (2 or 3).
    pub fn format_version(&self) -> u16 {
        self.format_version
    }

    pub fn metadata(&self) -> &MultiwaySolutionMetadata {
        &self.metadata
    }

    pub fn strategy_count(&self) -> usize {
        self.strategy_count
    }

    pub fn read_strategy_page(
        &mut self,
        cursor: usize,
        limit: usize,
    ) -> Result<MwSolStrategyPage, MwSolError> {
        if limit == 0 || limit > MWSOL_MAX_PAGE_LIMIT {
            return Err(MwSolError::InvalidPageLimit {
                limit,
                max: MWSOL_MAX_PAGE_LIMIT,
            });
        }
        if cursor > self.strategy_count {
            return Err(MwSolError::InvalidCursor {
                cursor,
                total: self.strategy_count,
            });
        }
        let end = cursor.saturating_add(limit).min(self.strategy_count);
        let page_len = end - cursor;
        let cursor = u64::try_from(cursor).map_err(|_| MwSolError::LengthOverflow)?;
        let index_position = self
            .index_start
            .checked_add(
                cursor
                    .checked_mul(MWSOL_INDEX_ENTRY_LEN as u64)
                    .ok_or(MwSolError::LengthOverflow)?,
            )
            .ok_or(MwSolError::LengthOverflow)?;
        self.file.seek(SeekFrom::Start(index_position))?;

        let mut entries = Vec::with_capacity(page_len);
        for _ in 0..page_len {
            let mut encoded_entry = [0u8; MWSOL_INDEX_ENTRY_LEN];
            self.file.read_exact(&mut encoded_entry)?;
            entries.push(decode_index_entry(&encoded_entry));
        }

        let cursor = usize::try_from(cursor).map_err(|_| MwSolError::LengthOverflow)?;
        let mut strategies = Vec::with_capacity(page_len);
        for (offset, entry) in entries.into_iter().enumerate() {
            let index = cursor + offset;
            let frame_position = self
                .frames_start
                .checked_add(entry.offset)
                .ok_or(MwSolError::LengthOverflow)?;
            self.file.seek(SeekFrom::Start(frame_position))?;
            let compressed_len =
                usize::try_from(entry.compressed_len).map_err(|_| MwSolError::LengthOverflow)?;
            let mut compressed = vec![0u8; compressed_len];
            self.file.read_exact(&mut compressed)?;
            if *blake3::hash(&compressed).as_bytes() != entry.checksum {
                return Err(MwSolError::StrategyChecksumMismatch { index });
            }

            let raw = decompress_exact(
                &compressed,
                entry.uncompressed_len,
                MAX_STRATEGY_BLOCK_UNCOMPRESSED_BYTES,
            )?;
            let block: MultiwayStrategyBlock = if self.format_version >= 3 {
                let frame: FrameBlock = postcard::from_bytes(&raw)?;
                frame.into_block()
            } else {
                postcard::from_bytes(&raw)?
            };
            if block.key != entry.key {
                return Err(MwSolError::StrategyKeyMismatch {
                    index,
                    indexed: entry.key,
                    decoded: block.key,
                });
            }
            validate_strategy_block(&block, &self.metadata.histories)?;
            strategies.push(block);
        }

        Ok(MwSolStrategyPage {
            cursor,
            next_cursor: (end < self.strategy_count).then_some(end),
            total: self.strategy_count,
            strategies,
        })
    }
}

fn encode_header(header: MwSolHeader, format_version: u16) -> [u8; MWSOL_HEADER_LEN] {
    let mut bytes = [0; MWSOL_HEADER_LEN];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&format_version.to_le_bytes());
    bytes[10..42].copy_from_slice(&header.config_hash);
    bytes[42..74].copy_from_slice(&header.abstraction_fingerprint);
    bytes[74..82].copy_from_slice(&header.sweeps.to_le_bytes());
    bytes[82..90].copy_from_slice(&header.metadata_compressed_len.to_le_bytes());
    bytes[90..98].copy_from_slice(&header.metadata_uncompressed_len.to_le_bytes());
    bytes[98..106].copy_from_slice(&header.strategy_count.to_le_bytes());
    bytes[106..114].copy_from_slice(&header.strategy_payload_len.to_le_bytes());
    bytes[114..146].copy_from_slice(&header.metadata_checksum);
    bytes[146..178].copy_from_slice(&header.index_checksum);
    bytes
}

/// Decodes the fixed header, returning the on-disk format version alongside
/// it so callers that need to branch on frame layout (`MwSolReader::open`)
/// can use it directly.
fn decode_header(bytes: &[u8; MWSOL_HEADER_LEN]) -> Result<(MwSolHeader, u16), MwSolError> {
    if &bytes[..8] != MAGIC {
        return Err(MwSolError::BadMagic);
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().expect("two bytes"));
    if !(MWSOL_MIN_FORMAT_VERSION..=MWSOL_FORMAT_VERSION).contains(&version) {
        return Err(MwSolError::BadVersion {
            found: version,
            min: MWSOL_MIN_FORMAT_VERSION,
            max: MWSOL_FORMAT_VERSION,
        });
    }
    Ok((
        MwSolHeader {
            config_hash: bytes[10..42].try_into().expect("32 bytes"),
            abstraction_fingerprint: bytes[42..74].try_into().expect("32 bytes"),
            sweeps: u64::from_le_bytes(bytes[74..82].try_into().expect("eight bytes")),
            metadata_compressed_len: u64::from_le_bytes(
                bytes[82..90].try_into().expect("eight bytes"),
            ),
            metadata_uncompressed_len: u64::from_le_bytes(
                bytes[90..98].try_into().expect("eight bytes"),
            ),
            strategy_count: u64::from_le_bytes(bytes[98..106].try_into().expect("eight bytes")),
            strategy_payload_len: u64::from_le_bytes(
                bytes[106..114].try_into().expect("eight bytes"),
            ),
            metadata_checksum: bytes[114..146].try_into().expect("32 bytes"),
            index_checksum: bytes[146..178].try_into().expect("32 bytes"),
        },
        version,
    ))
}

fn encode_index_entry(entry: MwSolStrategyIndexEntry) -> [u8; MWSOL_INDEX_ENTRY_LEN] {
    let mut bytes = [0u8; MWSOL_INDEX_ENTRY_LEN];
    bytes[..16].copy_from_slice(&entry.key.history);
    bytes[16] = entry.key.actor;
    bytes[17] = entry.key.street;
    bytes[18] = entry.key.active_opponents;
    for (index, bucket) in entry.key.bucket_path.iter().enumerate() {
        let start = 19 + index * 4;
        bytes[start..start + 4].copy_from_slice(&bucket.to_le_bytes());
    }
    bytes[35..43].copy_from_slice(&entry.offset.to_le_bytes());
    bytes[43..51].copy_from_slice(&entry.compressed_len.to_le_bytes());
    bytes[51..59].copy_from_slice(&entry.uncompressed_len.to_le_bytes());
    bytes[59..91].copy_from_slice(&entry.checksum);
    bytes
}

fn decode_index_entry(bytes: &[u8; MWSOL_INDEX_ENTRY_LEN]) -> MwSolStrategyIndexEntry {
    let mut bucket_path = [0u32; 4];
    for (index, bucket) in bucket_path.iter_mut().enumerate() {
        let start = 19 + index * 4;
        *bucket = u32::from_le_bytes(bytes[start..start + 4].try_into().expect("four bytes"));
    }
    MwSolStrategyIndexEntry {
        key: MultiwayStrategyKey {
            history: bytes[..16].try_into().expect("16 bytes"),
            actor: bytes[16],
            street: bytes[17],
            active_opponents: bytes[18],
            bucket_path,
        },
        offset: u64::from_le_bytes(bytes[35..43].try_into().expect("eight bytes")),
        compressed_len: u64::from_le_bytes(bytes[43..51].try_into().expect("eight bytes")),
        uncompressed_len: u64::from_le_bytes(bytes[51..59].try_into().expect("eight bytes")),
        checksum: bytes[59..91].try_into().expect("32 bytes"),
    }
}

fn compression_bound(uncompressed_len: u64) -> Result<u64, MwSolError> {
    let uncompressed_len =
        usize::try_from(uncompressed_len).map_err(|_| MwSolError::LengthOverflow)?;
    Ok(zstd::zstd_safe::compress_bound(uncompressed_len) as u64)
}

fn decompress_exact(
    compressed: &[u8],
    uncompressed_len: u64,
    limit: u64,
) -> Result<Vec<u8>, MwSolError> {
    if uncompressed_len > limit {
        return Err(MwSolError::UncompressedTooLarge {
            declared: uncompressed_len,
            limit,
        });
    }
    let capacity = usize::try_from(uncompressed_len).map_err(|_| MwSolError::LengthOverflow)?;
    let read_limit = uncompressed_len
        .checked_add(1)
        .ok_or(MwSolError::LengthOverflow)?;
    let decoder = zstd::stream::read::Decoder::new(compressed)?;
    let mut raw = Vec::with_capacity(capacity);
    decoder.take(read_limit).read_to_end(&mut raw)?;
    if raw.len() as u64 != uncompressed_len {
        return Err(MwSolError::UncompressedLengthMismatch {
            declared: uncompressed_len,
            actual: raw.len() as u64,
        });
    }
    Ok(raw)
}

fn hash_file_region(file: &mut File, start: u64, len: u64) -> Result<[u8; 32], MwSolError> {
    file.seek(SeekFrom::Start(start))?;
    let mut remaining = len;
    let mut buffer = [0u8; 64 * 1024];
    let mut hasher = blake3::Hasher::new();
    while remaining > 0 {
        let take = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| MwSolError::LengthOverflow)?;
        file.read_exact(&mut buffer[..take])?;
        hasher.update(&buffer[..take]);
        remaining -= take as u64;
    }
    Ok(*hasher.finalize().as_bytes())
}

#[derive(Debug, thiserror::Error)]
pub enum MwSolError {
    #[error("not a multiway solution (bad magic)")]
    BadMagic,
    #[error("unsupported .mwsol version {found} (expected {min}..={max})")]
    BadVersion { found: u16, min: u16, max: u16 },
    #[error("truncated .mwsol file")]
    Truncated,
    #[error(".mwsol length arithmetic overflow")]
    LengthOverflow,
    #[error(".mwsol length does not match its header")]
    LengthMismatch,
    #[error(".mwsol metadata checksum mismatch")]
    MetadataChecksumMismatch,
    #[error(".mwsol strategy index checksum mismatch")]
    IndexChecksumMismatch,
    #[error(".mwsol strategy frame {index} checksum mismatch")]
    StrategyChecksumMismatch { index: usize },
    #[error(".mwsol metadata uncompressed length {declared} is outside 1..={limit}")]
    MetadataTooLarge { declared: u64, limit: u64 },
    #[error(".mwsol metadata compressed length {declared} is outside 1..={limit}")]
    MetadataCompressedLengthInvalid { declared: u64, limit: u64 },
    #[error(".mwsol strategy count {declared} exceeds limit {limit}")]
    StrategyCountTooLarge { declared: u64, limit: u64 },
    #[error(".mwsol strategy block {index} uncompressed length {declared} is outside 1..={limit}")]
    StrategyBlockTooLarge {
        index: usize,
        declared: u64,
        limit: u64,
    },
    #[error(".mwsol strategy block {index} compressed length {declared} is outside 1..={limit}")]
    StrategyCompressedLengthInvalid {
        index: usize,
        declared: u64,
        limit: u64,
    },
    #[error(".mwsol total uncompressed length {declared} exceeds limit {limit}")]
    TotalUncompressedTooLarge { declared: u64, limit: u64 },
    #[error(".mwsol strategy {index} offset differs: declared {declared}, expected {expected}")]
    StrategyOffsetMismatch {
        index: usize,
        declared: u64,
        expected: u64,
    },
    #[error(".mwsol strategy payload length differs: declared {declared}, indexed {indexed}")]
    StrategyPayloadLengthMismatch { declared: u64, indexed: u64 },
    #[error(".mwsol uncompressed length {declared} exceeds limit {limit}")]
    UncompressedTooLarge { declared: u64, limit: u64 },
    #[error(".mwsol decoded length differs: declared {declared}, actual {actual}")]
    UncompressedLengthMismatch { declared: u64, actual: u64 },
    #[error(".mwsol config hash {header} does not match embedded config {computed}")]
    ConfigHashMismatch { header: String, computed: String },
    #[error(".mwsol header does not match metadata")]
    HeaderMismatch,
    #[error("multiway schema version {found} is unsupported (expected {expected})")]
    SchemaVersion { found: u16, expected: u16 },
    #[error("multiway results must be marked as an approximate profile")]
    MissingApproximationMarker,
    #[error("multiway strategy index is not strictly sorted")]
    UnsortedIndex,
    #[error("multiway public-history trie is invalid")]
    InvalidHistoryTrie,
    #[error("invalid strategy block for {0:?}")]
    InvalidStrategy(MultiwayStrategyKey),
    #[error(".mwsol cursor {cursor} is past strategy count {total}")]
    InvalidCursor { cursor: usize, total: usize },
    #[error(".mwsol page limit {limit} is outside 1..={max}")]
    InvalidPageLimit { limit: usize, max: usize },
    #[error(
        ".mwsol strategy {index} key differs between index ({indexed:?}) and frame ({decoded:?})"
    )]
    StrategyKeyMismatch {
        index: usize,
        indexed: MultiwayStrategyKey,
        decoded: MultiwayStrategyKey,
    },
    #[error(".mwsol I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error(".mwsol codec error: {0}")]
    Codec(#[from] postcard::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;

    /// Fully decodes an `.mwsol` file through the paged reader API (the
    /// same steps the now-removed `read_mwsol` convenience wrapper used to
    /// perform), for tests that want to assert on a complete
    /// [`MultiwaySolution`] rather than paging through it themselves.
    fn decode_via_reader(path: &Path) -> Result<MultiwaySolution, MwSolError> {
        let mut reader = MwSolReader::open(path)?;
        let total = reader.strategy_count();
        let mut cursor = 0;
        let mut strategies = Vec::with_capacity(total);
        while cursor < total {
            let page = reader.read_strategy_page(cursor, MWSOL_MAX_PAGE_LIMIT)?;
            strategies.extend(page.strategies);
            cursor = page.next_cursor.unwrap_or(total);
        }
        let metadata = reader.metadata().clone();
        let solution = MultiwaySolution {
            schema_version: metadata.schema_version,
            config_toml: metadata.config_toml,
            abstraction_fingerprint: metadata.abstraction_fingerprint,
            sweeps: metadata.sweeps,
            approximate_profile: metadata.approximate_profile,
            seats: metadata.seats,
            histories: metadata.histories,
            strategies,
        };
        solution.validate()?;
        Ok(solution)
    }

    /// Reads and decodes just the fixed header (the same steps the
    /// now-removed `peek_mwsol_header` convenience wrapper used to
    /// perform), for tests that corrupt specific byte regions and need the
    /// header's declared offsets/lengths to find them.
    fn peek_header(path: &Path) -> Result<MwSolHeader, MwSolError> {
        let mut file = std::fs::File::open(path)?;
        let mut bytes = [0u8; MWSOL_HEADER_LEN];
        file.read_exact(&mut bytes).map_err(|error| {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                MwSolError::Truncated
            } else {
                MwSolError::Io(error)
            }
        })?;
        let (header, _format_version) = decode_header(&bytes)?;
        Ok(header)
    }

    fn solution() -> MultiwaySolution {
        MultiwaySolution {
            schema_version: MULTIWAY_SCHEMA_VERSION,
            config_toml: "[game]\nkind = \"preflop-multiway\"\n".into(),
            abstraction_fingerprint: [7; 32],
            sweeps: 99,
            approximate_profile: true,
            seats: Vec::new(),
            histories: Vec::new(),
            strategies: (0..5)
                .map(|index| MultiwayStrategyBlock {
                    key: MultiwayStrategyKey {
                        history: [0; 16],
                        actor: 0,
                        street: 0,
                        active_opponents: 2,
                        bucket_path: [12 + index, 0, 0, 0],
                    },
                    actions: vec!["fold".into(), "call".into()],
                    probabilities: vec![0.25, 0.75],
                })
                .collect(),
        }
    }

    fn index_start(header: MwSolHeader) -> u64 {
        MWSOL_HEADER_LEN as u64 + header.metadata_compressed_len
    }

    fn frames_start(header: MwSolHeader) -> u64 {
        index_start(header) + header.strategy_count * MWSOL_INDEX_ENTRY_LEN as u64
    }

    fn read_index_entry(path: &Path, header: MwSolHeader, index: usize) -> MwSolStrategyIndexEntry {
        let mut file = File::open(path).unwrap();
        let position = index_start(header) + index as u64 * MWSOL_INDEX_ENTRY_LEN as u64;
        file.seek(SeekFrom::Start(position)).unwrap();
        let mut encoded = [0u8; MWSOL_INDEX_ENTRY_LEN];
        file.read_exact(&mut encoded).unwrap();
        decode_index_entry(&encoded)
    }

    fn rewrite_index_entry(
        path: &Path,
        mut header: MwSolHeader,
        index: usize,
        entry: MwSolStrategyIndexEntry,
    ) {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        let position = index_start(header) + index as u64 * MWSOL_INDEX_ENTRY_LEN as u64;
        file.seek(SeekFrom::Start(position)).unwrap();
        file.write_all(&encode_index_entry(entry)).unwrap();
        let index_len = header.strategy_count * MWSOL_INDEX_ENTRY_LEN as u64;
        header.index_checksum =
            hash_file_region(&mut file, index_start(header), index_len).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&encode_header(header, MWSOL_FORMAT_VERSION))
            .unwrap();
        file.sync_all().unwrap();
    }

    fn flip_byte(path: &Path, offset: u64) {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(&[byte[0] ^ 0x80]).unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn indexed_artifact_round_trips_and_pages_selected_frames() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.mwsol");
        let expected = solution();
        write_mwsol_with(&path, &expected, MwsolStorage::F32).unwrap();
        write_mwsol_with(&path, &expected, MwsolStorage::F32).unwrap();

        let header = peek_header(&path).unwrap();
        assert_eq!(header.sweeps, 99);
        assert_eq!(header.strategy_count, expected.strategies.len() as u64);
        assert!(header.metadata_compressed_len > 0);
        assert!(header.metadata_uncompressed_len > 0);

        let mut reader = MwSolReader::open(&path).unwrap();
        assert_eq!(peek_header(&path).unwrap(), header);
        assert_eq!(reader.format_version(), MWSOL_FORMAT_VERSION);
        assert_eq!(reader.strategy_count(), expected.strategies.len());
        assert_eq!(reader.metadata().sweeps, 99);
        let page = reader.read_strategy_page(1, 2).unwrap();
        assert_eq!(page.cursor, 1);
        assert_eq!(page.next_cursor, Some(3));
        assert_eq!(page.total, expected.strategies.len());
        assert_eq!(page.strategies.as_slice(), &expected.strategies[1..3]);
        assert!(matches!(
            reader.read_strategy_page(expected.strategies.len() + 1, 1),
            Err(MwSolError::InvalidCursor { .. })
        ));
        assert!(matches!(
            reader.read_strategy_page(0, 0),
            Err(MwSolError::InvalidPageLimit { .. })
        ));
        assert!(matches!(
            reader.read_strategy_page(0, MWSOL_MAX_PAGE_LIMIT + 1),
            Err(MwSolError::InvalidPageLimit { .. })
        ));

        let decoded = decode_via_reader(&path).unwrap();
        assert_eq!(decoded, expected);
        assert!(decoded.strategy(expected.strategies[0].key).is_some());
    }

    #[test]
    fn an_unselected_corrupt_frame_does_not_expand_but_fails_when_selected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("selective.mwsol");
        let expected = solution();
        write_mwsol_with(&path, &expected, MwsolStorage::F32).unwrap();
        let header = peek_header(&path).unwrap();
        let corrupt_index = expected.strategies.len() - 1;
        let entry = read_index_entry(&path, header, corrupt_index);
        flip_byte(
            &path,
            frames_start(header) + entry.offset + entry.compressed_len / 2,
        );

        let mut reader = MwSolReader::open(&path).unwrap();
        let first_page = reader.read_strategy_page(0, 2).unwrap();
        assert_eq!(first_page.strategies.as_slice(), &expected.strategies[..2]);
        assert!(matches!(
            reader.read_strategy_page(corrupt_index, 1),
            Err(MwSolError::StrategyChecksumMismatch { index })
                if index == corrupt_index
        ));
        assert!(matches!(
            decode_via_reader(&path),
            Err(MwSolError::StrategyChecksumMismatch { index })
                if index == corrupt_index
        ));
    }

    #[test]
    fn metadata_and_index_corruption_are_detected_before_strategy_reads() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("regions.mwsol");
        write_mwsol_with(&path, &solution(), MwsolStorage::F32).unwrap();
        let header = peek_header(&path).unwrap();
        flip_byte(
            &path,
            MWSOL_HEADER_LEN as u64 + header.metadata_compressed_len / 2,
        );
        assert!(matches!(
            MwSolReader::open(&path),
            Err(MwSolError::MetadataChecksumMismatch)
        ));

        write_mwsol_with(&path, &solution(), MwsolStorage::F32).unwrap();
        let header = peek_header(&path).unwrap();
        flip_byte(&path, index_start(header) + 1);
        assert!(matches!(
            MwSolReader::open(&path),
            Err(MwSolError::IndexChecksumMismatch)
        ));
    }

    #[test]
    fn truncation_and_trailing_bytes_are_rejected_by_exact_file_length() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("length.mwsol");
        write_mwsol_with(&path, &solution(), MwsolStorage::F32).unwrap();
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        let original_len = file.metadata().unwrap().len();
        file.set_len(original_len - 1).unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(matches!(
            MwSolReader::open(&path),
            Err(MwSolError::Truncated)
        ));

        write_mwsol_with(&path, &solution(), MwsolStorage::F32).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&[0]).unwrap();
        file.sync_all().unwrap();
        assert!(matches!(
            MwSolReader::open(&path),
            Err(MwSolError::LengthMismatch)
        ));
    }

    #[test]
    fn declared_decompression_lengths_and_limits_are_enforced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("limits.mwsol");
        write_mwsol_with(&path, &solution(), MwsolStorage::F32).unwrap();

        let mut header = peek_header(&path).unwrap();
        header.metadata_uncompressed_len = MAX_METADATA_UNCOMPRESSED_BYTES + 1;
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&encode_header(header, MWSOL_FORMAT_VERSION))
            .unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(matches!(
            MwSolReader::open(&path),
            Err(MwSolError::MetadataTooLarge { declared, .. })
                if declared == MAX_METADATA_UNCOMPRESSED_BYTES + 1
        ));

        write_mwsol_with(&path, &solution(), MwsolStorage::F32).unwrap();
        let header = peek_header(&path).unwrap();
        let mut entry = read_index_entry(&path, header, 0);
        entry.uncompressed_len += 1;
        rewrite_index_entry(&path, header, 0, entry);
        let mut reader = MwSolReader::open(&path).unwrap();
        assert!(matches!(
            reader.read_strategy_page(0, 1),
            Err(MwSolError::UncompressedLengthMismatch { .. })
        ));
        drop(reader);

        write_mwsol_with(&path, &solution(), MwsolStorage::F32).unwrap();
        let header = peek_header(&path).unwrap();
        let mut entry = read_index_entry(&path, header, 0);
        entry.uncompressed_len = MAX_STRATEGY_BLOCK_UNCOMPRESSED_BYTES + 1;
        rewrite_index_entry(&path, header, 0, entry);
        assert!(matches!(
            MwSolReader::open(&path),
            Err(MwSolError::StrategyBlockTooLarge { index: 0, .. })
        ));
    }

    #[test]
    fn index_offsets_are_checked_even_with_a_valid_index_checksum() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("offset.mwsol");
        write_mwsol_with(&path, &solution(), MwsolStorage::F32).unwrap();
        let header = peek_header(&path).unwrap();
        let mut entry = read_index_entry(&path, header, 0);
        entry.offset = 1;
        rewrite_index_entry(&path, header, 0, entry);
        assert!(matches!(
            MwSolReader::open(&path),
            Err(MwSolError::StrategyOffsetMismatch {
                index: 0,
                declared: 1,
                expected: 0
            })
        ));
    }

    #[test]
    fn version_one_is_explicitly_unsupported() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v1.mwsol");
        let mut bytes = vec![0u8; MWSOL_HEADER_LEN];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(
            MwSolReader::open(&path),
            Err(MwSolError::BadVersion {
                found: 1,
                min: MWSOL_MIN_FORMAT_VERSION,
                max: MWSOL_FORMAT_VERSION,
            })
        ));
    }

    #[test]
    fn rejects_unsorted_or_unmarked_profiles() {
        let mut value = solution();
        value.approximate_profile = false;
        assert!(matches!(
            value.validate(),
            Err(MwSolError::MissingApproximationMarker)
        ));

        let mut value = solution();
        value.strategies.swap(0, 1);
        assert!(matches!(value.validate(), Err(MwSolError::UnsortedIndex)));
    }

    #[test]
    fn compact_history_trie_resolves_and_is_integrity_checked() {
        let mut value = solution();
        let key = history_child([0; 16], 2, 1);
        value.histories.push(MultiwayHistoryNode {
            key,
            parent: [0; 16],
            actor: 2,
            action_index: 1,
            action: "raise-to:2500".into(),
        });
        for strategy in &mut value.strategies {
            strategy.key.history = key;
        }
        let expected = vec![MultiwayHistoryAction {
            actor: 2,
            action_index: 1,
            action: "raise-to:2500".into(),
        }];
        assert_eq!(value.resolve_history(key).unwrap(), expected);
        let metadata = MultiwaySolutionMetadata::from_solution(&value);
        assert_eq!(metadata.resolve_history(key).unwrap(), expected);
        value.validate().unwrap();
        value.histories[0].action_index = 0;
        assert!(matches!(
            value.validate(),
            Err(MwSolError::InvalidHistoryTrie)
        ));
    }

    /// Synthesizes a version-2 `.mwsol` file: same layout as production
    /// writes, but frames are plain postcard-encoded `MultiwayStrategyBlock`s
    /// (no `FrameBlock` wrapper) and the header declares version 2, exactly
    /// replicating the pre-i16 on-disk format.
    fn write_legacy_v2(path: &Path, solution: &MultiwaySolution) -> Result<(), MwSolError> {
        write_mwsol_frames(path, solution, 2, |block| Ok(postcard::to_allocvec(block)?))
    }

    #[test]
    fn i16_storage_round_trips_within_quantization_tolerance() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("quantized.mwsol");
        let mut expected = solution();
        // Exercise awkward, non-power-of-two probabilities in addition to
        // the default 0.25/0.75 split already present in `solution()`.
        expected.strategies[0].actions = vec!["fold".into(), "call".into(), "raise-to:2500".into()];
        expected.strategies[0].probabilities = vec![1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0];
        expected.strategies[1].probabilities = vec![0.9999, 0.0001];

        write_mwsol_with(&path, &expected, MwsolStorage::I16).unwrap();

        let mut reader = MwSolReader::open(&path).unwrap();
        assert_eq!(reader.format_version(), MWSOL_FORMAT_VERSION);
        let decoded = decode_via_reader(&path).unwrap();
        assert_eq!(decoded.strategies.len(), expected.strategies.len());
        for (got, want) in decoded.strategies.iter().zip(expected.strategies.iter()) {
            assert_eq!(got.key, want.key);
            assert_eq!(got.actions, want.actions);
            let mut max_error = 0f32;
            for (a, b) in got.probabilities.iter().zip(want.probabilities.iter()) {
                max_error = max_error.max((a - b).abs());
            }
            assert!(
                max_error < 1e-4,
                "max error {max_error} for key {:?}",
                got.key
            );
            let sum: f32 = got.probabilities.iter().sum();
            assert!(
                (sum - 1.0).abs() < 1e-4,
                "decoded sum {sum} not within f32 rounding of 1.0"
            );
        }
        let _ = reader.read_strategy_page(0, 1).unwrap();
    }

    #[test]
    fn quantize_i16_sums_exactly_and_stays_non_negative() {
        let cases: &[&[f32]] = &[
            &[1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
            &[0.9999, 0.0001],
            &[1.0],
            &[0.5, 0.5],
            &[0.999_999_9, 0.000_000_1],
            &[0.1, 0.2, 0.3, 0.4],
            // Does not sum to 1.0; quantization must normalize by its own
            // sum rather than assume a pre-normalized input.
            &[2.0, 1.0, 1.0],
        ];
        for probabilities in cases {
            let quantized = quantize_i16(probabilities);
            assert_eq!(quantized.len(), probabilities.len());
            assert!(
                quantized.iter().all(|&q| q >= 0),
                "negative entry in {quantized:?} for input {probabilities:?}"
            );
            let sum: i64 = quantized.iter().map(|&q| i64::from(q)).sum();
            assert_eq!(
                sum,
                i64::from(i16::MAX),
                "quantized {quantized:?} for input {probabilities:?} summed to {sum}, expected {}",
                i16::MAX
            );
        }
    }

    #[test]
    fn version_two_files_are_read_identically_to_version_three() {
        let directory = tempfile::tempdir().unwrap();
        let v2_path = directory.path().join("legacy.mwsol");
        let v3_path = directory.path().join("current.mwsol");
        let expected = solution();

        write_legacy_v2(&v2_path, &expected).unwrap();
        write_mwsol_with(&v3_path, &expected, MwsolStorage::F32).unwrap();

        let mut v2_reader = MwSolReader::open(&v2_path).unwrap();
        assert_eq!(v2_reader.format_version(), 2);
        let v2_decoded = decode_via_reader(&v2_path).unwrap();

        let mut v3_reader = MwSolReader::open(&v3_path).unwrap();
        assert_eq!(v3_reader.format_version(), MWSOL_FORMAT_VERSION);
        let v3_decoded = decode_via_reader(&v3_path).unwrap();

        assert_eq!(v2_decoded, expected);
        assert_eq!(v2_decoded, v3_decoded);

        let v2_page = v2_reader.read_strategy_page(0, 2).unwrap();
        let v3_page = v3_reader.read_strategy_page(0, 2).unwrap();
        assert_eq!(v2_page.strategies, v3_page.strategies);
    }
}
