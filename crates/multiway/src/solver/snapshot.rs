//! Serialization-only view of the existing state wire format. Policy values
//! and action labels remain borrowed; only ordering/ancestor scratch is owned.

use serde::Serialize;
use serde::ser::{SerializeSeq, Serializer};

use super::{
    DenseStorage, ExternalSamplingGame, HistoryEntry, HistoryKey, InfoKey, MultiwaySolver, NodeId,
    PolicyColumn, SOLVER_STATE_VERSION, SolverConfig,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum SnapshotError {
    #[error("snapshot scratch allocation of {requested} bytes failed")]
    AllocationFailed { requested: u64 },
    #[error("snapshot scratch length overflow")]
    LengthOverflow,
    #[error("snapshot state is inconsistent: {0}")]
    InconsistentState(&'static str),
}

// Keep the field order identical to SolverState. The sequence wrappers below
// deliberately serialize as Vec elements, without an enum discriminant.
#[derive(Serialize)]
pub(crate) struct BorrowedSolverState<'a> {
    schema_version: u16,
    config: SolverConfig,
    traversals: u64,
    completed_sweeps: u64,
    next_sample_id: u64,
    total_deal_attempts: u64,
    terminal_evaluations: u64,
    hand_updates: u64,
    histories: Histories<'a>,
    policies: Policies<'a>,
}

impl BorrowedSolverState<'_> {
    pub(crate) fn next_sample_id(&self) -> u64 {
        self.next_sample_id
    }
}

enum Histories<'a> {
    Sparse(Vec<&'a HistoryEntry>),
    Dense {
        storage: &'a DenseStorage,
        nodes: Vec<NodeId>,
    },
}

enum Policies<'a> {
    Sparse(Vec<(&'a InfoKey, &'a PolicyColumn)>),
    Dense {
        storage: &'a DenseStorage,
        nodes: Vec<NodeId>,
        count: usize,
    },
}

#[derive(Serialize)]
struct HistoryEntryRef<'a> {
    key: HistoryKey,
    parent: HistoryKey,
    actor: u8,
    action_index: u32,
    action_label: &'a str,
}

#[derive(Serialize)]
struct PolicyEntryRef<C: Serialize> {
    key: InfoKey,
    column: C,
}

#[derive(Serialize)]
struct PolicyColumnRef<'a> {
    action_labels: &'a [String],
    regrets: &'a [f32],
    strategy_sum: &'a [f32],
}

impl Serialize for Histories<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Sparse(entries) => entries.serialize(serializer),
            Self::Dense { storage, nodes } => {
                let mut sequence = serializer.serialize_seq(Some(nodes.len()))?;
                for &id in nodes {
                    let node = &storage.tree.nodes[id as usize];
                    let parent =
                        &storage.tree.nodes[node.parent.expect("validated ancestor") as usize];
                    sequence.serialize_element(&HistoryEntryRef {
                        key: node.history,
                        parent: parent.history,
                        actor: parent.actor,
                        action_index: node.parent_action_index,
                        action_label: &parent.action_labels[node.parent_action_index as usize],
                    })?;
                }
                sequence.end()
            }
        }
    }
}

impl Serialize for Policies<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Sparse(entries) => {
                let mut sequence = serializer.serialize_seq(Some(entries.len()))?;
                for &(key, column) in entries {
                    sequence.serialize_element(&PolicyEntryRef { key: *key, column })?;
                }
                sequence.end()
            }
            Self::Dense {
                storage,
                nodes,
                count,
            } => {
                let mut sequence = serializer.serialize_seq(Some(*count))?;
                for &id in nodes {
                    let node = &storage.tree.nodes[id as usize];
                    // Every field preceding bucket_path is constant within a
                    // node. Street recall changes only its current bucket, so
                    // bucket order is InfoKey order within this node prefix.
                    for bucket in 0..storage.arena.bucket_count_of(id) {
                        let column = storage
                            .arena
                            .column_id(id, bucket)
                            .expect("validated bucket");
                        if !storage.arena.is_touched(column) {
                            continue;
                        }
                        let range = storage
                            .arena
                            .slot_range(id, bucket)
                            .expect("validated bucket");
                        sequence.serialize_element(&PolicyEntryRef {
                            key: storage.info_key_for(id, bucket),
                            column: PolicyColumnRef {
                                action_labels: &node.action_labels,
                                regrets: &storage.arena.regrets[range.clone()],
                                strategy_sum: &storage.arena.strategy_sum[range],
                            },
                        })?;
                    }
                }
                sequence.end()
            }
        }
    }
}

fn scratch<T>(count: usize) -> Result<Vec<T>, SnapshotError> {
    let requested = count
        .checked_mul(std::mem::size_of::<T>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(SnapshotError::LengthOverflow)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| SnapshotError::AllocationFailed { requested })?;
    Ok(values)
}

fn dense_sequences(storage: &DenseStorage) -> Result<(Histories<'_>, Policies<'_>), SnapshotError> {
    let count = usize::try_from(storage.arena.touched_count())
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let mut nodes = scratch::<NodeId>(count.min(storage.tree.nodes.len()))?;
    let mut actual = 0usize;
    for (index, _) in storage.tree.nodes.iter().enumerate() {
        let id = NodeId::try_from(index).map_err(|_| SnapshotError::LengthOverflow)?;
        let before = actual;
        for bucket in 0..storage.arena.bucket_count_of(id) {
            let column = storage
                .arena
                .column_id(id, bucket)
                .map_err(|_| SnapshotError::InconsistentState("invalid dense column"))?;
            if storage.arena.is_touched(column) {
                actual = actual.checked_add(1).ok_or(SnapshotError::LengthOverflow)?;
                if actual > count {
                    return Err(SnapshotError::InconsistentState(
                        "dense touched count mismatch",
                    ));
                }
            }
        }
        if before != actual {
            nodes.push(id);
        }
    }
    if actual != count {
        return Err(SnapshotError::InconsistentState(
            "dense touched count mismatch",
        ));
    }
    nodes.sort_unstable_by_key(|&id| storage.info_key_for(id, 0));
    if nodes
        .windows(2)
        .any(|ids| storage.info_key_for(ids[0], 0) == storage.info_key_for(ids[1], 0))
    {
        return Err(SnapshotError::InconsistentState(
            "duplicate dense information-key prefix",
        ));
    }

    let mark_count = if nodes.is_empty() {
        0
    } else {
        storage.tree.nodes.len()
    };
    let mut marked = scratch::<u8>(mark_count)?;
    marked.resize(mark_count, 0);
    let mut history_count = 0usize;
    for &id in &nodes {
        let mut ancestor = id;
        loop {
            let node = &storage.tree.nodes[ancestor as usize];
            let Some(parent) = node.parent else { break };
            if parent >= ancestor
                || storage.tree.nodes[parent as usize]
                    .action_labels
                    .get(node.parent_action_index as usize)
                    .is_none()
            {
                return Err(SnapshotError::InconsistentState("invalid dense ancestor"));
            }
            if marked[ancestor as usize] != 0 {
                break;
            }
            marked[ancestor as usize] = 1;
            history_count += 1;
            ancestor = parent;
        }
    }
    let mut histories = scratch::<NodeId>(history_count)?;
    for (id, flag) in marked.into_iter().enumerate() {
        if flag != 0 {
            histories.push(NodeId::try_from(id).map_err(|_| SnapshotError::LengthOverflow)?);
        }
    }
    histories.sort_unstable_by_key(|&id| storage.tree.nodes[id as usize].history);
    if histories.windows(2).any(|ids| {
        storage.tree.nodes[ids[0] as usize].history == storage.tree.nodes[ids[1] as usize].history
    }) {
        return Err(SnapshotError::InconsistentState("duplicate dense history"));
    }
    Ok((
        Histories::Dense {
            storage,
            nodes: histories,
        },
        Policies::Dense {
            storage,
            nodes,
            count,
        },
    ))
}

impl<G: ExternalSamplingGame> MultiwaySolver<G> {
    /// Same values and sequence order as snapshot_state(), without cloning
    /// policy columns or labels. The immutable borrow prevents training until
    /// serialization finishes; scratch scales with public nodes for dense
    /// storage and stored entry counts for sparse storage, outside the arena.
    pub(crate) fn snapshot_state_ref(&self) -> Result<BorrowedSolverState<'_>, SnapshotError> {
        let (histories, policies) = match &self.dense {
            Some(storage) => dense_sequences(storage)?,
            None => {
                // Sparse history storage also contains entries that need not
                // be ancestors of a retained policy. Preserve every entry.
                let mut histories = scratch(self.histories.len())?;
                histories.extend(self.histories.values());
                histories.sort_unstable_by_key(|entry| entry.key);
                let mut policies = scratch(self.policies.len())?;
                policies.extend(self.policies.iter());
                policies.sort_unstable_by_key(|(key, _)| **key);
                (Histories::Sparse(histories), Policies::Sparse(policies))
            }
        };
        Ok(BorrowedSolverState {
            schema_version: SOLVER_STATE_VERSION,
            config: self.config,
            traversals: self.traversals,
            completed_sweeps: self.completed_sweeps,
            next_sample_id: self.next_sample_id,
            total_deal_attempts: self.total_deal_attempts,
            terminal_evaluations: self.terminal_evaluations,
            hand_updates: self.hand_updates,
            histories,
            policies,
        })
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::*;

    #[test]
    fn snapshot_scratch_rejects_length_overflow_and_impossible_reservation() {
        assert!(matches!(
            scratch::<u64>(usize::MAX),
            Err(SnapshotError::LengthOverflow)
        ));
        assert!(matches!(
            scratch::<u8>(usize::MAX),
            Err(SnapshotError::AllocationFailed { .. })
        ));
    }
}
