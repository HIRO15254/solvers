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
//! Version 3 stores checked metadata and independently compressed, indexed
//! groups of strategy/value nodes. See `sol_indexed` for the binary layout.

use serde::{Deserialize, Serialize};

pub use crate::sol_indexed::{read_sol, write_sol};

/// Postcard encodes a byte slice and a `Vec<u8>` identically: a varint
/// length followed by the bytes. Use its bulk byte path instead of visiting
/// every byte as a separate sequence element. JSON keeps its numeric array.
mod wire_bytes {
    use serde::de::{SeqAccess, Visitor, value::SeqAccessDeserializer};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return bytes.serialize(serializer);
        }
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        // Some human-readable byte decoders also accept strings. Retain the
        // old sequence-only input contract instead of broadening it.
        if deserializer.is_human_readable() {
            return Vec::<u8>::deserialize(deserializer);
        }
        struct BytesVisitor;

        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a byte array")
            }

            fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
                Ok(bytes.to_vec())
            }

            fn visit_byte_buf<E: serde::de::Error>(self, bytes: Vec<u8>) -> Result<Self::Value, E> {
                Ok(bytes)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, sequence: A) -> Result<Self::Value, A::Error> {
                // Preserve Vec<u8>'s checks if a binary format presents its
                // byte buffer as a sequence rather than a contiguous slice.
                Vec::<u8>::deserialize(SeqAccessDeserializer::new(sequence))
            }
        }

        deserializer.deserialize_byte_buf(BytesVisitor)
    }
}

/// Fixed header layout, all multi-byte fields little-endian: magic (8
/// bytes) + format version (u16) + config hash (32 bytes) + iteration
/// (u64) = 50 bytes. Same shape as `checkpoint::HEADER_LEN` so both formats
/// can share tooling that only needs to peek progress/identity without
/// paying for a zstd decompression.
pub const HEADER_LEN: usize = 8 + 2 + 32 + 8;

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
    #[error("invalid .sol layout: {0}")]
    InvalidLayout(String),
    #[error(".sol I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error(".sol payload codec error: {0}")]
    Codec(#[from] postcard::Error),
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
    #[serde(with = "wire_bytes")]
    pub probs: Vec<u8>,
}

/// One action node's per-hand values on the original subgame-start basis.
/// The utility-valued baseline offset is identical at every node; wagers
/// already made in the subgame remain costs. Values use chips for chip EV
/// and prize units for ICM, including tournament ICM.
///
/// Stored rather than recomputed because a `NoRivers` artifact cannot
/// recompute them: its river strategies are gone, and the viewer's lazy
/// re-solve produces different play and therefore different values. Even
/// for `Full` artifacts, recomputing means a whole value pass per query.
///
/// A hand that cannot be held at the node is stored as zero. A
/// counterfactual value exists for every hand whether or not it can arrive
/// there, so keeping them would make these blocks dense where the strategy
/// blocks are sparse — most of an artifact's size — for numbers no reader
/// wants.
/// Quantized to `i16` against a per-block scale, the way strategies are
/// quantized to `u16` against a fixed denominator: a node's values are
/// bounded by the chips it can still win or lose, so one scale per block
/// carries them at about one part in 32,767 — far finer than any solve's
/// own convergence error, at half the bytes of `f32` and with much better
/// compression, since the low mantissa bits that survive `f32` are noise
/// no reader can use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueBlock {
    /// Same `sref` space as [`StrategyBlock`].
    pub sref: u32,
    /// Chips (or prize units) one quantization step represents. Zero when
    /// every value in the block is zero.
    pub scale: f32,
    /// OOP's per-hand values then IP's, little-endian `i16` multiples of
    /// `scale`, stored as raw bytes for the same reason
    /// [`StrategyBlock::probs`] is.
    #[serde(with = "wire_bytes")]
    pub values: Vec<u8>,
}

/// The full decoded `.sol` contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolPayload {
    /// Self-contained effective config TOML text (not just its hash)
    /// so the viewer can rebuild the exact same tree deterministically.
    /// The header's config hash MUST equal `blake3(config_toml.as_bytes())`
    /// — `write_sol` derives it from this field and `read_sol` verifies it,
    /// so a `.sol` can never silently drift from the config it describes.
    pub config_toml: String,
    pub meta: SolMeta,
    pub mode: StreetsStored,
    /// Number of nodes in the complete compiled public tree.
    pub node_count: u64,
    /// One entry per stored action node, ascending by `sref`.
    pub blocks: Vec<StrategyBlock>,
    /// Per-hand values for the same nodes as `blocks`, ascending by
    /// `sref`. The two lists always cover the same node set: a reader that
    /// found a strategy for a node can always find its values too.
    pub values: Vec<ValueBlock>,
}

/// Quantizes an already-normalized probability slice (values in `[0, 1]`,
/// caller-guaranteed to be a valid strategy) to 16-bit fixed point:
/// `q = round(p * 65535)`. Deliberately does not renormalize on the way in
/// — `dequantize_probs` is responsible for restoring an exact per-hand sum
/// of 1.0 after the rounding error introduced here.
/// Quantizes a value block to `i16` against a scale chosen from its own
/// largest magnitude. Returns `(scale, bytes)`; `scale` is `0.0` when every
/// value is zero, which `dequantize_values` reads back as all zeros.
pub fn quantize_values(values: &[f32]) -> (f32, Vec<u8>) {
    let peak = values.iter().fold(0.0f32, |peak, v| peak.max(v.abs()));
    let scale = if peak > 0.0 { peak / 32_767.0 } else { 0.0 };
    let mut bytes = Vec::with_capacity(values.len() * 2);
    for &v in values {
        let q = if scale > 0.0 {
            (v / scale).round().clamp(-32_767.0, 32_767.0) as i16
        } else {
            0
        };
        bytes.extend_from_slice(&q.to_le_bytes());
    }
    (scale, bytes)
}

/// Dequantizes a [`quantize_values`] blob. `bytes.len()` must be twice
/// `len`; a mismatch is `SolError::Truncated` rather than a panic, for the
/// same reason [`dequantize_probs`] reports it that way.
pub fn dequantize_values(bytes: &[u8], scale: f32, len: usize) -> Result<Vec<f32>, SolError> {
    let expected = len * 2;
    if bytes.len() != expected {
        return Err(SolError::Truncated {
            expected,
            actual: bytes.len(),
        });
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 * scale)
        .collect())
}

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
            node_count: 100,
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
            // Same srefs as `blocks`, in the same order: the two lists
            // always describe the same nodes.
            values: vec![
                value_block(3, &[2.0, -1.0, 0.5, 0.0, -2.0, 1.0, -0.5, 0.0]),
                value_block(9, &[7.5, -7.5, -7.5, 7.5]),
            ],
        }
    }

    fn value_block(sref: u32, values: &[f32]) -> ValueBlock {
        let (scale, bytes) = quantize_values(values);
        ValueBlock {
            sref,
            scale,
            values: bytes,
        }
    }

    /// Values survive the `i16` round trip well inside any solve's own
    /// convergence error, and an all-zero block stays exactly zero rather
    /// than dividing by a zero scale.
    #[test]
    fn value_quantization_round_trips_within_tolerance() {
        let values = [0.0, 1.0, -1.0, 250.0, -250.0, 0.125, -0.125];
        let (scale, bytes) = quantize_values(&values);
        let back = dequantize_values(&bytes, scale, values.len()).unwrap();
        let step = 250.0 / 32_767.0;
        for (original, restored) in values.iter().zip(&back) {
            assert!(
                (original - restored).abs() <= step,
                "{original} restored as {restored}"
            );
        }

        let (scale, bytes) = quantize_values(&[0.0, 0.0, 0.0]);
        assert_eq!(scale, 0.0);
        assert_eq!(dequantize_values(&bytes, scale, 3).unwrap(), vec![0.0; 3]);

        assert!(
            dequantize_values(&bytes, 1.0, 4).is_err(),
            "length mismatch"
        );
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
    fn indexed_reads_are_independent_and_detect_damaged_node_frames() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("indexed.sol");
        let payload = sample_payload();
        write_sol(&path, &payload).unwrap();
        let mut reader = crate::SolReader::open(&path).unwrap();
        assert_eq!(reader.metadata().node_count, 100);
        assert_eq!(reader.stored_srefs().collect::<Vec<_>>(), [3, 9]);
        assert_eq!(
            reader.read_node(3).unwrap().unwrap(),
            (payload.blocks[0].clone(), payload.values[0].clone())
        );
        assert!(reader.read_node(4).unwrap().is_none());
        drop(reader);
        let mut bytes = std::fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        std::fs::write(&path, bytes).unwrap();
        let mut reader = crate::SolReader::open(&path).unwrap();
        assert!(
            reader.read_node(3).is_ok(),
            "another node is still readable"
        );
        assert!(reader.read_node(9).is_err());
        assert!(
            read_sol(&path).is_err(),
            "full validation must inspect every frame"
        );
    }

    #[test]
    fn indexed_layout_rejects_legacy_and_invalid_directory_extents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.sol");
        write_sol(&path, &sample_payload()).unwrap();
        let original = std::fs::read(&path).unwrap();
        for version in [1u16, 2] {
            let mut legacy = original.clone();
            legacy[8..10].copy_from_slice(&version.to_le_bytes());
            std::fs::write(&path, legacy).unwrap();
            assert!(matches!(
                read_sol(&path),
                Err(SolError::BadVersion { found, expected: 3 }) if found == version
            ));
        }
        for (offset, value) in [(50, u64::MAX), (58, u64::MAX), (98, u64::MAX)] {
            let mut bytes = original.clone();
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            std::fs::write(&path, bytes).unwrap();
            assert!(crate::SolReader::open(&path).is_err());
        }
        let mut bytes = original.clone();
        bytes.extend_from_slice(b"trailing");
        std::fs::write(&path, bytes).unwrap();
        assert!(crate::SolReader::open(&path).is_err());
        let mut bytes = original;
        bytes[42] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(crate::SolReader::open(&path).is_err());
    }

    #[test]
    fn indexed_writer_rejects_mismatched_nodes_and_preserves_old_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("atomic.sol");
        let mut payload = sample_payload();
        write_sol(&path, &payload).unwrap();
        let original = std::fs::read(&path).unwrap();
        payload.values[1].sref = 10;
        assert!(write_sol(&path, &payload).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        payload.blocks.clear();
        payload.values.clear();
        write_sol(&path, &payload).unwrap();
        assert_eq!(read_sol(&path).unwrap(), payload);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
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
                assert_eq!(expected, crate::sol_indexed::SOL_FORMAT_VERSION);
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
