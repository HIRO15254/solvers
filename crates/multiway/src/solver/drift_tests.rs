use super::*;

fn compare_refresh<G: ExternalSamplingGame>(
    solver: &MultiwaySolver<G>,
    legacy: &mut std::collections::HashMap<InfoKey, Vec<f32>>,
    compact: &mut StrategyDriftTracker,
) -> Vec<f64> {
    let before = solver.snapshot_state();
    let expected = solver.strategy_drift_refresh(legacy);
    let actual = solver.strategy_drift_refresh_compact(compact).unwrap();
    assert_eq!(
        actual.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        expected.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
    );
    assert_eq!(compact.observed_columns(), legacy.len());
    assert_eq!(
        compact.observed_action_slots(),
        legacy.values().map(Vec::len).sum::<usize>()
    );
    assert_eq!(solver.snapshot_state(), before);
    actual
}

#[test]
fn compact_drift_matches_dense_growth_and_stable_refresh_bits() {
    for vector in [false, true] {
        let mut solver = if vector {
            dense_vector_toy_solver(913, 2)
        } else {
            dense_toy_solver(913, 2)
        };
        let mut legacy = std::collections::HashMap::new();
        let mut compact = StrategyDriftTracker::new();
        assert_eq!(
            compare_refresh(&solver, &mut legacy, &mut compact),
            [0.0; 2]
        );
        let mut moved = false;
        for sweeps in [2, 6, 18, 26] {
            solver.run_sweeps_with_threads(sweeps, 2).unwrap();
            moved |= compare_refresh(&solver, &mut legacy, &mut compact)
                .iter()
                .any(|&x| x > 0.0);
            let capacity = compact.retained_payload_bytes();
            assert_eq!(
                compare_refresh(&solver, &mut legacy, &mut compact),
                [0.0; 2]
            );
            assert_eq!(compact.retained_payload_bytes(), capacity);
        }
        assert!(moved, "the comparison must include a changing profile");
    }
}

#[test]
fn compact_drift_keeps_first_seen_zero_columns_and_fallbacks_across_streets() {
    let (game, sampler, config) = initialization_holdem_fixture();
    let mut solver = MultiwaySolver::new(game, sampler, config).unwrap();
    let dense = solver.dense.as_mut().unwrap();
    let mut selected = Vec::new();
    for street in [Street::Preflop, Street::Flop, Street::Turn, Street::River] {
        let id = dense
            .tree
            .nodes
            .iter()
            .position(|n| n.street == street)
            .unwrap() as NodeId;
        let bucket = dense.arena.bucket_count_of(id) - 1;
        assert!(bucket > 0);
        let column = dense.arena.column_id(id, bucket).unwrap();
        dense.arena.touched_set(column);
        selected.push((id, bucket));
    }
    let mut legacy = std::collections::HashMap::new();
    let mut compact = StrategyDriftTracker::new();
    assert!(
        compare_refresh(&solver, &mut legacy, &mut compact)
            .iter()
            .all(|&x| x == 0.0)
    );
    // Change stored columns from uniform regret matching to positive regret, and
    // insert earlier bucket IDs between previously observed columns.
    let dense = solver.dense.as_mut().unwrap();
    for &(id, bucket) in &selected {
        let range = dense.arena.slot_range(id, bucket).unwrap();
        dense.arena.regrets[range.clone()].fill(-3.0);
        dense.arena.regrets[range.start] = 9.0;
        let column = dense.arena.column_id(id, 0).unwrap();
        dense.arena.touched_set(column);
    }
    assert!(
        compare_refresh(&solver, &mut legacy, &mut compact)
            .iter()
            .any(|&x| x > 0.0)
    );
    assert_eq!(compact.observed_columns(), 8);
    // Average mass now overrides the positive-regret fallback in the same
    // touched layout, exercising the in-place path with nonzero drift.
    let dense = solver.dense.as_mut().unwrap();
    for &(id, bucket) in &selected {
        let range = dense.arena.slot_range(id, bucket).unwrap();
        dense.arena.strategy_sum[range.clone()].fill(0.0);
        dense.arena.strategy_sum[range.end - 1] = 7.0;
    }
    assert!(
        compare_refresh(&solver, &mut legacy, &mut compact)
            .iter()
            .any(|&x| x > 0.0)
    );
    assert!(
        compare_refresh(&solver, &mut legacy, &mut compact)
            .iter()
            .all(|&x| x == 0.0)
    );
}

#[test]
fn compact_drift_preserves_sparse_full_recall_refresh() {
    let mut solver = four_street_solver(914);
    let mut legacy = std::collections::HashMap::new();
    let mut compact = StrategyDriftTracker::new();
    compare_refresh(&solver, &mut legacy, &mut compact);
    for sweeps in [1, 5, 11] {
        solver.run_sweeps(sweeps).unwrap();
        compare_refresh(&solver, &mut legacy, &mut compact);
    }
    assert!(solver.dense.is_none());
    assert!(
        legacy
            .keys()
            .any(|k| k.street == 3 && k.bucket_path[0] != UNREACHED_BUCKET)
    );
    let keys: Vec<_> = solver.policies.keys().copied().collect();
    for key in keys {
        let column = solver.policies.get_mut(&key).unwrap();
        column.strategy_sum.fill(0.0);
        column.regrets.fill(-1.0);
    }
    compare_refresh(&solver, &mut legacy, &mut compact);
    compact.reset();
    legacy.clear();
    assert!(
        compare_refresh(&solver, &mut legacy, &mut compact)
            .iter()
            .all(|&x| x == 0.0)
    );
}

#[test]
fn compact_drift_resume_seeds_the_checkpoint_profile_before_training() {
    let mut original = dense_vector_toy_solver(915, 2);
    original.run_sweeps_with_threads(18, 2).unwrap();
    let mut resumed = MultiwaySolver::from_state(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        original.snapshot_state(),
    )
    .unwrap();
    let mut legacy = std::collections::HashMap::new();
    let mut compact = StrategyDriftTracker::new();
    assert_eq!(
        compare_refresh(&resumed, &mut legacy, &mut compact),
        [0.0; 2]
    );
    resumed.run_sweeps_with_threads(12, 1).unwrap();
    original.run_sweeps_with_threads(12, 2).unwrap();
    compare_refresh(&resumed, &mut legacy, &mut compact);
    assert_eq!(resumed.snapshot_state(), original.snapshot_state());
}

#[test]
fn compact_drift_rejects_different_configuration_until_explicit_reset() {
    let mut first = dense_toy_solver(916, 1);
    first.run_sweeps(4).unwrap();
    let mut other = dense_toy_solver(917, 1);
    other.run_sweeps(4).unwrap();
    let mut tracker = StrategyDriftTracker::new();
    first.strategy_drift_refresh_compact(&mut tracker).unwrap();
    let count = tracker.observed_columns();
    assert_eq!(
        other.strategy_drift_refresh_compact(&mut tracker),
        Err(StrategyDriftError::IncompatibleLayout)
    );
    assert_eq!(tracker.observed_columns(), count);
    assert_eq!(
        first.strategy_drift_refresh_compact(&mut tracker).unwrap(),
        [0.0; 2]
    );
    tracker.reset();
    assert_eq!(tracker.observed_columns(), 0);
    assert_eq!(
        other.strategy_drift_refresh_compact(&mut tracker).unwrap(),
        [0.0; 2]
    );
}
