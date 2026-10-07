//! CFR precision accuracy, evaluation isolation and bitwise determinism.
use crate::game::{ChipEv, NoRake, PayoffPipeline};
use crate::*;
use hu_engine::{
    Dcfr, F32Storage, I16Storage, MixedStorage, Storage, StorageState, TerminalEvaluator,
};
use nlh::{Chips, combo_cards};

fn config() -> PostflopConfig {
    PostflopConfig {
        board: "Ks 7h 2d 3c"
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect(),
        ranges: PerPlayer::new(
            "22+,A2s+,KTs+,QJs,JTs,AJo+".parse().unwrap(),
            "99-22,AJs-A2s,KJs-K2s,QJs,JTs,AJo-A2o".parse().unwrap(),
        ),
        pot: Chips(6),
        effective_stack: Chips(9),
        streets: PerStreet {
            turn: StreetTree {
                include_allin: true,
                max_aggressive_actions: 1,
                ..Default::default()
            },
            river: StreetTree {
                include_allin: true,
                max_aggressive_actions: 1,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}
fn game() -> PostflopGame {
    build_postflop_game(
        &config(),
        PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        },
    )
}

// Zero-valued CFVs make pointwise relative error undefined. Report the relative
// infinity-norm error max|variant-exact| / max|exact| for each terminal vector.
fn relative_error(exact: &[f32], variant: &[f32]) -> f64 {
    let scale = exact.iter().map(|v| f64::from(v.abs())).fold(0.0, f64::max);
    let error = exact
        .iter()
        .zip(variant)
        .map(|(&a, &b)| {
            assert!(b.is_finite());
            (f64::from(a) - f64::from(b)).abs()
        })
        .fold(0.0, f64::max);
    if scale == 0.0 {
        assert_eq!(error, 0.0);
        0.0
    } else {
        error / scale
    }
}

#[test]
fn f32_real_terminals_close_and_eval_stays_exact() {
    let mut game = game();
    let evaluator = &mut game.game.evaluator;
    let mut maxima = [0.0f64; 2];
    let mut folds = 0;
    let mut showdowns = 0;
    for node in &game.game.tree.nodes {
        if node.kind != NodeKind::Terminal {
            continue;
        }
        let term = &evaluator.terminals[node.aux as usize];
        let board = term.board_mask;
        match term.kind {
            super::TerminalKind::Fold { .. } => folds += 1,
            _ => showdowns += 1,
        }
        for p in Player::BOTH {
            for pattern in 0..3 {
                let reach: Vec<f32> = evaluator
                    .hands
                    .combos(p.opponent())
                    .iter()
                    .enumerate()
                    .map(|(i, &h)| {
                        let (a, b) = combo_cards(h as usize);
                        if board & ((1u64 << a.index()) | (1u64 << b.index())) != 0
                            || (i + pattern) % 7 == 0
                        {
                            0.0
                        } else {
                            ((i * 37 % 101 + 1) as f32 / 103.0) * [1.0, 0.001, 100.0][pattern]
                        }
                    })
                    .collect();
                let mut exact = vec![0.0; evaluator.hands.len(p)];
                evaluator.eval(node.aux, p, &reach, &mut exact);
                for (idx, variant) in [CfrPrecision::F64, CfrPrecision::F32]
                    .into_iter()
                    .enumerate()
                {
                    evaluator.set_cfr_precision(variant);
                    let mut out = vec![f32::NAN; exact.len()];
                    evaluator.eval_cfr(node.aux, p, &reach, &mut out);
                    maxima[idx] = maxima[idx].max(relative_error(&exact, &out));
                    assert!(maxima[idx] < 1e-5, "{variant:?}: {}", maxima[idx]);
                    // Calling eval after selecting a relaxed CFR kernel stays bit-exact.
                    evaluator.eval(node.aux, p, &reach, &mut out);
                    assert_eq!(bits(&out), bits(&exact));
                }
            }
        }
    }
    assert!(folds > 0 && showdowns > 0);
    println!(
        "real terminal relative infinity errors: {maxima:?}; folds={folds} showdowns={showdowns}"
    );
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|v| v.to_bits()).collect()
}
fn state_bits(state: StorageState) -> Vec<u8> {
    let mut out = Vec::new();
    let mut float = |v: &[f32]| {
        for x in v {
            out.extend_from_slice(&x.to_le_bytes());
        }
    };
    match state {
        StorageState::F32 {
            regrets,
            strategy_sum,
        } => {
            float(&regrets);
            float(&strategy_sum);
        }
        StorageState::I16 {
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        } => {
            float(&regret_scales);
            float(&strategy_scales);
            for x in regrets.into_iter().chain(strategy_sum) {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        StorageState::Mixed {
            regrets,
            strategy_sum,
            regret_scales,
        } => {
            float(&strategy_sum);
            float(&regret_scales);
            for x in regrets {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
    }
    out
}
fn deterministic<S: Storage>() {
    for precision in [CfrPrecision::F64, CfrPrecision::F32] {
        let solve = |threads| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| {
                    let game = game().game;
                    assert!(game.tree.storage_len > 0);
                    let mut solver =
                        hu_engine::Solver::<_, S>::new(game, Box::<Dcfr>::default(), Some(12));
                    // One call selects both the regret matching and the evaluator kernels.
                    solver.set_cfr_precision(precision);
                    solver.set_par(hu_engine::ParConfig {
                        chance_depth: 2,
                        min_children: 2,
                    });
                    solver.run(12);
                    let state = solver.state();
                    // Restore the same state into an explicitly exact solver.
                    // EV/BR and the streaming save-value pass must ignore CFR choices.
                    let exact_game = self::game().game;
                    let mut exact = hu_engine::Solver::<_, S>::new(
                        exact_game,
                        Box::<Dcfr>::default(),
                        Some(12),
                    );
                    exact.set_cfr_precision(CfrPrecision::F64);
                    exact.restore_state(state.clone()).unwrap();
                    let evaluated = solver.evaluate();
                    let expected = exact.evaluate();
                    for p in Player::BOTH {
                        assert_eq!(evaluated.0[p].to_bits(), expected.0[p].to_bits());
                        assert_eq!(evaluated.1[p].to_bits(), expected.1[p].to_bits());
                    }
                    let saved = std::sync::Mutex::new(std::collections::BTreeMap::new());
                    solver.visit_expected_values(|node, values, reaches, sigma| {
                        saved.lock().unwrap().insert(
                            node,
                            (
                                bits(values[Player::P0]),
                                bits(values[Player::P1]),
                                bits(reaches[Player::P0]),
                                bits(reaches[Player::P1]),
                                bits(sigma),
                            ),
                        );
                    });
                    let exact_saved = std::sync::Mutex::new(std::collections::BTreeMap::new());
                    exact.visit_expected_values(|node, values, reaches, sigma| {
                        exact_saved.lock().unwrap().insert(
                            node,
                            (
                                bits(values[Player::P0]),
                                bits(values[Player::P1]),
                                bits(reaches[Player::P0]),
                                bits(reaches[Player::P1]),
                                bits(sigma),
                            ),
                        );
                    });
                    assert_eq!(
                        saved.into_inner().unwrap(),
                        exact_saved.into_inner().unwrap()
                    );
                    if precision == CfrPrecision::F32 && threads == 1 {
                        // Exercise the actual quantized .sol save path as well.
                        let temp = tempfile::tempdir().unwrap();
                        let summary =
                            crate::run::summarize(&solver, std::time::Duration::ZERO, evaluated.0);
                        let export = |s: &hu_engine::Solver<crate::PostflopEvaluator, S>,
                                      name: &str| {
                            let spec = crate::artifact::SolExportSpec {
                                path: temp.path().join(name),
                                mode: crate::artifact::SolStreets::Full,
                                config_toml: String::new(),
                                storage_name: std::any::type_name::<S>().into(),
                            };
                            crate::artifact::export_sol(
                                &spec,
                                s,
                                PerPlayer::new(0.0, 0.0),
                                nlh::Street::Turn,
                                &summary,
                                &mut |_| {},
                            )
                            .unwrap();
                            let payload = crate::sol::read_sol(&spec.path).unwrap();
                            postcard::to_allocvec(&payload).unwrap()
                        };
                        assert_eq!(export(&solver, "f32.sol"), export(&exact, "f64.sol"));
                    }
                    (state.iteration, state_bits(state.storage), evaluated)
                })
        };
        let a = solve(1);
        let b = solve(4);
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1, "{} {precision:?}", std::any::type_name::<S>());
        for p in Player::BOTH {
            assert_eq!(a.2.0[p].to_bits(), b.2.0[p].to_bits());
            assert_eq!(a.2.1[p].to_bits(), b.2.1[p].to_bits());
        }
    }
}
#[test]
fn precision_all_backends_deterministic_and_saved_ev_exact() {
    deterministic::<F32Storage>();
    deterministic::<I16Storage>();
    deterministic::<MixedStorage>();
}
