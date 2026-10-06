//! Bitwise scheduling and fused-evaluation checks, including nested action
//! forks without chance and mixed-size transition fan-out.
use std::collections::BTreeSet;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use hu_engine::{
    CompiledGame, Dcfr, F32Storage, I16Storage, NodeKind, ParConfig, PublicTree, ReachMap, Solver,
    SparseTransition, Storage, StorageState, TempNode, TerminalEvaluator, TreeSpec, reach_at,
};
use nlh::{PerPlayer, Player};

#[derive(Default)]
struct Evaluator {
    general_sum: bool,
    calls: AtomicUsize,
    workers: Mutex<BTreeSet<usize>>,
}
impl TerminalEvaluator for Evaluator {
    fn eval(&self, terminal: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.workers
            .lock()
            .unwrap()
            .insert(rayon::current_thread_index().unwrap());
        let total: f32 = reach.iter().sum();
        let other: f32 = reach
            .iter()
            .enumerate()
            .map(|(h, &r)| r * ((h * 7 % 19) as f32 - 9.0))
            .sum();
        let t = (terminal * 13 % 23) as f32 - 11.0;
        for (h, v) in out.iter_mut().enumerate() {
            let hero = (h * 7 % 19) as f32 - 9.0;
            *v = match p {
                Player::P0 => (t + hero) * total - other,
                Player::P1 => (-t + hero) * total - other,
            };
            if self.general_sum {
                *v -= 0.17 * total;
            }
        }
    }
}
fn action_tree(depth: usize, p: Player, next: &mut u32) -> TempNode {
    if depth == 0 {
        let id = *next;
        *next += 1;
        TempNode::Terminal { id, tag: 0 }
    } else {
        TempNode::Action {
            player: p,
            tag: 0,
            children: (0..3)
                .map(|_| action_tree(depth - 1, p.opponent(), next))
                .collect(),
        }
    }
}
fn game(
    dim: usize,
    chance: bool,
    general_sum: bool,
    root_player: Player,
) -> CompiledGame<Evaluator> {
    let mut next = 0;
    let root = if chance {
        TempNode::Chance {
            tag: 0,
            deals: (0..4)
                .map(|i| {
                    let maps = if i % 2 == 0 {
                        PerPlayer::new(ReachMap::Identity, ReachMap::Mask(0))
                    } else {
                        PerPlayer::new(ReachMap::Transition(0), ReachMap::Transition(1))
                    };
                    (
                        0.13 + i as f32 * 0.07,
                        maps,
                        action_tree(6, root_player, &mut next),
                    )
                })
                .collect(),
        }
    } else {
        action_tree(6, root_player, &mut next)
    };
    let transitions = [dim / 2, dim / 2 + 1]
        .into_iter()
        .map(|out_dim| SparseTransition {
            in_dim: dim as u32,
            out_dim: out_dim as u32,
            entries: (0..dim)
                .map(|h| (h as u32, (h % out_dim) as u32, 1.0))
                .collect(),
        })
        .collect();
    let tree = PublicTree::compile(TreeSpec {
        root,
        transitions,
        masks: vec![
            (0..dim)
                .map(|h| if h % 5 == 0 { 0.0 } else { 1.0 })
                .collect(),
        ],
        root_dims: PerPlayer::new(dim as u32, dim as u32),
    });
    CompiledGame {
        tree,
        evaluator: Evaluator {
            general_sum,
            ..Default::default()
        },
        root_ranges: PerPlayer::new(
            (0..dim).map(|h| (h % 7) as f32 / 9.0).collect(),
            (0..dim).map(|h| (h % 11) as f32 / 13.0).collect(),
        ),
        normalizer: dim.max(1) as f64 * dim.max(1) as f64,
        zero_sum: !general_sum,
    }
}
fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}
fn state_bits(state: StorageState) -> Vec<u32> {
    match state {
        StorageState::F32 {
            regrets,
            strategy_sum,
        } => [bits(&regrets), bits(&strategy_sum)].concat(),
        StorageState::I16 {
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        } => {
            let mut v: Vec<_> = regrets
                .into_iter()
                .chain(strategy_sum)
                .map(|x| x as u16 as u32)
                .collect();
            v.extend(bits(&regret_scales));
            v.extend(bits(&strategy_scales));
            v
        }
    }
}
#[derive(Debug, PartialEq)]
struct Snapshot {
    states: Vec<Vec<u32>>,
    values: Vec<u64>,
}
fn solve<S: Storage>(
    threads: usize,
    par: ParConfig,
    dim: usize,
    chance: bool,
    general_sum: bool,
    root_player: Player,
) -> Snapshot {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| {
            let mut solver = Solver::<_, S>::new(
                game(dim, chance, general_sum, root_player),
                Box::<Dcfr>::default(),
                None,
            );
            solver.set_par(par);
            let mut states = Vec::new();
            for _ in 0..5 {
                solver.step();
                states.push(state_bits(solver.state().storage));
            }
            if threads > 1 && dim == 257 {
                assert!(
                    solver.game().evaluator.workers.lock().unwrap().len() > 1,
                    "large chance-free action subtrees must use multiple workers"
                );
            }
            let counter = &solver.game().evaluator.calls;
            counter.store(0, Ordering::Relaxed);
            let expl = solver.exploitability();
            let terminals = solver
                .game()
                .tree
                .nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Terminal)
                .count();
            assert_eq!(
                counter.load(Ordering::Relaxed),
                2 * terminals,
                "one terminal evaluation per seat"
            );
            let mut values = Vec::new();
            let ev0 = solver.expected_value(Player::P0);
            for p in Player::BOTH {
                let ev = solver.expected_value(p);
                let br = solver.best_response_value(p);
                let profile_ev = if p == Player::P1 && !general_sum {
                    -ev0
                } else {
                    ev
                };
                assert_eq!(
                    expl[p].to_bits(),
                    (br - profile_ev).to_bits(),
                    "fused EV/BR {p:?}"
                );
                values.extend([ev.to_bits(), br.to_bits(), expl[p].to_bits()]);
                let everywhere = solver.expected_values_everywhere(p);
                for entry in &everywhere {
                    values.extend(entry.as_ref().unwrap().iter().map(|v| v.to_bits() as u64));
                }
                let tree = &solver.game().tree;
                // Root plus a deeper action node exercises the public per-node APIs.
                for node in [0, tree.storage_refs.len().min(9) as u32] {
                    let reaches = reach_at(
                        tree,
                        solver.game().root_ranges.as_ref().map(|r| r.as_slice()),
                        node,
                        |id, _, out| out.copy_from_slice(&solver.average_strategy_at(id)),
                    );
                    let reach = reaches.as_ref().map(|r| r.as_slice());
                    let evs = solver.expected_values_at(node, p, reach);
                    let brs = solver.best_response_values_at(node, p, reach);
                    if tree.node(node).kind == NodeKind::Action {
                        let idx = tree.storage_ref(tree.node(node)).index as usize;
                        assert_eq!(bits(&evs), bits(everywhere[idx].as_ref().unwrap()));
                    }
                    values.extend(evs.into_iter().chain(brs).map(|v| v.to_bits() as u64));
                }
            }
            Snapshot { states, values }
        })
}
fn check<S: Storage>(dim: usize, chance: bool, general_sum: bool, player: Player) {
    let off = ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    };
    let on = ParConfig {
        chance_depth: 2,
        min_children: 2,
    };
    let baseline = solve::<S>(1, off, dim, chance, general_sum, player);
    for (threads, par) in [(1, on), (2, off), (4, on)] {
        assert_eq!(
            baseline,
            solve::<S>(threads, par, dim, chance, general_sum, player),
            "threads={threads}, dim={dim}, chance={chance}, general_sum={general_sum}"
        );
    }
}
#[test]
fn action_and_fused_values_f32() {
    for general_sum in [false, true] {
        for player in Player::BOTH {
            check::<F32Storage>(257, false, general_sum, player);
        }
    }
    check::<F32Storage>(8, false, true, Player::P0);
}
#[test]
fn action_and_fused_values_i16() {
    check::<I16Storage>(257, false, true, Player::P0);
    check::<I16Storage>(8, false, false, Player::P1);
}
#[test]
fn mixed_transition_chance_and_nested_actions() {
    check::<F32Storage>(257, true, true, Player::P0);
    check::<I16Storage>(257, true, false, Player::P1);
}

#[test]
fn empty_private_spaces_preserve_traversal() {
    check::<F32Storage>(0, false, true, Player::P0);
    check::<I16Storage>(0, false, false, Player::P1);
    check_asymmetric_empty::<F32Storage>();
    check_asymmetric_empty::<I16Storage>();
}

fn check_asymmetric_empty<S: Storage>() {
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let mut game = game(257, false, false, Player::P0);
                game.tree = PublicTree::compile(TreeSpec {
                    root: action_tree(6, Player::P0, &mut 0),
                    root_dims: PerPlayer::new(0, 257),
                    masks: Vec::new(),
                    transitions: Vec::new(),
                });
                game.root_ranges[Player::P0].clear();
                game.normalizer = 1.0;
                let mut solver = Solver::<_, S>::new(game, Box::<Dcfr>::default(), None);
                solver.set_par(ParConfig {
                    chance_depth: 0,
                    min_children: usize::MAX,
                });
                let mut states = Vec::new();
                for _ in 0..5 {
                    solver.step();
                    states.push(state_bits(solver.state().storage));
                }
                let mut values = Vec::new();
                let expl = solver.exploitability();
                for p in Player::BOTH {
                    values.extend([
                        solver.expected_value(p).to_bits(),
                        solver.best_response_value(p).to_bits(),
                        expl[p].to_bits(),
                    ]);
                    let reach = solver.game().root_ranges.as_ref().map(|r| r.as_slice());
                    values.extend(
                        solver
                            .expected_values_at(0, p, reach)
                            .into_iter()
                            .chain(solver.best_response_values_at(0, p, reach))
                            .map(|v| v.to_bits() as u64),
                    );
                    for entry in solver.expected_values_everywhere(p) {
                        values.extend(entry.unwrap().into_iter().map(|v| v.to_bits() as u64));
                    }
                }
                Snapshot { states, values }
            })
    };
    assert_eq!(run(1), run(4));
}
