//! Versioned, checksummed, atomically replaced multiway solver checkpoints.

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::solver::{ExternalSamplingGame, MultiwaySolver, SOLVER_STATE_VERSION, SolverState};

const CHECKPOINT_MAGIC: &[u8; 8] = b"SLVRMWCP";
pub const CHECKPOINT_VERSION: u16 = 3;
const HEADER_LEN: usize = 8 + 2 + 32 + 32 + 8 + 8 + 8 + 32 + 4 + 32;
const CHUNK_ENTRY_LEN: usize = 8 + 8 + 8 + 32;
const CHUNK_UNCOMPRESSED_BYTES: usize = 4 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiwayCheckpointHeader {
    pub version: u16,
    pub configuration_fingerprint: [u8; 32],
    pub abstraction_fingerprint: [u8; 32],
    pub next_sample_id: u64,
    pub payload_len: u64,
    pub uncompressed_len: u64,
    pub payload_checksum: [u8; 32],
    pub chunk_count: u32,
    pub chunk_table_checksum: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChunkEntry {
    compressed_offset: u64,
    compressed_len: u64,
    uncompressed_len: u64,
    checksum: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiwayCheckpoint {
    pub header: MultiwayCheckpointHeader,
    pub state: SolverState,
}

impl MultiwayCheckpoint {
    pub fn new(
        state: SolverState,
        configuration_fingerprint: [u8; 32],
        abstraction_fingerprint: [u8; 32],
    ) -> Self {
        Self {
            header: MultiwayCheckpointHeader {
                version: CHECKPOINT_VERSION,
                configuration_fingerprint,
                abstraction_fingerprint,
                next_sample_id: state.next_sample_id,
                payload_len: 0,
                uncompressed_len: 0,
                payload_checksum: [0; 32],
                chunk_count: 0,
                chunk_table_checksum: [0; 32],
            },
            state,
        }
    }

    pub fn capture<G: ExternalSamplingGame>(solver: &MultiwaySolver<G>) -> Self {
        Self::new(
            solver.snapshot_state(),
            solver.configuration_fingerprint(),
            solver.abstraction_fingerprint(),
        )
    }

    /// Writes to a temporary file in the destination directory, fsyncs it,
    /// and atomically persists it over `path`.
    pub fn write_atomic(&self, path: &Path) -> Result<(), CheckpointError> {
        if self.state.schema_version != SOLVER_STATE_VERSION {
            return Err(CheckpointError::SolverStateVersion {
                found: self.state.schema_version,
                expected: SOLVER_STATE_VERSION,
            });
        }
        if self.header.next_sample_id != self.state.next_sample_id {
            return Err(CheckpointError::SampleIdMismatch {
                header: self.header.next_sample_id,
                state: self.state.next_sample_id,
            });
        }

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let directory = parent.unwrap_or_else(|| Path::new("."));

        // Serialize through std::io into an intermediate file. Keeping the
        // raw postcard payload off-heap is essential when a resource-limit
        // checkpoint is captured near the configured policy budget.
        let mut raw_file = tempfile::NamedTempFile::new_in(directory)?;
        {
            let writer = BufWriter::new(raw_file.as_file_mut());
            let mut writer = postcard::to_io(&self.state, writer)?;
            writer.flush()?;
        }
        let uncompressed_len = raw_file.as_file().metadata()?.len();
        if uncompressed_len > MAX_UNCOMPRESSED_BYTES {
            return Err(CheckpointError::UncompressedTooLarge {
                declared: uncompressed_len,
                limit: MAX_UNCOMPRESSED_BYTES,
            });
        }
        let chunk_count = chunk_count_for_len(uncompressed_len);
        let chunk_table_len = u64::from(chunk_count)
            .checked_mul(CHUNK_ENTRY_LEN as u64)
            .ok_or(CheckpointError::LengthOverflow)?;
        let payload_start = (HEADER_LEN as u64)
            .checked_add(chunk_table_len)
            .ok_or(CheckpointError::LengthOverflow)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.seek(SeekFrom::Start(payload_start))?;
        raw_file.seek(SeekFrom::Start(0))?;

        let mut chunks = Vec::new();
        try_reserve_bytes(&mut chunks, chunk_count as usize, size_of_chunk_entry())?;
        let mut raw_chunk = Vec::new();
        try_reserve_u8(&mut raw_chunk, CHUNK_UNCOMPRESSED_BYTES)?;
        raw_chunk.resize(CHUNK_UNCOMPRESSED_BYTES, 0);
        let mut payload_len = 0u64;
        let mut payload_hasher = blake3::Hasher::new();
        let mut remaining = uncompressed_len;
        for index in 0..chunk_count {
            let raw_len = remaining.min(CHUNK_UNCOMPRESSED_BYTES as u64) as usize;
            raw_file.read_exact(&mut raw_chunk[..raw_len])?;
            let compressed = zstd::stream::encode_all(&raw_chunk[..raw_len], 3)?;
            let compressed_len =
                u64::try_from(compressed.len()).map_err(|_| CheckpointError::LengthOverflow)?;
            let compressed_limit = zstd::zstd_safe::compress_bound(raw_len) as u64;
            if compressed_len == 0 || compressed_len > compressed_limit {
                return Err(CheckpointError::ChunkCompressedLengthInvalid {
                    index,
                    declared: compressed_len,
                    limit: compressed_limit,
                });
            }

            let checksum = *blake3::hash(&compressed).as_bytes();
            chunks.push(ChunkEntry {
                compressed_offset: payload_len,
                compressed_len,
                uncompressed_len: raw_len as u64,
                checksum,
            });
            payload_len = payload_len
                .checked_add(compressed_len)
                .ok_or(CheckpointError::LengthOverflow)?;
            payload_hasher.update(&compressed);
            temporary.write_all(&compressed)?;
            remaining -= raw_len as u64;
        }
        debug_assert_eq!(chunks.len(), chunk_count as usize);
        debug_assert_eq!(remaining, 0);

        let chunk_table = encode_chunk_table(&chunks);
        let mut header = self.header.clone();
        header.version = CHECKPOINT_VERSION;
        header.payload_len = payload_len;
        header.uncompressed_len = uncompressed_len;
        header.payload_checksum = *payload_hasher.finalize().as_bytes();
        header.chunk_count = chunk_count;
        header.chunk_table_checksum = *blake3::hash(&chunk_table).as_bytes();

        temporary.seek(SeekFrom::Start(0))?;
        temporary.write_all(&encode_header(&header))?;
        temporary.write_all(&chunk_table)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(path)
            .map_err(|error| CheckpointError::Io(error.error))?;
        Ok(())
    }

    pub fn load(
        path: &Path,
        expected_configuration: [u8; 32],
        expected_abstraction: [u8; 32],
    ) -> Result<Self, CheckpointError> {
        let checkpoint = Self::load_unchecked(path)?;
        if checkpoint.header.configuration_fingerprint != expected_configuration {
            return Err(CheckpointError::ConfigurationMismatch);
        }
        if checkpoint.header.abstraction_fingerprint != expected_abstraction {
            return Err(CheckpointError::AbstractionMismatch);
        }
        Ok(checkpoint)
    }

    /// Loads and integrity-checks a checkpoint without compatibility hashes.
    /// This is intended for inspection tools; resumes should use [`Self::load`].
    pub fn load_unchecked(path: &Path) -> Result<Self, CheckpointError> {
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len < HEADER_LEN as u64 {
            return Err(CheckpointError::Truncated);
        }
        let mut encoded_header = [0u8; HEADER_LEN];
        file.read_exact(&mut encoded_header)?;
        let header = decode_header(&encoded_header)?;

        if header.uncompressed_len > MAX_UNCOMPRESSED_BYTES {
            return Err(CheckpointError::UncompressedTooLarge {
                declared: header.uncompressed_len,
                limit: MAX_UNCOMPRESSED_BYTES,
            });
        }
        let expected_chunk_count = chunk_count_for_len(header.uncompressed_len);
        if header.chunk_count != expected_chunk_count {
            return Err(CheckpointError::ChunkCountMismatch {
                declared: header.chunk_count,
                expected: expected_chunk_count,
            });
        }

        let chunk_table_len = u64::from(header.chunk_count)
            .checked_mul(CHUNK_ENTRY_LEN as u64)
            .ok_or(CheckpointError::LengthOverflow)?;
        let payload_start = (HEADER_LEN as u64)
            .checked_add(chunk_table_len)
            .ok_or(CheckpointError::LengthOverflow)?;
        let expected_file_len = payload_start
            .checked_add(header.payload_len)
            .ok_or(CheckpointError::LengthOverflow)?;
        if file_len < expected_file_len {
            return Err(CheckpointError::Truncated);
        }
        if file_len > expected_file_len {
            return Err(CheckpointError::LengthMismatch {
                declared: expected_file_len,
                actual: file_len,
            });
        }

        let chunk_table_len =
            usize::try_from(chunk_table_len).map_err(|_| CheckpointError::LengthOverflow)?;
        let mut encoded_chunk_table = vec![0u8; chunk_table_len];
        file.read_exact(&mut encoded_chunk_table)?;
        if *blake3::hash(&encoded_chunk_table).as_bytes() != header.chunk_table_checksum {
            return Err(CheckpointError::ChunkTableChecksumMismatch);
        }
        let chunks = decode_chunk_table(&encoded_chunk_table);

        let mut indexed_payload_len = 0u64;
        for (index, chunk) in chunks.iter().enumerate() {
            let index = index as u32;
            let expected_uncompressed_len =
                expected_chunk_len(header.uncompressed_len, index, header.chunk_count);
            if chunk.uncompressed_len != expected_uncompressed_len {
                return Err(CheckpointError::ChunkUncompressedLengthInvalid {
                    index,
                    declared: chunk.uncompressed_len,
                    expected: expected_uncompressed_len,
                });
            }
            if chunk.compressed_offset != indexed_payload_len {
                return Err(CheckpointError::ChunkOffsetMismatch {
                    index,
                    declared: chunk.compressed_offset,
                    expected: indexed_payload_len,
                });
            }
            let compressed_limit = zstd::zstd_safe::compress_bound(
                usize::try_from(expected_uncompressed_len)
                    .map_err(|_| CheckpointError::LengthOverflow)?,
            ) as u64;
            if chunk.compressed_len == 0 || chunk.compressed_len > compressed_limit {
                return Err(CheckpointError::ChunkCompressedLengthInvalid {
                    index,
                    declared: chunk.compressed_len,
                    limit: compressed_limit,
                });
            }
            indexed_payload_len = indexed_payload_len
                .checked_add(chunk.compressed_len)
                .ok_or(CheckpointError::LengthOverflow)?;
        }
        if indexed_payload_len != header.payload_len {
            return Err(CheckpointError::ChunkPayloadLengthMismatch {
                declared: header.payload_len,
                indexed: indexed_payload_len,
            });
        }

        let expected_raw_len = usize::try_from(header.uncompressed_len)
            .map_err(|_| CheckpointError::LengthOverflow)?;
        let mut raw = Vec::new();
        try_reserve_u8(&mut raw, expected_raw_len)?;
        let mut decoded_chunk = Vec::new();
        try_reserve_u8(
            &mut decoded_chunk,
            CHUNK_UNCOMPRESSED_BYTES
                .checked_add(1)
                .ok_or(CheckpointError::LengthOverflow)?,
        )?;
        let mut payload_hasher = blake3::Hasher::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let compressed_len = usize::try_from(chunk.compressed_len)
                .map_err(|_| CheckpointError::LengthOverflow)?;
            let mut compressed = vec![0u8; compressed_len];
            file.read_exact(&mut compressed)?;
            payload_hasher.update(&compressed);
            if *blake3::hash(&compressed).as_bytes() != chunk.checksum {
                return Err(CheckpointError::ChunkChecksumMismatch {
                    index: index as u32,
                });
            }

            decoded_chunk.clear();
            let read_limit = chunk
                .uncompressed_len
                .checked_add(1)
                .ok_or(CheckpointError::LengthOverflow)?;
            let decoder = zstd::stream::read::Decoder::new(compressed.as_slice())?;
            decoder.take(read_limit).read_to_end(&mut decoded_chunk)?;
            let actual = decoded_chunk.len() as u64;
            if actual != chunk.uncompressed_len {
                return Err(CheckpointError::ChunkDecodedLengthMismatch {
                    index: index as u32,
                    declared: chunk.uncompressed_len,
                    actual,
                });
            }
            raw.extend_from_slice(&decoded_chunk);
        }
        if *payload_hasher.finalize().as_bytes() != header.payload_checksum {
            return Err(CheckpointError::ChecksumMismatch);
        }
        if raw.len() != expected_raw_len {
            return Err(CheckpointError::UncompressedLengthMismatch {
                declared: header.uncompressed_len,
                actual: raw.len() as u64,
            });
        }

        let state: SolverState = postcard::from_bytes(&raw)?;
        if state.schema_version != SOLVER_STATE_VERSION {
            return Err(CheckpointError::SolverStateVersion {
                found: state.schema_version,
                expected: SOLVER_STATE_VERSION,
            });
        }
        if state.next_sample_id != header.next_sample_id {
            return Err(CheckpointError::SampleIdMismatch {
                header: header.next_sample_id,
                state: state.next_sample_id,
            });
        }
        Ok(Self { header, state })
    }
}

fn encode_header(header: &MultiwayCheckpointHeader) -> [u8; HEADER_LEN] {
    let mut bytes = [0u8; HEADER_LEN];
    let mut offset = 0;
    put(&mut bytes, &mut offset, CHECKPOINT_MAGIC);
    put(&mut bytes, &mut offset, &header.version.to_le_bytes());
    put(&mut bytes, &mut offset, &header.configuration_fingerprint);
    put(&mut bytes, &mut offset, &header.abstraction_fingerprint);
    put(
        &mut bytes,
        &mut offset,
        &header.next_sample_id.to_le_bytes(),
    );
    put(&mut bytes, &mut offset, &header.payload_len.to_le_bytes());
    put(
        &mut bytes,
        &mut offset,
        &header.uncompressed_len.to_le_bytes(),
    );
    put(&mut bytes, &mut offset, &header.payload_checksum);
    put(&mut bytes, &mut offset, &header.chunk_count.to_le_bytes());
    put(&mut bytes, &mut offset, &header.chunk_table_checksum);
    debug_assert_eq!(offset, HEADER_LEN);
    bytes
}

fn decode_header(bytes: &[u8; HEADER_LEN]) -> Result<MultiwayCheckpointHeader, CheckpointError> {
    if &bytes[..8] != CHECKPOINT_MAGIC {
        return Err(CheckpointError::BadMagic);
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().expect("two bytes"));
    if version != CHECKPOINT_VERSION {
        return Err(CheckpointError::UnsupportedVersion {
            found: version,
            expected: CHECKPOINT_VERSION,
        });
    }
    Ok(MultiwayCheckpointHeader {
        version,
        configuration_fingerprint: bytes[10..42].try_into().expect("32 bytes"),
        abstraction_fingerprint: bytes[42..74].try_into().expect("32 bytes"),
        next_sample_id: u64::from_le_bytes(bytes[74..82].try_into().expect("eight bytes")),
        payload_len: u64::from_le_bytes(bytes[82..90].try_into().expect("eight bytes")),
        uncompressed_len: u64::from_le_bytes(bytes[90..98].try_into().expect("eight bytes")),
        payload_checksum: bytes[98..130].try_into().expect("32 bytes"),
        chunk_count: u32::from_le_bytes(bytes[130..134].try_into().expect("four bytes")),
        chunk_table_checksum: bytes[134..166].try_into().expect("32 bytes"),
    })
}

fn chunk_count_for_len(uncompressed_len: u64) -> u32 {
    let chunk_count = uncompressed_len.div_ceil(CHUNK_UNCOMPRESSED_BYTES as u64);
    u32::try_from(chunk_count).expect("checkpoint size limit bounds chunk count")
}

fn expected_chunk_len(uncompressed_len: u64, index: u32, chunk_count: u32) -> u64 {
    debug_assert!(index < chunk_count);
    if index + 1 < chunk_count {
        CHUNK_UNCOMPRESSED_BYTES as u64
    } else {
        uncompressed_len - u64::from(index) * CHUNK_UNCOMPRESSED_BYTES as u64
    }
}

fn encode_chunk_table(chunks: &[ChunkEntry]) -> Vec<u8> {
    let capacity = chunks
        .len()
        .checked_mul(CHUNK_ENTRY_LEN)
        .expect("checkpoint size limit bounds chunk table");
    let mut bytes = Vec::with_capacity(capacity);
    for chunk in chunks {
        bytes.extend_from_slice(&chunk.compressed_offset.to_le_bytes());
        bytes.extend_from_slice(&chunk.compressed_len.to_le_bytes());
        bytes.extend_from_slice(&chunk.uncompressed_len.to_le_bytes());
        bytes.extend_from_slice(&chunk.checksum);
    }
    bytes
}

fn decode_chunk_table(bytes: &[u8]) -> Vec<ChunkEntry> {
    debug_assert_eq!(bytes.len() % CHUNK_ENTRY_LEN, 0);
    bytes
        .chunks_exact(CHUNK_ENTRY_LEN)
        .map(|chunk| ChunkEntry {
            compressed_offset: u64::from_le_bytes(chunk[0..8].try_into().expect("eight bytes")),
            compressed_len: u64::from_le_bytes(chunk[8..16].try_into().expect("eight bytes")),
            uncompressed_len: u64::from_le_bytes(chunk[16..24].try_into().expect("eight bytes")),
            checksum: chunk[24..56].try_into().expect("32 bytes"),
        })
        .collect()
}

fn size_of_chunk_entry() -> usize {
    std::mem::size_of::<ChunkEntry>()
}

fn try_reserve_u8(target: &mut Vec<u8>, additional: usize) -> Result<(), CheckpointError> {
    target
        .try_reserve_exact(additional)
        .map_err(|_| CheckpointError::AllocationFailed {
            requested: u64::try_from(additional).unwrap_or(u64::MAX),
        })
}

fn try_reserve_bytes<T>(
    target: &mut Vec<T>,
    additional: usize,
    element_size: usize,
) -> Result<(), CheckpointError> {
    target
        .try_reserve_exact(additional)
        .map_err(|_| CheckpointError::AllocationFailed {
            requested: u64::try_from(additional.saturating_mul(element_size)).unwrap_or(u64::MAX),
        })
}

fn put(target: &mut [u8], offset: &mut usize, source: &[u8]) {
    let end = *offset + source.len();
    target[*offset..end].copy_from_slice(source);
    *offset = end;
}

#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("checkpoint has bad magic bytes")]
    BadMagic,
    #[error("checkpoint version {found} is unsupported (expected {expected})")]
    UnsupportedVersion { found: u16, expected: u16 },
    #[error("checkpoint is shorter than its header")]
    Truncated,
    #[error("checkpoint length overflow")]
    LengthOverflow,
    #[error("checkpoint buffer allocation of {requested} bytes failed")]
    AllocationFailed { requested: u64 },
    #[error("checkpoint length differs from header: declared {declared}, actual {actual}")]
    LengthMismatch { declared: u64, actual: u64 },
    #[error("checkpoint uncompressed payload {declared} exceeds limit {limit}")]
    UncompressedTooLarge { declared: u64, limit: u64 },
    #[error("checkpoint chunk count differs: declared {declared}, expected {expected}")]
    ChunkCountMismatch { declared: u32, expected: u32 },
    #[error("checkpoint chunk table checksum mismatch")]
    ChunkTableChecksumMismatch,
    #[error(
        "checkpoint chunk {index} uncompressed length differs: declared {declared}, expected {expected}"
    )]
    ChunkUncompressedLengthInvalid {
        index: u32,
        declared: u64,
        expected: u64,
    },
    #[error("checkpoint chunk {index} offset differs: declared {declared}, expected {expected}")]
    ChunkOffsetMismatch {
        index: u32,
        declared: u64,
        expected: u64,
    },
    #[error(
        "checkpoint chunk {index} compressed length {declared} is outside the valid range 1..={limit}"
    )]
    ChunkCompressedLengthInvalid {
        index: u32,
        declared: u64,
        limit: u64,
    },
    #[error("checkpoint payload length differs: declared {declared}, indexed {indexed}")]
    ChunkPayloadLengthMismatch { declared: u64, indexed: u64 },
    #[error("checkpoint chunk {index} checksum mismatch")]
    ChunkChecksumMismatch { index: u32 },
    #[error(
        "checkpoint chunk {index} decoded length differs: declared {declared}, actual {actual}"
    )]
    ChunkDecodedLengthMismatch {
        index: u32,
        declared: u64,
        actual: u64,
    },
    #[error("checkpoint uncompressed length differs: declared {declared}, actual {actual}")]
    UncompressedLengthMismatch { declared: u64, actual: u64 },
    #[error("checkpoint payload checksum mismatch")]
    ChecksumMismatch,
    #[error("checkpoint belongs to a different solver configuration")]
    ConfigurationMismatch,
    #[error("checkpoint belongs to a different card abstraction")]
    AbstractionMismatch,
    #[error("solver state version {found} is unsupported (expected {expected})")]
    SolverStateVersion { found: u16, expected: u16 },
    #[error("checkpoint sample id differs between header ({header}) and state ({state})")]
    SampleIdMismatch { header: u64, state: u64 },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checkpoint codec error: {0}")]
    Codec(#[from] postcard::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom};

    use crate::solver::{
        HistoryEntry, HistoryKey, InfoKey, PolicyColumn, PolicyEntry, SolverConfig,
    };

    fn state() -> SolverState {
        SolverState {
            schema_version: SOLVER_STATE_VERSION,
            config: SolverConfig {
                seed: 9,
                max_memory_bytes: 1 << 20,
                max_traversal_depth: 64,
                exploration_epsilon: crate::solver::DEFAULT_EXPLORATION_EPSILON,
                discount_every: crate::solver::DEFAULT_DISCOUNT_EVERY,
                discount_until: crate::solver::DEFAULT_DISCOUNT_UNTIL,
            },
            traversals: 6,
            completed_sweeps: 2,
            next_sample_id: 6,
            total_deal_attempts: 8,
            terminal_evaluations: 20,
            histories: vec![HistoryEntry {
                key: HistoryKey::ROOT.child(1, 2),
                parent: HistoryKey::ROOT,
                actor: 1,
                action_index: 2,
                action_label: "raise".into(),
            }],
            policies: vec![PolicyEntry {
                key: InfoKey {
                    history: HistoryKey::ROOT.child(1, 2),
                    player: 2,
                    street: 1,
                    active_opponents: 2,
                    bucket_path: [
                        4,
                        17,
                        crate::solver::UNREACHED_BUCKET,
                        crate::solver::UNREACHED_BUCKET,
                    ],
                },
                column: PolicyColumn {
                    action_labels: vec!["fold".into(), "call".into()],
                    regrets: vec![-1.0, 2.5],
                    strategy_sum: vec![3.0, 7.0],
                },
            }],
        }
    }

    fn multi_chunk_state() -> SolverState {
        let mut state = state();
        state.histories[0].action_label = "x".repeat(CHUNK_UNCOMPRESSED_BYTES + 4096);
        assert!(
            postcard::to_allocvec(&state).unwrap().len() > CHUNK_UNCOMPRESSED_BYTES,
            "fixture must cross a production chunk boundary"
        );
        state
    }

    fn read_layout(path: &Path) -> (MultiwayCheckpointHeader, Vec<ChunkEntry>, u64) {
        let mut file = File::open(path).unwrap();
        let mut encoded_header = [0u8; HEADER_LEN];
        file.read_exact(&mut encoded_header).unwrap();
        let header = decode_header(&encoded_header).unwrap();
        let table_len = header.chunk_count as usize * CHUNK_ENTRY_LEN;
        let mut encoded_table = vec![0u8; table_len];
        file.read_exact(&mut encoded_table).unwrap();
        let chunks = decode_chunk_table(&encoded_table);
        let payload_start = HEADER_LEN as u64 + table_len as u64;
        (header, chunks, payload_start)
    }

    #[test]
    fn chunked_checkpoint_round_trips_multiple_chunks_and_overwrites() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("solve.mwckpt");
        let configuration = [3; 32];
        let abstraction = [7; 32];
        let checkpoint = MultiwayCheckpoint::new(multi_chunk_state(), configuration, abstraction);
        checkpoint.write_atomic(&path).unwrap();
        // Persisting over an existing destination exercises the atomic
        // replacement path used by periodic checkpoints.
        checkpoint.write_atomic(&path).unwrap();

        let loaded = MultiwayCheckpoint::load(&path, configuration, abstraction).unwrap();
        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.header.configuration_fingerprint, configuration);
        assert_eq!(loaded.header.abstraction_fingerprint, abstraction);
        assert!(loaded.header.payload_len > 0);
        assert!(loaded.header.uncompressed_len > CHUNK_UNCOMPRESSED_BYTES as u64);
        assert_eq!(
            loaded.header.chunk_count,
            chunk_count_for_len(loaded.header.uncompressed_len)
        );
        assert!(loaded.header.chunk_count > 1);
    }

    #[test]
    fn identical_state_produces_identical_checkpoint_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.mwcp");
        let second = directory.path().join("second.mwcp");
        let checkpoint = MultiwayCheckpoint::new(state(), [3; 32], [7; 32]);
        checkpoint.write_atomic(&first).unwrap();
        checkpoint.write_atomic(&second).unwrap();
        assert_eq!(
            std::fs::read(first).unwrap(),
            std::fs::read(second).unwrap()
        );
    }

    #[test]
    fn chunk_checksum_identifies_corrupted_chunk() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("corrupt.mwckpt");
        MultiwayCheckpoint::new(multi_chunk_state(), [1; 32], [2; 32])
            .write_atomic(&path)
            .unwrap();
        let (_, chunks, payload_start) = read_layout(&path);
        assert!(chunks.len() > 1);
        let corruption_offset =
            payload_start + chunks[0].compressed_offset + chunks[0].compressed_len / 2;

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.seek(SeekFrom::Start(corruption_offset)).unwrap();
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).unwrap();
        file.seek(SeekFrom::Start(corruption_offset)).unwrap();
        file.write_all(&[byte[0] ^ 0x80]).unwrap();
        file.sync_all().unwrap();

        assert!(matches!(
            MultiwayCheckpoint::load_unchecked(&path),
            Err(CheckpointError::ChunkChecksumMismatch { index: 0 })
        ));
    }

    #[test]
    fn truncated_chunk_payload_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("truncated.mwckpt");
        MultiwayCheckpoint::new(state(), [1; 32], [2; 32])
            .write_atomic(&path)
            .unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        let original_len = file.metadata().unwrap().len();
        file.set_len(original_len - 1).unwrap();
        file.sync_all().unwrap();

        assert!(matches!(
            MultiwayCheckpoint::load_unchecked(&path),
            Err(CheckpointError::Truncated)
        ));
    }

    #[test]
    fn chunk_offsets_are_validated_even_with_a_valid_table_checksum() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bad-offset.mwckpt");
        MultiwayCheckpoint::new(state(), [1; 32], [2; 32])
            .write_atomic(&path)
            .unwrap();
        let (mut header, mut chunks, _) = read_layout(&path);
        chunks[0].compressed_offset = 1;
        let encoded_table = encode_chunk_table(&chunks);
        header.chunk_table_checksum = *blake3::hash(&encoded_table).as_bytes();

        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&encode_header(&header)).unwrap();
        file.write_all(&encoded_table).unwrap();
        file.sync_all().unwrap();

        assert!(matches!(
            MultiwayCheckpoint::load_unchecked(&path),
            Err(CheckpointError::ChunkOffsetMismatch {
                index: 0,
                declared: 1,
                expected: 0
            })
        ));
    }

    #[test]
    fn declared_uncompressed_size_is_bounded_before_table_allocation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversize.mwckpt");
        MultiwayCheckpoint::new(state(), [1; 32], [2; 32])
            .write_atomic(&path)
            .unwrap();
        let (mut header, _, _) = read_layout(&path);
        header.uncompressed_len = MAX_UNCOMPRESSED_BYTES + 1;

        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&encode_header(&header)).unwrap();
        file.sync_all().unwrap();

        assert!(matches!(
            MultiwayCheckpoint::load_unchecked(&path),
            Err(CheckpointError::UncompressedTooLarge {
                declared,
                limit: MAX_UNCOMPRESSED_BYTES
            }) if declared == MAX_UNCOMPRESSED_BYTES + 1
        ));
    }

    #[test]
    fn impossible_raw_reservation_fails_gracefully() {
        let mut raw = Vec::new();
        assert!(matches!(
            try_reserve_u8(&mut raw, usize::MAX),
            Err(CheckpointError::AllocationFailed { .. })
        ));
        assert!(raw.is_empty());
    }

    #[test]
    fn compatibility_hashes_are_enforced_before_resume() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hashes.mwcp");
        MultiwayCheckpoint::new(state(), [1; 32], [2; 32])
            .write_atomic(&path)
            .unwrap();
        assert!(matches!(
            MultiwayCheckpoint::load(&path, [9; 32], [2; 32]),
            Err(CheckpointError::ConfigurationMismatch)
        ));
        assert!(matches!(
            MultiwayCheckpoint::load(&path, [1; 32], [9; 32]),
            Err(CheckpointError::AbstractionMismatch)
        ));
    }
}
