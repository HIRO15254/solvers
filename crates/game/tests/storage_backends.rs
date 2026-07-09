//! Correctness of the quantized `I16Storage` backend against the plain
//! `F32Storage` backend, and of checkpointable solver state, on Leduc.
//!
//! Reuses `game::leduc`/`game::kuhn` (the same toy games `tests/toys.rs`
//! solves) rather than duplicating game setup, since these tests exercise
//! `engine::Storage` backends, not game-layer logic.

use cards::Player;
use engine::{Dcfr, F32Storage, I16Storage, Solver, StateMismatch, Storage};
use game::{ChipEv, NoRake, PayoffPipeline};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

const LEDUC_VALUE: f64 = -0.0856;

fn solve<S: Storage>(iters: u64) -> Solver<game::ToyEvaluator, S> {
    let mut solver = Solver::<_, S>::new(
        game::leduc(chip_ev()).game,
        Box::<Dcfr>::default(),
        Some(iters),
    );
    solver.run(iters);
    solver
}

#[test]
fn leduc_i16_game_value_and_exploitability() {
    let solver = solve::<I16Storage>(2000);
    let value = solver.expected_value(Player::P0);
    assert!(
        (value - LEDUC_VALUE).abs() < 2e-3,
        "i16 leduc value {value}, expected about {LEDUC_VALUE}"
    );
    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    assert!(nash_conv < 5e-3, "i16 leduc NashConv = {nash_conv}");
}

#[test]
fn leduc_i16_matches_f32_at_500_iters() {
    // Root history "": both builds are identical deterministic trees, so
    // the root node id is the same for either.
    let root = game::leduc(chip_ev())
        .node_by_history("")
        .expect("leduc has a root");

    let f32_solver = solve::<F32Storage>(500);
    let i16_solver = solve::<I16Storage>(500);

    let ev_f32 = f32_solver.expected_value(Player::P0);
    let ev_i16 = i16_solver.expected_value(Player::P0);
    assert!(
        (ev_f32 - ev_i16).abs() < 1e-3,
        "expected_value diverged: f32={ev_f32} i16={ev_i16}"
    );

    let sigma_f32 = f32_solver.average_strategy_at(root);
    let sigma_i16 = i16_solver.average_strategy_at(root);
    assert_eq!(sigma_f32.len(), sigma_i16.len());
    for (a, b) in sigma_f32.iter().zip(&sigma_i16) {
        assert!(
            (a - b).abs() < 2e-2,
            "root avg strategy diverged: {sigma_f32:?} vs {sigma_i16:?}"
        );
    }
}

/// Solve 60 iterations, checkpoint, run 40 more directly vs. restore the
/// checkpoint into a fresh solver and run the same 40 — the solver has no
/// RNG or other hidden state, so both paths must land on bit-identical
/// storage state and iteration count.
fn state_round_trip_is_deterministic<S: Storage>() {
    let mut solver_a =
        Solver::<_, S>::new(game::leduc(chip_ev()).game, Box::<Dcfr>::default(), None);
    solver_a.run(60);
    let checkpoint = solver_a.state();
    solver_a.run(40);

    let mut solver_b =
        Solver::<_, S>::new(game::leduc(chip_ev()).game, Box::<Dcfr>::default(), None);
    solver_b
        .restore_state(checkpoint)
        .expect("checkpoint must restore into a freshly built solver of the same shape");
    solver_b.run(40);

    assert_eq!(solver_a.iteration(), solver_b.iteration());
    assert_eq!(solver_a.storage().state(), solver_b.storage().state());
}

#[test]
fn f32_state_round_trip_is_deterministic() {
    state_round_trip_is_deterministic::<F32Storage>();
}

#[test]
fn i16_state_round_trip_is_deterministic() {
    state_round_trip_is_deterministic::<I16Storage>();
}

#[test]
fn restore_state_rejects_wrong_variant_or_length() {
    let mut f32_solver =
        Solver::<_, F32Storage>::new(game::leduc(chip_ev()).game, Box::<Dcfr>::default(), None);
    f32_solver.run(10);

    let mut i16_solver =
        Solver::<_, I16Storage>::new(game::leduc(chip_ev()).game, Box::<Dcfr>::default(), None);
    i16_solver.run(10);

    // Wrong variant: an F32 checkpoint can't restore into an I16 solver.
    let f32_state = f32_solver.state();
    assert_eq!(
        i16_solver.restore_state(f32_state),
        Err(StateMismatch::WrongVariant)
    );

    // Wrong length: Kuhn's storage shape differs from Leduc's.
    let kuhn_solver =
        Solver::<_, F32Storage>::new(game::kuhn(chip_ev()).game, Box::<Dcfr>::default(), None);
    let kuhn_state = kuhn_solver.state();
    assert!(matches!(
        f32_solver.restore_state(kuhn_state),
        Err(StateMismatch::WrongLength { .. })
    ));
}

#[test]
fn bytes_for_i16_smaller_than_f32_for_leduc_shape() {
    let leduc = game::leduc(chip_ev());
    let len = leduc.game.tree.storage_len;
    let num_refs = leduc.game.tree.storage_refs.len();

    let f32_bytes = F32Storage::bytes_for(len, num_refs);
    let i16_bytes = I16Storage::bytes_for(len, num_refs);
    assert!(
        i16_bytes < f32_bytes,
        "i16 ({i16_bytes}) should be smaller than f32 ({f32_bytes}) for equal shapes"
    );

    // At a large element count the per-ref scale overhead is negligible, so
    // i16 should land close to half of f32 (2 bytes/element vs 4).
    let (big_len, big_refs) = (2_000_000usize, 5_000usize);
    let big_f32 = F32Storage::bytes_for(big_len, big_refs);
    let big_i16 = I16Storage::bytes_for(big_len, big_refs);
    let ratio = big_i16 as f64 / big_f32 as f64;
    assert!(
        (0.49..0.51).contains(&ratio),
        "expected i16 close to half of f32 for large len, ratio = {ratio}"
    );
}
