//! Install as holdem/tests/root_action_oracle.rs; cfr-ref remains unmodified.
//! Independent scalar state machine, including zero-payoff removed chance mass.
#[path = "../../engine/tests/root_action_paths.rs"]
mod paths;

use cards::Player;
use cfr_ref::{RefGame, best_response_value, expected_value};
use engine::{Dcfr, F32Storage, I16Storage, ParConfig, Solver, Storage};

#[derive(Clone)]
struct State {
    phase: u8,
    private: usize,
    a: usize,
    b: usize,
    deal: usize,
    z: usize,
    c: usize,
    d: usize,
    removed: bool,
}

struct Scalar;

impl Scalar {
    // These rules are declared independently; no engine tree, map, payoff
    // function or fixture constants are read by the scalar game.
    fn hands(s: &State) -> (usize, usize) {
        match s.deal {
            3 | 5 => (0, s.private),
            4 | 7 => (1, 2 * s.private),
            _ => (0, 0),
        }
    }
}

impl RefGame for Scalar {
    type State = State;

    fn initial_states(&self) -> Vec<(State, f64)> {
        (0..2)
            .map(|private| {
                (
                    State {
                        phase: 0,
                        private,
                        a: 0,
                        b: 0,
                        deal: 0,
                        z: 0,
                        c: 0,
                        d: 0,
                        removed: false,
                    },
                    0.5,
                )
            })
            .collect()
    }

    fn is_terminal(&self, s: &State) -> bool {
        s.phase == 6
    }

    fn utility(&self, s: &State, player: usize) -> f64 {
        assert_eq!(s.phase, 6);
        if s.removed {
            return 0.0;
        }
        let (h0, h1) = Self::hands(s);
        // Separate payoff tables rather than reusing the engine evaluator.
        let value = if player == 0 {
            [2, 5][s.a] + [0, -2][s.b] + [0, 4][s.c] + [0, -1][s.d] + [0, 1][s.z] + h0 as i32
                - 2 * h1 as i32
        } else {
            [-1, 0][s.a]
                + [0, 5][s.b]
                + [0, -3][s.c]
                + [0, 2][s.d]
                + [0, -1][s.z]
                + 2 * h0 as i32
                + h1 as i32
        };
        value as f64
    }

    fn player_to_act(&self, s: &State) -> Option<usize> {
        match s.phase {
            0 | 4 => Some(0),
            1 | 5 => Some(1),
            2 | 3 => None,
            _ => panic!("terminal actor query"),
        }
    }

    fn chance_outcomes(&self, s: &State) -> Vec<(State, f64)> {
        match s.phase {
            2 => (0..8)
                .map(|deal| {
                    let mut next = s.clone();
                    next.deal = deal;
                    if matches!(deal, 3 | 4 | 5 | 7) {
                        next.phase = 3;
                    } else {
                        next.phase = 6;
                        next.removed = true;
                    }
                    (next, 0.125)
                })
                .collect(),
            3 => (0..2)
                .map(|z| {
                    let mut next = s.clone();
                    next.z = z;
                    next.phase = 4;
                    (next, 0.5)
                })
                .collect(),
            _ => panic!("non-chance outcomes query"),
        }
    }

    fn num_actions(&self, s: &State) -> usize {
        assert!(matches!(s.phase, 0 | 1 | 4 | 5));
        2
    }

    fn next(&self, s: &State, action: usize) -> State {
        assert!(action < 2);
        let mut next = s.clone();
        match s.phase {
            0 => {
                next.a = action;
                next.phase = 1;
            }
            1 => {
                next.b = action;
                next.phase = 2;
            }
            4 => {
                next.c = action;
                next.phase = 5;
            }
            5 => {
                next.d = action;
                next.phase = 6;
            }
            _ => panic!("non-action successor query"),
        }
        next
    }

    fn infoset_key(&self, s: &State) -> String {
        let (h0, h1) = Self::hands(s);
        match s.phase {
            0 => "0|root".to_owned(),
            1 => format!("{}|pre:{}", if s.private == 0 { 0 } else { 2047 }, s.a),
            4 => format!("{h0}|play0:{},{},{},{}", s.a, s.b, s.deal, s.z),
            5 => format!("{h1}|play1:{},{},{},{},{}", s.a, s.b, s.deal, s.z, s.c),
            _ => panic!("non-action infoset query"),
        }
    }
}

fn compare<S: Storage>() {
    for workers in [1, 2] {
        let mut solver = Solver::<_, S>::new(paths::fixture(), Box::<Dcfr>::default(), Some(2));
        solver.set_par(ParConfig {
            chance_depth: 1,
            min_children: 2,
        });
        rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| {
                solver.run(2);
                let profile = paths::export(&solver);
                for player in Player::BOTH {
                    let p = player.index();
                    let ev = solver.expected_value(player);
                    let br = solver.best_response_value(player);
                    let ref_ev = expected_value(&Scalar, &profile, p);
                    let ref_br = best_response_value(&Scalar, &profile, p);
                    assert!(
                        (ev - ref_ev).abs() <= 1e-4,
                        "workers={workers}, player={p}, EV engine={ev}, oracle={ref_ev}"
                    );
                    assert!(
                        (br - ref_br).abs() <= 1e-4,
                        "workers={workers}, player={p}, BR engine={br}, oracle={ref_br}"
                    );
                }
            });
    }
}

#[test]
fn root_action_profile_f32_matches_independent_scalar_ev_br() {
    compare::<F32Storage>();
}

#[test]
fn root_action_profile_i16_matches_independent_scalar_ev_br() {
    compare::<I16Storage>();
}
