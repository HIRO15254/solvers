use super::*;
use crate::abstraction::{FeatureHashAbstraction, FeatureHashParams};
use crate::config::{
    AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
    SeatConfig, SizeSpec, UtilityConfig,
};
use cards::{CardSet, combo_index};

type TestSolver = MultiwaySolver<HoldemGame<FeatureHashAbstraction>>;

fn combo(text: &str) -> usize {
    combo_index(text[..2].parse().unwrap(), text[2..].parse().unwrap())
}

fn class(combo: usize) -> usize {
    let (a, b) = combo_cards(combo);
    class_index(a.rank(), b.rank(), a.suit() == b.suit())
}

fn game<A: MultiwayAbstraction>(abstraction: A) -> HoldemGame<A> {
    let mut betting = BettingConfig::default();
    betting.preflop.max_aggressive_actions = 1;
    betting.preflop.include_allin = false;
    betting.preflop.bet_sizes = vec![SizeSpec::PreviousBetMultiple { factor: 2.0 }];
    betting.preflop.raise_sizes = betting.preflop.bet_sizes.clone();
    for street in [&mut betting.flop, &mut betting.turn, &mut betting.river] {
        street.bet_sizes.clear();
        street.raise_sizes.clear();
        street.include_allin = false;
    }
    HoldemGame::new(
        &MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 6.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: crate::SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting,
            forced_bets: None,
            abstraction: AbstractionConfig {
                recall: RecallMode::Street,
                ..AbstractionConfig::default()
            },
        },
        &UtilityConfig::ChipEv,
        &RakeConfig::None,
        abstraction,
    )
    .unwrap()
}

fn abstraction() -> FeatureHashAbstraction {
    FeatureHashAbstraction::new(FeatureHashParams {
        flop_buckets: 4,
        turn_buckets: 4,
        river_buckets: 4,
    })
    .unwrap()
}

fn config() -> SolverConfig {
    SolverConfig {
        max_memory_bytes: 1 << 25,
        max_traversal_depth: 64,
        ..SolverConfig::default()
    }
}

fn set_probability(
    solver: &mut TestSolver,
    history: HistoryKey,
    selected: usize,
    probability: impl Fn(usize) -> f32,
) {
    let dense = solver.dense.as_mut().unwrap();
    let node = dense
        .tree
        .nodes
        .iter()
        .position(|node| node.history == history)
        .unwrap() as NodeId;
    let count = dense.tree.nodes[node as usize].action_labels.len();
    let other = (0..count).find(|&a| a != selected).unwrap();
    for bucket in 0..NUM_CLASSES as u32 {
        let p = probability(bucket as usize);
        let mut strategy = vec![0.0; count];
        strategy[selected] = p;
        strategy[other] = 1.0 - p;
        let range = dense.arena.slot_range(node, bucket).unwrap();
        dense.arena.strategy_sum[range.clone()].copy_from_slice(&strategy);
        dense.arena.regrets[range].copy_from_slice(&strategy);
        dense
            .arena
            .touched_set(dense.arena.column_id(node, bucket).unwrap());
    }
}

struct Fixture {
    solver: TestSolver,
    path: Vec<usize>,
    support: Vec<Vec<usize>>,
    probabilities: Vec<Vec<f64>>,
}

fn fixture() -> Fixture {
    let support = [
        vec!["AsAh", "AdAc", "KsKh", "2c3c"],
        vec!["AsQs", "KdQd", "4c5c"],
        vec!["AhJh", "KsJs", "6c7c"],
    ]
    .into_iter()
    .map(|row| row.into_iter().map(combo).collect::<Vec<_>>())
    .collect::<Vec<_>>();
    let prior = [
        vec![0.8, 0.4, 0.6, 0.2],
        vec![0.9, 1e-12, 0.3],
        vec![0.3, 0.5, 0.8],
    ];
    let probabilities = vec![
        vec![0.75, 0.75, 0.125, 0.0],
        vec![0.25, 0.5, 0.75],
        vec![0.125, 0.5, 0.875],
    ];
    let mut ranges = vec![Range::default(); 3];
    for seat in 0..3 {
        for (&hand, &weight) in support[seat].iter().zip(&prior[seat]) {
            ranges[seat].set_weight(hand, weight);
        }
    }
    let mut solver = MultiwaySolver::new(
        game(abstraction()),
        DealSampler::new(ranges).unwrap(),
        config(),
    )
    .unwrap();
    let mut state = solver.game.root_state();
    let mut history = HistoryKey::ROOT;
    let mut path = Vec::new();
    for (seat, prefix) in ["call:", "raise-to:", "fold"].into_iter().enumerate() {
        assert_eq!(solver.game.actor(&state), Some(seat));
        let menu = solver.game.node_actions(&state);
        let action = (0..solver.game.num_actions_of(&menu))
            .find(|&a| solver.game.action_label_of(&menu, a).starts_with(prefix))
            .unwrap();
        set_probability(&mut solver, history, action, |bucket| {
            support[seat]
                .iter()
                .position(|&h| class(h) == bucket)
                .map_or(0.5, |i| probabilities[seat][i] as f32)
        });
        state = solver.game.next_state_with(&state, &menu, action);
        history = history.child(seat, action);
        path.push(action);
    }
    assert_eq!(solver.game.actor(&state), Some(0));
    assert_eq!(state.street, Street::Preflop);
    assert_eq!(state.non_folded_mask().len(), 2);
    Fixture {
        solver,
        path,
        support,
        probabilities,
    }
}

fn close(left: f64, right: f64) {
    assert!(
        (left - right).abs() <= 2e-12 * left.abs().max(right.abs()).max(1e-100),
        "{left} != {right}"
    );
}

#[test]
fn counterfactual_proposal_exact_two_target_oracle_preserves_folded_blockers() {
    let Fixture {
        solver,
        path,
        support,
        probabilities,
    } = fixture();
    let before = solver.snapshot_state();
    let prepared = solver
        .prepare_counterfactual_preflop_proposal(path.clone(), ProfileVariant::default(), 0)
        .unwrap();
    let proposal = &prepared.proposal;
    let actual = solver
        .prepare_preflop_proposal(path.clone(), ProfileVariant::default())
        .unwrap();
    assert_eq!(prepared.validated_class_contexts, path.len() + 1);
    assert_eq!(prepared.own_prefix_probability_by_bucket.len(), NUM_CLASSES);
    assert_eq!(proposal.metadata.positive_target_combos_by_seat, [4, 3, 3]);
    assert_eq!(actual.metadata.positive_target_combos_by_seat, [3, 3, 3]);
    assert_eq!(proposal.metadata.floor_adjusted_combos_by_seat, [0, 1, 0]);
    assert_eq!(
        proposal.metadata.root_range_fingerprint,
        solver.sampler.range_fingerprint()
    );
    let scale = proposal
        .metadata
        .target_scale_by_seat
        .iter()
        .product::<f64>();
    let actual_scale = actual.metadata.target_scale_by_seat.iter().product::<f64>();
    // [target][class][mass, weighted observable]; this oracle sums every legal
    // physical tuple, including the folded seat's nonuniform reaching range.
    let mut direct = vec![vec![[0.0; 2]; NUM_CLASSES]; 2];
    let mut corrected = direct.clone();
    let mut illegal = 0;
    let mut legal = 0;
    let mut ignored_folded_blocker_aces_mass = 0.0;
    let mut ignored_folded_blocker_total = 0.0;
    for (own, &a) in support[0].iter().enumerate() {
        for (opp, &b) in support[1].iter().enumerate() {
            for (folded, &c) in support[2].iter().enumerate() {
                let hands = [a, b, c];
                let cf_target = hands
                    .iter()
                    .enumerate()
                    .map(|(seat, &h)| solver.sampler.evaluation_combo_weight(seat, h))
                    .product::<f64>()
                    * probabilities[1][opp]
                    * probabilities[2][folded];
                let mut dead = CardSet::EMPTY;
                let mut valid = true;
                for (seat, hand) in hands.into_iter().enumerate() {
                    let (hi, lo) = combo_cards(hand);
                    if dead.contains(hi) || dead.contains(lo) {
                        valid = false;
                    }
                    dead.insert(hi);
                    dead.insert(lo);
                    // Deliberately incorrect comparison law omits only folded
                    // seat collisions, while retaining its prior/action factor.
                    if seat == 1 && valid {
                        ignored_folded_blocker_total += cf_target;
                        ignored_folded_blocker_aces_mass +=
                            cf_target * f64::from(class(a) == class(support[0][0]));
                    }
                }
                if !valid {
                    illegal += 1;
                    continue;
                }
                legal += 1;
                let live = cards::ALL_CARDS
                    .into_iter()
                    .filter(|card| !dead.contains(*card))
                    .collect::<Vec<_>>();
                let world =
                    SampledWorld::new(hands.to_vec(), live[..5].try_into().unwrap()).unwrap();
                // Independent exact expected first runout card: all six hole
                // cards remain dead even though seat 2 has folded publicly.
                let observable =
                    live.iter().map(|card| card.index() as f64).sum::<f64>() / live.len() as f64;
                let q = hands
                    .iter()
                    .enumerate()
                    .map(|(seat, &h)| proposal.sampler.evaluation_combo_weight(seat, h))
                    .product::<f64>();
                let w = proposal.correction(&world).unwrap();
                let li = prepared.own_prefix_probability_by_bucket[class(a)];
                assert_eq!(li, probabilities[0][own]);
                close(q * w * scale, cf_target);
                for (target, own_factor) in [1.0, li].into_iter().enumerate() {
                    direct[target][class(a)][0] += cf_target * own_factor;
                    direct[target][class(a)][1] += cf_target * own_factor * observable;
                    corrected[target][class(a)][0] += q * w * own_factor;
                    corrected[target][class(a)][1] += q * w * own_factor * observable;
                }
                if li > 0.0 {
                    let qa = hands
                        .iter()
                        .enumerate()
                        .map(|(seat, &h)| actual.sampler.evaluation_combo_weight(seat, h))
                        .product::<f64>();
                    close(
                        qa * actual.correction(&world).unwrap() * actual_scale,
                        cf_target * li,
                    );
                } else {
                    assert!(q > 0.0 && w > 0.0);
                    assert_eq!(actual.sampler.evaluation_combo_weight(0, a), 0.0);
                }
            }
        }
    }
    assert!(illegal > 0 && legal > 0 && legal + illegal == 36);
    for bucket in 0..NUM_CLASSES {
        for target in 0..2 {
            for moment in 0..2 {
                close(
                    corrected[target][bucket][moment] * scale,
                    direct[target][bucket][moment],
                );
            }
        }
        if direct[1][bucket][0] > 0.0 {
            close(
                direct[0][bucket][1] / direct[0][bucket][0],
                direct[1][bucket][1] / direct[1][bucket][0],
            );
        }
    }
    let zero = class(support[0][3]);
    assert!(direct[0][zero][0] > 0.0);
    assert_eq!(direct[1][zero][0], 0.0);
    let cf_total = direct[0].iter().map(|row| row[0]).sum::<f64>();
    let aces_fraction = direct[0][class(support[0][0])][0] / cf_total;
    assert!(
        (aces_fraction - ignored_folded_blocker_aces_mass / ignored_folded_blocker_total).abs()
            > 1e-3
    );
    // The two complete-population means also differ despite exact positive-
    // own-class cancellation; aggregation weights define a different target.
    let means = direct
        .iter()
        .map(|rows| rows.iter().map(|r| r[1]).sum::<f64>() / rows.iter().map(|r| r[0]).sum::<f64>())
        .collect::<Vec<_>>();
    assert!((means[0] - means[1]).abs() > 1e-3);
    assert_eq!(solver.snapshot_state(), before);
}

#[test]
fn counterfactual_proposal_root_matches_legacy_and_all_zero_own_is_supported() {
    let Fixture {
        mut solver, path, ..
    } = fixture();
    for variant in [
        ProfileVariant::default(),
        ProfileVariant {
            use_current_strategy: true,
            purify_threshold: 0.0,
        },
        ProfileVariant {
            use_current_strategy: false,
            purify_threshold: 0.35,
        },
    ] {
        let root = solver
            .prepare_counterfactual_preflop_proposal(vec![], variant, 0)
            .unwrap();
        let legacy = solver.prepare_preflop_proposal(vec![], variant).unwrap();
        assert_eq!(root.validated_class_contexts, 1);
        assert!(
            root.own_prefix_probability_by_bucket
                .iter()
                .all(|&p| p == 1.0)
        );
        assert_eq!(root.proposal.metadata, legacy.metadata);
        assert_eq!(root.proposal.corrections, legacy.corrections);
    }
    set_probability(&mut solver, HistoryKey::ROOT, path[0], |_| 0.0);
    let cf = solver
        .prepare_counterfactual_preflop_proposal(path.clone(), ProfileVariant::default(), 0)
        .unwrap();
    assert!(
        cf.own_prefix_probability_by_bucket
            .iter()
            .all(|&p| p == 0.0)
    );
    assert_eq!(cf.proposal.metadata.positive_target_combos_by_seat[0], 4);
    assert!(matches!(
        solver.prepare_preflop_proposal(path.clone(), ProfileVariant::default()),
        Err(SolverError::Sample(SampleError::EmptyRange { seat: 0 }))
    ));
    let opponent_history = HistoryKey::ROOT.child(0, path[0]);
    set_probability(&mut solver, opponent_history, path[1], |_| 0.0);
    assert!(matches!(
        solver.prepare_counterfactual_preflop_proposal(path, ProfileVariant::default(), 0),
        Err(SolverError::Sample(SampleError::EmptyRange { seat: 1 }))
    ));
}

#[test]
fn counterfactual_proposal_preserves_incompatible_joint_range_error() {
    let Fixture {
        mut solver,
        path,
        support,
        ..
    } = fixture();
    let mut ranges = vec![Range::default(); 3];
    ranges[0].set_weight(support[0][0], 1.0);
    for seat in 1..3 {
        for &hand in &support[seat] {
            ranges[seat].set_weight(hand, 1.0);
        }
    }
    solver.sampler = DealSampler::new(ranges).unwrap();
    set_probability(
        &mut solver,
        HistoryKey::ROOT.child(0, path[0]),
        path[1],
        |bucket| f32::from(bucket == class(support[1][0])),
    );
    // Seat 0's only AsAh and seat 1's selected AsQs cannot coexist, even
    // though the original root ranges admitted collision-free deals.
    assert!(matches!(
        solver.prepare_counterfactual_preflop_proposal(path, ProfileVariant::default(), 0),
        Err(SolverError::Sample(SampleError::IncompatibleRanges))
    ));
}

struct WrongClasses {
    wrong_count: bool,
}

impl MultiwayAbstraction for WrongClasses {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
        if street == Street::Preflop {
            if self.wrong_count && active_opponents == 1 {
                168
            } else {
                169
            }
        } else {
            4
        }
    }
    fn bucket(&self, context: BucketContext<'_>) -> BucketId {
        let value = class(context.combo) as u32;
        if context.street != Street::Preflop {
            0
        } else if context.active_opponents == 1 {
            (value + 1) % self.num_buckets(context.street, context.active_opponents)
        } else {
            value
        }
    }
    fn fingerprint(&self) -> [u8; 32] {
        [u8::from(self.wrong_count); 32]
    }
}

#[test]
fn counterfactual_proposal_rejects_wrong_class_mapping_in_later_context() {
    for wrong_count in [false, true] {
        let game = game(WrongClasses { wrong_count });
        let sampler = game.deal_sampler().unwrap();
        let solver = MultiwaySolver::new(game, sampler, config()).unwrap();
        assert!(
            solver
                .prepare_counterfactual_preflop_proposal(vec![], ProfileVariant::default(), 0)
                .is_ok()
        );
        let state = solver.game.root_state();
        let menu = solver.game.node_actions(&state);
        let fold = (0..solver.game.num_actions_of(&menu))
            .find(|&a| solver.game.action_label_of(&menu, a) == "fold")
            .unwrap();
        let error = solver
            .prepare_counterfactual_preflop_proposal(vec![fold], ProfileVariant::default(), 1)
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            SolverError::InvalidState(if wrong_count {
                "counterfactual proposal requires exactly 169 preflop classes"
            } else {
                "counterfactual proposal requires exact preflop class mapping"
            })
            .to_string()
        );
    }
}

#[test]
fn counterfactual_proposal_rejects_wrong_actor_bounds_terminal_and_postflop() {
    let Fixture { mut solver, .. } = fixture();
    let variant = ProfileVariant::default();
    assert!(matches!(
        solver.prepare_counterfactual_preflop_proposal(vec![], variant, 3),
        Err(SolverError::InvalidActor {
            actor: 3,
            num_players: 3
        })
    ));
    assert!(
        solver
            .prepare_counterfactual_preflop_proposal(vec![], variant, 1)
            .is_err()
    );
    assert!(
        solver
            .prepare_counterfactual_preflop_proposal(vec![usize::MAX], variant, 0)
            .is_err()
    );
    assert!(
        solver
            .prepare_counterfactual_preflop_proposal(
                vec![],
                ProfileVariant {
                    purify_threshold: f32::NAN,
                    ..variant
                },
                0
            )
            .is_err()
    );
    for fold in [false, true] {
        let mut state = solver.game.root_state();
        let mut path = Vec::new();
        while state.street == Street::Preflop && solver.game.actor(&state).is_some() {
            let menu = solver.game.node_actions(&state);
            let action = (0..solver.game.num_actions_of(&menu))
                .find(|&a| {
                    let label = solver.game.action_label_of(&menu, a);
                    if fold {
                        label == "fold"
                    } else {
                        label.starts_with("call:") || label == "check"
                    }
                })
                .unwrap();
            state = solver.game.next_state_with(&state, &menu, action);
            path.push(action);
        }
        assert!(
            solver
                .prepare_counterfactual_preflop_proposal(
                    path,
                    variant,
                    solver.game.actor(&state).unwrap_or(0)
                )
                .is_err()
        );
    }
    solver.config.max_traversal_depth = 1;
    assert!(matches!(
        solver.prepare_counterfactual_preflop_proposal(vec![0], variant, 0),
        Err(SolverError::DepthLimit { limit: 1 })
    ));
}
