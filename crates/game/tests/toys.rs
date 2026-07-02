//! Correctness harness on toy games with known solutions. These tests
//! exercise the full production path: spec -> TempNode -> PublicTree ->
//! payoff baking -> vector CFR -> best-response exploitability.

use cards::Player;
use engine::{CfrPlus, Dcfr, DiscountSchedule, F32Storage, Solver, Vanilla};
use game::{ChipEv, Icm, NoRake, PayoffPipeline, PercentCapRake, ToyGame};

fn solve(
    game: ToyGame,
    schedule: Box<dyn DiscountSchedule>,
    iters: u64,
) -> Solver<game::ToyEvaluator, F32Storage> {
    let mut solver = Solver::<_, F32Storage>::new(game.game, schedule, Some(iters));
    solver.run(iters);
    solver
}

fn chip_ev(rake: &dyn game::RakeModel) -> PayoffPipeline<'_> {
    PayoffPipeline {
        rake,
        utility: &ChipEv,
    }
}

const KUHN_VALUE: f64 = -1.0 / 18.0;
const LEDUC_VALUE: f64 = -0.0856;

#[test]
fn kuhn_game_value_and_exploitability() {
    let solver = solve(
        game::kuhn(chip_ev(&NoRake)),
        Box::new(Dcfr::default()),
        10_000,
    );
    let value = solver.expected_value(Player::P0);
    assert!(
        (value - KUHN_VALUE).abs() < 1e-4,
        "kuhn value {value}, expected {KUHN_VALUE}"
    );
    let expl = solver.exploitability();
    assert!(
        expl[Player::P0].abs() < 1e-4,
        "expl P0 = {}",
        expl[Player::P0]
    );
    assert!(
        expl[Player::P1].abs() < 1e-4,
        "expl P1 = {}",
        expl[Player::P1]
    );
}

#[test]
fn kuhn_zero_sum_invariant() {
    let solver = solve(
        game::kuhn(chip_ev(&NoRake)),
        Box::new(Dcfr::default()),
        1_000,
    );
    let sum = solver.expected_value(Player::P0) + solver.expected_value(Player::P1);
    assert!(
        sum.abs() < 1e-6,
        "chip-EV no-rake game must be zero-sum, sum = {sum}"
    );
}

#[test]
fn kuhn_equilibrium_family_membership() {
    // Kuhn equilibria: P0 bets J with probability a in [0, 1/3], bets K
    // with 3a, always checks Q at the root; P0 calls a bet with Q with
    // probability a + 1/3.
    let toy = game::kuhn(chip_ev(&NoRake));
    let root = toy.node_by_history("").unwrap();
    let after_check_bet = toy.node_by_history("cb").unwrap();
    let solver = solve(toy, Box::new(Dcfr::default()), 50_000);

    let root_sigma = solver.average_strategy_at(root);
    // Layout: action-major over 3 hands (J, Q, K); action 0 = check, 1 = bet.
    let bet_j = root_sigma[3] as f64;
    let bet_q = root_sigma[4] as f64;
    let bet_k = root_sigma[5] as f64;
    assert!(
        (-0.005..=1.0 / 3.0 + 0.005).contains(&bet_j),
        "bet(J) = {bet_j}"
    );
    assert!(bet_q < 0.005, "P0 must not bet Q at the root, got {bet_q}");
    assert!(
        (bet_k - 3.0 * bet_j).abs() < 0.01,
        "bet(K) = {bet_k} should be 3 * bet(J) = {}",
        3.0 * bet_j
    );

    // After check-bet, P0 calls with Q with probability bet(J) + 1/3.
    let sigma = solver.average_strategy_at(after_check_bet);
    let call_q = sigma[4] as f64; // action 0 = fold, 1 = call
    assert!(
        (call_q - (bet_j + 1.0 / 3.0)).abs() < 0.01,
        "call(Q) = {call_q}, expected {}",
        bet_j + 1.0 / 3.0
    );
}

#[test]
fn leduc_game_value_and_exploitability() {
    let solver = solve(
        game::leduc(chip_ev(&NoRake)),
        Box::new(Dcfr::default()),
        5_000,
    );
    let value = solver.expected_value(Player::P0);
    assert!(
        (value - LEDUC_VALUE).abs() < 3e-3,
        "leduc value {value}, expected about {LEDUC_VALUE}"
    );
    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    assert!(nash_conv < 2e-3, "leduc NashConv = {nash_conv}");
}

#[test]
fn discount_schedules_beat_vanilla() {
    // At a fixed iteration budget on Leduc, both modern schedules should
    // crush vanilla CFR. The DCFR-vs-CFR+ ordering reported on real poker
    // subgame trees is not stable on a game this tiny (both are already at
    // ~1e-3 NashConv here), so we only pin the vanilla separation.
    let budget = 300;
    let nash_conv = |schedule: Box<dyn DiscountSchedule>| -> f64 {
        let solver = solve(game::leduc(chip_ev(&NoRake)), schedule, budget);
        let expl = solver.exploitability();
        expl[Player::P0] + expl[Player::P1]
    };
    let vanilla = nash_conv(Box::new(Vanilla));
    let cfr_plus = nash_conv(Box::new(CfrPlus));
    let dcfr = nash_conv(Box::<Dcfr>::default());
    assert!(
        dcfr < vanilla / 3.0 && cfr_plus < vanilla / 3.0,
        "expected dcfr ({dcfr}) and cfr+ ({cfr_plus}) well below vanilla ({vanilla})"
    );
}

#[test]
fn raked_game_is_not_zero_sum_and_asymmetric() {
    let rake = PercentCapRake {
        rate: 0.10,
        cap: 1.0,
        no_flop_no_drop: false,
    };
    let solver = solve(game::kuhn(chip_ev(&rake)), Box::new(Dcfr::default()), 5_000);
    let sum = solver.expected_value(Player::P0) + solver.expected_value(Player::P1);
    assert!(sum < -1e-3, "raked game must leak value, sum = {sum}");
}

#[test]
fn pure_hu_icm_solve_matches_chip_ev_solve() {
    // Malmuth-Harville ICM with two players is affine in stacks, so the
    // solved strategy must match the chip-EV strategy. This validates the
    // whole payoff pipeline (bake -> matrices -> solve) at once.
    let icm = Icm {
        payouts: [100.0, 60.0],
    };
    let icm_pipeline = PayoffPipeline {
        rake: &NoRake,
        utility: &icm,
    };
    let toy_icm = game::kuhn(icm_pipeline);
    let toy_cev = game::kuhn(chip_ev(&NoRake));
    let root = toy_cev.node_by_history("").unwrap();

    let solver_icm = solve(toy_icm, Box::new(Dcfr::default()), 10_000);
    let solver_cev = solve(toy_cev, Box::new(Dcfr::default()), 10_000);

    let sigma_icm = solver_icm.average_strategy_at(root);
    let sigma_cev = solver_cev.average_strategy_at(root);
    for (a, b) in sigma_icm.iter().zip(&sigma_cev) {
        assert!(
            (a - b).abs() < 5e-3,
            "ICM and chip-EV strategies diverged: {sigma_icm:?} vs {sigma_cev:?}"
        );
    }
}

#[test]
fn exploitability_decreases_over_time() {
    let toy = game::leduc(chip_ev(&NoRake));
    let mut solver = Solver::<_, F32Storage>::new(toy.game, Box::new(Dcfr::default()), None);
    let mut last = f64::INFINITY;
    for _ in 0..4 {
        solver.run(250);
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        assert!(
            nash_conv < last,
            "NashConv should keep shrinking: {nash_conv} vs previous {last}"
        );
        last = nash_conv;
    }
}
