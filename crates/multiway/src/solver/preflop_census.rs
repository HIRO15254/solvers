//! Complete, read-only storage and numeric-support census of the dense
//! preflop tree. These counts are not visits, reach, ESS or strategic quality.

use super::*;
use crate::{BettingState, HoldemGame, MultiwayAbstraction};

/// Compact support summary for one materialized preflop decision. Numeric
/// counts concern touched columns only; an all-zero touched column is stored
/// but does not establish nonzero regret or positive average support.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflopSupportNode {
    pub node_id: NodeId,
    pub history: HistoryKey,
    pub parent_history: Option<HistoryKey>,
    pub parent_action_index: Option<u32>,
    pub actor: u8,
    pub active_opponents: u8,
    pub bucket_active_opponents: u8,
    pub aggressive_actions: u8,
    pub preflop_limpers: u8,
    pub preflop_flats: u8,
    pub action_labels: Vec<String>,
    pub expected_buckets: u64,
    pub stored_buckets: u64,
    pub nonzero_regret_buckets: u64,
    pub positive_regret_buckets: u64,
    pub positive_average_buckets: u64,
    pub average_and_nonzero_regret_buckets: u64,
    /// BLAKE3 of the framed public metadata and every expected bucket's
    /// touched byte and raw f32 bits, including untouched columns and -0.
    pub raw_state_fingerprint: String,
}

/// All materialized preflop decisions, including completely untouched nodes.
/// Counts describe storage/numeric support, not visit counts, ESS, convergence
/// or quality. No sampled world, private-card lookup or solver update occurs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflopSupportCensus {
    pub schema_version: &'static str,
    pub total_materialized_nodes: u64,
    pub total_materialized_columns: u64,
    pub preflop_nodes: u64,
    pub total_expected_buckets: u64,
    pub total_stored_buckets: u64,
    pub total_nonzero_regret_buckets: u64,
    pub total_positive_regret_buckets: u64,
    pub total_positive_average_buckets: u64,
    pub total_average_and_nonzero_regret_buckets: u64,
    /// Ordered preflop node digests only. Postflop policy contents do not
    /// contribute; the all-street materialized counts are separate metadata.
    pub raw_state_fingerprint: String,
    /// Increasing materialized node id, equivalent to public-action preorder.
    pub nodes: Vec<PreflopSupportNode>,
}

fn reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<(), SolverError> {
    let requested_bytes = values
        .len()
        .checked_add(additional)
        .and_then(|count| count.checked_mul(size_of::<T>()))
        .ok_or(SolverError::MemoryAccountingOverflow)?;
    values
        .try_reserve(additional)
        .map_err(|_| TreeError::AllocationFailed {
            buffer: "preflop support census",
            requested_bytes,
        })?;
    Ok(())
}

fn add(total: &mut u64, value: u64) -> Result<(), SolverError> {
    *total = total
        .checked_add(value)
        .ok_or(SolverError::CounterOverflow)?;
    Ok(())
}

/// Canonical v1 node framing: domain with NUL terminator; node id u32;
/// history[16]; parent-present byte, optional parent history[16] and action
/// u32; six u8 context fields; expected bucket count u64; action count u64;
/// each UTF-8 label's u64 byte length and bytes; then ascending buckets as
/// bucket u32, touched byte, all regret f32 bits, all average f32 bits.
/// Every multi-byte integer is little-endian. Counts are derivable from this
/// payload and are not independently inserted into the node digest.
fn node_hasher(node: &PreflopSupportNode) -> blake3::Hasher {
    let mut hash = blake3::Hasher::new();
    hash.update(b"solvers.multiway.preflop-support-node.v1\0");
    hash.update(&node.node_id.to_le_bytes());
    hash.update(&node.history.0);
    match (node.parent_history, node.parent_action_index) {
        (Some(parent), Some(action)) => {
            hash.update(&[1]);
            hash.update(&parent.0);
            hash.update(&action.to_le_bytes());
        }
        (None, None) => {
            hash.update(&[0]);
        }
        _ => unreachable!("census parent metadata is constructed together"),
    }
    hash.update(&[
        node.actor,
        node.active_opponents,
        node.bucket_active_opponents,
        node.aggressive_actions,
        node.preflop_limpers,
        node.preflop_flats,
    ]);
    hash.update(&node.expected_buckets.to_le_bytes());
    hash.update(&(node.action_labels.len() as u64).to_le_bytes());
    for label in &node.action_labels {
        hash.update(&(label.len() as u64).to_le_bytes());
        hash.update(label.as_bytes());
    }
    hash
}

impl<A: MultiwayAbstraction> MultiwaySolver<HoldemGame<A>> {
    /// Inspect every materialized preflop decision using public states only.
    /// Requires current-street dense storage; unsupported storage is rejected.
    /// All expected raw preflop columns are checked for finite values and
    /// nonnegative average mass, including untouched columns. Numeric-support
    /// counters include touched columns only. The method borrows policy slices
    /// and retains compact node summaries, never a cloned solver snapshot.
    ///
    /// The full fingerprint frames a NUL-terminated census domain, preflop
    /// node count as little-endian u64, then fixed 32-byte node digests in
    /// public preorder. It is a raw preflop identity, not a quality metric or
    /// a replacement for configuration/abstraction/checkpoint identities.
    pub fn preflop_support_census(&self) -> Result<PreflopSupportCensus, SolverError> {
        if self.game.recall_mode() != RecallMode::Street {
            return Err(SolverError::PreallocatedStorageRequiresStreetRecall);
        }
        let dense = self.dense.as_ref().ok_or(SolverError::InvalidState(
            "preflop support census requires dense storage",
        ))?;
        let tree = &dense.tree;
        let arena = &dense.arena;
        if arena.node_count() != tree.nodes.len() {
            return Err(SolverError::InvalidState(
                "preflop census tree/arena mismatch",
            ));
        }
        let expected_nodes = tree
            .nodes
            .iter()
            .filter(|node| node.street == Street::Preflop)
            .count();
        let mut census = PreflopSupportCensus {
            schema_version: "solvers.multiway-preflop-support-census/v1",
            total_materialized_nodes: tree.nodes.len() as u64,
            total_materialized_columns: arena.total_columns(),
            preflop_nodes: expected_nodes as u64,
            total_expected_buckets: 0,
            total_stored_buckets: 0,
            total_nonzero_regret_buckets: 0,
            total_positive_regret_buckets: 0,
            total_positive_average_buckets: 0,
            total_average_and_nonzero_regret_buckets: 0,
            raw_state_fingerprint: String::new(),
            nodes: vec![],
        };
        reserve(&mut census.nodes, expected_nodes)?;
        let mut hash = blake3::Hasher::new();
        hash.update(b"solvers.multiway.preflop-support-census.v1\0");
        hash.update(&(expected_nodes as u64).to_le_bytes());
        // Iterating materialized preflop ids independently of DFS detects
        // omitted, duplicated or out-of-order preflop nodes without a bitset
        // covering the much larger postflop tree.
        let mut expected_order = tree
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.street == Street::Preflop)
            .map(|(id, _)| id as NodeId);
        let mut pending: Vec<(NodeId, BettingState)> = vec![];
        let root = self.game.root_state();
        if root.street == Street::Preflop && self.game.actor(&root).is_some() {
            let node = tree
                .nodes
                .first()
                .ok_or(SolverError::InvalidState("preflop census missing root"))?;
            if node.parent.is_some() || node.history != HistoryKey::ROOT {
                return Err(SolverError::InvalidState(
                    "preflop census invalid root history",
                ));
            }
            reserve(&mut pending, 1)?;
            pending.push((0, root));
        }
        let mut label = String::new();
        while let Some((id, state)) = pending.pop() {
            if expected_order.next() != Some(id) {
                return Err(SolverError::InvalidState(
                    "preflop census node order mismatch",
                ));
            }
            let node = tree
                .nodes
                .get(id as usize)
                .ok_or(SolverError::InvalidState("preflop census missing decision"))?;
            let context = self.game.dense_node_context(&state);
            if state.street != Street::Preflop
                || node.street != state.street
                || self.game.actor(&state) != Some(node.actor as usize)
                || node.active_opponents != context.active_opponents
                || node.bucket_active_opponents != context.bucket_active_opponents
                || tree.by_history.get(&node.history) != Some(&id)
            {
                return Err(SolverError::InvalidState(
                    "preflop census public context mismatch",
                ));
            }
            let actions = self.game.node_actions(&state);
            let action_count = self.game.num_actions_of(&actions);
            if action_count == 0
                || action_count != node.action_labels.len()
                || action_count != node.children.len()
            {
                return Err(SolverError::InvalidState(
                    "preflop census public menu mismatch",
                ));
            }
            validate_action_labels(&node.action_labels)?;
            let expected_buckets = self
                .game
                .bucket_count(Street::Preflop, context.bucket_active_opponents);
            if expected_buckets == 0 || expected_buckets != arena.bucket_count_of(id) {
                return Err(SolverError::InvalidState(
                    "preflop census bucket count mismatch",
                ));
            }
            let parent_history = match node.parent {
                Some(parent) => Some(
                    tree.nodes
                        .get(parent as usize)
                        .ok_or(SolverError::InvalidState("preflop census missing parent"))?
                        .history,
                ),
                None => None,
            };
            let mut summary = PreflopSupportNode {
                node_id: id,
                history: node.history,
                parent_history,
                parent_action_index: node.parent.map(|_| node.parent_action_index),
                actor: node.actor,
                active_opponents: node.active_opponents,
                bucket_active_opponents: node.bucket_active_opponents,
                aggressive_actions: state.aggressive_actions,
                preflop_limpers: state.preflop_limpers,
                preflop_flats: state.preflop_flats,
                action_labels: node.action_labels.clone(),
                expected_buckets: u64::from(expected_buckets),
                stored_buckets: 0,
                nonzero_regret_buckets: 0,
                positive_regret_buckets: 0,
                positive_average_buckets: 0,
                average_and_nonzero_regret_buckets: 0,
                raw_state_fingerprint: String::new(),
            };
            let mut node_hash = node_hasher(&summary);
            for bucket in 0..expected_buckets {
                let column = arena.column_id(id, bucket)?;
                let range = arena.slot_range(id, bucket)?;
                if range.len() != action_count {
                    return Err(SolverError::InvalidState(
                        "preflop census column width mismatch",
                    ));
                }
                let regrets = arena
                    .regrets
                    .get(range.clone())
                    .ok_or(SolverError::InvalidState(
                        "preflop census missing regret slots",
                    ))?;
                let average = arena
                    .strategy_sum
                    .get(range)
                    .ok_or(SolverError::InvalidState(
                        "preflop census missing average slots",
                    ))?;
                let touched = arena.is_touched(column);
                node_hash.update(&bucket.to_le_bytes());
                node_hash.update(&[u8::from(touched)]);
                let mut nonzero = false;
                let mut positive = false;
                let mut positive_average = false;
                for &regret in regrets {
                    if !regret.is_finite() {
                        return Err(SolverError::InvalidState("preflop census nonfinite regret"));
                    }
                    nonzero |= regret != 0.0;
                    positive |= regret > 0.0;
                    node_hash.update(&regret.to_bits().to_le_bytes());
                }
                for &value in average {
                    if !value.is_finite() || value < 0.0 {
                        return Err(SolverError::InvalidState(
                            "preflop census invalid average mass",
                        ));
                    }
                    positive_average |= value > 0.0;
                    node_hash.update(&value.to_bits().to_le_bytes());
                }
                if touched {
                    summary.stored_buckets += 1;
                    summary.nonzero_regret_buckets += u64::from(nonzero);
                    summary.positive_regret_buckets += u64::from(positive);
                    summary.positive_average_buckets += u64::from(positive_average);
                    summary.average_and_nonzero_regret_buckets +=
                        u64::from(positive_average && nonzero);
                }
            }
            let node_digest = node_hash.finalize();
            hash.update(node_digest.as_bytes());
            summary.raw_state_fingerprint = node_digest.to_hex().to_string();
            add(&mut census.total_expected_buckets, summary.expected_buckets)?;
            add(&mut census.total_stored_buckets, summary.stored_buckets)?;
            add(
                &mut census.total_nonzero_regret_buckets,
                summary.nonzero_regret_buckets,
            )?;
            add(
                &mut census.total_positive_regret_buckets,
                summary.positive_regret_buckets,
            )?;
            add(
                &mut census.total_positive_average_buckets,
                summary.positive_average_buckets,
            )?;
            add(
                &mut census.total_average_and_nonzero_regret_buckets,
                summary.average_and_nonzero_regret_buckets,
            )?;
            census.nodes.push(summary);

            reserve(&mut pending, action_count)?;
            for action in (0..action_count).rev() {
                label.clear();
                self.game.write_action_label(&actions, action, &mut label);
                if label != node.action_labels[action] {
                    return Err(SolverError::InvalidState(
                        "preflop census action label mismatch",
                    ));
                }
                let next = self.game.next_state_with(&state, &actions, action);
                match node.children[action] {
                    Child::Terminal => {
                        if self.game.actor(&next).is_some() {
                            return Err(SolverError::InvalidState(
                                "preflop census terminal mismatch",
                            ));
                        }
                    }
                    Child::Decision(child_id) => {
                        let child = tree
                            .nodes
                            .get(child_id as usize)
                            .ok_or(SolverError::InvalidState("preflop census missing child"))?;
                        if child.parent != Some(id)
                            || child.parent_action_index as usize != action
                            || child.history != node.history.child(node.actor as usize, action)
                            || child.street != next.street
                            || self.game.actor(&next) != Some(child.actor as usize)
                        {
                            return Err(SolverError::InvalidState(
                                "preflop census child context mismatch",
                            ));
                        }
                        if next.street == Street::Preflop {
                            pending.push((child_id, next));
                        }
                    }
                }
            }
        }
        if expected_order.next().is_some() || census.nodes.len() != expected_nodes {
            return Err(SolverError::InvalidState(
                "preflop census omitted materialized nodes",
            ));
        }
        census.raw_state_fingerprint = hash.finalize().to_hex().to_string();
        Ok(census)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestSolver = MultiwaySolver<HoldemGame<crate::abstraction::FeatureHashAbstraction>>;

    fn fixture() -> TestSolver {
        let (game, sampler, config) = super::super::tests::initialization_holdem_fixture();
        MultiwaySolver::new(game, sampler, config).unwrap()
    }

    #[test]
    fn preflop_census_contains_every_untouched_materialized_decision() {
        let solver = fixture();
        let dense = solver.dense.as_ref().unwrap();
        let ids = dense
            .tree
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.street == Street::Preflop)
            .map(|(id, _)| id as NodeId)
            .collect::<Vec<_>>();
        let expected_columns = ids
            .iter()
            .map(|&id| u64::from(dense.arena.bucket_count_of(id)))
            .sum::<u64>();
        let before = solver.snapshot_state();
        let census = solver.preflop_support_census().unwrap();
        assert!(ids.len() > 1);
        assert_eq!(census.preflop_nodes, ids.len() as u64);
        assert_eq!(
            census
                .nodes
                .iter()
                .map(|node| node.node_id)
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(census.total_expected_buckets, expected_columns);
        assert_eq!(
            census.total_materialized_nodes,
            dense.tree.nodes.len() as u64
        );
        assert_eq!(
            census.total_materialized_columns,
            dense.arena.total_columns()
        );
        assert_eq!(census.total_stored_buckets, 0);
        assert_eq!(census.total_nonzero_regret_buckets, 0);
        assert_eq!(census.total_positive_regret_buckets, 0);
        assert_eq!(census.total_positive_average_buckets, 0);
        assert_eq!(census.total_average_and_nonzero_regret_buckets, 0);
        assert!(
            census
                .nodes
                .iter()
                .all(|node| node.expected_buckets == 169 && node.stored_buckets == 0)
        );
        assert_eq!(solver.preflop_support_census().unwrap(), census);
        assert_eq!(solver.snapshot_state(), before);
    }

    #[test]
    fn preflop_census_distinguishes_stored_zero_negative_positive_and_average_only() {
        let mut solver = fixture();
        let arena = &mut solver.dense.as_mut().unwrap().arena;
        for bucket in 0..6 {
            let column = arena.column_id(0, bucket).unwrap();
            arena.touched_set(column);
        }
        // 0: stored zero, 1: negative-only, 2: positive-only, 3: average-only,
        // 4: negative plus average, 5: equal positive regrets (uniform policy).
        for (bucket, regret) in [(1, -2.0), (2, 3.0), (4, -1.0)] {
            let range = arena.slot_range(0, bucket).unwrap();
            arena.regrets[range.start] = regret;
        }
        let range = arena.slot_range(0, 5).unwrap();
        arena.regrets[range].fill(1.0);
        for bucket in [3, 4] {
            let range = arena.slot_range(0, bucket).unwrap();
            arena.strategy_sum[range.start] = 0.5;
        }
        let before = solver.snapshot_state();
        let census = solver.preflop_support_census().unwrap();
        let root = &census.nodes[0];
        assert_eq!(root.stored_buckets, 6);
        assert_eq!(root.nonzero_regret_buckets, 4);
        assert_eq!(root.positive_regret_buckets, 2);
        assert_eq!(root.positive_average_buckets, 2);
        assert_eq!(root.average_and_nonzero_regret_buckets, 1);
        assert_eq!(census.total_stored_buckets, 6);
        assert_eq!(census.total_nonzero_regret_buckets, 4);
        assert_eq!(census.total_positive_regret_buckets, 2);
        assert_eq!(census.total_positive_average_buckets, 2);
        assert_eq!(census.total_average_and_nonzero_regret_buckets, 1);
        assert_eq!(solver.snapshot_state(), before);
        assert_eq!(
            serde_json::to_value(&census).unwrap()["schemaVersion"],
            "solvers.multiway-preflop-support-census/v1"
        );
    }

    #[test]
    fn preflop_census_fingerprint_includes_untouched_bits_signed_zero_and_touched_state() {
        let mut solver = fixture();
        let initial = solver.preflop_support_census().unwrap();
        let range = solver
            .dense
            .as_ref()
            .unwrap()
            .arena
            .slot_range(0, 0)
            .unwrap();
        solver.dense.as_mut().unwrap().arena.regrets[range.start] = -0.0;
        let minus_regret_zero = solver.preflop_support_census().unwrap();
        assert_ne!(
            initial.raw_state_fingerprint,
            minus_regret_zero.raw_state_fingerprint
        );
        solver.dense.as_mut().unwrap().arena.strategy_sum[range.start] = -0.0;
        let minus_average_zero = solver.preflop_support_census().unwrap();
        assert_ne!(
            minus_regret_zero.raw_state_fingerprint,
            minus_average_zero.raw_state_fingerprint
        );
        assert_eq!(minus_average_zero.total_stored_buckets, 0);
        assert_eq!(minus_average_zero.total_nonzero_regret_buckets, 0);
        let arena = &mut solver.dense.as_mut().unwrap().arena;
        arena.touched_set(arena.column_id(0, 0).unwrap());
        let touched_zero = solver.preflop_support_census().unwrap();
        assert_ne!(
            minus_average_zero.raw_state_fingerprint,
            touched_zero.raw_state_fingerprint
        );
        assert_eq!(touched_zero.total_stored_buckets, 1);
        assert_eq!(touched_zero.total_nonzero_regret_buckets, 0);
        // Raw numeric bits in an untouched column are fingerprinted, but the
        // column still supplies no stored policy to the evaluation fallback.
        let arena = &mut solver.dense.as_mut().unwrap().arena;
        let range = arena.slot_range(0, 1).unwrap();
        arena.regrets[range.start] = 2.0;
        let unmarked_numeric = solver.preflop_support_census().unwrap();
        assert_ne!(
            touched_zero.raw_state_fingerprint,
            unmarked_numeric.raw_state_fingerprint
        );
        assert_eq!(unmarked_numeric.total_nonzero_regret_buckets, 0);
        assert_ne!(
            initial.nodes[0].raw_state_fingerprint,
            unmarked_numeric.nodes[0].raw_state_fingerprint
        );
        assert_eq!(initial.nodes[1..], unmarked_numeric.nodes[1..]);
        let arena = &solver.dense.as_ref().unwrap().arena;
        assert_eq!(arena.regrets[0].to_bits(), (-0.0f32).to_bits());
        assert_eq!(arena.strategy_sum[0].to_bits(), (-0.0f32).to_bits());
    }

    #[test]
    fn preflop_census_does_not_hash_or_count_postflop_policy() {
        let mut solver = fixture();
        let before = solver.preflop_support_census().unwrap();
        let dense = solver.dense.as_mut().unwrap();
        let postflop = dense
            .tree
            .nodes
            .iter()
            .position(|node| node.street != Street::Preflop)
            .unwrap() as NodeId;
        let range = dense.arena.slot_range(postflop, 0).unwrap();
        dense.arena.regrets[range.start] = 9.0;
        dense.arena.strategy_sum[range.start] = 7.0;
        dense
            .arena
            .touched_set(dense.arena.column_id(postflop, 0).unwrap());
        assert_eq!(solver.preflop_support_census().unwrap(), before);
    }

    #[test]
    fn preflop_census_rejects_invalid_raw_columns_even_when_untouched() {
        for touched in [false, true] {
            for defect in 0..4 {
                let mut solver = fixture();
                let arena = &mut solver.dense.as_mut().unwrap().arena;
                let range = arena.slot_range(0, 0).unwrap();
                match defect {
                    0 => arena.regrets[range.start] = f32::NAN,
                    1 => arena.regrets[range.start] = f32::INFINITY,
                    2 => arena.strategy_sum[range.start] = f32::NAN,
                    _ => arena.strategy_sum[range.start] = -1.0,
                }
                if touched {
                    arena.touched_set(arena.column_id(0, 0).unwrap());
                }
                let regret_bits = arena
                    .regrets
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>();
                let average_bits = arena
                    .strategy_sum
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>();
                assert!(solver.preflop_support_census().is_err());
                let arena = &solver.dense.as_ref().unwrap().arena;
                assert_eq!(
                    arena
                        .regrets
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                    regret_bits
                );
                assert_eq!(
                    arena
                        .strategy_sum
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                    average_bits
                );
                assert_eq!(arena.touched_count(), u64::from(touched));
            }
        }
    }

    #[test]
    fn preflop_census_rejects_public_tree_corruption_and_absent_dense_storage() {
        for defect in 0..4 {
            let mut solver = fixture();
            let dense = solver.dense.as_mut().unwrap();
            match defect {
                0 => dense.tree.nodes[0].action_labels[0].push_str("-wrong"),
                1 => dense.tree.nodes[0].active_opponents = 0,
                2 => dense.tree.nodes[0].children[0] = Child::Terminal,
                _ => dense.tree.nodes[0].history = HistoryKey([1; 16]),
            }
            assert!(solver.preflop_support_census().is_err());
        }
        let mut solver = fixture();
        solver.dense = None;
        assert!(solver.preflop_support_census().is_err());
    }

    #[test]
    fn preflop_census_rejects_full_recall() {
        let config: crate::config::MultiwayConfig = serde_json::from_value(serde_json::json!({
            "seats": [{"stack_bb": 6.0}, {"stack_bb": 6.0}, {"stack_bb": 6.0}],
            "button": 0,
            "abstraction": {
                "flop_buckets": 4, "turn_buckets": 4, "river_buckets": 4,
                "recall": "full"
            }
        }))
        .unwrap();
        let game = HoldemGame::new(
            &config,
            &crate::config::UtilityConfig::ChipEv,
            &crate::config::RakeConfig::None,
            crate::abstraction::FeatureHashAbstraction::new(
                crate::abstraction::FeatureHashParams {
                    flop_buckets: 4,
                    turn_buckets: 4,
                    river_buckets: 4,
                },
            )
            .unwrap(),
        )
        .unwrap();
        let sampler = game.deal_sampler().unwrap();
        let solver = MultiwaySolver::new(game, sampler, SolverConfig::default()).unwrap();
        assert!(matches!(
            solver.preflop_support_census(),
            Err(SolverError::PreallocatedStorageRequiresStreetRecall)
        ));
    }
}
