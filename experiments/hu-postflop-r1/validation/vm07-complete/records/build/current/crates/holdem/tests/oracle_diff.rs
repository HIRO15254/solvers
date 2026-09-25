//! Multi-street differential test: a turn-start micro hold'em game (tiny
//! explicit-combo ranges, one pot-sized bet per street, no raises, iso
//! off) is solved by the vectorized engine and re-evaluated by an
//! independent scalar implementation of the same rules built on the frozen
//! `cfr-ref` oracle core. Expected value and best-response value must
//! agree — for converged and barely-converged profiles alike.

use std::collections::HashMap;

use cards::{Card, CardSet, NUM_COMBOS, PerPlayer, Player, Range, combo_cards, rank_of};
use cfr_ref::{RefGame, best_response_value, expected_value};
use engine::{Dcfr, F32Storage, NodeKind, ParConfig, Solver};
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{PerStreet, PostflopConfig, StreetTree, build_postflop_game};

const BOARD: &str = "Ks Qs 7h 2d";
const P0_RANGE: &str = "AhAd,QhQd,7c7d";
const P1_RANGE: &str = "KhKd,JhJd,8c8h";
const POT: u32 = 4;
const STACK: u32 = 100;

fn board_cards() -> [Card; 4] {
    BOARD
        .split_whitespace()
        .map(|c| c.parse().unwrap())
        .collect::<Vec<Card>>()
        .try_into()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Scalar oracle side: per-(combo pair, history) states mirroring the
// builder's rules (sizing = round(f * pot-after-call), history tokens
// x / c / f / r{total} / [card]).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fin {
    Fold { folder: usize },
    Showdown,
}

#[derive(Clone, Debug)]
struct MicroState {
    combos: [usize; 2],
    street: usize, // 0 = turn, 1 = river
    river: Option<Card>,
    to_act: usize,
    contrib: [u32; 2],
    outstanding: u32,
    raises_used: u32,
    first_checked: bool,
    history: String,
    finished: Option<Fin>,
    awaiting_river: bool,
}

struct MicroHoldem {
    board: [Card; 4],
    ranges: [Vec<usize>; 2],
}

impl MicroHoldem {
    fn new() -> Self {
        let combos_of = |s: &str| -> Vec<usize> {
            let range: Range = s.parse().unwrap();
            (0..NUM_COMBOS).filter(|&c| range.weight(c) > 0.0).collect()
        };
        MicroHoldem {
            board: board_cards(),
            ranges: [combos_of(P0_RANGE), combos_of(P1_RANGE)],
        }
    }

    /// Mirrors the builder's action menu order: [check(, bet)] or
    /// [fold, call(, raise...)] — raises never fire here (max_raises 1).
    fn actions(&self, s: &MicroState) -> Vec<char> {
        let behind = STACK - s.contrib[s.to_act];
        if s.outstanding == 0 {
            let mut acts = vec!['x'];
            if s.raises_used < 1 && behind > 0 {
                acts.push('b');
            }
            acts
        } else {
            vec!['f', 'c']
        }
    }

    fn apply(&self, s: &MicroState, action: char) -> MicroState {
        let mut next = s.clone();
        let actor = s.to_act;
        let pot_now = POT + s.contrib[0] + s.contrib[1];
        match action {
            'x' => {
                next.history.push('x');
                if s.first_checked {
                    self.street_done(&mut next);
                } else {
                    next.first_checked = true;
                    next.to_act = 1 - actor;
                }
            }
            'b' => {
                let behind = STACK - s.contrib[actor];
                let raw = (1.0 * pot_now as f64).round() as u32;
                let extra = raw.max(1).min(behind);
                next.contrib[actor] += extra;
                next.outstanding = extra;
                next.raises_used += 1;
                next.to_act = 1 - actor;
                next.history.push_str(&format!("r{}", next.contrib[actor]));
            }
            'f' => {
                next.history.push('f');
                next.finished = Some(Fin::Fold { folder: actor });
            }
            'c' => {
                next.history.push('c');
                next.contrib[actor] += s.outstanding;
                next.outstanding = 0;
                self.street_done(&mut next);
            }
            _ => unreachable!(),
        }
        next
    }

    fn street_done(&self, s: &mut MicroState) {
        if s.street == 1 {
            s.finished = Some(Fin::Showdown);
        } else {
            s.awaiting_river = true;
        }
    }
}

impl RefGame for MicroHoldem {
    type State = MicroState;

    fn initial_states(&self) -> Vec<(MicroState, f64)> {
        let mut roots = Vec::new();
        for &h in &self.ranges[0] {
            for &o in &self.ranges[1] {
                let (h1, h2) = combo_cards(h);
                let (o1, o2) = combo_cards(o);
                if h1 == o1 || h1 == o2 || h2 == o1 || h2 == o2 {
                    continue;
                }
                roots.push((
                    MicroState {
                        combos: [h, o],
                        street: 0,
                        river: None,
                        to_act: 0,
                        contrib: [0, 0],
                        outstanding: 0,
                        raises_used: 0,
                        first_checked: false,
                        history: String::new(),
                        finished: None,
                        awaiting_river: false,
                    },
                    1.0,
                ));
            }
        }
        let w = 1.0 / roots.len() as f64;
        roots.iter_mut().for_each(|(_, p)| *p = w);
        roots
    }

    fn is_terminal(&self, s: &MicroState) -> bool {
        s.finished.is_some()
    }

    fn utility(&self, s: &MicroState, player: usize) -> f64 {
        let total: [f64; 2] = [
            (POT / 2 + s.contrib[0]) as f64,
            (POT / 2 + s.contrib[1]) as f64,
        ];
        let pot = total[0] + total[1];
        match s.finished.unwrap() {
            Fin::Fold { folder } => {
                if player == folder {
                    -total[player]
                } else {
                    pot - total[player]
                }
            }
            Fin::Showdown => {
                let rank = |combo: usize| {
                    let (c1, c2) = combo_cards(combo);
                    rank_of(self.board.iter().copied().chain([s.river.unwrap(), c1, c2]))
                };
                match rank(s.combos[player]).cmp(&rank(s.combos[1 - player])) {
                    std::cmp::Ordering::Greater => pot - total[player],
                    std::cmp::Ordering::Less => -total[player],
                    std::cmp::Ordering::Equal => pot / 2.0 - total[player],
                }
            }
        }
    }

    fn player_to_act(&self, s: &MicroState) -> Option<usize> {
        if s.awaiting_river {
            None
        } else {
            Some(s.to_act)
        }
    }

    fn chance_outcomes(&self, s: &MicroState) -> Vec<(MicroState, f64)> {
        debug_assert!(s.awaiting_river);
        let mut used: CardSet = self.board.iter().copied().collect();
        for &combo in &s.combos {
            let (c1, c2) = combo_cards(combo);
            used.insert(c1);
            used.insert(c2);
        }
        (!used)
            .iter()
            .map(|card| {
                let mut next = s.clone();
                next.river = Some(card);
                next.awaiting_river = false;
                next.street = 1;
                next.first_checked = false;
                next.to_act = 0;
                next.outstanding = 0;
                next.raises_used = 0;
                next.history.push_str(&format!("[{card}]"));
                (next, 1.0 / 44.0)
            })
            .collect()
    }

    fn num_actions(&self, s: &MicroState) -> usize {
        self.actions(s).len()
    }

    fn next(&self, s: &MicroState, action: usize) -> MicroState {
        self.apply(s, self.actions(s)[action])
    }

    fn infoset_key(&self, s: &MicroState) -> String {
        format!("{}|{}", s.combos[s.to_act], s.history)
    }
}

// ---------------------------------------------------------------------------
// Engine side.
// ---------------------------------------------------------------------------

fn engine_game() -> holdem::PostflopGame {
    let pipeline = PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    };
    build_postflop_game(
        &PostflopConfig {
            board: board_cards().to_vec(),
            ranges: PerPlayer::new(P0_RANGE.parse().unwrap(), P1_RANGE.parse().unwrap()),
            pot: cards::Chips(POT),
            effective_stack: cards::Chips(STACK),
            streets: PerStreet {
                flop: StreetTree::pot_fractions(&[], &[], 0),
                turn: StreetTree::pot_fractions(&[1.0], &[1.0], 1),
                river: StreetTree::pot_fractions(&[1.0], &[1.0], 1),
            },
            iso_merging: false,
            ..Default::default()
        },
        pipeline,
    )
}

fn export_profile(
    node_info: &[holdem::PostflopNodeInfo],
    solver: &Solver<holdem::PostflopEvaluator, F32Storage>,
) -> HashMap<String, Vec<f64>> {
    let tree = &solver.game().tree;
    let mut profile = HashMap::new();
    for node_id in 0..tree.nodes.len() as u32 {
        let node = tree.node(node_id);
        if node.kind != NodeKind::Action {
            continue;
        }
        let info = &node_info[tree.tags[node_id as usize] as usize];
        let sigma = solver.average_strategy_at(node_id);
        let sref = tree.storage_ref(node);
        let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
        for hand in 0..num_hands {
            let key = format!("{hand}|{}", info.history);
            let dist: Vec<f64> = (0..num_actions)
                .map(|a| sigma[a * num_hands + hand] as f64)
                .collect();
            profile.insert(key, dist);
        }
    }
    profile
}

fn sequential() -> ParConfig {
    ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    }
}

fn assert_engine_matches_oracle(iters: u64, tol: f64) {
    let game = engine_game();
    let node_info = game.node_info.clone();
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(iters));
    solver.set_par(sequential());
    solver.run(iters);
    let profile = export_profile(&node_info, &solver);

    let oracle = MicroHoldem::new();
    for (p, oracle_p) in [(Player::P0, 0), (Player::P1, 1)] {
        let engine_ev = solver.expected_value(p);
        let oracle_ev = expected_value(&oracle, &profile, oracle_p);
        assert!(
            (engine_ev - oracle_ev).abs() < tol,
            "EV mismatch for {p:?} at {iters} iters: engine {engine_ev} vs oracle {oracle_ev}"
        );
        let engine_br = solver.best_response_value(p);
        let oracle_br = best_response_value(&oracle, &profile, oracle_p);
        assert!(
            (engine_br - oracle_br).abs() < tol,
            "BR mismatch for {p:?} at {iters} iters: engine {engine_br} vs oracle {oracle_br}"
        );
    }
}

#[test]
fn multistreet_engine_matches_scalar_oracle_early_iterates() {
    // Agreement must hold for arbitrary (non-converged) profiles too.
    assert_engine_matches_oracle(3, 1e-4);
}

#[test]
#[ignore = "200-iteration turn-tree solve is slow unoptimized; CI runs it in release"]
fn multistreet_engine_matches_scalar_oracle() {
    assert_engine_matches_oracle(200, 1e-4);
}

#[test]
fn uniform_profiles_agree_between_engine_and_oracle() {
    // Iteration 0: the engine's average strategy is uniform; an empty
    // oracle profile is uniform too. Any disagreement is a rules mismatch
    // rather than a solve bug, so this is the first test to consult when
    // the differential tests above fail.
    let game = engine_game();
    let solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(1));
    let oracle = MicroHoldem::new();
    let empty: HashMap<String, Vec<f64>> = HashMap::new();
    for (p, op) in [(Player::P0, 0), (Player::P1, 1)] {
        let e = solver.expected_value(p);
        let o = expected_value(&oracle, &empty, op);
        assert!(
            (e - o).abs() < 1e-4,
            "uniform EV mismatch for {p:?}: engine {e} vs oracle {o}"
        );
    }
}
