//! Public betting-tree enumeration and dense-arena preallocation backing
//! [`crate::config::RecallMode::Street`].
//!
//! No chance branching exists in this game family (chance is sampled once,
//! up front, outside the public tree; see [`crate::solver::ExternalSamplingGame`]),
//! so the entire public game tree is deterministic and can be walked once,
//! independent of any physical card world. [`enumerate_tree`] does exactly
//! that; [`build_arena`] then sizes and preallocates one contiguous,
//! node-major `[node][bucket][action]` arena of regrets/strategy sums for
//! every information set the tree can ever reach, so a Street-recall solve's
//! memory footprint is fixed at preflight time instead of growing with the
//! number of visited information sets.

use std::ops::Range;

use rustc_hash::FxHashMap;

use crate::abstraction::BucketId;
use crate::solver::{ExternalSamplingGame, HistoryKey};
use crate::types::Street;

/// Preorder index into [`PublicTree::nodes`]. Root is always `0`.
pub type NodeId = u32;

/// Hard safety cap on enumerated decision nodes. A malformed or extremely
/// wide betting configuration (many bet/raise sizes, high
/// `max_aggressive_actions`, many seats) can blow up combinatorially even
/// though it does not depend on any card range; this cap turns that into a
/// prompt, typed failure instead of an unbounded allocation.
pub const MAX_TREE_NODES: usize = 50_000_000;

/// One action's destination out of a decision node: either another decision
/// node, or a terminal state (settlement happens outside the public tree,
/// so terminals are not materialized as nodes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Child {
    Decision(NodeId),
    Terminal,
}

/// One public decision node. `action_labels` is the single shared label list
/// for every bucket at this node (there is exactly one legal-action shape
/// per public node, independent of private information), reused both to
/// build [`crate::solver::HistoryEntry`] edges and as every column's
/// `PolicyColumn::action_labels` in the dense arena.
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub history: HistoryKey,
    /// `None` only at the root.
    pub parent: Option<NodeId>,
    /// Index into the parent's `action_labels`/`children` that reaches this
    /// node. Meaningless (`0`) at the root.
    pub parent_action_index: u32,
    pub actor: u8,
    pub street: Street,
    /// Current non-folded opponent count excluding `actor`, matching what
    /// [`crate::solver::PrivateInfo::active_opponents`] carries for any
    /// physical world at this node (folding is public).
    pub active_opponents: u8,
    /// Opponent count `bucket()` feeds the abstraction for this node's
    /// street (the count as of that street's start; see
    /// `BettingState::players_on_street`). This can differ from
    /// `active_opponents` once a same-street fold has happened, since a
    /// street's abstraction context is fixed at that street's start.
    pub bucket_active_opponents: u8,
    pub action_labels: Vec<String>,
    /// Same length and order as `action_labels`.
    pub children: Vec<Child>,
}

/// The fully enumerated public betting tree, in preorder.
#[derive(Clone, Debug, Default)]
pub struct PublicTree {
    pub nodes: Vec<TreeNode>,
    pub by_history: FxHashMap<HistoryKey, NodeId>,
}

/// Walks the public game tree once, deterministically, from
/// `game.root_state()`. Only decision nodes are recorded; terminals are
/// [`Child::Terminal`] markers on their parent.
pub fn enumerate_tree<G: ExternalSamplingGame>(game: &G) -> Result<PublicTree, TreeError> {
    let root = game.root_state();
    if game.actor(&root).is_none() {
        return Err(TreeError::RootIsTerminal);
    }
    let mut nodes = Vec::new();
    let mut by_history = FxHashMap::default();
    enumerate_node(
        game,
        root,
        HistoryKey::ROOT,
        None,
        0,
        &mut nodes,
        &mut by_history,
    )?;
    Ok(PublicTree { nodes, by_history })
}

#[allow(clippy::too_many_arguments)]
fn enumerate_node<G: ExternalSamplingGame>(
    game: &G,
    state: G::State,
    history: HistoryKey,
    parent: Option<NodeId>,
    parent_action_index: u32,
    nodes: &mut Vec<TreeNode>,
    by_history: &mut FxHashMap<HistoryKey, NodeId>,
) -> Result<NodeId, TreeError> {
    let actor = game
        .actor(&state)
        .expect("caller only recurses into decision states");
    if nodes.len() >= MAX_TREE_NODES {
        return Err(TreeError::TooManyNodes {
            limit: MAX_TREE_NODES,
        });
    }
    let node_id = nodes.len() as NodeId;
    let actions = game.node_actions(&state);
    let num_actions = game.num_actions_of(&actions);
    if num_actions == 0 {
        return Err(TreeError::NoActions { actor });
    }
    let mut action_labels = Vec::with_capacity(num_actions);
    let mut label_buf = String::new();
    for index in 0..num_actions {
        label_buf.clear();
        game.write_action_label(&actions, index, &mut label_buf);
        if label_buf.is_empty() {
            return Err(TreeError::EmptyActionLabel { node: node_id });
        }
        if action_labels.contains(&label_buf) {
            return Err(TreeError::DuplicateActionLabel { node: node_id });
        }
        action_labels.push(std::mem::take(&mut label_buf));
    }
    let context = game.dense_node_context(&state);

    nodes.push(TreeNode {
        history,
        parent,
        parent_action_index,
        actor: actor as u8,
        street: context.street,
        active_opponents: context.active_opponents,
        bucket_active_opponents: context.bucket_active_opponents,
        action_labels,
        children: Vec::new(),
    });
    by_history.insert(history, node_id);

    let mut children = Vec::with_capacity(num_actions);
    for index in 0..num_actions {
        let child_state = game.next_state_with(&state, &actions, index);
        let child_history = history.child(actor, index);
        if game.actor(&child_state).is_none() {
            children.push(Child::Terminal);
        } else {
            let child_id = enumerate_node(
                game,
                child_state,
                child_history,
                Some(node_id),
                index as u32,
                nodes,
                by_history,
            )?;
            children.push(Child::Decision(child_id));
        }
    }
    nodes[node_id as usize].children = children;
    Ok(node_id)
}

/// Purely public per-node context an [`ExternalSamplingGame`] implementor
/// supplies for dense-arena preallocation; see [`TreeNode`] field docs.
#[derive(Clone, Copy, Debug)]
pub struct DenseNodeContext {
    pub street: Street,
    pub active_opponents: u8,
    pub bucket_active_opponents: u8,
}

/// Node-major contiguous `[node][bucket][action]` policy storage for every
/// information set the enumerated tree can reach. Preallocated once at
/// preflight time; memory is therefore fixed for the run's lifetime.
#[derive(Debug)]
pub struct DenseArena {
    /// Cumulative bucket counts; length `nodes.len() + 1`. Node `n`'s
    /// columns are `column_base[n]..column_base[n + 1]`.
    column_base: Vec<u64>,
    /// Cumulative `bucket_count * num_actions`; length `nodes.len() + 1`.
    slot_base: Vec<u64>,
    bucket_count: Vec<u32>,
    num_actions: Vec<u32>,
    pub regrets: Vec<f32>,
    pub strategy_sum: Vec<f32>,
    touched: Vec<u64>,
    total_columns: u64,
    total_slots: u64,
    touched_count: u64,
    estimated_bytes: u64,
}

impl DenseArena {
    pub fn node_count(&self) -> usize {
        self.bucket_count.len()
    }

    pub fn total_columns(&self) -> u64 {
        self.total_columns
    }

    pub fn total_slots(&self) -> u64 {
        self.total_slots
    }

    pub fn estimated_bytes(&self) -> u64 {
        self.estimated_bytes
    }

    pub fn touched_count(&self) -> u64 {
        self.touched_count
    }

    pub fn bucket_count_of(&self, node: NodeId) -> u32 {
        self.bucket_count[node as usize]
    }

    /// Global column id for `(node, bucket)`, used both to address the
    /// touched-bitset and as the traversal delta's wire-format column id.
    pub fn column_id(&self, node: NodeId, bucket: BucketId) -> Result<u32, TreeError> {
        let count = self.bucket_count[node as usize];
        if bucket >= count {
            return Err(TreeError::BucketOutOfRange {
                node,
                bucket,
                count,
            });
        }
        let column = self.column_base[node as usize] + u64::from(bucket);
        u32::try_from(column).map_err(|_| TreeError::ColumnIdOverflow)
    }

    /// Slot range (into [`Self::regrets`]/[`Self::strategy_sum`]) for
    /// `(node, bucket)`'s `num_actions(node)`-long action vector.
    pub fn slot_range(&self, node: NodeId, bucket: BucketId) -> Result<Range<usize>, TreeError> {
        let count = self.bucket_count[node as usize];
        if bucket >= count {
            return Err(TreeError::BucketOutOfRange {
                node,
                bucket,
                count,
            });
        }
        let num_actions = u64::from(self.num_actions[node as usize]);
        let start = self.slot_base[node as usize] + u64::from(bucket) * num_actions;
        let end = start + num_actions;
        Ok(start as usize..end as usize)
    }

    /// Resolves a wire-format `column_id` back to its slot range, checking
    /// the caller's expected action count against the node's actual one.
    pub fn slot_range_for_column(
        &self,
        column: u32,
        expected_actions: usize,
    ) -> Result<Range<usize>, TreeError> {
        let node = self.node_for_column(column)?;
        let bucket = (u64::from(column) - self.column_base[node as usize]) as u32;
        let range = self.slot_range(node, bucket)?;
        if range.len() != expected_actions {
            return Err(TreeError::ActionCountMismatch);
        }
        Ok(range)
    }

    fn node_for_column(&self, column: u32) -> Result<NodeId, TreeError> {
        let column = u64::from(column);
        if column >= self.total_columns {
            return Err(TreeError::ColumnOutOfRange(column));
        }
        // `column_base` is sorted ascending; find the last index whose base
        // does not exceed `column`.
        let idx = self.column_base.partition_point(|&base| base <= column) - 1;
        Ok(idx as NodeId)
    }

    pub fn is_touched(&self, column: u32) -> bool {
        let column = column as usize;
        let word = self.touched[column / 64];
        (word >> (column % 64)) & 1 != 0
    }

    /// Sets `column`'s touched bit, returning `true` iff this call is the
    /// first time it was set (and bumping [`Self::touched_count`] exactly
    /// once per column over the arena's lifetime).
    pub fn touched_set(&mut self, column: u32) -> bool {
        let index = column as usize;
        let word = index / 64;
        let mask = 1u64 << (index % 64);
        let already = self.touched[word] & mask != 0;
        if !already {
            self.touched[word] |= mask;
            self.touched_count += 1;
        }
        !already
    }
}

/// Enumerates `game`'s public tree, then sizes and preallocates a
/// [`DenseArena`] for it. Fails *before* allocating the (potentially huge)
/// regret/strategy-sum buffers when the estimate exceeds `max_memory_bytes`.
pub fn build_arena<G: ExternalSamplingGame>(
    game: &G,
    tree: &PublicTree,
    max_memory_bytes: u64,
) -> Result<DenseArena, TreeError> {
    let node_count = tree.nodes.len();
    let mut column_base = Vec::with_capacity(node_count + 1);
    let mut slot_base = Vec::with_capacity(node_count + 1);
    let mut bucket_count = Vec::with_capacity(node_count);
    let mut num_actions = Vec::with_capacity(node_count);
    column_base.push(0u64);
    slot_base.push(0u64);
    let mut total_columns = 0u64;
    let mut total_slots = 0u64;
    for node in &tree.nodes {
        let buckets = u64::from(game.bucket_count(node.street, node.bucket_active_opponents));
        if buckets == 0 {
            return Err(TreeError::ZeroBuckets);
        }
        let actions = node.action_labels.len() as u64;
        bucket_count.push(u32::try_from(buckets).map_err(|_| TreeError::SizeOverflow)?);
        num_actions.push(u32::try_from(actions).map_err(|_| TreeError::SizeOverflow)?);
        total_columns = total_columns
            .checked_add(buckets)
            .ok_or(TreeError::SizeOverflow)?;
        let slots = buckets
            .checked_mul(actions)
            .ok_or(TreeError::SizeOverflow)?;
        total_slots = total_slots
            .checked_add(slots)
            .ok_or(TreeError::SizeOverflow)?;
        column_base.push(total_columns);
        slot_base.push(total_slots);
    }

    let estimated_bytes = estimate_bytes(node_count, total_columns, total_slots)?;
    if estimated_bytes > max_memory_bytes {
        return Err(TreeError::MemoryLimit {
            node_count,
            total_columns,
            limit: max_memory_bytes,
            needed: estimated_bytes,
        });
    }

    let total_slots_usize = usize::try_from(total_slots).map_err(|_| TreeError::SizeOverflow)?;
    let touched_words =
        usize::try_from(total_columns.div_ceil(64)).map_err(|_| TreeError::SizeOverflow)?;
    Ok(DenseArena {
        column_base,
        slot_base,
        bucket_count,
        num_actions,
        regrets: vec![0.0; total_slots_usize],
        strategy_sum: vec![0.0; total_slots_usize],
        touched: vec![0u64; touched_words],
        total_columns,
        total_slots,
        touched_count: 0,
        estimated_bytes,
    })
}

/// Two `f32` arrays per slot (regrets + strategy sums) plus one touched bit
/// per column plus a small constant per-node table overhead
/// (`column_base`/`slot_base`/`bucket_count`/`num_actions` entries).
fn estimate_bytes(
    node_count: usize,
    total_columns: u64,
    total_slots: u64,
) -> Result<u64, TreeError> {
    let slot_bytes = total_slots
        .checked_mul(2 * size_of_f32())
        .ok_or(TreeError::SizeOverflow)?;
    let touched_bytes = total_columns
        .div_ceil(64)
        .checked_mul(8)
        .ok_or(TreeError::SizeOverflow)?;
    let node_table_bytes = (node_count as u64)
        .checked_mul(24)
        .ok_or(TreeError::SizeOverflow)?;
    slot_bytes
        .checked_add(touched_bytes)
        .and_then(|value| value.checked_add(node_table_bytes))
        .ok_or(TreeError::SizeOverflow)
}

const fn size_of_f32() -> u64 {
    std::mem::size_of::<f32>() as u64
}

#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("root state has no acting seat; nothing to enumerate")]
    RootIsTerminal,
    #[error(
        "public betting tree exceeds the {limit} decision-node safety cap; restrict the betting \
         tree (fewer bet/raise sizes, lower max_aggressive_actions, fewer seats) before using \
         recall = \"street\""
    )]
    TooManyNodes { limit: usize },
    #[error("decision node for actor {actor} has no legal actions")]
    NoActions { actor: usize },
    #[error("public tree node {node} produced an empty action label")]
    EmptyActionLabel { node: NodeId },
    #[error("public tree node {node} produced a duplicate action label")]
    DuplicateActionLabel { node: NodeId },
    #[error("abstraction reported zero buckets for a reachable street")]
    ZeroBuckets,
    #[error("dense arena size overflowed while summing bucket/slot counts")]
    SizeOverflow,
    #[error(
        "dense arena preflight for {node_count} nodes / {total_columns} columns needs {needed} \
         bytes, exceeding the {limit} byte memory limit; use recall = \"full\" or shrink the \
         abstraction/betting tree"
    )]
    MemoryLimit {
        node_count: usize,
        total_columns: u64,
        limit: u64,
        needed: u64,
    },
    #[error("dense arena column id overflowed u32")]
    ColumnIdOverflow,
    #[error("bucket {bucket} is outside node {node}'s {count} buckets")]
    BucketOutOfRange {
        node: NodeId,
        bucket: BucketId,
        count: u32,
    },
    #[error("column {0} is outside the dense arena")]
    ColumnOutOfRange(u64),
    #[error("dense arena column action count does not match the caller's expectation")]
    ActionCountMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstraction::FeatureHashAbstraction;
    use crate::config::{AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, SeatConfig};
    use crate::config::{MultiwayConfig, RakeConfig, UtilityConfig};
    use crate::holdem::HoldemGame;
    use crate::types::SeatId;

    fn smoke_config() -> MultiwayConfig {
        MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 10.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            abstraction: AbstractionConfig::default(),
        }
    }

    fn smoke_game() -> HoldemGame<FeatureHashAbstraction> {
        HoldemGame::new(
            &smoke_config(),
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::default(),
        )
        .unwrap()
    }

    #[test]
    fn enumerated_tree_node_and_terminal_counts_are_stable() {
        let game = smoke_game();
        let tree = enumerate_tree(&game).unwrap();
        // Exact counts pin the enumeration; any change to the smoke config's
        // betting tree, or a bug in `enumerate_node`, moves these numbers.
        let terminal_count: usize = tree
            .nodes
            .iter()
            .flat_map(|node| node.children.iter())
            .filter(|child| matches!(child, Child::Terminal))
            .count();
        assert_eq!(tree.nodes.len(), 3_658);
        assert_eq!(terminal_count, 4_366);
        assert_eq!(tree.by_history.len(), tree.nodes.len());
        assert_eq!(tree.nodes[0].history, HistoryKey::ROOT);
        assert!(tree.nodes[0].parent.is_none());
    }

    #[test]
    fn arena_preflight_matches_manual_bucket_sum() {
        let game = smoke_game();
        let tree = enumerate_tree(&game).unwrap();
        let mut expected_columns = 0u64;
        let mut expected_slots = 0u64;
        for node in &tree.nodes {
            let buckets = u64::from(game.bucket_count(node.street, node.bucket_active_opponents));
            expected_columns += buckets;
            expected_slots += buckets * node.action_labels.len() as u64;
        }
        let arena = build_arena(&game, &tree, u64::MAX).unwrap();
        assert_eq!(arena.total_columns(), expected_columns);
        assert_eq!(arena.total_slots(), expected_slots);
        assert_eq!(arena.regrets.len() as u64, expected_slots);
        assert_eq!(arena.strategy_sum.len() as u64, expected_slots);
    }

    #[test]
    fn preflight_fails_before_allocating_when_memory_limit_is_tiny() {
        let game = smoke_game();
        let tree = enumerate_tree(&game).unwrap();
        let error = build_arena(&game, &tree, 1).unwrap_err();
        assert!(matches!(error, TreeError::MemoryLimit { .. }));
    }

    #[test]
    fn column_and_slot_addressing_round_trip() {
        let game = smoke_game();
        let tree = enumerate_tree(&game).unwrap();
        let arena = build_arena(&game, &tree, u64::MAX).unwrap();
        for (node_id, node) in tree.nodes.iter().enumerate() {
            let node_id = node_id as NodeId;
            let bucket_count = arena.bucket_count_of(node_id);
            for bucket in 0..bucket_count.min(5) {
                let column = arena.column_id(node_id, bucket).unwrap();
                let range = arena.slot_range(node_id, bucket).unwrap();
                assert_eq!(range.len(), node.action_labels.len());
                let resolved = arena
                    .slot_range_for_column(column, node.action_labels.len())
                    .unwrap();
                assert_eq!(resolved, range);
            }
        }
    }

    #[test]
    fn touched_bit_is_idempotent_and_counts_once() {
        let game = smoke_game();
        let tree = enumerate_tree(&game).unwrap();
        let mut arena = build_arena(&game, &tree, u64::MAX).unwrap();
        let column = arena.column_id(0, 0).unwrap();
        assert!(!arena.is_touched(column));
        assert!(arena.touched_set(column));
        assert!(arena.is_touched(column));
        assert!(!arena.touched_set(column));
        assert_eq!(arena.touched_count(), 1);
    }
}
