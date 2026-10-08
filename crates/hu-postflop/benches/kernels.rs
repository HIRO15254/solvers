//! Terminal micro-benchmarks through the real evaluator dispatch: f64 `eval`
//! and f32 `eval_cfr`, on narrow asymmetric and full-support river ranges.
//! Realistic calls and fold/showdown sibling pairs use the same f64-solved
//! reach workload across baseline and optimized executables.
//!
//! Run with `cargo bench -p hu-postflop` (see `docs/development.md`); `cargo bench -p
//! hu-postflop -- --test` runs one iteration per bench as a smoke test.

use std::{collections::BTreeSet, time::Duration};

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use hu_engine::{CfrPrecision, Dcfr, F32Storage, NodeKind, Solver, TerminalEvaluator, reach_at};
use hu_postflop::game::{ChipEv, NoRake, PayoffPipeline};
use hu_postflop::{RiverConfig, RiverGame, build_river_game};
use nlh::{Card, Chips, NUM_COMBOS, PerPlayer, Player, Range};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

fn parse_cards(s: &str) -> Vec<Card> {
    s.split_whitespace().map(|c| c.parse().unwrap()).collect()
}

/// The original narrow, asymmetric single-bet river subgame. One bet size
/// and `max_raises = 1` keep the tree tiny while still producing exactly
/// the two terminal shapes this bench needs (see `terminal_ids`).
fn river_config() -> RiverConfig {
    RiverConfig {
        board: parse_cards("Ks 7h 2d Jc 9s").try_into().unwrap(),
        ranges: PerPlayer::new(
            "22+,A2s+,KTo+".parse::<Range>().unwrap(),
            "55-22,QJs,A5s-A2s,KQo,T9s".parse::<Range>().unwrap(),
        ),
        pot: Chips(20),
        effective_stack: Chips(80),
        bet_fractions: PerPlayer::new(vec![0.75], vec![0.75]),
        max_raises: 1,
    }
}

/// Full support with asymmetric fractional weights: 1,081 live combos per
/// seat after the river board is removed from the 1,326-combo table.
fn wide_river_config() -> RiverConfig {
    let range = |offset| {
        let mut range = Range::full();
        for combo in 0..NUM_COMBOS {
            range.set_weight(combo, ((combo * 17 + offset) % 101 + 1) as f32 / 103.0);
        }
        range
    };
    RiverConfig {
        board: parse_cards("2c 7d 9h Js Qs").try_into().unwrap(),
        ranges: PerPlayer::new(range(3), range(11)),
        ..river_config()
    }
}

/// Locates the fold-terminal and showdown-terminal ids reached after the
/// river's single bet. `max_raises = 1` means the facing player's node
/// (after that one bet) has no further raise available, so its `node_info`
/// entry is exactly the two-action `["fold", "call"]` shape; on the river,
/// `call` resolves straight to a showdown terminal (never a chance node).
/// `PublicTree::compile` lays a node's children out in the builder's action
/// order, so `first_child`/`first_child + 1` are the fold/showdown
/// terminals in that same order -- this is the "distinguish two terminals
/// from outside the crate" trick: `PostflopTerminal::kind` is private, but
/// the tree shape around a bet-with-no-more-raises node isn't.
fn terminal_ids(game: &RiverGame) -> (u32, u32) {
    let info = game
        .node_info
        .iter()
        .find(|info| {
            info.actions.len() == 2 && info.actions[0] == "fold" && info.actions[1] == "call"
        })
        .expect("expected a facing-bet node with exactly fold+call actions");
    let node_id = game
        .node_by_history(&info.history)
        .expect("node_by_history must find the tagged node");
    let node = game.game.tree.node(node_id);
    assert_eq!(
        node.num_children, 2,
        "expected exactly fold + call children"
    );
    let fold_id = game.game.tree.node(node.first_child).aux;
    let showdown_id = game.game.tree.node(node.first_child + 1).aux;
    (fold_id, showdown_id)
}

fn bench_kernels(c: &mut Criterion) {
    bench_case(c, "kernels", river_config());
    bench_case(c, "kernels_wide", wide_river_config());
    bench_realistic(c);
    bench_t25_btn_bb(c);
}

fn bench_case(c: &mut Criterion, name: &str, config: RiverConfig) {
    let mut built = build_river_game(&config, chip_ev());
    built.game.evaluator.set_cfr_precision(CfrPrecision::F32);
    let (fold_id, showdown_id) = terminal_ids(&built);

    // Board-compatible compact reach, as passed by CFR and value traversal.
    let opp_reach = built.game.root_ranges[Player::P1].clone();
    let mut out = vec![0.0f32; built.game.evaluator.hands.len(Player::P0)];

    let mut group = c.benchmark_group(name);

    group.bench_function("fold", |b| {
        b.iter(|| {
            built.game.evaluator.eval(
                black_box(fold_id),
                black_box(Player::P0),
                black_box(&opp_reach),
                &mut out,
            );
            black_box(&out);
        });
    });

    group.bench_function("showdown", |b| {
        b.iter(|| {
            built.game.evaluator.eval(
                black_box(showdown_id),
                black_box(Player::P0),
                black_box(&opp_reach),
                &mut out,
            );
            black_box(&out);
        });
    });

    for (name, terminal) in [("fold_f32", fold_id), ("showdown_f32", showdown_id)] {
        group.bench_function(name, |b| {
            b.iter(|| {
                built.game.evaluator.eval_cfr(
                    black_box(terminal),
                    black_box(Player::P0),
                    black_box(&opp_reach),
                    &mut out,
                );
                black_box(&out);
            });
        });
    }

    // One deterministic permutation gives nested masks with the requested
    // zero counts (rounded to the nearest hand), rather than probabilities.
    let mut order: Vec<usize> = (0..opp_reach.len()).collect();
    let mut state = 0x1234_5678u32;
    for i in (1..order.len()).rev() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        order.swap(i, state as usize % (i + 1));
    }
    for zeros in [0, 50, 80, 95] {
        let mut reach = opp_reach.clone();
        let zero_count = (reach.len() * zeros + 50) / 100;
        for &i in &order[..zero_count] {
            reach[i] = 0.0;
        }
        for (kind, terminal) in [("fold", fold_id), ("showdown", showdown_id)] {
            group.bench_function(format!("t18_{kind}_{zeros:02}"), |b| {
                b.iter(|| {
                    built.game.evaluator.eval_cfr(
                        black_box(terminal),
                        black_box(Player::P0),
                        black_box(&reach),
                        &mut out,
                    );
                    black_box(&out);
                });
            });
        }
    }

    group.finish();
}

fn bench_realistic(c: &mut Criterion) {
    let config = RiverConfig {
        bet_fractions: PerPlayer::new(vec![0.33, 0.75, 1.5], vec![0.33, 0.75, 1.5]),
        max_raises: 3,
        effective_stack: Chips(200),
        ..wide_river_config()
    };
    let built = build_river_game(&config, chip_ev());
    // On the river every terminal except a child labeled "fold" is showdown.
    let mut folds = BTreeSet::new();
    let mut sibling_nodes = Vec::new();
    for info in &built.node_info {
        let id = built.node_by_history(&info.history).unwrap();
        let node = built.game.tree.node(id);
        let terminal = |label: &str| {
            info.actions.iter().position(|s| s == label).and_then(|a| {
                let child = built.game.tree.node(node.first_child + a as u32);
                (child.kind == NodeKind::Terminal).then_some(child.aux)
            })
        };
        if let (Some(fold), Some(showdown)) = (terminal("fold"), terminal("call")) {
            sibling_nodes.push((id, node.player, fold, showdown));
        }
        for (action, label) in info.actions.iter().enumerate() {
            if label == "fold" {
                let child = built.game.tree.node(node.first_child + action as u32);
                assert_eq!(child.kind, NodeKind::Terminal);
                folds.insert(child.aux);
            }
        }
    }
    let mut solver = Solver::<_, F32Storage>::new(built.game, Box::new(Dcfr::default()), Some(400));
    // Freeze collection arithmetic across base/A/A+B.
    solver.set_cfr_precision(CfrPrecision::F64);
    let mut sets = [Vec::new(), Vec::new()];
    let mut siblings = Vec::new();
    let mut opponents = Vec::new();
    let mut t25 = Vec::new();
    for iterations in [300, 50, 50] {
        solver.run(iterations);
        for p in Player::BOTH {
            t25.push(t25_workload(&solver, p));
        }
        let game = solver.game();
        for &(id, p, fold, showdown) in &sibling_nodes {
            let reach = reach_at(
                &game.tree,
                game.root_ranges.as_ref().map(|r| r.as_slice()),
                id,
                |id, _, out| out.copy_from_slice(&solver.current_strategy_at(id)),
            );
            let strategy = solver.current_strategy_at(id);
            let node = game.tree.node(id);
            let child_reach = |terminal| {
                let a = game
                    .tree
                    .children(id)
                    .position(|child| {
                        let child = game.tree.node(child);
                        child.kind == NodeKind::Terminal && child.aux == terminal
                    })
                    .unwrap();
                reach[p]
                    .iter()
                    .zip(&strategy[a * reach[p].len()..(a + 1) * reach[p].len()])
                    .map(|(&r, &s)| r * s)
                    .collect::<Vec<_>>()
            };
            assert_eq!(node.player, p);
            let fold_reach = child_reach(fold);
            let call_reach = child_reach(showdown);
            if fold_reach.iter().any(|&r| r != 0.0) && call_reach.iter().any(|&r| r != 0.0) {
                opponents.push((p.opponent(), fold, showdown, fold_reach, call_reach));
            }
            let opponent = &reach[p.opponent()];
            if opponent.iter().any(|&r| r != 0.0) {
                siblings.push((p, fold, showdown, opponent.clone()));
            }
        }
        for (id, node) in game.tree.nodes.iter().enumerate() {
            if node.kind != NodeKind::Terminal {
                continue;
            }
            let reach = reach_at(
                &game.tree,
                game.root_ranges.as_ref().map(|r| r.as_slice()),
                id as u32,
                |id, _, out| out.copy_from_slice(&solver.current_strategy_at(id)),
            );
            for p in Player::BOTH {
                let opponent = &reach[p.opponent()];
                // Match cfr_pass's all-zero-terminal pruning.
                if opponent.iter().any(|&r| r != 0.0) {
                    let kind = usize::from(!folds.contains(&node.aux));
                    sets[kind].push((node.aux, p, opponent.clone()));
                }
            }
        }
    }
    solver.set_cfr_precision(CfrPrecision::F32);
    bench_t25(c, "wide", &solver, &t25);
    let game = solver.game();
    let mut out = PerPlayer::new(
        vec![0.0; game.evaluator.hands.len(Player::P0)],
        vec![0.0; game.evaluator.hands.len(Player::P1)],
    );
    let mut group = c.benchmark_group("kernels_realistic");
    let hash = siblings
        .iter()
        .flat_map(|(_, _, _, r)| r)
        .fold(0xcbf2_9ce4_8422_2325u64, |h, r| {
            (h ^ u64::from(r.to_bits())).wrapping_mul(0x100_0000_01b3)
        });
    eprintln!(
        "t20 realistic siblings: {} pairs, reach hash {hash:016x}",
        siblings.len()
    );
    let opponent_hash = opponents
        .iter()
        .flat_map(|(_, _, _, f, s)| f.iter().chain(s))
        .fold(0xcbf2_9ce4_8422_2325u64, |h, r| {
            (h ^ u64::from(r.to_bits())).wrapping_mul(0x100_0000_01b3)
        });
    eprintln!(
        "t21 realistic opponents: {} pairs, reach hash {opponent_hash:016x}",
        opponents.len()
    );
    let mut tmp = out.clone();
    for fused in [false, true] {
        group.bench_function(
            if fused {
                "t21_opponent_add"
            } else {
                "t21_opponent_default"
            },
            |b| {
                b.iter(|| {
                    for (p, fold, showdown, fold_reach, call_reach) in &opponents {
                        out[*p].fill(0.0);
                        let terminals = [*fold, *showdown];
                        let reaches = [fold_reach.as_slice(), call_reach.as_slice()];
                        if fused {
                            game.evaluator.add_cfr_opponent_terminals(
                                black_box(&terminals),
                                black_box(*p),
                                black_box(&reaches),
                                &mut out[*p],
                                &mut tmp[*p],
                            );
                        } else {
                            baseline_opponent_add(
                                &game.evaluator,
                                black_box(&terminals),
                                black_box(*p),
                                black_box(&reaches),
                                &mut out[*p],
                                &mut tmp[*p],
                            );
                        }
                        black_box(&out[*p]);
                    }
                });
            },
        );
    }
    let mut fold_out = out.clone();
    for fused in [false, true] {
        group.bench_function(
            if fused {
                "t20_siblings"
            } else {
                "t20_separate"
            },
            |b| {
                b.iter(|| {
                    for (p, fold, showdown, reach) in &siblings {
                        if fused {
                            game.evaluator.eval_cfr_siblings(
                                black_box(&[*fold, *showdown]),
                                black_box(*p),
                                black_box(reach),
                                &mut [&mut fold_out[*p], &mut out[*p]],
                            );
                        } else {
                            game.evaluator.eval_cfr(
                                black_box(*fold),
                                black_box(*p),
                                black_box(reach),
                                &mut fold_out[*p],
                            );
                            game.evaluator.eval_cfr(
                                black_box(*showdown),
                                black_box(*p),
                                black_box(reach),
                                &mut out[*p],
                            );
                        }
                        black_box((&fold_out[*p], &out[*p]));
                    }
                });
            },
        );
    }
    for (kind, set) in ["fold", "showdown"].into_iter().zip(sets) {
        let entries: usize = set.iter().map(|(_, _, r)| r.len()).sum();
        let zeros: usize = set
            .iter()
            .map(|(_, _, r)| r.iter().filter(|&&v| v == 0.0).count())
            .sum();
        // Verify that both executables construct exactly the same workload.
        let hash = set
            .iter()
            .flat_map(|(_, _, r)| r)
            .fold(0xcbf2_9ce4_8422_2325u64, |h, r| {
                (h ^ u64::from(r.to_bits())).wrapping_mul(0x100_0000_01b3)
            });
        eprintln!(
            "t18 realistic {kind}: {} calls, {zeros}/{entries} zeros ({:.3}%), reach hash {hash:016x}",
            set.len(),
            100.0 * zeros as f64 / entries as f64
        );
        group.bench_function(format!("t18_{kind}"), |b| {
            b.iter(|| {
                for (terminal, player, reach) in &set {
                    game.evaluator.eval_cfr(
                        black_box(*terminal),
                        black_box(*player),
                        black_box(reach),
                        &mut out[*player],
                    );
                    black_box(&out[*player]);
                }
            });
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .measurement_time(Duration::from_secs(3));
    targets = bench_kernels
}
criterion_main!(benches);

struct T25Call {
    // Indices of terminal children in child order (after zero-reach pruning).
    indices: Vec<usize>,
    opponent: bool,
}

// Primary T25 workload: the P1 test spot's actual BTN/BB preflop supports,
// with the same betting menu, stack and f64 snapshot schedule as the wide case.
// RiverConfig has no explicit include_allin option; its fractions are clamped
// to the remaining stack by the existing river builder.
fn bench_t25_btn_bb(c: &mut Criterion) {
    let config = RiverConfig {
        board: parse_cards("Ks 7h 2d 3c 9s").try_into().unwrap(),
        ranges: PerPlayer::new(
            "22+,A2s+,K2s+,Q5s+,J7s+,T7s+,97s+,86s+,75s+,65s,54s,A5o+,K9o+,Q9o+,J9o+,T9o".parse().unwrap(),
            "99-22,AJs-A2s,KJs-K2s,Q4s+,J6s+,T6s+,96s+,85s+,74s+,64s+,53s+,43s,AJo-A2o,K8o+,Q9o+,J9o+,T8o+,98o".parse().unwrap(),
        ),
        bet_fractions: PerPlayer::new(vec![0.33, 0.75, 1.5], vec![0.33, 0.75, 1.5]),
        max_raises: 3,
        effective_stack: Chips(200),
        ..river_config()
    };
    let built = build_river_game(&config, chip_ev());
    let mut solver = Solver::<_, F32Storage>::new(built.game, Box::new(Dcfr::default()), Some(400));
    solver.set_cfr_precision(CfrPrecision::F64);
    let mut workloads = Vec::new();
    for iterations in [300, 50, 50] {
        solver.run(iterations);
        for p in Player::BOTH {
            workloads.push(t25_workload(&solver, p));
        }
    }
    solver.set_cfr_precision(CfrPrecision::F32);
    bench_t25(c, "btn_bb", &solver, &workloads);
}
struct T25Workload {
    p: Player,
    terminals: Vec<u32>,
    reaches: Vec<Vec<f32>>,
    calls: Vec<T25Call>,
}

fn t25_workload(
    solver: &Solver<hu_postflop::PostflopEvaluator, F32Storage>,
    p: Player,
) -> T25Workload {
    let game = solver.game();
    let mut workload = T25Workload {
        p,
        terminals: Vec::new(),
        reaches: Vec::new(),
        calls: Vec::new(),
    };
    for (id, node) in game.tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let reach = reach_at(
            &game.tree,
            game.root_ranges.as_ref().map(|r| r.as_slice()),
            id as u32,
            |id, _, out| out.copy_from_slice(&solver.current_strategy_at(id)),
        );
        let strategy = solver.current_strategy_at(id as u32);
        let opponent = node.player != p;
        let mut indices = Vec::new();
        for (a, child) in game.tree.children(id as u32).enumerate() {
            let child = game.tree.node(child);
            if child.kind != NodeKind::Terminal {
                continue;
            }
            let mut r = reach[p.opponent()].clone();
            if opponent {
                let len = r.len();
                for (r, &s) in r.iter_mut().zip(&strategy[a * len..(a + 1) * len]) {
                    *r *= s;
                }
            }
            let index = workload.terminals.len();
            workload.terminals.push(child.aux);
            if r.iter().any(|&r| r != 0.0) {
                indices.push(index);
            }
            workload.reaches.push(r);
        }
        if !indices.is_empty() {
            workload.calls.push(T25Call { indices, opponent });
        }
    }
    assert_eq!(
        workload.terminals.len(),
        game.tree
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Terminal)
            .count()
    );
    workload
}

fn bench_t25(
    c: &mut Criterion,
    label: &str,
    solver: &Solver<hu_postflop::PostflopEvaluator, F32Storage>,
    workloads: &[T25Workload],
) {
    let e = &solver.game().evaluator;
    eprintln!(
        "t25 {label} supports: P0={} P1={}",
        e.hands.len(Player::P0),
        e.hands.len(Player::P1)
    );
    for (snapshot, w) in workloads.iter().enumerate() {
        let batches = w.terminals.len().div_ceil(8);
        let active: usize = w.calls.iter().map(|c| c.indices.len()).sum();
        let hash = w
            .reaches
            .iter()
            .flatten()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, r| {
                (h ^ u64::from(r.to_bits())).wrapping_mul(0x100_0000_01b3)
            });
        eprintln!(
            "t25 {label} snapshot={} player={:?}: terminals={} active={} batches={} lane_fill={:.6} reach_hash={hash:016x}",
            snapshot / 2,
            w.p,
            w.terminals.len(),
            active,
            batches,
            w.terminals.len() as f64 / (batches * 8) as f64
        );
    }
    // Allocate outputs, references and all call metadata before the timer.
    let mut outputs: Vec<Vec<Vec<f32>>> = workloads
        .iter()
        .map(|w| vec![vec![0.0; e.hands.len(w.p)]; w.terminals.len()])
        .collect();
    let mut refs: Vec<Vec<&mut [f32]>> = outputs
        .iter_mut()
        .map(|rows| rows.iter_mut().map(Vec::as_mut_slice).collect())
        .collect();
    let reaches: Vec<Vec<&[f32]>> = workloads
        .iter()
        .map(|w| w.reaches.iter().map(Vec::as_slice).collect())
        .collect();
    let mut node_outputs: Vec<Vec<Vec<f32>>> = workloads
        .iter()
        .map(|w| vec![vec![0.0; e.hands.len(w.p)]; w.calls.len()])
        .collect();
    let mut tmp = vec![0.0; e.hands.len(Player::P0).max(e.hands.len(Player::P1))];
    // Validate the optimized executable on the exact timed snapshot reaches.
    // This also checks width-1 versus full-batch bitwise agreement after LTO.
    let mut max_error = 0.0f64;
    for ((w, outs), rs) in workloads.iter().zip(&mut refs).zip(&reaches) {
        e.eval_cfr_batch(&w.terminals, w.p, rs, outs);
        let mut exact = vec![0.0; e.hands.len(w.p)];
        let mut alone = exact.clone();
        for ((&id, &reach), out) in w.terminals.iter().zip(rs).zip(outs.iter()) {
            e.eval(id, w.p, reach, &mut exact);
            let scale = exact.iter().map(|v| f64::from(v.abs())).fold(0.0, f64::max);
            let error = exact
                .iter()
                .zip(out.iter())
                .map(|(&a, &b)| {
                    assert!(b.is_finite());
                    (f64::from(a) - f64::from(b)).abs()
                })
                .fold(0.0, f64::max);
            if scale == 0.0 {
                assert_eq!(error, 0.0);
            } else {
                max_error = max_error.max(error / scale);
            }
            alone.fill(0.0);
            e.eval_cfr_batch(&[id], w.p, &[reach], &mut [&mut alone]);
            assert!(
                alone
                    .iter()
                    .zip(out.iter())
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
        }
        for out in outs {
            out.fill(0.0);
        }
    }
    assert!(
        max_error < 1e-5,
        "{label} relative max-norm error {max_error}"
    );
    eprintln!(
        "t25 {label} maximum relative infinity error={max_error:e}; width-1/full grouping bitwise equal"
    );
    let mut group = c.benchmark_group("kernels_realistic");
    for mode in ["t25_lone", "t25_current", "t25_batch"] {
        group.bench_function(format!("{mode}_{label}"), |b| {
            b.iter(|| {
                for (((w, outs), reaches), node_outs) in workloads
                    .iter()
                    .zip(&mut refs)
                    .zip(&reaches)
                    .zip(&mut node_outputs)
                {
                    // The engine supplies zeroed terminal rows/node outputs.
                    for out in outs.iter_mut() {
                        out.fill(0.0);
                    }
                    match mode {
                        "t25_lone" => {
                            for ((&id, &reach), out) in
                                w.terminals.iter().zip(reaches).zip(outs.iter_mut())
                            {
                                e.eval_cfr(black_box(id), black_box(w.p), black_box(reach), out);
                            }
                        }
                        "t25_batch" => e.eval_cfr_batch(
                            black_box(&w.terminals),
                            black_box(w.p),
                            black_box(reaches),
                            outs,
                        ),
                        _ => {
                            for (call, node_out) in w.calls.iter().zip(node_outs) {
                                node_out.fill(0.0);
                                for pair in call.indices.chunks(2) {
                                    let a = pair[0];
                                    let mut ids = [w.terminals[a], 0];
                                    let mut rs = [reaches[a], &[]];
                                    if pair.len() == 2 {
                                        ids[1] = w.terminals[pair[1]];
                                        rs[1] = reaches[pair[1]];
                                    }
                                    if call.opponent {
                                        let num_hands = node_out.len();
                                        e.add_cfr_opponent_terminals(
                                            black_box(&ids[..pair.len()]),
                                            black_box(w.p),
                                            black_box(&rs[..pair.len()]),
                                            node_out,
                                            &mut tmp[..num_hands],
                                        );
                                    } else if pair.len() == 2 {
                                        let (before, after) = outs.split_at_mut(pair[1]);
                                        e.eval_cfr_siblings(
                                            black_box(&ids),
                                            black_box(w.p),
                                            black_box(rs[0]),
                                            &mut [&mut *before[a], &mut *after[0]],
                                        );
                                    } else {
                                        e.eval_cfr_siblings(
                                            black_box(&ids[..1]),
                                            black_box(w.p),
                                            black_box(rs[0]),
                                            &mut [&mut *outs[a]],
                                        );
                                    }
                                }
                                black_box(node_out);
                            }
                        }
                    }
                    black_box(outs);
                }
            })
        });
    }
    // Fixed-work estimates, measured separately from the whole-subtree replay.
    // Zero reaches still exercise the complete batch path (no zero skipping).
    let w = &workloads[0];
    let zero = vec![0.0; e.hands.len(w.p.opponent())];
    let zero_rows = vec![zero.clone(); 8];
    let zero_refs: Vec<_> = zero_rows.iter().map(Vec::as_slice).collect();
    group.bench_function(format!("t25_zero_eight_{label}"), |b| {
        b.iter(|| {
            for out in &mut refs[0][..8] {
                out.fill(0.0);
            }
            e.eval_cfr_batch(
                black_box(&w.terminals[..8]),
                black_box(w.p),
                black_box(&zero_refs),
                &mut refs[0][..8],
            );
            black_box(&refs[0][..8]);
        })
    });
    // Use the same showdown terminal for the real and zero one-lane calls.
    // All non-fold terminals on the river are showdowns; locate check-check.
    let terminal = w.terminals[0];
    for (kind, reach) in [("one_lane", reaches[0][0]), ("zero_lane", zero.as_slice())] {
        group.bench_function(format!("t25_{kind}_{label}"), |b| {
            b.iter(|| {
                refs[0][0].fill(0.0);
                e.eval_cfr_batch(
                    black_box(&[terminal]),
                    black_box(w.p),
                    black_box(&[reach]),
                    &mut [&mut *refs[0][0]],
                );
                black_box(&refs[0][0]);
            })
        });
    }
    group.finish();
}

// Baseline of the engine's current opponent-terminal path.
fn baseline_opponent_add<E: TerminalEvaluator>(
    evaluator: &E,
    terminals: &[u32],
    p: Player,
    reaches: &[&[f32]],
    out: &mut [f32],
    tmp: &mut [f32],
) {
    for (&terminal, &reach) in terminals.iter().zip(reaches) {
        tmp.fill(0.0);
        evaluator.eval_cfr(terminal, p, reach, tmp);
        for (dst, &v) in out.iter_mut().zip(tmp.iter()) {
            *dst += v;
        }
    }
}
