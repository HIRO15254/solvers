use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use cards::Range;
use rand::RngCore;

use super::*;

#[derive(Clone, Copy)]
struct State {
    stage: u8,
    flop: usize,
    turn: usize,
}

struct Game {
    recall: RecallMode,
    changed_menu: AtomicBool,
}

impl Game {
    fn street(state: &State) -> Street {
        match state.stage {
            0 => Street::Preflop,
            1 | 2 => Street::Flop,
            3 | 4 => Street::Turn,
            _ => Street::River,
        }
    }
}

impl ExternalSamplingGame for Game {
    type State = State;
    type Actions = State;

    fn num_players(&self) -> usize {
        2
    }
    fn root_state(&self) -> State {
        State {
            stage: 0,
            flop: 0,
            turn: 0,
        }
    }
    fn actor(&self, state: &State) -> Option<usize> {
        match state.stage {
            2 | 4 => Some(1),
            6 => None,
            _ => Some(0),
        }
    }
    fn node_actions(&self, state: &State) -> State {
        *state
    }
    fn num_actions_of(&self, actions: &State) -> usize {
        match actions.stage {
            0 => 3,
            6 => 0,
            _ => 2,
        }
    }
    fn next_state_with(&self, state: &State, _: &State, action: usize) -> State {
        let mut next = *state;
        next.stage = match state.stage {
            0 => [2, 1, 6][action],
            2 => {
                next.flop = action;
                3
            }
            3 if action == 0 => 4,
            4 => {
                next.turn = action;
                5
            }
            _ => 6,
        };
        next
    }
    fn write_action_label(&self, actions: &State, action: usize, out: &mut String) {
        if actions.stage == 2 && self.changed_menu.load(Ordering::Relaxed) {
            out.push_str(["bet-to:10", "check"][action]);
            return;
        }
        out.push_str(match actions.stage {
            0 => ["continue", "zero-reach", "stop"][action],
            2 | 5 => ["check", "bet-to:10"][action],
            4 => ["fold", "raise-to:30"][action],
            _ => ["continue", "stop"][action],
        });
    }
    fn bucket(&self, state: &State, world: &SampledWorld, actor: usize) -> PrivateInfo {
        assert_eq!(
            actor, 0,
            "opponent proposal must never inspect private buckets"
        );
        PrivateInfo::from_current_bucket(Self::street(state), 1, (world.hole_combo(0) % 2) as u32)
    }
    fn terminal_utilities(&self, _: &State, _: &SampledWorld, _: &mut [f64]) {
        panic!("an average-only walk never evaluates terminal utility");
    }
    fn recall_mode(&self) -> RecallMode {
        self.recall
    }
    fn bucket_count(&self, _: Street, _: u8) -> u32 {
        2
    }
    fn dense_node_context(&self, state: &State) -> DenseNodeContext {
        DenseNodeContext {
            street: Self::street(state),
            active_opponents: 1,
            bucket_active_opponents: 1,
        }
    }
    fn bucket_for_combo(
        &self,
        _: &State,
        _: &SampledWorld,
        actor: usize,
        combo: usize,
    ) -> BucketId {
        assert_eq!(
            actor, 0,
            "opponent proposal must never inspect private combos"
        );
        (combo % 2) as u32
    }
}

fn solver(recall: RecallMode) -> MultiwaySolver<Game> {
    MultiwaySolver::new(
        Game {
            recall,
            changed_menu: AtomicBool::new(false),
        },
        DealSampler::new(vec![Range::full(); 2]).unwrap(),
        SolverConfig {
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            ..SolverConfig::default()
        },
    )
    .unwrap()
}

fn worlds(solver: &MultiwaySolver<Game>) -> [SampledWorld; 2] {
    let mut by_bucket = [None, None];
    let mut rng = ChaCha20Rng::seed_from_u64(875);
    for _ in 0..128 {
        let world = solver.sampler.sample(&mut rng).unwrap();
        let bucket = world.hole_combo(0) % 2;
        by_bucket[bucket] = Some(world);
        if by_bucket.iter().all(Option::is_some) {
            break;
        }
    }
    by_bucket.map(Option::unwrap)
}

fn turn_history(flop: usize) -> HistoryKey {
    HistoryKey::ROOT.child(0, 0).child(1, flop)
}

fn river_history(flop: usize, turn: usize) -> HistoryKey {
    turn_history(flop).child(0, 0).child(1, turn)
}

// Independent, exact rational target strategies. No production normalization
// helper is used to form the expected update vectors below.
fn strategy(phase: usize, bucket: usize, street: Street) -> Vec<f64> {
    match street {
        Street::Preflop => {
            if phase == bucket {
                vec![0.25, 0.0, 0.75]
            } else {
                vec![0.75, 0.0, 0.25]
            }
        }
        Street::Turn => {
            if phase == bucket {
                vec![0.25, 0.75]
            } else {
                vec![0.75, 0.25]
            }
        }
        Street::River => match (phase, bucket) {
            (0, 0) | (1, 1) => vec![0.25, 0.75],
            (0, 1) => vec![0.75, 0.25],
            _ => vec![0.5, 0.5],
        },
        Street::Flop => vec![0.5, 0.5],
    }
}

fn set_phase(solver: &mut MultiwaySolver<Game>, phase: usize) {
    let dense = solver.dense.as_mut().unwrap();
    for (id, node) in dense.tree.nodes.iter().enumerate() {
        if node.actor != 0 {
            continue;
        }
        for bucket in 0..2 {
            let range = dense.arena.slot_range(id as NodeId, bucket).unwrap();
            dense.arena.regrets[range].copy_from_slice(
                &strategy(phase, bucket as usize, node.street)
                    .into_iter()
                    .map(|value| (4.0 * value) as f32)
                    .collect::<Vec<_>>(),
            );
        }
    }
}

type Updates = BTreeMap<u32, Vec<f64>>;

fn column(dense: &DenseStorage, history: HistoryKey, bucket: BucketId) -> u32 {
    dense
        .arena
        .column_id(dense.tree.by_history[&history], bucket)
        .unwrap()
}

fn add(updates: &mut Updates, column: u32, values: &[f64], factor: f64) {
    let target = updates
        .entry(column)
        .or_insert_with(|| vec![0.0; values.len()]);
    for (target, value) in target.iter_mut().zip(values) {
        *target += factor * value;
    }
}

fn walk(
    solver: &MultiwaySolver<Game>,
    world: &SampledWorld,
    vector: bool,
    combos: &[usize],
    linear_weight: f64,
    seed: u64,
) -> (Updates, u64) {
    let dense = solver.dense.as_ref().unwrap();
    let mut worker = DenseAverageStrategyWorker::with_sampling(
        &solver.game,
        dense,
        solver.config,
        linear_weight,
        AverageOpponentSampling::PostflopContinuation,
    );
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    if vector {
        worker
            .traverse_vector(
                solver.game.root_state(),
                0,
                world,
                0,
                combos,
                &[0.25, 0.75],
                &[1.0, 1.0],
                &mut rng,
                0,
            )
            .unwrap();
    } else {
        worker
            .traverse_scalar(solver.game.root_state(), 0, world, 0, 1.0, &mut rng, 0)
            .unwrap();
    }
    let mut updates = Updates::new();
    for event in worker.finish() {
        let DenseEvent::AddStrategy { column, values } = event else {
            panic!("not an average event")
        };
        add(&mut updates, column, &values, 1.0);
    }
    (updates, rng.next_u64())
}

fn representative_outcomes(
    solver: &MultiwaySolver<Game>,
    world: &SampledWorld,
    vector: bool,
    combos: &[usize],
    linear_weight: f64,
) -> BTreeMap<(usize, usize), Updates> {
    let dense = solver.dense.as_ref().unwrap();
    let bucket = if vector {
        0
    } else {
        (world.hole_combo(0) % 2) as u32
    };
    let mut outcomes = BTreeMap::new();
    for seed in 0..256 {
        let (updates, _) = walk(solver, world, vector, combos, linear_weight, seed);
        let path = (0..2)
            .flat_map(|flop| (0..2).map(move |turn| (flop, turn)))
            .find(|&(flop, turn)| {
                updates.contains_key(&column(dense, river_history(flop, turn), bucket))
            })
            .unwrap();
        if let Some(previous) = outcomes.insert(path, updates.clone()) {
            assert_eq!(previous, updates);
        }
        if outcomes.len() == 4 {
            break;
        }
    }
    assert_eq!(
        outcomes.len(),
        4,
        "every public branch has positive proposal support"
    );
    outcomes
}

#[test]
fn postflop_continuation_expected_accumulators_match_independent_finite_tree_oracle() {
    let mut solver = solver(RecallMode::Street);
    let worlds = worlds(&solver);
    let combos = [worlds[0].hole_combo(0), worlds[1].hole_combo(0)];
    let mut scalar_expected = Updates::new();
    let mut vector_expected = Updates::new();
    let mut target = Updates::new();
    let mut proposal_scalars = BTreeMap::new();
    for (phase, linear_weight) in [1.0, 3.0].into_iter().enumerate() {
        set_phase(&mut solver, phase);
        let dense = solver.dense.as_ref().unwrap();
        for (bucket, world_weight) in [0.25, 0.75].into_iter().enumerate() {
            let outcomes =
                representative_outcomes(&solver, &worlds[bucket], false, &combos, linear_weight);
            for ((flop, _), updates) in outcomes {
                // Flop: check has q=3/4; bet q=1/4. Turn's C is empty,
                // so both actions have q=1/2. This is a finite outcome sum,
                // not a Monte Carlo estimate from the seed search frequencies.
                let q = if flop == 0 { 0.75 } else { 0.25 } * 0.5;
                for (column, values) in updates {
                    add(&mut scalar_expected, column, &values, q * world_weight);
                }
            }
            let root_strategy = strategy(phase, bucket, Street::Preflop);
            let root = column(dense, HistoryKey::ROOT, bucket as u32);
            add(
                &mut target,
                root,
                &root_strategy,
                linear_weight * world_weight,
            );
            proposal_scalars.insert(root, 1.0);
            for flop in 0..2 {
                let q = if flop == 0 { 0.75 } else { 0.25 };
                let turn_strategy = strategy(phase, bucket, Street::Turn);
                let turn = column(dense, turn_history(flop), bucket as u32);
                let reach = root_strategy[0];
                add(
                    &mut target,
                    turn,
                    &turn_strategy,
                    linear_weight * world_weight * reach,
                );
                proposal_scalars.insert(turn, q);
                for turn_action in 0..2 {
                    let river = column(dense, river_history(flop, turn_action), bucket as u32);
                    add(
                        &mut target,
                        river,
                        &strategy(phase, bucket, Street::River),
                        linear_weight * world_weight * reach * turn_strategy[0],
                    );
                    proposal_scalars.insert(river, q * 0.5);
                }
            }
        }
        for ((flop, _), updates) in
            representative_outcomes(&solver, &worlds[0], true, &combos, linear_weight)
        {
            let q = if flop == 0 { 0.75 } else { 0.25 } * 0.5;
            for (column, values) in updates {
                add(&mut vector_expected, column, &values, q);
            }
        }
    }
    assert_eq!(
        scalar_expected.keys().collect::<Vec<_>>(),
        target.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        vector_expected.keys().collect::<Vec<_>>(),
        target.keys().collect::<Vec<_>>()
    );
    for (column, target) in target {
        for expected in [&scalar_expected[&column], &vector_expected[&column]] {
            let q = proposal_scalars[&column];
            for (actual, target) in expected.iter().zip(&target) {
                assert!((actual - q * target).abs() < 1e-12);
            }
            let actual_mass = expected.iter().sum::<f64>();
            let target_mass = target.iter().sum::<f64>();
            // Normalize expected accumulators, never E[finite-sample ratio].
            for (actual, target) in expected.iter().zip(&target) {
                assert!((actual / actual_mass - target / target_mass).abs() < 1e-12);
            }
        }
    }
    let dense = solver.dense.as_ref().unwrap();
    for bucket in 0..2 {
        assert!(!scalar_expected.contains_key(&column(
            dense,
            HistoryKey::ROOT.child(0, 1),
            bucket
        )));
        assert!(!vector_expected.contains_key(&column(
            dense,
            HistoryKey::ROOT.child(0, 1),
            bucket
        )));
    }
}

#[test]
fn postflop_continuation_preserves_uniform_fallback_draws_and_validates_menus() {
    for (street, labels) in [
        (Street::Preflop, vec!["fold", "call:10", "raise-to:30"]),
        (Street::Turn, vec!["fold", "raise-to:30"]),
    ] {
        let labels = labels.into_iter().map(str::to_string).collect::<Vec<_>>();
        for seed in 0..64 {
            let mut actual = ChaCha20Rng::seed_from_u64(seed);
            let mut expected = actual.clone();
            assert_eq!(
                postflop_continuation_action(street, &labels, &mut actual).unwrap(),
                expected.gen_range(0..labels.len())
            );
            assert_eq!(actual.next_u64(), expected.next_u64());
        }
    }
    let labels = ["fold", "check", "call:10", "raise-to:30"].map(str::to_string);
    let mut counts = [0_u32; 4];
    let mut rng = ChaCha20Rng::seed_from_u64(123456);
    for _ in 0..16384 {
        counts[postflop_continuation_action(Street::Flop, &labels, &mut rng).unwrap()] += 1;
    }
    for (count, expected) in counts.into_iter().zip([0.125, 0.375, 0.375, 0.125]) {
        assert!((count as f64 / 16384.0 - expected).abs() < 0.02);
    }
    for labels in [
        vec![],
        vec!["".into()],
        vec!["check".into(), "check".into()],
    ] {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let mut before = rng.clone();
        assert!(postflop_continuation_action(Street::Flop, &labels, &mut rng).is_err());
        assert_eq!(rng.next_u64(), before.next_u64());
    }
}

#[test]
fn postflop_continuation_is_card_independent_and_rejects_changed_public_menu() {
    let mut solver = solver(RecallMode::Street);
    set_phase(&mut solver, 0);
    let first = worlds(&solver)[0].clone();
    let mut rng = ChaCha20Rng::seed_from_u64(877);
    let second = (0..128)
        .map(|_| solver.sampler.sample(&mut rng).unwrap())
        .find(|world| world.hole_combo(0) % 2 == 0 && world.runout() != first.runout())
        .unwrap();
    // Both boards and opponent cards can change, but the same own bucket and
    // current strategy leave identical public proposal paths and draw usage.
    assert_ne!(first.hole_combo(1), second.hole_combo(1));
    for seed in 0..16 {
        assert_eq!(
            walk(&solver, &first, false, &[], 1.0, seed),
            walk(&solver, &second, false, &[], 1.0, seed)
        );
    }
    solver.game.changed_menu.store(true, Ordering::Relaxed);
    let dense = solver.dense.as_ref().unwrap();
    let mut worker = DenseAverageStrategyWorker::with_sampling(
        &solver.game,
        dense,
        solver.config,
        1.0,
        AverageOpponentSampling::PostflopContinuation,
    );
    assert!(matches!(
        worker.traverse_scalar(
            solver.game.root_state(),
            0,
            &first,
            0,
            1.0,
            &mut ChaCha20Rng::seed_from_u64(2),
            0
        ),
        Err(SolverError::InvalidActionLabels(_))
    ));
}

#[test]
fn postflop_continuation_sparse_entry_fails_before_rng_or_average_events() {
    let solver = solver(RecallMode::Full);
    let world = worlds(&solver)[0].clone();
    let mut worker = SparseAverageStrategyWorker::with_sampling(
        &solver,
        1.0,
        AverageOpponentSampling::PostflopContinuation,
    );
    let mut rng = ChaCha20Rng::seed_from_u64(3);
    let mut before = rng.clone();
    assert!(
        worker
            .traverse(
                solver.game.root_state(),
                &world,
                0,
                HistoryKey::ROOT,
                1.0,
                &mut rng,
                0
            )
            .is_err()
    );
    assert!(worker.finish().is_empty());
    assert_eq!(rng.next_u64(), before.next_u64());
}
