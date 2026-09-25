use super::*;
use cards::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const SUPPORT_FIT_TRAVERSALS: u64 = 60;
// Found by the one-time bounded search over seeds 0..64: seed 0 did not
// yield six Child visits, and seed 1 did. Normal tests never search seeds.
pub(super) const SUPPORT_FIT_SEED: u64 = 1;
const BASELINE_BUCKET: u32 = 5;
const REFERENCE_BUCKET: u32 = 7;
// Keep the actual f32 sampling law explicit. The final action receives the
// residual probability, so the first branch has exactly f64::from(0.1f32).
const CHILD_PROBABILITY: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SupportState {
    Root,
    Opponent,
    Child,
    Terminal(i32),
}

#[derive(Clone)]
pub(super) struct SupportGame {
    terminal_counts: Arc<[AtomicU64; 3]>,
}

impl SupportGame {
    fn terminal_counts(&self) -> [u64; 3] {
        std::array::from_fn(|i| self.terminal_counts[i].load(Ordering::Relaxed))
    }
}

impl SupportState {
    fn actor(self) -> usize {
        match self {
            Self::Root | Self::Child => 0,
            Self::Opponent => 1,
            Self::Terminal(_) => panic!("terminal has no actor"),
        }
    }

    fn history(self) -> HistoryKey {
        match self {
            Self::Root => HistoryKey::ROOT,
            Self::Opponent => HistoryKey::ROOT.child(0, 1),
            Self::Child => HistoryKey::ROOT.child(0, 1).child(1, 0),
            Self::Terminal(_) => panic!("terminal history is unused"),
        }
    }

    fn labels(self) -> [&'static str; 2] {
        match self {
            Self::Root => ["safe", "risk"],
            Self::Opponent => ["continue", "finish"],
            Self::Child => ["lose-ten", "win-one"],
            Self::Terminal(_) => panic!("terminal has no actions"),
        }
    }

    fn key(self, bucket: u32) -> InfoKey {
        InfoKey {
            history: self.history(),
            player: self.actor() as u8,
            street: Street::Preflop as u8,
            active_opponents: 1,
            bucket_path: [bucket, UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET],
        }
    }

    pub(super) fn reference_key(self) -> InfoKey {
        self.key(REFERENCE_BUCKET)
    }

    pub(super) fn baseline_key(self) -> InfoKey {
        self.key(BASELINE_BUCKET)
    }
}

impl ExternalSamplingGame for SupportGame {
    type State = SupportState;
    type Actions = SupportState;

    fn num_players(&self) -> usize {
        2
    }

    fn root_state(&self) -> Self::State {
        SupportState::Root
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        match state {
            SupportState::Terminal(_) => None,
            _ => Some(state.actor()),
        }
    }

    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        *state
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        assert!(!matches!(actions, SupportState::Terminal(_)));
        2
    }

    fn next_state_with(
        &self,
        state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        assert_eq!(state, actions);
        use SupportState::*;
        match (*state, action_index) {
            (Root, 0) | (Opponent, 1) => Terminal(0),
            (Root, 1) => Opponent,
            (Opponent, 0) => Child,
            (Child, 0) => Terminal(-10),
            (Child, 1) => Terminal(1),
            _ => panic!("invalid action"),
        }
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        out.push_str(actions.labels()[action_index]);
    }

    fn bucket(&self, state: &Self::State, _world: &SampledWorld, actor: usize) -> PrivateInfo {
        assert_eq!(actor, state.actor());
        PrivateInfo::from_current_bucket(Street::Preflop, 1, BASELINE_BUCKET)
    }

    fn deviation_bucket(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        actor: usize,
    ) -> PrivateInfo {
        assert_eq!(actor, state.actor());
        PrivateInfo::from_current_bucket(Street::Preflop, 1, REFERENCE_BUCKET)
    }

    fn terminal_utilities(
        &self,
        state: &Self::State,
        _world: &SampledWorld,
        utilities: &mut [f64],
    ) {
        let SupportState::Terminal(value) = *state else {
            panic!("not terminal")
        };
        let index = match value {
            0 => 0,
            -10 => 1,
            1 => 2,
            _ => panic!("unknown terminal"),
        };
        self.terminal_counts[index].fetch_add(1, Ordering::Relaxed);
        utilities[0] = f64::from(value);
        utilities[1] = -f64::from(value);
    }
}

pub(super) fn support_solver() -> MultiwaySolver<SupportGame> {
    let mut solver = MultiwaySolver::new(
        SupportGame {
            terminal_counts: Arc::new(std::array::from_fn(|_| AtomicU64::new(0))),
        },
        DealSampler::new(vec![Range::full(), Range::full()]).unwrap(),
        SolverConfig {
            max_memory_bytes: 1 << 20,
            max_traversal_depth: 4,
            ..SolverConfig::default()
        },
    )
    .unwrap();
    for state in [
        SupportState::Root,
        SupportState::Opponent,
        SupportState::Child,
    ] {
        solver.policies.insert(
            state.baseline_key(),
            PolicyColumn {
                action_labels: state.labels().map(str::to_owned).to_vec(),
                // An accidental current/regret-greedy lookup is observable:
                // it avoids Child as the opponent, or wins there as hero.
                regrets: vec![0.0, 1.0],
                strategy_sum: if state == SupportState::Opponent {
                    vec![CHILD_PROBABILITY, 1.0 - CHILD_PROBABILITY]
                } else {
                    vec![1.0, 0.0]
                },
            },
        );
    }
    solver
}

/// Exact full-tree arithmetic, independent of traversal/regret code and cards.
/// A missing reference key uses the frozen action-0 baseline, not the greedy
/// action-1 policy stored in the main regrets. Opponent reach is never fitted.
pub(super) fn support_oracle_value(policy: &DeviatorPolicy) -> f64 {
    assert_eq!(policy.seat, 0);
    let action = |state: SupportState| {
        let action = policy
            .actions
            .get(&state.reference_key())
            .copied()
            .unwrap_or(0);
        assert!(action < 2);
        action
    };
    if action(SupportState::Root) == 0 {
        0.0
    } else {
        let child_utility = if action(SupportState::Child) == 0 {
            -10.0
        } else {
            1.0
        };
        f64::from(CHILD_PROBABILITY) * child_utility
    }
}

pub(super) fn support_policies(policy: DeviatorPolicy) -> Vec<DeviatorPolicy> {
    vec![
        policy,
        DeviatorPolicy {
            seat: 1,
            actions: FxHashMap::default(),
        },
    ]
}

#[test]
fn preflop_support_legacy_fit_loses_after_discarding_the_child_policy() {
    let solver = support_solver();
    let before = solver.snapshot_state();
    let trained = solver
        .train_deviator_core::<true>(0, SUPPORT_FIT_TRAVERSALS, SUPPORT_FIT_SEED, 0.0, false)
        .unwrap();
    // The root's risk action has zero baseline probability, but every own
    // action remains explored. Six counterfactual Child visits are enough for
    // its local fit to win, while still insufficient for final retention.
    assert_eq!(trained.coverage.traversals, 60);
    assert_eq!(trained.coverage.visited_infosets, 2);
    assert_eq!(trained.coverage.retained_infosets, 1);
    assert_eq!(trained.coverage.total_visits, 66);
    assert_eq!(trained.coverage.retained_visits, 60);
    assert_eq!(solver.game.terminal_counts(), [114, 6, 6]);
    assert_eq!(trained.policy.actions.len(), 1);
    assert_eq!(
        trained
            .policy
            .actions
            .get(&SupportState::Root.reference_key()),
        Some(&1)
    );
    assert!(
        !trained
            .policy
            .actions
            .contains_key(&SupportState::Child.reference_key())
    );
    assert!(
        !trained
            .policy
            .actions
            .contains_key(&SupportState::Root.baseline_key())
    );

    // At the first Child visit local RM is uniform: (-10+1)/2=-4.5.
    // Its five later values are +1. Root cumulative regret differences are
    // therefore sum(Vrisk - Vsafe) = -4.5 + 5 = +0.5, so extraction chooses
    // risk. Replacing that learned child with baseline reverses its value.
    let child_visits = trained.coverage.total_visits - SUPPORT_FIT_TRAVERSALS;
    assert_eq!(-4.5 + (child_visits - 1) as f64, 0.5);
    assert_eq!(
        support_oracle_value(&trained.policy),
        -10.0 * f64::from(CHILD_PROBABILITY)
    );
    let mut retained_child = trained.policy.clone();
    retained_child
        .actions
        .insert(SupportState::Child.reference_key(), 1);
    assert_eq!(
        support_oracle_value(&retained_child),
        f64::from(CHILD_PROBABILITY)
    );
    let baseline = DeviatorPolicy {
        seat: 0,
        actions: FxHashMap::default(),
    };
    assert_eq!(support_oracle_value(&baseline), 0.0);

    let held = solver
        .evaluate_frozen_preflop_deviators(
            256,
            820,
            &support_policies(trained.policy),
            ProfileVariant::default(),
            2,
        )
        .unwrap();
    assert_eq!(held.baseline[0].mean, 0.0);
    assert_eq!(held.coverage[0].trained_action_visits, held.samples);
    let child_samples = held.coverage[0].baseline_fallback_visits;
    assert!(child_samples > 0);
    let expected_sample_mean = -10.0 * child_samples as f64 / held.samples as f64;
    assert!((held.gains[0].mean - expected_sample_mean).abs() < 1e-12);
    assert!(held.gains[0].mean < 0.0);
    assert_eq!(held.gains[1].mean, 0.0);
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn preflop_support_retention_gate_keeps_the_safe_parent_without_skipping_own_actions() {
    let solver = support_solver();
    let before = solver.snapshot_state();
    let trained = solver
        .train_deviator_core_with_retention_gate::<true, true>(
            0,
            SUPPORT_FIT_TRAVERSALS,
            SUPPORT_FIT_SEED,
            0.0,
            false,
        )
        .unwrap();
    assert_eq!(trained.coverage.total_visits, 66);
    assert_eq!(trained.coverage.visited_infosets, 2);
    assert_eq!(trained.coverage.retained_visits, 60);
    assert_eq!(trained.coverage.retained_infosets, 1);
    assert_eq!(solver.game.terminal_counts(), [114, 6, 6]);
    assert_eq!(
        trained
            .policy
            .actions
            .get(&SupportState::Root.reference_key()),
        Some(&0)
    );
    assert!(
        !trained
            .policy
            .actions
            .contains_key(&SupportState::Child.reference_key())
    );
    // The gate changes the continuation returned to the parent, not which
    // counterfactual own actions are explored. All six off-baseline children
    // still contribute visits and local action comparisons.
    assert_eq!(support_oracle_value(&trained.policy), 0.0);
    let held = solver
        .evaluate_frozen_preflop_deviators(
            256,
            820,
            &support_policies(trained.policy),
            ProfileVariant::default(),
            2,
        )
        .unwrap();
    assert_eq!(held.gains[0].mean, 0.0);
    assert_eq!(held.gains[0].stderr, 0.0);
    assert_eq!(held.gains[0].ci95, [0.0, 0.0]);
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn preflop_support_retention_gate_uses_candidate_key_baseline_at_the_child() {
    let mut solver = support_solver();
    // The reference bucket has no main policy. Looking the baseline up there
    // would substitute uniform (-4.5), while the actual candidate baseline
    // now wins +1. This must reverse the parent's retained action.
    solver
        .policies
        .get_mut(&SupportState::Child.baseline_key())
        .unwrap()
        .strategy_sum = vec![0.0, 1.0];
    let before = solver.snapshot_state();
    let trained = solver
        .train_deviator_core_with_retention_gate::<true, true>(
            0,
            SUPPORT_FIT_TRAVERSALS,
            SUPPORT_FIT_SEED,
            0.0,
            false,
        )
        .unwrap();
    assert_eq!(trained.coverage.total_visits, 66);
    assert_eq!(trained.coverage.retained_infosets, 1);
    assert_eq!(solver.game.terminal_counts(), [114, 6, 6]);
    assert_eq!(
        trained
            .policy
            .actions
            .get(&SupportState::Root.reference_key()),
        Some(&1)
    );
    assert!(
        !trained
            .policy
            .actions
            .contains_key(&SupportState::Child.reference_key())
    );
    let held = solver
        .evaluate_frozen_preflop_deviators(
            256,
            820,
            &support_policies(trained.policy),
            ProfileVariant::default(),
            2,
        )
        .unwrap();
    let child_samples = held.coverage[0].baseline_fallback_visits;
    assert!(child_samples > 0);
    assert!((held.gains[0].mean - child_samples as f64 / held.samples as f64).abs() < 1e-12);
    assert!(held.gains[0].mean > 0.0);
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn preflop_support_retention_gate_can_learn_a_supported_beneficial_continuation() {
    let solver = support_solver();
    let before = solver.snapshot_state();
    let traversals = 2048;
    let trained = solver
        .train_deviator_core_with_retention_gate::<true, true>(
            0,
            traversals,
            SUPPORT_FIT_SEED,
            0.0,
            false,
        )
        .unwrap();
    let child_visits = trained.coverage.total_visits - traversals;
    // The first seven Child visits return the baseline value -10. Starting
    // with its eighth visit, local RM chooses +1. Cumulative root action
    // difference is child_visits - 77, independently of root RM. A longer
    // fixed fit clears this finite penalty; the gate must not freeze the
    // profitable off-baseline branch permanently.
    assert!(child_visits > 77);
    assert_eq!(trained.coverage.visited_infosets, 2);
    assert_eq!(trained.coverage.retained_infosets, 2);
    assert_eq!(
        trained.coverage.retained_visits,
        trained.coverage.total_visits
    );
    assert_eq!(
        solver.game.terminal_counts(),
        [2 * traversals - child_visits, child_visits, child_visits]
    );
    for state in [SupportState::Root, SupportState::Child] {
        assert_eq!(trained.policy.actions.get(&state.reference_key()), Some(&1));
        assert!(!trained.policy.actions.contains_key(&state.baseline_key()));
    }
    assert_eq!(
        support_oracle_value(&trained.policy),
        f64::from(CHILD_PROBABILITY)
    );
    let held = solver
        .evaluate_frozen_preflop_deviators(
            256,
            820,
            &support_policies(trained.policy),
            ProfileVariant::default(),
            2,
        )
        .unwrap();
    assert_eq!(held.coverage[0].baseline_fallback_visits, 0);
    let held_child_samples = held.coverage[0].trained_action_visits - held.samples;
    assert!(held_child_samples > 0);
    assert!((held.gains[0].mean - held_child_samples as f64 / held.samples as f64).abs() < 1e-12);
    assert!(held.gains[0].mean > 0.0);
    assert_eq!(solver.snapshot_state(), before);
}
