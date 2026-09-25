use super::*;
use cards::Range;

/// Independent arithmetic oracle: choosing the first action at every node
/// earns 4. Restricting changes to preflop also has optimum 4, whereas an
/// unrestricted deviator can choose gamble/continue/take and earn 100.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContinuationState {
    Root,
    Safe,
    Gamble,
    Postflop,
    Terminal(i32),
}

#[derive(Clone, Copy)]
pub(super) struct ContinuationGame {
    forbid_postflop_reference: bool,
}

impl ContinuationState {
    fn street(self) -> Street {
        match self {
            Self::Postflop => Street::Flop,
            Self::Root | Self::Safe | Self::Gamble => Street::Preflop,
            Self::Terminal(_) => panic!("terminal has no private information"),
        }
    }

    fn history(self) -> HistoryKey {
        match self {
            Self::Root => HistoryKey::ROOT,
            Self::Safe => HistoryKey::ROOT.child(0, 0),
            Self::Gamble => HistoryKey::ROOT.child(0, 1),
            Self::Postflop => HistoryKey::ROOT.child(0, 1).child(0, 1),
            Self::Terminal(_) => panic!("terminal history is not needed"),
        }
    }

    fn labels(self) -> [&'static str; 2] {
        match self {
            Self::Root => ["safe", "gamble"],
            Self::Safe => ["four", "two"],
            Self::Gamble => ["exit", "continue"],
            Self::Postflop => ["baseline", "take"],
            Self::Terminal(_) => panic!("terminal has no actions"),
        }
    }

    pub(super) fn key(self, bucket: u32) -> InfoKey {
        let private = continuation_private(self, bucket);
        InfoKey {
            history: self.history(),
            player: 0,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        }
    }
}

fn continuation_private(state: ContinuationState, bucket: u32) -> PrivateInfo {
    PrivateInfo::from_path(
        state.street(),
        1,
        BucketPath {
            preflop: bucket,
            flop: bucket,
            turn: bucket,
            river: bucket,
        },
    )
}

impl ExternalSamplingGame for ContinuationGame {
    type State = ContinuationState;
    type Actions = ContinuationState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        ContinuationState::Root
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        (!matches!(state, ContinuationState::Terminal(_))).then_some(0)
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        usize::from(!matches!(actions, ContinuationState::Terminal(_))) * 2
    }

    fn next_state_with(
        &self,
        state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        assert_eq!(state, actions);
        use ContinuationState::*;
        match (*state, action_index) {
            (Root, 0) => Safe,
            (Root, 1) => Gamble,
            (Safe, 0) => Terminal(4),
            (Safe, 1) => Terminal(2),
            (Gamble, 0) => Terminal(1),
            (Gamble, 1) => Postflop,
            (Postflop, 0) => Terminal(-3),
            (Postflop, 1) => Terminal(100),
            _ => panic!("invalid action"),
        }
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        out.push_str(actions.labels()[action_index]);
    }

    fn bucket(&self, state: &Self::State, _world: &SampledWorld, _actor: usize) -> PrivateInfo {
        continuation_private(*state, 5)
    }

    fn deviation_bucket(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        _actor: usize,
    ) -> PrivateInfo {
        assert!(
            !self.forbid_postflop_reference || *state != ContinuationState::Postflop,
            "preflop-only fit/replay must not inspect the postflop reference partition"
        );
        // The frozen baseline is deliberately absent at this reference key.
        // Looking it up here would yield a uniform postflop strategy, whose
        // value 48.5 incorrectly reverses the preflop root decision.
        continuation_private(*state, 7)
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        let ContinuationState::Terminal(value) = *state else {
            panic!("not terminal")
        };
        utilities[0] = f64::from(value);
        utilities[1] = -f64::from(value);
    }
}

pub(super) fn continuation_solver() -> MultiwaySolver<ContinuationGame> {
    let mut solver = MultiwaySolver::new(
        ContinuationGame {
            forbid_postflop_reference: false,
        },
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 8,
            ..SolverConfig::default()
        },
    )
    .unwrap();
    for state in [
        ContinuationState::Root,
        ContinuationState::Safe,
        ContinuationState::Gamble,
        ContinuationState::Postflop,
    ] {
        solver.policies.insert(
            state.key(5),
            PolicyColumn {
                action_labels: state.labels().map(str::to_owned).to_vec(),
                // Frozen average always selects action 0. Regret-greedy
                // selects 1, so a fallback to the old heuristic is visible.
                regrets: vec![0.0, 1.0],
                strategy_sum: vec![1.0, 0.0],
            },
        );
    }
    solver
}

#[test]
fn preflop_deviation_oracle_all_street_optimum_changes_the_root_action() {
    let solver = continuation_solver();
    let before = solver.snapshot_state();
    let trained = solver
        .train_deviator_with_report(0, 128, 610, ProfileVariant::default())
        .unwrap();
    assert_eq!(trained.policy.actions.len(), 4);
    for (state, action) in [
        (ContinuationState::Root, 1),
        (ContinuationState::Safe, 0),
        (ContinuationState::Gamble, 1),
        (ContinuationState::Postflop, 1),
    ] {
        assert_eq!(trained.policy.actions.get(&state.key(7)), Some(&action));
        assert!(!trained.policy.actions.contains_key(&state.key(5)));
    }
    let replay = solver
        .evaluate_reference_deviators(
            16,
            710,
            &[
                trained.policy,
                DeviatorPolicy {
                    seat: 1,
                    actions: FxHashMap::default(),
                },
            ],
            ProfileVariant::default(),
        )
        .unwrap();
    for world in replay.worlds {
        assert_eq!(world.baseline_utilities, vec![4.0, -4.0]);
        assert_eq!(world.deviating_seat_utilities, vec![100.0, -4.0]);
        assert_eq!(world.gains, vec![96.0, 0.0]);
    }
    assert_eq!(solver.snapshot_state(), before);
}

fn frozen_tables(entries: &[(ContinuationState, u16)]) -> Vec<DeviatorPolicy> {
    vec![
        DeviatorPolicy {
            seat: 0,
            actions: entries
                .iter()
                .map(|&(state, action)| (state.key(7), action))
                .collect(),
        },
        DeviatorPolicy {
            seat: 1,
            actions: FxHashMap::default(),
        },
    ]
}

fn assert_constant_estimate(estimate: &ProfileEstimate, expected: f64) {
    assert_eq!(estimate.mean, expected);
    assert_eq!(estimate.stderr, 0.0);
    assert_eq!(estimate.ci95, [expected, expected]);
}

#[test]
fn preflop_deviation_fit_freezes_postflop_and_fits_zero_own_reach_preflop_nodes() {
    let mut solver = continuation_solver();
    solver.game.forbid_postflop_reference = true;
    let before = solver.snapshot_state();
    let trained = solver
        .train_deviator_core::<true>(0, 128, 610, 0.0, false)
        .unwrap();
    assert_eq!(trained.coverage.traversals, 128);
    assert_eq!(trained.coverage.visited_infosets, 3);
    assert_eq!(trained.coverage.retained_infosets, 3);
    assert_eq!(trained.coverage.total_visits, 3 * 128);
    assert_eq!(trained.coverage.retained_visits, 3 * 128);
    assert_eq!(trained.policy.actions.len(), 3);
    for state in [
        ContinuationState::Root,
        ContinuationState::Safe,
        ContinuationState::Gamble,
    ] {
        assert_eq!(trained.policy.actions.get(&state.key(7)), Some(&0));
        assert!(!trained.policy.actions.contains_key(&state.key(5)));
    }
    // The main profile never enters Gamble. Local fitting still visits its
    // second own decision on every traversal; no own-prefix multiplier or
    // missing baseline source may erase its counterfactual alternatives.
    assert_eq!(
        solver
            .policy(ContinuationState::Root.key(5))
            .unwrap()
            .average_strategy(),
        vec![1.0, 0.0]
    );
    assert!(
        !trained
            .policy
            .actions
            .contains_key(&ContinuationState::Postflop.key(7))
    );
    let mut policies = frozen_tables(&[]);
    policies[0] = trained.policy;
    let held = solver
        .evaluate_frozen_preflop_deviators(16, 710, &policies, ProfileVariant::default(), 2)
        .unwrap();
    assert_constant_estimate(&held.baseline[0], 4.0);
    assert_constant_estimate(&held.deviating[0], 4.0);
    assert_constant_estimate(&held.gains[0], 0.0);
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn preflop_deviation_missing_keys_use_candidate_baseline_not_regret_greedy() {
    let mut solver = continuation_solver();
    solver.game.forbid_postflop_reference = true;
    let before = solver.snapshot_state();
    for (entries, expected, trained, fallback) in [
        (vec![], 4.0, 0, 2),
        (vec![(ContinuationState::Root, 1)], 1.0, 1, 1),
    ] {
        let held = solver
            .evaluate_frozen_preflop_deviators(
                17,
                711,
                &frozen_tables(&entries),
                ProfileVariant::default(),
                2,
            )
            .unwrap();
        assert_constant_estimate(&held.baseline[0], 4.0);
        assert_constant_estimate(&held.deviating[0], expected);
        assert_constant_estimate(&held.gains[0], expected - 4.0);
        assert_eq!(held.coverage[0].trained_action_visits, trained * 17);
        assert_eq!(held.coverage[0].baseline_fallback_visits, fallback * 17);
    }
    // A candidate-keyed entry is not a supported reference-keyed entry.
    let mut wrong_partition = frozen_tables(&[]);
    wrong_partition[0]
        .actions
        .insert(ContinuationState::Root.key(5), 1);
    let held = solver
        .evaluate_frozen_preflop_deviators(17, 711, &wrong_partition, ProfileVariant::default(), 2)
        .unwrap();
    assert_constant_estimate(&held.gains[0], 0.0);
    assert_eq!(held.coverage[0].trained_action_visits, 0);
    assert_eq!(held.coverage[0].baseline_fallback_visits, 34);
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn preflop_deviation_signed_loss_and_fixed_postflop_survive_a_chunk_boundary() {
    let mut solver = continuation_solver();
    solver.game.forbid_postflop_reference = true;
    let before = solver.snapshot_state();
    let policies = frozen_tables(&[(ContinuationState::Root, 1), (ContinuationState::Gamble, 1)]);
    let samples = 4097;
    for threads in [1, 2, 8] {
        let held = solver
            .evaluate_frozen_preflop_deviators(
                samples,
                712,
                &policies,
                ProfileVariant::default(),
                threads,
            )
            .unwrap();
        assert_eq!(held.samples, samples);
        assert_constant_estimate(&held.baseline[0], 4.0);
        assert_constant_estimate(&held.deviating[0], -3.0);
        assert_constant_estimate(&held.gains[0], -7.0);
        assert_constant_estimate(&held.gains[1], 0.0);
        assert_eq!(held.coverage[0].decision_visits, 3 * samples);
        assert_eq!(held.coverage[0].trained_action_visits, 2 * samples);
        assert_eq!(held.coverage[0].baseline_fallback_visits, samples);
        assert_eq!(
            held.coverage[0].trained_action_visits_by_street.preflop,
            2 * samples
        );
        assert_eq!(held.coverage[0].trained_action_visits_by_street.flop, 0);
        assert_eq!(
            held.coverage[0].baseline_fallback_visits_by_street.flop,
            samples
        );
    }
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn preflop_deviation_rejects_postflop_tables_wrong_seats_and_illegal_actions() {
    let solver = continuation_solver();
    let before = solver.snapshot_state();
    let mut wrong_seat = frozen_tables(&[]);
    wrong_seat[0].seat = 1;
    let mut wrong_key_seat = frozen_tables(&[]);
    let mut key = ContinuationState::Root.key(7);
    key.player = 1;
    wrong_key_seat[0].actions.insert(key, 0);
    for policies in [
        frozen_tables(&[(ContinuationState::Postflop, 1)]),
        frozen_tables(&[(ContinuationState::Root, 2)]),
        wrong_seat,
        wrong_key_seat,
        vec![],
    ] {
        assert!(solver
            .evaluate_frozen_preflop_deviators(
                16,
                713,
                &policies,
                ProfileVariant::default(),
                2,
            )
            .is_err());
    }
    assert_eq!(solver.snapshot_state(), before);
}

fn without_elapsed<T: Serialize>(value: &T) -> serde_json::Value {
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                object.remove("elapsedSecs");
                object.remove("fitElapsedSecs");
                for child in object.values_mut() {
                    strip(child);
                }
            }
            serde_json::Value::Array(array) => array.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(value).unwrap();
    strip(&mut value);
    value
}

#[test]
fn preflop_deviation_public_fit_and_held_out_are_separate_and_read_only() {
    let mut solver = continuation_solver();
    solver.game.forbid_postflop_reference = true;
    let before = solver.snapshot_state();
    let config = PreflopDeviationConfig {
        fit_traversals_per_seat: 128,
        fit_seed: 614,
        held_out_samples: 19,
        held_out_seeds: vec![714, 715],
    };
    let first = solver
        .evaluate_preflop_deviation(ProfileVariant::default(), 1, &config)
        .unwrap();
    assert_eq!(first.fit_mode, PreflopDeviationFitMode::LocalRegretMatching);
    let explicit = solver
        .evaluate_preflop_deviation_with_fit_mode(
            ProfileVariant::default(),
            1,
            &config,
            PreflopDeviationFitMode::LocalRegretMatching,
        )
        .unwrap();
    assert_eq!(without_elapsed(&explicit), without_elapsed(&first));
    assert_eq!(first.fit_coverage[0].retained_infosets, 3);
    assert_eq!(first.fit_coverage[1].retained_infosets, 0);
    assert_eq!(first.held_out.len(), 2);
    for (held, &seed) in first.held_out.iter().zip(&config.held_out_seeds) {
        assert_eq!(held.seed, seed);
        assert_eq!(held.samples, config.held_out_samples);
        assert_constant_estimate(&held.gains[0], 0.0);
        assert_constant_estimate(&held.gains[1], 0.0);
    }
    for threads in [2, 8] {
        let repeated = solver
            .evaluate_preflop_deviation(ProfileVariant::default(), threads, &config)
            .unwrap();
        assert_eq!(without_elapsed(&repeated), without_elapsed(&first));
    }
    let gated = solver
        .evaluate_preflop_deviation_with_fit_mode(
            ProfileVariant::default(),
            1,
            &config,
            PreflopDeviationFitMode::RetentionGated,
        )
        .unwrap();
    assert_eq!(gated.fit_mode, PreflopDeviationFitMode::RetentionGated);
    assert_eq!(gated.fit_coverage, first.fit_coverage);
    assert_eq!(gated.fit_policy_fingerprint, first.fit_policy_fingerprint);
    for threads in [2, 8] {
        let repeated = solver
            .evaluate_preflop_deviation_with_fit_mode(
                ProfileVariant::default(),
                threads,
                &config,
                PreflopDeviationFitMode::RetentionGated,
            )
            .unwrap();
        assert_eq!(without_elapsed(&repeated), without_elapsed(&gated));
    }
    let unsupported = solver
        .evaluate_preflop_deviation(
            ProfileVariant::default(),
            2,
            &PreflopDeviationConfig {
                fit_traversals_per_seat: u64::from(MIN_DEVIATOR_POLICY_VISITS - 1),
                ..config
            },
        )
        .unwrap();
    assert_eq!(unsupported.fit_coverage[0].visited_infosets, 3);
    assert_eq!(unsupported.fit_coverage[0].retained_infosets, 0);
    for held in unsupported.held_out {
        assert_constant_estimate(&held.gains[0], 0.0);
        assert_eq!(held.coverage[0].trained_action_visits, 0);
    }
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn preflop_deviation_holdem_stream_matches_independent_world_moments_and_threads() {
    let (game, sampler, config) = super::tests::initialization_holdem_fixture();
    let mut solver = MultiwaySolver::new(game, sampler, config).unwrap();
    solver.run_sweeps(8).unwrap();
    let before = solver.snapshot_state();
    let dense = solver.dense.as_ref().unwrap();
    let mut policies = (0..3)
        .map(|seat| DeviatorPolicy {
            seat,
            actions: FxHashMap::default(),
        })
        .collect::<Vec<_>>();
    for (id, node) in dense.tree.nodes.iter().enumerate() {
        if node.street == Street::Preflop {
            for bucket in 0..dense.arena.bucket_count_of(id as NodeId) {
                policies[node.actor as usize]
                    .actions
                    .insert(dense.info_key_for(id as NodeId, bucket), 0);
            }
        }
    }
    let samples = 4097;
    let seed = 716;
    let variant = ProfileVariant::default();
    let reference = solver
        .evaluate_reference_deviators(samples, seed, &policies, variant)
        .unwrap();
    let first = solver
        .evaluate_frozen_preflop_deviators(samples, seed, &policies, variant, 1)
        .unwrap();
    assert_eq!(first.coverage, reference.coverage);
    assert_eq!(
        first.candidate_policy_coverage,
        reference.candidate_policy_coverage
    );
    // Compute mean and SE independently, in two passes over raw paired
    // world values. The old wrapper's clipped aggregate is not the oracle.
    for seat in 0..3 {
        for (estimates, values) in [
            (
                &first.baseline,
                reference
                    .worlds
                    .iter()
                    .map(|w| w.baseline_utilities[seat])
                    .collect::<Vec<_>>(),
            ),
            (
                &first.deviating,
                reference
                    .worlds
                    .iter()
                    .map(|w| w.deviating_seat_utilities[seat])
                    .collect::<Vec<_>>(),
            ),
            (
                &first.gains,
                reference
                    .worlds
                    .iter()
                    .map(|w| w.gains[seat])
                    .collect::<Vec<_>>(),
            ),
        ] {
            let n = values.len() as f64;
            let mean = values.iter().sum::<f64>() / n;
            let stderr = (values
                .iter()
                .map(|value| (value - mean).powi(2))
                .sum::<f64>()
                / (n - 1.0)
                / n)
                .sqrt();
            assert!((estimates[seat].mean - mean).abs() < 1e-10);
            assert!((estimates[seat].stderr - stderr).abs() < 1e-10);
            assert!((estimates[seat].ci95[0] - (mean - 1.96 * stderr)).abs() < 1e-10);
            assert!((estimates[seat].ci95[1] - (mean + 1.96 * stderr)).abs() < 1e-10);
        }
    }
    for threads in [2, 8] {
        let parallel = solver
            .evaluate_frozen_preflop_deviators(samples, seed, &policies, variant, threads)
            .unwrap();
        assert_eq!(without_elapsed(&parallel), without_elapsed(&first));
    }
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn preflop_deviation_current_and_purified_continuations_use_the_specified_baseline() {
    // The current and average sources deliberately disagree at the same
    // candidate postflop key. A threshold of .5 removes the .4 action and
    // leaves a pure strategy, giving exact terminal values without a Monte
    // Carlo tolerance. Reference postflop lookup is forbidden in every case.
    for (current, threshold, regrets, average, expected, root_action) in [
        (true, 0.0, [0.0, 1.0], [1.0, 0.0], 100.0, 1),
        (false, 0.5, [0.0, 1.0], [3.0, 2.0], -3.0, 0),
        (false, 0.5, [1.0, 0.0], [2.0, 3.0], 100.0, 1),
        (true, 0.5, [3.0, 2.0], [0.0, 1.0], -3.0, 0),
    ] {
        let mut solver = continuation_solver();
        solver.game.forbid_postflop_reference = true;
        for state in [
            ContinuationState::Root,
            ContinuationState::Safe,
            ContinuationState::Gamble,
        ] {
            solver.policies.get_mut(&state.key(5)).unwrap().regrets = vec![1.0, 0.0];
        }
        let postflop = solver
            .policies
            .get_mut(&ContinuationState::Postflop.key(5))
            .unwrap();
        postflop.regrets = regrets.to_vec();
        postflop.strategy_sum = average.to_vec();
        let before = solver.snapshot_state();
        let variant = ProfileVariant {
            use_current_strategy: current,
            purify_threshold: threshold,
        };
        let fit = solver
            .train_deviator_core::<true>(0, 128, 617, threshold, current)
            .unwrap();
        assert_eq!(
            fit.policy.actions.get(&ContinuationState::Root.key(7)),
            Some(&root_action)
        );
        assert_eq!(
            fit.policy.actions.get(&ContinuationState::Gamble.key(7)),
            Some(&root_action)
        );
        assert_eq!(fit.policy.actions.len(), 3);
        let held = solver
            .evaluate_frozen_preflop_deviators(
                128,
                717,
                &frozen_tables(&[(ContinuationState::Root, 1), (ContinuationState::Gamble, 1)]),
                variant,
                2,
            )
            .unwrap();
        assert_constant_estimate(&held.baseline[0], 4.0);
        assert_constant_estimate(&held.deviating[0], expected);
        assert_constant_estimate(&held.gains[0], expected - 4.0);
        assert_eq!(held.coverage[0].trained_action_visits_by_street.flop, 0);
        assert_eq!(
            held.coverage[0].baseline_fallback_visits_by_street.flop,
            128
        );
        assert_eq!(solver.snapshot_state(), before);
    }
}
