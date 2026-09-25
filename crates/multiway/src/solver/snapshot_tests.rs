use super::*;
use crate::checkpoint::{CheckpointRuntimeState, MultiwayCheckpoint};

fn assert_snapshot_encoding<G: ExternalSamplingGame>(solver: &MultiwaySolver<G>) {
    let owned = solver.snapshot_state();
    let borrowed = solver.snapshot_state_ref().unwrap();
    assert_eq!(borrowed.next_sample_id(), owned.next_sample_id);
    assert_eq!(
        postcard::to_allocvec(&borrowed).unwrap(),
        postcard::to_allocvec(&owned).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&borrowed).unwrap(),
        serde_json::to_value(&owned).unwrap()
    );
    assert_eq!(
        solver.snapshot_state(),
        owned,
        "serialization must not mutate state"
    );
}

fn assert_checkpoint_encoding<G: ExternalSamplingGame>(
    solver: &MultiwaySolver<G>,
    config: &str,
    runtime: CheckpointRuntimeState,
) -> SolverState {
    let directory = tempfile::tempdir().unwrap();
    let owned_path = directory.path().join("owned.mwckpt");
    let borrowed_path = directory.path().join("borrowed.mwckpt");
    let checkpoint = MultiwayCheckpoint::capture(solver).with_runtime_metadata(config, runtime);
    checkpoint.write_atomic(&owned_path).unwrap();
    MultiwayCheckpoint::write_solver_atomic(solver, &borrowed_path, config, runtime).unwrap();
    assert_eq!(
        std::fs::read(&borrowed_path).unwrap(),
        std::fs::read(&owned_path).unwrap()
    );
    let loaded = MultiwayCheckpoint::load(
        &borrowed_path,
        solver.configuration_fingerprint(),
        solver.abstraction_fingerprint(),
    )
    .unwrap();
    let owned_loaded = MultiwayCheckpoint::load(
        &owned_path,
        solver.configuration_fingerprint(),
        solver.abstraction_fingerprint(),
    )
    .unwrap();
    assert_eq!(loaded, owned_loaded);
    assert_eq!(loaded.state, solver.snapshot_state());
    assert_eq!(loaded.runtime, runtime);
    loaded.state
}

#[test]
fn borrowed_snapshot_matches_empty_dense_and_sparse_state_and_container() {
    let dense = dense_toy_solver(15, 1);
    let sparse = solver(15, 1 << 20);
    assert_snapshot_encoding(&dense);
    assert_snapshot_encoding(&sparse);
    assert_checkpoint_encoding(&dense, "", CheckpointRuntimeState::default());
    assert_checkpoint_encoding(&sparse, "", CheckpointRuntimeState::default());

    // Absence of owned metadata has always encoded as an empty string.
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("without-metadata.mwckpt");
    let borrowed = directory.path().join("empty-metadata.mwckpt");
    MultiwayCheckpoint::capture(&dense)
        .write_atomic(&original)
        .unwrap();
    MultiwayCheckpoint::write_solver_atomic(
        &dense,
        &borrowed,
        "",
        CheckpointRuntimeState::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(original).unwrap(),
        std::fs::read(borrowed).unwrap()
    );
}

#[test]
fn borrowed_snapshot_trained_dense_and_sparse_resume_exactly() {
    let runtime = CheckpointRuntimeState {
        confirmations_met: 3,
        next_evaluation_sweep: 48,
        evaluation_samples: 4096,
        evaluation_sequence: 7,
        cumulative_solve_millis: 123456,
    };
    let mut dense = dense_vector_toy_solver(431, 2);
    dense.run_sweeps_with_threads(18, 2).unwrap();
    assert_snapshot_encoding(&dense);
    let state = assert_checkpoint_encoding(&dense, "# borrowed metadata\nseed = 431\n", runtime);
    let mut resumed = MultiwaySolver::from_state(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        state,
    )
    .unwrap();
    dense.run_sweeps_with_threads(6, 2).unwrap();
    resumed.run_sweeps_with_threads(6, 1).unwrap();
    assert_eq!(dense.snapshot_state(), resumed.snapshot_state());

    let mut sparse = solver(431, 1 << 20);
    sparse.run_sweeps(18).unwrap();
    assert_snapshot_encoding(&sparse);
    let state = assert_checkpoint_encoding(&sparse, "# full recall\n", runtime);
    let mut resumed = MultiwaySolver::from_state(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        state,
    )
    .unwrap();
    sparse.run_sweeps(6).unwrap();
    resumed.run_sweeps(6).unwrap();
    assert_eq!(sparse.snapshot_state(), resumed.snapshot_state());
}

#[test]
fn borrowed_snapshot_keeps_zero_touched_columns_and_untouched_ancestors_across_streets() {
    let (game, sampler, config) = initialization_holdem_fixture();
    let mut solver = MultiwaySolver::new(game, sampler, config).unwrap();
    let dense = solver.dense.as_mut().unwrap();
    let mut selected = Vec::new();
    for street in [Street::Preflop, Street::Flop, Street::Turn, Street::River] {
        let id = dense
            .tree
            .nodes
            .iter()
            .position(|node| node.street == street)
            .unwrap() as NodeId;
        let bucket = dense.arena.bucket_count_of(id) - 1;
        let column = dense.arena.column_id(id, bucket).unwrap();
        dense.arena.touched_set(column);
        selected.push(dense.info_key_for(id, bucket));
    }
    // A second bucket distinguishes within-node ordering from public-node ordering.
    let last = *selected.last().unwrap();
    let river = dense.tree.by_history[&last.history];
    assert!(dense.arena.bucket_count_of(river) > 1);
    dense
        .arena
        .touched_set(dense.arena.column_id(river, 0).unwrap());
    selected.push(dense.info_key_for(river, 0));
    selected.sort_unstable();
    let state = solver.snapshot_state();
    assert_eq!(
        state
            .policies
            .iter()
            .map(|entry| entry.key)
            .collect::<Vec<_>>(),
        selected
    );
    assert!(state.policies.iter().all(|entry| {
        entry.column.regrets.iter().all(|&value| value == 0.0)
            && entry.column.strategy_sum.iter().all(|&value| value == 0.0)
    }));
    assert!(
        state
            .histories
            .iter()
            .any(|entry| !selected.iter().any(|key| key.history == entry.key))
    );
    assert!(
        !state
            .histories
            .iter()
            .any(|entry| entry.key == HistoryKey::ROOT)
    );
    for &key in &selected {
        let mut ancestor = key.history;
        while ancestor != HistoryKey::ROOT {
            ancestor = state
                .histories
                .iter()
                .find(|entry| entry.key == ancestor)
                .unwrap()
                .parent;
        }
    }
    assert_snapshot_encoding(&solver);
    assert_checkpoint_encoding(&solver, "", CheckpointRuntimeState::default());
}

#[test]
fn borrowed_snapshot_sparse_preserves_all_history_and_full_recall_keys_in_sorted_order() {
    let mut sparse = four_street_solver(601);
    sparse.run_sweeps(8).unwrap();
    let original = sparse.snapshot_state();
    assert!(
        original
            .policies
            .iter()
            .any(|entry| entry.key.street == 3 && entry.key.bucket_path[0] != UNREACHED_BUCKET)
    );
    sparse.policies.clear();
    sparse.histories.clear();
    for entry in original.policies.iter().rev() {
        sparse.policies.insert(entry.key, entry.column.clone());
    }
    for entry in original.histories.iter().rev() {
        sparse.histories.insert(entry.key, entry.clone());
    }
    // A stored edge with no descendant policy must survive in sparse mode.
    let extra = HistoryEntry {
        key: HistoryKey::ROOT.child(0, 17),
        parent: HistoryKey::ROOT,
        actor: 0,
        action_index: 17,
        action_label: "history-only-é".into(),
    };
    assert!(!sparse.policies.keys().any(|key| key.history == extra.key));
    sparse.histories.insert(extra.key, extra.clone());
    let first = sparse.policies.values_mut().next().unwrap();
    first.regrets[0] = -0.0;
    first.strategy_sum[0] = f32::from_bits(1);
    assert_snapshot_encoding(&sparse);
    let json = serde_json::to_value(sparse.snapshot_state_ref().unwrap()).unwrap();
    assert_eq!(
        json["histories"].as_array().unwrap().len(),
        original.histories.len() + 1
    );
    assert!(sparse.snapshot_state().histories.contains(&extra));
}

#[test]
fn borrowed_snapshot_rejects_duplicate_dense_key_prefix_and_invalid_ancestor() {
    let mut solver = dense_toy_solver(12, 1);
    let dense = solver.dense.as_mut().unwrap();
    assert_eq!(dense.tree.nodes.len(), 3);
    for id in [1, 2] {
        dense
            .arena
            .touched_set(dense.arena.column_id(id, 0).unwrap());
    }
    dense.tree.nodes[2].history = dense.tree.nodes[1].history;
    assert!(matches!(
        solver.snapshot_state_ref(),
        Err(SnapshotError::InconsistentState(
            "duplicate dense information-key prefix"
        ))
    ));

    let mut solver = dense_toy_solver(12, 1);
    let dense = solver.dense.as_mut().unwrap();
    dense
        .arena
        .touched_set(dense.arena.column_id(1, 0).unwrap());
    dense.tree.nodes[1].parent = Some(1);
    assert!(matches!(
        solver.snapshot_state_ref(),
        Err(SnapshotError::InconsistentState("invalid dense ancestor"))
    ));
}

#[test]
fn borrowed_snapshot_rejects_touched_count_mismatch_before_serializing_a_length() {
    let mut solver = dense_toy_solver(12, 1);
    let dense = solver.dense.as_mut().unwrap();
    dense
        .arena
        .touched_set(dense.arena.column_id(2, 0).unwrap());
    // Simulate an inconsistent tree/arena pair: the stored touched count
    // remains one, while the retained nodes can no longer emit that column.
    dense.tree.nodes.pop();
    assert_eq!(dense.arena.touched_count(), 1);
    assert!(matches!(
        solver.snapshot_state_ref(),
        Err(SnapshotError::InconsistentState(
            "dense touched count mismatch"
        ))
    ));
}

#[test]
fn borrowed_snapshot_container_matches_across_compressed_chunk_boundary() {
    let mut solver = solver(981, 1 << 20);
    solver.run_sweeps(2).unwrap();
    // This is serialization evidence, not an adapter-valid menu for resume.
    // A long borrowed string crosses the writer's 4 MiB chunk boundary.
    solver.policies.get_mut(&root_key()).unwrap().action_labels[0] = "x".repeat((4 << 20) + 17);
    assert_checkpoint_encoding(&solver, "", CheckpointRuntimeState::default());
}

#[test]
fn borrowed_snapshot_atomic_overwrite_preserves_destination_on_snapshot_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.mwckpt");
    let mut solver = dense_toy_solver(319, 1);
    MultiwayCheckpoint::capture(&solver)
        .write_atomic(&path)
        .unwrap();
    let original_bytes = std::fs::read(&path).unwrap();

    solver.run_sweeps(4).unwrap();
    let dense = solver.dense.as_mut().unwrap();
    dense
        .arena
        .touched_set(dense.arena.column_id(2, 0).unwrap());
    MultiwayCheckpoint::write_solver_atomic(
        &solver,
        &path,
        "# replacement\n",
        CheckpointRuntimeState::default(),
    )
    .unwrap();
    let replaced_bytes = std::fs::read(&path).unwrap();
    assert_ne!(replaced_bytes, original_bytes);
    let loaded = MultiwayCheckpoint::load(
        &path,
        solver.configuration_fingerprint(),
        solver.abstraction_fingerprint(),
    )
    .unwrap();
    assert_eq!(loaded.state, solver.snapshot_state());
    assert_eq!(loaded.config_toml.as_deref(), Some("# replacement\n"));

    solver.dense.as_mut().unwrap().tree.nodes.pop();
    assert!(matches!(
        MultiwayCheckpoint::write_solver_atomic(
            &solver,
            &path,
            "# invalid\n",
            CheckpointRuntimeState::default(),
        ),
        Err(crate::checkpoint::CheckpointError::InvalidSnapshot(
            "dense touched count mismatch"
        ))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), replaced_bytes);
}
