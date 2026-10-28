use hu_engine::{
    CfrPrecision, CompiledGame, F32Storage, NodeIncentive, NodeLock, Profile, PublicTree, Solver,
    TempNode, TerminalEvaluator, TreeSpec, Vanilla,
};
use nlh::{PerPlayer, Player};

struct Evaluator {
    modified: bool,
    supported: bool,
}
impl TerminalEvaluator for Evaluator {
    fn eval(&self, terminal: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        let value = terminal as f32
            + if self.modified && terminal == 0 && p == Player::P0 {
                2.0
            } else {
                0.0
            };
        out[0] = if p == Player::P0 {
            value * reach[0]
        } else {
            -(terminal as f32) * reach[0]
        };
    }
    fn supports_compat_mass(&self) -> bool {
        self.supported
    }
    fn compat_mass(&self, _: u32, _: Player, reach: &[f32], out: &mut [f32]) {
        out[0] = reach[0];
    }
}
fn solver(modified: bool) -> Solver<Evaluator, F32Storage> {
    solver_with_support(modified, true)
}
fn solver_with_support(modified: bool, supported: bool) -> Solver<Evaluator, F32Storage> {
    let root = TempNode::Action {
        player: Player::P0,
        tag: 0,
        children: (0..3).map(|id| TempNode::Terminal { id, tag: 0 }).collect(),
    };
    let tree = PublicTree::compile(TreeSpec {
        root,
        masks: vec![],
        transitions: vec![],
        root_dims: PerPlayer::new(1, 1),
    });
    Solver::new(
        CompiledGame {
            tree,
            evaluator: Evaluator {
                modified,
                supported,
            },
            root_ranges: PerPlayer::new(vec![1.0], vec![1.0]),
            normalizer: 1.0,
            zero_sum: !modified,
        },
        Box::new(Vanilla),
        None,
    )
}
fn profile(lock: Option<NodeLock>, incentive: Option<NodeIncentive>) -> Profile {
    Profile {
        locks: vec![lock],
        incentives: vec![incentive],
    }
}

#[test]
fn empty_profile_matches_unprofiled() {
    let mut plain = solver(false);
    let mut empty = solver(false);
    plain.set_cfr_precision(CfrPrecision::F32);
    empty.set_cfr_precision(CfrPrecision::F32);
    empty.set_profile(profile(None, None)).unwrap();
    plain.run(20);
    empty.run(20);
    assert_eq!(plain.state(), empty.state());
    assert_eq!(plain.evaluate(), empty.evaluate());
}
#[test]
fn partial_and_soft_lock_best_response() {
    let mut s = solver(false);
    s.set_profile(profile(
        Some(NodeLock {
            fixed: vec![0.25, f32::NAN, f32::NAN],
            confidence: vec![1.0],
        }),
        None,
    ))
    .unwrap();
    s.run(1000);
    let strategy = s.average_strategy_at(0);
    assert_eq!(strategy[0], 0.25);
    assert!((strategy.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    assert!((s.best_response_value(Player::P0) - 1.5).abs() < 1e-6);
    assert!(s.exploitability()[Player::P0] < 0.01);
    let mut soft = solver(false);
    soft.set_profile(profile(
        Some(NodeLock {
            fixed: vec![0.5, 0.5, 0.0],
            confidence: vec![0.4],
        }),
        None,
    ))
    .unwrap();
    soft.run(1000);
    let strategy = soft.average_strategy_at(0);
    assert!((strategy[0] - 0.2).abs() < 0.001);
    assert!((strategy[1] - 0.2).abs() < 0.001);
    assert!((soft.best_response_value(Player::P0) - 1.4).abs() < 1e-6);
}
#[test]
fn incentive_matches_modified_game_and_true_ev() {
    let mut a = solver(false);
    let mut b = solver(true);
    a.set_profile(profile(
        None,
        Some(NodeIncentive {
            bonus: vec![2.0, 0.0, 0.0],
            space: 0,
        }),
    ))
    .unwrap();
    a.run(500);
    b.run(500);
    for (x, y) in a
        .average_strategy_at(0)
        .iter()
        .zip(b.average_strategy_at(0))
    {
        assert!((x - y).abs() < 1e-6);
    }
    let (ae, ag) = a.evaluate();
    let (be, bg) = b.evaluate();
    assert!((ae[Player::P0] - be[Player::P0]).abs() < 1e-6);
    assert!((ag[Player::P0] - bg[Player::P0]).abs() < 1e-6);
    assert!(
        (a.true_expected_values()[Player::P0]
            - (be[Player::P0] - 2.0 * a.average_strategy_at(0)[0] as f64))
            .abs()
            < 1e-6
    );
}

#[test]
fn changing_incentive_values_updates_evaluation() {
    let mut s = solver(false);
    s.set_profile(profile(
        None,
        Some(NodeIncentive {
            bonus: vec![0.0; 3],
            space: 0,
        }),
    ))
    .unwrap();
    let zero = s.exploitability();
    s.profile_mut().unwrap().incentives[0]
        .as_mut()
        .unwrap()
        .bonus[0] = 2.0;
    let active = s.exploitability();
    let mut reference = solver(true);
    reference.run(0);
    assert_eq!(active, reference.exploitability());
    s.profile_mut().unwrap().incentives[0]
        .as_mut()
        .unwrap()
        .bonus[0] = 0.0;
    assert_eq!(s.exploitability(), zero);
}

#[test]
fn incentive_propagates_to_upstream_choices() {
    fn game(modified: bool) -> Solver<Evaluator, F32Storage> {
        let root = TempNode::Action {
            player: Player::P0,
            tag: 0,
            children: vec![
                TempNode::Terminal { id: 1, tag: 0 },
                TempNode::Action {
                    player: Player::P0,
                    tag: 0,
                    children: vec![
                        TempNode::Terminal { id: 0, tag: 0 },
                        TempNode::Terminal { id: 2, tag: 0 },
                    ],
                },
            ],
        };
        let tree = PublicTree::compile(TreeSpec {
            root,
            masks: vec![],
            transitions: vec![],
            root_dims: PerPlayer::new(1, 1),
        });
        Solver::new(
            CompiledGame {
                tree,
                evaluator: Evaluator {
                    modified,
                    supported: true,
                },
                root_ranges: PerPlayer::new(vec![1.0], vec![1.0]),
                normalizer: 1.0,
                zero_sum: !modified,
            },
            Box::new(Vanilla),
            None,
        )
    }
    let mut profiled = game(false);
    let mut modified = game(true);
    profiled
        .set_profile(Profile {
            locks: vec![None, None],
            incentives: vec![
                None,
                Some(NodeIncentive {
                    bonus: vec![2.0, 0.0],
                    space: 0,
                }),
            ],
        })
        .unwrap();
    profiled.run(500);
    modified.run(500);
    for (a, b) in profiled
        .average_strategy_at(0)
        .iter()
        .zip(modified.average_strategy_at(0))
    {
        assert!((a - b).abs() < 1e-6);
    }
    assert!((profiled.evaluate().1[Player::P0] - modified.evaluate().1[Player::P0]).abs() < 1e-6);
}
#[test]
fn invalid_profiles_rejected() {
    let mut s = solver(false);
    for lock in [
        NodeLock {
            fixed: vec![0.8, 0.8, f32::NAN],
            confidence: vec![1.0],
        },
        NodeLock {
            fixed: vec![0.5, 0.5, f32::NAN],
            confidence: vec![0.5],
        },
        NodeLock {
            fixed: vec![1.0],
            confidence: vec![1.0],
        },
    ] {
        assert!(s.set_profile(profile(Some(lock), None)).is_err());
    }
    let mut unsupported = solver_with_support(false, false);
    assert!(
        unsupported
            .set_profile(profile(
                None,
                Some(NodeIncentive {
                    bonus: vec![0.0; 3],
                    space: 0,
                })
            ))
            .is_err()
    );
}

#[test]
fn f32_profile_uses_restricted_strategy() {
    let mut s = solver(false);
    s.set_cfr_precision(CfrPrecision::F32);
    s.set_profile(profile(
        Some(NodeLock {
            fixed: vec![0.25, f32::NAN, f32::NAN],
            confidence: vec![1.0],
        }),
        None,
    ))
    .unwrap();
    s.run(500);
    assert_eq!(s.average_strategy_at(0)[0], 0.25);
    assert!(s.exploitability()[Player::P0] < 0.01);
}

#[test]
fn full_lock_preserves_fixed_strategy_and_restricts_response() {
    let mut s = solver(false);
    s.set_profile(profile(
        Some(NodeLock {
            fixed: vec![0.2, 0.3, 0.5],
            confidence: vec![1.0],
        }),
        None,
    ))
    .unwrap();
    s.run(100);
    assert_eq!(s.average_strategy_at(0), vec![0.2, 0.3, 0.5]);
    assert!((s.best_response_value(Player::P0) - 1.3).abs() < 1e-6);
    assert!(s.exploitability()[Player::P0].abs() < 1e-6);
    assert!((s.expected_value(Player::P1) - s.best_response_value(Player::P1)).abs() < 1e-6);
}

#[test]
fn locking_converged_strategy_preserves_value() {
    let mut s = solver(false);
    s.run(1000);
    let strategy = s.average_strategy_at(0);
    let value = s.expected_value(Player::P0);
    s.set_profile(profile(
        Some(NodeLock {
            fixed: strategy.clone(),
            confidence: vec![1.0],
        }),
        None,
    ))
    .unwrap();
    s.run(100);
    assert_eq!(s.average_strategy_at(0), strategy);
    assert!((s.expected_value(Player::P0) - value).abs() < 1e-6);
}
