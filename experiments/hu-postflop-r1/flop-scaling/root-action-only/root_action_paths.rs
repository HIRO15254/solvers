//! Research-only integration fixture; install as engine/tests/root_action_paths.rs.
//! A wide but sparse prefix crosses the existing grain with no large payoff matrix.
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use cards::{PerPlayer, Player};
use engine::{
    CompiledGame, Dcfr, F32Storage, I16Storage, NodeKind, ParConfig, PublicTree, ReachMap, Solver,
    SparseTransition, Storage, StorageState, TempNode, TerminalEvaluator, TreeSpec,
};

pub const WIDTH: usize = 2_048;
pub const DIMS: [(usize, usize); 8] = [
    (0, 2),
    (2, 0),
    (0, 0),
    (1, 2),
    (2, 3),
    (1, 2),
    (0, 0),
    (2, 3),
];

#[derive(Default)]
struct Rendezvous {
    threads: Mutex<[Option<std::thread::ThreadId>; 2]>,
    changed: Condvar,
}

impl Rendezvous {
    fn arrive(&self, branch: usize) {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut threads = self.threads.lock().unwrap();
        threads[branch] = Some(std::thread::current().id());
        self.changed.notify_all();
        while threads.iter().any(Option::is_none) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "root branches did not overlap before fixed timeout"
            );
            let (next, waited) = self.changed.wait_timeout(threads, remaining).unwrap();
            threads = next;
            assert!(
                !waited.timed_out() || threads.iter().all(Option::is_some),
                "root action fork did not execute"
            );
        }
        assert_ne!(
            threads[0], threads[1],
            "both branches must occupy different workers"
        );
    }
}

pub struct Payoffs {
    rendezvous: Option<Arc<Rendezvous>>,
}

impl TerminalEvaluator for Payoffs {
    fn eval(&self, terminal: u32, player: Player, opp_reach: &[f32], out: &mut [f32]) {
        let d = terminal % 2;
        let c = (terminal / 2) % 2;
        let z = (terminal / 4) % 2;
        let deal = (terminal / 8) % 8;
        let b = (terminal / 64) % 2;
        let a = terminal / 128;
        if player == Player::P0 && b == 0 && deal == 3 && z == 0 && c == 0 && d == 0 {
            if let Some(gate) = &self.rendezvous {
                gate.arrive(a as usize);
            }
        }
        for (h, value) in out.iter_mut().enumerate() {
            *value = 0.0;
            for (o, reach) in opp_reach.iter().copied().enumerate() {
                let (h0, h1) = if player == Player::P0 { (h, o) } else { (o, h) };
                let utility = match player {
                    Player::P0 => {
                        2 + 3 * a as i32 - 2 * b as i32 + 4 * c as i32 - d as i32
                            + z as i32
                            + h0 as i32
                            - 2 * h1 as i32
                    }
                    Player::P1 => {
                        -1 + a as i32 + 5 * b as i32 - 3 * c as i32 + 2 * d as i32 - z as i32
                            + 2 * h0 as i32
                            + h1 as i32
                    }
                };
                *value += utility as f32 * reach;
            }
        }
    }
}

fn final_actions(a: u32, b: u32, deal: u32, z: u32) -> TempNode {
    let prefix = ((a * 2 + b) * 8 + deal) * 2 + z;
    TempNode::Action {
        player: Player::P0,
        children: (0..2)
            .map(|c| TempNode::Action {
                player: Player::P1,
                children: (0..2)
                    .map(|d| TempNode::Terminal {
                        id: (((a * 2 + b) * 8 + deal) * 2 + z) * 4 + c * 2 + d,
                        tag: 0,
                    })
                    .collect(),
                tag: 100 + prefix * 3 + 1 + c,
            })
            .collect(),
        tag: 100 + prefix * 3,
    }
}

pub fn fixture() -> CompiledGame<Payoffs> {
    let mut transitions = Vec::new();
    let mut root_children = Vec::new();
    for a in 0..2 {
        let mut before_deal = Vec::new();
        for b in 0..2 {
            let mut deals = Vec::new();
            for (deal, &(d0, d1)) in DIMS.iter().enumerate() {
                let mut maps = PerPlayer::new(ReachMap::Identity, ReachMap::Identity);
                for (p, width, dim) in [(Player::P0, 1, d0), (Player::P1, WIDTH, d1)] {
                    let entries = if dim == 0 {
                        Vec::new()
                    } else if p == Player::P0 {
                        vec![(0, (dim - 1) as u32, 1.0)]
                    } else {
                        vec![(0, 0, 1.0), ((WIDTH - 1) as u32, (dim - 1) as u32, 1.0)]
                    };
                    maps[p] = ReachMap::Transition(transitions.len() as u32);
                    transitions.push(SparseTransition {
                        in_dim: width as u32,
                        out_dim: dim as u32,
                        entries,
                    });
                }
                deals.push((
                    0.125,
                    maps,
                    TempNode::Chance {
                        deals: (0..2)
                            .map(|z| {
                                (
                                    0.5,
                                    PerPlayer::new(ReachMap::Identity, ReachMap::Identity),
                                    final_actions(a, b, deal as u32, z),
                                )
                            })
                            .collect(),
                        tag: 0,
                    },
                ));
            }
            before_deal.push(TempNode::Chance { deals, tag: 0 });
        }
        root_children.push(TempNode::Action {
            player: Player::P1,
            children: before_deal,
            tag: 1 + a,
        });
    }
    let tree = PublicTree::compile(TreeSpec {
        root: TempNode::Action {
            player: Player::P0,
            children: root_children,
            tag: 0,
        },
        masks: Vec::new(),
        transitions,
        root_dims: PerPlayer::new(1, WIDTH as u32),
    });
    // At2/4workers the grain clamps to4096. Each prefix P1 node alone has4096;
    // its descendants add a little more. Both root subtrees must qualify.
    assert!(tree.subtree_has_chance[0]);
    assert_eq!(tree.storage_len, 8_706);
    assert_eq!(tree.storage_refs.len(), 195);
    assert_eq!(tree.nodes.len(), 487);
    for workers in [2_usize, 4, 32] {
        let grain = tree.storage_len.div_ceil(4 * workers).clamp(4096, 65_536);
        assert_eq!(
            tree.children(0)
                .filter(|&id| {
                    let span = tree.storage_spans[id as usize];
                    span.end - span.start >= grain
                })
                .count(),
            2
        );
    }
    let mut p1 = vec![0.0; WIDTH];
    p1[0] = 1.0;
    p1[WIDTH - 1] = 1.0;
    CompiledGame {
        tree,
        evaluator: Payoffs { rendezvous: None },
        root_ranges: PerPlayer::new(vec![1.0], p1),
        normalizer: 2.0,
        zero_sum: false,
    }
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|x| x.to_bits()).collect()
}

fn state_bits(state: StorageState) -> Vec<Vec<u32>> {
    match state {
        StorageState::F32 {
            regrets,
            strategy_sum,
        } => vec![bits(&regrets), bits(&strategy_sum)],
        StorageState::I16 {
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        } => vec![
            regrets.iter().map(|&x| x as u16 as u32).collect(),
            strategy_sum.iter().map(|&x| x as u16 as u32).collect(),
            bits(&regret_scales),
            bits(&strategy_scales),
        ],
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    iteration: u64,
    state: Vec<Vec<u32>>,
    ev: Vec<u64>,
    br: Vec<u64>,
    gains: Vec<u64>,
    cfv: Vec<Vec<Option<Vec<u32>>>>,
}

fn observe<S: Storage>(workers: usize, depth: u32, steps: bool) -> Observation {
    let mut solver = Solver::<_, S>::new(fixture(), Box::<Dcfr>::default(), Some(2));
    solver.set_par(ParConfig {
        chance_depth: depth,
        min_children: 2,
    });
    rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .unwrap()
        .install(|| {
            if steps {
                solver.step();
                solver.step();
            } else {
                solver.run(2);
            }
            let before = state_bits(solver.state().storage);
            let mut observation = Observation {
                iteration: solver.iteration(),
                state: before.clone(),
                ev: Vec::new(),
                br: Vec::new(),
                gains: Vec::new(),
                cfv: Vec::new(),
            };
            for p in Player::BOTH {
                observation.ev.push(solver.expected_value(p).to_bits());
                observation.br.push(solver.best_response_value(p).to_bits());
                let all = solver.expected_values_everywhere(p);
                let selected = solver.expected_values_where(p, |id| {
                    solver
                        .game()
                        .tree
                        .storage_ref(solver.game().tree.node(id))
                        .index
                        % 3
                        == 0
                });
                let none = solver.expected_values_where(p, |_| false);
                assert!(none.iter().all(Option::is_none));
                for (index, (a, b)) in all.iter().zip(&selected).enumerate() {
                    if index % 3 == 0 {
                        assert_eq!(a.as_deref().map(bits), b.as_deref().map(bits));
                    } else {
                        assert!(b.is_none());
                    }
                }
                let ranges = PerPlayer::new(
                    solver.game().root_ranges[Player::P0].as_slice(),
                    solver.game().root_ranges[Player::P1].as_slice(),
                );
                let root = solver.expected_values_at(0, p, ranges);
                assert_eq!(bits(&root), bits(all[0].as_ref().unwrap()));
                let root_br = solver.best_response_values_at(0, p, ranges);
                let aggregate: f64 = root_br
                    .iter()
                    .zip(&solver.game().root_ranges[p])
                    .map(|(&v, &r)| v as f64 * r as f64)
                    .sum();
                assert_eq!(
                    (aggregate / 2.0).to_bits(),
                    solver.best_response_value(p).to_bits()
                );
                observation
                    .cfv
                    .push(all.iter().map(|v| v.as_deref().map(bits)).collect());
            }
            observation.gains = solver
                .exploitability()
                .0
                .iter()
                .map(|x| x.to_bits())
                .collect();
            assert_eq!(
                before,
                state_bits(solver.state().storage),
                "read-only evaluation changed storage"
            );
            observation
        })
}

fn compare<S: Storage>() {
    let baseline = observe::<S>(1, 0, false);
    for workers in [1, 2, 4] {
        for depth in [0, 1, 2] {
            for steps in [false, true] {
                assert_eq!(
                    observe::<S>(workers, depth, steps),
                    baseline,
                    "workers={workers}, depth={depth}, steps={steps}"
                );
            }
        }
    }
}

#[test]
fn root_action_paths_f32_state_values_cfv_and_steps() {
    compare::<F32Storage>();
}

#[test]
fn root_action_paths_i16_state_values_cfv_and_steps() {
    compare::<I16Storage>();
}

#[test]
fn root_action_path_is_actually_parallel_with_chance_disabled() {
    let gate = Arc::new(Rendezvous::default());
    let mut game = fixture();
    game.evaluator.rendezvous = Some(gate.clone());
    let mut solver = Solver::<_, F32Storage>::new(game, Box::<Dcfr>::default(), Some(1));
    solver.set_par(ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    });
    rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap()
        .install(|| solver.run(1));
    assert!(gate.threads.lock().unwrap().iter().all(Option::is_some));
}

/// Export boundary only. The separate scalar oracle never reads engine tree,
/// terminal payoff code or transition tables to implement its own rules.
pub fn export<S: Storage>(
    solver: &Solver<Payoffs, S>,
) -> std::collections::HashMap<String, Vec<f64>> {
    let mut profile = std::collections::HashMap::new();
    let tree = &solver.game().tree;
    for id in 0..tree.nodes.len() as u32 {
        let node = tree.node(id);
        if node.kind != NodeKind::Action {
            continue;
        }
        let tag = tree.tags[id as usize];
        let history = if tag == 0 {
            "root".to_owned()
        } else if tag < 100 {
            format!("pre:{}", tag - 1)
        } else {
            let prefix = (tag - 100) / 3;
            let kind = (tag - 100) % 3;
            let z = prefix % 2;
            let deal = (prefix / 2) % 8;
            let b = (prefix / 16) % 2;
            let a = prefix / 32;
            if kind == 0 {
                format!("play0:{a},{b},{deal},{z}")
            } else {
                format!("play1:{a},{b},{deal},{z},{}", kind - 1)
            }
        };
        let sref = tree.storage_ref(node);
        let sigma = solver.average_strategy_at(id);
        let hands = sref.num_hands as usize;
        for h in 0..hands {
            profile.insert(
                format!("{h}|{history}"),
                (0..sref.num_actions as usize)
                    .map(|a| sigma[a * hands + h] as f64)
                    .collect(),
            );
        }
    }
    profile
}
