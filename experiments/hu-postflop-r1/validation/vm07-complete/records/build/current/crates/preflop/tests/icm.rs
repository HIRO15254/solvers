//! End-to-end ICM validation on the preflop trunk (roadmap M6).
//!
//! Heads-up Malmuth–Harville ICM is affine in stacks
//! (`$EV_i = p2 + (p1 - p2) * s_i / S`), so a pure HU-ICM solve must
//! reproduce the chip-EV solve's strategy exactly (up to float noise):
//! regret matching is invariant under positive affine utility rescaling and
//! the discount schedule is multiplicative. This exercises the whole payoff
//! pipeline (bake -> coefficients -> evaluator -> solver) under a
//! non-trivial utility model for free.

use cards::{Chips, NUM_CLASSES, PerPlayer, Player, Range};
use engine::{Dcfr, F32Storage, Solver};
use game::{ChipEv, Icm, NoRake, PayoffPipeline, UtilityModel};
use preflop::{EquityShowdown, EquityTable, PreflopConfig, build_preflop_game};

fn push_fold_config() -> PreflopConfig {
    PreflopConfig {
        effective_stack: Chips(100),
        sb: Chips(5),
        bb: Chips(10),
        ranges: PerPlayer::new(Range::full(), Range::full()),
        open_sizes_bb: Vec::new(),
        raise_factors: Vec::new(),
        max_raises: 1,
        include_allin: true,
        allow_limp: false,
        track_node_info: true,
    }
}

/// Deterministic asymmetric tie-free table (win(h,o) + win(o,h) == 1).
fn synthetic_table() -> EquityTable {
    let s = |x: usize| (x + 1) as f64;
    let mut win = vec![0.0f64; NUM_CLASSES * NUM_CLASSES];
    for h in 0..NUM_CLASSES {
        for o in 0..NUM_CLASSES {
            win[h * NUM_CLASSES + o] = 0.25 + 0.5 * s(h) / (s(h) + s(o));
        }
    }
    EquityTable::from_probabilities(win, vec![0.0; NUM_CLASSES * NUM_CLASSES])
}

fn solve_strategies(utility: &dyn UtilityModel) -> (Vec<f32>, Vec<f32>, PerPlayer<f64>) {
    let config = push_fold_config();
    let table = synthetic_table();
    let model = EquityShowdown::default();
    let pipeline = PayoffPipeline {
        rake: &NoRake,
        utility,
    };
    let game = build_preflop_game(&config, &table, &model, pipeline);
    assert!(
        game.game.zero_sum,
        "unraked affine utility must be zero-sum"
    );
    let root = game.node_by_history("").unwrap();
    let jam = game.node_by_history("r100").unwrap();
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::new(Dcfr::default()), Some(1000));
    solver.run(1000);
    (
        solver.average_strategy_at(root),
        solver.average_strategy_at(jam),
        solver.exploitability(),
    )
}

#[test]
fn hu_icm_solve_matches_chip_ev_solve() {
    let (root_ev, jam_ev, expl_ev) = solve_strategies(&ChipEv);
    let (root_icm, jam_icm, expl_icm) = solve_strategies(&Icm {
        payouts: [100.0, 60.0],
    });

    // Same equilibrium, same trajectory: strategies match to f32 noise.
    for (i, (a, b)) in root_ev.iter().zip(&root_icm).enumerate() {
        assert!(
            (a - b).abs() < 1e-4,
            "root strategy diverged at {i}: chip-ev {a} vs icm {b}"
        );
    }
    for (i, (a, b)) in jam_ev.iter().zip(&jam_icm).enumerate() {
        assert!(
            (a - b).abs() < 1e-4,
            "jam response diverged at {i}: chip-ev {a} vs icm {b}"
        );
    }

    // Both converge. ICM exploitability lives on the $EV scale (the chip
    // scale times the affine slope (p1 - p2) / total = 0.2), so its bound
    // is proportionally tighter; no ratio assertion — at convergence both
    // gaps sit at float-noise zero where a ratio is meaningless.
    let nc_ev = expl_ev[Player::P0] + expl_ev[Player::P1];
    let nc_icm = expl_icm[Player::P0] + expl_icm[Player::P1];
    assert!(nc_ev.abs() < 1e-2, "chip-ev nash_conv {nc_ev}");
    assert!(nc_icm.abs() < 1e-2 * 0.2, "icm nash_conv {nc_icm}");
}
