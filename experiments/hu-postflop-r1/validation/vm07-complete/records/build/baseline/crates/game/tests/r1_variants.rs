//! Independent scalar physical-deal games for R1. Only observation-key
//! strings and action ordering are shared conventions. Rules, probability,
//! observations and utility here do not call the production builder or
//! settlement helpers. The frozen cfr-ref implementation is unchanged.

use std::collections::{HashMap, HashSet};

use cards::{PerPlayer, Player};
use cfr_ref::{RefGame, best_response_value, expected_value};
use engine::{Dcfr, F32Storage, NodeKind, ParConfig, Solver, StorageState};
use game::r1::{
    self, Action, Award, BoundaryError, Decision, DrawRules, Phase, Pot, PrivateHistory, Prototype,
    PublicObservation, Settlement,
};
use game::{ChipEv, UtilityModel};

struct Squared;

impl UtilityModel for Squared {
    fn utility(&self, stacks: &PerPlayer<f64>) -> PerPlayer<f64> {
        stacks.map(|stack| stack * stack)
    }

    fn is_zero_sum_affine(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy)]
enum Variant {
    Stud,
    Draw,
}

struct Oracle {
    variant: Variant,
    squared: bool,
}

#[derive(Clone, Debug)]
struct World {
    down: [u8; 2],
    up: Option<[u8; 2]>,
    replacement: Option<u8>,
    exchanged: Option<bool>,
    waiting: bool,
    actor: usize,
    history: String,
    paid: [i32; 2],
    folded: Option<usize>,
    terminal: bool,
}

impl RefGame for Oracle {
    type State = World;

    fn initial_states(&self) -> Vec<(World, f64)> {
        let mut roots = Vec::new();
        let deck = match self.variant {
            Variant::Stud => 6,
            Variant::Draw => 5,
        };
        for a in 0..deck {
            for b in 0..deck {
                if a == b {
                    continue;
                }
                let state = World {
                    down: [a, b],
                    up: None,
                    replacement: None,
                    exchanged: None,
                    waiting: false,
                    actor: 0,
                    history: String::new(),
                    paid: [1, 1],
                    folded: None,
                    terminal: false,
                };
                match self.variant {
                    Variant::Draw => roots.push((state, 1.0 / 20.0)),
                    Variant::Stud => {
                        for c in 0..6 {
                            for d in 0..6 {
                                if [a, b].contains(&c) || [a, b, c].contains(&d) {
                                    continue;
                                }
                                let mut dealt = state.clone();
                                dealt.up = Some([c, d]);
                                dealt.actor = usize::from(c < d);
                                roots.push((dealt, 1.0 / 360.0));
                            }
                        }
                    }
                }
            }
        }
        roots
    }

    fn is_terminal(&self, s: &World) -> bool {
        s.terminal
    }

    fn utility(&self, s: &World, player: usize) -> f64 {
        let share = if let Some(folder) = s.folded {
            if folder == player { 0.0 } else { 1.0 }
        } else {
            let ranks = if let Some(up) = s.up {
                [s.down[0] + up[0], s.down[1] + up[1]]
            } else {
                [s.replacement.unwrap_or(s.down[0]), s.down[1]]
            };
            if ranks[player] > ranks[1 - player] {
                1.0
            } else if ranks[player] == ranks[1 - player] {
                0.5
            } else {
                0.0
            }
        };
        let won = f64::from(s.paid[0] + s.paid[1]) * share;
        let after = 8.0 - f64::from(s.paid[player]) + won;
        if self.squared {
            after * after - 64.0
        } else {
            after - 8.0
        }
    }

    fn player_to_act(&self, s: &World) -> Option<usize> {
        (!s.waiting).then_some(s.actor)
    }

    fn chance_outcomes(&self, s: &World) -> Vec<(World, f64)> {
        assert!(s.waiting);
        (0..5)
            .filter(|card| !s.down.contains(card))
            .map(|card| {
                let mut dealt = s.clone();
                dealt.replacement = Some(card);
                dealt.waiting = false;
                (dealt, 1.0 / 3.0)
            })
            .collect()
    }

    fn num_actions(&self, _s: &World) -> usize {
        2
    }

    fn next(&self, s: &World, action: usize) -> World {
        let mut next = s.clone();
        if matches!(self.variant, Variant::Draw) && s.exchanged.is_none() {
            next.exchanged = Some(action == 1);
            next.waiting = action == 1;
            next.actor = 1;
            return next;
        }
        if s.history.ends_with('b') {
            next.terminal = true;
            if action == 0 {
                next.history.push('f');
                next.folded = Some(s.actor);
            } else {
                next.history.push('k');
                next.paid[s.actor] += 1;
            }
        } else if action == 0 {
            next.history.push('c');
            next.terminal = s.history == "c";
            next.actor = 1 - s.actor;
        } else {
            next.history.push('b');
            next.paid[s.actor] += 1;
            next.actor = 1 - s.actor;
        }
        next
    }

    fn infoset_key(&self, s: &World) -> String {
        let public = if let Some([a, b]) = s.up {
            format!("S:{a},{b}")
        } else {
            format!(
                "D:{}",
                match s.exchanged {
                    None => "-",
                    Some(false) => "K",
                    Some(true) => "R",
                }
            )
        };
        let mut private = format!("h{}", s.down[s.actor]);
        if s.actor == 0
            && let Some(card) = s.replacement
        {
            private.push_str(&format!(">{card}"));
        }
        format!("{public}|P{}|{private}|{}", s.actor, s.history)
    }
}

fn visit(oracle: &Oracle, state: &World, keys: &mut HashSet<String>) {
    if oracle.is_terminal(state) {
        return;
    }
    if oracle.player_to_act(state).is_none() {
        for (next, _) in oracle.chance_outcomes(state) {
            visit(oracle, &next, keys);
        }
    } else {
        keys.insert(oracle.infoset_key(state));
        for action in 0..2 {
            visit(oracle, &oracle.next(state, action), keys);
        }
    }
}

fn export(
    decisions: &[Decision],
    solver: &Solver<r1::MatrixEvaluator, F32Storage>,
) -> HashMap<String, Vec<f64>> {
    let mut profile = HashMap::new();
    for (id, node) in solver.game().tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let decision = &decisions[solver.game().tree.tags[id] as usize];
        assert_eq!(decision.actor, node.player);
        let strategy = solver.average_strategy_at(id as u32);
        let dim = decision.private_states[node.player].len();
        for hand in 0..dim {
            let key = decision.information_key(hand).unwrap();
            let probabilities = vec![f64::from(strategy[hand]), f64::from(strategy[dim + hand])];
            assert!(
                profile.insert(key, probabilities).is_none(),
                "an infoset was split across public nodes"
            );
        }
    }
    profile
}

fn fixed_profile(solver: &mut Solver<r1::MatrixEvaluator, F32Storage>, decisions: &[Decision]) {
    let mut state = solver.state();
    let StorageState::F32 { strategy_sum, .. } = &mut state.storage else {
        unreachable!()
    };
    for (id, node) in solver.game().tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let sref = solver.game().tree.storage_ref(node);
        let decision = &decisions[solver.game().tree.tags[id] as usize];
        for hand in 0..sref.num_hands as usize {
            let key = decision.information_key(hand).unwrap();
            // Fixed before observing any solve results; nonuniform across
            // observation histories, but never conditioned on a full world.
            let hash = key.bytes().fold(0u32, |sum, byte| {
                sum.wrapping_mul(31).wrapping_add(u32::from(byte))
            });
            let probability = 0.15 + (hash % 7) as f32 * 0.1;
            strategy_sum[sref.offset + hand] = probability;
            strategy_sum[sref.offset + sref.num_hands as usize + hand] = 1.0 - probability;
        }
    }
    solver.restore_state(state).unwrap();
}

fn compare(prototype: Prototype, oracle: &Oracle, mode: usize) {
    let mut solver = Solver::<_, F32Storage>::new(prototype.game, Box::<Dcfr>::default(), Some(8));
    solver.set_par(ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    });
    if mode == 1 {
        fixed_profile(&mut solver, &prototype.decisions);
    } else if mode == 2 {
        solver.run(8);
    }
    let profile = export(&prototype.decisions, &solver);
    let mut keys = HashSet::new();
    for (state, _) in oracle.initial_states() {
        visit(oracle, &state, &mut keys);
    }
    assert!(
        keys.iter().all(|key| profile.contains_key(key)),
        "oracle must not fall back to uniform for missing keys"
    );
    for p in Player::BOTH {
        let ev = expected_value(oracle, &profile, p.index());
        let br = best_response_value(oracle, &profile, p.index());
        // Absolute chip / artificial squared-chip units; tolerances are
        // deliberately fixed, not estimated from these test results.
        let tolerance = if oracle.squared { 2e-4 } else { 2e-5 };
        assert!(
            (solver.expected_value(p) - ev).abs() < tolerance,
            "mode={mode}, {p:?}: engine EV={}, oracle EV={ev}",
            solver.expected_value(p)
        );
        assert!(
            (solver.best_response_value(p) - br).abs() < tolerance,
            "mode={mode}, {p:?}: engine BR={}, oracle BR={br}",
            solver.best_response_value(p)
        );
    }
}

#[test]
fn stud_ev_and_br_match_independent_physical_deals() {
    for mode in 0..3 {
        compare(
            r1::stud(&ChipEv).unwrap(),
            &Oracle {
                variant: Variant::Stud,
                squared: false,
            },
            mode,
        );
    }
}

#[test]
fn draw_ev_and_br_match_independent_private_replacement() {
    for mode in 0..3 {
        compare(
            r1::draw(DrawRules::default(), &ChipEv).unwrap(),
            &Oracle {
                variant: Variant::Draw,
                squared: false,
            },
            mode,
        );
    }
}

#[test]
fn nonlinear_games_keep_both_players_utilities() {
    for variant in [Variant::Stud, Variant::Draw] {
        let game = match variant {
            Variant::Stud => r1::stud(&Squared).unwrap(),
            Variant::Draw => r1::draw(DrawRules::default(), &Squared).unwrap(),
        };
        assert!(!game.game.zero_sum);
        compare(
            game,
            &Oracle {
                variant,
                squared: true,
            },
            1,
        );
    }
}

#[test]
fn stud_owned_upcards_preserve_every_world_probability_and_actor() {
    let prototype = r1::stud(&ChipEv).unwrap();
    let tree = &prototype.game.tree;
    assert_eq!(
        prototype.phases,
        [
            Phase::InitialDeal,
            Phase::OwnedPublicDeal,
            Phase::Betting,
            Phase::Settlement
        ]
    );
    assert_eq!(tree.node(0).num_children, 30);
    let mut worlds = 0;
    for down0 in 0..6 {
        for down1 in 0..6 {
            if down0 == down1 {
                continue;
            }
            let mut mass = 0.0;
            for (position, child) in tree.children(0).enumerate() {
                let deal = tree.deal(tree.node(0), position);
                let mut basis0 = vec![0.0; 6];
                let mut basis1 = vec![0.0; 6];
                basis0[down0] = 1.0;
                basis1[down1] = 1.0;
                let reach0 = tree.map_reach(deal.maps[Player::P0], &basis0);
                let reach1 = tree.map_reach(deal.maps[Player::P1], &basis1);
                let joint = f64::from(deal.weight * reach0[down0] * reach1[down1]);
                let decision = &prototype.decisions[tree.tags[child as usize] as usize];
                let PublicObservation::Stud { upcards } = decision.public else {
                    panic!()
                };
                assert_eq!(
                    decision.actor,
                    if upcards[Player::P0] > upcards[Player::P1] {
                        Player::P0
                    } else {
                        Player::P1
                    }
                );
                let live =
                    !upcards.0.contains(&(down0 as u8)) && !upcards.0.contains(&(down1 as u8));
                assert_eq!(joint > 0.0, live);
                if live {
                    worlds += 1;
                    assert!((joint / prototype.game.normalizer - 1.0 / 360.0).abs() < 1e-9);
                }
                mass += joint;
            }
            assert!((mass - 1.0).abs() < 1e-6);
        }
    }
    assert_eq!(worlds, 360);
}

#[test]
fn draw_joint_mass_and_discard_memory_are_not_current_card_abstraction() {
    let prototype = r1::draw(DrawRules::default(), &ChipEv).unwrap();
    let tree = &prototype.game.tree;
    let chance = tree.node(tree.node(0).first_child + 1);
    assert_eq!(chance.kind, NodeKind::Chance);
    assert_eq!(
        chance.num_children, 1,
        "replacement identity must stay private"
    );
    let p1_node = chance.first_child;
    let p0_node = tree.node(p1_node).first_child;
    let terminal = tree.node(tree.node(p0_node).first_child);
    let p1 = &prototype.decisions[tree.tags[p1_node as usize] as usize];
    let p0 = &prototype.decisions[tree.tags[p0_node as usize] as usize];
    assert_eq!(p1.private_states[Player::P1].len(), 5);
    assert_eq!(p0.private_states[Player::P0].len(), 20);
    assert_eq!(p1.actions, [Action::Check, Action::Bet]);
    let transition = &tree.transitions[0];
    let mut worlds = 0;
    for old in 0..5 {
        for opponent in 0..5 {
            if old == opponent {
                continue;
            }
            let mut mass = 0.0;
            for &(source, destination, weight) in &transition.entries {
                if source as usize == old
                    && prototype
                        .game
                        .evaluator
                        .payoff(terminal.aux, Player::P0, destination as usize, opponent)
                        .is_some()
                {
                    assert!(
                        (f64::from(weight) / prototype.game.normalizer - 1.0 / 60.0).abs() < 1e-9
                    );
                    worlds += 1;
                    mass += f64::from(weight);
                }
            }
            assert!((mass - 1.0).abs() < 1e-6);
        }
    }
    assert_eq!(worlds, 60);
    let mut keys = Vec::new();
    for (old, expected) in [(0, -1.0 / 3.0), (4, 1.0 / 3.0)] {
        let index = p0.private_states[Player::P0]
            .iter()
            .position(|state| {
                *state
                    == PrivateHistory::Replaced {
                        discarded: old,
                        replacement: 2,
                    }
            })
            .unwrap();
        let payoffs: Vec<_> = (0..5)
            .filter_map(|opponent| {
                prototype
                    .game
                    .evaluator
                    .payoff(terminal.aux, Player::P0, index, opponent)
            })
            .collect();
        assert_eq!(payoffs.len(), 3);
        assert!((f64::from(payoffs.iter().sum::<f32>()) / 3.0 - expected).abs() < 1e-12);
        keys.push(p0.information_key(index).unwrap());
    }
    assert_ne!(keys[0], keys[1]);
    assert!(p1.information_key(2).unwrap().contains("|h2|"));
    assert!(!p1.information_key(2).unwrap().contains('>'));
}

#[test]
fn observations_hide_other_cards_and_remember_own_replacement() {
    let oracle = Oracle {
        variant: Variant::Draw,
        squared: false,
    };
    let root = oracle
        .initial_states()
        .into_iter()
        .find(|(world, _)| world.down == [0, 4])
        .unwrap()
        .0;
    let draw = oracle.next(&root, 1);
    let outcomes = oracle.chance_outcomes(&draw);
    let p1_keys: HashSet<_> = outcomes
        .iter()
        .map(|(world, _)| oracle.infoset_key(world))
        .collect();
    assert_eq!(p1_keys.len(), 1, "P1 cannot observe the replacement");
    let p0_keys: HashSet<_> = outcomes
        .iter()
        .map(|(world, _)| oracle.infoset_key(&oracle.next(world, 0)))
        .collect();
    assert_eq!(p0_keys.len(), 3, "P0 observes its own replacement");

    let stud = r1::stud(&ChipEv).unwrap();
    let first = stud
        .decisions
        .iter()
        .find(|decision| {
            decision.public
                == PublicObservation::Stud {
                    upcards: PerPlayer::new(5, 1),
                }
                && decision.betting_history.is_empty()
        })
        .unwrap();
    assert_eq!(first.actor, Player::P0);
    assert_eq!(first.information_key(0).unwrap(), "S:5,1|P0|h0|");
    // Swapping ownership changes both the public observation and actor;
    // this is not an unordered shared board.
    let swapped = stud
        .decisions
        .iter()
        .find(|decision| {
            decision.public
                == PublicObservation::Stud {
                    upcards: PerPlayer::new(1, 5),
                }
                && decision.betting_history.is_empty()
        })
        .unwrap();
    assert_eq!(swapped.actor, Player::P1);
    assert_eq!(swapped.information_key(0).unwrap(), "S:1,5|P1|h0|");
}

#[test]
fn unsupported_recall_and_hidden_action_lowerings_are_explicit_errors() {
    assert!(matches!(
        r1::draw(
            DrawRules {
                remember_discard: false,
                ..DrawRules::default()
            },
            &ChipEv
        ),
        Err(BoundaryError::RecallLoss)
    ));
    assert!(matches!(
        r1::draw(
            DrawRules {
                exchange_action_public: false,
                ..DrawRules::default()
            },
            &ChipEv
        ),
        Err(BoundaryError::HiddenExchangeAction)
    ));
}

fn split(high: Award, low: Option<Award>) -> Settlement {
    Settlement {
        stacks_before: PerPlayer::new(100, 100),
        contributions: PerPlayer::new(4, 4),
        returned: PerPlayer::new(0, 0),
        dead_money: 0,
        pots: vec![Pot {
            amount: 8,
            rake: 0,
            eligible: PerPlayer::new(true, true),
            high,
            low,
        }],
    }
}

#[test]
fn split_table_and_nonlinear_utility_are_allocated_before_utility() {
    for (high, low, chips) in [
        (Award::P0, Some(Award::P0), [8, 0]),
        (Award::P0, Some(Award::P1), [4, 4]),
        (Award::P0, Some(Award::Both), [6, 2]),
        (Award::P1, None, [0, 8]),
        (Award::Both, Some(Award::Both), [4, 4]),
    ] {
        let input = split(high, low);
        let result = r1::settle(&input, &ChipEv).unwrap();
        assert_eq!(result.awards.0, chips);
        assert_eq!(
            result.utility.0,
            [f64::from(chips[0]) - 4.0, f64::from(chips[1]) - 4.0]
        );
    }
    let nonlinear = r1::settle(&split(Award::P0, Some(Award::Both)), &Squared).unwrap();
    assert_eq!(nonlinear.stacks_after.0, [102, 98]);
    assert_eq!(nonlinear.utility.0, [404.0, -396.0]);
    assert_ne!(nonlinear.utility.0, [408.0, -392.0]);
}

#[test]
fn odd_chips_rake_returns_dead_money_and_pot_eligibility() {
    let mut odd = split(Award::P0, Some(Award::Both));
    odd.contributions = PerPlayer::new(4, 3);
    odd.pots[0].amount = 7;
    assert_eq!(r1::settle(&odd, &ChipEv).unwrap().awards.0, [6, 1]);
    let mixed = Settlement {
        stacks_before: PerPlayer::new(100, 100),
        contributions: PerPlayer::new(6, 4),
        returned: PerPlayer::new(2, 0),
        dead_money: 3,
        pots: vec![
            Pot {
                amount: 8,
                rake: 2,
                eligible: PerPlayer::new(true, true),
                high: Award::P1,
                low: None,
            },
            Pot {
                amount: 3,
                rake: 0,
                eligible: PerPlayer::new(true, false),
                high: Award::P0,
                low: None,
            },
        ],
    };
    let result = r1::settle(&mixed, &ChipEv).unwrap();
    assert_eq!(result.awards.0, [3, 6]);
    assert_eq!(result.stacks_after.0, [99, 102]);
    assert_eq!(result.utility.0, [-1.0, 2.0]);
    assert_eq!(result.rake, 2);
    let mut invalid = mixed.clone();
    invalid.pots[1].high = Award::P1;
    assert!(matches!(
        r1::settle(&invalid, &ChipEv),
        Err(BoundaryError::InvalidSettlement)
    ));
    invalid = mixed.clone();
    invalid.pots[0].amount += 1;
    assert!(matches!(
        r1::settle(&invalid, &ChipEv),
        Err(BoundaryError::InvalidSettlement)
    ));
    invalid = mixed;
    invalid.pots[0].rake = 9;
    assert!(matches!(
        r1::settle(&invalid, &ChipEv),
        Err(BoundaryError::InvalidSettlement)
    ));
}

#[test]
fn fold_awards_the_whole_pot_to_the_only_eligible_seat() {
    let input = Settlement {
        stacks_before: PerPlayer::new(8, 8),
        contributions: PerPlayer::new(2, 1),
        returned: PerPlayer::new(0, 0),
        dead_money: 0,
        pots: vec![Pot {
            amount: 3,
            rake: 0,
            eligible: PerPlayer::new(false, true),
            high: Award::P1,
            low: None,
        }],
    };
    let result = r1::settle(&input, &ChipEv).unwrap();
    assert_eq!(result.awards.0, [0, 3]);
    assert_eq!(result.utility.0, [-2.0, 2.0]);
    let mut invalid = input;
    invalid.pots[0].low = Some(Award::Both);
    assert!(matches!(
        r1::settle(&invalid, &ChipEv),
        Err(BoundaryError::InvalidSettlement)
    ));
}

#[test]
fn settlement_rejects_nonfinite_utility_and_overflow() {
    struct Infinite;
    impl UtilityModel for Infinite {
        fn utility(&self, _stacks: &PerPlayer<f64>) -> PerPlayer<f64> {
            PerPlayer::new(f64::INFINITY, 0.0)
        }
        fn is_zero_sum_affine(&self) -> bool {
            false
        }
    }
    assert!(matches!(
        r1::settle(&split(Award::P0, None), &Infinite),
        Err(BoundaryError::NonFiniteUtility)
    ));
    let mut overflow = split(Award::P0, None);
    overflow.dead_money = u32::MAX;
    assert!(matches!(
        r1::settle(&overflow, &ChipEv),
        Err(BoundaryError::ArithmeticOverflow)
    ));
}
