use super::tests::{config, preflop_game, without_timings};
use super::*;

type TestGame = HoldemGame<crate::abstraction::FeatureHashAbstraction>;

fn solver() -> MultiwaySolver<TestGame> {
    let game = preflop_game(true);
    let sampler = game.deal_sampler().unwrap();
    MultiwaySolver::new(
        game,
        sampler,
        SolverConfig {
            max_memory_bytes: 1 << 27,
            max_traversal_depth: 64,
            ..SolverConfig::default()
        },
    )
    .unwrap()
}

fn action(solver: &MultiwaySolver<TestGame>, state: &BettingState, label: &str) -> usize {
    let menu = solver.game.node_actions(state);
    (0..solver.game.num_actions_of(&menu))
        .find(|&i| solver.game.action_label_of(&menu, i) == label)
        .unwrap()
}

fn pure_policy(solver: &mut MultiwaySolver<TestGame>, history: HistoryKey, label: &str) {
    let dense = solver.dense.as_mut().unwrap();
    let (id, node) = dense
        .tree
        .nodes
        .iter()
        .enumerate()
        .find(|(_, node)| node.history == history)
        .unwrap();
    let chosen = node.action_labels.iter().position(|v| v == label).unwrap();
    for bucket in 0..dense.arena.bucket_count_of(id as NodeId) {
        let slots = dense.arena.slot_range(id as NodeId, bucket).unwrap();
        dense.arena.strategy_sum[slots.clone()].fill(0.0);
        dense.arena.strategy_sum[slots.start + chosen] = 1.0;
        let column = dense.arena.column_id(id as NodeId, bucket).unwrap();
        dense.arena.touched_set(column);
    }
}

#[test]
fn counterfactual_endpoint_without_own_prefix_matches_legacy_and_threads_exactly() {
    let mut solver = solver();
    solver.run_sweeps(32).unwrap();
    let root = solver.game.root_state();
    let fold = action(&solver, &root, "fold");
    let before = solver.snapshot_state();
    for path in [vec![], vec![fold]] {
        for variant in [
            ProfileVariant::default(),
            ProfileVariant {
                use_current_strategy: true,
                purify_threshold: 0.1,
            },
        ] {
            let actual = without_timings(
                solver
                    .evaluate_endpoint_deviation_preflop(&path, variant, 1, &config())
                    .unwrap(),
            );
            for threads in [1, 2, 8] {
                let cf = solver
                    .evaluate_endpoint_deviation_preflop_counterfactual(
                        &path,
                        variant,
                        threads,
                        &config(),
                    )
                    .unwrap();
                assert_eq!(cf.target, "opponents-prefix");
                assert_eq!(cf.proposal_kind, "preflop-opponents-proposal");
                assert_eq!(cf.excluded_actor, actual.actor);
                assert_eq!(cf.validated_class_contexts, path.len() + 1);
                assert_eq!(cf.own_prefix_probability_by_bucket, vec![1.0; 169]);
                assert_eq!(without_timings(cf.evaluation), actual);
            }
        }
    }
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn counterfactual_zero_own_reach_evaluates_every_action_and_keeps_unsupported_weight() {
    let mut solver = solver();
    let mut state = solver.game.root_state();
    let mut history = HistoryKey::ROOT;
    let mut path = Vec::new();
    let mut own_prefix_history = None;
    for label in ["fold", "raise-to:2000", "raise-to:4000"] {
        let actor = solver.game.actor(&state).unwrap();
        if actor == 1 {
            own_prefix_history = Some(history);
        }
        let a = action(&solver, &state, label);
        let menu = solver.game.node_actions(&state);
        state = solver.game.next_state_with(&state, &menu, a);
        history = history.child(actor, a);
        path.push(a);
    }
    assert_eq!(solver.game.actor(&state), Some(1));
    // The endpoint actor always folds before its initial raise. Thus every
    // own hand has zero actual reach at the endpoint, while opponent reach
    // remains positive. The endpoint baseline itself also always folds.
    pure_policy(&mut solver, own_prefix_history.unwrap(), "fold");
    pure_policy(&mut solver, history, "fold");
    let jam = action(&solver, &state, "raise-to:100000:all-in");
    pure_policy(&mut solver, history.child(1, jam), "fold");
    let before = solver.snapshot_state();
    assert!(matches!(
        solver.evaluate_endpoint_deviation_preflop(&path, ProfileVariant::default(), 1, &config()),
        Err(SolverError::Sample(SampleError::EmptyRange { seat: 1 }))
    ));
    let (endpoint, trunk) = solver.endpoint_definition(&path).unwrap();
    let prepared = solver
        .prepare_counterfactual_preflop_proposal(trunk, ProfileVariant::default(), 1)
        .unwrap();
    assert_eq!(prepared.own_prefix_probability_by_bucket, vec![0.0; 169]);
    let fold = action(&solver, &state, "fold");
    for id in 0..16 {
        let sample = solver
            .endpoint_sample(
                &endpoint,
                &prepared.proposal,
                ProfileVariant::default(),
                981,
                id,
                None,
            )
            .unwrap();
        assert!(sample.weight > 0.0);
        assert_eq!(sample.terminal_replays, endpoint.labels.len() as u64 + 1);
        // Independent chip arithmetic: folding loses the actor's 2bb open;
        // jamming makes BB fold its 4bb, for a +6bb difference from baseline.
        assert_eq!(sample.baseline[1], -2.0);
        assert_eq!(sample.gains[fold], 0.0);
        assert_eq!(sample.gains[jam], 6.0);
    }
    let mut cfg = config();
    cfg.min_fit_ess = 1e9;
    let one = solver
        .evaluate_endpoint_deviation_preflop_counterfactual(
            &path,
            ProfileVariant::default(),
            1,
            &cfg,
        )
        .unwrap();
    assert_eq!(one.evaluation.fit.rows.len(), 169);
    assert_eq!(one.evaluation.fit.retained_buckets, 0);
    assert_eq!(
        one.evaluation.fit.sampling.terminal_replays,
        cfg.fit_samples * (endpoint.labels.len() as u64 + 1)
    );
    for held in &one.evaluation.held_out {
        assert_eq!(held.sampling.positive_weight_samples, cfg.held_out_samples);
        assert_eq!(held.sampling.terminal_replays, cfg.held_out_samples);
        assert!(held.sampling.relative_weight_mean.mean > 0.0);
        assert_eq!(held.retained_key_weight_fraction.unwrap().mean, 0.0);
        assert_eq!(held.gain.unwrap().mean, 0.0);
    }
    let parallel = solver
        .evaluate_endpoint_deviation_preflop_counterfactual(
            &path,
            ProfileVariant::default(),
            2,
            &cfg,
        )
        .unwrap();
    assert_eq!(
        without_timings(one.evaluation),
        without_timings(parallel.evaluation)
    );
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn counterfactual_endpoint_rejects_postflop_and_terminal_paths() {
    let solver = solver();
    let mut state = solver.game.root_state();
    let mut path = Vec::new();
    while state.street == Street::Preflop {
        let menu = solver.game.node_actions(&state);
        let a = (0..solver.game.num_actions_of(&menu))
            .find(|&a| {
                let label = solver.game.action_label_of(&menu, a);
                label == "check" || label.starts_with("call:")
            })
            .unwrap();
        state = solver.game.next_state_with(&state, &menu, a);
        path.push(a);
    }
    assert!(solver.game.actor(&state).is_some());
    assert!(matches!(
        solver.evaluate_endpoint_deviation_preflop_counterfactual(
            &path,
            ProfileVariant::default(),
            1,
            &config()
        ),
        Err(SolverError::InvalidState(
            "counterfactual endpoint requires a preflop decision"
        ))
    ));
    let mut state = solver.game.root_state();
    let mut folds = Vec::new();
    while solver.game.actor(&state).is_some() {
        let a = action(&solver, &state, "fold");
        let menu = solver.game.node_actions(&state);
        state = solver.game.next_state_with(&state, &menu, a);
        folds.push(a);
    }
    assert!(
        solver
            .evaluate_endpoint_deviation_preflop_counterfactual(
                &folds,
                ProfileVariant::default(),
                1,
                &config()
            )
            .is_err()
    );
}
