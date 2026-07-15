//! Versioned, checksummed, atomically replaced multiway solver checkpoints.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::solver::{ExternalSamplingGame, MultiwaySolver, SOLVER_STATE_VERSION, SolverState};

const CHECKPOINT_MAGIC: &[u8; 8] = b"SLVRMWCP";
pub const CHECKPOINT_VERSION: u16 = 2;
const HEADER_LEN: usize = 8 + 2 + 32 + 32 + 8 + 8 + 8 + 32;
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

        let raw = postcard::to_allocvec(&self.state)?;
        let payload = zstd::stream::encode_all(raw.as_slice(), 3)?;
        let mut header = self.header.clone();
        header.version = CHECKPOINT_VERSION;
        header.payload_len = payload.len() as u64;
        header.uncompressed_len = raw.len() as u64;
        header.payload_checksum = *blake3::hash(&payload).as_bytes();

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let directory = parent.unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(&encode_header(&header))?;
        temporary.write_all(&payload)?;
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

        let expected_file_len = (HEADER_LEN as u64)
            .checked_add(header.payload_len)
            .ok_or(CheckpointError::LengthOverflow)?;
        if file_len != expected_file_len {
            return Err(CheckpointError::LengthMismatch {
                declared: expected_file_len,
                actual: file_len,
            });
        }
        if header.uncompressed_len > MAX_UNCOMPRESSED_BYTES {
            return Err(CheckpointError::UncompressedTooLarge {
                declared: header.uncompressed_len,
                limit: MAX_UNCOMPRESSED_BYTES,
            });
        }

        let payload_len =
            usize::try_from(header.payload_len).map_err(|_| CheckpointError::LengthOverflow)?;
        let mut payload = vec![0u8; payload_len];
        file.read_exact(&mut payload)?;
        let actual_checksum = *blake3::hash(&payload).as_bytes();
        if actual_checksum != header.payload_checksum {
            return Err(CheckpointError::ChecksumMismatch);
        }

        let expected_raw_len = usize::try_from(header.uncompressed_len)
            .map_err(|_| CheckpointError::LengthOverflow)?;
        let read_limit = header
            .uncompressed_len
            .checked_add(1)
            .ok_or(CheckpointError::LengthOverflow)?;
        let decoder = zstd::stream::read::Decoder::new(payload.as_slice())?;
        let mut raw = Vec::with_capacity(expected_raw_len);
        decoder.take(read_limit).read_to_end(&mut raw)?;
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
    #[error("checkpoint length differs from header: declared {declared}, actual {actual}")]
    LengthMismatch { declared: u64, actual: u64 },
    #[error("checkpoint uncompressed payload {declared} exceeds limit {limit}")]
    UncompressedTooLarge { declared: u64, limit: u64 },
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

    #[test]
    fn compressed_checkpoint_round_trips_and_overwrites() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("solve.mwcp");
        let configuration = [3; 32];
        let abstraction = [7; 32];
        let checkpoint = MultiwayCheckpoint::new(state(), configuration, abstraction);
        checkpoint.write_atomic(&path).unwrap();
        // Persisting over an existing destination exercises the atomic
        // replacement path used by periodic checkpoints.
        checkpoint.write_atomic(&path).unwrap();

        let loaded = MultiwayCheckpoint::load(&path, configuration, abstraction).unwrap();
        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.header.configuration_fingerprint, configuration);
        assert_eq!(loaded.header.abstraction_fingerprint, abstraction);
        assert!(loaded.header.payload_len > 0);
        assert!(loaded.header.uncompressed_len > 0);
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
    fn checksum_detects_payload_corruption() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("corrupt.mwcp");
        MultiwayCheckpoint::new(state(), [1; 32], [2; 32])
            .write_atomic(&path)
            .unwrap();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        file.write_all(&[byte[0] ^ 0x80]).unwrap();
        file.sync_all().unwrap();

        assert!(matches!(
            MultiwayCheckpoint::load_unchecked(&path),
            Err(CheckpointError::ChecksumMismatch)
        ));
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
