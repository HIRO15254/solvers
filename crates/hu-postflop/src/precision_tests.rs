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

#[test]
fn weighted_showdown_matches_head_f32_and_f64_on_real_boards() {
    let game = game();
    let e = &game.game.evaluator;
    let mut maxima = [0.0f64; 2];
    let mut seed = 0x719c_b123u32;
    // All 48 river tables, both asymmetric seats, dense/sparse/zero reaches.
    for table in &e.rank_tables {
        for p in Player::BOTH {
            for pattern in 0..4 {
                let mut reach: Vec<f32> = (0..e.hands.len(p.opponent()))
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        match pattern {
                            2 => -0.0,
                            1 if !seed.is_multiple_of(10) => -0.0,
                            _ => (seed >> 8) as f32 / (1u32 << 24) as f32,
                        }
                    })
                    .collect();
                let mut live = vec![false; reach.len()];
                for h in &table[p.opponent()].hands {
                    live[h.local as usize] = true;
                }
                for (r, live) in reach.iter_mut().zip(live) {
                    if !live {
                        *r = 0.0;
                    }
                }
                for utilities in [
                    [1.25, -0.125, -1.75],
                    [0.0; 3],
                    [-2.0, 1.0, 0.5],
                    [1.0, 0.0, -1.0],
                    [0.0, 0.25, 0.0],
                ] {
                    let mut out = vec![0.0; e.hands.len(p)];
                    let mut exact = out.clone();
                    let mut head = out.clone();
                    crate::kernel::showdown_kernel(
                        &table[p],
                        &table[p.opponent()],
                        &e.hands.same[p],
                        utilities,
                        &reach,
                        &mut exact,
                    );
                    crate::kernel::head_showdown_kernel_relaxed_f32(
                        &table[p],
                        &table[p.opponent()],
                        &e.hands.same[p],
                        utilities,
                        &reach,
                        &mut head,
                    );
                    crate::kernel::showdown_kernel_relaxed_f32(
                        &table[p],
                        &table[p.opponent()],
                        &e.hands.same[p],
                        utilities,
                        &reach,
                        &mut out,
                    );
                    maxima[0] = maxima[0].max(relative_error(&exact, &out));
                    maxima[1] = maxima[1].max(relative_error(&head, &out));
                    for u_fold in [-1.5, 0.0, 0.75] {
                        let mut expected_fold = vec![0.0; out.len()];
                        crate::kernel::fold_kernel_relaxed_f32(
                            &table[p].hands,
                            &table[p.opponent()].hands,
                            &e.hands.same[p],
                            u_fold,
                            0,
                            &reach,
                            &mut expected_fold,
                        );
                        let mut fused_show = vec![0.0; out.len()];
                        let mut fused_fold = vec![f32::NAN; out.len()];
                        crate::kernel::showdown_fold_kernel_relaxed_f32(
                            &table[p],
                            &table[p.opponent()],
                            &e.hands.same[p],
                            [utilities[0], utilities[1], utilities[2], u_fold],
                            &reach,
                            &mut fused_show,
                            &mut fused_fold,
                        );
                        assert_eq!(bits(&out), bits(&fused_show));
                        assert_eq!(bits(&expected_fold), bits(&fused_fold));
                    }
                }
            }
        }
    }
    println!("T20 weighted relative infinity errors [f64, HEAD f32]: {maxima:?}");
    assert!(maxima.iter().all(|&v| v < 1e-5), "{maxima:?}");
}

#[test]
fn siblings_match_separate_calls_cover_live_hands_and_zero_dead_entries() {
    let mut game = game();
    let e = &mut game.game.evaluator;
    let mut pairs = Vec::new();
    let mut fallback = Vec::new();
    for node in &game.game.tree.nodes {
        if node.kind != NodeKind::Action {
            continue;
        }
        let children: Vec<_> = (node.first_child..node.first_child + u32::from(node.num_children))
            .map(|id| game.game.tree.node(id))
            .filter(|n| n.kind == NodeKind::Terminal)
            .map(|n| n.aux)
            .collect();
        if children.len() == 2 {
            let a = &e.terminals[children[0] as usize];
            let b = &e.terminals[children[1] as usize];
            if matches!(a.kind, super::TerminalKind::Fold { .. })
                && matches!(b.kind, super::TerminalKind::Showdown)
                && a.board_mask == b.board_mask
            {
                pairs.push([children[0], children[1]]);
            }
        } else if !children.is_empty() {
            fallback.extend(children);
        }
    }
    assert!(!pairs.is_empty());
    // Different boards and same-kind siblings must also retain separate behavior.
    let other = pairs
        .iter()
        .find(|pair| {
            e.terminals[pair[0] as usize].board_mask != e.terminals[pairs[0][0] as usize].board_mask
        })
        .unwrap();
    let mut cases = pairs.clone();
    cases.extend([
        [pairs[0][0], pairs[0][0]],
        [pairs[0][1], pairs[0][1]],
        [pairs[0][0], other[1]],
    ]);
    let mut maximum = 0.0f64;
    let mut dead = 0;
    for precision in [CfrPrecision::F32, CfrPrecision::F64] {
        e.set_cfr_precision(precision);
        for pair in &cases {
            let board =
                e.terminals[pair[0] as usize].board_mask | e.terminals[pair[1] as usize].board_mask;
            for p in Player::BOTH {
                let show = &e.terminals[pair[1] as usize];
                if matches!(show.kind, super::TerminalKind::Showdown) {
                    let mut covered = vec![false; e.hands.len(p)];
                    for h in &e.rank_tables[show.table as usize][p].hands {
                        assert!(!covered[h.local as usize]);
                        covered[h.local as usize] = true;
                    }
                    for (i, &h) in e.hands.combos(p).iter().enumerate() {
                        let (a, b) = combo_cards(h as usize);
                        assert_eq!(
                            covered[i],
                            show.board_mask & ((1u64 << a.index()) | (1u64 << b.index())) == 0
                        );
                    }
                }
                for pattern in 0..3 {
                    let reach: Vec<f32> = e
                        .hands
                        .combos(p.opponent())
                        .iter()
                        .enumerate()
                        .map(|(i, &h)| {
                            let (a, b) = combo_cards(h as usize);
                            if board & ((1u64 << a.index()) | (1u64 << b.index())) != 0 {
                                0.0
                            } else if pattern == 2 || (pattern == 1 && !i.is_multiple_of(13)) {
                                -0.0
                            } else {
                                ((i * 37 % 101 + 1) as f32) / 103.0
                            }
                        })
                        .collect();
                    let mut expected = [
                        vec![f32::NAN; e.hands.len(p)],
                        vec![f32::NAN; e.hands.len(p)],
                    ];
                    for i in 0..2 {
                        e.eval_cfr(pair[i], p, &reach, &mut expected[i]);
                    }
                    for reversed in [false, true] {
                        let ids = if reversed { [pair[1], pair[0]] } else { *pair };
                        let mut first = vec![f32::NAN; e.hands.len(p)];
                        let mut second = first.clone();
                        e.eval_cfr_siblings(&ids, p, &reach, &mut [&mut first, &mut second]);
                        let actual = if reversed {
                            [second, first]
                        } else {
                            [first, second]
                        };
                        for i in 0..2 {
                            if precision == CfrPrecision::F64 {
                                assert_eq!(bits(&expected[i]), bits(&actual[i]));
                            }
                            maximum = maximum.max(relative_error(&expected[i], &actual[i]));
                            for (h, &value) in e.hands.combos(p).iter().zip(&actual[i]) {
                                let (a, b) = combo_cards(*h as usize);
                                if e.terminals[pair[i] as usize].board_mask
                                    & ((1u64 << a.index()) | (1u64 << b.index()))
                                    != 0
                                {
                                    dead += 1;
                                    assert_eq!(value.to_bits(), 0);
                                }
                            }
                        }
                    }
                }
            }
        }
        // More than two siblings: fuse one pair and evaluate the remainder.
        let ids = [pairs[0][0], pairs[0][1], pairs[0][0]];
        let reach = vec![0.0; e.hands.len(Player::P1)];
        let mut outs = [
            vec![f32::NAN; e.hands.len(Player::P0)],
            vec![f32::NAN; e.hands.len(Player::P0)],
            vec![f32::NAN; e.hands.len(Player::P0)],
        ];
        let mut refs: Vec<_> = outs.iter_mut().map(Vec::as_mut_slice).collect();
        e.eval_cfr_siblings(&ids, Player::P0, &reach, &mut refs);
        for (id, out) in ids.into_iter().zip(outs) {
            let mut exact = vec![f32::NAN; out.len()];
            e.eval_cfr(id, Player::P0, &reach, &mut exact);
            assert_eq!(bits(&out), bits(&exact));
        }
        for id in &fallback {
            let mut out = vec![f32::NAN; e.hands.len(Player::P0)];
            e.eval_cfr_siblings(&[*id], Player::P0, &reach, &mut [&mut out]);
            let mut exact = out.clone();
            e.eval_cfr(*id, Player::P0, &reach, &mut exact);
            assert_eq!(bits(&out), bits(&exact));
        }
    }
    assert!(dead > 0);
    println!(
        "T20 fused vs separate relative infinity error: {maximum:e}; pairs={} dead entries={dead}",
        pairs.len()
    );
    assert!(maximum < 1e-5, "{maximum}");
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
