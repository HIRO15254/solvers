use std::collections::HashMap;
use std::mem::size_of;

use super::InfoKey;

/// Compact previous-profile storage for
/// [`super::MultiwaySolver::strategy_drift_refresh_compact`].
///
/// Dense solvers retain one `u32` column id and one `u32` action count per
/// observed column, plus all normalized `f32` probabilities in one contiguous
/// allocation. Sparse research solvers keep the legacy map because they have
/// no fixed column layout.
#[derive(Default)]
pub struct StrategyDriftTracker {
    pub(super) identity: Option<StrategyDriftIdentity>,
    pub(super) dense_columns: Vec<u32>,
    pub(super) dense_action_counts: Vec<u32>,
    pub(super) dense_probabilities: Vec<f32>,
    pub(super) sparse_prior: HashMap<InfoKey, Vec<f32>>,
}

impl StrategyDriftTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets the prior profile and its layout binding. The next refresh
    /// seeds a new baseline and reports zero drift for every first-seen
    /// column, matching the legacy API's empty-map behavior.
    pub fn reset(&mut self) {
        self.identity = None;
        self.dense_columns.clear();
        self.dense_action_counts.clear();
        self.dense_probabilities.clear();
        self.sparse_prior.clear();
    }

    /// Retained heap capacity attributable to the tracker's policy payload.
    /// Allocator and `HashMap` control-byte overhead are deliberately omitted.
    pub fn retained_payload_bytes(&self) -> usize {
        self.dense_columns.capacity() * size_of::<u32>()
            + self.dense_action_counts.capacity() * size_of::<u32>()
            + self.dense_probabilities.capacity() * size_of::<f32>()
            + self.sparse_prior.capacity() * (size_of::<InfoKey>() + size_of::<Vec<f32>>())
            + self
                .sparse_prior
                .values()
                .map(|probabilities| probabilities.capacity() * size_of::<f32>())
                .sum::<usize>()
    }

    pub fn observed_columns(&self) -> usize {
        match self.identity {
            Some(StrategyDriftIdentity { dense: true, .. }) => self.dense_columns.len(),
            Some(StrategyDriftIdentity { dense: false, .. }) => self.sparse_prior.len(),
            None => 0,
        }
    }

    pub fn observed_action_slots(&self) -> usize {
        match self.identity {
            Some(StrategyDriftIdentity { dense: true, .. }) => self.dense_probabilities.len(),
            Some(StrategyDriftIdentity { dense: false, .. }) => self
                .sparse_prior
                .values()
                .map(|probabilities| probabilities.len())
                .sum(),
            None => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StrategyDriftIdentity {
    pub(super) dense: bool,
    pub(super) players: u8,
    pub(super) total_columns: u64,
    pub(super) total_slots: u64,
    pub(super) configuration: [u8; 32],
    pub(super) abstraction: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StrategyDriftError {
    #[error("strategy-drift tracker belongs to an incompatible solver layout; reset it explicitly")]
    IncompatibleLayout,
    #[error("strategy-drift tracker action-count metadata is corrupt")]
    CorruptTracker,
}
