//! Correctness of the quantized `I16Storage` backend against the plain
//! `F32Storage` backend, and of checkpointable solver state, on Leduc.
//!
//! Reuses `hu_postflop::game::leduc`/`hu_postflop::game::kuhn` (the same toy games `tests/toys.rs`
//! solves) rather than duplicating game setup, since these tests exercise
//! `hu_engine::Storage` backends, not game-layer logic.

use hu_engine::{Dcfr, F32Storage, I16Storage, MixedStorage, Solver, StateMismatch, Storage};
use hu_postflop::game::{ChipEv, NoRake, PayoffPipeline};
use nlh::Player;

// Independent composition of the two existing backends. This records the
// actual reach-weighted strategy inputs produced by a complete traversal,
// including split views, without using MixedStorage/MixedView's op routing.
struct Reference<A, B> {
    quantized: A,
    float: B,
}

type ReferenceStorage = Reference<I16Storage, F32Storage>;
type ReferenceView<'a> = Reference<hu_engine::I16View<'a>, hu_engine::F32View<'a>>;

impl<A: hu_engine::StorageOps, B: hu_engine::StorageOps> hu_engine::StorageOps for Reference<A, B> {
    fn regret_matching(&self, r: hu_engine::StorageRef, idx: u32, out: &mut [f32]) {
        self.quantized.regret_matching(r, idx, out);
    }
    fn update_regrets(
        &mut self,
        r: hu_engine::StorageRef,
        idx: u32,
        inst: &[f32],
        d: &hu_engine::Discounts,
    ) {
        self.quantized.update_regrets(r, idx, inst, d);
    }
    fn accumulate_strategy(
        &mut self,
        r: hu_engine::StorageRef,
        idx: u32,
        weighted: &[f32],
        d: &hu_engine::Discounts,
    ) {
        self.float.accumulate_strategy(r, idx, weighted, d);
    }
    fn average_strategy(&self, r: hu_engine::StorageRef, idx: u32, out: &mut [f32]) {
        self.float.average_strategy(r, idx, out);
    }
    fn raw_regrets(&self, r: hu_engine::StorageRef, idx: u32, out: &mut [f32]) {
        self.quantized.raw_regrets(r, idx, out);
    }
}

impl Storage for ReferenceStorage {
    type View<'a> = ReferenceView<'a>;

    fn new(len: usize, refs: usize) -> Self {
        Self {
            quantized: I16Storage::new(len, refs),
            float: F32Storage::new(len, refs),
        }
    }
    fn view_mut(&mut self) -> ReferenceView<'_> {
        Reference {
            quantized: self.quantized.view_mut(),
            float: self.float.view_mut(),
        }
    }
    fn bytes_for(len: usize, refs: usize) -> u64 {
        I16Storage::bytes_for(len, refs) + F32Storage::bytes_for(len, refs)
    }
    fn state(&self) -> hu_engine::StorageState {
        let hu_engine::StorageArrays::Mixed {
            regrets,
            strategy_sum,
            regret_scales,
        } = self.arrays()
        else {
            unreachable!()
        };
        hu_engine::StorageState::Mixed {
            regrets: regrets.to_vec(),
            strategy_sum: strategy_sum.to_vec(),
            regret_scales: regret_scales.to_vec(),
        }
    }
    fn restore_state(&mut self, state: hu_engine::StorageState) -> Result<(), StateMismatch> {
        let hu_engine::StorageState::Mixed {
            regrets,
            strategy_sum,
            regret_scales,
        } = state
        else {
            return Err(StateMismatch::WrongVariant);
        };
        let hu_engine::StorageArraysMut::Mixed {
            regrets: dst_r,
            strategy_sum: dst_s,
            regret_scales: dst_scales,
        } = self.arrays_mut()
        else {
            unreachable!()
        };
        for (expected, actual) in [
            (dst_r.len(), regrets.len()),
            (dst_s.len(), strategy_sum.len()),
            (dst_scales.len(), regret_scales.len()),
        ] {
            if expected != actual {
                return Err(StateMismatch::WrongLength { expected, actual });
            }
        }
        dst_r.copy_from_slice(&regrets);
        dst_s.copy_from_slice(&strategy_sum);
        dst_scales.copy_from_slice(&regret_scales);
        Ok(())
    }
    fn arrays(&self) -> hu_engine::StorageArrays<'_> {
        match (self.quantized.arrays(), self.float.arrays()) {
            (
                hu_engine::StorageArrays::I16 {
                    regrets,
                    regret_scales,
                    ..
                },
                hu_engine::StorageArrays::F32 { strategy_sum, .. },
            ) => hu_engine::StorageArrays::Mixed {
                regrets,
                strategy_sum,
                regret_scales,
            },
            _ => unreachable!(),
        }
    }
    fn arrays_mut(&mut self) -> hu_engine::StorageArraysMut<'_> {
        match (self.quantized.arrays_mut(), self.float.arrays_mut()) {
            (
                hu_engine::StorageArraysMut::I16 {
                    regrets,
                    regret_scales,
                    ..
                },
                hu_engine::StorageArraysMut::F32 { strategy_sum, .. },
            ) => hu_engine::StorageArraysMut::Mixed {
                regrets,
                strategy_sum,
                regret_scales,
            },
            _ => unreachable!(),
        }
    }
    fn scale_all(&mut self, regret: f32, strategy: f32) {
        self.quantized.scale_all(regret, 1.0);
        self.float.scale_all(1.0, strategy);
    }
}

impl<'a> hu_engine::StorageView for ReferenceView<'a> {
    fn split(&mut self, spans: &[hu_engine::StorageSpan]) -> Vec<Self> {
        self.quantized
            .split(spans)
            .into_iter()
            .zip(self.float.split(spans))
            .map(|(quantized, float)| Reference { quantized, float })
            .collect()
    }
}

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

const LEDUC_VALUE: f64 = -0.0856;

fn solve<S: Storage>(iters: u64) -> Solver<hu_postflop::game::ToyEvaluator, S> {
    let mut solver = Solver::<_, S>::new(
        hu_postflop::game::leduc(chip_ev()).game,
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
    let root = hu_postflop::game::leduc(chip_ev())
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
    let mut solver_a = Solver::<_, S>::new(
        hu_postflop::game::leduc(chip_ev()).game,
        Box::<Dcfr>::default(),
        None,
    );
    solver_a.run(60);
    let checkpoint = solver_a.state();
    solver_a.run(40);

    let mut solver_b = Solver::<_, S>::new(
        hu_postflop::game::leduc(chip_ev()).game,
        Box::<Dcfr>::default(),
        None,
    );
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
    let mut f32_solver = Solver::<_, F32Storage>::new(
        hu_postflop::game::leduc(chip_ev()).game,
        Box::<Dcfr>::default(),
        None,
    );
    f32_solver.run(10);

    let mut i16_solver = Solver::<_, I16Storage>::new(
        hu_postflop::game::leduc(chip_ev()).game,
        Box::<Dcfr>::default(),
        None,
    );
    i16_solver.run(10);

    // Wrong variant: an F32 checkpoint can't restore into an I16 solver.
    let f32_state = f32_solver.state();
    assert_eq!(
        i16_solver.restore_state(f32_state),
        Err(StateMismatch::WrongVariant)
    );

    // Wrong length: Kuhn's storage shape differs from Leduc's.
    let kuhn_solver = Solver::<_, F32Storage>::new(
        hu_postflop::game::kuhn(chip_ev()).game,
        Box::<Dcfr>::default(),
        None,
    );
    let kuhn_state = kuhn_solver.state();
    assert!(matches!(
        f32_solver.restore_state(kuhn_state),
        Err(StateMismatch::WrongLength { .. })
    ));
}

#[test]
fn bytes_for_i16_smaller_than_f32_for_leduc_shape() {
    let leduc = hu_postflop::game::leduc(chip_ev());
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

#[test]
fn mixed_state_round_trip_is_deterministic() {
    state_round_trip_is_deterministic::<MixedStorage>();
}

#[test]
fn mixed_postflop_regrets_match_i16_and_threads_are_bitwise_equal() {
    use hu_engine::{ParConfig, StorageState};
    use hu_postflop::{PerStreet, PostflopConfig, StreetTree, build_postflop_game};
    use nlh::{Chips, PerPlayer};
    let config = PostflopConfig {
        board: "Ks 7h 2d 3c"
            .split_whitespace()
            .map(|c| c.parse().unwrap())
            .collect(),
        ranges: PerPlayer::new(
            "AA,QQ,AJs,76s".parse().unwrap(),
            "KK,JJ,KQs,54s".parse().unwrap(),
        ),
        pot: Chips(2000),
        effective_stack: Chips(5000),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::pot_fractions(&[0.5], &[1.0], 1),
            river: StreetTree::pot_fractions(&[0.5], &[1.0], 1),
        },
        iso_merging: false,
        ..Default::default()
    };
    let run = |threads, mixed| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let game = build_postflop_game(&config, chip_ev()).game;
            let par = ParConfig {
                chance_depth: 2,
                min_children: 2,
            };
            if mixed {
                let mut solver =
                    Solver::<_, MixedStorage>::new(game, Box::<Dcfr>::default(), Some(100));
                solver.set_par(par);
                solver.run(100);
                solver.state().storage
            } else {
                let mut solver =
                    Solver::<_, I16Storage>::new(game, Box::<Dcfr>::default(), Some(100));
                solver.set_par(par);
                solver.run(100);
                solver.state().storage
            }
        })
    };
    let one = run(1, true);
    let four = run(4, true);
    assert_eq!(
        postcard::to_allocvec(&one).unwrap(),
        postcard::to_allocvec(&four).unwrap()
    );
    for threads in [1, 4] {
        let StorageState::I16 {
            regrets,
            regret_scales,
            ..
        } = run(threads, false)
        else {
            unreachable!()
        };
        let StorageState::Mixed {
            regrets: actual,
            regret_scales: scales,
            ..
        } = &one
        else {
            unreachable!()
        };
        assert_eq!(&regrets, actual);
        assert_eq!(
            regret_scales
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>(),
            scales.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
        );
        let reference = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let mut solver = Solver::<_, ReferenceStorage>::new(
                    build_postflop_game(&config, chip_ev()).game,
                    Box::<Dcfr>::default(),
                    Some(100),
                );
                solver.set_par(ParConfig {
                    chance_depth: 2,
                    min_children: 2,
                });
                solver.run(100);
                solver.state().storage
            });
        assert_eq!(
            postcard::to_allocvec(&one).unwrap(),
            postcard::to_allocvec(&reference).unwrap(),
            "real-game strategy sums must match f32 accumulation of the same inputs ({threads} threads)"
        );
    }
}
