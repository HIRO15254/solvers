use super::*;
use crate::kernel;
use crate::postflop::{PostflopEvaluator, TerminalKind};

struct DefaultAdd<'a>(&'a PostflopEvaluator);
impl TerminalEvaluator for DefaultAdd<'_> {
    fn eval(&self, id: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        self.0.eval(id, p, reach, out);
    }
    fn eval_cfr(&self, id: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        self.0.eval_cfr(id, p, reach, out);
    }
}
fn initial(dim: usize) -> Vec<f32> {
    (0..dim)
        .map(|i| {
            if i % 3 == 0 {
                -0.0
            } else {
                (i % 19) as f32 * 0.125 - 1.0
            }
        })
        .collect()
}
fn reach(e: &PostflopEvaluator, p: Player, board: u64, pattern: usize, offset: usize) -> Vec<f32> {
    e.hands
        .combos(p.opponent())
        .iter()
        .enumerate()
        .map(|(i, &h)| {
            let (a, b) = combo_cards(h as usize);
            if board & ((1u64 << a.index()) | (1u64 << b.index())) != 0 {
                0.0
            } else if pattern == 2 || (pattern == 1 && !(i + offset).is_multiple_of(13)) {
                -0.0
            } else {
                ((i * 37 + offset * 11) % 101 + 1) as f32 / 103.0
            }
        })
        .collect()
}
fn assert_dead_unchanged(
    e: &PostflopEvaluator,
    p: Player,
    board: u64,
    before: &[f32],
    after: &[f32],
) -> usize {
    let mut dead = 0;
    for (i, &h) in e.hands.combos(p).iter().enumerate() {
        let (a, b) = combo_cards(h as usize);
        if board & ((1u64 << a.index()) | (1u64 << b.index())) != 0 {
            assert_eq!(before[i].to_bits(), after[i].to_bits());
            dead += 1;
        }
    }
    dead
}

#[test]
fn opponent_add_dispatch_matches_default_and_preserves_dead_hands() {
    let mut g = game();
    let e = &mut g.game.evaluator;
    let mut pairs = Vec::new();
    let mut singles = Vec::new();
    for node in &g.game.tree.nodes {
        if node.kind != NodeKind::Action {
            continue;
        }
        let ids: Vec<_> = (node.first_child..node.first_child + u32::from(node.num_children))
            .map(|id| g.game.tree.node(id))
            .filter(|n| n.kind == NodeKind::Terminal)
            .map(|n| n.aux)
            .collect();
        if ids.len() == 2
            && matches!(e.terminals[ids[0] as usize].kind, TerminalKind::Fold { .. })
            && matches!(e.terminals[ids[1] as usize].kind, TerminalKind::Showdown)
        {
            pairs.push(ids);
        } else {
            singles.extend(ids.into_iter().map(|id| vec![id]));
        }
    }
    assert!(!pairs.is_empty() && !singles.is_empty());
    let other = pairs
        .iter()
        .find(|ids| {
            e.terminals[ids[0] as usize].board_mask != e.terminals[pairs[0][0] as usize].board_mask
        })
        .unwrap();
    let mut cases = pairs.clone();
    cases.extend(pairs.iter().map(|ids| vec![ids[1], ids[0]]));
    cases.extend(singles);
    cases.extend([
        vec![pairs[0][0], pairs[0][0]],
        vec![pairs[0][1], pairs[0][1]],
        vec![pairs[0][0], other[1]],
        vec![pairs[0][0], pairs[0][1], pairs[0][0]],
        vec![pairs[0][0], pairs[0][0], pairs[0][1]],
        vec![],
    ]);
    let mut maximum = 0.0f64;
    let mut dead = 0;
    for precision in [CfrPrecision::F32, CfrPrecision::F64] {
        e.set_cfr_precision(precision);
        for ids in &cases {
            let union = ids
                .iter()
                .fold(0, |mask, &id| mask | e.terminals[id as usize].board_mask);
            let common = ids.iter().fold(u64::MAX, |mask, &id| {
                mask & e.terminals[id as usize].board_mask
            });
            for p in Player::BOTH {
                for pattern in 0..5 {
                    // Include only-fold-zero and only-call-zero pairs, too.
                    let reaches: Vec<_> = ids
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            reach(
                                e,
                                p,
                                union,
                                if pattern >= 3 {
                                    if i == pattern - 3 { 2 } else { 0 }
                                } else {
                                    pattern
                                },
                                i + 1,
                            )
                        })
                        .collect();
                    let refs: Vec<_> = reaches.iter().map(Vec::as_slice).collect();
                    let before = initial(e.hands.len(p));
                    let mut expected = before.clone();
                    let mut actual = before.clone();
                    let mut tmp = vec![f32::NAN; before.len()];
                    DefaultAdd(e).add_cfr_opponent_terminals(
                        ids,
                        p,
                        &refs,
                        &mut expected,
                        &mut tmp,
                    );
                    tmp.fill(f32::NAN);
                    e.add_cfr_opponent_terminals(ids, p, &refs, &mut actual, &mut tmp);
                    maximum = maximum.max(relative_error(&expected, &actual));
                    if precision == CfrPrecision::F64 {
                        assert_eq!(bits(&expected), bits(&actual));
                    } else {
                        assert!(tmp.iter().all(|v| v.is_nan()));
                        if !ids.is_empty() {
                            dead += assert_dead_unchanged(e, p, common, &before, &actual);
                        }
                    }
                }
            }
        }
    }
    assert!(dead > 0);
    println!(
        "T21 dispatch relative infinity error: {maximum:e}; pairs={} dead checks={dead}",
        pairs.len()
    );
    assert!(maximum < 1e-5, "{maximum}");
}

#[test]
fn opponent_add_real_tables_utilities_and_rank_coverage() {
    let g = game();
    let e = &g.game.evaluator;
    let mut maximum = 0.0f64;
    let mut exact_maximum = 0.0f64;
    let mut dead = 0;
    for (index, table) in e.rank_tables.iter().enumerate() {
        let board = e
            .terminals
            .iter()
            .find(|t| t.table as usize == index)
            .unwrap()
            .board_mask;
        for p in Player::BOTH {
            // This is the opponent list used for the fold sums, independently
            // checked against board compatibility of every support entry.
            let mut covered = vec![false; e.hands.len(p.opponent())];
            for h in &table[p.opponent()].hands {
                assert!(!covered[h.local as usize]);
                covered[h.local as usize] = true;
            }
            for (i, &h) in e.hands.combos(p.opponent()).iter().enumerate() {
                let (a, b) = combo_cards(h as usize);
                assert_eq!(
                    covered[i],
                    board & ((1u64 << a.index()) | (1u64 << b.index())) == 0
                );
            }
            for pattern in 0..5 {
                let call = reach(
                    e,
                    p,
                    board,
                    if pattern == 3 { 2 } else { pattern.min(2) },
                    1,
                );
                let fold = reach(
                    e,
                    p,
                    board,
                    if pattern == 4 { 2 } else { pattern.min(2) },
                    7,
                );
                let full = kernel::compat_sums_relaxed_f32(&e.fold_combos[p.opponent()], &fold);
                let ranked = kernel::compat_sums_relaxed_f32(&table[p.opponent()].hands, &fold);
                assert!(relative_error(&[full.0], &[ranked.0]) < 1e-5);
                assert!(relative_error(&full.1, &ranked.1) < 1e-5);
                for utilities in [
                    [1.25, -0.125, -1.75],
                    [0.0; 3],
                    [-2.0, 1.0, 0.5],
                    [0.0, 0.25, 0.0],
                ] {
                    for u_fold in [-1.5, 0.0, 0.75] {
                        let before = initial(e.hands.len(p));
                        let mut show = vec![0.0; before.len()];
                        let mut f = show.clone();
                        kernel::showdown_kernel_relaxed_f32(
                            &table[p],
                            &table[p.opponent()],
                            &e.hands.same[p],
                            utilities,
                            &call,
                            &mut show,
                        );
                        kernel::fold_kernel_relaxed_f32(
                            &e.fold_combos[p],
                            &e.fold_combos[p.opponent()],
                            &e.hands.same[p],
                            u_fold,
                            board,
                            &fold,
                            &mut f,
                        );
                        // Separate add kernels must reproduce write-then-add on
                        // live hands exactly, including a nonzero accumulator.
                        for (is_fold, values) in [(false, &show), (true, &f)] {
                            let mut added = before.clone();
                            if is_fold {
                                kernel::add_fold_kernel_relaxed_f32(
                                    &e.fold_combos[p],
                                    &e.fold_combos[p.opponent()],
                                    &e.hands.same[p],
                                    u_fold,
                                    board,
                                    &fold,
                                    &mut added,
                                );
                            } else {
                                kernel::add_showdown_kernel_relaxed_f32(
                                    &table[p],
                                    &table[p.opponent()],
                                    &e.hands.same[p],
                                    utilities,
                                    &call,
                                    &mut added,
                                );
                            }
                            for h in &table[p].hands {
                                let i = h.local as usize;
                                assert_eq!((before[i] + values[i]).to_bits(), added[i].to_bits());
                            }
                            dead += assert_dead_unchanged(e, p, board, &before, &added);
                        }
                        let expected: Vec<_> = before
                            .iter()
                            .zip(&f)
                            .zip(&show)
                            .map(|((&v, &f), &s)| (v + f) + s)
                            .collect();
                        let mut fused = before.clone();
                        kernel::add_showdown_fold_kernel_relaxed_f32(
                            &table[p],
                            &table[p.opponent()],
                            &e.hands.same[p],
                            [utilities[0], utilities[1], utilities[2], u_fold],
                            &call,
                            &fold,
                            &mut fused,
                        );
                        maximum = maximum.max(relative_error(&expected, &fused));
                        dead += assert_dead_unchanged(e, p, board, &before, &fused);
                        kernel::showdown_kernel(
                            &table[p],
                            &table[p.opponent()],
                            &e.hands.same[p],
                            utilities,
                            &call,
                            &mut show,
                        );
                        kernel::fold_kernel(
                            &e.fold_combos[p],
                            &e.fold_combos[p.opponent()],
                            &e.hands.same[p],
                            u_fold,
                            board,
                            &fold,
                            &mut f,
                        );
                        let exact: Vec<_> = before
                            .iter()
                            .zip(&f)
                            .zip(&show)
                            .map(|((&v, &f), &s)| (v + f) + s)
                            .collect();
                        exact_maximum = exact_maximum.max(relative_error(&exact, &fused));
                    }
                }
            }
        }
    }
    assert!(dead > 0);
    println!(
        "T21 fused relative infinity error [default f32, f64]: [{maximum:e}, {exact_maximum:e}]; dead checks={dead}"
    );
    assert!(maximum < 1e-5 && exact_maximum < 1e-5);
}
