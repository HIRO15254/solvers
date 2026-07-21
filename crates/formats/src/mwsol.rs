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
pub const MWSOL_FORMAT_VERSION: u16 = 4;
/// Oldest on-disk version this reader still accepts during the real-data
/// migration gate. Version 2 frames are plain postcard blocks, version 3
/// introduces the framed F32/I16 representation, and version 4 adds the
/// unsigned U16 representation used by the v1 contract.
pub const MWSOL_MIN_FORMAT_VERSION: u16 = 2;
pub const MWSOL_MAX_PAGE_LIMIT: usize = 4096;
/// Denominator used by unsigned v4 probability quantization.
const MWSOL_U16_DENOMINATOR: u16 = u16::MAX;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiwayPublicAction {
    Fold,
    Check,
    Call {
        amount_millibb: u64,
        all_in: bool,
    },
    BetTo {
        amount_millibb: u64,
        all_in: bool,
        full_raise: bool,
    },
    RaiseTo {
        amount_millibb: u64,
        all_in: bool,
        full_raise: bool,
    },
}

impl MultiwayPublicAction {
    pub fn label(&self) -> String {
        match self {
            Self::Fold => "fold".into(),
            Self::Check => "check".into(),
            Self::Call {
                amount_millibb,
                all_in,
            } => format!(
                "call:{amount_millibb}{}",
                if *all_in { ":all-in" } else { "" }
            ),
            Self::BetTo {
                amount_millibb,
                all_in,
                ..
            } => format!(
                "bet-to:{amount_millibb}{}",
                if *all_in { ":all-in" } else { "" }
            ),
            Self::RaiseTo {
                amount_millibb,
                all_in,
                ..
            } => format!(
                "raise-to:{amount_millibb}{}",
                if *all_in { ":all-in" } else { "" }
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiwayPublicState {
    pub history: [u8; 16],
    pub street: u8,
    pub actor: Option<u8>,
    pub pot_millibb: u64,
    pub remaining_stacks_millibb: Vec<u64>,
    pub legal_actions: Vec<MultiwayPublicAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiwayStrategyWeight {
    pub key: MultiwayStrategyKey,
    pub weight: f64,
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
    /// Quantize probabilities to unsigned 16-bit fixed point.
    U16,
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
    U16 {
        key: MultiwayStrategyKey,
        actions: Vec<String>,
        quantized: Vec<u16>,
    },
}

impl FrameBlock {
    #[cfg(test)]
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
            MwsolStorage::U16 => FrameBlock::U16 {
                key: block.key,
                actions: block.actions.clone(),
                quantized: quantize_u16(&block.probabilities),
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

            FrameBlock::U16 {
                key,
                actions,
                quantized,
            } => {
                let probabilities = quantized
                    .iter()
                    .map(|&q| f32::from(q) / f32::from(MWSOL_U16_DENOMINATOR))
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum FrameBlockV4 {
    F32 {
        key: MultiwayStrategyKey,
        probabilities: Vec<f32>,
    },
    U16 {
        key: MultiwayStrategyKey,
        quantized: Vec<u16>,
    },
}

impl FrameBlockV4 {
    fn from_block(
        block: &MultiwayStrategyBlock,
        storage: MwsolStorage,
    ) -> Result<Self, MwSolError> {
        match storage {
            MwsolStorage::F32 => Ok(Self::F32 {
                key: block.key,
                probabilities: block.probabilities.clone(),
            }),
            MwsolStorage::U16 => Ok(Self::U16 {
                key: block.key,
                quantized: quantize_u16(&block.probabilities),
            }),
            MwsolStorage::I16 => Err(MwSolError::UnsupportedStrategyEncoding),
        }
    }

    fn into_block(self, actions: Vec<String>) -> MultiwayStrategyBlock {
        match self {
            Self::F32 { key, probabilities } => MultiwayStrategyBlock {
                key,
                actions,
                probabilities,
            },
            Self::U16 { key, quantized } => MultiwayStrategyBlock {
                key,
                actions,
                probabilities: quantized
                    .iter()
                    .map(|value| f32::from(*value) / f32::from(MWSOL_U16_DENOMINATOR))
                    .collect(),
            },
        }
    }
}

/// Largest-remainder quantization shared by the signed legacy codec and the
/// unsigned v4 codec. Keeping the allocation and tie-breaking logic in one
/// place prevents the two wire encodings from drifting apart.
fn quantize_largest_remainder(probabilities: &[f32], denominator: u64) -> Vec<u64> {
    let sum: f64 = probabilities.iter().map(|&p| f64::from(p)).sum();
    if probabilities.is_empty() || sum <= 0.0 || !sum.is_finite() {
        return vec![0; probabilities.len()];
    }

    let denominator_f64 = denominator as f64;
    let mut quantized = Vec::with_capacity(probabilities.len());
    let mut fractions = Vec::with_capacity(probabilities.len());
    let mut floor_sum = 0u64;
    for (index, &probability) in probabilities.iter().enumerate() {
        let scaled = f64::from(probability) / sum * denominator_f64;
        let floor = scaled.floor() as u64;
        floor_sum = floor_sum.saturating_add(floor);
        quantized.push(floor);
        fractions.push((index, scaled - floor as f64));
    }

    let remainder = denominator
        .saturating_sub(floor_sum)
        .min(quantized.len() as u64);
    fractions.sort_by(|&(index_a, fraction_a), &(index_b, fraction_b)| {
        fraction_b
            .partial_cmp(&fraction_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| index_a.cmp(&index_b))
    });
    for &(index, _) in fractions.iter().take(remainder as usize) {
        quantized[index] += 1;
    }
    quantized
}

/// Quantizes probabilities to `i16` fixed point with denominator
/// `i16::MAX` (32767) using the largest-remainder method, so
/// `sum(quantized) == 32767` exactly and every entry is `>= 0`. Input need
/// not sum exactly to `1.0`; it is normalized by its own sum first, so this
/// tolerates the same `1e-4` slack `validate_strategy_block` allows.
#[cfg(test)]
fn quantize_i16(probabilities: &[f32]) -> Vec<i16> {
    quantize_largest_remainder(probabilities, MWSOL_I16_DENOMINATOR as u64)
        .into_iter()
        .map(|value| value.min(MWSOL_I16_DENOMINATOR as u64) as i16)
        .collect()
}

/// Quantizes a distribution to unsigned fixed point with denominator 65,535.
/// Largest-remainder assignment and stable index tie-breaks preserve an exact
/// encoded sum while keeping the result deterministic.
fn quantize_u16(probabilities: &[f32]) -> Vec<u16> {
    quantize_largest_remainder(probabilities, u64::from(MWSOL_U16_DENOMINATOR))
        .into_iter()
        .map(|value| value.min(u64::from(MWSOL_U16_DENOMINATOR)) as u16)
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
    pub config_fingerprint: [u8; 32],
    pub game_fingerprint: [u8; 32],
    pub algorithm_fingerprint: [u8; 32],
    pub abstraction_fingerprint: [u8; 32],
    pub configuration_fingerprint: [u8; 32],
    pub stop_status: String,
    pub chip_unit_bb: f64,
    pub sweeps: u64,
    pub approximate_profile: bool,
    pub seats: Vec<MultiwaySeatResult>,
    /// Strictly key-sorted compact trie; shared by all private buckets.
    pub histories: Vec<MultiwayHistoryNode>,
    pub public_states: Vec<MultiwayPublicState>,
    pub strategy_weights: Vec<MultiwayStrategyWeight>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiwaySolution {
    pub schema_version: u16,
    pub config_toml: String,
    pub config_fingerprint: [u8; 32],
    pub game_fingerprint: [u8; 32],
    pub algorithm_fingerprint: [u8; 32],
    pub abstraction_fingerprint: [u8; 32],
    pub configuration_fingerprint: [u8; 32],
    pub stop_status: String,
    pub chip_unit_bb: f64,
    pub sweeps: u64,
    pub approximate_profile: bool,
    pub seats: Vec<MultiwaySeatResult>,
    /// Strictly key-sorted compact trie; shared by all private buckets.
    pub histories: Vec<MultiwayHistoryNode>,
    pub public_states: Vec<MultiwayPublicState>,
    pub strategy_weights: Vec<MultiwayStrategyWeight>,
    /// Must be strictly sorted by `key`.
    pub strategies: Vec<MultiwayStrategyBlock>,
}

#[derive(Deserialize, Serialize)]
struct LegacyMultiwaySolutionMetadata {
    schema_version: u16,
    config_toml: String,
    abstraction_fingerprint: [u8; 32],
    sweeps: u64,
    approximate_profile: bool,
    seats: Vec<MultiwaySeatResult>,
    histories: Vec<MultiwayHistoryNode>,
}

impl From<LegacyMultiwaySolutionMetadata> for MultiwaySolutionMetadata {
    fn from(value: LegacyMultiwaySolutionMetadata) -> Self {
        let config_fingerprint = config_hash(value.config_toml.as_bytes());
        Self {
            schema_version: value.schema_version,
            config_toml: value.config_toml,
            config_fingerprint,
            game_fingerprint: [0; 32],
            algorithm_fingerprint: [0; 32],
            abstraction_fingerprint: value.abstraction_fingerprint,
            configuration_fingerprint: [0; 32],
            stop_status: "legacy".into(),
            chip_unit_bb: 0.001,
            sweeps: value.sweeps,
            approximate_profile: value.approximate_profile,
            seats: value.seats,
            histories: value.histories,
            public_states: Vec::new(),
            strategy_weights: Vec::new(),
        }
    }
}

impl MultiwaySolutionMetadata {
    fn from_solution(solution: &MultiwaySolution) -> Self {
        Self {
            schema_version: solution.schema_version,
            config_toml: solution.config_toml.clone(),
            config_fingerprint: solution.config_fingerprint,
            game_fingerprint: solution.game_fingerprint,
            algorithm_fingerprint: solution.algorithm_fingerprint,
            abstraction_fingerprint: solution.abstraction_fingerprint,
            configuration_fingerprint: solution.configuration_fingerprint,
            stop_status: solution.stop_status.clone(),
            chip_unit_bb: solution.chip_unit_bb,
            sweeps: solution.sweeps,
            approximate_profile: solution.approximate_profile,
            seats: solution.seats.clone(),
            histories: solution.histories.clone(),
            public_states: solution.public_states.clone(),
            strategy_weights: solution.strategy_weights.clone(),
        }
    }

    pub fn resolve_history(&self, key: [u8; 16]) -> Option<Vec<MultiwayHistoryAction>> {
        resolve_history(&self.histories, key)
    }

    fn actions_for_strategy(&self, key: MultiwayStrategyKey) -> Result<Vec<String>, MwSolError> {
        Ok(public_state_for_strategy(&self.public_states, key)?
            .legal_actions
            .iter()
            .map(MultiwayPublicAction::label)
            .collect())
    }

    fn validate(&self) -> Result<(), MwSolError> {
        validate_metadata_fields(
            self.schema_version,
            self.approximate_profile,
            &self.histories,
        )?;
        validate_v4_identity(
            V4Identity {
                config_toml: &self.config_toml,
                config_fingerprint: self.config_fingerprint,
                game_fingerprint: self.game_fingerprint,
                algorithm_fingerprint: self.algorithm_fingerprint,
                configuration_fingerprint: self.configuration_fingerprint,
                stop_status: &self.stop_status,
                chip_unit_bb: self.chip_unit_bb,
            },
            !self.public_states.is_empty() || !self.strategy_weights.is_empty(),
        )?;
        validate_v4_metadata(&self.histories, &self.public_states, &self.strategy_weights)
    }
}

fn public_state_for_strategy(
    public_states: &[MultiwayPublicState],
    key: MultiwayStrategyKey,
) -> Result<&MultiwayPublicState, MwSolError> {
    public_states
        .binary_search_by_key(&key.history, |state| state.history)
        .ok()
        .map(|index| &public_states[index])
        .filter(|state| state.actor == Some(key.actor))
        .ok_or(MwSolError::InvalidPublicStates)
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
        validate_v4_identity(
            V4Identity {
                config_toml: &self.config_toml,
                config_fingerprint: self.config_fingerprint,
                game_fingerprint: self.game_fingerprint,
                algorithm_fingerprint: self.algorithm_fingerprint,
                configuration_fingerprint: self.configuration_fingerprint,
                stop_status: &self.stop_status,
                chip_unit_bb: self.chip_unit_bb,
            },
            !self.public_states.is_empty() || !self.strategy_weights.is_empty(),
        )?;
        validate_v4_metadata(&self.histories, &self.public_states, &self.strategy_weights)?;
        if self
            .strategies
            .windows(2)
            .any(|pair| pair[0].key >= pair[1].key)
        {
            return Err(MwSolError::UnsortedIndex);
        }
        if self.strategy_weights.len() != self.strategies.len()
            || self
                .strategy_weights
                .iter()
                .zip(&self.strategies)
                .any(|(weight, block)| weight.key != block.key)
        {
            return Err(MwSolError::InvalidStrategyWeights);
        }
        let mut cached_public_node = None;
        let mut expected_actions = Vec::new();
        for block in &self.strategies {
            validate_strategy_block(block, &self.histories)?;
            let public_node = (block.key.history, block.key.actor);
            if cached_public_node != Some(public_node) {
                expected_actions.clear();
                expected_actions.extend(
                    public_state_for_strategy(&self.public_states, block.key)?
                        .legal_actions
                        .iter()
                        .map(MultiwayPublicAction::label),
                );
                cached_public_node = Some(public_node);
            }
            if block.actions != expected_actions {
                return Err(MwSolError::InvalidStrategy(block.key));
            }
        }
        Ok(())
    }
}
struct V4Identity<'a> {
    config_toml: &'a str,
    config_fingerprint: [u8; 32],
    game_fingerprint: [u8; 32],
    algorithm_fingerprint: [u8; 32],
    configuration_fingerprint: [u8; 32],
    stop_status: &'a str,
    chip_unit_bb: f64,
}

fn validate_v4_identity(identity: V4Identity<'_>, has_v4_content: bool) -> Result<(), MwSolError> {
    if !has_v4_content {
        return Ok(());
    }
    if identity.config_fingerprint != config_hash(identity.config_toml.as_bytes())
        || identity.game_fingerprint == [0; 32]
        || identity.algorithm_fingerprint == [0; 32]
        || identity.configuration_fingerprint == [0; 32]
        || identity.chip_unit_bb.to_bits() != 0.001_f64.to_bits()
        || !matches!(
            identity.stop_status,
            "completed" | "sweep-limit" | "target-reached" | "time-limit" | "converged"
        )
    {
        return Err(MwSolError::InvalidSolutionMetadata);
    }
    Ok(())
}

fn validate_v4_metadata(
    histories: &[MultiwayHistoryNode],
    public_states: &[MultiwayPublicState],
    strategy_weights: &[MultiwayStrategyWeight],
) -> Result<(), MwSolError> {
    if public_states.is_empty() && strategy_weights.is_empty() {
        return Ok(());
    }
    if public_states.is_empty()
        || public_states
            .windows(2)
            .any(|pair| pair[0].history >= pair[1].history)
        || public_states.iter().any(|state| {
            (state.history != [0; 16]
                && histories
                    .binary_search_by_key(&state.history, |entry| entry.key)
                    .is_err())
                || state.remaining_stacks_millibb.is_empty()
                || (state.actor.is_some() && state.legal_actions.is_empty())
        })
    {
        return Err(MwSolError::InvalidPublicStates);
    }
    if strategy_weights.is_empty()
        || strategy_weights
            .windows(2)
            .any(|pair| pair[0].key >= pair[1].key)
        || strategy_weights
            .iter()
            .any(|entry| !entry.weight.is_finite() || entry.weight <= 0.0)
    {
        return Err(MwSolError::InvalidStrategyWeights);
    }
    Ok(())
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

    // Resolve every parent once. The previous per-node walk back to the root
    // repeated the same ancestry scans and became quadratic on deep trees.
    let mut parents = Vec::with_capacity(histories.len());
    for node in histories {
        if node.key == [0; 16]
            || node.key != history_child(node.parent, node.actor, node.action_index)
            || node.action.is_empty()
        {
            return Err(MwSolError::InvalidHistoryTrie);
        }
        let parent = if node.parent == [0; 16] {
            None
        } else {
            Some(
                histories
                    .binary_search_by_key(&node.parent, |entry| entry.key)
                    .map_err(|_| MwSolError::InvalidHistoryTrie)?,
            )
        };
        parents.push(parent);
    }

    // Each node has at most one parent, so a three-color walk detects cycles
    // in linear time after the parent lookup pass.
    let mut state = vec![0u8; histories.len()];
    let mut path = Vec::new();
    for start in 0..histories.len() {
        if state[start] == 2 {
            continue;
        }
        path.clear();
        let mut current = Some(start);
        while let Some(index) = current {
            match state[index] {
                0 => {
                    state[index] = 1;
                    path.push(index);
                    current = parents[index];
                }
                1 => return Err(MwSolError::InvalidHistoryTrie),
                2 => break,
                _ => unreachable!(),
            }
        }
        for &index in &path {
            state[index] = 2;
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

/// Writes an `.mwsol` file (version 4), encoding each strategy frame as
/// unsigned U16 or F32 according to `storage`. Signed I16 is rejected; the
/// in-memory `MultiwaySolution` stays `f32` throughout.
pub fn write_mwsol_with(
    path: &Path,
    solution: &MultiwaySolution,
    storage: MwsolStorage,
) -> Result<(), MwSolError> {
    write_mwsol_frames(path, solution, MWSOL_FORMAT_VERSION, |block| {
        let frame = FrameBlockV4::from_block(block, storage)?;
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
    let metadata_raw = if format_version >= 4 {
        postcard::to_allocvec(&metadata)?
    } else {
        postcard::to_allocvec(&LegacyMultiwaySolutionMetadata {
            schema_version: solution.schema_version,
            config_toml: solution.config_toml.clone(),
            abstraction_fingerprint: solution.abstraction_fingerprint,
            sweeps: solution.sweeps,
            approximate_profile: solution.approximate_profile,
            seats: solution.seats.clone(),
            histories: solution.histories.clone(),
        })?
    };
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
        let metadata: MultiwaySolutionMetadata = if format_version >= 4 {
            postcard::from_bytes(&metadata_raw)?
        } else {
            postcard::from_bytes::<LegacyMultiwaySolutionMetadata>(&metadata_raw)?.into()
        };
        metadata.validate()?;
        if format_version >= 4
            && (metadata.strategy_weights.len() != strategy_count
                || (strategy_count > 0 && metadata.public_states.is_empty()))
        {
            return Err(MwSolError::InvalidStrategyWeights);
        }

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
            if format_version >= 4
                && metadata
                    .strategy_weights
                    .get(index)
                    .map(|weight| weight.key)
                    != Some(entry.key)
            {
                return Err(MwSolError::InvalidStrategyWeights);
            }
            if format_version >= 4 {
                public_state_for_strategy(&metadata.public_states, entry.key)?;
            }
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
            let block: MultiwayStrategyBlock = if self.format_version >= 4 {
                let frame: FrameBlockV4 = postcard::from_bytes(&raw)?;
                frame.into_block(self.metadata.actions_for_strategy(entry.key)?)
            } else if self.format_version >= 3 {
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
    let mut buffer = vec![0u8; 64 * 1024];
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
    #[error("multiway public-state table is invalid")]
    InvalidPublicStates,
    #[error("multiway solution identity/quality metadata is invalid")]
    InvalidSolutionMetadata,
    #[error("multiway strategy-weight table is invalid")]
    InvalidStrategyWeights,
    #[error("signed i16 strategy encoding is unsupported in .mwsol v4")]
    UnsupportedStrategyEncoding,
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
            config_fingerprint: metadata.config_fingerprint,
            game_fingerprint: metadata.game_fingerprint,
            algorithm_fingerprint: metadata.algorithm_fingerprint,
            abstraction_fingerprint: metadata.abstraction_fingerprint,
            configuration_fingerprint: metadata.configuration_fingerprint,
            stop_status: metadata.stop_status,
            chip_unit_bb: metadata.chip_unit_bb,
            sweeps: metadata.sweeps,
            approximate_profile: metadata.approximate_profile,
            seats: metadata.seats,
            histories: metadata.histories,
            public_states: metadata.public_states,
            strategy_weights: metadata.strategy_weights,
            strategies,
        };
        if reader.format_version() >= 4 {
            solution.validate()?;
        }
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
        let key = |index| MultiwayStrategyKey {
            history: [0; 16],
            actor: 0,
            street: 0,
            active_opponents: 2,
            bucket_path: [12 + index as u32, 0, 0, 0],
        };
        let config_toml = "[game]\nkind = \"preflop-multiway\"\n".to_string();
        MultiwaySolution {
            schema_version: MULTIWAY_SCHEMA_VERSION,
            config_fingerprint: config_hash(config_toml.as_bytes()),
            config_toml,
            game_fingerprint: [5; 32],
            algorithm_fingerprint: [6; 32],
            abstraction_fingerprint: [7; 32],
            configuration_fingerprint: [8; 32],
            stop_status: "completed".into(),
            chip_unit_bb: 0.001,
            sweeps: 99,
            approximate_profile: true,
            seats: Vec::new(),
            histories: Vec::new(),
            public_states: vec![MultiwayPublicState {
                history: [0; 16],
                street: 0,
                actor: Some(0),
                pot_millibb: 1_500,
                remaining_stacks_millibb: vec![99_500, 99_000],
                legal_actions: vec![
                    MultiwayPublicAction::Fold,
                    MultiwayPublicAction::Call {
                        amount_millibb: 500,
                        all_in: false,
                    },
                ],
            }],
            strategy_weights: (0..5)
                .map(|index| MultiwayStrategyWeight {
                    key: key(index),
                    weight: 10.0,
                })
                .collect(),
            strategies: (0..5)
                .map(|index| MultiwayStrategyBlock {
                    key: key(index),
                    actions: vec!["fold".into(), "call:500".into()],
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
        value.public_states[0].history = key;
        for strategy in &mut value.strategies {
            strategy.key.history = key;
        }
        for weight in &mut value.strategy_weights {
            weight.key.history = key;
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

    #[test]
    fn deep_history_trie_validation_is_stack_safe_and_linear_after_sorting() {
        let mut histories = Vec::with_capacity(4_096);
        let mut parent = [0; 16];
        for action_index in 0..4_096u32 {
            let actor = (action_index % 9) as u8;
            let key = history_child(parent, actor, action_index);
            histories.push(MultiwayHistoryNode {
                key,
                parent,
                actor,
                action_index,
                action: "check".into(),
            });
            parent = key;
        }
        histories.sort_unstable_by_key(|node| node.key);
        validate_histories(&histories).unwrap();
    }
    /// Synthesizes a version-2 `.mwsol` file: same layout as production
    /// writes, but frames are plain postcard-encoded `MultiwayStrategyBlock`s
    /// (no `FrameBlock` wrapper) and the header declares version 2, exactly
    /// replicating the pre-i16 on-disk format.
    fn write_legacy_v2(path: &Path, solution: &MultiwaySolution) -> Result<(), MwSolError> {
        write_mwsol_frames(path, solution, 2, |block| Ok(postcard::to_allocvec(block)?))
    }

    fn write_legacy_v3(path: &Path, solution: &MultiwaySolution) -> Result<(), MwSolError> {
        write_mwsol_frames(path, solution, 3, |block| {
            Ok(postcard::to_allocvec(&FrameBlock::from_block(
                block,
                MwsolStorage::F32,
            ))?)
        })
    }

    #[test]
    fn i16_storage_is_rejected_by_v4() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("signed.mwsol");
        assert!(matches!(
            write_mwsol_with(&path, &solution(), MwsolStorage::I16),
            Err(MwSolError::UnsupportedStrategyEncoding)
        ));
        assert!(!path.exists());
    }

    #[test]
    fn shared_quantizer_matches_the_previous_wire_algorithm() {
        let reference = |probabilities: &[f32], denominator: u64| {
            let sum: f64 = probabilities.iter().map(|&p| f64::from(p)).sum();
            let scaled: Vec<f64> = probabilities
                .iter()
                .map(|&p| f64::from(p) / sum * denominator as f64)
                .collect();
            let mut floors: Vec<u64> = scaled.iter().map(|&value| value.floor() as u64).collect();
            let floor_sum: u64 = floors.iter().sum();
            let remainder = denominator
                .saturating_sub(floor_sum)
                .min(floors.len() as u64);
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
        };

        let mut random = 0x9e37_79b9_7f4a_7c15u64;
        for len in 1..=32 {
            let mut probabilities = Vec::with_capacity(len);
            for _ in 0..len {
                random = random
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                probabilities.push(((random >> 32) as u32 % 10_000 + 1) as f32);
            }
            for denominator in [
                MWSOL_I16_DENOMINATOR as u64,
                u64::from(MWSOL_U16_DENOMINATOR),
            ] {
                assert_eq!(
                    quantize_largest_remainder(&probabilities, denominator),
                    reference(&probabilities, denominator)
                );
            }
        }
    }
    #[test]
    fn quantize_i16_sums_exactly_and_stays_non_negative() {
        assert_eq!(quantize_i16(&[0.5, 0.5]), vec![16_384, 16_383]);
        assert_eq!(quantize_u16(&[0.5, 0.5]), vec![32_768, 32_767]);
        assert_eq!(quantize_u16(&[1.0 / 3.0; 3]), vec![21_845; 3]);

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
    fn u16_storage_round_trips_and_uses_the_full_denominator() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("unsigned.mwsol");
        let expected = solution();
        write_mwsol_with(&path, &expected, MwsolStorage::U16).unwrap();

        let reader = MwSolReader::open(&path).unwrap();
        assert_eq!(reader.format_version(), 4);
        let decoded = decode_via_reader(&path).unwrap();
        for (got, want) in decoded.strategies.iter().zip(&expected.strategies) {
            assert_eq!(got.actions, want.actions);
            let max_error = got
                .probabilities
                .iter()
                .zip(&want.probabilities)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(max_error <= 1.0 / f32::from(u16::MAX));
        }
        let quantized = quantize_u16(&[1.0 / 3.0; 3]);
        assert_eq!(quantized.iter().map(|&q| u64::from(q)).sum::<u64>(), 65_535);
    }

    #[test]
    fn legacy_versions_remain_readable_behind_the_migration_gate() {
        let directory = tempfile::tempdir().unwrap();
        let v2_path = directory.path().join("v2.mwsol");
        let v3_path = directory.path().join("v3.mwsol");
        let v4_path = directory.path().join("v4.mwsol");
        let expected = solution();

        write_legacy_v2(&v2_path, &expected).unwrap();
        write_legacy_v3(&v3_path, &expected).unwrap();
        write_mwsol_with(&v4_path, &expected, MwsolStorage::F32).unwrap();

        let mut v2_reader = MwSolReader::open(&v2_path).unwrap();
        let mut v3_reader = MwSolReader::open(&v3_path).unwrap();
        let v4_reader = MwSolReader::open(&v4_path).unwrap();
        assert_eq!(v2_reader.format_version(), 2);
        assert_eq!(v3_reader.format_version(), 3);
        assert_eq!(v4_reader.format_version(), 4);

        let v2_decoded = decode_via_reader(&v2_path).unwrap();
        let v3_decoded = decode_via_reader(&v3_path).unwrap();
        let v4_decoded = decode_via_reader(&v4_path).unwrap();
        assert!(v2_decoded.public_states.is_empty());
        assert!(v2_decoded.strategy_weights.is_empty());
        assert!(v3_decoded.public_states.is_empty());
        assert!(v3_decoded.strategy_weights.is_empty());
        assert_eq!(v2_decoded.strategies, expected.strategies);
        assert_eq!(v3_decoded.strategies, expected.strategies);
        assert_eq!(v4_decoded, expected);

        let v2_page = v2_reader.read_strategy_page(0, 2).unwrap();
        let v3_page = v3_reader.read_strategy_page(0, 2).unwrap();
        assert_eq!(v2_page.strategies, v3_page.strategies);
    }
}
