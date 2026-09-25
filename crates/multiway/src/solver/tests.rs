use super::eval::*;
use super::support::*;
use super::workers::*;
use super::*;
use cards::Range;
use rand::RngCore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToyState {
    Choose,
    Terminal(usize),
}

#[derive(Clone, Copy)]
struct DominatedChoice;

#[derive(Clone, Copy)]
struct CoarseCandidateReferenceChoice;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FourStreetState {
    Decision(u8),
    Terminal,
}

#[derive(Clone, Copy)]
struct FourStreetGame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrefixState {
    Opponent,
    Hero,
    Terminal(usize),
}

#[derive(Clone, Copy)]
struct PrefixImportanceGame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThreePlayerOracleState {
    Opponent,
    Hero {
        opponent_action: usize,
    },
    Terminal {
        opponent_action: usize,
        hero_action: usize,
    },
}

#[derive(Clone, Copy)]
struct ThreePlayerOracleGame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AveragePathState {
    FirstOpponent,
    SecondOpponent { first: usize },
    Averager { first: usize, second: usize },
    Terminal,
}

#[derive(Clone, Copy)]
struct AveragePathGame;

const ORACLE_HERO_PAYOFFS: [[f64; 2]; 2] = [[4.0, 0.0], [-2.0, 2.0]];

impl ExternalSamplingGame for AveragePathGame {
    type State = AveragePathState;
    type Actions = AveragePathState;

    fn num_players(&self) -> usize {
        3
    }

    fn root_state(&self) -> Self::State {
        AveragePathState::FirstOpponent
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        match state {
            AveragePathState::FirstOpponent => Some(1),
            AveragePathState::SecondOpponent { .. } => Some(2),
            AveragePathState::Averager { .. } => Some(0),
            AveragePathState::Terminal => None,
        }
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(!matches!(actions, AveragePathState::Terminal)) * 2
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        match *actions {
            AveragePathState::FirstOpponent => AveragePathState::SecondOpponent {
                first: action_index,
            },
            AveragePathState::SecondOpponent { first } => AveragePathState::Averager {
                first,
                second: action_index,
            },
            AveragePathState::Averager { .. } => AveragePathState::Terminal,
            AveragePathState::Terminal => panic!("terminal state has no child"),
        }
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        let prefix = match actions {
            AveragePathState::FirstOpponent => "first",
            AveragePathState::SecondOpponent { .. } => "second",
            AveragePathState::Averager { .. } => "hero",
            AveragePathState::Terminal => panic!("terminal state has no actions"),
        };
        use std::fmt::Write;
        write!(out, "{prefix}-{action_index}").unwrap();
    }

    fn bucket(&self, _state: &Self::State, _world: &SampledWorld, _actor: usize) -> PrivateInfo {
        PrivateInfo {
            street: 0,
            active_opponents: 2,
            bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        }
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        assert_eq!(*state, AveragePathState::Terminal);
        utilities.fill(0.0);
    }
}

impl ExternalSamplingGame for FourStreetGame {
    type State = FourStreetState;
    type Actions = FourStreetState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        FourStreetState::Decision(0)
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        matches!(state, FourStreetState::Decision(_)).then_some(0)
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(matches!(actions, FourStreetState::Decision(_)))
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        assert_eq!(action_index, 0);
        match *actions {
            FourStreetState::Decision(street) if street < 3 => {
                FourStreetState::Decision(street + 1)
            }
            FourStreetState::Decision(3) => FourStreetState::Terminal,
            FourStreetState::Decision(_) => panic!("street out of range"),
            FourStreetState::Terminal => panic!("terminal state has no child"),
        }
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        assert!(matches!(actions, FourStreetState::Decision(_)));
        assert_eq!(action_index, 0);
        out.push_str("continue");
    }

    fn bucket(&self, state: &Self::State, _world: &SampledWorld, _actor: usize) -> PrivateInfo {
        let FourStreetState::Decision(street) = *state else {
            panic!("terminal state has no bucket")
        };
        PrivateInfo::from_path(
            Street::ALL[street as usize],
            1,
            BucketPath {
                preflop: 0,
                flop: 0,
                turn: 0,
                river: 0,
            },
        )
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        assert_eq!(*state, FourStreetState::Terminal);
        utilities.fill(0.0);
    }
}

impl ExternalSamplingGame for PrefixImportanceGame {
    type State = PrefixState;
    // Cheap for a toy game: the label/count logic only needs to know
    // which node kind it is, so the state itself doubles as the
    // precomputed action list.
    type Actions = PrefixState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        PrefixState::Opponent
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        match state {
            PrefixState::Opponent => Some(1),
            PrefixState::Hero => Some(0),
            PrefixState::Terminal(_) => None,
        }
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(!matches!(actions, PrefixState::Terminal(_))) * 2
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        match actions {
            PrefixState::Opponent => PrefixState::Hero,
            PrefixState::Hero => PrefixState::Terminal(action_index),
            PrefixState::Terminal(_) => panic!("terminal state has no child"),
        }
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        let labels = match actions {
            PrefixState::Opponent => ["left", "right"],
            PrefixState::Hero => ["win", "pass"],
            PrefixState::Terminal(_) => panic!("terminal state has no actions"),
        };
        out.push_str(labels[action_index]);
    }

    fn bucket(&self, _state: &Self::State, _world: &SampledWorld, _actor: usize) -> PrivateInfo {
        PrivateInfo {
            street: 0,
            active_opponents: 1,
            bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        }
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        let PrefixState::Terminal(action) = *state else {
            panic!("not terminal")
        };
        utilities[0] = f64::from(action == 0);
        utilities[1] = -utilities[0];
    }
}

impl ExternalSamplingGame for ThreePlayerOracleGame {
    type State = ThreePlayerOracleState;
    type Actions = ThreePlayerOracleState;

    fn num_players(&self) -> usize {
        3
    }

    fn root_state(&self) -> Self::State {
        ThreePlayerOracleState::Opponent
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        match state {
            ThreePlayerOracleState::Opponent => Some(1),
            ThreePlayerOracleState::Hero { .. } => Some(0),
            ThreePlayerOracleState::Terminal { .. } => None,
        }
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(!matches!(actions, ThreePlayerOracleState::Terminal { .. })) * 2
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        match *actions {
            ThreePlayerOracleState::Opponent => ThreePlayerOracleState::Hero {
                opponent_action: action_index,
            },
            ThreePlayerOracleState::Hero { opponent_action } => ThreePlayerOracleState::Terminal {
                opponent_action,
                hero_action: action_index,
            },
            ThreePlayerOracleState::Terminal { .. } => {
                panic!("terminal state has no child")
            }
        }
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        let labels = match actions {
            ThreePlayerOracleState::Opponent => ["left", "right"],
            ThreePlayerOracleState::Hero { .. } => ["take", "pass"],
            ThreePlayerOracleState::Terminal { .. } => {
                panic!("terminal state has no actions")
            }
        };
        out.push_str(labels[action_index]);
    }

    fn bucket(&self, _state: &Self::State, _world: &SampledWorld, _actor: usize) -> PrivateInfo {
        PrivateInfo {
            street: 0,
            active_opponents: 2,
            bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        }
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        let ThreePlayerOracleState::Terminal {
            opponent_action,
            hero_action,
        } = *state
        else {
            panic!("not terminal")
        };
        utilities[0] = ORACLE_HERO_PAYOFFS[opponent_action][hero_action];
        // Deliberately general-sum: neither opponent payoff is the
        // negative of the traverser's payoff.
        utilities[1] = [[1.0, 3.0], [5.0, -1.0]][opponent_action][hero_action];
        utilities[2] = [[2.0, -1.0], [1.0, 4.0]][opponent_action][hero_action];
    }
}

fn full_enumeration_oracle_regret(
    opponent_strategy: [f64; 2],
    hero_strategy: [f64; 2],
) -> [f64; 2] {
    let mut regrets = [0.0; 2];
    for opponent_action in 0..2 {
        let values = ORACLE_HERO_PAYOFFS[opponent_action];
        let node_value = hero_strategy
            .iter()
            .zip(values)
            .map(|(&probability, value)| probability * value)
            .sum::<f64>();
        for action in 0..2 {
            regrets[action] += opponent_strategy[opponent_action] * (values[action] - node_value);
        }
    }
    regrets
}

impl ExternalSamplingGame for DominatedChoice {
    type State = ToyState;
    type Actions = ToyState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        ToyState::Choose
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        matches!(state, ToyState::Choose).then_some(0)
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(matches!(actions, ToyState::Choose)) * 2
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        assert_eq!(*actions, ToyState::Choose);
        ToyState::Terminal(action_index)
    }

    fn write_action_label(&self, _actions: &Self::Actions, action_index: usize, out: &mut String) {
        let label = match action_index {
            0 => "best",
            1 => "dominated",
            _ => panic!("action out of range"),
        };
        out.push_str(label);
    }

    fn bucket(&self, _state: &Self::State, _world: &SampledWorld, _actor: usize) -> PrivateInfo {
        PrivateInfo {
            street: 0,
            active_opponents: 1,
            bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        }
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        let ToyState::Terminal(action) = *state else {
            panic!("not terminal")
        };
        utilities[0] = if action == 0 { 1.0 } else { -1.0 };
        utilities[1] = -utilities[0];
    }

    fn game_fingerprint(&self) -> [u8; 32] {
        *blake3::hash(b"dominated-choice-v1").as_bytes()
    }
}

impl ExternalSamplingGame for CoarseCandidateReferenceChoice {
    type State = ToyState;
    type Actions = ToyState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        ToyState::Choose
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        matches!(state, ToyState::Choose).then_some(0)
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(matches!(actions, ToyState::Choose)) * 2
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        assert_eq!(*actions, ToyState::Choose);
        ToyState::Terminal(action_index)
    }

    fn write_action_label(&self, _actions: &Self::Actions, action_index: usize, out: &mut String) {
        out.push_str(match action_index {
            0 => "best",
            1 => "dominated",
            _ => panic!("action out of range"),
        });
    }

    /// Deliberately coarse candidate abstraction: all deals share one key.
    fn bucket(&self, _state: &Self::State, _world: &SampledWorld, _actor: usize) -> PrivateInfo {
        PrivateInfo::from_path(
            Street::Preflop,
            1,
            BucketPath {
                preflop: 0,
                flop: 0,
                turn: 0,
                river: 0,
            },
        )
    }

    /// Common reference abstraction separates the sampled deals.
    fn deviation_bucket(
        &self,
        _state: &Self::State,
        world: &SampledWorld,
        actor: usize,
    ) -> PrivateInfo {
        PrivateInfo::from_path(
            Street::Preflop,
            1,
            BucketPath {
                preflop: (world.hole_combo(actor) % 2) as u32,
                flop: 0,
                turn: 0,
                river: 0,
            },
        )
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        let ToyState::Terminal(action) = *state else {
            panic!("not terminal")
        };
        utilities[0] = if action == 0 { 1.0 } else { -1.0 };
        utilities[1] = -utilities[0];
    }
}

fn solver(seed: u64, memory: u64) -> MultiwaySolver<DominatedChoice> {
    solver_with_batch(seed, memory, 1)
}

fn solver_with_batch(seed: u64, memory: u64, sweep_batch: u64) -> MultiwaySolver<DominatedChoice> {
    MultiwaySolver::new(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed,
            max_memory_bytes: memory,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: 5,
            discount_until: 100,
            sweep_batch,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap()
}

fn reference_choice_solver(seed: u64) -> MultiwaySolver<CoarseCandidateReferenceChoice> {
    MultiwaySolver::new(
        CoarseCandidateReferenceChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: 5,
            discount_until: 100,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap()
}

fn four_street_solver(seed: u64) -> MultiwaySolver<FourStreetGame> {
    MultiwaySolver::new(
        FourStreetGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: 5,
            discount_until: 100,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap()
}

fn root_key() -> InfoKey {
    InfoKey {
        history: HistoryKey::ROOT,
        player: 0,
        street: 0,
        active_opponents: 1,
        bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
    }
}

#[test]
fn dominated_action_disappears_from_current_and_average_policy() {
    let mut solver = solver(11, 1 << 20);
    solver.run_sweeps(100).unwrap();
    assert!(solver.current_strategy(root_key()).unwrap()[0] > 0.99);
    assert!(solver.average_strategy(root_key()).unwrap()[0] > 0.95);
    assert_eq!(
        solver.average_action_probabilities(root_key()).unwrap()[0].action,
        "best"
    );
    let metrics = solver.metrics();
    assert_eq!(metrics.sweeps, 100);
    assert_eq!(metrics.traversals, 200);
    assert_eq!(metrics.average_positive_regret.len(), 2);
    assert!(metrics.infosets >= 1);
}

#[test]
fn batched_discount_scales_regret_and_strategy_at_cadence() {
    let mut solver = solver(1, 1 << 20);
    solver.config.discount_every = 2;
    solver.config.discount_until = 5;
    solver.run_sweeps(1).unwrap();
    assert_eq!(solver.policy(root_key()).unwrap().regrets, vec![1.0, -1.0]);
    solver.run_sweeps(1).unwrap();
    let column = solver.policy(root_key()).unwrap();
    assert_eq!(column.regrets, vec![0.5, -1.5]);
    assert_eq!(column.strategy_sum, vec![1.25, 0.25]);
}

#[test]
fn held_out_profile_evaluation_is_deterministic_and_read_only() {
    let mut solver = solver(91, 1 << 20);
    solver.run_sweeps(25).unwrap();
    let before = solver.snapshot_state();
    let a = solver.evaluate_average_profile(128, 2026).unwrap();
    let b = solver.evaluate_average_profile(128, 2026).unwrap();
    assert_eq!(a, b);
    assert_eq!(solver.snapshot_state(), before);
    assert_eq!(a.samples, 128);
    assert_eq!(a.seats.len(), 2);
    // This single-decision game visits seat 0 exactly once per baseline
    // sample. Regret-greedy deviation replays must not inflate coverage.
    assert_eq!(a.candidate_policy_coverage.len(), 2);
    assert_eq!(a.candidate_policy_coverage[0].decision_visits, 128);
    assert_eq!(a.candidate_policy_coverage[0].average_strategy_visits, 128);
    assert_eq!(a.candidate_policy_coverage[0].regret_fallback_visits, 0);
    assert_eq!(
        a.candidate_policy_coverage[1],
        CandidatePolicyCoverage::default()
    );
    assert!(a.seats[0].mean > 0.8);
    let gains = a.deviation_gain_lower_bound.as_ref().unwrap();
    assert_eq!(gains.len(), 2);
    assert!(
        gains
            .iter()
            .all(|gain| gain.mean >= 0.0 && gain.ci95[0] >= 0.0)
    );
    assert_eq!(gains[1].mean, 0.0);
    assert_eq!(gains[1].stderr, 0.0);
    assert!(
        a.seats
            .iter()
            .all(|seat| seat.ci95[0] <= seat.mean && seat.mean <= seat.ci95[1])
    );
}

#[test]
fn held_out_profile_parallelism_is_bit_identical() {
    let mut solver = prefix_solver(92);
    solver.run_sweeps(5).unwrap();
    let deviators = (0..2)
        .map(|seat| {
            solver
                .train_deviator(seat, 64, 3030 + seat as u64, ProfileVariant::default())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let before = solver.snapshot_state();

    for samples in [2, 257, 4097] {
        for trained in [None, Some(deviators.as_slice())] {
            let serial = solver
                .evaluate_profile_with_threads(samples, 4040, trained, ProfileVariant::default(), 1)
                .unwrap();
            let two_threads = solver
                .evaluate_profile_with_threads(samples, 4040, trained, ProfileVariant::default(), 2)
                .unwrap();
            let eight_threads = solver
                .evaluate_profile_with_threads(samples, 4040, trained, ProfileVariant::default(), 8)
                .unwrap();
            let repeated = solver
                .evaluate_profile_with_threads(samples, 4040, trained, ProfileVariant::default(), 8)
                .unwrap();

            assert_eq!(two_threads, serial);
            assert_eq!(eight_threads, serial);
            assert_eq!(repeated, serial);
        }
    }
    assert_eq!(solver.snapshot_state(), before);
    assert!(matches!(
        solver.evaluate_profile_with_threads(2, 1, None, ProfileVariant::default(), 0),
        Err(SolverError::ZeroThreads)
    ));
}

fn prefix_solver(seed: u64) -> MultiwaySolver<PrefixImportanceGame> {
    MultiwaySolver::new(
        PrefixImportanceGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: 5,
            discount_until: 100,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap()
}

#[test]
fn train_deviator_is_deterministic() {
    let mut solver = prefix_solver(41);
    solver.run_sweeps(3).unwrap();
    let before = solver.snapshot_state();
    let first = solver
        .train_deviator(0, 300, 555, ProfileVariant::default())
        .unwrap();
    let reported = solver
        .train_deviator_with_report(0, 300, 555, ProfileVariant::default())
        .unwrap();
    assert_eq!(first, reported.policy);
    assert_eq!(first.seat, 0);
    assert_eq!(reported.coverage.traversals, 300);
    assert_eq!(solver.snapshot_state(), before);
    // A different seat/seed/traversal count must not accidentally
    // collide with the same trained policy.
    let other_seat = solver
        .train_deviator(1, 300, 555, ProfileVariant::default())
        .unwrap();
    assert_eq!(other_seat.seat, 1);
}

#[test]
fn deviator_training_uses_reference_keys_over_a_coarse_candidate() {
    let solver = reference_choice_solver(71);
    let trained = solver
        .train_deviator_with_report(0, 256, 991, ProfileVariant::default())
        .unwrap();
    let mut buckets = trained
        .policy
        .actions
        .keys()
        .map(|key| key.bucket_path[0])
        .collect::<Vec<_>>();
    buckets.sort_unstable();
    buckets.dedup();
    assert_eq!(buckets, vec![0, 1]);
    assert_eq!(trained.coverage.visited_infosets, 2);
    assert_eq!(trained.coverage.retained_infosets, 2);
    assert_eq!(trained.coverage.total_visits, 256);
    assert_eq!(trained.coverage.retained_visits, 256);
}

#[test]
fn reference_evaluation_falls_back_to_candidate_baseline_not_greedy() {
    let mut solver = reference_choice_solver(81);
    solver.run_sweeps(1).unwrap();
    let candidate = solver
        .policies
        .get_mut(&root_key())
        .expect("one sweep creates the candidate root policy");
    // Candidate baseline is pure action 1, while candidate regret-greedy is
    // pure action 0. This makes the required fallback distinction exact.
    candidate.strategy_sum = vec![0.0, 1.0];
    candidate.regrets = vec![1.0, 0.0];
    let deviators = vec![
        DeviatorPolicy {
            seat: 0,
            actions: FxHashMap::default(),
        },
        DeviatorPolicy {
            seat: 1,
            actions: FxHashMap::default(),
        },
    ];

    let legacy = solver
        .evaluate_profile(32, 1234, Some(&deviators), ProfileVariant::default())
        .unwrap();
    assert_eq!(
        legacy.deviation_gain_lower_bound.as_ref().unwrap()[0].mean,
        2.0
    );

    let reference = solver
        .evaluate_reference_deviators(32, 1234, &deviators, ProfileVariant::default())
        .unwrap();
    assert_eq!(reference.evaluation.seats[0].mean, -1.0);
    assert_eq!(
        reference.candidate_policy_coverage[0],
        CandidatePolicyCoverage {
            decision_visits: 32,
            stored_strategy_visits: 32,
            uniform_fallback_visits: 0,
            decision_visits_by_street: StreetVisitCounts {
                preflop: 32,
                ..StreetVisitCounts::default()
            },
            stored_strategy_visits_by_street: StreetVisitCounts {
                preflop: 32,
                ..StreetVisitCounts::default()
            },
            uniform_fallback_visits_by_street: StreetVisitCounts::default(),
            average_strategy_visits: 32,
            average_strategy_visits_by_street: StreetVisitCounts {
                preflop: 32,
                ..StreetVisitCounts::default()
            },
            ..CandidatePolicyCoverage::default()
        }
    );
    assert_eq!(
        reference.candidate_policy_coverage[0].stored_strategy_fraction(),
        1.0
    );
    assert_eq!(
        reference
            .evaluation
            .deviation_gain_lower_bound
            .as_ref()
            .unwrap()[0]
            .mean,
        0.0
    );
    assert_eq!(
        reference.coverage[0],
        ReferenceDeviationCoverage {
            decision_visits: 32,
            trained_action_visits: 0,
            baseline_fallback_visits: 32,
            decision_visits_by_street: StreetVisitCounts {
                preflop: 32,
                ..StreetVisitCounts::default()
            },
            trained_action_visits_by_street: StreetVisitCounts::default(),
            baseline_fallback_visits_by_street: StreetVisitCounts {
                preflop: 32,
                ..StreetVisitCounts::default()
            },
        }
    );
    assert_eq!(reference.coverage[0].trained_action_fraction(), 0.0);
    assert!(
        reference
            .worlds
            .iter()
            .all(|world| world.gains == vec![0.0, 0.0])
    );
}

#[test]
fn reference_evaluation_empty_policies_are_exactly_paired_with_baseline() {
    let mut solver = prefix_solver(83);
    solver.run_sweeps(3).unwrap();
    let deviators = vec![
        DeviatorPolicy {
            seat: 0,
            actions: FxHashMap::default(),
        },
        DeviatorPolicy {
            seat: 1,
            actions: FxHashMap::default(),
        },
    ];

    let result = solver
        .evaluate_reference_deviators(128, 4321, &deviators, ProfileVariant::default())
        .unwrap();

    for world in &result.worlds {
        assert_eq!(world.deviating_seat_utilities, world.baseline_utilities);
        assert_eq!(world.gains, vec![0.0; 2]);
    }
    for gain in result
        .evaluation
        .deviation_gain_lower_bound
        .as_ref()
        .unwrap()
    {
        assert_eq!(gain.mean, 0.0);
        assert_eq!(gain.stderr, 0.0);
        assert_eq!(gain.ci95, [0.0, 0.0]);
    }
}

#[test]
fn reference_evaluation_reports_candidate_uniform_fallback_coverage() {
    let solver = reference_choice_solver(810);
    let deviators = vec![
        DeviatorPolicy {
            seat: 0,
            actions: FxHashMap::default(),
        },
        DeviatorPolicy {
            seat: 1,
            actions: FxHashMap::default(),
        },
    ];
    let reference = solver
        .evaluate_reference_deviators(16, 9876, &deviators, ProfileVariant::default())
        .unwrap();
    assert_eq!(
        reference.candidate_policy_coverage[0],
        CandidatePolicyCoverage {
            decision_visits: 16,
            stored_strategy_visits: 0,
            uniform_fallback_visits: 16,
            decision_visits_by_street: StreetVisitCounts {
                preflop: 16,
                ..StreetVisitCounts::default()
            },
            stored_strategy_visits_by_street: StreetVisitCounts::default(),
            uniform_fallback_visits_by_street: StreetVisitCounts {
                preflop: 16,
                ..StreetVisitCounts::default()
            },
            ..CandidatePolicyCoverage::default()
        }
    );
    assert_eq!(
        reference.candidate_policy_coverage[0].stored_strategy_fraction(),
        0.0
    );
}

#[test]
fn reference_evaluation_looks_up_trained_actions_by_reference_key() {
    let mut solver = reference_choice_solver(82);
    solver.run_sweeps(1).unwrap();
    let candidate = solver
        .policies
        .get_mut(&root_key())
        .expect("one sweep creates the candidate root policy");
    candidate.strategy_sum = vec![0.0, 1.0];
    candidate.regrets = vec![1.0, 0.0];

    let mut reference_one = root_key();
    reference_one.bucket_path[0] = 1;
    let deviators = vec![
        DeviatorPolicy {
            seat: 0,
            actions: FxHashMap::from_iter([(reference_one, 0)]),
        },
        DeviatorPolicy {
            seat: 1,
            actions: FxHashMap::default(),
        },
    ];
    let reference = solver
        .evaluate_reference_deviators(64, 4321, &deviators, ProfileVariant::default())
        .unwrap();
    let coverage = reference.coverage[0];
    assert_eq!(coverage.decision_visits, 64);
    assert!(coverage.trained_action_visits > 0);
    assert!(coverage.baseline_fallback_visits > 0);
    assert_eq!(
        coverage.trained_action_visits + coverage.baseline_fallback_visits,
        coverage.decision_visits
    );
    assert_eq!(
        reference
            .worlds
            .iter()
            .filter(|world| world.gains[0] == 2.0)
            .count() as u64,
        coverage.trained_action_visits
    );
}

#[test]
fn reference_evaluation_attributes_coverage_to_each_street() {
    let solver = four_street_solver(83);
    let hero = solver
        .train_deviator_with_report(0, 8, 7654, ProfileVariant::default())
        .unwrap();
    let opponent = solver
        .train_deviator_with_report(1, 8, 7655, ProfileVariant::default())
        .unwrap();
    assert_eq!(hero.policy.actions.len(), Street::ALL.len());
    assert!(opponent.policy.actions.is_empty());

    let samples = 5;
    let reference = solver
        .evaluate_reference_deviators(
            samples,
            4322,
            &[hero.policy, opponent.policy],
            ProfileVariant::default(),
        )
        .unwrap();
    let every_street = StreetVisitCounts {
        preflop: samples,
        flop: samples,
        turn: samples,
        river: samples,
    };

    let candidate = reference.candidate_policy_coverage[0];
    assert_eq!(candidate.decision_visits, 4 * samples);
    assert_eq!(candidate.uniform_fallback_visits, 4 * samples);
    assert_eq!(candidate.stored_strategy_visits, 0);
    assert_eq!(candidate.decision_visits_by_street, every_street);
    assert_eq!(candidate.uniform_fallback_visits_by_street, every_street);
    assert_eq!(
        candidate.stored_strategy_visits_by_street,
        StreetVisitCounts::default()
    );
    assert_eq!(
        candidate.decision_visits_by_street.total(),
        candidate.decision_visits
    );
    assert_eq!(
        candidate.uniform_fallback_visits_by_street.total(),
        candidate.uniform_fallback_visits
    );
    assert_eq!(
        candidate.stored_strategy_visits_by_street.total(),
        candidate.stored_strategy_visits
    );
    assert_eq!(
        candidate.decision_visits,
        candidate.stored_strategy_visits + candidate.uniform_fallback_visits
    );

    let deviator = reference.coverage[0];
    assert_eq!(deviator.decision_visits, 4 * samples);
    assert_eq!(deviator.trained_action_visits, 4 * samples);
    assert_eq!(deviator.baseline_fallback_visits, 0);
    assert_eq!(deviator.decision_visits_by_street, every_street);
    assert_eq!(deviator.trained_action_visits_by_street, every_street);
    assert_eq!(
        deviator.baseline_fallback_visits_by_street,
        StreetVisitCounts::default()
    );
    assert_eq!(
        deviator.decision_visits_by_street.total(),
        deviator.decision_visits
    );
    assert_eq!(
        deviator.trained_action_visits_by_street.total(),
        deviator.trained_action_visits
    );
    assert_eq!(
        deviator.baseline_fallback_visits_by_street.total(),
        deviator.baseline_fallback_visits
    );
    assert_eq!(
        deviator.decision_visits,
        deviator.trained_action_visits + deviator.baseline_fallback_visits
    );

    assert_eq!(
        reference.candidate_policy_coverage[1],
        CandidatePolicyCoverage::default()
    );
    assert_eq!(reference.coverage[1], ReferenceDeviationCoverage::default());

    let json = serde_json::to_value(&reference).unwrap();
    assert_eq!(
        json["candidate_policy_coverage"][0]["decision_visits_by_street"]["flop"],
        samples
    );
    assert_eq!(
        json["coverage"][0]["trained_action_visits_by_street"]["river"],
        samples
    );
}

#[test]
fn reference_evaluation_distinguishes_average_mass_and_regret_fallback_by_street() {
    let mut solver = four_street_solver(831);
    solver.run_sweeps(1).unwrap();
    assert_eq!(solver.policies.len(), 4);
    for (key, column) in &mut solver.policies {
        // The one-action policies are identical at every street, but only
        // preflop and river have an observed average. A storage-only counter
        // would conceal the missing flop and turn average samples.
        column
            .strategy_sum
            .fill(if key.street == 0 || key.street == 3 {
                1.0
            } else {
                0.0
            });
    }
    let deviators = (0..2)
        .map(|seat| DeviatorPolicy {
            seat,
            actions: FxHashMap::default(),
        })
        .collect::<Vec<_>>();
    let before = solver.snapshot_state();
    let samples = 7;
    let average = solver
        .evaluate_reference_deviators(samples, 4323, &deviators, ProfileVariant::default())
        .unwrap();
    let coverage = average.candidate_policy_coverage[0];
    assert_eq!(coverage.decision_visits, 4 * samples);
    assert_eq!(coverage.stored_strategy_visits, 4 * samples);
    assert_eq!(coverage.average_strategy_visits, 2 * samples);
    assert_eq!(coverage.regret_fallback_visits, 2 * samples);
    assert_eq!(coverage.current_strategy_visits, 0);
    assert_eq!(coverage.uniform_fallback_visits, 0);
    assert_eq!(coverage.stored_strategy_fraction(), 1.0);
    assert_eq!(coverage.average_strategy_fraction(), 0.5);
    assert_eq!(
        coverage.average_strategy_visits_by_street,
        StreetVisitCounts {
            preflop: samples,
            river: samples,
            ..StreetVisitCounts::default()
        }
    );
    assert_eq!(
        coverage.regret_fallback_visits_by_street,
        StreetVisitCounts {
            flop: samples,
            turn: samples,
            ..StreetVisitCounts::default()
        }
    );
    assert_eq!(
        coverage.stored_strategy_visits,
        coverage.average_strategy_visits
            + coverage.regret_fallback_visits
            + coverage.current_strategy_visits
    );
    let current = solver
        .evaluate_reference_deviators(
            samples,
            4323,
            &deviators,
            ProfileVariant {
                use_current_strategy: true,
                ..ProfileVariant::default()
            },
        )
        .unwrap();
    let current_coverage = current.candidate_policy_coverage[0];
    assert_eq!(current_coverage.current_strategy_visits, 4 * samples);
    assert_eq!(current_coverage.average_strategy_visits, 0);
    assert_eq!(current_coverage.regret_fallback_visits, 0);
    assert_eq!(
        current_coverage.current_strategy_visits_by_street,
        coverage.decision_visits_by_street
    );
    // Coverage bookkeeping consumes no random draws and changes no utilities
    // or mutable solver state.
    assert_eq!(current.worlds, average.worlds);
    assert_eq!(current.evaluation.seats, average.evaluation.seats);
    assert_eq!(
        current.evaluation.deviation_gain_lower_bound,
        average.evaluation.deviation_gain_lower_bound
    );
    assert_eq!(
        average.evaluation.candidate_policy_coverage,
        average.candidate_policy_coverage
    );
    assert_eq!(
        current.evaluation.candidate_policy_coverage,
        current.candidate_policy_coverage
    );
    assert_eq!(solver.snapshot_state(), before);
    let json = serde_json::to_value(&average).unwrap();
    assert_eq!(
        json["candidate_policy_coverage"][0]["regret_fallback_visits_by_street"]["turn"],
        samples
    );
}

#[test]
fn reference_evaluation_dense_zero_average_mass_is_regret_fallback() {
    let mut solver = dense_dominated_solver(832);
    solver.run_sweeps(1).unwrap();
    solver.dense.as_mut().unwrap().arena.strategy_sum.fill(0.0);
    let deviators = (0..2)
        .map(|seat| DeviatorPolicy {
            seat,
            actions: FxHashMap::default(),
        })
        .collect::<Vec<_>>();
    let before = solver.snapshot_state();
    let reference = solver
        .evaluate_reference_deviators(16, 4324, &deviators, ProfileVariant::default())
        .unwrap();
    let coverage = reference.candidate_policy_coverage[0];
    assert_eq!(coverage.decision_visits, 16);
    assert_eq!(coverage.stored_strategy_visits, 16);
    assert_eq!(coverage.regret_fallback_visits, 16);
    assert_eq!(coverage.average_strategy_visits, 0);
    assert_eq!(coverage.uniform_fallback_visits, 0);
    assert_eq!(coverage.average_strategy_fraction(), 0.0);
    let ordinary = solver.evaluate_average_profile(16, 4324).unwrap();
    assert_eq!(reference.evaluation.seats, ordinary.seats);
    assert_eq!(
        reference.candidate_policy_coverage,
        ordinary.candidate_policy_coverage
    );
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn coverage_without_street_counters_deserializes_with_zero_defaults() {
    let evaluation: ProfileEvaluation = serde_json::from_value(serde_json::json!({
        "samples": 3,
        "total_deal_attempts": 3,
        "seats": [],
        "deviation_gain_lower_bound": null
    }))
    .unwrap();
    assert!(evaluation.candidate_policy_coverage.is_empty());
    let candidate: CandidatePolicyCoverage = serde_json::from_value(serde_json::json!({
        "decision_visits": 3,
        "stored_strategy_visits": 2,
        "uniform_fallback_visits": 1
    }))
    .unwrap();
    assert_eq!(candidate.decision_visits, 3);
    assert_eq!(candidate.average_strategy_visits, 0);
    assert_eq!(candidate.current_strategy_visits, 0);
    assert_eq!(candidate.regret_fallback_visits, 0);
    assert_eq!(
        candidate.average_strategy_visits_by_street,
        StreetVisitCounts::default()
    );
    assert_eq!(
        candidate.current_strategy_visits_by_street,
        StreetVisitCounts::default()
    );
    assert_eq!(
        candidate.regret_fallback_visits_by_street,
        StreetVisitCounts::default()
    );
    assert_eq!(
        candidate.decision_visits_by_street,
        StreetVisitCounts::default()
    );

    let reference: ReferenceDeviationCoverage = serde_json::from_value(serde_json::json!({
        "decision_visits": 3,
        "trained_action_visits": 1,
        "baseline_fallback_visits": 2
    }))
    .unwrap();
    assert_eq!(reference.decision_visits, 3);
    assert_eq!(
        reference.trained_action_visits_by_street,
        StreetVisitCounts::default()
    );
}

#[test]
fn prefix_coverage_counts_only_decisions_at_and_below_each_street() {
    let mut solver = four_street_solver(840);
    solver.run_sweeps(1).unwrap();
    solver.policies.retain(|key, _| key.street != 1);
    for (key, column) in &mut solver.policies {
        column
            .strategy_sum
            .fill(if key.street == 2 { 0.0 } else { 1.0 });
    }
    let root = HistoryKey::ROOT;
    let flop = root.child(0, 0);
    let turn = flop.child(0, 0);
    let river = turn.child(0, 0);
    let before = solver.snapshot_state();
    let samples = 17;
    let result = solver
        .evaluate_profile_with_prefixes(
            samples,
            551,
            None,
            ProfileVariant::default(),
            2,
            &[root, flop, turn, river],
        )
        .unwrap();
    assert_eq!(
        result.evaluation,
        solver.evaluate_average_profile(samples, 551).unwrap()
    );
    assert_eq!(
        result.prefixes[0].candidate_policy_coverage,
        result.evaluation.candidate_policy_coverage
    );
    for (start, prefix) in result.prefixes.iter().enumerate() {
        assert_eq!(prefix.reached_samples, samples);
        let coverage = prefix.candidate_policy_coverage[0];
        assert_eq!(coverage.decision_visits, (4 - start) as u64 * samples);
        assert_eq!(
            coverage.uniform_fallback_visits,
            if start <= 1 { samples } else { 0 }
        );
        assert_eq!(
            coverage.regret_fallback_visits,
            if start <= 2 { samples } else { 0 }
        );
        for (street_index, street) in Street::ALL.into_iter().enumerate() {
            assert_eq!(
                prefix.trajectory_visits_by_street.get(street),
                if street_index >= start { samples } else { 0 }
            );
        }
        assert_eq!(
            prefix.candidate_policy_coverage[1],
            CandidatePolicyCoverage::default()
        );
    }
    let mut expected_baseline = result.evaluation;
    expected_baseline.deviation_gain_lower_bound = None;
    let baseline = solver
        .evaluate_profile_coverage(
            samples,
            551,
            ProfileVariant::default(),
            2,
            &[root, flop, turn, river],
        )
        .unwrap();
    assert_eq!(baseline.evaluation, expected_baseline);
    assert_eq!(baseline.prefixes, result.prefixes);
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn prefix_coverage_excludes_unreached_branches_and_all_deviation_trajectories() {
    let mut solver = prefix_solver(841);
    solver.run_sweeps(4).unwrap();
    let root = HistoryKey::ROOT;
    let left = root.child(1, 0);
    let right = root.child(1, 1);
    assert!(solver.history_entry(left).is_some());
    assert!(solver.history_entry(right).is_some());
    for (key, column) in &mut solver.policies {
        if key.history == root {
            column.strategy_sum = vec![1.0, 0.0];
            column.regrets = vec![0.0, 1.0];
        }
    }
    let samples = 257;
    let result = solver
        .evaluate_profile_with_prefixes(
            samples,
            552,
            None,
            ProfileVariant::default(),
            2,
            &[root, left, right],
        )
        .unwrap();
    assert_eq!(result.prefixes[0].reached_samples, samples);
    assert_eq!(
        result.prefixes[0].trajectory_visits_by_street.preflop,
        samples
    );
    assert_eq!(
        result.prefixes[0]
            .candidate_policy_coverage
            .iter()
            .map(|c| c.decision_visits)
            .sum::<u64>(),
        2 * samples
    );
    assert_eq!(result.prefixes[1].reached_samples, samples);
    assert_eq!(
        result.prefixes[1].candidate_policy_coverage[0].decision_visits,
        samples
    );
    assert_eq!(
        result.prefixes[1].candidate_policy_coverage[1].decision_visits,
        0
    );
    // The regret-greedy deviation at player 1 takes right, but baseline
    // average play never does: that trajectory must not enter these counts.
    assert_eq!(result.prefixes[2].reached_samples, 0);
    assert_eq!(
        result.prefixes[2].trajectory_visits_by_street,
        StreetVisitCounts::default()
    );
    assert!(
        result.prefixes[2]
            .candidate_policy_coverage
            .iter()
            .all(|c| *c == CandidatePolicyCoverage::default())
    );
}

#[test]
fn prefix_coverage_is_identical_across_threads_chunks_and_profile_variants() {
    let mut solver = prefix_solver(842);
    solver.run_sweeps(7).unwrap();
    let prefixes = [
        HistoryKey::ROOT,
        HistoryKey::ROOT.child(1, 0),
        HistoryKey::ROOT.child(1, 1),
    ];
    let deviators = (0..2)
        .map(|seat| DeviatorPolicy {
            seat,
            actions: FxHashMap::default(),
        })
        .collect::<Vec<_>>();
    let before = solver.snapshot_state();
    for variant in [
        ProfileVariant::default(),
        ProfileVariant {
            use_current_strategy: true,
            ..ProfileVariant::default()
        },
        ProfileVariant {
            purify_threshold: 0.2,
            ..ProfileVariant::default()
        },
    ] {
        let expected = solver
            .evaluate_profile_with_threads(4097, 553, Some(&deviators), variant, 1)
            .unwrap();
        let serial = solver
            .evaluate_profile_with_prefixes(4097, 553, Some(&deviators), variant, 1, &prefixes)
            .unwrap();
        assert_eq!(serial.evaluation, expected);
        for threads in [2, 8] {
            assert_eq!(
                solver
                    .evaluate_profile_with_prefixes(
                        4097,
                        553,
                        Some(&deviators),
                        variant,
                        threads,
                        &prefixes
                    )
                    .unwrap(),
                serial
            );
            let baseline = solver
                .evaluate_profile_coverage(4097, 553, variant, threads, &prefixes)
                .unwrap();
            assert_eq!(baseline.prefixes, serial.prefixes);
            assert_eq!(baseline.evaluation.seats, serial.evaluation.seats);
            assert_eq!(
                baseline.evaluation.candidate_policy_coverage,
                serial.evaluation.candidate_policy_coverage
            );
            assert!(baseline.evaluation.deviation_gain_lower_bound.is_none());
        }
    }
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn prefix_coverage_rejects_unknown_duplicate_and_excessive_prefixes() {
    let solver = prefix_solver(843);
    let root = HistoryKey::ROOT;
    for prefixes in [
        vec![root, root],
        vec![HistoryKey([255; 16])],
        vec![root; 65],
    ] {
        assert!(matches!(
            solver.evaluate_profile_coverage(2, 0, ProfileVariant::default(), 1, &prefixes),
            Err(SolverError::InvalidState(_))
        ));
    }
    assert!(matches!(
        solver.evaluate_profile_coverage(0, 0, ProfileVariant::default(), 1, &[root]),
        Err(SolverError::ZeroEvaluationSamples)
    ));
    assert!(matches!(
        solver.evaluate_profile_coverage(2, 0, ProfileVariant::default(), 0, &[root]),
        Err(SolverError::ZeroThreads)
    ));
    let empty = solver
        .evaluate_profile_coverage(2, 0, ProfileVariant::default(), 1, &[])
        .unwrap();
    assert!(empty.prefixes.is_empty());
}

#[test]
fn evaluate_with_none_matches_plain_evaluation() {
    let mut solver = prefix_solver(7);
    solver.run_sweeps(5).unwrap();
    let plain = solver.evaluate_average_profile(64, 999).unwrap();
    let explicit_none = solver
        .evaluate_profile(64, 999, None, ProfileVariant::default())
        .unwrap();
    assert_eq!(plain, explicit_none);
}

#[test]
fn evaluate_with_rejects_mismatched_deviator_slices() {
    let mut solver = prefix_solver(7);
    solver.run_sweeps(2).unwrap();
    let wrong_seat = vec![
        DeviatorPolicy {
            seat: 1,
            actions: FxHashMap::default(),
        },
        DeviatorPolicy {
            seat: 0,
            actions: FxHashMap::default(),
        },
    ];
    assert!(matches!(
        solver.evaluate_profile(8, 1, Some(&wrong_seat), ProfileVariant::default()),
        Err(SolverError::InvalidState(_))
    ));
    let wrong_len = vec![DeviatorPolicy {
        seat: 0,
        actions: FxHashMap::default(),
    }];
    assert!(matches!(
        solver.evaluate_profile(8, 1, Some(&wrong_len), ProfileVariant::default()),
        Err(SolverError::InvalidState(_))
    ));
}

#[test]
fn purify_strategy_zeroes_and_renormalizes() {
    // Typical case: two entries survive, renormalized to sum to 1.
    let mut typical = vec![0.5f32, 0.3, 0.1, 0.1];
    purify_strategy(&mut typical, 0.15);
    assert_eq!(typical, vec![0.625, 0.375, 0.0, 0.0]);
    assert!((typical.iter().sum::<f32>() - 1.0).abs() < 1e-6);

    // Every entry below threshold: the argmax survives at probability
    // 1, lowest index wins ties.
    let mut all_below = vec![0.4f32, 0.4, 0.2];
    purify_strategy(&mut all_below, 0.5);
    assert_eq!(all_below, vec![1.0, 0.0, 0.0]);

    let mut tie_at_zero = vec![0.0f32, 0.0, 0.0];
    purify_strategy(&mut tie_at_zero, 0.5);
    assert_eq!(tie_at_zero, vec![1.0, 0.0, 0.0]);

    // Threshold 0.0 does not change any nonnegative distribution
    // (nothing is `< 0.0`), modulo the renormalization division.
    let mut zero_threshold = vec![0.2f32, 0.3, 0.5];
    purify_strategy(&mut zero_threshold, 0.0);
    assert!((zero_threshold[0] - 0.2).abs() < 1e-6);
    assert!((zero_threshold[1] - 0.3).abs() < 1e-6);
    assert!((zero_threshold[2] - 0.5).abs() < 1e-6);

    // Threshold 1.0 (or above the max) degenerates to a pure strategy
    // at the argmax.
    let mut full_purify = vec![0.2f32, 0.5, 0.3];
    purify_strategy(&mut full_purify, 1.0);
    assert_eq!(full_purify, vec![0.0, 1.0, 0.0]);
}

#[test]
fn purified_evaluation_with_zero_threshold_is_byte_identical() {
    let mut solver = prefix_solver(7);
    solver.run_sweeps(5).unwrap();
    let plain = solver.evaluate_average_profile(64, 999).unwrap();
    let purified_zero = solver
        .evaluate_profile(
            64,
            999,
            None,
            ProfileVariant {
                purify_threshold: 0.0,
                use_current_strategy: false,
            },
        )
        .unwrap();
    assert_eq!(plain, purified_zero);
}

#[test]
fn purified_evaluation_runs_and_stays_finite() {
    let mut solver = prefix_solver(23);
    solver.run_sweeps(4).unwrap();
    let samples = 256;
    let seed = 4242;

    for &threshold in &[0.1f32, 1.0f32] {
        let num_players = 2;
        let training_seed = 616;
        let variant = ProfileVariant {
            purify_threshold: threshold,
            use_current_strategy: false,
        };
        let deviators: Vec<DeviatorPolicy> = (0..num_players)
            .map(|seat| {
                solver
                    .train_deviator(seat, 200, training_seed, variant)
                    .unwrap()
            })
            .collect();
        let evaluation = solver
            .evaluate_profile(samples, seed, Some(&deviators), variant)
            .unwrap();
        assert_eq!(evaluation.samples, samples);
        for seat in &evaluation.seats {
            assert!(seat.mean.is_finite());
            assert!(seat.stderr.is_finite());
            assert!(seat.ci95[0].is_finite() && seat.ci95[1].is_finite());
        }
        let gains = evaluation.deviation_gain_lower_bound.as_ref().unwrap();
        for gain in gains {
            assert!(gain.mean.is_finite());
            assert!(gain.mean >= 0.0, "gain must be clamped nonnegative");
            assert!(gain.ci95[0] >= 0.0);
        }
    }
}

#[test]
fn purify_threshold_out_of_range_is_rejected() {
    let mut solver = prefix_solver(7);
    solver.run_sweeps(2).unwrap();
    let variant_of = |purify_threshold: f32| ProfileVariant {
        purify_threshold,
        use_current_strategy: false,
    };
    assert!(matches!(
        solver.evaluate_profile(8, 1, None, variant_of(-0.01)),
        Err(SolverError::InvalidState(_))
    ));
    assert!(matches!(
        solver.evaluate_profile(8, 1, None, variant_of(1.5)),
        Err(SolverError::InvalidState(_))
    ));
    assert!(matches!(
        solver.evaluate_profile(8, 1, None, variant_of(f32::NAN)),
        Err(SolverError::InvalidState(_))
    ));
    assert!(matches!(
        solver.train_deviator(0, 10, 1, variant_of(-0.01)),
        Err(SolverError::InvalidState(_))
    ));
    assert!(matches!(
        solver.train_deviator(0, 10, 1, variant_of(1.5)),
        Err(SolverError::InvalidState(_))
    ));
}

#[test]
// Only 1-2 real CFR sweeps run before evaluating, so seat 0's average
// strategy over "win"/"pass" is still close to uniform even though
// "win" always beats "pass" regardless of the opponent's action -- a
// trained deviator should find (and the regret-greedy heuristic may or
// may not fully find) that slack. Verified stable across several
// seed/sample-count choices during development (see PR discussion);
// this exact configuration (2 sweeps, 4096 samples, 4000 training
// traversals) was chosen because it was not flaky across repeated
// `cargo test --test-threads=1` runs. Per the spec, the comparison
// uses the max over seats (rather than a strict per-seat comparison)
// because the per-seat comparison was occasionally flaky: the
// regret-greedy baseline can, by chance, already be near-optimal for
// one particular seat on a given sample set while the trained
// deviator's held-out estimate for that same seat has more sampling
// noise, even though the trained deviator is uniformly at least as
// strong in aggregate.
fn trained_deviator_finds_gain_against_a_barely_trained_profile() {
    let mut solver = prefix_solver(13);
    solver.run_sweeps(2).unwrap();

    let samples = 4096;
    let seed = 2024;
    let without = solver.evaluate_average_profile(samples, seed).unwrap();

    let num_players = 2;
    let training_traversals = 4000;
    let training_seed = 909;
    let deviators: Vec<DeviatorPolicy> = (0..num_players)
        .map(|seat| {
            solver
                .train_deviator(
                    seat,
                    training_traversals,
                    training_seed,
                    ProfileVariant::default(),
                )
                .unwrap()
        })
        .collect();
    let with = solver
        .evaluate_profile(samples, seed, Some(&deviators), ProfileVariant::default())
        .unwrap();

    let with_gains = with.deviation_gain_lower_bound.as_ref().unwrap();
    let without_gains = without.deviation_gain_lower_bound.as_ref().unwrap();
    assert!(with_gains.iter().all(|gain| gain.mean >= 0.0));

    let with_max = with_gains
        .iter()
        .map(|gain| gain.mean)
        .fold(f64::NEG_INFINITY, f64::max);
    let without_max = without_gains
        .iter()
        .map(|gain| gain.mean)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        with_max > 0.0,
        "trained deviator found no gain at all: {with_max}"
    );
    // Sampling-noise guard: 4096 samples of a 0/1 paired gain have
    // stderr on the order of 1e-2, so 1e-6 would be too tight if the
    // two profiles were genuinely close; use a small multiple of the
    // observed stderr instead of a bare constant.
    let epsilon = 6.0
        * with_gains
            .iter()
            .chain(without_gains.iter())
            .map(|gain| gain.stderr)
            .fold(0.0, f64::max);
    assert!(
        with_max >= without_max - epsilon,
        "trained deviator ({with_max}) should be at least as strong as the regret-greedy \
             heuristic ({without_max}) within noise guard {epsilon}"
    );
}

#[test]
fn ordered_deltas_make_thread_counts_and_resume_bit_identical() {
    let mut uninterrupted = solver(777, 1 << 20);
    uninterrupted.run_sweeps_with_threads(40, 4).unwrap();

    let mut single_threaded = solver(777, 1 << 20);
    single_threaded.run_sweeps_with_threads(40, 1).unwrap();
    assert_eq!(
        uninterrupted.snapshot_state(),
        single_threaded.snapshot_state()
    );

    let mut first_half = solver(777, 1 << 20);
    first_half.run_sweeps_with_threads(17, 1).unwrap();
    let state = first_half.snapshot_state();
    let mut resumed = MultiwaySolver::from_state(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        state,
    )
    .unwrap();
    resumed.run_sweeps_with_threads(23, 4).unwrap();

    assert_eq!(uninterrupted.snapshot_state(), resumed.snapshot_state());
    assert_eq!(uninterrupted.metrics(), resumed.metrics());
}

#[test]
fn sweep_batching_is_bit_identical_across_thread_counts() {
    let mut single_threaded = solver_with_batch(99, 1 << 20, 4);
    single_threaded.run_sweeps_with_threads(24, 1).unwrap();

    let mut multi_threaded = solver_with_batch(99, 1 << 20, 4);
    multi_threaded.run_sweeps_with_threads(24, 8).unwrap();

    assert_eq!(
        single_threaded.snapshot_state(),
        multi_threaded.snapshot_state()
    );
    assert_eq!(single_threaded.metrics(), multi_threaded.metrics());

    // An uneven final batch (24 sweeps is not a multiple of the batch
    // size below) still lands on exactly the requested sweep count.
    let mut uneven = solver_with_batch(99, 1 << 20, 5);
    assert_eq!(
        uneven
            .run_sweeps_with_threads_until(24, 4, || true)
            .unwrap(),
        24
    );
    assert_eq!(uneven.completed_sweeps(), 24);
}

#[test]
fn observed_sweep_batches_report_committed_state_without_changing_results() {
    let mut observed = solver_with_batch(99, 1 << 20, 4);
    let mut boundaries = Vec::new();
    observed
        .run_sweeps_with_threads_until_observed(
            10,
            4,
            || true,
            |solver| boundaries.push(solver.completed_sweeps()),
        )
        .unwrap();

    let mut reference = solver_with_batch(99, 1 << 20, 4);
    reference.run_sweeps_with_threads(10, 4).unwrap();

    assert_eq!(boundaries, vec![4, 8, 10]);
    assert_eq!(observed.snapshot_state(), reference.snapshot_state());
}

#[test]
fn stopping_and_restarting_on_a_full_batch_boundary_preserves_state() {
    let mut interrupted = solver_with_batch(99, 1 << 20, 4);
    let mut polls = 0;
    let completed = interrupted
        .run_sweeps_with_threads_until(24, 4, || {
            polls += 1;
            polls <= 2
        })
        .unwrap();
    assert_eq!(completed, 8);
    interrupted.run_sweeps_with_threads(16, 4).unwrap();

    let mut uninterrupted = solver_with_batch(99, 1 << 20, 4);
    uninterrupted.run_sweeps_with_threads(24, 4).unwrap();

    assert_eq!(interrupted.snapshot_state(), uninterrupted.snapshot_state());
    assert_eq!(interrupted.metrics(), uninterrupted.metrics());
}

#[test]
fn sweep_batching_reduces_regret_comparably_to_unbatched() {
    const SWEEPS: u64 = 400;

    // Disables early discounting for this comparison (unlike the
    // `solver`/`solver_with_batch` fixtures above, which intentionally
    // discount every 5 sweeps to exercise that path elsewhere): batching
    // interacts with aggressive discounting cadence in ways unrelated to
    // what this test checks, and would otherwise dominate the
    // batch-vs-unbatched difference in an already-tiny toy-game regret
    // signal.
    fn solver_without_discount(seed: u64, sweep_batch: u64) -> MultiwaySolver<DominatedChoice> {
        MultiwaySolver::new(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed,
                max_memory_bytes: 1 << 20,
                max_traversal_depth: 16,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch,
                traverser_vector: false,
                prune: false,
                prune_threshold: DEFAULT_PRUNE_THRESHOLD,
                prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
            },
        )
        .unwrap()
    }

    fn mean_positive_regret(regrets: &[f64]) -> f64 {
        regrets.iter().sum::<f64>() / regrets.len() as f64
    }

    let mut baseline = solver_without_discount(4242, 1);
    baseline.run_sweeps_with_threads(SWEEPS, 1).unwrap();
    let baseline_regret = mean_positive_regret(&baseline.metrics().average_positive_regret);

    let mut batched = solver_without_discount(4242, 4);
    batched.run_sweeps_with_threads(SWEEPS, 1).unwrap();
    let batched_regret = mean_positive_regret(&batched.metrics().average_positive_regret);

    assert!(baseline_regret.is_finite() && baseline_regret >= 0.0);
    assert!(batched_regret.is_finite() && batched_regret >= 0.0);
    // Both must actually be converging (small relative to the game's
    // unit-scale payoffs), and neither must be more than 2x the other --
    // an absolute floor keeps the ratio check meaningful once both sides
    // are already near zero.
    let tolerance = (baseline_regret.max(batched_regret) * 2.0).max(1e-3);
    assert!(
        batched_regret <= tolerance && baseline_regret <= tolerance,
        "batch=4 average positive regret {batched_regret} should be within 2x of batch=1's {baseline_regret}"
    );
}

#[test]
fn resume_rejects_mismatched_sweep_batch() {
    let state = solver_with_batch(21, 1 << 20, 1).snapshot_state();
    let mut changed = state.config;
    changed.sweep_batch = 4;
    assert!(matches!(
        MultiwaySolver::from_state_with_config(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state,
            changed,
        ),
        Err(SolverError::ResumeConfigurationMismatch)
    ));
}

#[test]
fn zero_sweep_batch_is_rejected() {
    let mut config = SolverConfig {
        seed: 1,
        max_memory_bytes: 1 << 20,
        max_traversal_depth: 16,
        exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
        discount_every: 5,
        discount_until: 100,
        sweep_batch: 1,
        traverser_vector: false,
        prune: false,
        prune_threshold: DEFAULT_PRUNE_THRESHOLD,
        prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
    };
    config.sweep_batch = 0;
    assert!(matches!(
        MultiwaySolver::new(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            config,
        ),
        Err(SolverError::ZeroSweepBatch)
    ));
}

#[test]
fn interruptible_parallel_run_polls_boundaries_and_preserves_determinism() {
    let mut interrupted = solver(0x1234, 1 << 20);
    let mut polls = 0u64;
    let completed = interrupted
        .run_sweeps_with_threads_until(40, 4, || {
            polls += 1;
            polls <= 7
        })
        .unwrap();
    assert_eq!(completed, 7);
    assert_eq!(polls, 8);

    let mut reference = solver(0x1234, 1 << 20);
    reference.run_sweeps_with_threads(7, 1).unwrap();
    assert_eq!(interrupted.snapshot_state(), reference.snapshot_state());
}

#[test]
fn resource_limit_state_resumes_with_a_larger_operational_limit() {
    let mut limited = solver(55, 1);
    assert!(matches!(
        limited.run_sweeps_with_threads(1, 2),
        Err(SolverError::MemoryLimit { limit: 1, .. })
    ));
    let fingerprint = limited.configuration_fingerprint();
    let state = limited.snapshot_state();
    let mut expanded_config = state.config;
    expanded_config.max_memory_bytes = 1 << 20;

    let mut resumed = MultiwaySolver::from_state_with_config(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        state,
        expanded_config,
    )
    .unwrap();
    assert_eq!(resumed.config().max_memory_bytes, 1 << 20);
    assert_eq!(resumed.configuration_fingerprint(), fingerprint);
    resumed.run_sweeps_with_threads(10, 4).unwrap();

    let mut reference = solver(55, 1 << 20);
    reference.run_sweeps_with_threads(10, 1).unwrap();
    assert_eq!(resumed.snapshot_state(), reference.snapshot_state());
}

#[test]
fn restore_rejects_algorithm_changes_even_when_memory_limit_changes() {
    let state = solver(9, 1 << 10).snapshot_state();
    let mut changed = state.config;
    changed.max_memory_bytes = 1 << 20;
    changed.exploration_epsilon = 0.5;
    assert!(matches!(
        MultiwaySolver::from_state_with_config(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state,
            changed,
        ),
        Err(SolverError::ResumeConfigurationMismatch)
    ));
}

#[test]
fn independent_action_substream_ignores_deal_rng_consumption() {
    let mut short_deal = traversal_deal_rng(41, 19, 1);
    let mut long_deal = traversal_deal_rng(41, 19, 1);
    let _ = short_deal.next_u64();
    for _ in 0..10_000 {
        let _ = long_deal.next_u64();
    }

    let mut after_short = traversal_action_rng(41, 19, 1);
    let mut after_long = traversal_action_rng(41, 19, 1);
    let left: Vec<_> = (0..32).map(|_| after_short.next_u64()).collect();
    let right: Vec<_> = (0..32).map(|_| after_long.next_u64()).collect();
    assert_eq!(left, right);
}

#[test]
fn resume_rejects_inconsistent_sample_id() {
    let solver = solver(17, 1 << 20);
    let mut state = solver.snapshot_state();
    state.next_sample_id = 1;
    assert!(matches!(
        MultiwaySolver::from_state(
            DominatedChoice,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state
        ),
        Err(SolverError::InvalidState(
            "next sample id is inconsistent with traversals"
        ))
    ));
}

#[test]
fn memory_cap_stops_before_allocating_first_column() {
    let mut limited = solver(0, 1);
    let before = limited.snapshot_state();
    assert!(matches!(
        limited.run_sweeps_with_threads(1, 4),
        Err(SolverError::MemoryLimit { limit: 1, .. })
    ));
    assert_eq!(limited.snapshot_state(), before);
    assert_eq!(limited.metrics().infosets, 0);
    assert_eq!(limited.metrics().traversals, 0);

    // merge_sweep's transactional guarantee also means the solver stays
    // resumable after a MemoryLimit error: raising the memory budget and
    // continuing from the untouched (pre-failure) state must produce the
    // exact same result as a solver that had that budget from the start.
    let mut expanded_config = limited.config();
    expanded_config.max_memory_bytes = 1 << 20;
    let mut resumed = MultiwaySolver::from_state_with_config(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        before,
        expanded_config,
    )
    .unwrap();
    resumed.run_sweeps_with_threads(5, 4).unwrap();

    let mut reference = solver(0, 1 << 20);
    reference.run_sweeps_with_threads(5, 4).unwrap();
    assert_eq!(resumed.snapshot_state(), reference.snapshot_state());
}

#[test]
fn exploration_corrects_regret_updates_below_sampled_opponent_actions() {
    let mut solver = MultiwaySolver::new(
        PrefixImportanceGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed: 73,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: 1.0,
            discount_every: 1,
            discount_until: 0,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap();
    let opponent_key = InfoKey {
        history: HistoryKey::ROOT,
        player: 1,
        street: 0,
        active_opponents: 1,
        bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
    };
    solver
        .strategy_for(opponent_key, &PrefixState::Opponent)
        .unwrap();
    solver.policies.get_mut(&opponent_key).unwrap().regrets = vec![9.0, 1.0];

    solver.run_traversals(1).unwrap();

    let (hero_key, hero_column) = solver
        .policies
        .iter()
        .find(|(key, _)| key.player == 0)
        .expect("sampled branch creates the hero information set");
    let target_probability = if hero_key.history == HistoryKey::ROOT.child(1, 0) {
        0.9
    } else {
        assert_eq!(hero_key.history, HistoryKey::ROOT.child(1, 1));
        0.1
    };
    let importance = target_probability / 0.5;
    assert!((f64::from(hero_column.regrets[0]) - 0.5 * importance).abs() < 1e-6);
    assert!((f64::from(hero_column.regrets[1]) + 0.5 * importance).abs() < 1e-6);
}

#[test]
fn three_player_one_step_estimator_matches_full_enumeration_oracle() {
    let mut solver = MultiwaySolver::new(
        ThreePlayerOracleGame,
        DealSampler::new(vec![Range::full(), Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed: 0x5eed,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            // Sampling q is uniform while the target opponent strategy
            // below is [0.75, 0.25], exercising importance correction.
            exploration_epsilon: 1.0,
            discount_every: 1,
            discount_until: 0,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap();
    let opponent_key = InfoKey {
        history: HistoryKey::ROOT,
        player: 1,
        street: 0,
        active_opponents: 2,
        bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
    };
    solver
        .strategy_for(opponent_key, &ThreePlayerOracleState::Opponent)
        .unwrap();
    solver.policies.get_mut(&opponent_key).unwrap().regrets = vec![3.0, 1.0];

    let oracle = full_enumeration_oracle_regret([0.75, 0.25], [0.5, 0.5]);
    assert_eq!(oracle, [1.0, -1.0]);
    let mut terminal = [0.0; 3];
    ThreePlayerOracleGame.terminal_utilities(
        &ThreePlayerOracleState::Terminal {
            opponent_action: 0,
            hero_action: 0,
        },
        &solver
            .sampler
            .sample(&mut traversal_deal_rng(9, 0, 0))
            .unwrap(),
        &mut terminal,
    );
    assert_ne!(terminal.iter().sum::<f64>(), 0.0);

    const SAMPLES: u64 = 20_000;
    let mut means = [0.0; 2];
    let mut m2 = [0.0; 2];
    for sample_id in 0..SAMPLES {
        let delta = solver.generate_traversal_delta(sample_id, 0, 1.0).unwrap();
        let AnyTraversalDelta::Sparse(delta) = delta else {
            panic!("full-recall solver must produce a sparse traversal delta")
        };
        let values = delta
            .events
            .into_iter()
            .find_map(|event| match event {
                TraversalEvent::AddRegret { key, values } if key.player == 0 => Some(values),
                _ => None,
            })
            .expect("traverser update must be present");
        assert_eq!(values.len(), 2);
        let count = (sample_id + 1) as f64;
        for action in 0..2 {
            let difference = values[action] - means[action];
            means[action] += difference / count;
            m2[action] += difference * (values[action] - means[action]);
        }
    }
    for action in 0..2 {
        let variance = m2[action] / (SAMPLES - 1) as f64;
        let stderr = (variance / SAMPLES as f64).sqrt();
        assert!(
            (means[action] - oracle[action]).abs() <= 5.0 * stderr,
            "action {action}: sample mean {}, oracle {}, stderr {stderr}",
            means[action],
            oracle[action]
        );
    }
}

#[test]
fn history_keys_are_stable_and_action_sensitive() {
    assert_eq!(HistoryKey::ROOT.child(0, 1), HistoryKey::ROOT.child(0, 1));
    assert_ne!(HistoryKey::ROOT.child(0, 0), HistoryKey::ROOT.child(0, 1));
    assert_ne!(HistoryKey::ROOT.child(0, 1), HistoryKey::ROOT.child(1, 1));
}

#[test]
fn visited_history_trie_resolves_stable_action_labels() {
    let mut solver = solver(5, 1 << 20);
    solver.run_traversals(1).unwrap();
    let best = HistoryKey::ROOT.child(0, 0);
    let dominated = HistoryKey::ROOT.child(0, 1);
    assert_eq!(solver.resolve_history(HistoryKey::ROOT), Some(Vec::new()));
    assert_eq!(solver.resolve_history(best), Some(vec!["best".to_string()]));
    assert_eq!(
        solver.resolve_history(dominated),
        Some(vec!["dominated".to_string()])
    );
    let state = solver.snapshot_state();
    assert_eq!(state.histories.len(), 2);
    assert!(
        state
            .histories
            .windows(2)
            .all(|pair| pair[0].key < pair[1].key)
    );
}

#[test]
fn node_children_and_strategies_at_expose_a_live_average_profile() {
    let mut solver = solver(7, 1 << 20);
    solver.run_sweeps(20).unwrap();

    let children = solver.node_children(HistoryKey::ROOT);
    assert!(!children.is_empty(), "expected root's children to exist");
    let mut labels: Vec<&str> = children
        .iter()
        .map(|entry| entry.action_label.as_str())
        .collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["best", "dominated"]);
    assert!(children.windows(2).all(|pair| {
        (pair[0].actor, pair[0].action_index) <= (pair[1].actor, pair[1].action_index)
    }));

    let rows = solver.strategies_at(HistoryKey::ROOT);
    assert!(!rows.is_empty(), "expected a live policy at root");
    for (_, action_labels, probabilities) in &rows {
        assert_eq!(action_labels.len(), probabilities.len());
        let sum: f32 = probabilities.iter().sum();
        assert!((sum - 1.0).abs() < 1e-3, "probabilities summed to {sum}");
    }
}

// --- RecallMode::Street (dense arena) -----------------------------------

/// Single-decision-node game shaped exactly like [`DominatedChoice`], but
/// opted into `RecallMode::Street` with a trivial one-bucket abstraction.
/// Because the game shape, seeds, and per-node math are otherwise
/// identical, a dense-mode run must reproduce the *exact* regret/
/// strategy-sum trajectory `DominatedChoice`'s sparse tests already pin
/// (see [`batched_discount_scales_regret_and_strategy_at_cadence`]) --
/// strong evidence the dense traversal/merge path computes the same
/// numbers as the sparse one, just through a different storage backend.
#[derive(Clone, Copy)]
struct DenseDominatedChoice;

impl ExternalSamplingGame for DenseDominatedChoice {
    type State = ToyState;
    type Actions = ToyState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        ToyState::Choose
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        matches!(state, ToyState::Choose).then_some(0)
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(matches!(actions, ToyState::Choose)) * 2
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        assert_eq!(*actions, ToyState::Choose);
        ToyState::Terminal(action_index)
    }

    fn write_action_label(&self, _actions: &Self::Actions, action_index: usize, out: &mut String) {
        let label = match action_index {
            0 => "best",
            1 => "dominated",
            _ => panic!("action out of range"),
        };
        out.push_str(label);
    }

    fn bucket(&self, _state: &Self::State, _world: &SampledWorld, _actor: usize) -> PrivateInfo {
        PrivateInfo::from_current_bucket(Street::Preflop, 1, 0)
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        let ToyState::Terminal(action) = *state else {
            panic!("not terminal")
        };
        utilities[0] = f64::from(action == 0) * 2.0 - 1.0;
        utilities[1] = -utilities[0];
    }

    fn recall_mode(&self) -> RecallMode {
        RecallMode::Street
    }

    fn bucket_count(&self, _street: Street, _active_opponents: u8) -> u32 {
        1
    }

    fn dense_node_context(&self, _state: &Self::State) -> DenseNodeContext {
        DenseNodeContext {
            street: Street::Preflop,
            active_opponents: 1,
            bucket_active_opponents: 1,
        }
    }
}

/// Two-decision-node game (player 0 then player 1, each choosing between
/// two labeled actions) with a real, world-dependent two-bucket
/// abstraction, used for the dense-arena tests that need more than one
/// tree node and more than one touched bucket per node.
#[derive(Clone, Copy)]
struct DenseToyGame;

#[derive(Clone)]
struct ConditionalWeightGame {
    street_recall: bool,
    bucket_zero_combo: usize,
}

#[test]
fn endpoint_deviation_override_changes_one_action_without_changing_prefix_weight() {
    use super::conditioned::ForcedPrefixReplay;

    let own = cards::combo_index("As".parse().unwrap(), "Ah".parse().unwrap());
    let opponent = cards::combo_index("Ks".parse().unwrap(), "Kh".parse().unwrap());
    let world = SampledWorld::new(
        vec![own, opponent],
        ["2c", "3d", "4h", "5s", "6c"].map(|c| c.parse().unwrap()),
    )
    .unwrap();
    let mut solver = MultiwaySolver::new(
        ConditionalWeightGame {
            street_recall: false,
            bucket_zero_combo: own,
        },
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 8,
            ..SolverConfig::default()
        },
    )
    .unwrap();
    let private = solver.game.bucket(&solver.game.root_state(), &world, 0);
    let key = |history| InfoKey {
        history,
        player: 0,
        street: private.street,
        active_opponents: private.active_opponents,
        bucket_path: private.bucket_path,
    };
    let column = |labels: [&str; 2], average: [f32; 2]| PolicyColumn {
        action_labels: labels.map(str::to_string).to_vec(),
        regrets: vec![1.0, 0.0],
        strategy_sum: average.to_vec(),
    };
    solver
        .policies
        .insert(key(HistoryKey::ROOT), column(["left", "right"], [1.0, 0.0]));
    for action in 0..2 {
        solver.policies.insert(
            key(HistoryKey::ROOT.child(0, action)),
            column(["up", "down"], [0.0, 1.0]),
        );
    }
    let right = solver
        .policies
        .get_mut(&key(HistoryKey::ROOT.child(0, 1)))
        .unwrap();
    right.strategy_sum = vec![1.0, 0.0];
    right.regrets = vec![0.0, 1.0];
    let before = solver.snapshot_state();
    let mut ordinary_rng = evaluation_action_rng(971, 0, None);
    let mut ordinary_coverage = vec![CandidatePolicyCoverage::default(); 2];
    let ordinary = solver
        .evaluate_world(
            &world,
            &mut ordinary_rng,
            None,
            None,
            0.0,
            false,
            Some(&mut ordinary_coverage),
            &mut [],
            None,
        )
        .unwrap();
    assert_eq!(ordinary, vec![-2.0, 2.0]);
    let mut disabled = ForcedPrefixReplay {
        actions: &[],
        endpoint_action: None,
        weight: 1.0,
        skip_weight_actions: 0,
        sources: [false; 3],
    };
    let mut disabled_rng = evaluation_action_rng(971, 0, None);
    let mut disabled_coverage = vec![CandidatePolicyCoverage::default(); 2];
    assert_eq!(
        ordinary,
        solver
            .evaluate_world(
                &world,
                &mut disabled_rng,
                None,
                None,
                0.0,
                false,
                Some(&mut disabled_coverage),
                &mut [],
                Some(&mut disabled)
            )
            .unwrap()
    );
    assert_eq!(disabled_coverage, ordinary_coverage);
    assert_eq!(disabled.weight, 1.0);
    assert_eq!(
        disabled_rng.clone().next_u64(),
        ordinary_rng.clone().next_u64()
    );

    // "right" has exactly zero baseline probability but remains a legal
    // deviation. The same actor's later decision must still play "up" (0),
    // unlike repeating the override (1) or regret-greedy fallback (1).
    let mut candidate = ForcedPrefixReplay {
        endpoint_action: Some(1),
        ..disabled
    };
    let mut candidate_rng = evaluation_action_rng(971, 0, None);
    let value = solver
        .evaluate_world(
            &world,
            &mut candidate_rng,
            None,
            None,
            0.0,
            false,
            None,
            &mut [],
            Some(&mut candidate),
        )
        .unwrap();
    assert_eq!(value, vec![1.0, -1.0]);
    assert_eq!(candidate.weight, 1.0);
    assert_eq!(candidate.sources, [false; 3]);
    assert_eq!(candidate_rng.next_u64(), ordinary_rng.next_u64());

    // With a nonempty path, the override moves to precisely its endpoint.
    let mut deeper = ForcedPrefixReplay {
        actions: &[0],
        endpoint_action: Some(0),
        weight: 1.0,
        skip_weight_actions: 0,
        sources: [false; 3],
    };
    assert_eq!(
        solver
            .evaluate_world(
                &world,
                &mut evaluation_action_rng(972, 0, None),
                None,
                None,
                0.0,
                false,
                None,
                &mut [],
                Some(&mut deeper)
            )
            .unwrap(),
        vec![4.0, -4.0]
    );
    assert_eq!(deeper.weight, 1.0);
    let mut invalid = ForcedPrefixReplay {
        endpoint_action: Some(2),
        ..deeper
    };
    assert!(
        solver
            .evaluate_world(
                &world,
                &mut evaluation_action_rng(972, 0, None),
                None,
                None,
                0.0,
                false,
                None,
                &mut [],
                Some(&mut invalid)
            )
            .is_err()
    );
    assert_eq!(solver.snapshot_state(), before);

    solver
        .policies
        .get_mut(&key(HistoryKey::ROOT))
        .unwrap()
        .strategy_sum = vec![0.0, 1.0];
    let mut zero = ForcedPrefixReplay {
        actions: &[0],
        endpoint_action: Some(0),
        weight: 1.0,
        skip_weight_actions: 0,
        sources: [false; 3],
    };
    assert_eq!(
        solver
            .evaluate_world(
                &world,
                &mut evaluation_action_rng(972, 0, None),
                None,
                None,
                0.0,
                false,
                None,
                &mut [],
                Some(&mut zero)
            )
            .unwrap(),
        vec![0.0; 2]
    );
    assert_eq!(zero.weight, 0.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConditionalWeightState {
    First,
    Second {
        first_action: usize,
    },
    Terminal {
        first_action: usize,
        second_action: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DenseToyState {
    First,
    Second { first_action: usize },
    Terminal { first_action: usize },
}

impl ExternalSamplingGame for DenseToyGame {
    type State = DenseToyState;
    type Actions = DenseToyState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        DenseToyState::First
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        match state {
            DenseToyState::First => Some(0),
            DenseToyState::Second { .. } => Some(1),
            DenseToyState::Terminal { .. } => None,
        }
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(!matches!(actions, DenseToyState::Terminal { .. })) * 2
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        match *actions {
            DenseToyState::First => DenseToyState::Second {
                first_action: action_index,
            },
            DenseToyState::Second { first_action } => DenseToyState::Terminal { first_action },
            DenseToyState::Terminal { .. } => panic!("terminal state has no child"),
        }
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        let labels = match actions {
            DenseToyState::First => ["best", "dominated"],
            DenseToyState::Second { .. } => ["call", "fold"],
            DenseToyState::Terminal { .. } => panic!("terminal state has no actions"),
        };
        out.push_str(labels[action_index]);
    }

    fn bucket(&self, _state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo {
        let bucket = (world.hole_combo(actor) % 2) as u32;
        PrivateInfo::from_current_bucket(Street::Preflop, 1, bucket)
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        let DenseToyState::Terminal { first_action } = *state else {
            panic!("not terminal")
        };
        // Action 0 ("best") is dominant for player 0 regardless of
        // player 1's action or either player's bucket.
        utilities[0] = f64::from(first_action == 0) * 2.0 - 1.0;
        utilities[1] = -utilities[0];
    }

    fn recall_mode(&self) -> RecallMode {
        RecallMode::Street
    }

    fn bucket_count(&self, _street: Street, _active_opponents: u8) -> u32 {
        2
    }

    fn dense_node_context(&self, _state: &Self::State) -> DenseNodeContext {
        DenseNodeContext {
            street: Street::Preflop,
            active_opponents: 1,
            bucket_active_opponents: 1,
        }
    }

    fn bucket_for_combo(
        &self,
        _state: &Self::State,
        _world: &SampledWorld,
        _actor: usize,
        combo: usize,
    ) -> BucketId {
        (combo % 2) as u32
    }

    fn terminal_utilities_for_combos(
        &self,
        state: &Self::State,
        world: &SampledWorld,
        traverser: usize,
        combos: &[usize],
        out: &mut Vec<f64>,
    ) {
        // Card-independent terminal, same as `terminal_utilities`: every
        // combo gets the same constant.
        let mut utilities = vec![0.0; 2];
        self.terminal_utilities(state, world, &mut utilities);
        out.clear();
        out.resize(combos.len(), utilities[traverser]);
    }
}

impl ConditionalWeightGame {
    fn combo_bucket(&self, combo: usize) -> BucketId {
        u32::from(combo != self.bucket_zero_combo)
    }

    fn payoff(&self, combo: usize, first_action: usize, second_action: usize) -> f64 {
        let hand_scale = if self.combo_bucket(combo) == 0 {
            1.0
        } else {
            3.0
        };
        let action_value = [[4.0, -2.0], [1.0, 3.0]][first_action][second_action];
        hand_scale * action_value
    }
}

impl ExternalSamplingGame for ConditionalWeightGame {
    type State = ConditionalWeightState;
    type Actions = ConditionalWeightState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        ConditionalWeightState::First
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        (!matches!(state, ConditionalWeightState::Terminal { .. })).then_some(0)
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(!matches!(actions, ConditionalWeightState::Terminal { .. })) * 2
    }

    fn next_state_with(
        &self,
        _state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        match *actions {
            ConditionalWeightState::First => ConditionalWeightState::Second {
                first_action: action_index,
            },
            ConditionalWeightState::Second { first_action } => ConditionalWeightState::Terminal {
                first_action,
                second_action: action_index,
            },
            ConditionalWeightState::Terminal { .. } => panic!("terminal state has no child"),
        }
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        let labels = match actions {
            ConditionalWeightState::First => ["left", "right"],
            ConditionalWeightState::Second { .. } => ["up", "down"],
            ConditionalWeightState::Terminal { .. } => panic!("terminal state has no actions"),
        };
        out.push_str(labels[action_index]);
    }

    fn bucket(&self, _state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo {
        let bucket = self.combo_bucket(world.hole_combo(actor));
        if self.street_recall {
            PrivateInfo::from_current_bucket(Street::Preflop, 1, bucket)
        } else {
            PrivateInfo::from_path(
                Street::Preflop,
                1,
                BucketPath {
                    preflop: bucket,
                    flop: 0,
                    turn: 0,
                    river: 0,
                },
            )
        }
    }

    fn terminal_utilities(&self, state: &Self::State, world: &SampledWorld, utilities: &mut [f64]) {
        let ConditionalWeightState::Terminal {
            first_action,
            second_action,
        } = *state
        else {
            panic!("not terminal")
        };
        utilities[0] = self.payoff(world.hole_combo(0), first_action, second_action);
        utilities[1] = -utilities[0];
    }

    fn recall_mode(&self) -> RecallMode {
        if self.street_recall {
            RecallMode::Street
        } else {
            RecallMode::Full
        }
    }

    fn bucket_count(&self, _street: Street, _active_opponents: u8) -> u32 {
        2
    }

    fn dense_node_context(&self, _state: &Self::State) -> DenseNodeContext {
        DenseNodeContext {
            street: Street::Preflop,
            active_opponents: 1,
            bucket_active_opponents: 1,
        }
    }

    fn bucket_for_combo(
        &self,
        _state: &Self::State,
        _world: &SampledWorld,
        _actor: usize,
        combo: usize,
    ) -> BucketId {
        self.combo_bucket(combo)
    }

    fn terminal_utilities_for_combos(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        _traverser: usize,
        combos: &[usize],
        out: &mut Vec<f64>,
    ) {
        let ConditionalWeightState::Terminal {
            first_action,
            second_action,
        } = *state
        else {
            panic!("not terminal")
        };
        out.clear();
        out.extend(
            combos
                .iter()
                .map(|&combo| self.payoff(combo, first_action, second_action)),
        );
    }
}

fn dense_dominated_solver(seed: u64) -> MultiwaySolver<DenseDominatedChoice> {
    MultiwaySolver::new(
        DenseDominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: 5,
            discount_until: 100,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap()
}

fn dense_toy_solver(seed: u64, sweep_batch: u64) -> MultiwaySolver<DenseToyGame> {
    MultiwaySolver::new(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap()
}

fn dense_vector_toy_solver(seed: u64, sweep_batch: u64) -> MultiwaySolver<DenseToyGame> {
    MultiwaySolver::new(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch,
            traverser_vector: true,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap()
}

/// Same shape as [`dense_vector_toy_solver`] but with every
/// [`SolverConfig::prune`] knob configurable, for the regret-based
/// pruning tests below.
fn dense_vector_toy_solver_with_prune(
    seed: u64,
    prune: bool,
    prune_threshold: f64,
    prune_skip_probability: f64,
) -> MultiwaySolver<DenseToyGame> {
    MultiwaySolver::new(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch: 1,
            traverser_vector: true,
            prune,
            prune_threshold,
            prune_skip_probability,
        },
    )
    .unwrap()
}

#[test]
fn prune_disabled_is_byte_identical() {
    let via_default = SolverConfig {
        seed: 909,
        max_memory_bytes: 1 << 20,
        max_traversal_depth: 16,
        traverser_vector: true,
        ..SolverConfig::default()
    };
    // Guard the plumbing: `SolverConfig::default()`'s prune knobs must
    // equal the ones every other test spells out explicitly.
    assert!(!via_default.prune);
    assert_eq!(via_default.prune_threshold, DEFAULT_PRUNE_THRESHOLD);
    assert_eq!(
        via_default.prune_skip_probability,
        DEFAULT_PRUNE_SKIP_PROBABILITY
    );

    let mut default_solver = MultiwaySolver::new(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        via_default,
    )
    .unwrap();
    let mut explicit_solver = dense_vector_toy_solver_with_prune(
        909,
        false,
        DEFAULT_PRUNE_THRESHOLD,
        DEFAULT_PRUNE_SKIP_PROBABILITY,
    );
    default_solver.run_sweeps(10).unwrap();
    explicit_solver.run_sweeps(10).unwrap();
    assert_eq!(
        default_solver.snapshot_state(),
        explicit_solver.snapshot_state()
    );
}

#[test]
fn prune_with_unreachable_threshold_is_byte_identical() {
    let seed = 4242;
    let mut baseline =
        dense_vector_toy_solver_with_prune(seed, false, DEFAULT_PRUNE_THRESHOLD, 0.95);
    // `prune_threshold` is astronomically far from any regret this toy
    // game's utilities (bounded in [-1, 1]) could ever accumulate, and
    // `prune_skip_probability = 1.0` maximizes how often the (never
    // taken) skip branch would fire if anything were ever prunable. If
    // the plumbing only ever touches the RNG or takes the pruning code
    // path when something is actually prunable, this run must be
    // byte-identical to a `prune = false` run with the same seed.
    let mut pruned = dense_vector_toy_solver_with_prune(seed, true, -1.0e30, 1.0);

    baseline.run_sweeps(30).unwrap();
    pruned.run_sweeps(30).unwrap();

    let mut baseline_state = baseline.snapshot_state();
    let mut pruned_state = pruned.snapshot_state();
    // Only the prune knobs themselves are expected to differ between
    // the two configs; strip them before comparing so the assertion is
    // about the actual arena contents (regrets, strategy sums, etc.),
    // not the config echo.
    baseline_state.config.prune = false;
    pruned_state.config.prune = false;
    baseline_state.config.prune_threshold = 0.0;
    pruned_state.config.prune_threshold = 0.0;
    baseline_state.config.prune_skip_probability = 0.0;
    pruned_state.config.prune_skip_probability = 0.0;
    assert_eq!(baseline_state, pruned_state);
}

#[test]
fn pruned_bucket_action_gets_no_update_and_others_are_unchanged() {
    let seed = 31337;
    // Node 0 is the public tree root (`DenseToyGame::First`, player 0's
    // only decision); bucket 0 is one of its two combo buckets
    // (`world.hole_combo(actor) % 2`).
    let node_id: NodeId = 0;
    let poisoned_bucket: BucketId = 0;
    // Action 0 ("best") has a regret-matched probability of exactly
    // zero (all mass on action 1) and a regret below the `-10.0`
    // threshold but above the `1.05 * threshold = -10.5` floor, so it is
    // a pruning candidate that the floor clamp must not disturb.
    let poisoned_regrets = [-10.4_f32, 5.0_f32];

    let mut pruned =
        dense_vector_toy_solver_with_prune(seed, true, -10.0, /* skip_probability */ 1.0);
    let mut unpruned = dense_vector_toy_solver_with_prune(seed, false, -10.0, 1.0);

    for solver in [&mut pruned, &mut unpruned] {
        let dense = solver
            .dense
            .as_mut()
            .expect("street recall builds a dense arena");
        let range = dense
            .arena
            .slot_range(node_id, poisoned_bucket)
            .expect("bucket 0 is touched by DenseToyGame's two-bucket abstraction");
        dense.arena.regrets[range].copy_from_slice(&poisoned_regrets);
    }

    pruned.run_sweeps(1).unwrap();
    unpruned.run_sweeps(1).unwrap();

    let pruned_dense = pruned.dense.as_ref().unwrap();
    let unpruned_dense = unpruned.dense.as_ref().unwrap();
    let pruned_range = pruned_dense
        .arena
        .slot_range(node_id, poisoned_bucket)
        .unwrap();
    let unpruned_range = unpruned_dense
        .arena
        .slot_range(node_id, poisoned_bucket)
        .unwrap();

    // (a) The poisoned (bucket, action) regret slot changed in the
    // unpruned run (a real sweep was played against it) but is
    // unchanged in the pruned run (the visit that would have updated it
    // skipped that (bucket, action) pair entirely).
    assert_ne!(unpruned_dense.arena.regrets[unpruned_range][0], -10.4);
    assert_eq!(pruned_dense.arena.regrets[pruned_range][0], -10.4);

    // (b) A pruning coin is drawn from the same `rng` stream used for
    // opponent action sampling, so a pruned run consumes an extra draw
    // and everything downstream of it (including other buckets' regret
    // updates) is free to diverge from the unpruned run -- exact
    // equality on other buckets would be testing RNG-stream luck, not
    // the pruning contract. What must hold regardless is that the
    // pruned run is still a valid, finite state.
    for &value in &pruned_dense.arena.regrets {
        assert!(value.is_finite());
    }
    for &value in &pruned_dense.arena.strategy_sum {
        assert!(value.is_finite());
    }
}

#[test]
fn vector_traverser_accepts_full_recall_game_with_sparse_storage() {
    let result = MultiwaySolver::new(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            traverser_vector: true,
            ..SolverConfig::default()
        },
    );
    let mut solver = result.expect("full-recall vector mode is supported");
    assert!(solver.dense.is_none());
    solver.run_sweeps(1).unwrap();
    assert!(solver.hand_updates > solver.traversals);
}

#[test]
fn prune_requires_vector_traverser() {
    let result = MultiwaySolver::new(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            traverser_vector: false,
            prune: true,
            prune_threshold: -1.0,
            prune_skip_probability: 0.5,
            ..SolverConfig::default()
        },
    );
    assert!(matches!(result, Err(SolverError::PruneRequiresVector)));
}

#[test]
fn prune_requires_street_recall() {
    let result = MultiwaySolver::new(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            traverser_vector: true,
            prune: true,
            prune_threshold: -1.0,
            prune_skip_probability: 0.5,
            ..SolverConfig::default()
        },
    );
    assert!(matches!(
        result,
        Err(SolverError::PruneRequiresStreetRecall)
    ));
}

#[test]
fn production_preallocation_rejects_full_recall_before_any_traversal() {
    let result = MultiwaySolver::new_preallocated(
        DominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig::default(),
    );
    assert!(matches!(
        result,
        Err(SolverError::PreallocatedStorageRequiresStreetRecall)
    ));
}

#[test]
fn prune_threshold_must_be_finite_and_negative() {
    let result = MultiwaySolver::new(
        DenseDominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            traverser_vector: true,
            prune: true,
            prune_threshold: 0.0,
            prune_skip_probability: 0.5,
            ..SolverConfig::default()
        },
    );
    assert!(matches!(
        result,
        Err(SolverError::PruneThresholdNotNegative(threshold)) if threshold == 0.0
    ));
}

#[test]
fn prune_skip_probability_must_be_in_unit_range() {
    let result = MultiwaySolver::new(
        DenseDominatedChoice,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            traverser_vector: true,
            prune: true,
            prune_threshold: -1.0,
            prune_skip_probability: 1.5,
            ..SolverConfig::default()
        },
    );
    assert!(matches!(
        result,
        Err(SolverError::PruneSkipProbabilityOutOfRange(probability)) if probability == 1.5
    ));
}

#[test]
fn vector_traverser_hand_updates_exceed_one_and_matches_scalar_metrics_shape() {
    let mut vector = dense_vector_toy_solver(77, 1);
    vector.run_sweeps_with_threads(6, 1).unwrap();
    let metrics = vector.metrics();
    // Every traversal's feasible set has close to (but at most) `C(50,
    // 2)` combos (full range minus the ~7 dead cards each world deals);
    // six sweeps of two traversals each is comfortably more than one
    // hand update per traversal.
    assert!(metrics.hand_updates > metrics.traversals);

    let mut scalar = dense_toy_solver(77, 1);
    scalar.run_sweeps_with_threads(6, 1).unwrap();
    // The scalar algorithm updates exactly one hand per traversal.
    assert_eq!(scalar.metrics().hand_updates, scalar.metrics().traversals);
}

#[test]
fn vector_conditional_weights_match_the_scalar_chance_expectation() {
    use std::collections::BTreeMap;

    let combo_a = cards::combo_index("As".parse().unwrap(), "Ah".parse().unwrap());
    let combo_b = cards::combo_index("Ks".parse().unwrap(), "Kh".parse().unwrap());
    let blocked = cards::combo_index("Qs".parse().unwrap(), "Qh".parse().unwrap());
    let opponent = cards::combo_index("Qs".parse().unwrap(), "Jc".parse().unwrap());
    let board = ["2c", "3d", "4h", "5s", "6c"].map(|card| card.parse().unwrap());
    let world = SampledWorld::new(vec![combo_a, opponent], board).unwrap();

    let mut own_range = Range::default();
    own_range.set_weight(combo_a, 0.1);
    own_range.set_weight(combo_b, 0.3);
    own_range.set_weight(blocked, 0.6);
    let mut opponent_range = Range::default();
    opponent_range.set_weight(opponent, 1.0);
    let sampler = DealSampler::new(vec![own_range, opponent_range]).unwrap();
    let feasible = sampler.feasible_combos(0, &world);
    assert_eq!(feasible.len(), 2);
    assert!(feasible.iter().any(|(combo, _)| *combo == combo_a));
    assert!(feasible.iter().any(|(combo, _)| *combo == combo_b));
    assert!(!feasible.iter().any(|(combo, _)| *combo == blocked));

    let config = SolverConfig {
        seed: 9,
        max_memory_bytes: 1 << 20,
        max_traversal_depth: 16,
        exploration_epsilon: 0.0,
        discount_every: DEFAULT_DISCOUNT_EVERY,
        discount_until: DEFAULT_DISCOUNT_UNTIL,
        sweep_batch: 1,
        traverser_vector: true,
        prune: false,
        prune_threshold: DEFAULT_PRUNE_THRESHOLD,
        prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
    };
    let sparse_game = ConditionalWeightGame {
        street_recall: false,
        bucket_zero_combo: combo_a,
    };
    let sparse = MultiwaySolver::new(sparse_game, sampler.clone(), config).unwrap();
    let dense_game = ConditionalWeightGame {
        street_recall: true,
        bucket_zero_combo: combo_a,
    };
    let dense_solver = MultiwaySolver::new(dense_game, sampler, config).unwrap();

    let mut normalized = feasible.clone();
    let mut normalized_weights = normalized
        .iter()
        .map(|(_, weight)| *weight)
        .collect::<Vec<_>>();
    normalize_feasible_weights(&mut normalized_weights).unwrap();
    for ((_, weight), &normalized_weight) in normalized.iter_mut().zip(&normalized_weights) {
        *weight = normalized_weight;
    }

    // Exact conditional expectation of the scalar worker, enumerating the
    // two feasible own hands. Each scalar traversal sees the same initial
    // policy snapshot and its event is weighted by P(hand | context).
    let mut scalar_regrets: BTreeMap<InfoKey, Vec<f64>> = BTreeMap::new();
    for &(combo, weight) in &normalized {
        let combo_world = SampledWorld::new(vec![combo, opponent], board).unwrap();
        let mut worker = TraversalWorker::new(&sparse, 7.0);
        let mut reach = vec![1.0; 2];
        let mut rng = ChaCha20Rng::seed_from_u64(123);
        worker
            .traverse(
                sparse.game.root_state(),
                &combo_world,
                0,
                HistoryKey::ROOT,
                &mut reach,
                1.0,
                &mut rng,
                0,
            )
            .unwrap();
        for event in worker.finish(0, 0, 0).events {
            if let TraversalEvent::AddRegret { key, values } = event {
                let total = scalar_regrets
                    .entry(key)
                    .or_insert_with(|| vec![0.0; values.len()]);
                for (sum, value) in total.iter_mut().zip(values) {
                    *sum += weight * value;
                }
            }
        }
    }

    // The dense vector worker receives raw unequal weights. Its constructor
    // must normalize by the full feasible mass (1+3), and its bucket updates
    // must retain bucket probabilities rather than divide them away.
    let dense = dense_solver.dense.as_ref().unwrap();
    let (combos, raw_weights): (Vec<_>, Vec<_>) = feasible.into_iter().unzip();
    let active = (0..combos.len()).collect::<Vec<_>>();
    let own_reach = vec![1.0; combos.len()];
    let mut worker = VectorTraversalWorker::new(
        &dense_solver.game,
        dense,
        config,
        combos.clone(),
        raw_weights,
    )
    .unwrap();
    let mut rng = ChaCha20Rng::seed_from_u64(123);
    worker
        .traverse(
            dense_solver.game.root_state(),
            0,
            &world,
            0,
            &active,
            1.0,
            &mut rng,
            0,
        )
        .unwrap();
    let mut vector_regrets: BTreeMap<InfoKey, Vec<f64>> = BTreeMap::new();
    for event in worker.finish(0, 0, 0).events {
        let (column, values) = match event {
            DenseEvent::AddRegret { column, values } => (column, values),
            DenseEvent::AddStrategy { .. } => {
                panic!("regret traversal must not update the average policy")
            }
        };
        let mut key = None;
        for node_id in 0..dense.tree.nodes.len() as NodeId {
            for bucket in 0..2 {
                if dense.arena.column_id(node_id, bucket).unwrap() == column {
                    key = Some(dense.info_key_for(node_id, bucket));
                }
            }
        }
        assert!(vector_regrets.insert(key.unwrap(), values).is_none());
    }

    assert_eq!(
        scalar_regrets.keys().collect::<Vec<_>>(),
        vector_regrets.keys().collect::<Vec<_>>()
    );
    for (key, expected) in scalar_regrets {
        let actual = &vector_regrets[&key];
        for (&left, &right) in expected.iter().zip(actual) {
            assert!((left - right).abs() < 1.0e-12, "{key:?}: {left} != {right}");
        }
    }

    // The separate sparse and dense-vector average-policy workers must agree
    // with the same conditional own-hand expectation. At the
    // second own decision, the nontrivial own reach is 1/2 from the root's
    // uniform strategy, so each action receives 7 * weight * 1/2 * 1/2.
    let mut sparse_strategy: BTreeMap<InfoKey, Vec<f64>> = BTreeMap::new();
    for &(combo, weight) in &normalized {
        let combo_world = SampledWorld::new(vec![combo, opponent], board).unwrap();
        let mut average = SparseAverageStrategyWorker::new(&sparse, 7.0);
        average
            .traverse(
                sparse.game.root_state(),
                &combo_world,
                0,
                HistoryKey::ROOT,
                weight,
                &mut ChaCha20Rng::seed_from_u64(456),
                0,
            )
            .unwrap();
        for event in average.finish() {
            if let TraversalEvent::AddStrategy { key, values } = event {
                let total = sparse_strategy
                    .entry(key)
                    .or_insert_with(|| vec![0.0; values.len()]);
                for (sum, value) in total.iter_mut().zip(values) {
                    *sum += value;
                }
            }
        }
    }

    let mut dense_average = DenseAverageStrategyWorker::new(&dense_solver.game, dense, config, 7.0);
    dense_average
        .traverse_vector(
            dense_solver.game.root_state(),
            0,
            &world,
            0,
            &combos,
            &normalized_weights,
            &own_reach,
            &mut ChaCha20Rng::seed_from_u64(456),
            0,
        )
        .unwrap();
    let mut vector_strategy: BTreeMap<InfoKey, Vec<f64>> = BTreeMap::new();
    for event in dense_average.finish() {
        let DenseEvent::AddStrategy { column, values } = event else {
            panic!("average traversal must not update regret")
        };
        let mut key = None;
        for node_id in 0..dense.tree.nodes.len() as NodeId {
            for bucket in 0..2 {
                if dense.arena.column_id(node_id, bucket).unwrap() == column {
                    key = Some(dense.info_key_for(node_id, bucket));
                }
            }
        }
        assert!(vector_strategy.insert(key.unwrap(), values).is_none());
    }

    let mut expected_strategy: BTreeMap<InfoKey, Vec<f64>> = BTreeMap::new();
    for &(combo, weight) in &normalized {
        let bucket = sparse.game.combo_bucket(combo);
        let root = InfoKey {
            history: HistoryKey::ROOT,
            player: 0,
            street: 0,
            active_opponents: 1,
            bucket_path: [bucket, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        };
        for (key, own_reach_at_node) in std::iter::once((root, 1.0)).chain((0..2).map(|action| {
            (
                InfoKey {
                    history: HistoryKey::ROOT.child(0, action),
                    ..root
                },
                0.5,
            )
        })) {
            let values = expected_strategy.entry(key).or_insert_with(|| vec![0.0; 2]);
            for value in values {
                *value += 7.0 * weight * own_reach_at_node * 0.5;
            }
        }
    }
    assert_eq!(
        expected_strategy.keys().collect::<Vec<_>>(),
        vector_strategy.keys().collect::<Vec<_>>()
    );
    for (key, expected) in expected_strategy {
        let sparse_actual = &sparse_strategy[&key];
        let actual = &vector_strategy[&key];
        for ((&left, &middle), &right) in expected.iter().zip(sparse_actual).zip(actual) {
            assert!(
                (left - middle).abs() < 1.0e-12,
                "{key:?}: {left} != {middle}"
            );
            assert!((left - right).abs() < 1.0e-12, "{key:?}: {left} != {right}");
        }
    }
}

#[test]
fn independent_average_pass_tracks_temporal_own_strategy_and_samples_zero_opponent_actions() {
    use std::collections::BTreeMap;

    let mut solver = MultiwaySolver::new(
        AveragePathGame,
        DealSampler::new(vec![Range::full(), Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed: 71,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            exploration_epsilon: 0.0,
            ..SolverConfig::default()
        },
    )
    .unwrap();
    let board = ["2c", "3d", "4h", "5s", "6c"].map(|card| card.parse().unwrap());
    let world = SampledWorld::new(
        vec![
            cards::combo_index("As".parse().unwrap(), "Ah".parse().unwrap()),
            cards::combo_index("Ks".parse().unwrap(), "Kh".parse().unwrap()),
            cards::combo_index("Qs".parse().unwrap(), "Qh".parse().unwrap()),
        ],
        board,
    )
    .unwrap();
    let info_key = |history: HistoryKey, player: u8| InfoKey {
        history,
        player,
        street: 0,
        active_opponents: 2,
        bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
    };
    let column = |labels: [&str; 2], regrets: [f32; 2]| PolicyColumn {
        action_labels: labels.into_iter().map(str::to_string).collect(),
        regrets: regrets.to_vec(),
        strategy_sum: vec![0.0; 2],
    };

    // Both opponents put zero current-policy probability on at least one
    // action. The independent average proposal must still visit all four
    // descendant public histories with positive probability.
    solver.policies.insert(
        info_key(HistoryKey::ROOT, 1),
        column(["first-0", "first-1"], [1.0, -1.0]),
    );
    let mut hero_keys = Vec::new();
    for first in 0..2 {
        let second_history = HistoryKey::ROOT.child(1, first);
        solver.policies.insert(
            info_key(second_history, 2),
            column(["second-0", "second-1"], [-1.0, 1.0]),
        );
        for second in 0..2 {
            let hero_history = second_history.child(2, second);
            let key = info_key(hero_history, 0);
            solver
                .policies
                .insert(key, column(["hero-0", "hero-1"], [1.0, -1.0]));
            hero_keys.push(key);
        }
    }
    assert_eq!(
        solver.policies[&info_key(HistoryKey::ROOT, 1)].current_strategy(),
        vec![1.0, 0.0]
    );

    let run_phase = |solver: &MultiwaySolver<AveragePathGame>, linear_weight: f64| {
        let mut visited = Vec::new();
        let mut sums: BTreeMap<InfoKey, Vec<f64>> = BTreeMap::new();
        for seed in 0..64 {
            let mut worker = SparseAverageStrategyWorker::new(solver, linear_weight);
            worker
                .traverse(
                    solver.game.root_state(),
                    &world,
                    0,
                    HistoryKey::ROOT,
                    1.0,
                    &mut ChaCha20Rng::seed_from_u64(seed),
                    0,
                )
                .unwrap();
            let strategy_events = worker
                .finish()
                .into_iter()
                .filter_map(|event| match event {
                    TraversalEvent::AddStrategy { key, values } => Some((key, values)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(strategy_events.len(), 1);
            let (key, values) = &strategy_events[0];
            visited.push(*key);
            let total = sums.entry(*key).or_insert_with(|| vec![0.0; values.len()]);
            for (sum, &value) in total.iter_mut().zip(values) {
                *sum += value;
            }
        }
        (visited, sums)
    };

    let (first_visited, first_sums) = run_phase(&solver, 1.0);
    assert_eq!(
        first_sums.len(),
        4,
        "all uniform opponent paths need support"
    );
    for (&key, values) in &first_sums {
        assert!(hero_keys.contains(&key));
        assert_eq!(values[1], 0.0);
        assert!(values[0] > 0.0);
    }

    // Reverse both opponents, including which actions have probability zero,
    // and change the averaged seat at the next linear-CFR time. Replaying the
    // same uniform RNG seeds must visit the same exact histories in the same
    // order; a proposal based on current opponent sigma would fail here.
    solver
        .policies
        .get_mut(&info_key(HistoryKey::ROOT, 1))
        .unwrap()
        .regrets = vec![-1.0, 1.0];
    for first in 0..2 {
        solver
            .policies
            .get_mut(&info_key(HistoryKey::ROOT.child(1, first), 2))
            .unwrap()
            .regrets = vec![1.0, -1.0];
    }
    for &key in &hero_keys {
        solver.policies.get_mut(&key).unwrap().regrets = vec![-1.0, 1.0];
    }
    let (second_visited, second_sums) = run_phase(&solver, 2.0);
    assert_eq!(first_visited, second_visited);
    assert_eq!(
        first_sums.keys().collect::<Vec<_>>(),
        second_sums.keys().collect::<Vec<_>>()
    );
    for key in hero_keys {
        let visits = first_sums[&key][0];
        assert_eq!(first_sums[&key], vec![visits, 0.0]);
        assert_eq!(second_sums[&key], vec![0.0, 2.0 * visits]);
    }

    // Integration guard: a seat-0 regret traversal plus its independent
    // average pass may add strategy mass only for seat 0. Reintroducing the
    // old actor!=traverser updates would produce seat-1/2 strategy events.
    let delta = solver.generate_traversal_delta(0, 0, 3.0).unwrap();
    let AnyTraversalDelta::Sparse(delta) = delta else {
        panic!("full recall must produce a sparse delta")
    };
    let strategy_players = delta
        .events
        .iter()
        .filter_map(|event| match event {
            TraversalEvent::AddStrategy { key, .. } => Some(key.player),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!strategy_players.is_empty());
    assert!(strategy_players.iter().all(|&player| player == 0));
}

#[test]
fn enumerate_first_opponent_stratifies_one_layer_and_samples_the_next() {
    let solver = MultiwaySolver::new(
        AveragePathGame,
        DealSampler::new(vec![Range::full(), Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed: 19,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            ..SolverConfig::default()
        },
    )
    .unwrap();
    let board = ["2c", "3d", "4h", "5s", "6c"].map(|card| card.parse().unwrap());
    let world = SampledWorld::new(
        vec![
            cards::combo_index("As".parse().unwrap(), "Ah".parse().unwrap()),
            cards::combo_index("Ks".parse().unwrap(), "Kh".parse().unwrap()),
            cards::combo_index("Qs".parse().unwrap(), "Qh".parse().unwrap()),
        ],
        board,
    )
    .unwrap();
    let strategy_events = |mode| {
        let mut worker = SparseAverageStrategyWorker::with_sampling(&solver, 1.0, mode);
        let mut rng = ChaCha20Rng::seed_from_u64(808);
        worker
            .traverse(
                solver.game.root_state(),
                &world,
                0,
                HistoryKey::ROOT,
                1.0,
                &mut rng,
                0,
            )
            .unwrap();
        let events = worker
            .finish()
            .into_iter()
            .filter_map(|event| match event {
                TraversalEvent::AddStrategy { key, values } => Some((key, values)),
                _ => None,
            })
            .collect::<Vec<_>>();
        (events, rng.next_u64())
    };

    let (uniform, uniform_next_rng) = strategy_events(AverageOpponentSampling::UniformOne);
    let (enumerated, enumerated_next_rng) =
        strategy_events(AverageOpponentSampling::EnumerateFirst);
    assert_eq!(
        uniform_next_rng, enumerated_next_rng,
        "enumeration must leave later averager siblings on the baseline RNG stream"
    );
    assert_eq!(uniform.len(), 1);
    assert_eq!(enumerated.len(), 2);
    assert!(enumerated.contains(&uniform[0]));
    assert!(enumerated.iter().all(|(_, values)| values == &[0.5, 0.5]));

    let first_prefixes = enumerated
        .iter()
        .map(|(key, _)| {
            if (0..2).any(|first| {
                (0..2)
                    .any(|second| key.history == HistoryKey::ROOT.child(1, first).child(2, second))
            }) {
                (0..2)
                    .find(|&first| {
                        (0..2).any(|second| {
                            key.history == HistoryKey::ROOT.child(1, first).child(2, second)
                        })
                    })
                    .unwrap()
            } else {
                panic!("unexpected averaged history: {:?}", key.history)
            }
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(first_prefixes, [0, 1].into_iter().collect());
}

#[test]
fn enumerate_first_dense_vector_keeps_regrets_exact_and_is_thread_deterministic() {
    let run = |threads| {
        let mut solver = dense_vector_toy_solver(919, 2);
        solver
            .run_sweeps_with_threads_until_observed_sampling(
                20,
                threads,
                || true,
                |_| {},
                AverageOpponentSampling::EnumerateFirst,
            )
            .unwrap();
        solver
    };
    let one = run(1);
    let four = run(4);
    let repeated = run(4);
    assert_eq!(one.snapshot_state(), four.snapshot_state());
    assert_eq!(four.snapshot_state(), repeated.snapshot_state());

    let mut uniform = dense_vector_toy_solver(919, 2);
    uniform.run_sweeps_with_threads(20, 4).unwrap();
    assert_eq!(
        uniform.dense.as_ref().unwrap().arena.regrets,
        four.dense.as_ref().unwrap().arena.regrets
    );
    assert_eq!(uniform.total_deal_attempts, four.total_deal_attempts);
    assert_eq!(uniform.terminal_evaluations, four.terminal_evaluations);
    assert_eq!(uniform.hand_updates, four.hand_updates);
    assert_ne!(
        uniform.dense.as_ref().unwrap().arena.strategy_sum,
        four.dense.as_ref().unwrap().arena.strategy_sum
    );
}

#[cfg(feature = "research-average-sampling")]
#[test]
fn consuming_average_sampling_research_api_is_repeatable_and_regret_identical() {
    let run = |variant, threads| {
        dense_vector_toy_solver(920, 2)
            .run_average_sampling_research(AverageSamplingResearchConfig {
                variant,
                sweeps: 20,
                threads,
                histories: vec![HistoryKey::ROOT, HistoryKey::ROOT.child(0, 0)],
                evaluation_samples: 32,
                evaluation_seeds: vec![9090],
                coverage_samples: 32,
                coverage_prefixes: vec![HistoryKey::ROOT, HistoryKey::ROOT.child(0, 0)],
            })
            .unwrap()
    };
    let uniform = run(AverageSamplingResearchVariant::UniformOne, 4);
    let enumerated_one = run(AverageSamplingResearchVariant::EnumerateFirstOpponent, 1);
    let enumerated_four = run(AverageSamplingResearchVariant::EnumerateFirstOpponent, 4);
    let repeated = run(AverageSamplingResearchVariant::EnumerateFirstOpponent, 4);

    assert_eq!(enumerated_one.threads, 1);
    assert_eq!(enumerated_four.threads, 4);
    assert_eq!(enumerated_one.metrics, enumerated_four.metrics);
    assert_eq!(
        enumerated_one.current_regret_fingerprint,
        enumerated_four.current_regret_fingerprint
    );
    assert_eq!(enumerated_one.histories, enumerated_four.histories);
    assert_eq!(enumerated_one.evaluations, enumerated_four.evaluations);
    assert_eq!(
        enumerated_one.coverage_evaluations,
        enumerated_four.coverage_evaluations
    );
    assert_eq!(enumerated_four.metrics, repeated.metrics);
    assert_eq!(enumerated_four.histories, repeated.histories);
    assert_eq!(enumerated_four.evaluations, repeated.evaluations);
    assert_eq!(
        enumerated_four.coverage_evaluations,
        repeated.coverage_evaluations
    );
    assert_eq!(
        enumerated_four.current_regret_fingerprint,
        repeated.current_regret_fingerprint
    );
    for result in [&uniform, &enumerated_one, &enumerated_four, &repeated] {
        assert!(result.solve_elapsed_secs.is_finite());
        assert!(result.solve_elapsed_secs >= 0.0);
    }
    assert_eq!(
        uniform.current_regret_fingerprint,
        enumerated_four.current_regret_fingerprint
    );
    assert_eq!(uniform.histories.len(), 2);
    assert_eq!(enumerated_four.histories.len(), 2);
    assert_eq!(
        uniform.metrics.total_deal_attempts,
        enumerated_four.metrics.total_deal_attempts
    );
    assert_eq!(
        uniform.metrics.hand_updates,
        enumerated_four.metrics.hand_updates
    );
}

#[cfg(feature = "research-average-sampling")]
#[test]
fn postflop_continuation_rejects_full_recall_before_learning() {
    let mut full = solver(925, 1 << 20);
    let before = full.snapshot_state();
    assert!(matches!(
        full.run_average_sampling_research_inner(AverageSamplingResearchConfig {
            variant: AverageSamplingResearchVariant::PostflopContinuation,
            sweeps: 16,
            threads: 2,
            histories: vec![],
            evaluation_samples: 0,
            evaluation_seeds: vec![],
            coverage_samples: 0,
            coverage_prefixes: vec![],
        }),
        Err(SolverError::InvalidState(
            "postflop continuation research requires preallocated current-street storage"
        ))
    ));
    assert_eq!(full.snapshot_state(), before);
    let (game, sampler, config) = initialization_holdem_fixture();
    let mut sparse = MultiwaySolver::new(game, sampler, config).unwrap();
    // Valid Street constructors always create dense storage. Exercise the
    // defensive guard with an intentionally unavailable arena, not by
    // claiming that the ordinary constructor creates sparse Street storage.
    assert!(sparse.dense.is_some());
    sparse.dense = None;
    let before = sparse.snapshot_state();
    assert!(
        sparse
            .run_average_sampling_research_inner(AverageSamplingResearchConfig {
                variant: AverageSamplingResearchVariant::PostflopContinuation,
                sweeps: 16,
                threads: 2,
                histories: vec![],
                evaluation_samples: 0,
                evaluation_seeds: vec![],
                coverage_samples: 0,
                coverage_prefixes: vec![],
            })
            .is_err()
    );
    assert_eq!(sparse.snapshot_state(), before);
}

#[cfg(feature = "research-average-sampling")]
#[test]
fn consuming_average_sampling_research_api_rejects_nonfresh_solver() {
    let mut solver = dense_vector_toy_solver(921, 1);
    solver.run_sweeps(1).unwrap();
    assert!(matches!(
        solver.run_average_sampling_research(AverageSamplingResearchConfig {
            variant: AverageSamplingResearchVariant::EnumerateFirstOpponent,
            sweeps: 1,
            threads: 1,
            histories: vec![HistoryKey::ROOT],
            evaluation_samples: 0,
            evaluation_seeds: Vec::new(),
            coverage_samples: 0,
            coverage_prefixes: Vec::new(),
        }),
        Err(SolverError::InvalidState(
            "average-sampling research requires a fresh solver"
        ))
    ));
}

#[cfg(feature = "research-average-sampling")]
#[test]
fn consuming_average_sampling_research_api_omits_zero_mass_fallbacks() {
    let solver = MultiwaySolver::new(
        AveragePathGame,
        DealSampler::new(vec![Range::full(), Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed: 922,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            ..SolverConfig::default()
        },
    )
    .unwrap();
    let histories = (0..2)
        .flat_map(|first| {
            (0..2).map(move |second| HistoryKey::ROOT.child(1, first).child(2, second))
        })
        .collect();
    let result = solver
        .run_average_sampling_research(AverageSamplingResearchConfig {
            variant: AverageSamplingResearchVariant::UniformOne,
            sweeps: 1,
            threads: 2,
            histories,
            evaluation_samples: 0,
            evaluation_seeds: Vec::new(),
            coverage_samples: 0,
            coverage_prefixes: Vec::new(),
        })
        .unwrap();
    let rows = result
        .histories
        .iter()
        .flat_map(|history| &history.strategies)
        .collect::<Vec<_>>();
    assert!(rows.iter().any(|row| {
        row.status == AverageSamplingResearchRowStatus::AverageObserved && row.actions.is_some()
    }));
    assert!(rows.iter().any(|row| {
        row.status == AverageSamplingResearchRowStatus::ZeroAverageMassOmitted
            && row.actions.is_none()
    }));
    assert!(result.coverage_evaluations.is_empty());
}

#[cfg(feature = "research-average-sampling")]
#[test]
fn average_sampling_research_coverage_preserves_ordinary_baseline_and_state() {
    let config = AverageSamplingResearchConfig {
        variant: AverageSamplingResearchVariant::UniformOne,
        sweeps: 20,
        threads: 2,
        histories: vec![HistoryKey::ROOT],
        evaluation_samples: 32,
        evaluation_seeds: vec![9090, 9091],
        coverage_samples: 32,
        coverage_prefixes: vec![HistoryKey::ROOT, HistoryKey::ROOT.child(0, 0)],
    };
    let mut ordinary = dense_vector_toy_solver(923, 2);
    ordinary
        .run_sweeps_with_threads(config.sweeps, config.threads)
        .unwrap();
    let result = dense_vector_toy_solver(923, 2)
        .run_average_sampling_research(config.clone())
        .unwrap();
    assert_eq!(result.metrics, ordinary.metrics());
    assert_eq!(
        result.current_regret_fingerprint,
        ordinary.research_regret_fingerprint()
    );
    assert_eq!(result.coverage_evaluations.len(), 2);
    for (normal, coverage) in result.evaluations.iter().zip(&result.coverage_evaluations) {
        let expected = ordinary
            .evaluate_profile_with_threads(32, normal.seed, None, ProfileVariant::default(), 2)
            .unwrap();
        assert_eq!(normal.result, expected);
        assert_eq!(coverage.seed, normal.seed);
        let mut expected_baseline = expected;
        expected_baseline.deviation_gain_lower_bound = None;
        assert_eq!(coverage.result.evaluation, expected_baseline);
        let root = &coverage.result.prefixes[0];
        assert_eq!(root.history, HistoryKey::ROOT);
        assert_eq!(root.reached_samples, 32);
        assert_eq!(
            root.candidate_policy_coverage,
            normal.result.candidate_policy_coverage
        );
        assert!(coverage.result.prefixes[1].reached_samples <= root.reached_samples);
    }
    let mut disabled = config;
    disabled.coverage_samples = 0;
    disabled.coverage_prefixes.clear();
    let disabled = dense_vector_toy_solver(923, 2)
        .run_average_sampling_research(disabled)
        .unwrap();
    assert_eq!(disabled.metrics, result.metrics);
    assert_eq!(
        disabled.current_regret_fingerprint,
        result.current_regret_fingerprint
    );
    assert_eq!(disabled.histories, result.histories);
    assert_eq!(disabled.evaluations, result.evaluations);
    assert!(disabled.coverage_evaluations.is_empty());
}

#[cfg(feature = "research-average-sampling")]
#[test]
fn average_sampling_research_coverage_rejects_invalid_settings_before_solving() {
    let base = AverageSamplingResearchConfig {
        variant: AverageSamplingResearchVariant::UniformOne,
        sweeps: 1,
        threads: 1,
        histories: vec![HistoryKey::ROOT],
        evaluation_samples: 32,
        evaluation_seeds: vec![9090],
        coverage_samples: 32,
        coverage_prefixes: vec![HistoryKey::ROOT],
    };
    for (samples, prefixes, error) in [
        (
            0,
            vec![HistoryKey::ROOT],
            "coverage prefixes require at least two coverage samples",
        ),
        (
            1,
            vec![HistoryKey::ROOT],
            "average-sampling research coverage requires at least two samples",
        ),
        (
            32,
            Vec::new(),
            "coverage samples require at least one coverage prefix",
        ),
        (
            32,
            vec![HistoryKey::ROOT; 2],
            "evaluation prefixes must be unique",
        ),
        (
            32,
            vec![HistoryKey([255; 16])],
            "unknown evaluation prefix history",
        ),
        (
            32,
            vec![HistoryKey::ROOT; 65],
            "at most 64 evaluation prefixes are supported",
        ),
    ] {
        let mut config = base.clone();
        config.coverage_samples = samples;
        config.coverage_prefixes = prefixes;
        let mut solver = dense_vector_toy_solver(924, 1);
        // Starting the sweep would fail with DepthLimit instead: these errors
        // must be resolved before the consuming API does expensive work.
        solver.config.max_traversal_depth = 0;
        assert!(matches!(
            solver.run_average_sampling_research(config),
            Err(SolverError::InvalidState(message)) if message == error
        ));
    }
    let mut disabled_evaluation = base;
    disabled_evaluation.evaluation_samples = 0;
    disabled_evaluation.evaluation_seeds.clear();
    assert!(matches!(
        dense_vector_toy_solver(924, 1).run_average_sampling_research(disabled_evaluation),
        Err(SolverError::InvalidState(
            "average-sampling research coverage requires ordinary evaluation"
        ))
    ));
}

#[cfg(feature = "research-average-sampling")]
#[test]
fn average_sampling_research_legacy_config_disables_coverage() {
    let config: AverageSamplingResearchConfig = serde_json::from_value(serde_json::json!({
        "variant": "uniform-one",
        "sweeps": 1,
        "threads": 1,
        "histories": [],
        "evaluation_samples": 0,
        "evaluation_seeds": []
    }))
    .unwrap();
    assert_eq!(config.coverage_samples, 0);
    assert!(config.coverage_prefixes.is_empty());
}

#[test]
fn vector_traverser_is_deterministic_across_thread_counts_and_reruns() {
    let mut single_threaded = dense_vector_toy_solver(4104, 1);
    single_threaded.run_sweeps_with_threads(20, 1).unwrap();

    let mut multi_threaded = dense_vector_toy_solver(4104, 1);
    multi_threaded.run_sweeps_with_threads(20, 8).unwrap();

    assert_eq!(
        single_threaded.snapshot_state(),
        multi_threaded.snapshot_state()
    );
    assert_eq!(single_threaded.metrics(), multi_threaded.metrics());

    let mut rerun = dense_vector_toy_solver(4104, 1);
    rerun.run_sweeps_with_threads(20, 4).unwrap();
    assert_eq!(single_threaded.snapshot_state(), rerun.snapshot_state());

    assert_eq!(
        single_threaded.configuration_fingerprint(),
        multi_threaded.configuration_fingerprint()
    );
    assert_eq!(
        single_threaded.abstraction_fingerprint(),
        multi_threaded.abstraction_fingerprint()
    );
    let directory = tempfile::tempdir().unwrap();
    let single_path = directory.path().join("single.mwckpt");
    let parallel_path = directory.path().join("parallel.mwckpt");
    crate::checkpoint::MultiwayCheckpoint::capture(&single_threaded)
        .write_atomic(&single_path)
        .unwrap();
    crate::checkpoint::MultiwayCheckpoint::capture(&multi_threaded)
        .write_atomic(&parallel_path)
        .unwrap();
    assert_eq!(
        std::fs::read(single_path).unwrap(),
        std::fs::read(parallel_path).unwrap(),
        "inner regret/average scheduling must not change checkpoint bytes"
    );
}

#[test]
fn dense_vector_delta_keeps_regret_events_before_average_events() {
    let solver = dense_vector_toy_solver(811, 1);
    let delta = solver.generate_traversal_delta(0, 0, 1.0).unwrap();
    let AnyTraversalDelta::Dense(delta) = delta else {
        panic!("street-recall toy must generate a dense delta")
    };
    let first_strategy = delta
        .events
        .iter()
        .position(|event| matches!(event, DenseEvent::AddStrategy { .. }))
        .expect("average traversal must emit strategy events");
    assert!(
        first_strategy > 0,
        "regret traversal must emit events first"
    );
    assert!(
        delta.events[..first_strategy]
            .iter()
            .all(|event| matches!(event, DenseEvent::AddRegret { .. }))
    );
    assert!(
        delta.events[first_strategy..]
            .iter()
            .all(|event| matches!(event, DenseEvent::AddStrategy { .. }))
    );
}

#[test]
fn vector_traverser_sweep_batch_is_internally_consistent() {
    let mut batch_four_a = dense_vector_toy_solver(606, 4);
    batch_four_a.run_sweeps_with_threads(12, 1).unwrap();
    let mut batch_four_b = dense_vector_toy_solver(606, 4);
    batch_four_b.run_sweeps_with_threads(12, 6).unwrap();
    assert_eq!(batch_four_a.snapshot_state(), batch_four_b.snapshot_state());
}

#[test]
fn resume_rejects_mismatched_traverser_vector() {
    let state = dense_toy_solver(21, 1).snapshot_state();
    let mut changed = state.config;
    changed.traverser_vector = true;
    assert!(matches!(
        MultiwaySolver::from_state_with_config(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state,
            changed,
        ),
        Err(SolverError::ResumeConfigurationMismatch)
    ));
}

#[test]
fn resume_rejects_pre_conditional_weight_solver_state() {
    let solver = dense_vector_toy_solver(22, 1);
    let mut state = solver.snapshot_state();
    state.schema_version = 2;
    assert!(matches!(
        MultiwaySolver::from_state(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state,
        ),
        Err(SolverError::StateVersion { found: 2, expected })
            if expected == SOLVER_STATE_VERSION
    ));
}

#[test]
fn resume_rejects_street_only_bucket_cache_solver_state() {
    let solver = dense_vector_toy_solver(23, 1);
    let mut state = solver.snapshot_state();
    state.schema_version = 3;
    assert!(matches!(
        MultiwaySolver::from_state(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            state,
        ),
        Err(SolverError::StateVersion { found: 3, expected })
            if expected == SOLVER_STATE_VERSION
    ));
}

#[test]
fn vector_traverser_checkpoint_v5_round_trip_and_resume() {
    let mut solver = dense_vector_toy_solver(303, 1);
    solver.run_sweeps_with_threads(10, 2).unwrap();

    let checkpoint = crate::checkpoint::MultiwayCheckpoint::capture(&solver);
    assert_eq!(
        checkpoint.header.version,
        crate::checkpoint::CHECKPOINT_VERSION
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vector.mwckpt");
    checkpoint.write_atomic(&path).unwrap();
    let loaded = crate::checkpoint::MultiwayCheckpoint::load(
        &path,
        solver.configuration_fingerprint(),
        solver.abstraction_fingerprint(),
    )
    .unwrap();
    assert_eq!(loaded.state, solver.snapshot_state());
    assert!(loaded.state.config.traverser_vector);
    assert!(loaded.state.hand_updates > 0);

    let mut resumed = MultiwaySolver::from_state(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        loaded.state,
    )
    .unwrap();
    resumed.run_sweeps_with_threads(5, 2).unwrap();

    let mut reference = dense_vector_toy_solver(303, 1);
    reference.run_sweeps_with_threads(15, 2).unwrap();
    assert_eq!(resumed.snapshot_state(), reference.snapshot_state());
}

#[test]
fn production_resume_rebuilds_and_commits_the_complete_arena_before_returning() {
    let mut original = dense_vector_toy_solver(919, 1);
    original.run_sweeps_with_threads(12, 2).unwrap();
    let state = original.snapshot_state();
    let expected_state = state.clone();
    let config = state.config;

    let mut resumed = MultiwaySolver::from_state_with_config_preallocated(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        state,
        config,
    )
    .unwrap();

    let allocation = resumed.policy_arena_allocation().unwrap();
    assert!(allocation.pages_committed);
    assert_eq!(resumed.completed_sweeps(), 12);
    assert_eq!(resumed.snapshot_state(), expected_state);

    let dense_before = resumed.dense.as_ref().unwrap();
    let regrets_layout = (
        dense_before.arena.regrets.as_ptr(),
        dense_before.arena.regrets.len(),
        dense_before.arena.regrets.capacity(),
    );
    let strategy_layout = (
        dense_before.arena.strategy_sum.as_ptr(),
        dense_before.arena.strategy_sum.len(),
        dense_before.arena.strategy_sum.capacity(),
    );
    let touched_layout = dense_before.arena.touched_storage_layout();

    resumed.run_sweeps_with_threads(5, 2).unwrap();
    original.run_sweeps_with_threads(5, 2).unwrap();
    assert_eq!(resumed.snapshot_state(), original.snapshot_state());
    let dense_after = resumed.dense.as_ref().unwrap();
    assert_eq!(
        (
            dense_after.arena.regrets.as_ptr(),
            dense_after.arena.regrets.len(),
            dense_after.arena.regrets.capacity(),
        ),
        regrets_layout
    );
    assert_eq!(
        (
            dense_after.arena.strategy_sum.as_ptr(),
            dense_after.arena.strategy_sum.len(),
            dense_after.arena.strategy_sum.capacity(),
        ),
        strategy_layout
    );
    assert_eq!(dense_after.arena.touched_storage_layout(), touched_layout);
    assert_eq!(resumed.policy_arena_allocation().unwrap(), allocation);
}

#[test]
fn production_resume_rejects_duplicate_dense_checkpoint_entries() {
    let mut original = dense_vector_toy_solver(920, 1);
    original.run_sweeps_with_threads(2, 1).unwrap();
    let state = original.snapshot_state();
    assert!(!state.histories.is_empty());
    assert!(!state.policies.is_empty());

    let mut duplicate_history = state.clone();
    duplicate_history
        .histories
        .push(duplicate_history.histories[0].clone());
    let config = duplicate_history.config;
    assert!(matches!(
        MultiwaySolver::from_state_with_config_preallocated(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            duplicate_history,
            config,
        ),
        Err(SolverError::DuplicateHistory(_))
    ));

    let mut duplicate_policy = state;
    duplicate_policy
        .policies
        .push(duplicate_policy.policies[0].clone());
    let config = duplicate_policy.config;
    assert!(matches!(
        MultiwaySolver::from_state_with_config_preallocated(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            duplicate_policy,
            config,
        ),
        Err(SolverError::DuplicatePolicy(_))
    ));
}

#[test]
fn street_recall_preflight_fails_before_allocating_when_memory_limit_is_tiny() {
    let result = MultiwaySolver::new(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            seed: 1,
            max_memory_bytes: 1,
            max_traversal_depth: 16,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    );
    let error = match result {
        Ok(_) => panic!("expected a memory-limit preflight error"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SolverError::Tree(TreeError::MemoryLimit { .. })
    ));
}

#[test]
fn production_preallocation_obeys_the_exact_arena_byte_boundary() {
    let measured = crate::tree::preflight_arena(&DenseToyGame, u64::MAX).unwrap();
    let build = |max_memory_bytes| {
        MultiwaySolver::new_preallocated(
            DenseToyGame,
            DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
            SolverConfig {
                seed: 1,
                max_memory_bytes,
                max_traversal_depth: 16,
                exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
                discount_every: DEFAULT_DISCOUNT_EVERY,
                discount_until: DEFAULT_DISCOUNT_UNTIL,
                sweep_batch: 1,
                traverser_vector: false,
                prune: false,
                prune_threshold: DEFAULT_PRUNE_THRESHOLD,
                prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
            },
        )
    };

    let exact = build(measured.estimated_arena_bytes).unwrap();
    let allocation = exact.policy_arena_allocation().unwrap();
    assert_eq!(allocation.bytes, measured.estimated_arena_bytes);
    assert!(allocation.pages_committed);
    assert_eq!(exact.completed_sweeps(), 0);

    assert!(matches!(
        build(measured.estimated_arena_bytes - 1),
        Err(SolverError::Tree(TreeError::MemoryLimit { .. }))
    ));
}

#[test]
fn street_recall_reproduces_the_sparse_dominated_choice_trajectory() {
    // Same single-decision-node shape and seed as `DominatedChoice`'s
    // sparse `dominated_action_disappears_from_current_and_average_policy`
    // test, so this pins the dense traversal/merge math against the
    // sparse path's already-established behavior.
    let mut solver = dense_dominated_solver(1);
    solver.run_sweeps(1).unwrap();
    assert_eq!(
        solver.policy(root_key_street()).unwrap().regrets,
        vec![1.0, -1.0]
    );
    solver.run_sweeps(100).unwrap();
    assert!(solver.current_strategy(root_key_street()).unwrap()[0] > 0.99);
    assert!(solver.average_strategy(root_key_street()).unwrap()[0] > 0.95);
    assert_eq!(
        solver
            .average_action_probabilities(root_key_street())
            .unwrap()[0]
            .action,
        "best"
    );
}

fn root_key_street() -> InfoKey {
    InfoKey {
        history: HistoryKey::ROOT,
        player: 0,
        street: 0,
        active_opponents: 1,
        bucket_path: [0, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
    }
}

#[test]
fn street_recall_snapshot_info_keys_carry_unreached_in_noncurrent_slots() {
    let mut solver = dense_toy_solver(3, 1);
    solver.run_sweeps(30).unwrap();
    let snapshot = solver.snapshot_state();
    assert!(!snapshot.policies.is_empty());
    for entry in &snapshot.policies {
        let street = entry.key.street as usize;
        for (index, &bucket) in entry.key.bucket_path.iter().enumerate() {
            if index == street {
                assert_ne!(bucket, UNREACHED_BUCKET);
            } else {
                assert_eq!(bucket, UNREACHED_BUCKET);
            }
        }
    }
    // Every policy's history must resolve to an ancestor already present
    // in the pruned (ancestors-of-touched) history trie.
    for entry in &snapshot.policies {
        let mut key = entry.key.history;
        while key != HistoryKey::ROOT {
            let found = snapshot.histories.iter().find(|history| history.key == key);
            let Some(found) = found else {
                panic!("missing ancestor history entry for a touched policy")
            };
            key = found.parent;
        }
    }
}

#[test]
fn street_recall_is_deterministic_across_thread_counts() {
    let mut single_threaded = dense_toy_solver(2024, 1);
    single_threaded.run_sweeps_with_threads(48, 1).unwrap();

    let mut multi_threaded = dense_toy_solver(2024, 1);
    multi_threaded.run_sweeps_with_threads(48, 8).unwrap();

    assert_eq!(
        single_threaded.snapshot_state(),
        multi_threaded.snapshot_state()
    );
    assert_eq!(single_threaded.metrics(), multi_threaded.metrics());
}

#[test]
fn street_recall_sweep_batch_one_and_four_are_each_internally_consistent() {
    // `sweep_batch` legitimately trades staleness for parallel
    // efficiency (see `run_sweeps_with_threads_until`'s docs), so a
    // batch-of-4 run is not expected to bit-match a batch-of-1 run; each
    // is checked for thread-count invariance against *itself* instead.
    let mut batch_one_a = dense_toy_solver(909, 1);
    batch_one_a.run_sweeps_with_threads(24, 1).unwrap();
    let mut batch_one_b = dense_toy_solver(909, 1);
    batch_one_b.run_sweeps_with_threads(24, 6).unwrap();
    assert_eq!(batch_one_a.snapshot_state(), batch_one_b.snapshot_state());

    let mut batch_four_a = dense_toy_solver(909, 4);
    batch_four_a.run_sweeps_with_threads(24, 1).unwrap();
    let mut batch_four_b = dense_toy_solver(909, 4);
    batch_four_b.run_sweeps_with_threads(24, 6).unwrap();
    assert_eq!(batch_four_a.snapshot_state(), batch_four_b.snapshot_state());
}

#[test]
fn street_recall_checkpoint_round_trip_and_resume() {
    let mut solver = dense_toy_solver(55, 1);
    solver.run_sweeps_with_threads(30, 2).unwrap();

    let checkpoint = crate::checkpoint::MultiwayCheckpoint::capture(&solver);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dense.mwckpt");
    checkpoint.write_atomic(&path).unwrap();
    let loaded = crate::checkpoint::MultiwayCheckpoint::load(
        &path,
        solver.configuration_fingerprint(),
        solver.abstraction_fingerprint(),
    )
    .unwrap();
    assert_eq!(loaded.state, solver.snapshot_state());

    let mut resumed = MultiwaySolver::from_state(
        DenseToyGame,
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        loaded.state,
    )
    .unwrap();
    assert_eq!(resumed.snapshot_state(), solver.snapshot_state());
    resumed.run_sweeps_with_threads(10, 2).unwrap();

    let mut reference = dense_toy_solver(55, 1);
    reference.run_sweeps_with_threads(40, 2).unwrap();
    assert_eq!(resumed.snapshot_state(), reference.snapshot_state());
    assert_eq!(resumed.metrics(), reference.metrics());
}

#[test]
fn abstraction_fingerprint_domain_separates_street_recall() {
    let backend = DenseToyGame.abstraction_fingerprint();
    assert_eq!(backend, [0; 32]);

    let street = dense_toy_solver(55, 1);
    assert_ne!(street.abstraction_fingerprint(), backend);

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("street-recall.mwckpt");
    crate::checkpoint::MultiwayCheckpoint::capture(&street)
        .write_atomic(&path)
        .unwrap();
    assert!(matches!(
        crate::checkpoint::MultiwayCheckpoint::load(
            &path,
            street.configuration_fingerprint(),
            backend,
        ),
        Err(crate::checkpoint::CheckpointError::AbstractionMismatch)
    ));

    let full = solver(55, 64 * 1024 * 1024);
    assert_eq!(
        full.abstraction_fingerprint(),
        full.game().abstraction_fingerprint(),
        "full recall must keep the historical backend fingerprint"
    );
}

#[test]
fn street_recall_discount_fires_and_matches_sparse_arithmetic() {
    // Same seed/game/config shape as `batched_discount_scales_regret_and_strategy_at_cadence`.
    let mut solver = dense_dominated_solver(1);
    solver.config.discount_every = 2;
    solver.config.discount_until = 5;
    solver.run_sweeps(1).unwrap();
    assert_eq!(
        solver.policy(root_key_street()).unwrap().regrets,
        vec![1.0, -1.0]
    );
    solver.run_sweeps(1).unwrap();
    let column = solver.policy(root_key_street()).unwrap();
    assert_eq!(column.regrets, vec![0.5, -1.5]);
    assert_eq!(column.strategy_sum, vec![1.25, 0.25]);
}

#[test]
fn street_recall_regret_trends_down_as_sweeps_accumulate() {
    let mut solver = dense_toy_solver(31, 1);
    solver.run_sweeps(20).unwrap();
    let early = solver.metrics().average_positive_regret;
    let early_mean = early.iter().sum::<f64>() / early.len() as f64;

    solver.run_sweeps(2_000).unwrap();
    let late = solver.metrics().average_positive_regret;
    let late_mean = late.iter().sum::<f64>() / late.len() as f64;

    assert!(early_mean.is_finite() && early_mean >= 0.0);
    assert!(late_mean.is_finite() && late_mean >= 0.0);
    assert!(
        late_mean < early_mean,
        "expected average positive regret to trend down: early {early_mean}, late {late_mean}"
    );
}

#[test]
fn street_recall_node_children_and_strategies_at_use_the_enumerated_tree() {
    let mut solver = dense_toy_solver(6, 1);
    solver.run_sweeps(50).unwrap();

    let children = solver.node_children(HistoryKey::ROOT);
    assert_eq!(children.len(), 2);
    let mut labels: Vec<&str> = children
        .iter()
        .map(|entry| entry.action_label.as_str())
        .collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["best", "dominated"]);

    for child in &children {
        let resolved = solver
            .history_entry(child.key)
            .expect("child is enumerated");
        assert_eq!(resolved, *child);
    }
    let root = solver
        .public_node_view(HistoryKey::ROOT)
        .expect("dense root metadata");
    assert_eq!(root.street, Street::Preflop);
    assert_eq!(root.actor, 0);
    assert_eq!(root.actions.len(), 2);
    let mut navigable_children = root
        .actions
        .iter()
        .filter_map(|action| match action.destination {
            PublicActionDestination::PreflopDecision(history) => Some(history),
            PublicActionDestination::PostflopBoundary | PublicActionDestination::Terminal => None,
        })
        .collect::<Vec<_>>();
    navigable_children.sort_unstable();
    let mut expected_children = children.iter().map(|child| child.key).collect::<Vec<_>>();
    expected_children.sort_unstable();
    assert_eq!(navigable_children, expected_children);

    let rows = solver.strategies_at(HistoryKey::ROOT);
    assert!(!rows.is_empty());
    for (_, action_labels, probabilities) in &rows {
        assert_eq!(action_labels.len(), probabilities.len());
        let sum: f32 = probabilities.iter().sum();
        assert!((sum - 1.0).abs() < 1e-3, "probabilities summed to {sum}");
    }
}

/// Every toy dense game above only ever reaches `Street::Preflop`
/// (street index `0`), so `validate_private_info`'s street-recall branch
/// (only the *current* street's slot is non-sentinel, including when
/// that street is not `0`) was never exercised by them: an earlier draft
/// of the `RecallMode::Street` branch there reused the full-recall
/// invariant unconditionally and panicked/errored the instant a real,
/// multi-street game reached the flop. This drives a real
/// [`crate::holdem::HoldemGame`] far enough to guarantee flop/turn/river
/// nodes are visited, as a regression guard.
#[test]
fn street_recall_holdem_game_reaches_every_street_without_error() {
    use crate::abstraction::FeatureHashAbstraction;
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
        SeatConfig, UtilityConfig,
    };
    use crate::holdem::HoldemGame;
    use crate::types::SeatId;

    let mut config = MultiwayConfig {
        seats: (0..3)
            .map(|_| SeatConfig {
                name: None,
                // Shallow stacks so a meaningful fraction of sampled
                // hands actually reach the turn/river instead of
                // resolving preflop.
                stack_bb: 6.0,
                range: String::new(),
                betting: None,
            })
            .collect(),
        button: SeatId(0),
        blinds: BlindConfig::default(),
        ante: AnteConfig::None,
        betting: BettingConfig::default(),
        forced_bets: None,
        abstraction: AbstractionConfig::default(),
    };
    config.abstraction.flop_buckets = 4;
    config.abstraction.turn_buckets = 4;
    config.abstraction.river_buckets = 4;
    config.abstraction.recall = RecallMode::Street;

    let game = HoldemGame::new(
        &config,
        &UtilityConfig::ChipEv,
        &RakeConfig::None,
        FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        })
        .unwrap(),
    )
    .unwrap();
    let sampler = game.deal_sampler().unwrap();
    let mut solver = MultiwaySolver::new_preallocated(
        game,
        sampler,
        SolverConfig {
            seed: 4,
            max_memory_bytes: 1 << 24,
            max_traversal_depth: 64,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch: 1,
            traverser_vector: false,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap();
    let allocation_before = solver.policy_arena_allocation().unwrap();
    assert!(allocation_before.pages_committed);
    assert_eq!(solver.completed_sweeps(), 0);
    let dense_before = solver.dense.as_ref().unwrap();
    assert!(
        dense_before
            .tree
            .nodes
            .iter()
            .any(|node| node.street == Street::Flop)
    );
    assert!(
        dense_before
            .tree
            .nodes
            .iter()
            .any(|node| node.street == Street::Turn)
    );
    assert!(
        dense_before
            .tree
            .nodes
            .iter()
            .any(|node| node.street == Street::River)
    );
    let mut postflop_nodes = [0u64; 3];
    let mut postflop_slots = 0u64;
    for (node_index, node) in dense_before.tree.nodes.iter().enumerate() {
        if node.street == Street::Preflop {
            continue;
        }
        postflop_nodes[node.street.index() - 1] += 1;
        let node_id = node_index as NodeId;
        let expected_buckets = solver
            .game
            .bucket_count(node.street, node.bucket_active_opponents);
        assert_eq!(
            dense_before.arena.bucket_count_of(node_id),
            expected_buckets
        );
        for bucket in 0..expected_buckets {
            let slots = dense_before.arena.slot_range(node_id, bucket).unwrap();
            assert_eq!(slots.len(), node.action_labels.len());
            postflop_slots += slots.len() as u64;
        }
    }
    assert!(
        postflop_nodes.iter().all(|&count| count > 0),
        "every postflop street must have preallocated public nodes: {postflop_nodes:?}"
    );
    assert!(postflop_slots > 0);
    assert_eq!(dense_before.arena.touched_count(), 0);
    let regrets_ptr = dense_before.arena.regrets.as_ptr();
    let strategy_ptr = dense_before.arena.strategy_sum.as_ptr();
    let (touched_ptr, touched_len, touched_capacity) = dense_before.arena.touched_storage_layout();
    let regrets_len = dense_before.arena.regrets.len();
    let strategy_len = dense_before.arena.strategy_sum.len();
    let regrets_capacity = dense_before.arena.regrets.capacity();
    let strategy_capacity = dense_before.arena.strategy_sum.capacity();
    solver.run_sweeps_with_threads(200, 4).unwrap();
    let dense_after = solver.dense.as_ref().unwrap();
    assert_eq!(dense_after.arena.regrets.as_ptr(), regrets_ptr);
    assert_eq!(dense_after.arena.strategy_sum.as_ptr(), strategy_ptr);
    assert_eq!(
        dense_after.arena.touched_storage_layout(),
        (touched_ptr, touched_len, touched_capacity)
    );
    assert_eq!(dense_after.arena.regrets.len(), regrets_len);
    assert_eq!(dense_after.arena.strategy_sum.len(), strategy_len);
    assert_eq!(dense_after.arena.regrets.capacity(), regrets_capacity);
    assert_eq!(dense_after.arena.strategy_sum.capacity(), strategy_capacity);
    assert_eq!(solver.policy_arena_allocation().unwrap(), allocation_before);

    let snapshot = solver.snapshot_state();
    assert!(!snapshot.policies.is_empty());
    let mut streets_seen = [false; 4];
    for entry in &snapshot.policies {
        streets_seen[entry.key.street as usize] = true;
        let street = entry.key.street as usize;
        for (index, &bucket) in entry.key.bucket_path.iter().enumerate() {
            if index == street {
                assert_ne!(bucket, UNREACHED_BUCKET);
            } else {
                assert_eq!(bucket, UNREACHED_BUCKET);
            }
        }
    }
    assert!(
        streets_seen.iter().all(|&seen| seen),
        "expected every street to be reached at least once: {streets_seen:?}"
    );
}

#[test]
fn enumerate_first_real_holdem_changes_only_the_average_accumulator() {
    use crate::abstraction::FeatureHashAbstraction;
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
        SeatConfig, UtilityConfig,
    };
    use crate::holdem::HoldemGame;
    use crate::types::SeatId;

    let build = || {
        let mut config = MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 6.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            forced_bets: None,
            abstraction: AbstractionConfig::default(),
        };
        config.abstraction.flop_buckets = 2;
        config.abstraction.turn_buckets = 2;
        config.abstraction.river_buckets = 2;
        config.abstraction.recall = RecallMode::Street;
        let game = HoldemGame::new(
            &config,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
                flop_buckets: 2,
                turn_buckets: 2,
                river_buckets: 2,
            })
            .unwrap(),
        )
        .unwrap();
        let sampler = game.deal_sampler().unwrap();
        MultiwaySolver::new(
            game,
            sampler,
            SolverConfig {
                seed: 922,
                max_memory_bytes: 1 << 24,
                max_traversal_depth: 64,
                sweep_batch: 1,
                traverser_vector: true,
                ..SolverConfig::default()
            },
        )
        .unwrap()
    };

    let mut uniform = build();
    uniform.run_sweeps_with_threads(2, 2).unwrap();
    let mut enumerated = build();
    enumerated
        .run_sweeps_with_threads_until_observed_sampling(
            2,
            2,
            || true,
            |_| {},
            AverageOpponentSampling::EnumerateFirst,
        )
        .unwrap();

    let uniform_dense = uniform.dense.as_ref().unwrap();
    let enumerated_dense = enumerated.dense.as_ref().unwrap();
    assert_eq!(uniform_dense.arena.regrets, enumerated_dense.arena.regrets);
    assert_eq!(uniform.total_deal_attempts, enumerated.total_deal_attempts);
    assert_eq!(
        uniform.terminal_evaluations,
        enumerated.terminal_evaluations
    );
    assert_eq!(uniform.hand_updates, enumerated.hand_updates);
    assert_ne!(
        uniform_dense.arena.strategy_sum,
        enumerated_dense.arena.strategy_sum
    );
}

#[test]
fn real_holdem_vector_bucket_cache_separates_opponent_contexts_on_one_street() {
    use crate::abstraction::{BucketContext, MultiwayAbstraction};
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
        SeatConfig, UtilityConfig,
    };
    use crate::holdem::HoldemGame;
    use crate::types::SeatId;

    #[derive(Clone, Copy)]
    struct OpponentCountBucket;

    impl MultiwayAbstraction for OpponentCountBucket {
        fn num_buckets(&self, _street: Street, _active_opponents: u8) -> u32 {
            MAX_SEATS as u32
        }

        fn bucket(&self, context: BucketContext<'_>) -> BucketId {
            u32::from(context.active_opponents)
        }

        fn fingerprint(&self) -> [u8; 32] {
            [0x91; 32]
        }
    }

    let mut config = MultiwayConfig {
        seats: (0..3)
            .map(|_| SeatConfig {
                name: None,
                stack_bb: 6.0,
                range: String::new(),
                betting: None,
            })
            .collect(),
        button: SeatId(0),
        blinds: BlindConfig::default(),
        ante: AnteConfig::None,
        betting: BettingConfig::default(),
        forced_bets: None,
        abstraction: AbstractionConfig::default(),
    };
    config.abstraction.recall = RecallMode::Street;
    let game = HoldemGame::new(
        &config,
        &UtilityConfig::ChipEv,
        &RakeConfig::None,
        OpponentCountBucket,
    )
    .unwrap();
    let sampler = game.deal_sampler().unwrap();
    let solver = MultiwaySolver::new(
        game,
        sampler,
        SolverConfig {
            seed: 923,
            max_memory_bytes: 1 << 25,
            max_traversal_depth: 64,
            traverser_vector: true,
            ..SolverConfig::default()
        },
    )
    .unwrap();
    let dense = solver.dense.as_ref().unwrap();
    let mut deal_rng = traversal_deal_rng(923, 0, 0);
    let sample = solver.sampler.sample_counted(&mut deal_rng).unwrap();
    let feasible = solver.sampler.feasible_combos(0, &sample.world);
    let (combos, mut weights): (Vec<_>, Vec<_>) = feasible.into_iter().unzip();
    normalize_feasible_weights(&mut weights).unwrap();
    let own_reach = vec![1.0; combos.len()];
    let column_context = dense
        .tree
        .nodes
        .iter()
        .enumerate()
        .flat_map(|(node_index, node)| {
            let node_id = node_index as NodeId;
            (0..dense.arena.bucket_count_of(node_id)).map(move |bucket| {
                (
                    dense.arena.column_id(node_id, bucket).unwrap(),
                    (node.street, node.bucket_active_opponents, bucket),
                )
            })
        })
        .collect::<FxHashMap<_, _>>();

    let mut average_contexts =
        std::array::from_fn::<_, 4, _>(|_| std::collections::BTreeSet::<u8>::new());
    for seed in 0..16 {
        let mut worker = DenseAverageStrategyWorker::with_sampling(
            &solver.game,
            dense,
            solver.config,
            1.0,
            AverageOpponentSampling::UniformOne,
        );
        worker
            .traverse_vector(
                solver.game.root_state(),
                0,
                &sample.world,
                0,
                &combos,
                &weights,
                &own_reach,
                &mut ChaCha20Rng::seed_from_u64(seed),
                0,
            )
            .unwrap();
        for event in worker.finish() {
            let DenseEvent::AddStrategy { column, .. } = event else {
                continue;
            };
            let &(street, active_opponents, bucket) = column_context
                .get(&column)
                .expect("strategy event column belongs to the dense arena");
            if street != Street::Preflop {
                assert_eq!(
                    bucket,
                    u32::from(active_opponents),
                    "average worker reused a bucket table across opponent contexts"
                );
                average_contexts[street.index()].insert(active_opponents);
            }
        }
    }
    assert!(
        average_contexts.iter().any(|contexts| contexts.len() >= 2),
        "fixture must visit two opponent contexts on one postflop street"
    );

    let active = (0..combos.len()).collect::<Vec<_>>();
    let mut regret_contexts =
        std::array::from_fn::<_, 4, _>(|_| std::collections::BTreeSet::<u8>::new());
    for seed in 0..16 {
        let mut worker = VectorTraversalWorker::new(
            &solver.game,
            dense,
            solver.config,
            combos.clone(),
            weights.clone(),
        )
        .unwrap();
        worker
            .traverse(
                solver.game.root_state(),
                0,
                &sample.world,
                0,
                &active,
                1.0,
                &mut ChaCha20Rng::seed_from_u64(seed),
                0,
            )
            .unwrap();
        for event in worker.finish(0, 0, 0).events {
            let DenseEvent::AddRegret { column, .. } = event else {
                continue;
            };
            let &(street, active_opponents, bucket) = column_context
                .get(&column)
                .expect("regret event column belongs to the dense arena");
            if street != Street::Preflop {
                assert_eq!(
                    bucket,
                    u32::from(active_opponents),
                    "regret worker reused a bucket table across opponent contexts"
                );
                regret_contexts[street.index()].insert(active_opponents);
            }
        }
    }
    assert!(
        regret_contexts.iter().any(|contexts| contexts.len() >= 2),
        "fixture must visit two opponent contexts on one postflop street"
    );
}

#[test]
fn vector_traverser_regret_trends_down_as_sweeps_accumulate() {
    let mut solver = dense_vector_toy_solver(31, 1);
    solver.run_sweeps(20).unwrap();
    let early = solver.metrics().average_positive_regret;
    let early_mean = early.iter().sum::<f64>() / early.len() as f64;

    solver.run_sweeps(2_000).unwrap();
    let late = solver.metrics().average_positive_regret;
    let late_mean = late.iter().sum::<f64>() / late.len() as f64;

    assert!(early_mean.is_finite() && early_mean >= 0.0);
    assert!(late_mean.is_finite() && late_mean >= 0.0);
    assert!(
        late_mean < early_mean,
        "expected average positive regret to trend down: early {early_mean}, late {late_mean}"
    );
}

/// Vector mode's average strategy must accumulate densely at a
/// traverser node on the very first sweep it is visited, not just on
/// whichever single hand an opponent traversal happened to sample.
///
/// Before this change, the vector opponent branch pushed `AddStrategy`
/// only for the acting seat's one dealt-hand bucket (mirroring scalar),
/// and the traverser branch pushed none at all. With three seats, the
/// root is visited as "opponent" only on the two sweeps' traversals
/// where a *different* seat is the traverser, each adding mass for
/// exactly one sampled class -- so two sweeps (six traversals) could
/// give the root at most a handful of classes with positive
/// `strategy_sum` under the old behavior. All 169 preflop classes are
/// feasible at the root under full ranges, and this change makes every
/// traverser-seat sweep touch every feasible one of them at once, so
/// after just two sweeps nearly all 169 should already show mass.
#[test]
fn vector_traverser_root_strategy_sum_is_dense_after_few_sweeps() {
    use crate::abstraction::FeatureHashAbstraction;
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
        SeatConfig, UtilityConfig,
    };
    use crate::holdem::HoldemGame;
    use crate::types::SeatId;

    let mut config = MultiwayConfig {
        seats: (0..3)
            .map(|_| SeatConfig {
                name: None,
                stack_bb: 6.0,
                range: String::new(),
                betting: None,
            })
            .collect(),
        button: SeatId(0),
        blinds: BlindConfig::default(),
        ante: AnteConfig::None,
        betting: BettingConfig::default(),
        forced_bets: None,
        abstraction: AbstractionConfig::default(),
    };
    config.abstraction.flop_buckets = 2;
    config.abstraction.turn_buckets = 2;
    config.abstraction.river_buckets = 2;
    config.abstraction.recall = RecallMode::Street;

    let game = HoldemGame::new(
        &config,
        &UtilityConfig::ChipEv,
        &RakeConfig::None,
        FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
            flop_buckets: 2,
            turn_buckets: 2,
            river_buckets: 2,
        })
        .unwrap(),
    )
    .unwrap();
    let sampler = game.deal_sampler().unwrap();
    let mut solver = MultiwaySolver::new(
        game,
        sampler,
        SolverConfig {
            seed: 55,
            max_memory_bytes: 1 << 24,
            max_traversal_depth: 64,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch: 1,
            traverser_vector: true,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap();
    solver.run_sweeps(2).unwrap();

    let snapshot = solver.snapshot_state();
    let root_buckets_with_mass = snapshot
        .policies
        .iter()
        .filter(|entry| entry.key.history == HistoryKey::ROOT)
        .filter(|entry| entry.column.strategy_sum.iter().any(|&value| value > 0.0))
        .count();
    assert!(
        root_buckets_with_mass >= 100,
        "expected dense root strategy_sum coverage close to all 169 classes, found {root_buckets_with_mass}"
    );
}

/// Same shape as [`street_recall_holdem_game_reaches_every_street_without_error`]
/// but with `traverser_vector` enabled: a real, multi-street `HoldemGame`
/// still reaches every street, produces only finite regrets/metrics, and
/// `hand_updates` accumulates faster than `traversals` (the whole point
/// of the mode).
#[test]
fn vector_traverser_holdem_game_reaches_every_street_without_error() {
    use crate::abstraction::FeatureHashAbstraction;
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
        SeatConfig, UtilityConfig,
    };
    use crate::holdem::HoldemGame;
    use crate::types::SeatId;

    let mut config = MultiwayConfig {
        seats: (0..3)
            .map(|_| SeatConfig {
                name: None,
                stack_bb: 6.0,
                range: String::new(),
                betting: None,
            })
            .collect(),
        button: SeatId(0),
        blinds: BlindConfig::default(),
        ante: AnteConfig::None,
        betting: BettingConfig::default(),
        forced_bets: None,
        abstraction: AbstractionConfig::default(),
    };
    config.abstraction.flop_buckets = 4;
    config.abstraction.turn_buckets = 4;
    config.abstraction.river_buckets = 4;
    config.abstraction.recall = RecallMode::Street;

    let game = HoldemGame::new(
        &config,
        &UtilityConfig::ChipEv,
        &RakeConfig::None,
        FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        })
        .unwrap(),
    )
    .unwrap();
    let sampler = game.deal_sampler().unwrap();
    let mut solver = MultiwaySolver::new(
        game,
        sampler,
        SolverConfig {
            seed: 4,
            max_memory_bytes: 1 << 24,
            max_traversal_depth: 64,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch: 1,
            traverser_vector: true,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap();
    solver.run_sweeps_with_threads(60, 4).unwrap();

    let snapshot = solver.snapshot_state();
    assert!(!snapshot.policies.is_empty());
    let mut streets_seen = [false; 4];
    for entry in &snapshot.policies {
        streets_seen[entry.key.street as usize] = true;
        let street = entry.key.street as usize;
        for (index, &bucket) in entry.key.bucket_path.iter().enumerate() {
            if index == street {
                assert_ne!(bucket, UNREACHED_BUCKET);
            } else {
                assert_eq!(bucket, UNREACHED_BUCKET);
            }
        }
    }
    assert!(
        streets_seen.iter().all(|&seen| seen),
        "expected every street to be reached at least once: {streets_seen:?}"
    );
    let metrics = solver.metrics();
    assert!(metrics.hand_updates > metrics.traversals);
    for &regret in &metrics.average_positive_regret {
        assert!(regret.is_finite());
    }
}

/// ICM smoke test: a small vector-traverser + `RecallMode::Street` +
/// `TournamentIcm` solve runs without error and produces finite
/// utilities/regrets. Exercises the terminal ICM path (which is exact
/// for this seat count) inside `terminal_utilities_for_combos`, called
/// up to hundreds of times per traversal instead of once.
#[test]
fn vector_traverser_icm_smoke_test() {
    use crate::abstraction::FeatureHashAbstraction;
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, FieldPlayerConfig,
        MultiwayConfig, RakeConfig, SeatConfig, UtilityConfig,
    };
    use crate::holdem::HoldemGame;
    use crate::types::SeatId;

    let mut config = MultiwayConfig {
        seats: (0..3)
            .map(|_| SeatConfig {
                name: None,
                stack_bb: 8.0,
                range: String::new(),
                betting: None,
            })
            .collect(),
        button: SeatId(0),
        blinds: BlindConfig::default(),
        ante: AnteConfig::None,
        betting: BettingConfig::default(),
        forced_bets: None,
        abstraction: AbstractionConfig::default(),
    };
    config.abstraction.flop_buckets = 3;
    config.abstraction.turn_buckets = 3;
    config.abstraction.river_buckets = 3;
    config.abstraction.recall = RecallMode::Street;

    let utility = UtilityConfig::TournamentIcm {
        outside_field: vec![FieldPlayerConfig {
            name: "field".into(),
            stack_bb: 40.0,
        }],
        payouts: vec![100.0, 60.0, 30.0, 0.0],
        samples: 4_000,
        seed: 11,
    };

    let game = HoldemGame::new(
        &config,
        &utility,
        &RakeConfig::None,
        FeatureHashAbstraction::new(crate::abstraction::FeatureHashParams {
            flop_buckets: 3,
            turn_buckets: 3,
            river_buckets: 3,
        })
        .unwrap(),
    )
    .unwrap();
    let sampler = game.deal_sampler().unwrap();
    let mut solver = MultiwaySolver::new(
        game,
        sampler,
        SolverConfig {
            seed: 9,
            max_memory_bytes: 1 << 24,
            max_traversal_depth: 64,
            exploration_epsilon: DEFAULT_EXPLORATION_EPSILON,
            discount_every: DEFAULT_DISCOUNT_EVERY,
            discount_until: DEFAULT_DISCOUNT_UNTIL,
            sweep_batch: 1,
            traverser_vector: true,
            prune: false,
            prune_threshold: DEFAULT_PRUNE_THRESHOLD,
            prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
        },
    )
    .unwrap();
    solver.run_sweeps_with_threads(15, 2).unwrap();

    let metrics = solver.metrics();
    assert!(metrics.hand_updates >= metrics.traversals);
    for &regret in &metrics.average_positive_regret {
        assert!(regret.is_finite());
    }
    let evaluation = solver.evaluate_average_profile(64, 5).unwrap();
    for seat in &evaluation.seats {
        assert!(seat.mean.is_finite());
    }
}

#[test]
fn strategy_mass_differs_across_buckets_after_a_short_solve() {
    let mut solver = dense_toy_solver(123, 1);
    solver.run_sweeps(200).unwrap();
    let rows = solver.strategies_at_with_mass(HistoryKey::ROOT);
    assert!(rows.len() >= 2, "expected both buckets to be touched");
    let masses: Vec<f64> = rows.iter().map(|(_, _, _, mass)| *mass).collect();
    assert!(masses.iter().all(|&mass| mass > 0.0));
    assert!(
        masses
            .windows(2)
            .any(|pair| (pair[0] - pair[1]).abs() > 1e-9),
        "expected differing masses across buckets, got {masses:?}"
    );

    // `strategies_at` (the unchanged, mass-free API) must still agree
    // exactly with the mass-carrying rows on every other field.
    let without_mass = solver.strategies_at(HistoryKey::ROOT);
    assert_eq!(without_mass.len(), rows.len());
    for ((key, labels, probs), (mass_key, mass_labels, mass_probs, _)) in
        without_mass.iter().zip(&rows)
    {
        assert_eq!(key, mass_key);
        assert_eq!(labels, mass_labels);
        assert_eq!(probs, mass_probs);
    }
}

// Same real, three-seat, shallow-stack Holdem shape as the street-recall
// smoke tests above; small buckets keep constructor/restore checks cheap.
pub(super) fn initialization_holdem_fixture() -> (
    crate::holdem::HoldemGame<crate::abstraction::FeatureHashAbstraction>,
    DealSampler,
    SolverConfig,
) {
    use crate::abstraction::{FeatureHashAbstraction, FeatureHashParams};
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
        SeatConfig, UtilityConfig,
    };
    let game_config = MultiwayConfig {
        seats: (0..3)
            .map(|_| SeatConfig {
                name: None,
                stack_bb: 6.0,
                range: String::new(),
                betting: None,
            })
            .collect(),
        button: crate::types::SeatId(0),
        blinds: BlindConfig::default(),
        ante: AnteConfig::None,
        betting: BettingConfig::default(),
        forced_bets: None,
        abstraction: AbstractionConfig {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
            recall: RecallMode::Street,
            ..AbstractionConfig::default()
        },
    };
    let game = crate::holdem::HoldemGame::new(
        &game_config,
        &UtilityConfig::ChipEv,
        &RakeConfig::None,
        FeatureHashAbstraction::new(FeatureHashParams {
            flop_buckets: 4,
            turn_buckets: 4,
            river_buckets: 4,
        })
        .unwrap(),
    )
    .unwrap();
    let sampler = game.deal_sampler().unwrap();
    let config = SolverConfig {
        seed: 4,
        max_memory_bytes: 1 << 24,
        max_traversal_depth: 64,
        sweep_batch: 2,
        ..SolverConfig::default()
    };
    (game, sampler, config)
}

fn assert_initialization_storage_equal<G: ExternalSamplingGame>(
    actual: &MultiwaySolver<G>,
    expected: &MultiwaySolver<G>,
) {
    assert_eq!(actual.snapshot_state(), expected.snapshot_state());
    assert_eq!(
        actual.policy_arena_allocation(),
        expected.policy_arena_allocation()
    );
    let actual = actual.dense.as_ref().unwrap();
    let expected = expected.dense.as_ref().unwrap();
    assert_eq!(actual.tree, expected.tree);
    assert!(actual.arena.pages_committed());
    assert_eq!(actual.arena.regrets, expected.arena.regrets);
    assert_eq!(actual.arena.strategy_sum, expected.arena.strategy_sum);
    assert_eq!(actual.arena.touched_count(), expected.arena.touched_count());
    for node in 0..actual.tree.nodes.len() as NodeId {
        let buckets = actual.arena.bucket_count_of(node);
        assert_eq!(buckets, expected.arena.bucket_count_of(node));
        for bucket in 0..buckets {
            let column = actual.arena.column_id(node, bucket).unwrap();
            assert_eq!(column, expected.arena.column_id(node, bucket).unwrap());
            assert_eq!(
                actual.arena.slot_range(node, bucket).unwrap(),
                expected.arena.slot_range(node, bucket).unwrap()
            );
            assert_eq!(
                actual.arena.is_touched(column),
                expected.arena.is_touched(column)
            );
        }
    }
}

fn initialization_error<T>(result: Result<T, SolverError>) -> SolverError {
    match result {
        Ok(_) => panic!("expected initialization to reject this configuration"),
        Err(error) => error,
    }
}

#[test]
fn parallel_initialization_holdem_matches_serial_tree_arena_and_checkpoint() {
    let (game, sampler, config) = initialization_holdem_fixture();
    let mut serial = MultiwaySolver::new_preallocated(game, sampler, config).unwrap();
    let initial_state = serial.snapshot_state();
    let allocation = serial.policy_arena_allocation().unwrap();
    assert!(allocation.pages_committed);
    for street in [Street::Preflop, Street::Flop, Street::Turn, Street::River] {
        assert!(
            serial
                .dense
                .as_ref()
                .unwrap()
                .tree
                .nodes
                .iter()
                .any(|node| node.street == street)
        );
    }
    serial.run_sweeps_with_threads(12, 2).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let reference_path = directory.path().join("serial.mwckpt");
    crate::checkpoint::MultiwayCheckpoint::capture(&serial)
        .write_atomic(&reference_path)
        .unwrap();
    let expected_bytes = std::fs::read(reference_path).unwrap();

    for threads in [1, 2, 8, 16] {
        let (game, sampler, config) = initialization_holdem_fixture();
        let mut parallel =
            MultiwaySolver::new_preallocated_with_threads(game, sampler, config, threads).unwrap();
        assert_eq!(parallel.snapshot_state(), initial_state);
        assert_eq!(parallel.policy_arena_allocation().unwrap(), allocation);
        parallel.run_sweeps_with_threads(12, 2).unwrap();
        assert_initialization_storage_equal(&parallel, &serial);
        let path = directory.path().join(format!("parallel-{threads}.mwckpt"));
        crate::checkpoint::MultiwayCheckpoint::capture(&parallel)
            .write_atomic(&path)
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), expected_bytes);
    }
}

#[test]
fn parallel_initialization_resume_adopts_memory_and_preserves_continuation() {
    let (game, sampler, config) = initialization_holdem_fixture();
    let mut original = MultiwaySolver::new_preallocated(game, sampler, config).unwrap();
    original.run_sweeps_with_threads(8, 2).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let original_path = directory.path().join("original.mwckpt");
    crate::checkpoint::MultiwayCheckpoint::capture(&original)
        .write_atomic(&original_path)
        .unwrap();
    let state = crate::checkpoint::MultiwayCheckpoint::load(
        &original_path,
        original.configuration_fingerprint(),
        original.abstraction_fingerprint(),
    )
    .unwrap()
    .state;
    let current_config = SolverConfig {
        max_memory_bytes: original.policy_arena_allocation().unwrap().bytes,
        ..config
    };
    assert!(current_config.max_memory_bytes < state.config.max_memory_bytes);
    let mut expected_restored = state.clone();
    expected_restored.config.max_memory_bytes = current_config.max_memory_bytes;
    let (game, sampler, _) = initialization_holdem_fixture();
    let mut serial = MultiwaySolver::from_state_with_config_preallocated(
        game,
        sampler,
        state.clone(),
        current_config,
    )
    .unwrap();
    assert_eq!(serial.snapshot_state(), expected_restored);
    serial.run_sweeps_with_threads(4, 2).unwrap();
    original.run_sweeps_with_threads(4, 2).unwrap();
    let mut uninterrupted = original.snapshot_state();
    uninterrupted.config.max_memory_bytes = current_config.max_memory_bytes;
    assert_eq!(serial.snapshot_state(), uninterrupted);
    let serial_path = directory.path().join("serial-resumed.mwckpt");
    crate::checkpoint::MultiwayCheckpoint::capture(&serial)
        .write_atomic(&serial_path)
        .unwrap();
    let expected_bytes = std::fs::read(serial_path).unwrap();

    for threads in [1, 2, 8, 16] {
        let (game, sampler, _) = initialization_holdem_fixture();
        let mut resumed = MultiwaySolver::from_state_with_config_preallocated_with_threads(
            game,
            sampler,
            state.clone(),
            current_config,
            threads,
        )
        .unwrap();
        assert_eq!(resumed.snapshot_state(), expected_restored);
        assert!(resumed.policy_arena_allocation().unwrap().pages_committed);
        resumed.run_sweeps_with_threads(4, 2).unwrap();
        assert_initialization_storage_equal(&resumed, &serial);
        let path = directory.path().join(format!("resumed-{threads}.mwckpt"));
        crate::checkpoint::MultiwayCheckpoint::capture(&resumed)
            .write_atomic(&path)
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), expected_bytes);
    }
}

#[test]
fn parallel_initialization_preserves_exact_memory_and_depth_errors() {
    let (game, _, config) = initialization_holdem_fixture();
    let census = tree::preflight_arena_with_limits_and_depth(
        &game,
        tree::MAX_TREE_NODES,
        u64::MAX,
        config.max_traversal_depth,
    )
    .unwrap();
    let exact_bytes = census.estimated_arena_bytes;
    for threads in [1, 2, 8, 16] {
        let (game, sampler, config) = initialization_holdem_fixture();
        let exact = MultiwaySolver::new_preallocated_with_threads(
            game,
            sampler,
            SolverConfig {
                max_memory_bytes: exact_bytes,
                ..config
            },
            threads,
        )
        .unwrap();
        let allocation = exact.policy_arena_allocation().unwrap();
        assert_eq!(allocation.bytes, exact_bytes);
        assert_eq!(allocation.nodes as usize, census.node_count);
        assert!(allocation.pages_committed);
        for (max_memory_bytes, max_traversal_depth) in
            [(exact_bytes - 1, 64), (exact_bytes, 1), (1, 1)]
        {
            let limited = SolverConfig {
                max_memory_bytes,
                max_traversal_depth,
                ..config
            };
            let (game, sampler, _) = initialization_holdem_fixture();
            let expected =
                initialization_error(MultiwaySolver::new_preallocated(game, sampler, limited));
            if max_memory_bytes == exact_bytes {
                assert!(matches!(
                    expected,
                    SolverError::Tree(TreeError::DepthLimit { limit: 1 })
                ));
            } else {
                assert!(matches!(
                    expected,
                    SolverError::Tree(TreeError::MemoryLimit { .. })
                ));
            }
            let (game, sampler, _) = initialization_holdem_fixture();
            let actual = initialization_error(MultiwaySolver::new_preallocated_with_threads(
                game, sampler, limited, threads,
            ));
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        }
    }
}

#[test]
fn parallel_initialization_rejects_zero_threads_and_insufficient_resume_memory() {
    let (game, sampler, config) = initialization_holdem_fixture();
    assert!(matches!(
        MultiwaySolver::new_preallocated_with_threads(game, sampler, config, 0),
        Err(SolverError::ZeroThreads)
    ));
    let (game, sampler, config) = initialization_holdem_fixture();
    let original = MultiwaySolver::new_preallocated(game, sampler, config).unwrap();
    let state = original.snapshot_state();
    let (game, sampler, _) = initialization_holdem_fixture();
    assert!(matches!(
        MultiwaySolver::from_state_with_config_preallocated_with_threads(
            game,
            sampler,
            state.clone(),
            config,
            0,
        ),
        Err(SolverError::ZeroThreads)
    ));
    let limited = SolverConfig {
        max_memory_bytes: original.policy_arena_allocation().unwrap().bytes - 1,
        ..config
    };
    let (game, sampler, _) = initialization_holdem_fixture();
    let expected = initialization_error(MultiwaySolver::from_state_with_config_preallocated(
        game,
        sampler,
        state.clone(),
        limited,
    ));
    assert!(matches!(
        expected,
        SolverError::Tree(TreeError::MemoryLimit { .. })
    ));
    for threads in [1, 2, 8, 16] {
        let (game, sampler, _) = initialization_holdem_fixture();
        let actual = initialization_error(
            MultiwaySolver::from_state_with_config_preallocated_with_threads(
                game,
                sampler,
                state.clone(),
                limited,
                threads,
            ),
        );
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    }
}

#[test]
fn parallel_initialization_uses_requested_pool_instead_of_ambient_pool() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ObservedGame<G> {
        inner: G,
        observed_pool_sizes: Arc<AtomicUsize>,
    }

    impl<G: ExternalSamplingGame> ExternalSamplingGame for ObservedGame<G> {
        type State = G::State;
        type Actions = G::Actions;

        fn num_players(&self) -> usize {
            self.inner.num_players()
        }

        fn root_state(&self) -> Self::State {
            self.inner.root_state()
        }

        fn actor(&self, state: &Self::State) -> Option<usize> {
            self.inner.actor(state)
        }

        fn node_actions(&self, state: &Self::State) -> Self::Actions {
            self.observed_pool_sizes
                .fetch_or(1 << rayon::current_num_threads(), Ordering::Relaxed);
            self.inner.node_actions(state)
        }

        fn num_actions_of(&self, actions: &Self::Actions) -> usize {
            self.inner.num_actions_of(actions)
        }

        fn next_state_with(
            &self,
            state: &Self::State,
            actions: &Self::Actions,
            action_index: usize,
        ) -> Self::State {
            self.inner.next_state_with(state, actions, action_index)
        }

        fn write_action_label(&self, actions: &Self::Actions, index: usize, out: &mut String) {
            self.inner.write_action_label(actions, index, out);
        }

        fn bucket(&self, state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo {
            self.inner.bucket(state, world, actor)
        }

        fn terminal_utilities(
            &self,
            state: &Self::State,
            world: &SampledWorld,
            utilities: &mut [f64],
        ) {
            self.inner.terminal_utilities(state, world, utilities);
        }

        fn recall_mode(&self) -> RecallMode {
            self.inner.recall_mode()
        }

        fn bucket_count(&self, street: Street, active_opponents: u8) -> u32 {
            self.inner.bucket_count(street, active_opponents)
        }

        fn dense_node_context(&self, state: &Self::State) -> DenseNodeContext {
            self.inner.dense_node_context(state)
        }
    }

    let ambient = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();
    ambient.install(|| {
        for requested_threads in [1, 2] {
            let seen = Arc::new(AtomicUsize::new(0));
            let (inner, sampler, config) = initialization_holdem_fixture();
            let game = ObservedGame {
                inner,
                observed_pool_sizes: Arc::clone(&seen),
            };
            let solver = MultiwaySolver::new_preallocated_with_threads(
                game,
                sampler,
                config,
                requested_threads,
            )
            .unwrap();
            assert_eq!(seen.load(Ordering::Relaxed), 1 << requested_threads);
            assert_eq!(rayon::current_num_threads(), 8);
            let state = solver.snapshot_state();
            seen.store(0, Ordering::Relaxed);
            let (inner, sampler, _) = initialization_holdem_fixture();
            let game = ObservedGame {
                inner,
                observed_pool_sizes: Arc::clone(&seen),
            };
            let resumed = MultiwaySolver::from_state_with_config_preallocated_with_threads(
                game,
                sampler,
                state.clone(),
                config,
                requested_threads,
            )
            .unwrap();
            assert_eq!(seen.load(Ordering::Relaxed), 1 << requested_threads);
            assert_eq!(rayon::current_num_threads(), 8);
            assert_eq!(resumed.snapshot_state(), state);
        }
    });
}

#[test]
fn conditional_root_matches_baseline_and_is_deterministic_read_only() {
    let mut solver = prefix_solver(992);
    solver.run_sweeps(4).unwrap();
    for column in solver.policies.values_mut() {
        column.strategy_sum = vec![3.0, 7.0];
        column.regrets = vec![4.0, 1.0];
    }
    let before = solver.snapshot_state();
    for variant in [
        ProfileVariant::default(),
        ProfileVariant {
            use_current_strategy: true,
            purify_threshold: 0.0,
        },
        ProfileVariant {
            use_current_strategy: false,
            purify_threshold: 0.4,
        },
    ] {
        let paths = vec![vec![], vec![0], vec![1]];
        let one = solver
            .evaluate_profile_conditioned(4097, 775, variant, 1, &paths)
            .unwrap();
        let baseline = solver
            .evaluate_profile_coverage(4097, 775, variant, 1, &[])
            .unwrap();
        assert_eq!(
            one.total_deal_attempts,
            baseline.evaluation.total_deal_attempts
        );
        let root = &one.prefixes[0];
        assert_eq!(root.reach_probability.mean, 1.0);
        assert_eq!(root.reach_probability.stderr, 0.0);
        assert!((root.effective_sample_size - 4097.0).abs() < 1e-10);
        for (ours, ordinary) in root.seats.iter().zip(baseline.evaluation.seats) {
            let ours = ours.unwrap();
            assert_eq!(ours.mean, ordinary.mean);
            assert!((ours.stderr - ordinary.stderr).abs() < 1e-14);
        }
        assert_eq!(
            root.coverage_by_street[0].positive_weight_decision_visits,
            8194
        );
        for threads in [2, 8] {
            assert_eq!(
                one,
                solver
                    .evaluate_profile_conditioned(4097, 775, variant, threads, &paths)
                    .unwrap()
            );
        }
        let reversed = solver
            .evaluate_profile_conditioned(4097, 775, variant, 2, &[vec![1], vec![], vec![0]])
            .unwrap();
        assert_eq!(one.prefixes[0], reversed.prefixes[1]);
        assert_eq!(one.prefixes[1], reversed.prefixes[2]);
    }
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn conditional_rare_and_zero_reach_are_distinct_and_exclude_prefix_decisions() {
    let mut solver = prefix_solver(993);
    solver.run_sweeps(4).unwrap();
    for (key, column) in &mut solver.policies {
        column.strategy_sum = if key.history == HistoryKey::ROOT {
            vec![1e-9, 1.0]
        } else {
            vec![1.0, 0.0]
        };
        column.regrets = vec![0.0, 1.0];
    }
    let paths = [vec![0]];
    let rare = solver
        .evaluate_profile_conditioned(257, 778, ProfileVariant::default(), 2, &paths)
        .unwrap();
    let prefix = &rare.prefixes[0];
    assert_eq!(prefix.positive_weight_samples, 257);
    assert_eq!(prefix.reach_probability.mean, f64::from(1e-9_f32));
    assert_eq!(prefix.seats[0].unwrap().mean, 1.0);
    assert_eq!(
        prefix.coverage_by_street[0].positive_weight_decision_visits,
        257
    );
    assert_eq!(prefix.coverage_by_seat[1][0].average_fraction, None);
    assert_eq!(
        prefix.coverage_by_seat[0][0].average_fraction.unwrap().mean,
        1.0
    );
    let zero = solver
        .evaluate_profile_conditioned(
            257,
            778,
            ProfileVariant {
                use_current_strategy: true,
                purify_threshold: 0.0,
            },
            2,
            &paths,
        )
        .unwrap();
    let prefix = &zero.prefixes[0];
    assert_eq!(prefix.positive_weight_samples, 0);
    assert_eq!(prefix.effective_sample_size, 0.0);
    assert!(prefix.seats.iter().all(Option::is_none));
    assert_eq!(prefix.coverage_by_street[0].trajectory_probability, None);
    assert_eq!(prefix.coverage_by_street[0].average_fraction, None);
}

#[test]
fn conditional_street_sources_and_path_validation() {
    let mut solver = four_street_solver(994);
    solver.run_sweeps(1).unwrap();
    solver.policies.retain(|key, _| key.street != 1);
    for (key, column) in &mut solver.policies {
        column
            .strategy_sum
            .fill(if key.street == 2 { 0.0 } else { 1.0 });
    }
    let report = solver
        .evaluate_profile_conditioned(
            17,
            774,
            ProfileVariant::default(),
            2,
            &[vec![0], vec![0, 0, 0]],
        )
        .unwrap();
    let flop = &report.prefixes[0];
    assert_eq!(flop.coverage_by_street[0].average_fraction, None);
    assert_eq!(
        flop.coverage_by_street[1]
            .uniform_fallback_fraction
            .unwrap()
            .mean,
        1.0
    );
    assert_eq!(
        flop.coverage_by_street[2]
            .regret_fallback_fraction
            .unwrap()
            .mean,
        1.0
    );
    assert_eq!(
        flop.coverage_by_street[3].average_fraction.unwrap().mean,
        1.0
    );
    let river = &report.prefixes[1];
    assert_eq!(river.prefix_uniform_fallback_fraction.unwrap().mean, 1.0);
    assert_eq!(river.prefix_regret_fallback_fraction.unwrap().mean, 1.0);
    assert_eq!(river.prefix_current_fraction.unwrap().mean, 0.0);

    assert_eq!(
        river.coverage_by_street[2]
            .trajectory_probability
            .unwrap()
            .mean,
        0.0
    );
    assert_eq!(
        river.coverage_by_street[3]
            .trajectory_probability
            .unwrap()
            .mean,
        1.0
    );
    for paths in [
        vec![],
        vec![vec![], vec![]],
        vec![vec![1]],
        vec![vec![0; 4]],
        vec![vec![0; 5]],
        vec![vec![0]; 65],
    ] {
        assert!(
            solver
                .evaluate_profile_conditioned(2, 0, ProfileVariant::default(), 1, &paths)
                .is_err()
        );
    }
    for (samples, threads) in [(0, 1), (1, 1), (2, 0)] {
        assert!(
            solver
                .evaluate_profile_conditioned(
                    samples,
                    0,
                    ProfileVariant::default(),
                    threads,
                    &[vec![]]
                )
                .is_err()
        );
    }
    assert!(
        solver
            .evaluate_profile_conditioned(
                2,
                0,
                ProfileVariant {
                    purify_threshold: f32::NAN,
                    use_current_strategy: false
                },
                1,
                &[vec![]]
            )
            .is_err()
    );
}

#[derive(Clone, Copy)]
struct ConditionalWorldGame;

impl ExternalSamplingGame for ConditionalWorldGame {
    type State = PrefixState;
    type Actions = PrefixState;
    fn num_players(&self) -> usize {
        2
    }
    fn root_state(&self) -> Self::State {
        PrefixImportanceGame.root_state()
    }
    fn actor(&self, state: &Self::State) -> Option<usize> {
        PrefixImportanceGame.actor(state)
    }
    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }
    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        PrefixImportanceGame.num_actions_of(actions)
    }
    fn next_state_with(
        &self,
        state: &Self::State,
        actions: &Self::Actions,
        index: usize,
    ) -> Self::State {
        PrefixImportanceGame.next_state_with(state, actions, index)
    }
    fn write_action_label(&self, actions: &Self::Actions, index: usize, out: &mut String) {
        PrefixImportanceGame.write_action_label(actions, index, out);
    }
    fn bucket(&self, state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo {
        let mut private = PrefixImportanceGame.bucket(state, world, actor);
        private.bucket_path[0] = (world.hole_combo(0) % 2) as u32;
        private
    }
    fn terminal_utilities(&self, _: &Self::State, world: &SampledWorld, utilities: &mut [f64]) {
        utilities[0] = if world.hole_combo(0).is_multiple_of(2) {
            1.0
        } else {
            5.0
        };
        utilities[1] = -utilities[0];
    }
}

#[test]
fn conditional_correlated_deals_match_independent_weighted_oracle() {
    let config = prefix_solver(996).config;
    let mut solver = MultiwaySolver::new(
        ConditionalWorldGame,
        DealSampler::new(vec![Range::full(); 2]).unwrap(),
        config,
    )
    .unwrap();
    solver.run_sweeps(8).unwrap();
    for (key, column) in &mut solver.policies {
        column.strategy_sum = if key.bucket_path[0] == 0 {
            vec![1.0, 3.0]
        } else {
            vec![3.0, 1.0]
        };
    }
    let n = 4097;
    let report = solver
        .evaluate_profile_conditioned(n, 779, ProfileVariant::default(), 8, &[vec![0]])
        .unwrap();
    let mut weighted = Vec::new();
    let mut weights = Vec::new();
    for id in 0..n {
        let world = solver
            .sampler
            .sample_counted(&mut evaluation_deal_rng(779, id))
            .unwrap()
            .world;
        let even = world.hole_combo(0).is_multiple_of(2);
        let w = if even { 0.25 } else { 0.75 };
        weights.push(w);
        weighted.push(w * if even { 1.0 } else { 5.0 });
    }
    let den = weights.iter().sum::<f64>();
    let mean = weighted.iter().sum::<f64>() / den;
    let residual = weighted
        .iter()
        .zip(&weights)
        .map(|(x, w)| (x - mean * w).powi(2))
        .sum::<f64>();
    let stderr = (n as f64 * residual / (n - 1) as f64).sqrt() / den;
    let actual = report.prefixes[0].seats[0].unwrap();
    assert!((actual.mean - mean).abs() < 1e-12);
    assert!((actual.stderr - stderr).abs() < 1e-12);
    assert!((actual.mean - 4.0).abs() < 0.1);
    assert!(
        (report.prefixes[0].effective_sample_size
            - den.powi(2) / weights.iter().map(|w| w * w).sum::<f64>())
        .abs()
            < 1e-9
    );
}

#[test]
fn zero_target_exploration_touches_dense_column_without_regret_support() {
    // Reuse the win/pass payoffs (1/0), so an omitted importance correction
    // would produce nonzero descendant regrets. DenseToyGame's descendant
    // actions have equal payoffs and would not detect that regression.
    struct DensePrefixImportanceGame;

    impl ExternalSamplingGame for DensePrefixImportanceGame {
        type State = PrefixState;
        type Actions = PrefixState;

        fn num_players(&self) -> usize {
            PrefixImportanceGame.num_players()
        }
        fn root_state(&self) -> Self::State {
            PrefixImportanceGame.root_state()
        }
        fn actor(&self, state: &Self::State) -> Option<usize> {
            PrefixImportanceGame.actor(state)
        }
        fn node_actions(&self, state: &Self::State) -> Self::Actions {
            PrefixImportanceGame.node_actions(state)
        }
        fn num_actions_of(&self, actions: &Self::Actions) -> usize {
            PrefixImportanceGame.num_actions_of(actions)
        }
        fn next_state_with(
            &self,
            state: &Self::State,
            actions: &Self::Actions,
            action_index: usize,
        ) -> Self::State {
            PrefixImportanceGame.next_state_with(state, actions, action_index)
        }
        fn write_action_label(&self, actions: &Self::Actions, index: usize, out: &mut String) {
            PrefixImportanceGame.write_action_label(actions, index, out);
        }
        fn bucket(&self, state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo {
            PrefixImportanceGame.bucket(state, world, actor)
        }
        fn terminal_utilities(
            &self,
            state: &Self::State,
            world: &SampledWorld,
            utilities: &mut [f64],
        ) {
            PrefixImportanceGame.terminal_utilities(state, world, utilities);
        }
        fn recall_mode(&self) -> RecallMode {
            RecallMode::Street
        }
        fn bucket_count(&self, _street: Street, _active_opponents: u8) -> u32 {
            1
        }
        fn dense_node_context(&self, _state: &Self::State) -> DenseNodeContext {
            DenseNodeContext {
                street: Street::Preflop,
                active_opponents: 1,
                bucket_active_opponents: 1,
            }
        }
        fn bucket_for_combo(
            &self,
            _state: &Self::State,
            _world: &SampledWorld,
            _actor: usize,
            _combo: usize,
        ) -> BucketId {
            0
        }
        fn terminal_utilities_for_combos(
            &self,
            state: &Self::State,
            world: &SampledWorld,
            traverser: usize,
            combos: &[usize],
            out: &mut Vec<f64>,
        ) {
            let mut utilities = [0.0; 2];
            self.terminal_utilities(state, world, &mut utilities);
            out.clear();
            out.resize(combos.len(), utilities[traverser]);
        }
    }

    let combos = [("As", "Ah"), ("Ks", "Kh")]
        .map(|(first, second)| cards::combo_index(first.parse().unwrap(), second.parse().unwrap()));
    let board = ["2c", "3d", "4h", "5s", "6c"].map(|card| card.parse().unwrap());
    let world = SampledWorld::new(combos.to_vec(), board).unwrap();
    let ranges = combos
        .iter()
        .map(|&combo| {
            let mut range = Range::default();
            range.set_weight(combo, 1.0);
            range
        })
        .collect();
    let sampler = DealSampler::new(ranges).unwrap();
    let action_rng = ChaCha20Rng::seed_from_u64(73);
    // With epsilon=1 the proposal stays exactly [1/2, 1/2] for both target
    // policies below. The same draw must select the same public branch.
    let (sampled_action, q) = sample_exploratory_action(&[0.5, 0.5], 1.0, &mut action_rng.clone());
    assert_eq!(q, 0.5);

    for traverser_vector in [false, true] {
        let config = SolverConfig {
            exploration_epsilon: 1.0,
            traverser_vector,
            discount_until: 0,
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 16,
            ..SolverConfig::default()
        };
        let mut solver =
            MultiwaySolver::new(DensePrefixImportanceGame, sampler.clone(), config).unwrap();
        let history = HistoryKey::ROOT.child(1, sampled_action);
        let dense = solver.dense.as_ref().unwrap();
        let node_id = dense.tree.by_history[&history];
        let column = dense.arena.column_id(node_id, 0).unwrap();
        let key = dense.info_key_for(node_id, 0);
        assert!(solver.evaluation_policy(key).is_none());

        let mut zero_target_delta = None;
        for target_probability in [1.0, 0.0] {
            let dense = solver.dense.as_mut().unwrap();
            let root_range = dense.arena.slot_range(0, 0).unwrap();
            let mut root_regrets = [1.0_f32; 2];
            root_regrets[sampled_action] = target_probability as f32;
            root_regrets[1 - sampled_action] = (1.0 - target_probability) as f32;
            dense.arena.regrets[root_range].copy_from_slice(&root_regrets);
            let mut rng = action_rng.clone();
            let (delta, values) = if traverser_vector {
                let mut worker = VectorTraversalWorker::new(
                    &solver.game,
                    dense,
                    config,
                    vec![combos[0]],
                    vec![1.0],
                )
                .unwrap();
                let values = worker
                    .traverse(
                        solver.game.root_state(),
                        0,
                        &world,
                        0,
                        &[0],
                        1.0,
                        &mut rng,
                        0,
                    )
                    .unwrap();
                (worker.finish(0, 0, 0), values)
            } else {
                let mut worker = DenseTraversalWorker::new(&solver.game, dense, config, 1.0);
                let value = worker
                    .traverse(
                        solver.game.root_state(),
                        0,
                        &world,
                        0,
                        &mut [1.0; 2],
                        1.0,
                        &mut rng,
                        0,
                    )
                    .unwrap();
                (worker.finish(0, 0, 0), vec![value])
            };
            assert_eq!(delta.terminal_evaluations, 2);
            assert_eq!(values, vec![target_probability]);
            assert_eq!(delta.events.len(), 1);
            let DenseEvent::AddRegret {
                column: actual_column,
                values,
            } = &delta.events[0]
            else {
                panic!("regret traversal must not write average-strategy mass")
            };
            assert_eq!(*actual_column, column);
            // Hero's unweighted differences are [+1/2, -1/2]. The positive
            // control has rho=2; the zero-target branch has rho=0.
            assert_eq!(values, &vec![target_probability, -target_probability]);
            if target_probability == 0.0 {
                zero_target_delta = Some(delta);
            }
        }

        assert!(solver.evaluation_policy(key).is_none());
        // Isolate this worker's regret-only event. The empty second-seat
        // delta satisfies the ordered merge contract without adding an
        // independent average pass that could also touch the descendant.
        solver
            .merge_sweep_dense(vec![
                zero_target_delta.unwrap(),
                DenseTraversalDelta {
                    sample_id: 1,
                    traverser: 1,
                    deal_attempts: 0,
                    terminal_evaluations: 0,
                    hand_updates: 0,
                    events: Vec::new(),
                },
            ])
            .unwrap();
        let dense = solver.dense.as_ref().unwrap();
        assert!(dense.arena.is_touched(column));
        assert_eq!(dense.arena.touched_count(), 1);
        let owned = solver.policy(key).unwrap();
        assert_eq!(owned.regrets, &[0.0, 0.0]);
        assert_eq!(owned.strategy_sum, &[0.0, 0.0]);
        let stored = solver.evaluation_policy(key).unwrap();
        let (strategy, source) = stored.strategy(false).unwrap();
        assert_eq!(source, CandidatePolicySource::RegretFallback);
        assert_eq!(strategy, vec![0.5, 0.5]);
    }
}

#[path = "snapshot_tests.rs"]
mod snapshot_tests;

#[path = "drift_tests.rs"]
mod drift_tests;
