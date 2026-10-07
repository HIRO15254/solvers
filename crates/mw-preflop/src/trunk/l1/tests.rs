use super::pass::{Pass, Scratch, class_values, inputs, terminal_values};
use super::*;
use crate::trunk::{
    classes::{Classes, class},
    l0::{
        self, EvaluationOptions, FlopLeaves, Model, Profile, SolveOptions, Tables, Tree,
        eval::reaches,
    },
    tables::HuShowdownTable,
};
use crate::{FeatureHashAbstraction, HoldemGame, Street};
use anyhow::Result;
use nlh::{Card, NUM_COMBOS};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::path::Path;

struct Synthetic;
impl Tables for Synthetic {
    fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
        let k = Classes::get().k(c, d) as f64;
        Ok([k * 0.45, k * 0.1, k * 0.45])
    }
    fn p3(&self, _c: usize, _d: usize, _e: usize) -> Result<[f64; 13]> {
        Ok([1.0 / 13.0; 13])
    }
}

struct Buckets;
impl BucketSource for Buckets {
    fn count(&self, _street: Street) -> usize {
        4
    }
    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS] {
        let dead = board.iter().fold(0_u64, |b, c| b | (1 << c.index()));
        std::array::from_fn(|h| {
            if Classes::get().combo_mask(h) & dead != 0 {
                u16::MAX
            } else {
                (class(h) % 4) as u16
            }
        })
    }
}

fn config(players: usize, checkdown: bool) -> String {
    let mut raw = format!(
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = {players}\n[table.stacks_bb]\nBTN = 3\nBB = 3\n"
    );
    if players == 3 {
        raw.push_str("SB = 3\n");
    }
    raw.push_str("[ranges]\nBTN = 'AA,KK,AKs,72o'\nBB = 'AA,KK,AKs,72o'\n");
    if players == 3 {
        raw.push_str("SB = 'AA,KK,AKs,72o'\n");
    }
    raw.push_str("[tree]\nscript = '''\npreflop { replace raise [a] }\nflop, turn, river { replace bet [a] remove raise }\n");
    if checkdown {
        raw.push_str("flop, turn, river when players >= 2 { checkdown }\n");
    }
    raw.push_str(
        "'''\n[tree.max_aggressive_actions]\npreflop = 1\nflop = 1\nturn = 1\nriver = 1\n",
    );
    raw
}

fn game(raw: &str) -> HoldemGame<FeatureHashAbstraction> {
    l0::game_from_config(raw, Path::new("unit.toml")).unwrap()
}
fn fixed(s: &str) -> [Card; 5] {
    s.split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}
fn close(a: f64, b: f64) {
    assert!(
        (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0),
        "{a} != {b}"
    );
}

fn compare_trees(a: &Tree, b: &Tree) {
    assert_eq!(a.nodes.len(), b.nodes.len());
    for (a, b) in a.nodes.iter().zip(&b.nodes) {
        assert_eq!(a.history, b.history);
        assert_eq!(a.actor, b.actor);
        assert_eq!(a.labels, b.labels);
        assert_eq!(a.children, b.children);
        assert_eq!(a.parent, b.parent);
        if let (Some(a), Some(b)) = (&a.terminal, &b.terminal) {
            assert_eq!(a.active, b.active);
            for (a, b) in a.payoffs.iter().flatten().zip(b.payoffs.iter().flatten()) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
        } else {
            assert_eq!(a.terminal.is_some(), b.terminal.is_some());
        }
    }
}

#[test]
fn checkdown_tree_and_solver_are_bit_identical() {
    let a = game(&config(3, false));
    let b = game(&config(3, true));
    assert!(Tree::build(&a).is_err());
    let a_tree = Tree::build_with(&a, FlopLeaves::Checkdown).unwrap();
    let b_tree = Tree::build(&b).unwrap();
    compare_trees(&a_tree, &b_tree);
    let options = SolveOptions {
        iterations: 4,
        eval_every: 2,
        ..Default::default()
    };
    let am = Model::new(&a, &Synthetic, EvaluationOptions::default()).unwrap();
    let bm = Model::new(&b, &Synthetic, EvaluationOptions::default()).unwrap();
    let asol = l0::solve(&a_tree, &am, options, |_| {}).unwrap();
    let bsol = l0::solve(&b_tree, &bm, options, |_| {}).unwrap();
    for z in 0..a_tree.nodes.len() {
        if a_tree.nodes[z].actor.is_some() {
            for c in 0..169 {
                assert_eq!(
                    asol.average.row(&a_tree, z, c),
                    bsol.average.row(&b_tree, z, c)
                );
            }
        }
    }
    for (a, b) in asol.checkpoints.iter().zip(bsol.checkpoints) {
        assert_eq!(a.nash_conv.to_bits(), b.nash_conv.to_bits());
        assert_eq!(a.seats, b.seats);
    }
}

#[test]
fn b7_checkdown_matches_b3_and_l1_routes_only_active_pairs() {
    let b7 = game(include_str!("../../../../../examples/bench/6max_20bb.toml"));
    let b3 = game(include_str!(
        "../../../../../examples/bench/6max_20bb_checkdown.toml"
    ));
    compare_trees(
        &Tree::build_with(&b7, FlopLeaves::Checkdown).unwrap(),
        &Tree::build(&b3).unwrap(),
    );
    let b6 = game(include_str!(
        "../../../../../examples/bench/hu_20bb_postflop.toml"
    ));
    let tree = Tree::build_with(&b6, FlopLeaves::L1).unwrap();
    assert_eq!(tree.l1_leaf_count(), tree.flop_leaves);
    assert!(tree.l1_leaf_count() > 0);
    let g = game(&config(3, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    assert!(t.flop_leaves > t.l1_leaf_count());
    assert!(t.l1_leaf_count() > 0);
    for terminal in t.nodes.iter().filter_map(|n| n.terminal.as_ref()) {
        if let Some(subtree) = &terminal.l1 {
            assert_eq!(terminal.active, subtree.active);
            for n in &subtree.nodes {
                for u in &n.payoffs {
                    for (s, &utility) in u.iter().enumerate() {
                        if !terminal.active.contains(&s) {
                            assert_eq!(utility.to_bits(), terminal.payoffs[0][s].to_bits());
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "release: B7/B3 full trunk solver profile comparison"]
fn b7_checkdown_solver_matches_b3_bitwise() {
    let b7 = game(include_str!("../../../../../examples/bench/6max_20bb.toml"));
    let b3 = game(include_str!(
        "../../../../../examples/bench/6max_20bb_checkdown.toml"
    ));
    let a = Tree::build_with(&b7, FlopLeaves::Checkdown).unwrap();
    let b = Tree::build(&b3).unwrap();
    let options = EvaluationOptions {
        k4_samples: 8,
        ..Default::default()
    };
    let am = Model::new(&b7, &Synthetic, options).unwrap();
    let bm = Model::new(&b3, &Synthetic, options).unwrap();
    let options = SolveOptions {
        iterations: 2,
        eval_every: 0,
        ..Default::default()
    };
    let aa = l0::solve(&a, &am, options, |_| {}).unwrap();
    let bb = l0::solve(&b, &bm, options, |_| {}).unwrap();
    for (z, n) in a.nodes.iter().enumerate() {
        if n.actor.is_some() {
            for c in 0..169 {
                assert_eq!(aa.average.row(&a, z, c), bb.average.row(&b, z, c));
            }
        }
    }
    for (a, b) in aa.checkpoints.iter().zip(bb.checkpoints) {
        assert_eq!(a.nash_conv.to_bits(), b.nash_conv.to_bits());
        assert_eq!(a.seats, b.seats);
    }
}

/// Independent scalar tree walk: opponent combos are enumerated at each
/// terminal. The maximum is taken after summing opponents, so it does not
/// grant the responder knowledge of the opponent's cards.
#[allow(clippy::too_many_arguments)]
fn reference(
    subtree: &Subtree,
    storage: &LeafStrategy,
    rows: &[f64],
    board: &Board,
    hero: usize,
    h: usize,
    z: usize,
    opponent: &[f64],
    values: &mut [f64],
    best: &mut [f64],
) {
    let n = &subtree.nodes[z];
    let local = usize::from(subtree.active[0] != hero);
    if n.actor.is_none() {
        let mut v = 0.0;
        let mut correction = 0.0;
        for &o in &board.sorted {
            if Classes::get().combo_mask(h) & Classes::get().combo_mask(o) != 0 {
                continue;
            }
            let slot = if n.payoffs.len() == 1 {
                0
            } else {
                let order = board.ranks[h].cmp(&board.ranks[o]);
                match order {
                    std::cmp::Ordering::Equal => 1,
                    std::cmp::Ordering::Greater => {
                        if local == 0 {
                            0
                        } else {
                            2
                        }
                    }
                    std::cmp::Ordering::Less => {
                        if local == 0 {
                            2
                        } else {
                            0
                        }
                    }
                }
            };
            let x = opponent[o] * n.payoffs[slot][hero] - correction;
            let next = v + x;
            correction = (next - v) - x;
            v = next;
        }
        values[z] = v;
        best[z] = v;
        return;
    }
    let row =
        storage.offsets[z] + board.buckets[n.street.index() - 1][h] as usize * n.children.len();
    let mut v = 0.0;
    let mut b = if n.actor == Some(local) {
        f64::NEG_INFINITY
    } else {
        0.0
    };
    for (a, &child) in n.children.iter().enumerate() {
        if n.actor == Some(local) {
            reference(
                subtree, storage, rows, board, hero, h, child, opponent, values, best,
            );
            v += rows[row + a] * values[child];
            b = b.max(best[child]);
        } else {
            let next: Vec<_> = (0..NUM_COMBOS)
                .map(|o| {
                    if board.ranks[o] == 0 {
                        0.0
                    } else {
                        opponent[o]
                            * rows[storage.offsets[z]
                                + board.buckets[n.street.index() - 1][o] as usize
                                    * n.children.len()
                                + a]
                    }
                })
                .collect();
            reference(
                subtree, storage, rows, board, hero, h, child, &next, values, best,
            );
            v += values[child];
            b += best[child];
        }
    }
    values[z] = v;
    best[z] = b;
}

#[test]
fn vector_matches_pairwise_values_responses_and_regrets() {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let mut strategies = Strategies::new(&t, &Buckets);
    let storage = &mut strategies.leaves[0];
    let subtree = t.nodes[storage.terminal]
        .terminal
        .as_ref()
        .unwrap()
        .l1
        .as_ref()
        .unwrap();
    assert!(subtree.nodes.iter().any(|n| n.payoffs.len() == 1));
    assert!(
        subtree
            .nodes
            .iter()
            .any(|n| n.street == Street::River && n.payoffs.len() == 3)
    );
    assert!(
        subtree
            .nodes
            .iter()
            .any(|n| n.street == Street::Flop && n.payoffs.len() == 3)
    );
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    for r in &mut storage.regrets {
        *r = rng.gen_range(0.01..1.0);
    }
    let rows = storage.profile(subtree, false);
    let rho: [f64; 169] = std::array::from_fn(|_| rng.gen_range(0.0..1.0));
    let opponent = std::array::from_fn(|h| rho[class(h)]);
    let own = std::array::from_fn(|h| rho[(class(h) + 7) % 169]);
    let scale = std::array::from_fn(|h| 0.5 + rho[(class(h) + 13) % 169]);
    for cards in [fixed("Ah Kd 2c 3s 7h"), fixed("Ac Kc Qc Jc Tc")] {
        let board = Board::new(cards, &Buckets);
        for hero in subtree.active {
            let mut scratch = Scratch::default();
            let mut inc = vec![0.0; rows.len()];
            let mut sums = inc.clone();
            scratch.pass(
                &Pass {
                    tree: subtree,
                    storage,
                    rows: &rows,
                    board: &board,
                    hero,
                    opponent: &opponent,
                    own: &own,
                    scale: &scale,
                    auxiliary: true,
                },
                Some((&mut inc, &mut sums, 1.0)),
            );
            let mut expected = vec![0.0; rows.len()];
            let mut expected_sums = expected.clone();
            for &h in &board.sorted {
                let mut values = vec![0.0; subtree.nodes.len()];
                let mut best = values.clone();
                reference(
                    subtree,
                    storage,
                    &rows,
                    &board,
                    hero,
                    h,
                    0,
                    &opponent,
                    &mut values,
                    &mut best,
                );
                close(scratch.values[h], values[0]);
                close(scratch.best[h], best[0]);
                let mut own_reach = vec![own[h]; subtree.nodes.len()];
                for (z, n) in subtree.nodes.iter().enumerate() {
                    if let Some((parent, a)) = n.parent {
                        let pn = &subtree.nodes[parent];
                        own_reach[z] = own_reach[parent]
                            * if pn.actor.map(|i| subtree.active[i]) == Some(hero) {
                                rows[storage.offsets[parent]
                                    + board.buckets[pn.street.index() - 1][h] as usize
                                        * pn.children.len()
                                    + a]
                            } else {
                                1.0
                            };
                    }
                    if n.actor.map(|i| subtree.active[i]) != Some(hero) {
                        continue;
                    }
                    let row = storage.offsets[z]
                        + board.buckets[n.street.index() - 1][h] as usize * n.children.len();
                    for (a, &child) in n.children.iter().enumerate() {
                        expected[row + a] += scale[h] * (values[child] - values[z]);
                        expected_sums[row + a] += own_reach[z] * rows[row + a];
                    }
                }
            }
            for (a, b) in inc.iter().zip(expected) {
                close(*a, b);
            }
            for (a, b) in sums.iter().zip(expected_sums) {
                close(*a, b);
            }
        }
    }
}

fn checks(subtree: &Subtree, storage: &LeafStrategy) -> Vec<f64> {
    let mut rows = vec![0.0; storage.regrets.len()];
    for (z, n) in subtree.nodes.iter().enumerate() {
        if n.actor.is_none() {
            continue;
        }
        let a = n.labels.iter().position(|s| s == "check").unwrap_or(0);
        let end = storage.offsets.get(z + 1).copied().unwrap_or(rows.len());
        for row in rows[storage.offsets[z]..end].chunks_mut(n.children.len()) {
            row[a] = 1.0;
        }
    }
    rows
}

#[test]
fn check_only_matches_showdown_and_training_matches_evaluation() {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let p = Profile::uniform(&t);
    let reach = reaches(&t, &p, &m);
    let mut strategies = Strategies::new(&t, &Buckets);
    let storage = &mut strategies.leaves[0];
    let z = storage.terminal;
    let subtree = t.nodes[z].terminal.as_ref().unwrap().l1.as_ref().unwrap();
    let rows = checks(subtree, storage);
    storage.sums.copy_from_slice(&rows);
    let board = Board::new(fixed("Ah Kd 2c 3s 7h"), &Buckets);
    for hero in subtree.active {
        let (opponent, own, scale) = inputs(&t, &m, &reach[z], z, hero);
        let mut scratch = Scratch::default();
        scratch.pass(
            &Pass {
                tree: subtree,
                storage,
                rows: &rows,
                board: &board,
                hero,
                opponent: &opponent,
                own: &own,
                scale: &scale,
                auxiliary: true,
            },
            None,
        );
        let mut direct = vec![0.0; NUM_COMBOS];
        terminal_values(
            &board,
            &opponent,
            &t.nodes[z].terminal.as_ref().unwrap().payoffs,
            hero,
            usize::from(hero != subtree.active[0]),
            &mut direct,
        );
        for (a, b) in scratch.values[..NUM_COMBOS].iter().zip(&direct) {
            close(*a, *b);
        }
        let training = class_values(&scratch.values, &scale);
        let mut inc = vec![0.0; rows.len()];
        let mut sums = inc.clone();
        scratch.pass(
            &Pass {
                tree: subtree,
                storage,
                rows: &rows,
                board: &board,
                hero,
                opponent: &opponent,
                own: &own,
                scale: &scale,
                auxiliary: false,
            },
            Some((&mut inc, &mut sums, 1.0)),
        );
        assert_eq!(training, class_values(&scratch.values, &scale));
    }
}

#[test]
fn evaluator_matches_training_leaf_values_with_folded_seat_masses() {
    use crate::trunk::l0::{
        leaves::{K4Plan, leaf_values},
        solve::backward_values,
    };
    for players in [2, 3] {
        let g = game(&config(players, false));
        let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
        let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
        let mut document = Profile::uniform(&t).export(&t);
        let mut rng = ChaCha8Rng::seed_from_u64(81);
        for node in &mut document.nodes {
            for row in &mut node.probabilities {
                for p in row {
                    *p = rng.gen_range(0.01..1.0);
                }
            }
        }
        let profile = Profile::from_json(&t, &document).unwrap();
        let reach = reaches(&t, &profile, &m);
        let mut strategies = Strategies::new(&t, &Buckets);
        for storage in &mut strategies.leaves {
            for (r, s) in storage.regrets.iter_mut().zip(&mut storage.sums) {
                *r = rng.gen_range(0.01..1.0);
                *s = *r;
            }
        }
        let board = fixed("As 4d 5h 6c 7s");
        let boards = [Board::new(board, &Buckets), Board::new(board, &Buckets)];
        let evaluation = evaluate(&t, &profile, &m, &strategies, &boards, false, false).unwrap();
        let mut leaves = leaf_values(
            &t,
            &m,
            &reach,
            &(0..players).collect::<Vec<_>>(),
            K4Plan::model(&m),
        )
        .unwrap()
        .values;
        for (p, values) in leaves.iter_mut().enumerate() {
            let mut scratch = Scratch::default();
            for storage in &strategies.leaves {
                let z = storage.terminal;
                let subtree = t.nodes[z].terminal.as_ref().unwrap().l1.as_ref().unwrap();
                if !subtree.active.contains(&p) {
                    continue;
                }
                let rows = storage.profile(subtree, false);
                let (opponent, own, scale) = inputs(&t, &m, &reach[z], z, p);
                let mut increments = vec![0.0; rows.len()];
                let mut sums = increments.clone();
                scratch.pass(
                    &Pass {
                        tree: subtree,
                        storage,
                        rows: &rows,
                        board: &boards[0],
                        hero: p,
                        opponent: &opponent,
                        own: &own,
                        scale: &scale,
                        auxiliary: false,
                    },
                    Some((&mut increments, &mut sums, 1.0)),
                );
                values[z] = class_values(&scratch.values, &scale);
            }
            backward_values(&t, &profile, &m.support[p], p, values);
            let value = m.support[p]
                .iter()
                .map(|&c| Classes::get().n(c) as f64 * m.weights[p][c] * values[0][c])
                .sum::<f64>()
                / m.normalizers[p];
            close(value, evaluation.seats[p].value);
            close(evaluation.seats[p].value_a, evaluation.seats[p].value_b);
            close(evaluation.seats[p].gain, evaluation.seats[p].held_gain);
            close(
                evaluation.seats[p].auxiliary_gain,
                evaluation.seats[p].auxiliary_held_gain,
            );
        }
        assert!(l0::evaluate(&t, &profile, &m).is_err());
        assert!(l0::solve(&t, &m, SolveOptions::default(), |_| {}).is_err());
    }
}

#[test]
fn control_variate_with_check_only_postflop_reproduces_l0_checkdown() {
    for players in [2, 3] {
        let g = game(&config(players, false));
        let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
        let checkdown = Tree::build_with(&g, FlopLeaves::Checkdown).unwrap();
        let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
        let mut document = Profile::uniform(&t).export(&t);
        let mut rng = ChaCha8Rng::seed_from_u64(82);
        for node in &mut document.nodes {
            for row in &mut node.probabilities {
                for p in row {
                    *p = rng.gen_range(0.01..1.0);
                }
            }
        }
        let profile = Profile::from_json(&t, &document).unwrap();
        let mut strategies = Strategies::new(&t, &Buckets);
        for storage in &mut strategies.leaves {
            let subtree = t.nodes[storage.terminal]
                .terminal
                .as_ref()
                .unwrap()
                .l1
                .as_ref()
                .unwrap();
            storage.sums = checks(subtree, storage);
        }
        // Synthetic T2 differs from any board's showdown: only the control
        // variate's exact baseline can reproduce the L0 checkdown model.
        let boards = [
            Board::new(fixed("As 4d 5h 6c 7s"), &Buckets),
            Board::new(fixed("Ah Kd 2c 3s 7h"), &Buckets),
        ];
        let expected = l0::evaluate(
            &checkdown,
            &Profile::from_json(&checkdown, &document).unwrap(),
            &m,
        )
        .unwrap();
        let observed = evaluate(&t, &profile, &m, &strategies, &boards, true, false).unwrap();
        for (o, e) in observed.seats.iter().zip(&expected.seats) {
            close(o.value, e.value);
            close(o.value_a, e.value);
            close(o.value_b, e.value);
            close(o.gain, e.gain);
            close(o.held_gain, e.gain);
        }
        close(observed.nash_conv, expected.nash_conv);
        let plain = evaluate(&t, &profile, &m, &strategies, &boards, false, false).unwrap();
        // With check-only play L1 equals the checkdown on every board, so any
        // fitted coefficient leaves the exact baseline.
        let fitted = evaluate(&t, &profile, &m, &strategies, &boards, true, true).unwrap();
        close(fitted.nash_conv, expected.nash_conv);
        assert!((plain.nash_conv - expected.nash_conv).abs() > 1e-6);
    }
}

#[test]
#[ignore = "release: enumerate all canonical river boards with multiplicities"]
fn check_only_all_boards_equals_l0_t2() {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let strategies = Strategies::new(&t, &Buckets);
    let storage = &strategies.leaves[0];
    let terminal = t.nodes[storage.terminal].terminal.as_ref().unwrap();
    let subtree = terminal.l1.as_ref().unwrap();
    let mut z = 0;
    while subtree.nodes[z].actor.is_some() {
        let n = &subtree.nodes[z];
        z = n.children[n.labels.iter().position(|s| s == "check").unwrap()];
    }
    assert_eq!(subtree.nodes[z].payoffs, terminal.payoffs);
    let boards = crate::card_abstraction::buckets::canonical_river_sets();
    let classes = [0, 14]; // AA and KK
    let t2 = HuShowdownTable::build_for_boards(&classes, &boards).unwrap();
    let opponent: [f64; NUM_COMBOS] =
        std::array::from_fn(|h| if class(h) == 14 { 0.7 } else { 0.0 });
    let result: Vec<[f64; 169]> = {
        use rayon::prelude::*;
        boards
            .par_iter()
            .map(|&(cards, multiplicity)| {
                let board = Board::new(cards, &Buckets);
                let mut v = [0.0; NUM_COMBOS];
                terminal_values(
                    &board,
                    &opponent,
                    &subtree.nodes[z].payoffs,
                    subtree.active[0],
                    0,
                    &mut v,
                );
                let mut classes = class_values(&v, &[1.0 / super::cards::Q; NUM_COMBOS]);
                for c in &mut classes {
                    *c *= multiplicity as f64 / 2_598_960.0;
                }
                classes
            })
            .collect()
    };
    for c in classes {
        let observed: f64 = result.iter().map(|v| v[c]).sum();
        let counts = t2.counts(c, 14).unwrap();
        let expected: f64 = (0..3)
            .map(|o| {
                terminal.payoffs[o][subtree.active[0]] * counts[o] as f64 * 0.7
                    / (Classes::get().n(c) as f64 * 1_712_304.0)
            })
            .sum();
        assert!(
            (observed - expected).abs() <= 1e-10 * (1.0 + expected.abs()),
            "class {c}: {observed} != {expected}"
        );
    }
}

#[test]
fn solve_is_deterministic_across_threads_and_seed_changes_results() {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let run = |threads, seed| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                solve(
                    &t,
                    &m,
                    &Buckets,
                    Options {
                        trunk: SolveOptions {
                            iterations: 5,
                            eval_every: 5,
                            ..Default::default()
                        },
                        // Several board chunks per leaf in training and evaluation.
                        l1_boards: 20,
                        l1_eval_boards: 20,
                        l1_seed: seed,
                        l1_train_control: true,
                        l1_train_regression: true,
                        l1_postflop_beta: 0.5,
                        l1_eval_control: true,
                        l1_eval_regression: true,
                        l1_sampling: Sampling::Stratified,
                        ..Default::default()
                    },
                    |_| {},
                )
                .unwrap()
            })
    };
    let a = run(1, 0);
    let b = run(4, 0);
    for z in 0..t.nodes.len() {
        if t.nodes[z].actor.is_some() {
            for c in 0..169 {
                assert_eq!(a.average.row(&t, z, c), b.average.row(&t, z, c));
            }
        }
    }
    for (a, b) in a.postflop.leaves.iter().zip(&b.postflop.leaves) {
        assert_eq!(a.regrets, b.regrets);
        assert_eq!(a.sums, b.sums);
    }
    for (a, b) in a.checkpoints.iter().zip(&b.checkpoints) {
        assert_eq!(a.evaluation.seats, b.evaluation.seats);
        assert_eq!(
            a.evaluation.nash_conv.to_bits(),
            b.evaluation.nash_conv.to_bits()
        );
    }
    let c = run(1, 1);
    assert!(
        a.postflop
            .leaves
            .iter()
            .zip(c.postflop.leaves)
            .any(|(a, c)| a.regrets != c.regrets)
    );
    let invalid = [0, 1, 3]
        .map(|count| Options {
            l1_eval_boards: count,
            ..Default::default()
        })
        .into_iter()
        .chain([
            Options {
                l1_train_control: false,
                ..Default::default()
            },
            Options {
                l1_eval_control: false,
                ..Default::default()
            },
        ]);
    for options in invalid {
        assert!(
            solve(&t, &m, &Buckets, options, |_| {}).is_err(),
            "{options:?}"
        );
    }
}

#[test]
fn primary_nash_conv_convergence_smoke() {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let solution = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            solve(
                &t,
                &m,
                &Buckets,
                Options {
                    trunk: SolveOptions {
                        iterations: 100,
                        eval_every: 100,
                        ..Default::default()
                    },
                    // The plain estimator: random boards' L1 values, no control variates.
                    l1_eval_boards: 16,
                    l1_boards: 2,
                    l1_train_control: false,
                    l1_train_regression: false,
                    l1_eval_control: false,
                    l1_eval_regression: false,
                    l1_sampling: Sampling::Random,
                    ..Default::default()
                },
                |_| {},
            )
            .unwrap()
        });
    let initial = solution.checkpoints[0].evaluation.nash_conv;
    let end = solution.checkpoints.last().unwrap().evaluation.nash_conv;
    eprintln!("L1 smoke: {initial:.12} -> {end:.12}");
    assert!(end < initial * 0.25, "{initial} -> {end}");
}

#[test]
fn default_settings_solve_converges() {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let defaults = Options::default();
    assert_eq!(defaults.trunk.beta, 1.0);
    let solution = solve(
        &t,
        &m,
        &Buckets,
        Options {
            trunk: SolveOptions {
                iterations: 100,
                eval_every: 100,
                ..defaults.trunk
            },
            // Fewer boards than the default keep the test short.
            l1_boards: 8,
            l1_eval_boards: 16,
            ..defaults
        },
        |_| {},
    )
    .unwrap();
    let initial = solution.checkpoints[0].evaluation.nash_conv;
    let end = solution.checkpoints.last().unwrap().evaluation.nash_conv;
    eprintln!("L1 default-settings smoke: {initial:.12} -> {end:.12}");
    assert!(end < initial * 0.25, "{initial} -> {end}");
}

#[test]
fn stratified_boards_cover_turns_and_rivers_uniformly() {
    use super::cards::{board_at, boards};
    use std::collections::BTreeSet;
    // A grid over one flop's interval deals every (turn, river) exactly once.
    for flop in [0, 1, 777, 1754] {
        let ends: Vec<u32> = nlh::iso::canonical_flops()
            .iter()
            .scan(0, |end, (_, w)| {
                *end += w;
                Some(*end)
            })
            .collect();
        let start = if flop == 0 { 0 } else { ends[flop - 1] };
        let width = f64::from(ends[flop] - start);
        let mut seen = BTreeSet::new();
        for j in 0..49 * 48 {
            let x = f64::from(start) + (j as f64 + 0.5) / (49.0 * 48.0) * width;
            let b = board_at(x);
            let mask = b.iter().fold(0_u64, |m, c| m | (1 << c.index()));
            assert_eq!(mask.count_ones(), 5);
            seen.insert((b[3].index(), b[4].index()));
        }
        assert_eq!(seen.len(), 49 * 48);
    }
    // Unit spacing from any offset puts exactly each flop's multiplicity of
    // points in its interval: the paired-flop share is exact.
    let exact = 22_100.0 - 52.0 * 48.0 * 44.0 / 6.0;
    for offset in [0.0, 0.25, 0.999] {
        let paired = (0..22_100)
            .filter(|&k| {
                let b = board_at(offset + f64::from(k));
                let r: Vec<_> = b[..3].iter().map(|c| c.rank()).collect();
                r[0] == r[1] || r[1] == r[2] || r[0] == r[2]
            })
            .count() as f64;
        assert_eq!(paired, exact);
    }
    let a = boards(&Buckets, b"test", 3, Some(1), 8, Sampling::Stratified);
    let b = boards(&Buckets, b"test", 3, Some(2), 8, Sampling::Stratified);
    assert_ne!(a[0].cards, b[0].cards);
}

#[test]
fn regression_control_matches_per_board_reference() {
    use super::cards::boards;
    use crate::trunk::l0::{
        leaves::{K4Plan, leaf_values},
        solve::backward_values,
    };
    for players in [2, 3] {
        let g = game(&config(players, false));
        let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
        let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
        let mut document = Profile::uniform(&t).export(&t);
        let mut rng = ChaCha8Rng::seed_from_u64(83);
        for node in &mut document.nodes {
            for row in &mut node.probabilities {
                for p in row {
                    *p = rng.gen_range(0.01..1.0);
                }
            }
        }
        let profile = Profile::from_json(&t, &document).unwrap();
        let reach = reaches(&t, &profile, &m);
        let mut strategies = Strategies::new(&t, &Buckets);
        for storage in &mut strategies.leaves {
            for s in &mut storage.sums {
                *s = rng.gen_range(0.01..1.0);
            }
        }
        let sample = boards(&Buckets, b"regression", 5, None, 12, Sampling::Random);
        let evaluation = evaluate(&t, &profile, &m, &strategies, &sample, true, true).unwrap();
        let unit = evaluate(&t, &profile, &m, &strategies, &sample, true, false).unwrap();
        assert!(
            evaluation
                .seats
                .iter()
                .zip(&unit.seats)
                .any(|(a, b)| (a.value_a - b.value_a).abs() > 1e-9)
        );
        let base = leaf_values(
            &t,
            &m,
            &reach,
            &(0..players).collect::<Vec<_>>(),
            K4Plan::model(&m),
        )
        .unwrap()
        .values;
        for (p, base) in base.into_iter().enumerate() {
            let mut halves = [base.clone(), base];
            let mut scratch = Scratch::default();
            for storage in &strategies.leaves {
                let z = storage.terminal;
                let terminal = t.nodes[z].terminal.as_ref().unwrap();
                let subtree = terminal.l1.as_ref().unwrap();
                if !subtree.active.contains(&p) {
                    continue;
                }
                let rows = storage.profile(subtree, true);
                let (opponent, own, scale) = inputs(&t, &m, &reach[z], z, p);
                let mut values = [Vec::new(), Vec::new()];
                for (j, board) in sample.iter().enumerate() {
                    scratch.pass(
                        &Pass {
                            tree: subtree,
                            storage,
                            rows: &rows,
                            board,
                            hero: p,
                            opponent: &opponent,
                            own: &own,
                            scale: &scale,
                            auxiliary: false,
                        },
                        None,
                    );
                    let s = class_values(&scratch.values, &scale);
                    let x = scratch.checkdown(board, terminal, p, &opponent, &scale);
                    values[j % 2].push((s, x));
                }
                for (h, half) in values.iter().enumerate() {
                    let n = half.len() as f64;
                    for c in 0..169 {
                        let t2 = halves[h][z][c];
                        let ms = half.iter().map(|(s, _)| s[c]).sum::<f64>() / n;
                        let mx = half.iter().map(|(_, x)| x[c]).sum::<f64>() / n;
                        let var = half.iter().map(|(_, x)| (x[c] - mx).powi(2)).sum::<f64>() / n;
                        let cov = half
                            .iter()
                            .map(|(s, x)| (s[c] - ms) * (x[c] - mx))
                            .sum::<f64>()
                            / n;
                        let second = half.iter().map(|(_, x)| x[c] * x[c]).sum::<f64>() / n;
                        halves[h][z][c] = if var > 1e-12 * second {
                            ms - cov / var * (mx - t2)
                        } else {
                            t2 + ms - mx
                        };
                    }
                }
            }
            for (h, values) in halves.iter_mut().enumerate() {
                backward_values(&t, &profile, &m.support[p], p, values);
                let value = m.support[p]
                    .iter()
                    .map(|&c| Classes::get().n(c) as f64 * m.weights[p][c] * values[0][c])
                    .sum::<f64>()
                    / m.normalizers[p];
                let observed = if h == 0 {
                    evaluation.seats[p].value_a
                } else {
                    evaluation.seats[p].value_b
                };
                assert!(
                    (value - observed).abs() <= 1e-9 * value.abs().max(1.0),
                    "seat {p} half {h}: {value} != {observed}"
                );
            }
        }
    }
}
