//! Analytical values for untrained strategies through empty/variable hand spaces.
//! This exercises both EV combiners, including CFV recording, without training
//! one solver and treating another invocation of its evaluator as the oracle.

use cards::{PerPlayer, Player};
use engine::{
    CompiledGame, Dcfr, F32Storage, I16Storage, NodeKind, ParConfig, PublicTree, ReachMap, Solver,
    SparseTransition, Storage, StorageState, TempNode, TerminalEvaluator, TreeSpec,
};

const DIMS: [(usize, usize); 8] = [
    (0, 2),
    (1, 3),
    (2, 0),
    (3, 1),
    (1, 3),
    (0, 0),
    (3, 1),
    (0, 0),
];

struct PairPayoffs;

impl TerminalEvaluator for PairPayoffs {
    fn eval(&self, terminal: u32, player: Player, opp_reach: &[f32], out: &mut [f32]) {
        let a = (terminal / 2) as f32;
        let b = (terminal % 2) as f32;
        let payoff = match player {
            Player::P0 => 4.0 + 4.0 * a + 2.0 * b,
            Player::P1 => 8.0 + 2.0 * a + 8.0 * b,
        };
        out.fill(payoff * opp_reach.iter().sum::<f32>());
    }
}

fn game() -> CompiledGame<PairPayoffs> {
    let mut transitions = Vec::new();
    let deals = DIMS
        .iter()
        .enumerate()
        .map(|(deal, &(p0, p1))| {
            let mut maps = PerPlayer::new(ReachMap::Identity, ReachMap::Identity);
            for (player, dim) in [(Player::P0, p0), (Player::P1, p1)] {
                maps[player] = ReachMap::Transition(transitions.len() as u32);
                transitions.push(SparseTransition {
                    in_dim: 3,
                    out_dim: dim as u32,
                    entries: (0..dim).map(|h| (h as u32, h as u32, 1.0)).collect(),
                });
            }
            // Tags encode (deal, action node): 0=P0; 1/2=P1 after a=0/1.
            let child = TempNode::Action {
                player: Player::P0,
                children: (0..2)
                    .map(|a| TempNode::Action {
                        player: Player::P1,
                        children: (0..2)
                            .map(|b| TempNode::Terminal {
                                id: 2 * a + b,
                                tag: 0,
                            })
                            .collect(),
                        tag: 3 * deal as u32 + 1 + a,
                    })
                    .collect(),
                tag: 3 * deal as u32,
            };
            (0.125, maps, child)
        })
        .collect();
    CompiledGame {
        tree: PublicTree::compile(TreeSpec {
            root: TempNode::Chance { deals, tag: 0 },
            masks: Vec::new(),
            transitions,
            root_dims: PerPlayer::new(3, 3),
        }),
        evaluator: PairPayoffs,
        root_ranges: PerPlayer::new(vec![1.0; 3], vec![1.0; 3]),
        // Nine equally weighted root pairs. Removed states contribute zero;
        // the normalizer is fixed at the root, not recomputed on each deal.
        normalizer: 9.0,
        zero_sum: false,
    }
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|x| x.to_bits()).collect()
}

fn storage_bits(state: StorageState) -> Vec<Vec<u32>> {
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

fn check<S: Storage>() {
    for workers in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        for chance_depth in [0, 1] {
            // Construct outside the pool, then evaluate within it. No CFR step:
            // every zero strategy-sum column must use the uniform fallback.
            let mut solver = Solver::<_, S>::new(game(), Box::<Dcfr>::default(), Some(2));
            solver.set_par(ParConfig {
                chance_depth,
                min_children: 2,
            });
            let before = storage_bits(solver.state().storage);
            pool.install(|| {
                let tree = &solver.game().tree;
                assert_eq!(tree.storage_refs.len(), 24);
                let reaches = PerPlayer::new(
                    solver.game().root_ranges[Player::P0].as_slice(),
                    solver.game().root_ranges[Player::P1].as_slice(),
                );
                // Uniform policies have pair payoffs (7,13); best responses
                // choose a=1 for P0 and b=1 for P1, giving (9,17).
                // Two (1,3) and two (3,1) deals each weigh 1/8. Their
                // transpose maps give the explicit root vectors below.
                let ev = PerPlayer::new(7.0_f64 / 6.0, 13.0_f64 / 6.0);
                let br = PerPlayer::new(9.0_f64 / 6.0, 17.0_f64 / 6.0);
                for player in Player::BOTH {
                    let (value, best) = match player {
                        Player::P0 => (7.0, 9.0),
                        Player::P1 => (13.0, 17.0),
                    };
                    assert_eq!(
                        solver.expected_value(player).to_bits(),
                        ev[player].to_bits()
                    );
                    assert_eq!(
                        solver.best_response_value(player).to_bits(),
                        br[player].to_bits()
                    );
                    assert_eq!(
                        bits(&solver.expected_values_at(0, player, reaches)),
                        bits(&[value, value / 4.0, value / 4.0])
                    );
                    assert_eq!(
                        bits(&solver.best_response_values_at(0, player, reaches)),
                        bits(&[best, best / 4.0, best / 4.0])
                    );
                    let all = solver.expected_values_everywhere(player);
                    let selected = solver.expected_values_where(player, |id| {
                        tree.tags[id as usize].is_multiple_of(3)
                    });
                    let none = solver.expected_values_where(player, |_| false);
                    assert_eq!(all.len(), 24);
                    assert!(none.iter().all(Option::is_none));
                    let (mut kept, mut dropped, mut empty) = (0, 0, 0);
                    for (id, node) in tree.nodes.iter().enumerate() {
                        if node.kind != NodeKind::Action {
                            continue;
                        }
                        let tag = tree.tags[id] as usize;
                        let (d0, d1) = DIMS[tag / 3];
                        let stage = tag % 3;
                        let hands = if player == Player::P0 { d0 } else { d1 };
                        // At a P1 node the opponent P0 reach is halved by
                        // P0's earlier uniform action. P0's opponent reach
                        // stays unchanged. Chance weight belongs at the parent.
                        let expected = match (player, stage) {
                            (Player::P0, 0) => 7.0 * d1 as f32,
                            (Player::P1, 0) => 13.0 * d0 as f32,
                            (Player::P0, _) => (5.0 + 4.0 * (stage - 1) as f32) * d1 as f32,
                            (Player::P1, _) => (12.0 + 2.0 * (stage - 1) as f32) * d0 as f32 / 2.0,
                        };
                        let index = tree.storage_ref(node).index as usize;
                        let expected = bits(&vec![expected; hands]);
                        assert_eq!(bits(all[index].as_ref().unwrap()), expected);
                        if stage == 0 {
                            kept += 1;
                            assert_eq!(bits(selected[index].as_ref().unwrap()), expected);
                        } else {
                            dropped += 1;
                            assert!(selected[index].is_none());
                        }
                        if hands == 0 {
                            empty += 1;
                        }
                        assert_eq!(
                            bits(&solver.average_strategy_at(id as u32)),
                            bits(&vec![0.5; tree.storage_ref(node).len()])
                        );
                    }
                    assert_eq!((kept, dropped, empty), (8, 16, 9));
                }
                let gains = solver.exploitability();
                for player in Player::BOTH {
                    assert_eq!(gains[player].to_bits(), (br[player] - ev[player]).to_bits());
                }
                assert_eq!(solver.iteration(), 0);
                assert_eq!(storage_bits(solver.state().storage), before);
            });
        }
    }
}

#[test]
fn untrained_empty_and_variable_value_spaces_f32() {
    check::<F32Storage>();
}

#[test]
fn untrained_empty_and_variable_value_spaces_i16() {
    check::<I16Storage>();
}
