//! `.ckpt` checkpoint codec: a fixed manual-layout header (magic, format
//! version, config hash, iteration) followed by a zstd-compressed postcard
//! encoding of `engine::SolverState`.
//!
//! Writes are atomic (temp file in the same directory, then renamed into
//! place) so a process killed mid-write never corrupts a previously-good
//! checkpoint.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use engine::{SolverState, SolverStateRef};

/// Fixed header layout, all multi-byte fields little-endian: magic (8
/// bytes) + format version (u16) + config hash (32 bytes) + iteration
/// (u64) = 50 bytes. The iteration is duplicated from the (compressed)
/// payload so the header alone can report progress without paying for a
/// zstd decompression.
pub const HEADER_LEN: usize = 8 + 2 + 32 + 8;

const MAGIC: &[u8; 8] = b"SLVRCKPT";
// The state envelope is unchanged, but postflop private indices now use
// per-seat root support. Never restore a dense v1 state into this layout.
const FORMAT_VERSION: u16 = 2;

/// Errors from reading or writing a `.ckpt` file.
#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("not a solvers checkpoint file (bad magic bytes)")]
    BadMagic,
    #[error("unsupported checkpoint format version {found} (expected {expected})")]
    BadVersion { found: u16, expected: u16 },
    #[error("checkpoint file truncated: expected at least {expected} bytes, got {actual}")]
    Truncated { expected: usize, actual: usize },
    #[error("checkpoint I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checkpoint payload codec error: {0}")]
    Codec(#[from] postcard::Error),
}

/// Just the header, for cheap progress checks without decompressing the
/// (potentially large) payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckpointHeader {
    pub config_hash: [u8; 32],
    pub iteration: u64,
}

/// A fully decoded checkpoint: header fields plus the restored solver
/// state.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoint {
    pub config_hash: [u8; 32],
    pub iteration: u64,
    pub state: SolverState,
}

fn parse_header(buf: &[u8; HEADER_LEN]) -> Result<CheckpointHeader, CheckpointError> {
    if &buf[0..8] != MAGIC {
        return Err(CheckpointError::BadMagic);
    }
    let version = u16::from_le_bytes([buf[8], buf[9]]);
    if version != FORMAT_VERSION {
        return Err(CheckpointError::BadVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }
    let mut config_hash = [0u8; 32];
    config_hash.copy_from_slice(&buf[10..42]);
    let iteration = u64::from_le_bytes(buf[42..50].try_into().expect("8-byte slice"));
    Ok(CheckpointHeader {
        config_hash,
        iteration,
    })
}

fn build_header(config_hash: [u8; 32], iteration: u64) -> [u8; HEADER_LEN] {
    let mut buf = [0u8; HEADER_LEN];
    buf[0..8].copy_from_slice(MAGIC);
    buf[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    buf[10..42].copy_from_slice(&config_hash);
    buf[42..50].copy_from_slice(&iteration.to_le_bytes());
    buf
}

/// Reads and fully decodes a checkpoint (header + zstd-decompressed,
/// postcard-decoded `SolverState`).
pub fn read_checkpoint(path: &Path) -> Result<Checkpoint, CheckpointError> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size < HEADER_LEN as u64 {
        return Err(CheckpointError::Truncated {
            expected: HEADER_LEN,
            actual: size as usize,
        });
    }
    let mut header_buf = [0; HEADER_LEN];
    file.read_exact(&mut header_buf)?;
    let header = parse_header(&header_buf)?;
    let payload = zstd::decode_all(BufReader::new(file))?;
    let state: SolverState = postcard::from_bytes(&payload)?;
    Ok(Checkpoint {
        config_hash: header.config_hash,
        iteration: header.iteration,
        state,
    })
}

/// Writes a checkpoint atomically: header+payload go to a temp file in
/// `path`'s directory, `fsync`ed, then renamed into place.
pub fn write_checkpoint(
    path: &Path,
    config_hash: [u8; 32],
    state: &SolverState,
) -> Result<(), CheckpointError> {
    write_checkpoint_payload(path, config_hash, state.iteration, state)
}

/// Writes a live solver's borrowed state with the same payload as
/// [`write_checkpoint`], without allocating an owned copy of its storage.
pub fn write_checkpoint_ref(
    path: &Path,
    config_hash: [u8; 32],
    state: &SolverStateRef<'_>,
) -> Result<(), CheckpointError> {
    write_checkpoint_payload(path, config_hash, state.iteration, state)
}

fn write_checkpoint_payload<T: serde::Serialize>(
    path: &Path,
    config_hash: [u8; 32],
    iteration: u64,
    state: &T,
) -> Result<(), CheckpointError> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
    {
        let file = temporary.as_file_mut();
        file.write_all(&build_header(config_hash, iteration))?;
        let mut encoder = zstd::Encoder::new(&mut *file, 0)?;
        encoder.include_checksum(true)?;
        // Buffer postcard's small writes before compression. Neither the
        // complete serialized payload nor a compressed copy lives in RAM.
        let mut buffered = BufWriter::with_capacity(64 * 1024, encoder);
        postcard::to_io(state, &mut buffered)?;
        buffered.flush()?;
        buffered
            .into_inner()
            .map_err(|e| e.into_error())?
            .finish()?;
        file.sync_all()?;
    }
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{F32Storage, I16Storage, Storage, StorageState};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "formats-ckpt-test-{}-{}-{}",
            std::process::id(),
            id,
            name
        ))
    }

    fn sample_state_f32() -> SolverState {
        SolverState {
            iteration: 42,
            storage: StorageState::F32 {
                regrets: vec![1.0, -2.0, 3.5],
                strategy_sum: vec![0.1, 0.2, 0.3],
            },
        }
    }

    fn sample_state_i16() -> SolverState {
        SolverState {
            iteration: 7,
            storage: StorageState::I16 {
                regrets: vec![1, -2, 3],
                strategy_sum: vec![4, 5, 6],
                regret_scales: vec![0.01, 0.02],
                strategy_scales: vec![0.03, 0.04],
            },
        }
    }

    #[test]
    fn round_trip_f32() {
        let path = temp_path("f32.ckpt");
        let hash = [7u8; 32];
        let state = sample_state_f32();
        write_checkpoint(&path, hash, &state).unwrap();
        let loaded = read_checkpoint(&path).unwrap();
        assert_eq!(loaded.config_hash, hash);
        assert_eq!(loaded.iteration, state.iteration);
        assert_eq!(loaded.state, state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn round_trip_i16() {
        let path = temp_path("i16.ckpt");
        let hash = [9u8; 32];
        let state = sample_state_i16();
        write_checkpoint(&path, hash, &state).unwrap();
        let loaded = read_checkpoint(&path).unwrap();
        assert_eq!(loaded.state, state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn streaming_writer_preserves_payload_and_reads_single_buffer_frames() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("stream.ckpt");
        for state in [sample_state_f32(), sample_state_i16()] {
            let encoded = postcard::to_allocvec(&state).unwrap();
            write_checkpoint(&path, [7; 32], &state).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(zstd::decode_all(&bytes[HEADER_LEN..]).unwrap(), encoded);
            let mut legacy = build_header([7; 32], state.iteration).to_vec();
            legacy.extend(zstd::encode_all(encoded.as_slice(), 0).unwrap());
            std::fs::write(&path, legacy).unwrap();
            assert_eq!(read_checkpoint(&path).unwrap().state, state);
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    fn check_borrowed_writer<S: Storage>(mut storage: S, state: SolverState) {
        storage.restore_state(state.storage.clone()).unwrap();
        let borrowed = SolverStateRef {
            iteration: state.iteration,
            storage: storage.state_ref(),
        };
        let legacy = postcard::to_allocvec(&state).unwrap();
        assert_eq!(postcard::to_allocvec(&borrowed).unwrap(), legacy);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("borrowed.ckpt");
        write_checkpoint_ref(&path, [11; 32], &borrowed).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(zstd::decode_all(&bytes[HEADER_LEN..]).unwrap(), legacy);
        let checkpoint = read_checkpoint(&path).unwrap();
        assert_eq!(checkpoint.config_hash, [11; 32]);
        assert_eq!(checkpoint.iteration, state.iteration);
        assert_eq!(checkpoint.state, state);
        assert_eq!(storage.state(), state.storage);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn borrowed_f32_writer_preserves_owned_payload() {
        check_borrowed_writer(F32Storage::new(3, 0), sample_state_f32());
    }

    #[test]
    fn borrowed_i16_writer_preserves_owned_payload() {
        check_borrowed_writer(I16Storage::new(3, 2), sample_state_i16());
    }

    #[test]
    fn rejects_dense_v1_checkpoint_before_decoding_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dense-v1.ckpt");
        write_checkpoint(&path, [0; 32], &sample_state_f32()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(
            read_checkpoint(&path),
            Err(CheckpointError::BadVersion {
                found: 1,
                expected: 2
            })
        ));
    }

    #[test]
    fn rejects_bad_magic() {
        let path = temp_path("badmagic.ckpt");
        write_checkpoint(&path, [0u8; 32], &sample_state_f32()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[0] = b'X';
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            read_checkpoint(&path),
            Err(CheckpointError::BadMagic)
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_bad_version() {
        let path = temp_path("badversion.ckpt");
        write_checkpoint(&path, [0u8; 32], &sample_state_f32()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        match read_checkpoint(&path) {
            Err(CheckpointError::BadVersion { found, expected }) => {
                assert_eq!(found, 99);
                assert_eq!(expected, FORMAT_VERSION);
            }
            other => panic!("expected BadVersion, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_truncated_header() {
        let path = temp_path("truncated-header.ckpt");
        write_checkpoint(&path, [0u8; 32], &sample_state_f32()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..HEADER_LEN - 5]).unwrap();
        match read_checkpoint(&path) {
            Err(CheckpointError::Truncated { expected, actual }) => {
                assert_eq!(expected, HEADER_LEN);
                assert_eq!(actual, HEADER_LEN - 5);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_truncated_payload() {
        let path = temp_path("truncated-payload.ckpt");
        write_checkpoint(&path, [0u8; 32], &sample_state_f32()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // Keep the full header but chop the zstd payload short.
        let cut = bytes.len() - 3;
        std::fs::write(&path, &bytes[..cut]).unwrap();
        assert!(read_checkpoint(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_is_atomic_leaves_no_tmp_file_behind() {
        let path = temp_path("atomic.ckpt");
        write_checkpoint(&path, [1u8; 32], &sample_state_f32()).unwrap();
        let dir = path.parent().unwrap();
        let tmp_name = format!(".{}.tmp", path.file_name().unwrap().to_str().unwrap());
        assert!(!dir.join(tmp_name).exists());
        let _ = std::fs::remove_file(&path);
    }
}
