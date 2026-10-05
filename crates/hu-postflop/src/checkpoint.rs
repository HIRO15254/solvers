//! `.ckpt` checkpoint codec: a fixed manual-layout header (magic, format
//! version, config hash, iteration) followed by a zstd-compressed postcard
//! encoding of `hu_engine::SolverState`.
//!
//! Writes are atomic (temp file in the same directory, then renamed into
//! place) so a process killed mid-write never corrupts a previously-good
//! checkpoint.

use std::io::Write;
use std::path::Path;

use hu_engine::SolverState;

/// Fixed header layout, all multi-byte fields little-endian: magic (8
/// bytes) + format version (u16) + config hash (32 bytes) + iteration
/// (u64) = 50 bytes. The iteration is duplicated from the (compressed)
/// payload so the header alone can report progress without paying for a
/// zstd decompression.
pub const HEADER_LEN: usize = 8 + 2 + 32 + 8;

const MAGIC: &[u8; 8] = b"SLVRCKPT";
const FORMAT_VERSION: u16 = 1;

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
    /// Present only in common-input checkpoints (format 2).
    pub config_toml: Option<String>,
    /// Cumulative solve time, excluding tree validation/build and artifact export.
    pub elapsed_secs: Option<f64>,
}

fn parse_header(buf: &[u8; HEADER_LEN]) -> Result<CheckpointHeader, CheckpointError> {
    if &buf[0..8] != MAGIC {
        return Err(CheckpointError::BadMagic);
    }
    let version = u16::from_le_bytes([buf[8], buf[9]]);
    if version != FORMAT_VERSION && version != 2 {
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
    let bytes = std::fs::read(path)?;
    if bytes.len() < HEADER_LEN {
        return Err(CheckpointError::Truncated {
            expected: HEADER_LEN,
            actual: bytes.len(),
        });
    }
    let header_buf: [u8; HEADER_LEN] = bytes[..HEADER_LEN]
        .try_into()
        .expect("sliced to HEADER_LEN");
    let header = parse_header(&header_buf)?;
    let payload = zstd::decode_all(&bytes[HEADER_LEN..])?;
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    let (state, config_toml, elapsed_secs) = if version == 2 {
        let (state, config, elapsed): (SolverState, String, f64) = postcard::from_bytes(&payload)?;
        (state, Some(config), Some(elapsed))
    } else {
        (postcard::from_bytes(&payload)?, None, None)
    };
    Ok(Checkpoint {
        config_hash: header.config_hash,
        iteration: header.iteration,
        state,
        config_toml,
        elapsed_secs,
    })
}

/// Writes a checkpoint atomically: header+payload go to a temp file in
/// `path`'s directory, `fsync`ed, then renamed into place.
pub fn write_checkpoint(
    path: &Path,
    config_hash: [u8; 32],
    state: &SolverState,
) -> Result<(), CheckpointError> {
    let payload = postcard::to_allocvec(state)?;
    let compressed = zstd::encode_all(payload.as_slice(), 0)?;

    let mut buf = Vec::with_capacity(HEADER_LEN + compressed.len());
    buf.extend_from_slice(&build_header(config_hash, state.iteration));
    buf.extend_from_slice(&compressed);

    write_atomic(path, &buf)
}

/// Self-contained common-input checkpoint. The hash excludes [run]; the
/// embedded config preserves the complete normalized effective input.
/// Legacy callers continue writing byte-identical format 1 checkpoints.
pub fn write_checkpoint_with_config(
    path: &Path,
    compatibility_hash: [u8; 32],
    state: &SolverState,
    effective_config: &str,
    elapsed_secs: f64,
) -> Result<(), CheckpointError> {
    let payload = postcard::to_allocvec(&(state, effective_config, elapsed_secs))?;
    let compressed = zstd::encode_all(payload.as_slice(), 0)?;
    let mut header = build_header(compatibility_hash, state.iteration);
    header[8..10].copy_from_slice(&2u16.to_le_bytes());
    let mut buf = Vec::with_capacity(HEADER_LEN + compressed.len());
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&compressed);
    write_atomic(path, &buf)
}

fn write_atomic(path: &Path, buf: &[u8]) -> Result<(), CheckpointError> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let tmp_name = format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("checkpoint.ckpt")
    );
    let tmp_path = dir.join(tmp_name);
    {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(buf)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hu_engine::StorageState;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "hu-postflop-ckpt-test-{}-{}-{}",
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
    fn common_input_round_trip_preserves_config_and_elapsed_time() {
        let path = temp_path("nlh.ckpt");
        let state = sample_state_f32();
        let config = "schema = \"solvers.nlh/v1\"\n";
        write_checkpoint_with_config(&path, [8; 32], &state, config, 12.5).unwrap();
        let checkpoint = read_checkpoint(&path).unwrap();
        assert_eq!(checkpoint.state, state);
        assert_eq!(checkpoint.config_hash, [8; 32]);
        assert_eq!(checkpoint.config_toml.as_deref(), Some(config));
        assert_eq!(checkpoint.elapsed_secs, Some(12.5));
        std::fs::remove_file(path).unwrap();
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
