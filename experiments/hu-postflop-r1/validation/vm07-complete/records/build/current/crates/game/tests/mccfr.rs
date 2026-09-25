//! Correctness harness for the chance-sampled MCCFR driver
//! (`engine::McSolver`), mirroring `tests/toys.rs`'s full-traversal harness:
//! Kuhn (no chance node — the sampled pass degenerates to the exact full
//! traversal) as a fast smoke test, Leduc (one chance node — the board
//! card) as the real chance-sampling exit test.

use cards::Player;
use engine::{F32Storage, McCfg, McSolver, Solver, StorageState, Vanilla, linear_cfr};
use game::{ChipEv, NoRake, PayoffPipeline};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

const LEDUC_VALUE: f64 = -0.0856;

/// Kuhn has no chance node, so `McSolver`'s sampled pass never touches the
/// RNG: every node visited is an `Action` node, handled by `mccfr_pass`
/// with exactly the same vector-form arithmetic (same order of operations)
/// as `cfr_pass`. With batched discounting disabled (`discount_until: 0`,
/// so `scale_all` is never invoked) and pruning off, that means a `Vanilla`
/// full-traversal `Solver` and an `McSolver` should end up bit-for-bit
/// identical after the same number of iterations.
#[test]
fn mccfr_kuhn_matches_full_traversal_bit_exactly() {
    // Vanilla (uniform-weighted) CFR converges slowly (O(1/sqrt(T))); 50k
    // iterations comfortably clears 1e-4 on Kuhn (see the probe in the
    // implementation notes) while staying fast since Kuhn has no chance
    // node to traverse.
    let iters = 50_000;

    let toy_full = game::kuhn(chip_ev());
    let mut full_solver = Solver::<_, F32Storage>::new(toy_full.game, Box::new(Vanilla), None);
    full_solver.run(iters);

    let toy_mc = game::kuhn(chip_ev());
    let mc_cfg = McCfg {
        seed: 7,
        discount_until: 0,
        ..Default::default()
    };
    let mut mc_solver = McSolver::<_, F32Storage>::new(toy_mc.game, mc_cfg);
    mc_solver.run(iters);

    let (full_regrets, full_strategy) = full_solver.storage().snapshot();
    match mc_solver.state().storage {
        StorageState::F32 {
            regrets,
            strategy_sum,
        } => {
            assert_eq!(
                regrets, full_regrets,
                "regrets diverged from full traversal"
            );
            assert_eq!(
                strategy_sum, full_strategy,
                "strategy sums diverged from full traversal"
            );
        }
        StorageState::I16 { .. } => panic!("expected F32 storage state"),
    }

    let expl = mc_solver.exploitability();
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

/// The exit test: `McSolver`'s exact average-strategy evaluation
/// (`exploitability`/`expected_value`, both full-traversal walks over the
/// accumulated average strategy) must agree with a from-scratch
/// full-traversal `Solver` solve, and the average strategies at a handful
/// of early action nodes must agree per (action, hand) probability.
///
/// Chance sampling only touches Leduc's one chance node (the board deal),
/// so this is the only test that actually exercises the RNG-driven
/// sampling path end to end.
#[test]
#[ignore = "4M MCCFR iterations; CI runs it in release with --include-ignored"]
fn mccfr_matches_full_traversal_on_leduc() {
    let node_lookup = game::leduc(chip_ev());
    let root = node_lookup.node_by_history("").unwrap();
    let after_check = node_lookup.node_by_history("c").unwrap();

    // linear_cfr() (the schedule MCCFR's batched discounting approximates)
    // converges noticeably slower than the project default (Dcfr::default,
    // alpha=1.5/beta=0/gamma=3) — 20k iterations to clear 2e-3 vs. the 5k
    // `tests/toys.rs` uses with the default schedule.
    let full_toy = game::leduc(chip_ev());
    let mut full_solver =
        Solver::<_, F32Storage>::new(full_toy.game, Box::new(linear_cfr()), Some(20_000));
    full_solver.run(20_000);
    let full_expl = full_solver.exploitability();
    let full_nash_conv = full_expl[Player::P0] + full_expl[Player::P1];
    assert!(
        full_nash_conv < 2e-3,
        "full-traversal reference solve didn't converge: NashConv = {full_nash_conv}"
    );
    let full_value = full_solver.expected_value(Player::P0);
    assert!(
        (full_value - LEDUC_VALUE).abs() < 3e-3,
        "full-traversal reference value {full_value}, expected about {LEDUC_VALUE}"
    );

    // Only Leduc's one chance node (the board deal) is sampled; both
    // players' action nodes stay full vector-form. That bounds the
    // per-iteration variance to "which of 6 boards fired", but batched
    // early discounting (unit weight before/after, not linear_cfr's smooth
    // per-iteration discount) means the asymptotic averaging rate is
    // effectively vanilla CFR's O(1/sqrt(T)) once discount_until (10k) is
    // behind it — hence needing millions, not hundreds of thousands, of
    // sampled iterations for a tight bound (empirically: ~3.6e-3 NashConv
    // at 4M iterations, comfortably under the 5e-3 bar; ~1.3e-2 at 300k).
    let mc_toy = game::leduc(chip_ev());
    let mc_cfg = McCfg {
        seed: 42,
        ..Default::default()
    };
    let mut mc_solver = McSolver::<_, F32Storage>::new(mc_toy.game, mc_cfg);
    mc_solver.run(4_000_000);

    let mc_expl = mc_solver.exploitability();
    let mc_nash_conv = mc_expl[Player::P0] + mc_expl[Player::P1];
    assert!(mc_nash_conv < 5e-3, "mccfr NashConv = {mc_nash_conv}");

    let mc_value = mc_solver.expected_value(Player::P0);
    assert!(
        (mc_value - full_value).abs() < 2e-3,
        "mccfr value {mc_value} vs full-traversal value {full_value}"
    );

    for node in [root, after_check] {
        let full_sigma = full_solver.average_strategy_at(node);
        let mc_sigma = mc_solver.average_strategy_at(node);
        for (a, b) in full_sigma.iter().zip(&mc_sigma) {
            assert!(
                (a - b).abs() < 0.05,
                "average strategy diverged at node {node}: {full_sigma:?} vs {mc_sigma:?}"
            );
        }
    }
}

/// Two fresh solvers, same seed, same iteration count: the whole run
/// (regrets, strategy sums, and the RNG's own position) must be
/// reproducible from `(seed, iteration count)` alone.
#[test]
fn sampled_run_is_deterministic_for_fixed_seed() {
    let cfg = McCfg {
        seed: 123,
        ..Default::default()
    };

    let toy_a = game::leduc(chip_ev());
    let mut solver_a = McSolver::<_, F32Storage>::new(toy_a.game, cfg);
    solver_a.run(1_000);

    let toy_b = game::leduc(chip_ev());
    let mut solver_b = McSolver::<_, F32Storage>::new(toy_b.game, cfg);
    solver_b.run(1_000);

    assert_eq!(solver_a.state(), solver_b.state());
}

/// Snapshotting mid-run and resuming must reproduce the same continuation
/// as running straight through — this is what forces `McSolverState` to
/// capture the RNG's exact stream position, not just its seed.
#[test]
fn state_round_trip_resumes_bit_identically() {
    let cfg = McCfg {
        seed: 99,
        ..Default::default()
    };

    let toy_x = game::leduc(chip_ev());
    let mut solver_x = McSolver::<_, F32Storage>::new(toy_x.game, cfg);
    solver_x.run(500);
    let snapshot = solver_x.state();
    solver_x.run(500);
    let x = solver_x.state();

    let toy_y = game::leduc(chip_ev());
    let mut solver_y = McSolver::<_, F32Storage>::new(toy_y.game, cfg);
    solver_y.restore_state(snapshot).unwrap();
    solver_y.run(500);
    let y = solver_y.state();

    assert_eq!(x, y);
}

/// Aggressive negative-regret pruning (threshold above zero regret, so
/// almost every negative-regret action is skip-eligible) trades exactness
/// for speed but must still converge, and must never panic (in particular,
/// the "never produce an empty node" fallback when every action at a node
/// is pruned this visit). Needs several hundred thousand iterations to
/// clear the (loose) 2e-2 bar, which is slow uninstrumented in debug.
#[test]
#[ignore = "300k MCCFR iterations; CI runs it in release with --include-ignored"]
fn pruning_converges_and_never_panics() {
    let toy = game::leduc(chip_ev());
    let cfg = McCfg {
        seed: 5,
        prune_threshold: Some(-1.0),
        ..Default::default()
    };
    let mut solver = McSolver::<_, F32Storage>::new(toy.game, cfg);
    solver.run(300_000);

    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    assert!(nash_conv < 2e-2, "pruned mccfr NashConv = {nash_conv}");
}
