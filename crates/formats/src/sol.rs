//! `.sol` viewer-artifact codec: a compact, read-only export of a solved
//! postflop game's *normalized average strategy*, quantized to 16-bit
//! fixed point, plus the original config TOML and cached solve metadata.
//!
//! This is deliberately not a superset of `.ckpt`: `.ckpt` stores
//! full-precision regrets and strategy sums so a solve can resume bit-for-
//! bit, while `.sol` throws that away and keeps only what a viewer needs to
//! render strategies — the game tree itself is never serialized because it
//! is rebuilt deterministically from `config_toml` at load time. In
//! `NoRivers` mode river-street action nodes are omitted entirely (the
//! viewer re-solves the river lazily on demand), which is what keeps these
//! files small for deep trees.
//!
//! The on-disk layout mirrors `checkpoint.rs` exactly: a fixed 50-byte
//! little-endian header (magic + format version + config hash + iteration)
//! followed by a zstd-compressed postcard encoding of the payload, written
//! atomically via temp-file-then-rename.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::hash::{config_hash, config_hash_hex};

/// Fixed header layout, all multi-byte fields little-endian: magic (8
/// bytes) + format version (u16) + config hash (32 bytes) + iteration
/// (u64) = 50 bytes. Same shape as `checkpoint::HEADER_LEN` so both formats
/// can share tooling that only needs to peek progress/identity without
/// paying for a zstd decompression.
pub const HEADER_LEN: usize = 8 + 2 + 32 + 8;

const MAGIC: &[u8; 8] = b"SLVRSOLV";
const FORMAT_VERSION: u16 = 1;

/// Errors from reading or writing a `.sol` file.
#[derive(Debug, thiserror::Error)]
pub enum SolError {
    #[error("not a solvers .sol file (bad magic bytes)")]
    BadMagic,
    #[error("unsupported .sol format version {found} (expected {expected})")]
    BadVersion { found: u16, expected: u16 },
    #[error(".sol file truncated: expected at least {expected} bytes, got {actual}")]
    Truncated { expected: usize, actual: usize },
    #[error(".sol header hash {header_hash} does not match blake3(config_toml) {computed_hash}")]
    HashMismatch {
        header_hash: String,
        computed_hash: String,
    },
    #[error(".sol I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error(".sol payload codec error: {0}")]
    Codec(#[from] postcard::Error),
}

/// Just the header, for cheap identity/progress checks without
/// decompressing the (potentially large) payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SolHeader {
    pub config_hash: [u8; 32],
    pub iteration: u64,
}

/// Cached solve metadata: results a viewer wants to display immediately
/// without recomputing exploitability itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolMeta {
    pub iterations: u64,
    pub expl: [f64; 2],
    pub ev: [f64; 2],
    pub nash_conv: f64,
    /// Informational only: the storage representation ("f32" | "i16") the
    /// solve ran with. Never round-tripped back into `engine::StorageState`
    /// — `.sol` always quantizes to u16 fixed point regardless of this.
    pub storage: String,
    /// Informational only: wall-clock seconds the solve took.
    pub wall_secs: f64,
}

/// Which streets have stored strategy blocks. `NoRivers` artifacts omit
/// river action nodes entirely (see module docs); the viewer must re-solve
/// those subtrees on demand rather than expect a block for every `sref`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreetsStored {
    Full,
    NoRivers,
}

/// One action node's quantized strategy. `probs` is action-major (all
/// hands for action 0, then all hands for action 1, ...) u16 fixed point,
/// stored as raw little-endian bytes rather than `Vec<u16>` so postcard
/// serializes it as a length-prefixed byte blob instead of a varint-per-
/// element sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyBlock {
    /// Index into the rebuilt tree's `storage_refs` (i.e. the action
    /// node's `Node::aux`). Stable across a load because tree construction
    /// from a given config is deterministic, so this never needs to be
    /// resolved through any other indirection.
    pub sref: u32,
    pub probs: Vec<u8>,
}

/// The full decoded `.sol` contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolPayload {
    /// Full original config TOML text, kept verbatim (not just its hash)
    /// so the viewer can rebuild the exact same tree deterministically.
    /// The header's config hash MUST equal `blake3(config_toml.as_bytes())`
    /// — `write_sol` derives it from this field and `read_sol` verifies it,
    /// so a `.sol` can never silently drift from the config it describes.
    pub config_toml: String,
    pub meta: SolMeta,
    pub mode: StreetsStored,
    /// One entry per stored action node, ascending by `sref`.
    pub blocks: Vec<StrategyBlock>,
}

fn parse_header(buf: &[u8; HEADER_LEN]) -> Result<SolHeader, SolError> {
    if &buf[0..8] != MAGIC {
        return Err(SolError::BadMagic);
    }
    let version = u16::from_le_bytes([buf[8], buf[9]]);
    if version != FORMAT_VERSION {
        return Err(SolError::BadVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }
    let mut config_hash = [0u8; 32];
    config_hash.copy_from_slice(&buf[10..42]);
    let iteration = u64::from_le_bytes(buf[42..50].try_into().expect("8-byte slice"));
    Ok(SolHeader {
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

/// Reads and fully decodes a `.sol` file (header + zstd-decompressed,
/// postcard-decoded `SolPayload`), verifying that the header hash matches
/// `blake3` of the embedded config TOML.
pub fn read_sol(path: &Path) -> Result<SolPayload, SolError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < HEADER_LEN {
        return Err(SolError::Truncated {
            expected: HEADER_LEN,
            actual: bytes.len(),
        });
    }
    let header_buf: [u8; HEADER_LEN] = bytes[..HEADER_LEN]
        .try_into()
        .expect("sliced to HEADER_LEN");
    let header = parse_header(&header_buf)?;
    let payload = zstd::decode_all(&bytes[HEADER_LEN..])?;
    let payload: SolPayload = postcard::from_bytes(&payload)?;

    let computed = config_hash(payload.config_toml.as_bytes());
    if computed != header.config_hash {
        return Err(SolError::HashMismatch {
            header_hash: config_hash_hex(&header.config_hash),
            computed_hash: config_hash_hex(&computed),
        });
    }

    Ok(payload)
}

/// Writes a `.sol` file atomically: header+payload go to a temp file in
/// `path`'s directory, `fsync`ed, then renamed into place. The header's
/// config hash and iteration are derived from `payload` itself (from
/// `config_toml` and `meta.iterations` respectively) so caller and header
/// can never disagree.
pub fn write_sol(path: &Path, payload: &SolPayload) -> Result<(), SolError> {
    let hash = config_hash(payload.config_toml.as_bytes());
    let compressed_payload = postcard::to_allocvec(payload)?;
    let compressed = zstd::encode_all(compressed_payload.as_slice(), 0)?;

    let mut buf = Vec::with_capacity(HEADER_LEN + compressed.len());
    buf.extend_from_slice(&build_header(hash, payload.meta.iterations));
    buf.extend_from_slice(&compressed);

    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let tmp_name = format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("solve.sol")
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

/// Quantizes an already-normalized probability slice (values in `[0, 1]`,
/// caller-guaranteed to be a valid strategy) to 16-bit fixed point:
/// `q = round(p * 65535)`. Deliberately does not renormalize on the way in
/// — `dequantize_probs` is responsible for restoring an exact per-hand sum
/// of 1.0 after the rounding error introduced here.
pub fn quantize_probs(probs: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(probs.len() * 2);
    for &p in probs {
        let q = (p * 65535.0).round().clamp(0.0, 65535.0) as u16;
        bytes.extend_from_slice(&q.to_le_bytes());
    }
    bytes
}

/// Dequantizes a `quantize_probs` byte blob back into `f32` probabilities,
/// renormalizing per hand column (stride `num_hands`, action-major layout)
/// so each hand's action probabilities sum to exactly 1.0 despite the
/// quantization rounding error. Falls back to a uniform distribution over
/// actions for a hand column whose quantized values all round to zero;
/// this cannot happen for genuine `average_strategy` output (every hand
/// reaches at least one action with positive probability) but is handled
/// defensively rather than dividing by zero.
///
/// `bytes.len()` must equal `num_actions * num_hands * 2`; returns
/// `SolError::Truncated` describing the mismatch (reusing that variant
/// rather than adding a dedicated one) instead of panicking, since `bytes`
/// ultimately comes from a file on disk that could be corrupt or
/// mismatched against a stale tree shape.
pub fn dequantize_probs(
    bytes: &[u8],
    num_actions: usize,
    num_hands: usize,
) -> Result<Vec<f32>, SolError> {
    let expected = num_actions * num_hands * 2;
    if bytes.len() != expected {
        return Err(SolError::Truncated {
            expected,
            actual: bytes.len(),
        });
    }

    let mut q = vec![0u32; num_actions * num_hands];
    for (i, chunk) in bytes.chunks_exact(2).enumerate() {
        q[i] = u16::from_le_bytes([chunk[0], chunk[1]]) as u32;
    }

    let mut out = vec![0f32; num_actions * num_hands];
    for h in 0..num_hands {
        let mut sum: u32 = 0;
        for a in 0..num_actions {
            sum += q[a * num_hands + h];
        }
        if sum == 0 {
            let uniform = 1.0 / num_actions as f32;
            for a in 0..num_actions {
                out[a * num_hands + h] = uniform;
            }
        } else {
            for a in 0..num_actions {
                out[a * num_hands + h] = q[a * num_hands + h] as f32 / sum as f32;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "formats-sol-test-{}-{}-{}",
            std::process::id(),
            id,
            name
        ))
    }

    fn sample_payload() -> SolPayload {
        SolPayload {
            config_toml: "[game]\nstreet = \"flop\"\n".to_string(),
            meta: SolMeta {
                iterations: 1234,
                expl: [0.5, 0.25],
                ev: [1.5, -1.5],
                nash_conv: 0.75,
                storage: "f32".to_string(),
                wall_secs: 12.5,
            },
            mode: StreetsStored::NoRivers,
            blocks: vec![
                StrategyBlock {
                    sref: 3,
                    probs: quantize_probs(&[
                        0.5, 0.25, 0.25, 0.25, // action 0, 4 hands
                        0.3, 0.5, 0.5, 0.5, // action 1, 4 hands
                        0.2, 0.25, 0.25, 0.25, // action 2, 4 hands
                    ]),
                },
                StrategyBlock {
                    sref: 9,
                    probs: quantize_probs(&[
                        1.0, 0.5, // action 0, 2 hands
                        0.0, 0.5, // action 1, 2 hands
                    ]),
                },
            ],
        }
    }

    #[test]
    fn round_trip() {
        let path = temp_path("round-trip.sol");
        let payload = sample_payload();
        write_sol(&path, &payload).unwrap();

        let loaded = read_sol(&path).unwrap();
        assert_eq!(loaded, payload);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn quantize_dequantize_round_trip() {
        // 3 actions x 5 hands, action-major, each column sums to 1.0.
        // Includes a uniform column and a one-hot column.
        #[rustfmt::skip]
        let probs: [f32; 15] = [
            // action 0
            0.2, 1.0 / 3.0, 1.0, 0.0, 0.6,
            // action 1
            0.3, 1.0 / 3.0, 0.0, 0.0, 0.1,
            // action 2
            0.5, 1.0 / 3.0, 0.0, 1.0, 0.3,
        ];
        let num_actions = 3;
        let num_hands = 5;

        let bytes = quantize_probs(&probs);
        assert_eq!(bytes.len(), probs.len() * 2);
        let restored = dequantize_probs(&bytes, num_actions, num_hands).unwrap();
        assert_eq!(restored.len(), probs.len());

        for (orig, got) in probs.iter().zip(restored.iter()) {
            assert!(
                (orig - got).abs() < 1e-4,
                "orig={orig} got={got} diff={}",
                (orig - got).abs()
            );
        }

        for h in 0..num_hands {
            let col_sum: f32 = (0..num_actions).map(|a| restored[a * num_hands + h]).sum();
            assert!(
                (col_sum - 1.0).abs() < 1e-6,
                "hand {h} column sum {col_sum} not within 1e-6 of 1.0"
            );
        }
    }

    #[test]
    fn rejects_bad_magic() {
        let path = temp_path("badmagic.sol");
        write_sol(&path, &sample_payload()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[0] = b'X';
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(read_sol(&path), Err(SolError::BadMagic)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_bad_version() {
        let path = temp_path("badversion.sol");
        write_sol(&path, &sample_payload()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        match read_sol(&path) {
            Err(SolError::BadVersion { found, expected }) => {
                assert_eq!(found, 99);
                assert_eq!(expected, FORMAT_VERSION);
            }
            other => panic!("expected BadVersion, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_truncated_header() {
        let path = temp_path("truncated-header.sol");
        write_sol(&path, &sample_payload()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..HEADER_LEN - 5]).unwrap();
        match read_sol(&path) {
            Err(SolError::Truncated { expected, actual }) => {
                assert_eq!(expected, HEADER_LEN);
                assert_eq!(actual, HEADER_LEN - 5);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_truncated_payload() {
        let path = temp_path("truncated-payload.sol");
        write_sol(&path, &sample_payload()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // Keep the full header but chop the zstd payload short.
        let cut = bytes.len() - 3;
        std::fs::write(&path, &bytes[..cut]).unwrap();
        assert!(read_sol(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_hash_mismatch() {
        let path = temp_path("hash-mismatch.sol");
        write_sol(&path, &sample_payload()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        // Flip one byte inside the header's config-hash field (bytes
        // 10..42) so it no longer matches blake3(config_toml).
        bytes[10] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();
        match read_sol(&path) {
            Err(SolError::HashMismatch { .. }) => {}
            other => panic!("expected HashMismatch, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn dequantize_rejects_wrong_length() {
        let bytes = quantize_probs(&[0.5, 0.5]);
        // Claim 3 hands worth of data when only 2 hands' worth is present.
        match dequantize_probs(&bytes, 1, 3) {
            Err(SolError::Truncated { expected, actual }) => {
                assert_eq!(expected, 3 * 2);
                assert_eq!(actual, bytes.len());
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }
}
