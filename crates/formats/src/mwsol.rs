//! Indexed, read-only strategy artifact for sampled multiway solves.
//!
//! Unlike a `.mwckpt`, this format deliberately omits regrets and RNG state.
//! Blocks are sorted by their complete infoset key, allowing a viewer or the
//! bridge to binary-search one strategy without rebuilding a public tree.

use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Estimate, MULTIWAY_SCHEMA_VERSION, config_hash, config_hash_hex};

pub const MWSOL_HEADER_LEN: usize = 8 + 2 + 32 + 32 + 8 + 8 + 32;
const MAGIC: &[u8; 8] = b"SLVRMWSL";
const FORMAT_VERSION: u16 = 1;
const MAX_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MultiwayStrategyKey {
    pub history: [u8; 16],
    pub actor: u8,
    pub street: u8,
    pub active_opponents: u8,
    /// Full private recall. Entries after the current street are zero.
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiwaySeatResult {
    pub seat: u8,
    pub profile_ev: Option<Estimate>,
    pub average_positive_regret: f64,
    pub strategy_drift_l1: f64,
    pub deviation_gain_lower_bound: Option<Estimate>,
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

impl MultiwaySolution {
    pub fn strategy(&self, key: MultiwayStrategyKey) -> Option<&MultiwayStrategyBlock> {
        self.strategies
            .binary_search_by_key(&key, |block| block.key)
            .ok()
            .map(|index| &self.strategies[index])
    }

    pub fn resolve_history(&self, mut key: [u8; 16]) -> Option<Vec<MultiwayHistoryAction>> {
        let mut path = Vec::new();
        while key != [0; 16] {
            let index = self
                .histories
                .binary_search_by_key(&key, |node| node.key)
                .ok()?;
            let node = &self.histories[index];
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

    fn validate(&self) -> Result<(), MwSolError> {
        if self.schema_version != MULTIWAY_SCHEMA_VERSION {
            return Err(MwSolError::SchemaVersion {
                found: self.schema_version,
                expected: MULTIWAY_SCHEMA_VERSION,
            });
        }
        if !self.approximate_profile {
            return Err(MwSolError::MissingApproximationMarker);
        }
        if self
            .histories
            .windows(2)
            .any(|pair| pair[0].key >= pair[1].key)
        {
            return Err(MwSolError::InvalidHistoryTrie);
        }
        for node in &self.histories {
            if node.key == [0; 16]
                || node.key != history_child(node.parent, node.actor, node.action_index)
                || node.action.is_empty()
                || (node.parent != [0; 16]
                    && self
                        .histories
                        .binary_search_by_key(&node.parent, |entry| entry.key)
                        .is_err())
            {
                return Err(MwSolError::InvalidHistoryTrie);
            }
            let mut key = node.key;
            for depth in 0..=self.histories.len() {
                if key == [0; 16] {
                    break;
                }
                if depth == self.histories.len() {
                    return Err(MwSolError::InvalidHistoryTrie);
                }
                let index = self
                    .histories
                    .binary_search_by_key(&key, |entry| entry.key)
                    .map_err(|_| MwSolError::InvalidHistoryTrie)?;
                key = self.histories[index].parent;
            }
        }
        if self
            .strategies
            .windows(2)
            .any(|pair| pair[0].key >= pair[1].key)
        {
            return Err(MwSolError::UnsortedIndex);
        }
        for block in &self.strategies {
            if block.key.history != [0; 16]
                && self
                    .histories
                    .binary_search_by_key(&block.key.history, |node| node.key)
                    .is_err()
            {
                return Err(MwSolError::InvalidHistoryTrie);
            }
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
        }
        Ok(())
    }
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
pub struct MwSolHeader {
    pub config_hash: [u8; 32],
    pub abstraction_fingerprint: [u8; 32],
    pub sweeps: u64,
    pub payload_len: u64,
    pub checksum: [u8; 32],
}

pub fn write_mwsol(path: &Path, solution: &MultiwaySolution) -> Result<(), MwSolError> {
    solution.validate()?;
    let raw = postcard::to_allocvec(solution)?;
    let compressed = zstd::stream::encode_all(raw.as_slice(), 3)?;
    let header = MwSolHeader {
        config_hash: config_hash(solution.config_toml.as_bytes()),
        abstraction_fingerprint: solution.abstraction_fingerprint,
        sweeps: solution.sweeps,
        payload_len: compressed.len() as u64,
        checksum: *blake3::hash(&compressed).as_bytes(),
    };

    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&encode_header(header))?;
    temporary.write_all(&compressed)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| MwSolError::Io(error.error))?;
    Ok(())
}

pub fn peek_mwsol_header(path: &Path) -> Result<MwSolHeader, MwSolError> {
    let mut file = std::fs::File::open(path)?;
    let mut bytes = [0; MWSOL_HEADER_LEN];
    file.read_exact(&mut bytes).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            MwSolError::Truncated
        } else {
            MwSolError::Io(error)
        }
    })?;
    decode_header(&bytes)
}

pub fn read_mwsol(path: &Path) -> Result<MultiwaySolution, MwSolError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < MWSOL_HEADER_LEN {
        return Err(MwSolError::Truncated);
    }
    let encoded: [u8; MWSOL_HEADER_LEN] = bytes[..MWSOL_HEADER_LEN]
        .try_into()
        .expect("header-sized slice");
    let header = decode_header(&encoded)?;
    if bytes.len() as u64 != MWSOL_HEADER_LEN as u64 + header.payload_len {
        return Err(MwSolError::LengthMismatch);
    }
    let compressed = &bytes[MWSOL_HEADER_LEN..];
    if *blake3::hash(compressed).as_bytes() != header.checksum {
        return Err(MwSolError::ChecksumMismatch);
    }
    let decoder = zstd::stream::read::Decoder::new(compressed)?;
    let mut raw = Vec::new();
    decoder
        .take(MAX_UNCOMPRESSED_BYTES + 1)
        .read_to_end(&mut raw)?;
    if raw.len() as u64 > MAX_UNCOMPRESSED_BYTES {
        return Err(MwSolError::PayloadTooLarge);
    }
    let solution: MultiwaySolution = postcard::from_bytes(&raw)?;
    solution.validate()?;
    let computed = config_hash(solution.config_toml.as_bytes());
    if computed != header.config_hash {
        return Err(MwSolError::ConfigHashMismatch {
            header: config_hash_hex(&header.config_hash),
            computed: config_hash_hex(&computed),
        });
    }
    if solution.abstraction_fingerprint != header.abstraction_fingerprint
        || solution.sweeps != header.sweeps
    {
        return Err(MwSolError::HeaderMismatch);
    }
    Ok(solution)
}

fn encode_header(header: MwSolHeader) -> [u8; MWSOL_HEADER_LEN] {
    let mut bytes = [0; MWSOL_HEADER_LEN];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes[10..42].copy_from_slice(&header.config_hash);
    bytes[42..74].copy_from_slice(&header.abstraction_fingerprint);
    bytes[74..82].copy_from_slice(&header.sweeps.to_le_bytes());
    bytes[82..90].copy_from_slice(&header.payload_len.to_le_bytes());
    bytes[90..122].copy_from_slice(&header.checksum);
    bytes
}

fn decode_header(bytes: &[u8; MWSOL_HEADER_LEN]) -> Result<MwSolHeader, MwSolError> {
    if &bytes[..8] != MAGIC {
        return Err(MwSolError::BadMagic);
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().expect("two bytes"));
    if version != FORMAT_VERSION {
        return Err(MwSolError::BadVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }
    Ok(MwSolHeader {
        config_hash: bytes[10..42].try_into().expect("32 bytes"),
        abstraction_fingerprint: bytes[42..74].try_into().expect("32 bytes"),
        sweeps: u64::from_le_bytes(bytes[74..82].try_into().expect("eight bytes")),
        payload_len: u64::from_le_bytes(bytes[82..90].try_into().expect("eight bytes")),
        checksum: bytes[90..122].try_into().expect("32 bytes"),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum MwSolError {
    #[error("not a multiway solution (bad magic)")]
    BadMagic,
    #[error("unsupported .mwsol version {found} (expected {expected})")]
    BadVersion { found: u16, expected: u16 },
    #[error("truncated .mwsol file")]
    Truncated,
    #[error(".mwsol length does not match its header")]
    LengthMismatch,
    #[error(".mwsol payload checksum mismatch")]
    ChecksumMismatch,
    #[error(".mwsol payload exceeds safety limit")]
    PayloadTooLarge,
    #[error(".mwsol config hash {header} does not match embedded config {computed}")]
    ConfigHashMismatch { header: String, computed: String },
    #[error(".mwsol header does not match payload")]
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
    #[error(".mwsol I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error(".mwsol codec error: {0}")]
    Codec(#[from] postcard::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solution() -> MultiwaySolution {
        MultiwaySolution {
            schema_version: MULTIWAY_SCHEMA_VERSION,
            config_toml: "[game]\nkind = \"preflop-multiway\"\n".into(),
            abstraction_fingerprint: [7; 32],
            sweeps: 99,
            approximate_profile: true,
            seats: Vec::new(),
            histories: Vec::new(),
            strategies: vec![MultiwayStrategyBlock {
                key: MultiwayStrategyKey {
                    history: [0; 16],
                    actor: 0,
                    street: 0,
                    active_opponents: 2,
                    bucket_path: [12, 0, 0, 0],
                },
                actions: vec!["fold".into(), "call".into()],
                probabilities: vec![0.25, 0.75],
            }],
        }
    }

    #[test]
    fn indexed_artifact_round_trips() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.mwsol");
        let expected = solution();
        write_mwsol(&path, &expected).unwrap();
        let header = peek_mwsol_header(&path).unwrap();
        assert_eq!(header.sweeps, 99);
        let decoded = read_mwsol(&path).unwrap();
        assert_eq!(decoded, expected);
        assert!(decoded.strategy(expected.strategies[0].key).is_some());
    }

    #[test]
    fn rejects_unsorted_or_unmarked_profiles() {
        let mut value = solution();
        value.approximate_profile = false;
        assert!(matches!(
            value.validate(),
            Err(MwSolError::MissingApproximationMarker)
        ));
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
        value.strategies[0].key.history = key;
        assert_eq!(
            value.resolve_history(key).unwrap(),
            vec![MultiwayHistoryAction {
                actor: 2,
                action_index: 1,
                action: "raise-to:2500".into(),
            }]
        );
        value.validate().unwrap();
        value.histories[0].action_index = 0;
        assert!(matches!(
            value.validate(),
            Err(MwSolError::InvalidHistoryTrie)
        ));
    }
}
