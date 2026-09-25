//! Independent finite river games for T1-01: weighted/blocking ranges,
//! raises, short all-ins, odd pots, ties, and action-dependent rake.
//! The scalar side does not call production betting, rank, or payoff code.
//! Only card/combo identifiers and the frozen `cfr-ref` evaluator are shared.

use std::collections::{BTreeSet, HashMap};

use cards::{Card, Chips, PerPlayer, Player, Range, combo_index};
use cfr_ref::{RefGame, RefStrategy, best_response_value, expected_value};
use engine::{Dcfr, F32Storage, NodeKind, ParConfig, PublicTree, Solver};
use game::{ChipEv, NoRake, PayoffPipeline, PercentCapRake, RakeModel};
use holdem::{PerStreet, PostflopConfig, PostflopNodeInfo, StreetTree, build_postflop_game};

const POT: u32 = 5;
const MAX_AGGRESSIONS: u32 = 3;
const TOLERANCE: f64 = 1e-4;

#[derive(Clone, Copy)]
struct Fixture {
    name: &'static str,
    stack: u32,
    rake: Option<(f64, f64)>,
}

const FIXTURES: [Fixture; 3] = [
    Fixture {
        name: "river-raised-no-rake",
        stack: 40,
        rake: None,
    },
    Fixture {
        name: "river-short-allin-capped-rake",
        stack: 8,
        rake: Some((0.125, 1.5)),
    },
    Fixture {
        name: "river-raised-uncapped-rake",
        stack: 40,
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

#[derive(Clone, Copy, Debug)]
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
        if state.aggressions == MAX_AGGRESSIONS || facing == self.fixture.stack {
            return actions;
        }
        // Half-pot rounds up at odd integer pots; pot-sized wagers are exact.
        // Both sizes are raises ABOVE the matched contribution. A full raise
        // must repeat the last increment, but a smaller all-in is legal.
        let pot_after_call = POT + 2 * facing;
        let minimum = facing + state.last_increment.max(1);
        let targets: BTreeSet<_> = [pot_after_call.div_ceil(2), pot_after_call]
            .into_iter()
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
            ((POT + state.invested[0] + state.invested[1]) as f64 * rate).min(cap)
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
        build_postflop_game(
            &PostflopConfig {
                board: self.board.clone(),
                ranges: PerPlayer::new(range(&self.hands[0]), range(&self.hands[1])),
                pot: Chips(POT),
                effective_stack: Chips(self.fixture.stack),
                streets: PerStreet {
                    flop: StreetTree::pot_fractions(&[], &[], 0),
                    turn: StreetTree::pot_fractions(&[], &[], 0),
                    river: StreetTree::pot_fractions(&[0.5, 1.0], &[0.5, 1.0], MAX_AGGRESSIONS),
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
            for (p, initial) in [(Player::P0, POT / 2), (Player::P1, POT - POT / 2)] {
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
        let initial_share = if player == 0 { POT / 2 } else { POT - POT / 2 };
        let pot = POT + state.invested[0] + state.invested[1];
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
            let probabilities = (0..storage.num_actions as usize)
                .map(|a| sigma[a * storage.num_hands as usize + hand.combo()] as f64)
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
