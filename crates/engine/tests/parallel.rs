//! Chance-node fan-out determinism: a synthetic engine-only game (no
//! dependency on `game`/`holdem`) with a wide root chance node, solved twice
//! under different [`ParConfig`]s — one that triggers rayon fan-out at the
//! root, one that forces the fully sequential walk — and checked bit-for-bit
//! equal.

use cards::{PerPlayer, Player};
use engine::{
    CompiledGame, Dcfr, F32Storage, ParConfig, PublicTree, ReachMap, Solver, TempNode,
    TerminalEvaluator, TreeSpec,
};

/// Per-player private-state dimension: a small synthetic "hand space".
const DIM: usize = 8;
/// Root chance-node fan-out; comfortably over the default `min_children`
/// (12) so the default `ParConfig` actually parallelizes here.
const NUM_DEALS: usize = 16;

/// Fixed per-terminal `H*H` payoff matrices (action-major-free: just a
/// lookup table), the trivial `TerminalEvaluator` the spec calls for.
struct FixedEvaluator {
    /// `terminals[id][player]` is a flat `DIM*DIM` matrix, row-major by hero
    /// hand: entry `[h * DIM + o]`.
    terminals: Vec<PerPlayer<Vec<f32>>>,
}

impl TerminalEvaluator for FixedEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        let matrix = &self.terminals[terminal as usize][p];
        debug_assert_eq!(opp_reach.len(), DIM);
        debug_assert_eq!(out.len(), DIM);
        for h in 0..DIM {
            let row = &matrix[h * DIM..(h + 1) * DIM];
            out[h] = row.iter().zip(opp_reach).map(|(&u, &r)| u * r).sum();
        }
    }
}

/// Deterministic pseudo-random-looking payoff, zero on the diagonal (a stand
/// in for card removal): varies with terminal id and both hands so the walk
/// actually exercises per-hand vector math instead of a constant payoff.
fn payoff_value(terminal: u32, h: usize, o: usize, sign: f32) -> f32 {
    if h == o {
        return 0.0;
    }
    let raw = ((terminal as u64 * 131 + h as u64 * 17 + o as u64 * 7) % 13) as f32 - 6.0;
    raw * sign
}

fn make_matrix(terminal: u32, sign: f32) -> Vec<f32> {
    let mut m = vec![0.0f32; DIM * DIM];
    for h in 0..DIM {
        for o in 0..DIM {
            m[h * DIM + o] = payoff_value(terminal, h, o, sign);
        }
    }
    m
}

fn make_terminal(next_terminal: &mut u32, terminals: &mut Vec<PerPlayer<Vec<f32>>>) -> TempNode {
    let id = *next_terminal;
    *next_terminal += 1;
    terminals.push(PerPlayer::new(make_matrix(id, 1.0), make_matrix(id, -1.0)));
    TempNode::Terminal { id, tag: 0 }
}

/// One deal's action subtree: 2 players alternating, 2-3 actions each,
/// ending in terminals. P0 picks one of 3 actions, then P1 picks one of 2.
fn action_subtree(next_terminal: &mut u32, terminals: &mut Vec<PerPlayer<Vec<f32>>>) -> TempNode {
    let p0_children = (0..3)
        .map(|_| {
            let p1_children = (0..2)
                .map(|_| make_terminal(next_terminal, terminals))
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

/// Builds the synthetic game: a root chance node with `NUM_DEALS` deals
/// (each masking out one hand index, a stand-in for a dealt card), each
/// leading into its own small action subtree.
fn build_game() -> CompiledGame<FixedEvaluator> {
    let mut terminals = Vec::new();
    let mut next_terminal = 0u32;
    let mut masks = Vec::new();
    let deals = (0..NUM_DEALS)
        .map(|i| {
            let mut mask = vec![1.0f32; DIM];
            mask[i % DIM] = 0.0;
            masks.push(mask);
            let mask_id = (masks.len() - 1) as u32;
            let maps = PerPlayer::new(ReachMap::Mask(mask_id), ReachMap::Mask(mask_id));
            let weight = 1.0 / NUM_DEALS as f32;
            let child = action_subtree(&mut next_terminal, &mut terminals);
            (weight, maps, child)
        })
        .collect();
    let root = TempNode::Chance { deals, tag: 0 };
    let tree = PublicTree::compile(TreeSpec {
        root,
        masks,
        transitions: Vec::new(),
        root_dims: PerPlayer::new(DIM as u32, DIM as u32),
    });
    CompiledGame {
        tree,
        evaluator: FixedEvaluator { terminals },
        root_ranges: PerPlayer::new(vec![1.0; DIM], vec![1.0; DIM]),
        normalizer: (DIM * (DIM - 1)) as f64,
    }
}

fn solve(par: ParConfig, iters: u64) -> Solver<FixedEvaluator, F32Storage> {
    let game = build_game();
    let mut solver = Solver::<_, F32Storage>::new(game, Box::<Dcfr>::default(), Some(iters));
    solver.set_par(par);
    solver.run(iters);
    solver
}

fn bit_pattern(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn parallel_chance_fanout_is_bitwise_deterministic() {
    // Root has 16 >= min_children(12) deals, chance_depth 2: this
    // parallelizes at the root.
    let par_on = ParConfig {
        chance_depth: 2,
        min_children: 12,
    };
    // chance_depth 0 alone already forces sequential everywhere;
    // min_children(usize::MAX) is redundant belt-and-suspenders.
    let par_off = ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    };

    let solver_par = solve(par_on, 50);
    let solver_seq = solve(par_off, 50);

    let (regrets_par, strategy_par) = solver_par.storage().snapshot();
    let (regrets_seq, strategy_seq) = solver_seq.storage().snapshot();

    assert_eq!(regrets_par.len(), regrets_seq.len());
    assert_eq!(strategy_par.len(), strategy_seq.len());
    assert_eq!(
        bit_pattern(&regrets_par),
        bit_pattern(&regrets_seq),
        "regret bit patterns diverged between parallel and sequential passes"
    );
    assert_eq!(
        bit_pattern(&strategy_par),
        bit_pattern(&strategy_seq),
        "strategy-sum bit patterns diverged between parallel and sequential passes"
    );

    for p in Player::BOTH {
        assert_eq!(
            solver_par.expected_value(p).to_bits(),
            solver_seq.expected_value(p).to_bits(),
            "expected_value diverged for {p:?}"
        );
    }

    let expl_par = solver_par.exploitability();
    let expl_seq = solver_seq.exploitability();
    for p in Player::BOTH {
        assert_eq!(
            expl_par[p].to_bits(),
            expl_seq[p].to_bits(),
            "exploitability diverged for {p:?}"
        );
    }
}
