//! Independent finite river games for T1-01: weighted/blocking ranges,
//! raises, short all-ins, odd pots, ties, and action-dependent rake.
//! The scalar side does not call production betting, rank, or payoff code.
//! Only card/combo identifiers and the frozen `cfr-ref` evaluator are shared.

use std::collections::{BTreeSet, HashMap};

use cards::{Card, Chips, PerPlayer, Player, Range, combo_index};
use cfr_ref::{RefGame, RefStrategy, best_response_value, expected_value};
use engine::{Dcfr, F32Storage, NodeKind, ParConfig, PublicTree, Solver, StorageState};
use game::{ChipEv, NoRake, PayoffPipeline, PercentCapRake, RakeModel};
use holdem::{PerStreet, PostflopConfig, PostflopNodeInfo, StreetTree, build_postflop_game};

const TOLERANCE: f64 = 1e-4;

#[derive(Clone, Copy)]
struct Fixture {
    name: &'static str,
    pot: u32,
    stack: u32,
    max_aggressions: u32,
    percentages: &'static [u32],
    rake: Option<(f64, f64)>,
}

const FIXTURES: [Fixture; 3] = [
    Fixture {
        name: "river-raised-no-rake",
        pot: 5,
        stack: 40,
        max_aggressions: 3,
        percentages: &[50, 100],
        rake: None,
    },
    Fixture {
        name: "river-short-allin-capped-rake",
        pot: 5,
        stack: 8,
        max_aggressions: 3,
        percentages: &[50, 100],
        rake: Some((0.125, 1.5)),
    },
    Fixture {
        name: "river-raised-uncapped-rake",
        pot: 5,
        stack: 40,
        max_aggressions: 3,
        percentages: &[50, 100],
        rake: Some((0.125, 100.0)),
    },
];

#[derive(Clone, Copy)]
struct Hand {
    cards: [Card; 2],
    weight: f64,
    // On the fixed K Q 7 2 3 board: JJ < AA < 777 < KKK.
    // This manually specified order includes ties and does not use rank_of.
    strength: u8,
}

impl Hand {
    fn combo(self) -> usize {
        combo_index(self.cards[0], self.cards[1])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Check,
    Fold,
    Call,
    Wager(u32), // total river contribution, excluding the starting pot
}

impl Action {
    fn label(self, facing_bet: bool) -> String {
        match self {
            Self::Check => "check".into(),
            Self::Fold => "fold".into(),
            Self::Call => "call".into(),
            Self::Wager(to) if facing_bet => format!("raise to {to}"),
            Self::Wager(to) => format!("bet {to}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Finish {
    Fold(usize),
    Showdown,
}

#[derive(Clone)]
struct State {
    hands: [usize; 2],
    actor: usize,
    invested: [u32; 2],
    checked: bool,
    aggressions: u32,
    last_increment: u32,
    history: String,
    finish: Option<Finish>,
}

struct RiverOracle {
    fixture: Fixture,
    board: Vec<Card>,
    hands: [Vec<Hand>; 2],
}

impl RiverOracle {
    fn new(fixture: Fixture) -> Self {
        let hand = |a: &str, b: &str, weight: f64, strength| Hand {
            cards: [a.parse().unwrap(), b.parse().unwrap()],
            weight,
            strength,
        };
        Self {
            fixture,
            board: "Ks Qs 7h 2d 3c"
                .split_whitespace()
                .map(|c| c.parse().unwrap())
                .collect(),
            hands: [
                vec![
                    hand("Ah", "Ad", 0.75, 2),
                    hand("7c", "7d", 0.25, 3),
                    hand("Jh", "Jd", 0.5, 1),
                    hand("Ks", "Kd", 1.0, 4), // blocked by the board
                ],
                vec![
                    hand("Ac", "As", 0.5, 2),
                    hand("Kh", "Kd", 1.0, 4),
                    hand("Jc", "Js", 0.25, 1),
                    hand("Ac", "Ad", 0.125, 2), // conflicts with P0's AA
                ],
            ],
        }
    }

    fn actions(&self, state: &State) -> Vec<Action> {
        let mine = state.invested[state.actor];
        let facing = state.invested[1 - state.actor];
        let to_call = facing - mine;
        let mut actions = if to_call == 0 {
            vec![Action::Check]
        } else {
            vec![Action::Fold, Action::Call]
        };
        if state.aggressions == self.fixture.max_aggressions || facing == self.fixture.stack {
            return actions;
        }
        // Half-pot rounds up at odd integer pots; pot-sized wagers are exact.
        // Both sizes are raises ABOVE the matched contribution. A full raise
        // must repeat the last increment, but a smaller all-in is legal.
        let pot_after_call = self.fixture.pot + 2 * facing;
        let minimum = facing + state.last_increment.max(1);
        let targets: BTreeSet<_> = self
            .fixture
            .percentages
            .iter()
            .map(|percent| (pot_after_call * percent).div_ceil(100))
            .map(|increment| (facing + increment).max(minimum).min(self.fixture.stack))
            .collect();
        actions.extend(targets.into_iter().map(Action::Wager));
        actions
    }

    fn apply(&self, state: &State, action: Action) -> State {
        let mut next = state.clone();
        match action {
            Action::Check => {
                next.history.push('x');
                if state.checked {
                    next.finish = Some(Finish::Showdown);
                } else {
                    next.checked = true;
                    next.actor = 1 - state.actor;
                }
            }
            Action::Fold => {
                next.history.push('f');
                next.finish = Some(Finish::Fold(state.actor));
            }
            Action::Call => {
                next.history.push('c');
                next.invested[state.actor] = state.invested[1 - state.actor];
                next.finish = Some(Finish::Showdown);
            }
            Action::Wager(to) => {
                next.history.push_str(&format!("r{to}"));
                next.invested[state.actor] = to;
                next.last_increment = to - state.invested[1 - state.actor];
                next.aggressions += 1;
                next.actor = 1 - state.actor;
            }
        }
        next
    }

    fn rake(&self, state: &State) -> f64 {
        self.fixture.rake.map_or(0.0, |(rate, cap)| {
            ((self.fixture.pot + state.invested[0] + state.invested[1]) as f64 * rate).min(cap)
        })
    }

    fn engine_game(&self) -> holdem::PostflopGame {
        let range = |hands: &[Hand]| {
            let mut range = Range::default();
            for &hand in hands {
                range.set_weight(hand.combo(), hand.weight as f32);
            }
            range
        };
        let raked = self.fixture.rake.map(|(rate, cap)| PercentCapRake {
            rate,
            cap,
            no_flop_no_drop: false,
        });
        let rake: &dyn RakeModel = match &raked {
            Some(rake) => rake,
            None => &NoRake,
        };
        let fractions: Vec<_> = self
            .fixture
            .percentages
            .iter()
            .map(|p| f64::from(*p) / 100.0)
            .collect();
        build_postflop_game(
            &PostflopConfig {
                board: self.board.clone(),
                ranges: PerPlayer::new(range(&self.hands[0]), range(&self.hands[1])),
                pot: Chips(self.fixture.pot),
                effective_stack: Chips(self.fixture.stack),
                streets: PerStreet {
                    flop: StreetTree::pot_fractions(&[], &[], 0),
                    turn: StreetTree::pot_fractions(&[], &[], 0),
                    river: StreetTree::pot_fractions(
                        &fractions,
                        &fractions,
                        self.fixture.max_aggressions,
                    ),
                },
                iso_merging: false,
                ..Default::default()
            },
            PayoffPipeline {
                rake,
                utility: &ChipEv,
            },
        )
    }

    // Traverse every scalar private world alongside the public tree BEFORE
    // exporting any strategy. Matching history strings alone could silently
    // import the wrong action ordering, or miss a branch and default uniform.
    fn assert_tree_matches(&self, tree: &PublicTree, info: &[PostflopNodeInfo]) {
        fn walk(
            oracle: &RiverOracle,
            state: &State,
            tree: &PublicTree,
            info: &[PostflopNodeInfo],
            id: u32,
        ) {
            let node = tree.node(id);
            if state.finish.is_some() {
                assert_eq!(node.kind, NodeKind::Terminal, "{}", state.history);
                return;
            }
            assert_eq!(node.kind, NodeKind::Action, "{}", state.history);
            let player = [Player::P0, Player::P1][state.actor];
            assert_eq!(node.player, player, "{}", state.history);
            let metadata = &info[tree.tags[id as usize] as usize];
            assert_eq!(metadata.history, state.history);
            let facing = state.invested[1 - state.actor] > state.invested[state.actor];
            let actions = oracle.actions(state);
            let labels: Vec<_> = actions.iter().map(|a| a.label(facing)).collect();
            assert_eq!(metadata.actions, labels, "{}", state.history);
            assert_eq!(node.num_children as usize, actions.len());
            let pot = oracle.fixture.pot;
            for (p, initial) in [(Player::P0, pot / 2), (Player::P1, pot - pot / 2)] {
                assert_eq!(metadata.contrib[p].0, initial + state.invested[p.index()]);
            }
            for (a, action) in actions.into_iter().enumerate() {
                walk(
                    oracle,
                    &oracle.apply(state, action),
                    tree,
                    info,
                    node.first_child + a as u32,
                );
            }
        }
        let roots = self.initial_states();
        assert_eq!(
            roots.len(),
            11,
            "board and private-card blockers must both apply"
        );
        for (state, _) in roots {
            walk(self, &state, tree, info, 0);
        }
    }
}

impl RefGame for RiverOracle {
    type State = State;

    fn initial_states(&self) -> Vec<(State, f64)> {
        let mut roots = Vec::new();
        for (h, hero) in self.hands[0].iter().enumerate() {
            for (o, opponent) in self.hands[1].iter().enumerate() {
                let mut cards = self.board.clone();
                cards.extend(hero.cards);
                cards.extend(opponent.cards);
                let unique: BTreeSet<_> = cards.iter().map(|c| c.index()).collect();
                if unique.len() != cards.len() {
                    continue;
                }
                roots.push((
                    State {
                        hands: [h, o],
                        actor: 0,
                        invested: [0, 0],
                        checked: false,
                        aggressions: 0,
                        last_increment: 0,
                        history: String::new(),
                        finish: None,
                    },
                    hero.weight * opponent.weight,
                ));
            }
        }
        let total: f64 = roots.iter().map(|(_, weight)| weight).sum();
        for (_, weight) in &mut roots {
            *weight /= total;
        }
        roots
    }

    fn is_terminal(&self, state: &State) -> bool {
        state.finish.is_some()
    }

    fn utility(&self, state: &State, player: usize) -> f64 {
        let share = match state.finish.unwrap() {
            Finish::Fold(folder) => {
                if player == folder {
                    0.0
                } else {
                    1.0
                }
            }
            Finish::Showdown => {
                let my_rank = self.hands[player][state.hands[player]].strength;
                let their_rank = self.hands[1 - player][state.hands[1 - player]].strength;
                match my_rank.cmp(&their_rank) {
                    std::cmp::Ordering::Greater => 1.0,
                    std::cmp::Ordering::Equal => 0.5,
                    std::cmp::Ordering::Less => 0.0,
                }
            }
        };
        let initial = self.fixture.pot;
        let initial_share = if player == 0 {
            initial / 2
        } else {
            initial - initial / 2
        };
        let pot = initial + state.invested[0] + state.invested[1];
        share * (pot as f64 - self.rake(state)) - (initial_share + state.invested[player]) as f64
    }

    fn player_to_act(&self, state: &State) -> Option<usize> {
        Some(state.actor)
    }

    fn chance_outcomes(&self, _state: &State) -> Vec<(State, f64)> {
        panic!("river fixture has no public chance nodes")
    }

    fn num_actions(&self, state: &State) -> usize {
        self.actions(state).len()
    }

    fn next(&self, state: &State, action: usize) -> State {
        self.apply(state, self.actions(state)[action])
    }

    fn infoset_key(&self, state: &State) -> String {
        let combo = self.hands[state.actor][state.hands[state.actor]].combo();
        format!("{}|{combo}|{}", state.actor, state.history)
    }
}

struct CompleteProfile(HashMap<String, Vec<f64>>);

impl RefStrategy for CompleteProfile {
    fn strategy(&self, infoset: &str, num_actions: usize) -> Vec<f64> {
        let sigma = self
            .0
            .get(infoset)
            .expect("all oracle infosets must be exported");
        assert_eq!(sigma.len(), num_actions, "{infoset}");
        sigma.clone()
    }
}

fn compare(fixture: Fixture, iterations: u64) {
    let oracle = RiverOracle::new(fixture);
    let game = oracle.engine_game();
    assert_eq!(game.game.zero_sum, fixture.rake.is_none());
    oracle.assert_tree_matches(&game.game.tree, &game.node_info);
    let mut solver =
        Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(iterations.max(1)));
    solver.set_par(ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    });
    solver.run(iterations);
    let tree = &solver.game().tree;
    let mut profile = CompleteProfile(HashMap::new());
    for (id, node) in tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let actor = node.player.index();
        let history = &game.node_info[tree.tags[id] as usize].history;
        let sigma = solver.average_strategy_at(id as u32);
        let storage = tree.storage_ref(node);
        for &hand in &oracle.hands[actor] {
            let Some(local) = solver
                .game()
                .evaluator
                .hands()
                .index(node.player, hand.combo())
            else {
                continue;
            };
            let probabilities = (0..storage.num_actions as usize)
                .map(|a| sigma[a * storage.num_hands as usize + local] as f64)
                .collect();
            assert!(
                profile
                    .0
                    .insert(format!("{actor}|{}|{history}", hand.combo()), probabilities)
                    .is_none()
            );
        }
    }
    let gains = solver.exploitability();
    let mut oracle_ev_sum = 0.0;
    for player in [Player::P0, Player::P1] {
        let ev = expected_value(&oracle, &profile, player.index());
        let br = best_response_value(&oracle, &profile, player.index());
        oracle_ev_sum += ev;
        for (metric, actual, expected) in [
            ("EV", solver.expected_value(player), ev),
            ("BR", solver.best_response_value(player), br),
            ("gain", gains[player], br - ev),
        ] {
            assert!(
                (actual - expected).abs() < TOLERANCE,
                "{} at {iterations} iterations: {player:?} {metric}: engine {actual}, oracle {expected}",
                fixture.name
            );
        }
    }
    if fixture.rake.is_some() {
        // Every terminal pays positive rake, with different amounts across
        // paths. Deriving P1's EV from -EV(P0) would fail the comparisons above.
        assert!(oracle_ev_sum < -0.6);
    } else {
        assert!(oracle_ev_sum.abs() < TOLERANCE);
    }
}

#[test]
fn raised_and_raked_uniform_profiles_match_independent_oracle() {
    for fixture in FIXTURES {
        compare(fixture, 0);
    }
}

#[test]
fn raised_and_raked_average_profiles_match_independent_oracle() {
    for fixture in FIXTURES {
        compare(fixture, 3);
    }
}

#[test]
fn river_fixture_has_hand_checked_settlements_and_short_raise() {
    let oracle = RiverOracle::new(FIXTURES[1]);
    let root = oracle
        .initial_states()
        .into_iter()
        .find(|(state, _)| state.hands == [0, 0])
        .unwrap()
        .0; // AhAd versus AcAs: both make the same pair of aces.
    let checked = oracle.apply(&oracle.apply(&root, Action::Check), Action::Check);
    let bet = oracle.apply(&root, Action::Wager(5));
    let labels: Vec<_> = oracle.actions(&bet).iter().map(|a| a.label(true)).collect();
    assert_eq!(labels, ["fold", "call", "raise to 8"]);
    let folded = oracle.apply(&bet, Action::Fold);
    let raised = oracle.apply(&bet, Action::Wager(8));
    let called = oracle.apply(&raised, Action::Call);
    // The initial odd pot contributes 2/3 chips to the internal baseline.
    // Checked tie: net pot 4.375 / 2. Bet-fold: net pot 8.75, OOP wins.
    // Called short raise: net pot 19.5 / 2 after rake caps at 1.5.
    for (state, expected) in [
        (&checked, [0.1875, -0.8125]),
        (&folded, [1.75, -3.0]),
        (&called, [-0.25, -1.25]),
    ] {
        for (player, value) in expected.into_iter().enumerate() {
            assert_eq!(oracle.utility(state, player), value);
        }
    }
}

// T1-04's fixed bet-menu contrast. This is a test adapter, not a general
// migration API: it never transports by storage index or action position.
fn refinement_fixture(percentages: &'static [u32]) -> Fixture {
    Fixture {
        name: "river-bet-refinement",
        pot: 10,
        stack: 30,
        max_aggressions: 2,
        percentages,
        rake: None,
    }
}

fn all_infosets(oracle: &RiverOracle) -> HashMap<String, (State, Vec<Action>)> {
    fn visit(oracle: &RiverOracle, state: State, out: &mut HashMap<String, (State, Vec<Action>)>) {
        if oracle.is_terminal(&state) {
            return;
        }
        let actions = oracle.actions(&state);
        let entry = out
            .entry(oracle.infoset_key(&state))
            .or_insert_with(|| (state.clone(), actions.clone()));
        assert_eq!(entry.1, actions, "opponent cards cannot change my menu");
        for action in actions {
            visit(oracle, oracle.apply(&state, action), out);
        }
    }
    let mut result = HashMap::new();
    for (root, _) in oracle.initial_states() {
        visit(oracle, root, &mut result);
    }
    result
}

fn evaluate_in_engine(oracle: &RiverOracle, profile: &CompleteProfile) -> [[f64; 2]; 2] {
    let game = oracle.engine_game();
    oracle.assert_tree_matches(&game.game.tree, &game.node_info);
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(1));
    let mut state = solver.state();
    let StorageState::F32 { strategy_sum, .. } = &mut state.storage else {
        unreachable!()
    };
    let tree = &solver.game().tree;
    for (id, node) in tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let r = tree.storage_ref(node);
        let metadata = &game.node_info[tree.tags[id] as usize];
        // All retained initial-support hands receive a defined strategy.
        strategy_sum[r.offset..r.offset + r.len()].fill(1.0 / f32::from(r.num_actions));
        for hand in &oracle.hands[node.player.index()] {
            if hand.cards.iter().any(|card| oracle.board.contains(card)) {
                continue;
            }
            let key = format!(
                "{}|{}|{}",
                node.player.index(),
                hand.combo(),
                metadata.history
            );
            let sigma = profile.strategy(&key, r.num_actions as usize);
            let local = solver
                .game()
                .evaluator
                .hands()
                .index(node.player, hand.combo())
                .unwrap();
            for (a, probability) in sigma.into_iter().enumerate() {
                strategy_sum[r.offset + a * r.num_hands as usize + local] = probability as f32;
            }
        }
    }
    solver.restore_state(state).unwrap();
    let mut values = [[0.0; 2]; 2];
    for player in Player::BOTH {
        let expected = [
            expected_value(oracle, profile, player.index()),
            best_response_value(oracle, profile, player.index()),
        ];
        let actual = [
            solver.expected_value(player),
            solver.best_response_value(player),
        ];
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < TOLERANCE,
                "{player:?}: {actual} != {expected}"
            );
        }
        values[player.index()] = actual;
    }
    values
}

#[test]
fn coarse_bet_profile_lifts_completely_and_expanded_br_uses_the_fine_domain() {
    let coarse = RiverOracle::new(refinement_fixture(&[50]));
    let fine = RiverOracle::new(refinement_fixture(&[50, 100]));
    let root = fine.initial_states()[0].0.clone();
    assert_eq!(coarse.actions(&root), [Action::Check, Action::Wager(5)]);
    assert_eq!(
        fine.actions(&root),
        [Action::Check, Action::Wager(5), Action::Wager(10)]
    );
    let bet5 = fine.apply(&root, Action::Wager(5));
    assert_eq!(
        coarse.actions(&bet5),
        [Action::Fold, Action::Call, Action::Wager(15)]
    );
    assert_eq!(
        fine.actions(&bet5),
        [
            Action::Fold,
            Action::Call,
            Action::Wager(15),
            Action::Wager(25)
        ]
    );
    let bet10 = fine.apply(&root, Action::Wager(10));
    assert_eq!(
        fine.actions(&bet10),
        [
            Action::Fold,
            Action::Call,
            Action::Wager(25),
            Action::Wager(30)
        ]
    );

    let coarse_infosets = all_infosets(&coarse);
    let fine_infosets = all_infosets(&fine);
    assert_eq!(coarse_infosets.len(), 21); // 6 public decisions, 3/4 live private hands.
    assert_eq!(fine_infosets.len(), 49); // 14 public decisions, all legal private worlds.
    let mut coarse_profile = CompleteProfile(HashMap::new());
    for (key, (state, actions)) in &coarse_infosets {
        let mut sigma = match actions.len() {
            2 => vec![0.25, 0.75],
            3 => vec![0.25, 0.5, 0.25],
            _ => panic!("unexpected fixed coarse menu"),
        };
        // Same own hand/history gets the same policy in every opponent world.
        if state.hands[state.actor] % 2 == 1 {
            sigma.reverse();
        }
        coarse_profile.0.insert(key.clone(), sigma);
    }

    let mut lifted = CompleteProfile(HashMap::new());
    let mut added_history_infosets = 0;
    for (key, (_, fine_actions)) in &fine_infosets {
        let sigma = if let Some((_, coarse_actions)) = coarse_infosets.get(key) {
            let mut result = vec![0.0; fine_actions.len()];
            for (action, probability) in coarse_actions.iter().zip(&coarse_profile.0[key]) {
                let position = fine_actions
                    .iter()
                    .position(|candidate| candidate == action)
                    .unwrap();
                result[position] = *probability;
            }
            result
        } else {
            added_history_infosets += 1;
            // Explicitly selected completion, including both seats after the
            // first added action. Own-profile zero reach is not an omission.
            vec![1.0 / fine_actions.len() as f64; fine_actions.len()]
        };
        assert!(sigma.iter().all(|p| p.is_finite() && *p >= 0.0));
        assert!((sigma.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        lifted.0.insert(key.clone(), sigma);
    }
    assert_eq!(added_history_infosets, 28);
    assert_eq!(lifted.0.len(), fine_infosets.len());

    // Restore every original row by concrete action meaning, not new indices.
    let mut restored = CompleteProfile(HashMap::new());
    for (key, (_, coarse_actions)) in &coarse_infosets {
        let fine_actions = &fine_infosets[key].1;
        let sigma: Vec<_> = coarse_actions
            .iter()
            .map(|action| {
                lifted.0[key][fine_actions
                    .iter()
                    .position(|candidate| candidate == action)
                    .unwrap()]
            })
            .collect();
        for (a, action) in fine_actions.iter().enumerate() {
            if !coarse_actions.contains(action) {
                assert_eq!(lifted.0[key][a], 0.0);
            }
        }
        restored.0.insert(key.clone(), sigma);
    }
    assert_eq!(restored.0, coarse_profile.0);
    let coarse_values = evaluate_in_engine(&coarse, &coarse_profile);
    let fine_values = evaluate_in_engine(&fine, &lifted);
    for player in 0..2 {
        let coarse_ev = expected_value(&coarse, &coarse_profile, player);
        let fine_ev = expected_value(&fine, &lifted, player);
        let coarse_br = best_response_value(&coarse, &coarse_profile, player);
        let fine_br = best_response_value(&fine, &lifted, player);
        assert!((coarse_ev - fine_ev).abs() < 1e-12);
        assert!(fine_br + 1e-12 >= coarse_br);
        assert!((coarse_values[player][0] - fine_values[player][0]).abs() < TOLERANCE);
        assert!(fine_values[player][1] + TOLERANCE >= coarse_values[player][1]);
        println!(
            "bet refinement P{player}: coarse EV={coarse_ev:.12} BR={coarse_br:.12}; lifted EV={fine_ev:.12} expanded BR={fine_br:.12}"
        );
    }
}
