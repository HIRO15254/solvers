//! Dimension-changing `ReachMap::Transition` chance deals: `PublicTree` has
//! supported `SparseTransition { in_dim, out_dim }` with `out_dim != in_dim`
//! since M1 (see `crates/engine/src/tree.rs`'s `ReachMap` doc comment), but
//! every solver walk used to size its chance-branch scratch buffers as if
//! `out_dim == in_dim` always held. These tests build the same tiny game two
//! ways — once with duplicated private states left unmerged (a
//! dimension-preserving `Identity` deal), once with those duplicates merged
//! through a genuine dimension-changing `Transition` — and check the solved
//! results agree, the way suit-isomorphism class-folding is meant to (see
//! `Deal::weight`'s doc comment: "suit-isomorphism classes fold their class
//! size in here").

use cards::{PerPlayer, Player};
use engine::{
    CompiledGame, Dcfr, F32Storage, McCfg, McSolver, PublicTree, ReachMap, Solver,
    SparseTransition, TempNode, TerminalEvaluator, TreeSpec,
};

/// Payoff to P0 (row = hero's class, col = opponent's class) at each of the
/// 4 terminals, indexed `[terminal][class(hero)][class(opp)]`. Values are
/// arbitrary but distinct so the walk actually exercises per-hand/per-class
/// vector math instead of a constant payoff; P1's payoff is the zero-sum
/// negation (see `ClassEvaluator::eval`).
const BASE: [[[f32; 2]; 2]; 4] = [
    [[2.0, -1.0], [-3.0, 4.0]],
    [[-1.0, 3.0], [2.0, -2.0]],
    [[0.0, 2.0], [1.0, -1.0]],
    [[3.0, -2.0], [-1.0, 1.0]],
];

/// Game A's private-state -> class map: states {0,1} are class 0, {2,3} are
/// class 1 — the "duplicated, unmerged" hand space.
fn class4(h: usize) -> usize {
    h / 2
}

/// Game B's private-state -> class map: already merged, so it's the
/// identity (2 states, one per class).
fn class_identity(h: usize) -> usize {
    h
}

/// Terminal evaluator whose payoff depends only on `(class(hero), class(opp))`,
/// never on the specific hand within a class. That's exactly the property
/// that makes merging same-class states through a `Transition` (Game B)
/// equivalent to carrying them unmerged through `Identity` (Game A) with
/// duplicated root-range mass — a hand-isomorphism argument, not a
/// coincidence of these particular numbers.
struct ClassEvaluator {
    dim: usize,
    class: fn(usize) -> usize,
}

impl TerminalEvaluator for ClassEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        let t = terminal as usize;
        match p {
            Player::P0 => {
                for (h, out_h) in out.iter_mut().enumerate().take(self.dim) {
                    let ch = (self.class)(h);
                    let mut acc = 0.0f32;
                    for (o, &r) in opp_reach.iter().enumerate() {
                        let co = (self.class)(o);
                        acc += BASE[t][ch][co] * r;
                    }
                    *out_h = acc;
                }
            }
            Player::P1 => {
                for (o, out_o) in out.iter_mut().enumerate().take(self.dim) {
                    let co = (self.class)(o);
                    let mut acc = 0.0f32;
                    for (h, &r) in opp_reach.iter().enumerate() {
                        let ch = (self.class)(h);
                        acc += -BASE[t][ch][co] * r;
                    }
                    *out_o = acc;
                }
            }
        }
    }
}

/// P0 (2 actions) then P1 (2 actions) then a terminal: 4 terminals, ids
/// `a * 2 + b` in visitation order. Structural only — the per-node hand
/// count comes from context (`PublicTree::fill`'s `dims`), so this same
/// shape is reused unmodified for every dimension combination below.
fn build_action_tree(next_terminal: &mut u32) -> TempNode {
    let p0_children = (0..2)
        .map(|_a| {
            let p1_children = (0..2)
                .map(|_b| {
                    let id = *next_terminal;
                    *next_terminal += 1;
                    TempNode::Terminal { id, tag: 0 }
                })
                .collect();
            TempNode::Action {
                player: Player::P1,
                children: p1_children,
                tag: 0,
            }
        })
        .collect();
    TempNode::Action {
        player: Player::P0,
        children: p0_children,
        tag: 0,
    }
}

/// The dimension-changing transition merging {0,1}->0 and {2,3}->1 with
/// unit weights, shared by every test below.
fn merge_transition() -> SparseTransition {
    SparseTransition {
        in_dim: 4,
        out_dim: 2,
        entries: vec![(0, 0, 1.0), (1, 0, 1.0), (2, 1, 1.0), (3, 1, 1.0)],
    }
}

/// Builds Game A (`use_transition: false`, `dim: 4`, `class: class4`) or
/// Game B (`use_transition: true`, `dim: 2`, `class: class_identity`) — same
/// tree shape, same root dimension (4, since `SparseTransition::in_dim`
/// requires it), same root ranges; only the chance deal's maps and the
/// action subtree's resulting per-player dimension differ.
fn build_game(
    dim: usize,
    class: fn(usize) -> usize,
    use_transition: bool,
    p0_range: Vec<f32>,
    p1_range: Vec<f32>,
) -> CompiledGame<ClassEvaluator> {
    let mut next_terminal = 0u32;
    let action_root = build_action_tree(&mut next_terminal);

    let (maps, transitions) = if use_transition {
        (
            PerPlayer::new(ReachMap::Transition(0), ReachMap::Transition(0)),
            vec![merge_transition()],
        )
    } else {
        (
            PerPlayer::new(ReachMap::Identity, ReachMap::Identity),
            Vec::new(),
        )
    };

    let root = TempNode::Chance {
        deals: vec![(1.0, maps, action_root)],
        tag: 0,
    };
    let tree = PublicTree::compile(TreeSpec {
        root,
        masks: Vec::new(),
        transitions,
        root_dims: PerPlayer::new(4, 4),
    });

    let normalizer = p0_range.iter().sum::<f32>() as f64 * p1_range.iter().sum::<f32>() as f64;
    CompiledGame {
        tree,
        evaluator: ClassEvaluator { dim, class },
        root_ranges: PerPlayer::new(p0_range, p1_range),
        normalizer,
        zero_sum: true,
    }
}

/// By DFS-preorder construction (see `PublicTree::compile`/`fill`): 0 = root
/// (chance, one deal), 1 = P0's action node, 2/3 = P1's action nodes (under
/// P0's action 0/1). Identical in Game A and Game B — only per-node hand
/// counts differ.
const P0_NODE: engine::NodeId = 1;
const P1_NODE_A0: engine::NodeId = 2;
const P1_NODE_A1: engine::NodeId = 3;

fn root_ranges() -> (Vec<f32>, Vec<f32>) {
    // Classes: {0,1} carry mass w0/v0, {2,3} carry mass w1/v1. Distinct
    // per-player masses so the test can't accidentally pass via symmetry.
    (vec![1.3, 1.3, 0.6, 0.6], vec![0.9, 0.9, 1.7, 1.7])
}

/// Asserts `strat_a[action][h] == strat_a[action][h']` for every same-class
/// pair, and that both equal `strat_b[action][class]` (`num_actions` and
/// tolerance shared by every call site).
fn assert_strategies_agree(strat_a: &[f32], strat_b: &[f32], num_actions: usize, tol: f32) {
    for a in 0..num_actions {
        let row_a = &strat_a[a * 4..(a + 1) * 4];
        let row_b = &strat_b[a * 2..(a + 1) * 2];
        assert!(
            (row_a[0] - row_a[1]).abs() < tol,
            "Game A's duplicated class-0 states must have equal strategy: {row_a:?}"
        );
        assert!(
            (row_a[2] - row_a[3]).abs() < tol,
            "Game A's duplicated class-1 states must have equal strategy: {row_a:?}"
        );
        assert!(
            (row_a[0] - row_b[0]).abs() < tol,
            "class 0 strategy mismatch: A={} B={}",
            row_a[0],
            row_b[0]
        );
        assert!(
            (row_a[2] - row_b[1]).abs() < tol,
            "class 1 strategy mismatch: A={} B={}",
            row_a[2],
            row_b[1]
        );
    }
}

#[test]
fn merge_split_differential_solver() {
    let (p0_range, p1_range) = root_ranges();
    let game_a = build_game(4, class4, false, p0_range.clone(), p1_range.clone());
    let game_b = build_game(2, class_identity, true, p0_range, p1_range);

    let mut solver_a = Solver::<_, F32Storage>::new(game_a, Box::<Dcfr>::default(), None);
    let mut solver_b = Solver::<_, F32Storage>::new(game_b, Box::<Dcfr>::default(), None);
    solver_a.run(300);
    solver_b.run(300);

    for p in Player::BOTH {
        let ev_a = solver_a.expected_value(p);
        let ev_b = solver_b.expected_value(p);
        assert!(
            (ev_a - ev_b).abs() < 1e-6,
            "expected_value diverged for {p:?}: A={ev_a} B={ev_b}"
        );

        let br_a = solver_a.best_response_value(p);
        let br_b = solver_b.best_response_value(p);
        assert!(
            (br_a - br_b).abs() < 1e-6,
            "best_response_value diverged for {p:?}: A={br_a} B={br_b}"
        );
    }

    let expl_a = solver_a.exploitability();
    let expl_b = solver_b.exploitability();
    for p in Player::BOTH {
        assert!(
            (expl_a[p] - expl_b[p]).abs() < 1e-6,
            "exploitability diverged for {p:?}: A={} B={}",
            expl_a[p],
            expl_b[p]
        );
    }

    // P0's action node: 2 actions.
    assert_strategies_agree(
        &solver_a.average_strategy_at(P0_NODE),
        &solver_b.average_strategy_at(P0_NODE),
        2,
        1e-6,
    );
    // P1's action nodes (one per P0 action): 2 actions each.
    assert_strategies_agree(
        &solver_a.average_strategy_at(P1_NODE_A0),
        &solver_b.average_strategy_at(P1_NODE_A0),
        2,
        1e-6,
    );
    assert_strategies_agree(
        &solver_a.average_strategy_at(P1_NODE_A1),
        &solver_b.average_strategy_at(P1_NODE_A1),
        2,
        1e-6,
    );
}

#[test]
fn merge_split_differential_mcsolver() {
    let (p0_range, p1_range) = root_ranges();
    let game_a = build_game(4, class4, false, p0_range.clone(), p1_range.clone());
    let game_b = build_game(2, class_identity, true, p0_range, p1_range);

    // Chance sampling is trivial here (one deal, weight 1.0, always picked),
    // so this exercises `mccfr_pass`'s dimension-changing chance branch
    // without adding real sampling variance on top of the comparison.
    let cfg = McCfg {
        seed: 42,
        ..McCfg::default()
    };
    let mut mc_a = McSolver::<_, F32Storage>::new(game_a, cfg);
    let mut mc_b = McSolver::<_, F32Storage>::new(game_b, cfg);
    mc_a.run(20_000);
    mc_b.run(20_000);

    // Looser tolerance than the full-traversal `Solver` test: MCCFR's own
    // per-iteration regret updates are still exact vector-form at the
    // action nodes (see `mccfr.rs`'s module doc), but the two solvers'
    // `Scratch` pools take/put buffers in different sizes (4-dim vs 2-dim),
    // which can shift discount-schedule floating-point rounding slightly
    // over 20k iterations.
    for p in Player::BOTH {
        let ev_a = mc_a.expected_value(p);
        let ev_b = mc_b.expected_value(p);
        assert!(
            (ev_a - ev_b).abs() < 1e-3,
            "expected_value diverged for {p:?}: A={ev_a} B={ev_b}"
        );
    }

    let expl_a = mc_a.exploitability();
    let expl_b = mc_b.exploitability();
    for p in Player::BOTH {
        assert!(
            (expl_a[p] - expl_b[p]).abs() < 1e-3,
            "exploitability diverged for {p:?}: A={} B={}",
            expl_a[p],
            expl_b[p]
        );
    }

    assert_strategies_agree(
        &mc_a.average_strategy_at(P0_NODE),
        &mc_b.average_strategy_at(P0_NODE),
        2,
        1e-2,
    );
}

/// Terminal evaluator for the asymmetric-dimension game: P0's hand space has
/// 2 entries (post-merge), P1's has 3 (untouched by any transition).
/// Arbitrary, deliberately not zero-sum (general-sum accounting path).
struct AsymEvaluator;

impl TerminalEvaluator for AsymEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        let t = terminal as f32;
        match p {
            Player::P0 => {
                debug_assert_eq!(out.len(), 2);
                debug_assert_eq!(opp_reach.len(), 3);
                for (h, out_h) in out.iter_mut().enumerate() {
                    let mut acc = 0.0f32;
                    for (o, &r) in opp_reach.iter().enumerate() {
                        let v = ((t * 17.0 + h as f32 * 5.0 + o as f32 * 3.0) % 11.0) - 5.0;
                        acc += v * r;
                    }
                    *out_h = acc;
                }
            }
            Player::P1 => {
                debug_assert_eq!(out.len(), 3);
                debug_assert_eq!(opp_reach.len(), 2);
                for (o, out_o) in out.iter_mut().enumerate() {
                    let mut acc = 0.0f32;
                    for (h, &r) in opp_reach.iter().enumerate() {
                        let v = ((t * 13.0 + o as f32 * 7.0 + h as f32 * 2.0) % 9.0) - 4.0;
                        acc += v * r;
                    }
                    *out_o = acc;
                }
            }
        }
    }
}

/// P0: root dim 4, merged 4->2 by a `Transition`. P1: root dim 3, untouched
/// by `Identity`. Exercises `my_next`/`opp_next` having different sizes at
/// the same chance branch — the old code (sizing both from `my_reach.len()`/
/// `opp_reach.len()`) would trip `SparseTransition::apply_forward`'s
/// `debug_assert_eq!` here in debug builds.
fn build_asymmetric_game() -> CompiledGame<AsymEvaluator> {
    let mut next_terminal = 0u32;
    let action_root = build_action_tree(&mut next_terminal);

    let maps = PerPlayer::new(ReachMap::Transition(0), ReachMap::Identity);
    let root = TempNode::Chance {
        deals: vec![(1.0, maps, action_root)],
        tag: 0,
    };
    let tree = PublicTree::compile(TreeSpec {
        root,
        masks: Vec::new(),
        transitions: vec![merge_transition()],
        root_dims: PerPlayer::new(4, 3),
    });

    let p0_range = vec![1.0f32; 4];
    let p1_range = vec![1.0f32; 3];
    let normalizer = p0_range.iter().sum::<f32>() as f64 * p1_range.iter().sum::<f32>() as f64;
    CompiledGame {
        tree,
        evaluator: AsymEvaluator,
        root_ranges: PerPlayer::new(p0_range, p1_range),
        normalizer,
        zero_sum: false,
    }
}

#[test]
fn asymmetric_dims_run_without_panicking_and_converge() {
    let game = build_asymmetric_game();
    let mut solver = Solver::<_, F32Storage>::new(game, Box::<Dcfr>::default(), None);

    solver.run(20);
    let early = solver.exploitability();
    let early_total = early[Player::P0] + early[Player::P1];
    assert!(early_total.is_finite());

    solver.run(500);
    let late = solver.exploitability();
    let late_total = late[Player::P0] + late[Player::P1];
    assert!(late_total.is_finite());

    assert!(
        late_total < early_total,
        "exploitability should decrease with more iterations: early={early_total}, late={late_total}"
    );
}

#[test]
fn asymmetric_dims_mcsolver_runs_without_panicking() {
    let game = build_asymmetric_game();
    let cfg = McCfg {
        seed: 7,
        ..McCfg::default()
    };
    let mut solver = McSolver::<_, F32Storage>::new(game, cfg);
    solver.run(2_000);
    let expl = solver.exploitability();
    assert!(expl[Player::P0].is_finite());
    assert!(expl[Player::P1].is_finite());
}
