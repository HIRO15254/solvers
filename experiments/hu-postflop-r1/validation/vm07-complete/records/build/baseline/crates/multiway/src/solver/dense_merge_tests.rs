use super::*;

type TestSolver = MultiwaySolver<crate::HoldemGame<crate::abstraction::FeatureHashAbstraction>>;

fn fixture() -> TestSolver {
    let (game, sampler, mut config) = tests::initialization_holdem_fixture();
    config.traverser_vector = true;
    config.prune = true;
    config.prune_threshold = -10.0;
    config.discount_until = 0;
    MultiwaySolver::new(game, sampler, config).unwrap()
}

#[derive(Debug, PartialEq)]
struct CapturedState {
    state: SolverState,
    regret_bits: Vec<u32>,
    strategy_bits: Vec<u32>,
    touched: Vec<bool>,
    touched_count: u64,
}

fn capture(solver: &TestSolver) -> CapturedState {
    let arena = &solver.dense.as_ref().unwrap().arena;
    CapturedState {
        state: solver.snapshot_state(),
        regret_bits: arena.regrets.iter().map(|x| x.to_bits()).collect(),
        strategy_bits: arena.strategy_sum.iter().map(|x| x.to_bits()).collect(),
        touched: (0..arena.total_columns())
            .map(|column| arena.is_touched(column as u32))
            .collect(),
        touched_count: arena.touched_count(),
    }
}

fn deltas(solver: &TestSolver) -> Vec<DenseTraversalDelta> {
    (0..solver.game.num_players())
        .map(|seat| DenseTraversalDelta {
            sample_id: solver.next_sample_id + seat as u64,
            traverser: seat,
            deal_attempts: 1,
            terminal_evaluations: 2,
            hand_updates: 3,
            events: vec![],
        })
        .collect()
}

#[test]
fn dense_merge_late_slot_overflow_preserves_entire_state() {
    let mut solver = fixture();
    let dense = solver.dense.as_mut().unwrap();
    let width = dense.tree.nodes[0].action_labels.len();
    assert!(width >= 2);
    let touched = dense.arena.column_id(0, 0).unwrap();
    let untouched = dense.arena.column_id(0, 1).unwrap();
    let failing = dense.arena.column_id(0, 2).unwrap();
    let range = dense.arena.slot_range(0, 0).unwrap();
    dense.arena.regrets[range.start] = -0.0;
    dense.arena.touched_set(touched);
    let before = capture(&solver);
    let mut input = deltas(&solver);
    input[0].events.push(DenseEvent::AddRegret {
        column: touched,
        values: vec![-100.0; width],
    });
    input[0].events.push(DenseEvent::AddStrategy {
        column: untouched,
        values: vec![0.25; width],
    });
    let mut values = vec![0.75; width];
    values[width - 1] = 2.0 * f64::from(f32::MAX);
    input[2].events.push(DenseEvent::AddRegret {
        column: failing,
        values,
    });
    assert!(matches!(
        solver.merge_sweep_dense(input),
        Err(SolverError::NumericOverflow)
    ));
    assert_eq!(capture(&solver), before);
}

fn root_columns(solver: &TestSolver) -> (usize, [u32; 3]) {
    let dense = solver.dense.as_ref().unwrap();
    (
        dense.tree.nodes[0].action_labels.len(),
        [0, 1, 2].map(|bucket| dense.arena.column_id(0, bucket).unwrap()),
    )
}

fn seed_earlier_events(solver: &mut TestSolver) -> Vec<DenseTraversalDelta> {
    let (width, [touched, untouched, _]) = root_columns(solver);
    let dense = solver.dense.as_mut().unwrap();
    let range = dense.arena.slot_range(0, 0).unwrap();
    dense.arena.regrets[range.start] = -0.0;
    dense.arena.strategy_sum[range.start] = -0.0;
    dense.arena.touched_set(touched);
    let mut input = deltas(solver);
    input[0].events = vec![
        DenseEvent::AddRegret {
            column: touched,
            values: vec![-100.0; width],
        },
        DenseEvent::AddStrategy {
            column: touched,
            values: vec![0.5; width],
        },
        DenseEvent::AddStrategy {
            column: untouched,
            values: vec![0.25; width],
        },
    ];
    input[1].events = vec![
        DenseEvent::AddRegret {
            column: touched,
            values: vec![0.125; width],
        },
        DenseEvent::AddStrategy {
            column: untouched,
            values: vec![0.5; width],
        },
    ];
    input
}

#[test]
fn dense_merge_strategy_overflow_reverses_overlapping_events_and_floor() {
    let mut solver = fixture();
    let (width, [_, overlapping, _]) = root_columns(&solver);
    let mut input = seed_earlier_events(&mut solver);
    let before = capture(&solver);
    let mut values = vec![0.75; width];
    values[width - 1] = 2.0 * f64::from(f32::MAX);
    input[2].events.push(DenseEvent::AddStrategy {
        column: overlapping,
        values,
    });
    assert!(matches!(
        solver.merge_sweep_dense(input),
        Err(SolverError::NumericOverflow)
    ));
    assert!(
        capture(&solver) == before,
        "overlapping journal was not restored in reverse order"
    );
}

#[test]
fn dense_merge_late_invalid_column_or_shape_restores_prior_events() {
    for invalid_column in [false, true] {
        let mut solver = fixture();
        let (width, [_, _, column]) = root_columns(&solver);
        let mut input = seed_earlier_events(&mut solver);
        let before = capture(&solver);
        input[2].events.push(DenseEvent::AddRegret {
            column: if invalid_column { u32::MAX } else { column },
            values: vec![1.0; width + usize::from(!invalid_column)],
        });
        let error = solver.merge_sweep_dense(input).unwrap_err();
        if invalid_column {
            assert!(matches!(
                error,
                SolverError::Tree(TreeError::ColumnOutOfRange(_))
            ));
        } else {
            assert!(matches!(
                error,
                SolverError::Tree(TreeError::ActionCountMismatch)
            ));
        }
        assert!(
            capture(&solver) == before,
            "invalid event changed earlier columns"
        );
    }
}

#[test]
fn dense_merge_first_slot_or_first_event_failure_leaves_unprocessed_values_alone() {
    for early_event in [false, true] {
        for delta in [2.0 * f64::from(f32::MAX), f64::INFINITY, f64::NAN] {
            let mut solver = fixture();
            let (width, [_, _, failing]) = root_columns(&solver);
            let mut input = seed_earlier_events(&mut solver);
            if early_event {
                for seat in &mut input {
                    seat.events.clear();
                }
            }
            let before = capture(&solver);
            let mut values = vec![0.75; width];
            values[0] = delta;
            input[if early_event { 0 } else { 2 }]
                .events
                .push(DenseEvent::AddStrategy {
                    column: failing,
                    values,
                });
            assert!(matches!(
                solver.merge_sweep_dense(input),
                Err(SolverError::NumericOverflow)
            ));
            assert!(
                capture(&solver) == before,
                "unprocessed values were mistaken for journal entries"
            );
        }
    }
}

#[test]
fn dense_merge_preflights_late_seat_identity_and_all_counter_increments() {
    for defect in 0..8 {
        let mut solver = fixture();
        if defect == 5 {
            solver.traversals = u64::MAX - 2;
        }
        if defect == 6 {
            solver.next_sample_id = u64::MAX - 2;
        }
        if defect == 7 {
            solver.completed_sweeps = u64::MAX;
        }
        let mut input = seed_earlier_events(&mut solver);
        match defect {
            0 => input[2].traverser = 0,
            1 => input[2].sample_id += 1,
            2 => input[2].deal_attempts = u64::MAX,
            3 => input[2].terminal_evaluations = u64::MAX,
            4 => input[2].hand_updates = u64::MAX,
            _ => (),
        }
        let before = capture(&solver);
        let error = solver.merge_sweep_dense(input).unwrap_err();
        if defect < 2 {
            assert!(matches!(
                error,
                SolverError::InvalidState("parallel traversal deltas are not in sample-id order")
            ));
        } else {
            assert!(matches!(error, SolverError::CounterOverflow));
        }
        assert!(
            capture(&solver) == before,
            "counter/identity defect {defect} mutated state"
        );
    }
}

// Independent success-only arithmetic reference: seat/event/slot ordered
// f64 addition followed by f32 rounding, optional floor after every regret
// addition, touched bits, then counters and completed-sweep discount.
fn reference_successful_merge(solver: &mut TestSolver, input: Vec<DenseTraversalDelta>) {
    let arena = &mut solver.dense.as_mut().unwrap().arena;
    for delta in input {
        solver.total_deal_attempts += delta.deal_attempts;
        solver.terminal_evaluations += delta.terminal_evaluations;
        solver.hand_updates += delta.hand_updates;
        for event in delta.events {
            let (column, values, regret) = match event {
                DenseEvent::AddRegret { column, values } => (column, values, true),
                DenseEvent::AddStrategy { column, values } => (column, values, false),
            };
            let range = arena.slot_range_for_column(column, values.len()).unwrap();
            arena.touched_set(column);
            let targets = if regret {
                &mut arena.regrets[range]
            } else {
                &mut arena.strategy_sum[range]
            };
            for (target, delta) in targets.iter_mut().zip(values) {
                let value = f64::from(*target) + delta;
                assert!(value.is_finite() && value.abs() <= f64::from(f32::MAX));
                *target = value as f32;
                if regret && solver.config.prune {
                    *target = target.max((1.05 * solver.config.prune_threshold) as f32);
                }
            }
        }
    }
    solver.traversals += solver.game.num_players() as u64;
    solver.next_sample_id += solver.game.num_players() as u64;
    solver.completed_sweeps += 1;
    solver.apply_early_discount();
}

#[test]
fn dense_merge_success_matches_ordered_rounding_floor_discount_and_touched_reference() {
    let mut actual = fixture();
    let mut expected = fixture();
    for solver in [&mut actual, &mut expected] {
        solver.config.discount_every = 2;
        solver.config.discount_until = 5;
        let arena = &mut solver.dense.as_mut().unwrap().arena;
        let range = arena.slot_range(0, 0).unwrap();
        arena.regrets[range].fill(16_777_216.0);
        arena.touched_set(0);
    }
    for _ in 0..2 {
        let (width, [rounding, floored, strategy]) = root_columns(&actual);
        let mut input = deltas(&actual);
        input[0].events = vec![
            DenseEvent::AddRegret {
                column: rounding,
                values: vec![1.0; width],
            },
            DenseEvent::AddRegret {
                column: floored,
                values: vec![-100.0; width],
            },
            DenseEvent::AddStrategy {
                column: strategy,
                values: vec![0.1; width],
            },
        ];
        input[1].events = vec![
            DenseEvent::AddRegret {
                column: rounding,
                values: vec![-16_777_216.0; width],
            },
            DenseEvent::AddRegret {
                column: floored,
                values: vec![1.0; width],
            },
            DenseEvent::AddStrategy {
                column: strategy,
                values: vec![0.2; width],
            },
        ];
        // A zero event still deliberately makes this valid column observable.
        input[2].events.push(DenseEvent::AddStrategy {
            column: 3,
            values: vec![0.0; width],
        });
        reference_successful_merge(&mut expected, input.clone());
        actual.merge_sweep_dense(input).unwrap();
        assert!(
            capture(&actual) == capture(&expected),
            "successful arithmetic order changed"
        );
    }
}

#[test]
fn dense_merge_real_holdem_worker_deltas_match_success_reference() {
    let mut actual = fixture();
    let mut expected = fixture();
    for solver in [&mut actual, &mut expected] {
        solver.config.discount_every = 2;
        solver.config.discount_until = 5;
    }
    for _ in 0..4 {
        let input = (0..actual.game.num_players())
            .map(|seat| {
                match actual
                    .generate_traversal_delta(
                        actual.next_sample_id + seat as u64,
                        seat,
                        (actual.completed_sweeps + 1) as f64,
                    )
                    .unwrap()
                {
                    AnyTraversalDelta::Dense(delta) => delta,
                    AnyTraversalDelta::Sparse(_) => panic!("expected dense worker"),
                }
            })
            .collect::<Vec<_>>();
        reference_successful_merge(&mut expected, input.clone());
        actual.merge_sweep_dense(input).unwrap();
        assert!(
            capture(&actual) == capture(&expected),
            "real worker merge changed successful state"
        );
    }
}
