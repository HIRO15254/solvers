use super::pass::{Mapping, Pass, Scratch, class_values, inputs, terminal_values};
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
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = {players}\n[table.stacks]\nBTN = 3\nBB = 3\n"
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
    raw.push_str("preflop when aggressions >= 1 { remove raise }\n'''\n");
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
/// The vector pass sums the opponent's masses plainly and subtracts the
/// blocked ones, so its values carry rounding of order 1e-13 of those masses,
/// which the pairwise reference (a compensated sum per hand) does not.
fn near(a: f64, b: f64) {
    assert!(
        (a - b).abs() <= 1e-10 * a.abs().max(b.abs()).max(1.0),
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
    let mut scratch = Scratch::default();
    for cards in [fixed("Ah Kd 2c 3s 7h"), fixed("Ac Kc Qc Jc Tc")] {
        let board = Board::new(cards, &Buckets);
        for hero in subtree.active {
            // Reuse across heroes/boards, poisoning prior values so a stale
            // read cannot hide behind the previous pass's plausible results.
            scratch.values.fill(f64::NAN);
            scratch.best.fill(f64::NAN);
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
                    mapping: None,
                    own_reach_regret: false,
                },
                Some((&mut inc, &mut sums, 1.0)),
            );
            for h in 0..NUM_COMBOS {
                if board.ranks[h] == 0 {
                    assert_eq!(scratch.values[h].to_bits(), 0.0_f64.to_bits());
                    assert_eq!(scratch.best[h].to_bits(), 0.0_f64.to_bits());
                }
            }
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
                near(scratch.values[h], values[0]);
                near(scratch.best[h], best[0]);
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
                near(*a, b);
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
                mapping: None,
                own_reach_regret: false,
            },
            None,
        );
        let mut direct = vec![0.0; NUM_COMBOS];
        terminal_values(
            &board,
            &opponent[..],
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
                mapping: None,
                own_reach_regret: false,
            },
            Some((&mut inc, &mut sums, 1.0)),
        );
        assert_eq!(training, class_values(&scratch.values, &scale));
    }
}

#[test]
fn own_reach_regret_weights_bucket_members_by_their_own_reach() {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let reach = reaches(&t, &Profile::uniform(&t), &m);
    let strategies = Strategies::new(&t, &Buckets);
    let storage = &strategies.leaves[0];
    let z = storage.terminal;
    let subtree = t.nodes[z].terminal.as_ref().unwrap().l1.as_ref().unwrap();
    // Uniform rows keep every member of a bucket at the same own reach below the root.
    let rows = storage.profile(subtree, false);
    let board = Board::new(fixed("Ah Kd 2c 3s 7h"), &Buckets);
    let hero = subtree.active[0];
    let (opponent, own, scale) = inputs(&t, &m, &reach[z], z, hero);
    let increments = |own: &[f64; NUM_COMBOS], scale: &[f64; NUM_COMBOS], weighted: bool| {
        let mut inc = vec![0.0; rows.len()];
        let mut sums = inc.clone();
        Scratch::default().pass(
            &Pass {
                tree: subtree,
                storage,
                rows: &rows,
                board: &board,
                hero,
                opponent: &opponent,
                own,
                scale,
                auxiliary: false,
                mapping: None,
                own_reach_regret: weighted,
            },
            Some((&mut inc, &mut sums, 1.0)),
        );
        inc
    };
    let uniform = Box::new([0.25; NUM_COMBOS]);
    assert_eq!(
        increments(&uniform, &scale, true),
        increments(&uniform, &scale, false)
    );
    // Only one class reaches the leaf: its bucket learns from it alone, scaled
    // to the bucket's live combos, and the other buckets keep plain regret.
    let reached = (0..169)
        .find(|&c| own[Classes::get().representative(c)] > 0.0)
        .unwrap();
    let mut single = Box::new([0.0; NUM_COMBOS]);
    let mut only = Box::new([0.0; NUM_COMBOS]);
    for h in 0..NUM_COMBOS {
        if class(h) == reached {
            single[h] = own[h];
            only[h] = scale[h];
        }
    }
    let weighted = increments(&single, &scale, true);
    let plain = increments(&single, &scale, false);
    let members = increments(&single, &only, false);
    let mut checked = 0;
    for (node, n) in subtree.nodes.iter().enumerate() {
        if n.actor != Some(0) {
            continue;
        }
        let k = n.children.len();
        let map = &board.buckets[n.street.index() - 1];
        for b in 0..Buckets.count(n.street) {
            let live = board.sorted.iter().filter(|&&h| map[h] as usize == b);
            let count = live.clone().count() as f64;
            let reaching = live.filter(|&&h| class(h) == reached).count() as f64;
            let row = storage.offsets[node] + b * k;
            for a in row..row + k {
                if reaching > 0.0 {
                    close(weighted[a], members[a] * count / reaching);
                    checked += 1;
                } else {
                    assert_eq!(weighted[a], plain[a]);
                }
            }
        }
    }
    assert!(checked > 0);
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
                        mapping: None,
                        own_reach_regret: false,
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

fn override_fixture(tree: &Tree) -> (Profile, Strategies, Vec<Board>) {
    let mut document = Profile::uniform(tree).export(tree);
    let mut rng = ChaCha8Rng::seed_from_u64(81);
    for node in &mut document.nodes {
        for row in &mut node.probabilities {
            for p in row {
                *p = rng.gen_range(0.01..1.0);
            }
        }
    }
    let profile = Profile::from_json(tree, &document).unwrap();
    let mut strategies = Strategies::new(tree, &Buckets);
    for storage in &mut strategies.leaves {
        for (r, s) in storage.regrets.iter_mut().zip(&mut storage.sums) {
            *r = rng.gen_range(0.01..1.0);
            *s = *r;
        }
    }
    let boards = ["As 4d 5h 6c 7s", "Ah Kd 2c 3s 7h"]
        .map(|board| Board::new(fixed(board), &Buckets))
        .into_iter()
        .collect();
    (profile, strategies, boards)
}

#[test]
fn override_with_own_class_values_reproduces_evaluation() {
    for players in [2, 3] {
        let g = game(&config(players, false));
        let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
        let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
        let (profile, strategies, boards) = override_fixture(&t);
        let expected = evaluate(&t, &profile, &m, &strategies, &boards, false, false).unwrap();
        let mut overrides = Vec::new();
        for storage in &strategies.leaves {
            let terminal = storage.terminal;
            for &seat in &t.nodes[terminal].terminal.as_ref().unwrap().active {
                let d = leaf_diagnostic(&t, &profile, &m, &strategies, &boards, terminal, seat)
                    .unwrap();
                overrides.push(LeafOverride {
                    terminal,
                    seat,
                    values: std::array::from_fn(|c| {
                        (d.mass[c] > 0.0).then(|| d.profile[c] / d.mass[c])
                    }),
                });
            }
        }
        let actual = evaluate_with_overrides(
            &t,
            &profile,
            &m,
            &strategies,
            &boards,
            false,
            false,
            &overrides,
        )
        .unwrap();
        for (a, e) in actual.seats.iter().zip(&expected.seats) {
            assert!((a.value - e.value).abs() < 1e-9);
            assert!((a.best_response - e.best_response).abs() < 1e-9);
            assert!((a.gain - e.gain).abs() < 1e-9);
        }
        assert!((actual.nash_conv - expected.nash_conv).abs() < 1e-9);
    }
}

#[test]
fn override_is_linear_in_leaf_values() {
    let g = game(&config(3, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let (profile, strategies, boards) = override_fixture(&t);
    let terminal = strategies.leaves[0].terminal;
    let active = &t.nodes[terminal].terminal.as_ref().unwrap().active;
    let seat = active[0];
    let d = leaf_diagnostic(&t, &profile, &m, &strategies, &boards, terminal, seat).unwrap();
    let values = std::array::from_fn(|c| (d.mass[c] > 0.0).then(|| d.profile[c] / d.mass[c]));
    let value = |values| {
        evaluate_with_overrides(
            &t,
            &profile,
            &m,
            &strategies,
            &boards,
            true,
            true,
            &[LeafOverride {
                terminal,
                seat,
                values,
            }],
        )
        .unwrap()
        .seats[seat]
            .value
    };
    let e = value(values);
    let shifted = value(values.map(|v| v.map(|v| v + 0.5)));
    let one = value([Some(1.0); 169]);
    let zero = value([Some(0.0); 169]);
    assert!(((shifted - e) - 0.5 * (one - zero)).abs() < 1e-9);
    assert!(one - zero > 0.0);
    let inactive = (0..t.seats).find(|p| !active.contains(p)).unwrap();
    assert!(
        evaluate_with_overrides(
            &t,
            &profile,
            &m,
            &strategies,
            &boards,
            true,
            true,
            &[LeafOverride {
                terminal,
                seat: inactive,
                values
            }],
        )
        .is_err()
    );
}

fn train_override_fixture(tree: &Tree) -> (usize, usize, LeafOverride) {
    for (parent, node) in tree.nodes.iter().enumerate() {
        if let Some(seat) = node.actor {
            for (action, &terminal) in node.children.iter().enumerate() {
                if tree.nodes[terminal]
                    .terminal
                    .as_ref()
                    .is_some_and(|t| t.l1.is_some() && t.active.contains(&seat))
                {
                    return (
                        parent,
                        action,
                        LeafOverride {
                            terminal,
                            seat,
                            values: [Some(50.0); 169],
                        },
                    );
                }
            }
        }
    }
    panic!("fixture has no direct L1 decision");
}

#[test]
fn train_override_steers_the_trunk_toward_the_overridden_leaf() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let model = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let (parent, action, replacement) = train_override_fixture(&tree);
    let seat = replacement.seat;
    let solution = solve(
        &tree,
        &model,
        &Buckets,
        Options {
            trunk: SolveOptions {
                iterations: 100,
                eval_every: 0,
                ..Default::default()
            },
            l1_boards: 2,
            l1_train_overrides: &[replacement],
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let reach = reaches(&tree, &solution.average, &model);
    let mut checked = 0;
    for &c in &model.support[seat] {
        if reach[parent].pi(seat, c) > 0.0 {
            assert!(solution.average.row(&tree, parent, c)[action] >= 0.95);
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn train_override_checkpoint_matches_evaluation_with_the_same_override() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let model = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let (_, _, replacement) = train_override_fixture(&tree);
    let overrides = [replacement];
    let options = Options {
        trunk: SolveOptions {
            iterations: 2,
            eval_every: 2,
            ..Default::default()
        },
        l1_boards: 2,
        l1_eval_boards: 4,
        l1_train_overrides: &overrides,
        ..Default::default()
    };
    let solution = solve(&tree, &model, &Buckets, options, |_| {}).unwrap();
    let boards = evaluation_boards(&Buckets, &options);
    let evaluation = evaluate_with_overrides(
        &tree,
        &solution.average,
        &model,
        &solution.postflop,
        &boards,
        options.l1_eval_control,
        options.l1_eval_regression,
        &overrides,
    )
    .unwrap();
    assert!(
        (solution.checkpoints.last().unwrap().evaluation.nash_conv - evaluation.nash_conv).abs()
            < 1e-9
    );
}

#[test]
fn train_override_is_part_of_the_state_identity() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let model = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let (_, _, replacement) = train_override_fixture(&tree);
    let overrides = [replacement];
    let options = Options {
        trunk: SolveOptions {
            iterations: 1,
            eval_every: 0,
            ..Default::default()
        },
        l1_boards: 2,
        l1_train_overrides: &overrides,
        ..Default::default()
    };
    let mut bytes = Vec::new();
    solve(&tree, &model, &Buckets, options, |p| {
        if p.finished {
            p.write_state(&mut bytes).unwrap();
        }
    })
    .unwrap();
    let mut changed = overrides.clone();
    changed[0].values[0] = Some(51.0);
    let changed_options = Options {
        l1_train_overrides: &changed,
        ..options
    };
    assert!(
        read_state(
            &tree,
            &model,
            &Buckets,
            changed_options,
            &mut bytes.as_slice()
        )
        .is_err()
    );
    let state = read_state(&tree, &model, &Buckets, options, &mut bytes.as_slice()).unwrap();
    assert!(
        solve_with_state(
            &tree,
            &model,
            &Buckets,
            changed_options,
            Some(state),
            |_| Ok(())
        )
        .is_err()
    );
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
fn weighted_flop_subset_sampling_matches_weights_and_covers_runouts() {
    use super::cards::subset_boards_into;
    let subset = WeightedFlopSubset::parse("Ks 7h 2d 2\nAh Ac 3d 5\n8c 8d 8h 3").unwrap();
    let flops = [
        fixed("Ks 7h 2d 4c 5c")[..3].try_into().unwrap(),
        fixed("Ah Ac 3d 4c 5c")[..3].try_into().unwrap(),
        fixed("8c 8d 8h 4c 5c")[..3].try_into().unwrap(),
    ]
    .map(canonical_flop);
    let mut boards = Vec::new();
    subset_boards_into(&Buckets, &subset, 7, 13, 10, &mut boards);
    let mut counts = [0; 3];
    for board in &boards {
        let flop = canonical_flop(board.cards[..3].try_into().unwrap());
        let i = flops.iter().position(|&f| f == flop).unwrap();
        counts[i] += 1;
        let mask = board.cards.iter().fold(0_u64, |m, c| m | (1 << c.index()));
        assert_eq!(mask.count_ones(), 5);
    }
    assert_eq!(counts, [2, 5, 3]);
    // Nonintegral expected frequencies stay within a single sample of the
    // exact weights; no full Board preparation is needed for this larger grid.
    for offset in [0.0, 0.37, 0.999] {
        let mut counts = [0; 3];
        for k in 0..97 {
            let board = subset.board_at((offset + f64::from(k)) / 97.0 * 10.0);
            let flop = canonical_flop(board[..3].try_into().unwrap());
            counts[flops.iter().position(|&f| f == flop).unwrap()] += 1;
        }
        for (count, weight) in counts.into_iter().zip([2.0, 5.0, 3.0]) {
            assert!((f64::from(count) - 97.0 * weight / 10.0).abs() < 1.0);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for k in 0..49 * 48 {
        let board = subset.board_at((f64::from(k) + 0.5) / (49.0 * 48.0) * 2.0);
        seen.insert((board[3].index(), board[4].index()));
    }
    assert_eq!(seen.len(), 49 * 48);
    let options = Options {
        l1_eval_boards: 4,
        l1_flop_subset: Some(&subset),
        ..Options::default()
    };
    let subset_eval = evaluation_boards(&Buckets, &options);
    let full_eval = evaluation_boards(
        &Buckets,
        &Options {
            l1_flop_subset: None,
            ..options
        },
    );
    for (a, b) in subset_eval.iter().zip(full_eval) {
        assert_eq!(a.cards, b.cards);
    }
}

#[test]
fn flop_subset_spr_routing_preserves_standard_boards() {
    use super::solve::TrainingBoards;
    let subset = WeightedFlopSubset::parse("Ks 7h 2d 2\nAh Ac 3d 5").unwrap();
    let options = Options {
        l1_boards: 8,
        l1_seed: 17,
        l1_flop_subset: Some(&subset),
        l1_flop_subset_max_spr: Some(2.5),
        ..Options::default()
    };
    for sampling in [Sampling::Random, Sampling::Stratified] {
        let options = Options {
            l1_sampling: sampling,
            ..options
        };
        let mut routed = TrainingBoards::new(options, [2.5, 2.500001].into_iter());
        routed.prepare(&Buckets, options, None, 13);
        let plain = Options {
            l1_flop_subset: None,
            l1_flop_subset_max_spr: None,
            ..options
        };
        let mut standard = TrainingBoards::new(plain, [2.500001].into_iter());
        standard.prepare(&Buckets, plain, None, 13);
        assert_eq!(routed.for_spr(2.500001).len(), 8);
        for (a, b) in routed
            .for_spr(2.500001)
            .iter()
            .zip(standard.for_spr(2.500001))
        {
            assert_eq!(a.cards, b.cards);
        }
        let flops = ["Ks 7h 2d 4c 5c", "Ah Ac 3d 4c 5c"]
            .map(|s| canonical_flop(fixed(s)[..3].try_into().unwrap()));
        assert_eq!(routed.for_spr(2.5).len(), 8);
        for board in routed.for_spr(2.5) {
            assert!(flops.contains(&canonical_flop(board.cards[..3].try_into().unwrap())));
        }
        // Fixed-profile weighted flops use the identical routing and runouts.
        let fixed = FixedFlops::parse("Ks 7h 2d 2\nAh Ac 3d 5").unwrap();
        let fixed_options = Options {
            l1_flop_subset: None,
            ..options
        };
        let mut fixed_boards = TrainingBoards::new(fixed_options, [2.5, 2.500001].into_iter());
        fixed_boards.prepare(&Buckets, fixed_options, Some(&fixed), 13);
        for spr in [2.5, 2.500001] {
            for (a, b) in routed.for_spr(spr).iter().zip(fixed_boards.for_spr(spr)) {
                assert_eq!(a.cards, b.cards);
            }
        }
        let mut high_only = TrainingBoards::new(options, [2.500001].into_iter());
        high_only.prepare(&Buckets, options, None, 13);
        assert!(high_only.for_spr(2.5).is_empty());
        let mut low_only = TrainingBoards::new(options, [2.5].into_iter());
        low_only.prepare(&Buckets, options, None, 13);
        assert!(low_only.for_spr(2.500001).is_empty());
        let legacy = Options {
            l1_flop_subset_max_spr: None,
            ..options
        };
        let mut all_subset = TrainingBoards::new(legacy, [2.5, 100.0].into_iter());
        all_subset.prepare(&Buckets, legacy, None, 13);
        for (a, b) in all_subset.for_spr(100.0).iter().zip(routed.for_spr(2.5)) {
            assert_eq!(a.cards, b.cards);
        }
    }
}

#[test]
fn weighted_flop_subset_parsing_rejects_invalid_inputs() {
    for text in [
        "",
        "Ks 7h 2d",
        "Ks 7h 2d 1 extra",
        "Ks 7h 2d nope",
        "invalid 7h 2d 1",
        "Ks Ks 2d 1",
        "Ks 7h 2d 0",
        "Ks 7h 2d -1",
        "Ks 7h 2d NaN",
        "Ks 7h 2d inf",
        "Ks 7h 2d 1\n2d Ks 7h 2",
        "Ks 7h 2d 1\nKh 7s 2c 2",
    ] {
        assert!(WeightedFlopSubset::parse(text).is_err(), "{text}");
    }
    let a = WeightedFlopSubset::parse("Ks 7h 2d 2").unwrap();
    let b = WeightedFlopSubset::parse("2d Ks 7h 2.0").unwrap();
    assert_eq!(a.state_identity(), b.state_identity());
    assert!(matches!(
        FixedFlops::parse("Ks 7h 2d\nAh Ac 3d").unwrap(),
        FixedFlops::RoundRobin(_)
    ));
    assert!(matches!(
        FixedFlops::parse("Ks 7h 2d 2\nAh Ac 3d 5").unwrap(),
        FixedFlops::Weighted(_)
    ));
    for text in ["Ks 7h 2d 2\nAh Ac 3d", "Ks 7h 2d\nAh Ac 3d 5"] {
        assert!(
            FixedFlops::parse(text)
                .unwrap_err()
                .to_string()
                .contains("mix weighted and unweighted")
        );
    }
}

#[test]
fn weighted_flop_subset_state_identity_rejects_mismatch() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let model = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let storage = Strategies::new(&tree, &Buckets);
    let subset = WeightedFlopSubset::parse("Ks 7h 2d 2\nAh Ac 3d 5").unwrap();
    let changed = WeightedFlopSubset::parse("Ks 7h 2d 3\nAh Ac 3d 5").unwrap();
    let other_flop = WeightedFlopSubset::parse("Ks 7h 3d 2\nAh Ac 3d 5").unwrap();
    let options = Options {
        l1_flop_subset: Some(&subset),
        ..Options::default()
    };
    let identity = super::state::identity(&tree, &model, &Buckets, options, &storage).unwrap();
    let threshold_identity = super::state::identity(
        &tree,
        &model,
        &Buckets,
        Options {
            l1_flop_subset_max_spr: Some(2.5),
            ..options
        },
        &storage,
    )
    .unwrap();
    assert_ne!(identity, threshold_identity);
    let other_threshold_identity = super::state::identity(
        &tree,
        &model,
        &Buckets,
        Options {
            l1_flop_subset_max_spr: Some(3.0),
            ..options
        },
        &storage,
    )
    .unwrap();
    assert_ne!(threshold_identity, other_threshold_identity);
    // Loading checks identity before arrays. A real-format state header is
    // enough to exercise rejection without training or writing large arrays.
    let mut header = Vec::new();
    header.extend_from_slice(b"P2L1STA1");
    header.extend_from_slice(&1_u32.to_le_bytes());
    header.extend_from_slice(&tree.game_fingerprint);
    header.extend_from_slice(&identity);
    for l1_flop_subset in [None, Some(&changed), Some(&other_flop)] {
        let changed_options = Options {
            l1_flop_subset,
            ..options
        };
        let error = read_state(&tree, &model, &Buckets, changed_options, &mut &header[..])
            .err()
            .unwrap();
        assert!(error.to_string().contains("identity mismatch"));
    }
    let full_identity =
        super::state::identity(&tree, &model, &Buckets, Options::default(), &storage).unwrap();
    header[44..76].copy_from_slice(&full_identity);
    assert!(
        read_state(&tree, &model, &Buckets, options, &mut &header[..])
            .err()
            .unwrap()
            .to_string()
            .contains("identity mismatch")
    );
}

#[test]
fn weighted_flop_subset_requires_one_representative_per_group() {
    let mut text = String::new();
    for (board, _) in nlh::iso::canonical_flops() {
        let flop = &board.flop;
        let ace = flop.iter().any(|c| c.rank() == 12);
        text.push_str(&format!(
            "{} {} {} {}\n",
            flop[0],
            flop[1],
            flop[2],
            u8::from(ace)
        ));
    }
    let groups = FlopClusters::parse(&text).unwrap();
    WeightedFlopSubset::parse("Ks 7h 2d 2\nAh Ac 3d 5")
        .unwrap()
        .validate_groups(&groups)
        .unwrap();
    assert!(
        WeightedFlopSubset::parse("Ks 7h 2d 2")
            .unwrap()
            .validate_groups(&groups)
            .unwrap_err()
            .to_string()
            .contains("no representative of group 1")
    );
    assert!(
        WeightedFlopSubset::parse("Ks 7h 2d 2\nQc Jh 3d 1\nAh Ac 3d 5")
            .unwrap()
            .validate_groups(&groups)
            .unwrap_err()
            .to_string()
            .contains("multiple representatives of group 0")
    );
}

#[test]
fn stratified_evaluation_halves_have_separate_shifts() {
    use super::cards::boards;
    let options = Options {
        l1_eval_boards: 8,
        l1_eval_seed: 4,
        l1_eval_sampling: Sampling::Stratified,
        ..Options::default()
    };
    let all = super::solve::evaluation_boards(&Buckets, &options);
    assert_eq!(all.len(), 8);
    for (parity, domain) in [
        (0, b"solvers.p2.trunk.l1.eval.v1.even".as_slice()),
        (1, b"solvers.p2.trunk.l1.eval.v1.odd"),
    ] {
        let half = boards(&Buckets, domain, 4, None, 4, Sampling::Stratified);
        for (k, board) in half.iter().enumerate() {
            assert_eq!(all[2 * k + parity].cards, board.cards);
        }
    }
    let random = Options {
        l1_eval_sampling: Sampling::Random,
        ..options
    };
    let expected = boards(
        &Buckets,
        b"solvers.p2.trunk.l1.eval.v1",
        4,
        None,
        8,
        Sampling::Random,
    );
    let actual = super::solve::evaluation_boards(&Buckets, &random);
    assert!(
        actual
            .iter()
            .zip(&expected)
            .all(|(a, b)| a.cards == b.cards)
    );
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
                            mapping: None,
                            own_reach_regret: false,
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

#[test]
fn average_round_trip_is_bit_identical_and_evaluates_identically() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let mut original = Strategies::new(&tree, &Buckets);
    for (i, value) in original
        .leaves
        .iter_mut()
        .flat_map(|l| &mut l.sums)
        .enumerate()
    {
        *value = (i + 1) as f64 / 7.0;
    }
    let mut bytes = Vec::new();
    original.write_average(&tree, &mut bytes).unwrap();
    assert_eq!(
        bytes.len(),
        56 + original.leaves.len() * 16 + original.slots * 8 + 32
    );
    let mut loaded = Strategies::new(&tree, &Buckets);
    for leaf in &mut loaded.leaves {
        leaf.regrets.fill(123.0);
    }
    loaded.read_average(&tree, &mut bytes.as_slice()).unwrap();
    for (a, b) in original.leaves.iter().zip(&loaded.leaves) {
        assert_eq!(
            a.sums.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            b.sums.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        assert!(b.regrets.iter().all(|&v| v == 123.0));
    }
    let model = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let profile = Profile::uniform(&tree);
    let boards = evaluation_boards(
        &Buckets,
        &Options {
            l1_eval_boards: 4,
            ..Default::default()
        },
    );
    let a = evaluate(&tree, &profile, &model, &original, &boards, true, true).unwrap();
    let b = evaluate(&tree, &profile, &model, &loaded, &boards, true, true).unwrap();
    assert_eq!(a.seats, b.seats);
    for (a, b) in [
        a.nash_conv,
        a.held_nash_conv,
        a.auxiliary_nash_conv,
        a.auxiliary_held_nash_conv,
    ]
    .into_iter()
    .zip([
        b.nash_conv,
        b.held_nash_conv,
        b.auxiliary_nash_conv,
        b.auxiliary_held_nash_conv,
    ]) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
    // The format must preserve arbitrary f64 bits too, without numeric validation.
    let bits = [0_u64, 1 << 63, 0x7ff8_0000_0000_0042, 0x7ff0_0000_0000_0000];
    for (i, v) in original
        .leaves
        .iter_mut()
        .flat_map(|l| &mut l.sums)
        .enumerate()
    {
        *v = f64::from_bits(bits[i % bits.len()]);
    }
    bytes.clear();
    original.write_average(&tree, &mut bytes).unwrap();
    loaded.read_average(&tree, &mut bytes.as_slice()).unwrap();
    for (a, b) in original
        .leaves
        .iter()
        .flat_map(|l| &l.sums)
        .zip(loaded.leaves.iter().flat_map(|l| &l.sums))
    {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn leaf_average_reads_one_leaf_of_full_and_kept_leaf_averages() {
    let g = game(&config(3, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let mut full = Strategies::new(&tree, &Buckets);
    assert!(full.leaves.len() >= 2);
    for (i, value) in full.leaves.iter_mut().flat_map(|l| &mut l.sums).enumerate() {
        *value = (i + 1) as f64 / 7.0;
    }
    let z = full.leaves[1].terminal;
    let mut kept = Strategies::new_for(&tree, &Buckets, Some(&[z]));
    assert!(
        kept.leaves
            .iter()
            .all(|l| (l.terminal == z) != l.sums.is_empty())
    );
    kept.leaves[1].sums.clone_from(&full.leaves[1].sums);
    assert_eq!(kept.slots, kept.leaves[1].sums.len());
    let bits = |s: &Strategies| {
        s.leaves[1]
            .sums
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    };
    for saved in [&full, &kept] {
        let mut bytes = Vec::new();
        saved.write_average(&tree, &mut bytes).unwrap();
        let mut loaded = Strategies::new_for(&tree, &Buckets, Some(&[z]));
        loaded
            .read_average_leaf(&tree, z, &mut bytes.as_slice())
            .unwrap();
        assert_eq!(bits(&loaded), bits(&full));
    }
    // A whole-tree reader rejects an average without the other leaves.
    let mut bytes = Vec::new();
    kept.write_average(&tree, &mut bytes).unwrap();
    let error = Strategies::new(&tree, &Buckets)
        .read_average(&tree, &mut bytes.as_slice())
        .unwrap_err();
    assert!(error.to_string().contains("slots mismatch"), "{error}");
}

#[test]
fn average_rejects_corruption_truncation_trailing_bytes_and_wrong_tree() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let mut storage = Strategies::new(&tree, &Buckets);
    for leaf in &mut storage.leaves {
        leaf.sums.fill(7.0);
        leaf.regrets.fill(11.0);
    }
    let mut bytes = Vec::new();
    storage.write_average(&tree, &mut bytes).unwrap();
    let payload = 56 + storage.leaves.len() * 16;
    for (offset, message) in [
        (0, "magic"),
        (8, "fingerprint"),
        (40, "leaf count"),
        (48, "slots"),
        (56, "terminal"),
        (64, "sums length"),
        (payload, "hash"),
    ] {
        let mut bad = bytes.clone();
        bad[offset] ^= 1;
        let error = storage
            .read_average(&tree, &mut bad.as_slice())
            .unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        assert!(
            storage
                .leaves
                .iter()
                .flat_map(|l| &l.sums)
                .all(|&v| v == 7.0)
        );
    }
    for len in [0, 7, 55, payload + 1, bytes.len() - 1] {
        assert!(storage.read_average(&tree, &mut &bytes[..len]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(
        storage
            .read_average(&tree, &mut trailing.as_slice())
            .unwrap_err()
            .to_string()
            .contains("trailing")
    );
    let other_game = game(&config(2, false).replace("BTN = 3", "BTN = 4"));
    let other = Tree::build_with(&other_game, FlopLeaves::L1).unwrap();
    let mut other_storage = Strategies::new(&other, &Buckets);
    assert!(
        other_storage
            .read_average(&other, &mut bytes.as_slice())
            .unwrap_err()
            .to_string()
            .contains("fingerprint")
    );
    assert!(
        storage
            .leaves
            .iter()
            .flat_map(|l| &l.sums)
            .all(|&v| v == 7.0)
    );
    assert!(
        storage
            .leaves
            .iter()
            .flat_map(|l| &l.regrets)
            .all(|&v| v == 11.0)
    );
}

#[test]
fn rebind_stamps_the_tree_fingerprint_and_keeps_validation() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let mut storage = Strategies::new(&tree, &Buckets);
    for (i, v) in storage
        .leaves
        .iter_mut()
        .flat_map(|l| &mut l.sums)
        .enumerate()
    {
        *v = i as f64 * 0.25;
    }
    let mut current = Vec::new();
    storage.write_average(&tree, &mut current).unwrap();
    // The same file as written under another fingerprint scheme.
    let mut old = current.clone();
    let body = old.len() - 32;
    old[8..40].fill(0xab);
    let checksum = *blake3::hash(&old[..body]).as_bytes();
    old[body..].copy_from_slice(&checksum);
    let mut loaded = Strategies::new(&tree, &Buckets);
    assert!(
        loaded
            .read_average(&tree, &mut old.as_slice())
            .unwrap_err()
            .to_string()
            .contains("fingerprint")
    );
    let mut rebound = Vec::new();
    let replaced = loaded
        .rebind_average(&tree, &mut old.as_slice(), &mut rebound)
        .unwrap();
    assert_eq!(replaced, [0xab; 32]);
    assert_eq!(rebound, current);
    loaded.read_average(&tree, &mut rebound.as_slice()).unwrap();
    for (a, b) in storage
        .leaves
        .iter()
        .flat_map(|l| &l.sums)
        .zip(loaded.leaves.iter().flat_map(|l| &l.sums))
    {
        assert_eq!(a.to_bits(), b.to_bits());
    }
    let payload = 56 + storage.leaves.len() * 16;
    for (offset, message) in [
        (0, "magic"),
        (40, "leaf count"),
        (48, "slots"),
        (56, "terminal"),
        (64, "sums length"),
        (payload, "hash"),
        (body, "hash"),
    ] {
        let mut bad = old.clone();
        bad[offset] ^= 1;
        let error = loaded
            .rebind_average(&tree, &mut bad.as_slice(), &mut Vec::new())
            .unwrap_err();
        assert!(error.to_string().contains(message), "{offset}: {error}");
    }
    for len in [0, 55, payload + 1, old.len() - 1] {
        assert!(
            loaded
                .rebind_average(&tree, &mut &old[..len], &mut Vec::new())
                .is_err()
        );
    }
    let mut trailing = old.clone();
    trailing.push(0);
    assert!(
        loaded
            .rebind_average(&tree, &mut trailing.as_slice(), &mut Vec::new())
            .unwrap_err()
            .to_string()
            .contains("trailing")
    );
}

#[test]
fn relative_buckets_are_reach_percentiles_of_strength_groups_crossed_with_class() {
    use super::cards::Strength;
    let mut class = vec![0; NUM_COMBOS];
    class[3] = 1;
    // Combos 0..4 in ascending strength; 2 and 3 tie.
    let strength = Strength {
        count: 2,
        classes: 2,
        prior: 0.0,
        prior_fraction: 0.0,
        class,
        order: vec![0, 1, 2, 3],
        groups: vec![(0, 1), (1, 2), (2, 4)],
        low_spr: None,
        variants: Vec::new(),
        opponent: None,
    };
    let mut out = [u16::MAX; NUM_COMBOS];
    let mut reach = [0.0; NUM_COMBOS];
    reach[..4].copy_from_slice(&[0.5, 0.0, 0.25, 0.25]);
    super::pass::relative_buckets(&strength, &reach, &mut out);
    // Group mid-points 0.25, 0.5 and 0.75 of the reach; a tie shares its
    // percentile bucket and keeps its own class.
    assert_eq!(out[..4], [0, 2, 2, 3]);
    assert_eq!(out[4], u16::MAX);
    // Without reach, by position among the live combos: 0.125, 0.375, 0.75.
    super::pass::relative_buckets(&strength, &[0.0; NUM_COMBOS], &mut out);
    assert_eq!(out[..4], [0, 0, 2, 3]);
}

#[test]
fn relative_prior_regularizes_thin_ranges_and_preserves_group_classes() {
    use super::cards::Strength;
    let mut class = vec![0; NUM_COMBOS];
    class[3] = 1;
    let mut strength = Strength {
        count: 2,
        classes: 2,
        prior: 0.0,
        prior_fraction: 0.0,
        class,
        order: vec![0, 1, 2, 3],
        groups: vec![(0, 1), (1, 2), (2, 4)],
        low_spr: None,
        variants: Vec::new(),
        opponent: None,
    };
    let mut reach = [0.0; NUM_COMBOS];
    let mut fallback = [u16::MAX; NUM_COMBOS];
    super::pass::relative_buckets(&strength, &reach, &mut fallback);
    let mut out = fallback;
    strength.prior = 1.0;
    super::pass::relative_buckets(&strength, &reach, &mut out);
    assert_eq!(out, fallback);

    reach[3] = 1e-9;
    strength.prior = 0.0;
    super::pass::relative_buckets(&strength, &reach, &mut out);
    assert_eq!(out[..4], [0, 0, 2, 3]);
    // Three distinct strengths make the collapse of the weaker combos visible.
    strength.count = 3;
    super::pass::relative_buckets(&strength, &reach, &mut out);
    assert_eq!(out[..4], [0, 0, 2, 3]);
    super::pass::relative_buckets(&strength, &[0.0; NUM_COMBOS], &mut fallback);
    assert_eq!(fallback[..4], [0, 2, 4, 5]);
    strength.prior = 1.0;
    super::pass::relative_buckets(&strength, &reach, &mut out);
    assert_eq!(out, fallback);
    super::pass::relative_buckets(&strength, &[0.0; NUM_COMBOS], &mut out);
    assert_eq!(out, fallback);

    // Away from bucket boundaries a large reach dominates the prior.
    reach[..4].copy_from_slice(&[0.6e9, 0.0, 0.2e9, 0.2e9]);
    strength.prior = 0.0;
    let mut unregularized = [u16::MAX; NUM_COMBOS];
    super::pass::relative_buckets(&strength, &reach, &mut unregularized);
    strength.prior = 1.0;
    super::pass::relative_buckets(&strength, &reach, &mut out);
    assert_eq!(out, unregularized);

    // Even an arbitrarily small prior can move a zero-reach combo off an
    // exact boundary: the second group's midpoint falls just below 0.5.
    strength.count = 2;
    reach[..4].copy_from_slice(&[0.5e9, 0.0, 0.25e9, 0.25e9]);
    strength.prior = 0.0;
    super::pass::relative_buckets(&strength, &reach, &mut unregularized);
    assert_eq!(unregularized[..4], [0, 2, 2, 3]);
    strength.prior = 1.0;
    super::pass::relative_buckets(&strength, &reach, &mut out);
    assert_eq!(out[..4], [0, 0, 2, 3]);
}

#[test]
fn relative_prior_fraction_scales_with_the_reach_total() {
    use super::cards::Strength;
    let mut strength = Strength {
        count: 4,
        classes: 1,
        prior: 0.0,
        prior_fraction: 1.0,
        class: vec![0; NUM_COMBOS],
        order: vec![0, 1, 2, 3],
        groups: vec![(0, 1), (1, 2), (2, 3), (3, 4)],
        low_spr: None,
        variants: Vec::new(),
        opponent: None,
    };
    let buckets = |strength: &Strength, scale: f64| {
        let mut reach = [0.0; NUM_COMBOS];
        reach[3] = scale;
        let mut out = [u16::MAX; NUM_COMBOS];
        super::pass::relative_buckets(strength, &reach, &mut out);
        out[..4].to_vec()
    };
    // Mass s/4 per combo plus s on the strongest: midpoints 1/16, 3/16, 5/16, 11/16.
    assert_eq!(buckets(&strength, 1.0), [0, 0, 1, 2]);
    assert_eq!(buckets(&strength, 1e6), [0, 0, 1, 2]);
    // The same mass in combos only matches at the scale it was sized for.
    strength.prior_fraction = 0.0;
    strength.prior = 1.0;
    assert_eq!(buckets(&strength, 1.0), [0, 0, 1, 2]);
    assert_eq!(buckets(&strength, 1e6), [0, 0, 0, 2]);
    strength.prior = 1e6;
    assert_eq!(buckets(&strength, 1e6), [0, 0, 1, 2]);
}

#[test]
fn relative_mapping_matches_current_pass_bits_and_uses_separate_reaches_and_rows() {
    let source = Relative {
        strength: &Buckets,
        counts: [8; 3],
        levels: 2,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::default(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let mut strategies = Strategies::new(&t, &source);
    let storage = &mut strategies.leaves[0];
    let subtree = t.nodes[storage.terminal]
        .terminal
        .as_ref()
        .unwrap()
        .l1
        .as_ref()
        .unwrap();
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    for r in &mut storage.regrets {
        *r = rng.gen_range(0.01..1.0);
    }
    let rows = storage.profile(subtree, false);
    // Empty average sums yield uniform mapping rows, different from current.
    let average_rows = storage.profile(subtree, true);
    let opponent = std::array::from_fn(|_| rng.gen_range(0.01..1.0));
    let own = std::array::from_fn(|_| rng.gen_range(0.01..1.0));
    let scale = std::array::from_fn(|_| rng.gen_range(0.5..1.0));
    let map_opponent = std::array::from_fn(|h| if class(h) % 4 >= 2 { opponent[h] } else { 0.0 });
    let map_own = std::array::from_fn(|h| if class(h) % 4 >= 2 { own[h] } else { 0.0 });
    let mut scratch = Scratch::default();
    let bits = |values: &[f64]| values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    for cards in [fixed("Ah Kd 2c 3s 7h"), fixed("Ac Kc Qc Jc Tc")] {
        let board = Board::new(cards, &source);
        for hero in subtree.active {
            for updating in [false, true] {
                let mut pass = Pass {
                    tree: subtree,
                    storage,
                    rows: &rows,
                    board: &board,
                    hero,
                    opponent: &opponent,
                    own: &own,
                    scale: &scale,
                    auxiliary: true,
                    mapping: None,
                    own_reach_regret: false,
                };
                // Nonzero initial updates check increments rather than just zeros.
                let mut inc = vec![0.125; rows.len()];
                let mut sums = vec![0.25; rows.len()];
                scratch.pass(&pass, updating.then_some((&mut inc, &mut sums, 0.75)));
                let values = bits(&scratch.values[..NUM_COMBOS]);
                let best = bits(&scratch.best[..NUM_COMBOS]);
                let expected_inc = bits(&inc);
                let expected_sums = bits(&sums);

                pass.mapping = Some(Mapping {
                    opponent: &opponent,
                    own: &own,
                    rows: &rows,
                });
                scratch.values.fill(f64::NAN);
                scratch.best.fill(f64::NAN);
                inc.fill(0.125);
                sums.fill(0.25);
                scratch.pass(&pass, updating.then_some((&mut inc, &mut sums, 0.75)));
                assert_eq!(bits(&scratch.values[..NUM_COMBOS]), values);
                assert_eq!(bits(&scratch.best[..NUM_COMBOS]), best);
                assert_eq!(bits(&inc), expected_inc);
                assert_eq!(bits(&sums), expected_sums);

                pass.mapping = Some(Mapping {
                    opponent: &map_opponent,
                    own: &map_own,
                    rows: &rows,
                });
                scratch.pass(&pass, None);
                assert!(
                    bits(&scratch.values[..NUM_COMBOS]) != values,
                    "mapping must change values for hero {hero}, board {cards:?}"
                );
                // Root reaches match again; only propagation below the root
                // differs. This catches a mapping that ignores its own rows.
                pass.mapping = Some(Mapping {
                    opponent: &opponent,
                    own: &own,
                    rows: &average_rows,
                });
                scratch.pass(&pass, None);
                assert!(
                    bits(&scratch.values[..NUM_COMBOS]) != values,
                    "mapping must change values for hero {hero}, board {cards:?}"
                );
            }
        }
    }
}

#[test]
fn finished_iteration_state_resumes_bit_identically() {
    check_finished_iteration_resume(&Buckets, None);
}

#[test]
fn relative_average_mapping_resumes_bit_identically_across_thread_counts() {
    check_finished_iteration_resume(
        &Relative {
            strength: &Buckets,
            counts: [4; 3],
            levels: 2,
            prior: 1.0,
            prior_fraction: 0.0,
            reach: RelativeReach::Average,
            refinement: Refinement::NONE,
            flops: None,
            hand: HandSplit::default(),
            runout: Runout::default(),
            opponent_levels: [false; 3],
        },
        None,
    );
}

fn check_finished_iteration_resume(source: &dyn BucketSource, qre: Option<super::Qre>) {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let options = Options {
        trunk: SolveOptions {
            iterations: 6,
            eval_every: 3,
            ..Default::default()
        },
        l1_boards: 10,
        l1_eval_boards: 10,
        l1_postflop_beta: 0.5,
        qre,
        l1_train_regression: qre.is_none(),
        ..Default::default()
    };
    let pool = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
    };
    let straight = pool(1)
        .install(|| solve(&t, &m, source, options, |_| {}))
        .unwrap();
    let mut bytes = Vec::new();
    let first = pool(4)
        .install(|| {
            solve(
                &t,
                &m,
                source,
                Options {
                    trunk: SolveOptions {
                        iterations: 3,
                        ..options.trunk
                    },
                    ..options
                },
                |p| {
                    if p.finished {
                        p.write_state(&mut bytes).unwrap();
                    }
                },
            )
        })
        .unwrap();
    // Fresh tree/model ensure neither terminal caches nor thread-local pools
    // carry training state into the resumed invocation.
    let g2 = game(&config(2, false));
    let t2 = Tree::build_with(&g2, FlopLeaves::L1).unwrap();
    let m2 = Model::new(&g2, &Synthetic, EvaluationOptions::default()).unwrap();
    let load = |data: &[u8], opts| read_state(&t2, &m2, source, opts, &mut &data[..]);
    let state = load(&bytes, options).unwrap();
    assert_eq!(state.iteration(), 3);
    assert_eq!(state.checkpoints.len(), first.checkpoints.len());
    assert_eq!(
        state.timings.trunk.update.to_bits(),
        first.timings.trunk.update.to_bits()
    );
    assert!(state.elapsed >= first.checkpoints.last().unwrap().seconds);
    let resumed = pool(2)
        .install(|| solve_with_state(&t2, &m2, source, options, Some(state), |_| Ok(())))
        .unwrap();
    for (a, b) in straight
        .average
        .rows
        .iter()
        .flatten()
        .zip(resumed.average.rows.iter().flatten())
    {
        assert_eq!(a.to_bits(), b.to_bits());
    }
    for (a, b) in straight
        .postflop
        .leaves
        .iter()
        .zip(&resumed.postflop.leaves)
    {
        for (a, b) in a
            .regrets
            .iter()
            .chain(&a.sums)
            .zip(b.regrets.iter().chain(&b.sums))
        {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
    assert_eq!(straight.checkpoints.len(), resumed.checkpoints.len());
    for (a, b) in straight.checkpoints.iter().zip(&resumed.checkpoints) {
        assert_eq!(a.iteration, b.iteration);
        let mut a = a.evaluation.clone();
        let mut b = b.evaluation.clone();
        a.seconds = 0.0;
        b.seconds = 0.0;
        assert_eq!(
            postcard::to_allocvec(&a).unwrap(),
            postcard::to_allocvec(&b).unwrap()
        );
    }
    assert!(resumed.timings.trunk.update >= first.timings.trunk.update);
    assert!(resumed.seconds >= first.seconds);
    assert!(resumed.checkpoints.last().unwrap().seconds >= first.seconds);
    assert!(
        load(
            &bytes,
            Options {
                l1_seed: 1,
                ..options
            }
        )
        .err()
        .unwrap()
        .to_string()
        .contains("identity mismatch")
    );
    assert!(
        load(
            &bytes,
            Options {
                trunk: SolveOptions {
                    eval_every: 2,
                    ..options.trunk
                },
                ..options
            }
        )
        .is_err()
    );
    assert!(
        read_state(
            &t2,
            &m2,
            &Refined {
                base: &Buckets,
                refinement: Refinement::NONE
            },
            options,
            &mut &bytes[..]
        )
        .is_err()
    );
    assert!(
        read_state(
            &t2,
            &m2,
            &Relative {
                strength: &Buckets,
                counts: [4; 3],
                levels: 1,
                prior: 0.0,
                prior_fraction: 0.0,
                reach: RelativeReach::Current,
                refinement: Refinement::NONE,
                flops: None,
                hand: HandSplit::default(),
                runout: Runout::default(),
                opponent_levels: [false; 3],
            },
            options,
            &mut &bytes[..]
        )
        .is_err()
    );
    assert!(
        load(
            &bytes,
            Options {
                trunk: SolveOptions {
                    iterations: 2,
                    ..options.trunk
                },
                ..options
            }
        )
        .is_err()
    );
    for end in [0, 8, 44, bytes.len() / 2, bytes.len() - 1] {
        assert!(load(&bytes[..end], options).is_err());
    }
    let mut corrupt = bytes.clone();
    corrupt[100] ^= 1;
    assert!(
        load(&corrupt, options)
            .err()
            .unwrap()
            .to_string()
            .contains("checksum mismatch")
    );
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(
        load(&trailing, options)
            .err()
            .unwrap()
            .to_string()
            .contains("trailing bytes")
    );
    let mut version = bytes.clone();
    version[8] = 2;
    assert!(
        load(&version, options)
            .err()
            .unwrap()
            .to_string()
            .contains("version")
    );
    let mut fingerprint = bytes.clone();
    fingerprint[12] ^= 1;
    assert!(
        load(&fingerprint, options)
            .err()
            .unwrap()
            .to_string()
            .contains("fingerprint")
    );

    if source.relative() && source.relative_average() {
        for (prior, reach) in [(0.0, RelativeReach::Average), (1.0, RelativeReach::Current)] {
            let changed = Relative {
                strength: &Buckets,
                counts: [4; 3],
                levels: 2,
                prior,
                prior_fraction: 0.0,
                reach,
                refinement: Refinement::NONE,
                flops: None,
                hand: HandSplit::default(),
                runout: Runout::default(),
                opponent_levels: [false; 3],
            };
            assert!(read_state(&t2, &m2, &changed, options, &mut &bytes[..]).is_err());
        }
    }

    // A limit already consumed before resume returns without further updates.
    let state = load(&bytes, options).unwrap();
    let stopped = solve_with_state(
        &t2,
        &m2,
        source,
        Options {
            max_seconds: Some(f64::MIN_POSITIVE),
            ..options
        },
        Some(state),
        |p| {
            assert!(p.finished);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(stopped.iterations, 3);
    assert_eq!(stopped.checkpoints.len(), first.checkpoints.len());

    let directory = tempfile::tempdir().unwrap();
    let stop_file = directory.path().join("stop");
    let stop_options = Options {
        stop_file: Some(&stop_file),
        ..options
    };
    // Stop paths are operational bounds, not part of the state's identity.
    let state = load(&bytes, stop_options).unwrap();
    let mut final_state = Vec::new();
    let stopped = solve_with_state(&t2, &m2, source, stop_options, Some(state), |p| {
        if p.iteration == 4 {
            std::fs::write(&stop_file, []).unwrap();
        }
        if p.finished {
            assert!(p.average.is_some());
            p.write_state(&mut final_state)?;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(stopped.iterations, 5);
    assert!(stopped.stopped_by_stop_file);
    assert_eq!(stopped.checkpoints.last().unwrap().iteration, 5);
    assert_eq!(load(&final_state, options).unwrap().iteration(), 5);

    let state = load(&bytes, stop_options).unwrap();
    let stopped = solve_with_state(&t2, &m2, source, stop_options, Some(state), |p| {
        assert!(p.finished);
        Ok(())
    })
    .unwrap();
    assert_eq!(stopped.iterations, 3);
    assert!(stopped.stopped_by_stop_file);
}

#[test]
fn fixed_profile_trains_only_selected_leaves_deterministically() {
    let g = game(&config(3, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let profile = Profile::uniform(&t);
    let options = Options {
        trunk: SolveOptions {
            iterations: 4,
            ..Default::default()
        },
        l1_boards: 6,
        ..Default::default()
    };
    let all = Strategies::new(&t, &Buckets);
    assert!(all.leaves.len() >= 2);
    let selected = all.leaves[0].terminal;
    let average = |s: &Strategies| {
        let mut bytes = Vec::new();
        s.write_average(&t, &mut bytes).unwrap();
        bytes
    };
    let solve = |threads| {
        let mut seen = Vec::new();
        let postflop = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                solve_postflop_fixed(
                    &t,
                    &m,
                    &Buckets,
                    &profile,
                    options,
                    Some(&[selected]),
                    None,
                    |i, _| {
                        seen.push(i);
                        Ok(())
                    },
                )
            })
            .unwrap();
        assert_eq!(seen, [1, 2, 3, 4]);
        postflop
    };
    let one = solve(1);
    assert_eq!(average(&one), average(&solve(3)));
    for (trained, initial) in one.leaves.iter().zip(&all.leaves) {
        if trained.terminal != selected {
            // Leaves that are not trained get no accumulators.
            assert!(
                trained.regrets.is_empty() && trained.sums.is_empty(),
                "leaf {}",
                trained.terminal
            );
            continue;
        }
        let subtree = t.nodes[trained.terminal]
            .terminal
            .as_ref()
            .unwrap()
            .l1
            .as_ref()
            .unwrap();
        assert_ne!(
            *trained.profile(subtree, true),
            *initial.profile(subtree, true)
        );
    }
    assert_eq!(
        one.slots,
        one.leaves.iter().map(|l| l.regrets.len()).sum::<usize>()
    );
    assert!(
        solve_postflop_fixed(
            &t,
            &m,
            &Buckets,
            &profile,
            options,
            Some(&[0]),
            None,
            |_, _| Ok(())
        )
        .is_err()
    );
}

#[test]
fn fixed_profile_state_resume_is_bit_exact_and_rejects_mismatch() {
    let g = game(&config(3, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let profile = Profile::uniform(&t);
    let selected = Strategies::new(&t, &Buckets).leaves[0].terminal;
    let dir = std::env::temp_dir().join(format!("p2-fixed-state-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("resume.state");
    let _ = std::fs::remove_file(&path);
    let options = Options {
        trunk: SolveOptions {
            iterations: 4,
            ..Default::default()
        },
        l1_boards: 6,
        ..Default::default()
    };
    let run = |options, state, seen: &mut Vec<u64>| {
        solve_postflop_fixed_with_state(
            &t,
            &m,
            &Buckets,
            &profile,
            options,
            Some(&[selected]),
            None,
            state,
            |i, _| {
                seen.push(i);
                Ok(())
            },
        )
    };
    let mut seen = Vec::new();
    let uninterrupted = run(options, None, &mut seen).unwrap();
    let state = Some(FixedState {
        path: &path,
        every_seconds: 1800.0,
    });
    run(
        Options {
            trunk: SolveOptions {
                iterations: 2,
                ..options.trunk
            },
            ..options
        },
        state,
        &mut Vec::new(),
    )
    .unwrap();
    seen.clear();
    let resumed = run(options, state, &mut seen).unwrap();
    assert_eq!(seen, [3, 4]);
    for (a, b) in uninterrupted.leaves.iter().zip(&resumed.leaves) {
        assert_eq!(
            a.regrets.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            b.regrets.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        assert_eq!(
            a.sums.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            b.sums.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
    let mut a = Vec::new();
    let mut b = Vec::new();
    uninterrupted.write_average(&t, &mut a).unwrap();
    resumed.write_average(&t, &mut b).unwrap();
    assert_eq!(a, b);
    seen.clear();
    run(options, state, &mut seen).unwrap();
    assert!(seen.is_empty());
    let error = run(
        Options {
            l1_seed: 1,
            ..options
        },
        state,
        &mut Vec::new(),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("identity mismatch"), "{error}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn time_limit_takes_final_checkpoint_and_observer_can_fail() {
    let g = game(&config(2, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let options = Options {
        max_seconds: Some(f64::MIN_POSITIVE),
        trunk: SolveOptions {
            iterations: 9,
            eval_every: 5,
            ..Default::default()
        },
        l1_boards: 2,
        l1_eval_boards: 2,
        ..Default::default()
    };
    let mut saved = Vec::new();
    let s = solve(&t, &m, &Buckets, options, |p| {
        if p.finished {
            assert!(p.average.is_some());
            assert_eq!(p.checkpoint.unwrap().iteration, 1);
            p.write_state(&mut saved).unwrap();
        }
    })
    .unwrap();
    assert_eq!(s.iterations, 1);
    assert_eq!(
        s.checkpoints
            .iter()
            .map(|c| c.iteration)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        read_state(&t, &m, &Buckets, options, &mut &saved[..])
            .unwrap()
            .iteration(),
        1
    );
    assert!(
        solve_with_state(&t, &m, &Buckets, options, None, |_| anyhow::bail!(
            "observer write failed"
        ))
        .err()
        .unwrap()
        .to_string()
        .contains("observer write failed")
    );
    for max_seconds in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(
            solve(
                &t,
                &m,
                &Buckets,
                Options {
                    max_seconds: Some(max_seconds),
                    ..options
                },
                |_| {}
            )
            .is_err()
        );
    }
}

#[test]
fn flop_groups_split_relative_classes_by_isomorphic_flop() {
    let mut seen = std::collections::BTreeSet::new();
    let mut text = String::new();
    for a in 0..52 {
        for b in a + 1..52 {
            for c in b + 1..52 {
                let flop = [a, b, c].map(Card::from_index);
                if seen.insert(canonical_flop(flop)) {
                    let ace = flop.iter().any(|x| x.rank() == 12);
                    text.push_str(&format!(
                        "{} {} {} {}\n",
                        flop[0],
                        flop[1],
                        flop[2],
                        u8::from(ace)
                    ));
                }
            }
        }
    }
    let groups = FlopClusters::parse(&text).unwrap();
    assert_eq!(groups.count(), 2);
    let plain = Relative {
        strength: &Buckets,
        counts: [3; 3],
        levels: 2,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::default(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let split = Relative {
        flops: Some(&groups),
        ..plain
    };
    assert_eq!(split.classes(Street::Turn), 2 * plain.classes(Street::Turn));
    let ace = fixed("Ah 7c 2d Ks 3h");
    let ace_permuted = fixed("As 7d 2c Kh 3s");
    let no_ace = fixed("Kh 7c 2d As 3h");
    for street in 3..=5 {
        for strength in [0, 1] {
            let base = plain.class(&ace[..street], strength);
            assert_eq!(base, plain.class(&no_ace[..street], strength));
            assert_eq!(
                split.class(&ace[..street], strength),
                split.class(&ace_permuted[..street], strength)
            );
            assert_ne!(
                split.class(&ace[..street], strength),
                split.class(&no_ace[..street], strength)
            );
        }
    }
    assert_ne!(split.state_identity(), plain.state_identity());
    let mut flop_only = FlopClusters::parse(&text).unwrap();
    flop_only.flop_only = true;
    let flop_split = Relative {
        flops: Some(&flop_only),
        ..plain
    };
    assert_eq!(
        flop_split.classes(Street::Flop),
        split.classes(Street::Flop)
    );
    assert_eq!(
        flop_split.classes(Street::Turn),
        plain.classes(Street::Turn)
    );
    assert_ne!(
        flop_split.class(&ace[..3], 0),
        flop_split.class(&no_ace[..3], 0)
    );
    assert_eq!(flop_split.class(&ace[..4], 0), plain.class(&ace[..4], 0));
    assert_ne!(flop_split.state_identity(), split.state_identity());
    let missing: String = text.lines().skip(1).map(|l| format!("{l}\n")).collect();
    assert!(FlopClusters::parse(&missing).is_err());
}

#[test]
fn draws_mark_flush_and_open_ended_straight_draws_only() {
    let cards =
        |s: &str| -> Vec<Card> { s.split_whitespace().map(|c| c.parse().unwrap()).collect() };
    let d = |board: &str, hole: &str| {
        super::cards::draws(&cards(board), cards(hole).try_into().unwrap())
    };
    assert_eq!(d("Kh 9h 2c", "Ah 5h"), 1);
    assert_eq!(d("Ts Jh 2c", "8c 9d"), 2);
    assert_eq!(d("Ts Jh 2h", "8h 9h"), 3);
    // Gutshot, backdoor flush and made hands are not draws.
    assert_eq!(d("Ts Jh 2c", "7c 9d"), 0);
    assert_eq!(d("Kh 9c 2c", "Ah 5h"), 0);
    assert_eq!(d("Kh 9h 2h", "Ah 5h"), 0);
    assert_eq!(d("Ts Jh Qc", "8c 9d"), 0);
    // Wheel draws use the ace low; a board straight draw is shared.
    assert_eq!(d("2s 3h 4c", "Ac Kd"), 0);
    assert_eq!(d("2s 3h 9c", "Ac 4d"), 0);
    assert_eq!(d("2s 3h 9c", "5c 4d"), 2);
    assert_eq!(d("5s 6h 7c 8d", "Ac Kd"), 0);
    // Turn: four to a flush with a hole card.
    assert_eq!(d("Kh 9h 2c 7s", "Ah 5h"), 1);
}

#[test]
fn category_counts_what_the_hole_cards_add_to_the_board() {
    let cards =
        |s: &str| -> Vec<Card> { s.split_whitespace().map(|c| c.parse().unwrap()).collect() };
    let c = |board: &str, hole: &str| {
        let board = cards(board);
        let n = super::cards::category(&board, cards(hole).try_into().unwrap());
        if board.len() == 5 {
            (n, 0)
        } else {
            (n / 6, n % 6)
        }
    };
    // Made hands on the flop, without draws.
    assert_eq!(c("Kh 9c 2d", "7s 4c"), (0, 0));
    assert_eq!(c("Kh 9c 2d", "2s 4c"), (1, 0));
    assert_eq!(c("Kh 9c 2d", "9s 4c"), (2, 0));
    assert_eq!(c("Kh 9c 2d", "Tc Ts"), (2, 0));
    assert_eq!(c("Kh 9c 2d", "4c 4s"), (1, 0));
    assert_eq!(c("Kh 9c 2d", "Ks 4c"), (3, 0));
    assert_eq!(c("Kh 9c 2d", "As Ac"), (4, 0));
    assert_eq!(c("Kh 9c 2d", "Ks 9s"), (5, 0));
    assert_eq!(c("Kh 9c 2d", "9s 9h"), (6, 0));
    assert_eq!(c("Kh 9c 9d", "9s 4c"), (6, 0));
    assert_eq!(c("Th 9c 8d", "Js 7c"), (7, 0));
    assert_eq!(c("Kh 9h 2h", "Ah 4h"), (8, 0));
    assert_eq!(c("Kh 9c 9d", "Ks Kc"), (9, 0));
    // The board's own pair is not the hand's: a kicker is nothing, and a
    // hole card pairing the board on a paired board is a pair.
    assert_eq!(c("Kh Kc 2d", "As 4c"), (0, 0));
    assert_eq!(c("Kh Kc 2d", "2s 4c"), (2, 0));
    // Draws.
    assert_eq!(c("Kh 9h 2c", "Ah 5h"), (0, 4));
    assert_eq!(c("Ts Jh 2c", "8c 9d"), (0, 3));
    assert_eq!(c("Ts Jh 2c", "7c 9d"), (0, 2));
    assert_eq!(c("Ts Jh 2h", "8h 9h"), (0, 5));
    assert_eq!(c("Kh 9c 2c", "Ah 5h"), (0, 1));
    assert_eq!(c("Kh 9h 2c 7s", "Ah 5d"), (0, 0));
    // The river has no draws.
    assert_eq!(c("Kh 9h 2c 7s 3d", "Ks 5h"), (3, 0));
}

#[test]
fn hand_split_separates_made_hands_from_draws_within_a_class() {
    let plain = Relative {
        strength: &Buckets,
        counts: [3; 3],
        levels: 1,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::default(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let split = Relative {
        hand: HandSplit::parse("made:2,draws").unwrap(),
        ..plain
    };
    assert_eq!(split.classes(Street::Flop), 8 * plain.classes(Street::Flop));
    assert_eq!(split.classes(Street::River), plain.classes(Street::River));
    assert_ne!(split.state_identity(), plain.state_identity());
    let board = super::cards::Board::new(fixed("Kh 9h 2c 7s 3d"), &split);
    let flop = &board.relative.as_ref().unwrap()[0];
    let combo = |s: &str| {
        let cards: Vec<Card> = s.split_whitespace().map(|c| c.parse().unwrap()).collect();
        (0..NUM_COMBOS)
            .find(|&h| {
                let c = Classes::get().cards(h);
                (c[0] == cards[0] && c[1] == cards[1]) || (c[0] == cards[1] && c[1] == cards[0])
            })
            .unwrap()
    };
    let class = |s: &str| flop.class[combo(s)] % 8;
    // Low flush draw with no pair: low made-hand half, flush draw. Ace high
    // is above the median made hand.
    assert_eq!(class("6h 5h"), 1);
    assert_eq!(class("Ah 5h"), 5);
    // Top set: high made-hand half, no draw.
    assert_eq!(class("Ks Kd"), 4);
    // Nothing: low half, no draw.
    assert_eq!(class("4c 5d") % 4, 0);
    assert!(HandSplit::parse("made:0").is_err());
    assert!(HandSplit::parse("drawz").is_err());
}

#[test]
fn pairs_split_parses_and_counts_classes() {
    let split = HandSplit::parse("draws,pairs").unwrap();
    let source = Relative {
        strength: &Buckets,
        counts: [1; 3],
        levels: 1,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: split,
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    assert_eq!(source.hand_classes(Street::Flop), 8);
    assert_eq!(source.hand_classes(Street::Turn), 8);
    assert_eq!(source.hand_classes(Street::River), 1);
    let pairs = Relative {
        hand: HandSplit::parse("pairs").unwrap(),
        ..source
    };
    assert_eq!(pairs.hand_classes(Street::Flop), 2);
    assert!(HandSplit::parse("category,pairs").is_err());
}

#[test]
fn pairs_class_bit_marks_pocket_pairs() {
    let source = Relative {
        strength: &Buckets,
        counts: [3; 3],
        levels: 1,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::parse("draws,pairs").unwrap(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let cards = fixed("Kh 9h 2c 7s 3d");
    // Board stores classes only for combos live on the complete sampled runout.
    let dead = cards.iter().fold(0_u64, |mask, c| mask | (1 << c.index()));
    let board = super::cards::Board::new(cards, &source);
    let flop = &board.relative.as_ref().unwrap()[0];
    let catalog = Classes::get();
    for h in 0..NUM_COMBOS {
        if catalog.combo_mask(h) & dead == 0 {
            let [a, b] = catalog.cards(h);
            assert_eq!(flop.class[h] % 2 == 1, a.rank() == b.rank(), "combo {h}");
        }
    }
}

#[test]
fn pairs_change_the_relative_identity() {
    let plain = Relative {
        strength: &Buckets,
        counts: [3; 3],
        levels: 1,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::default(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let pairs = Relative {
        hand: HandSplit::parse("pairs").unwrap(),
        ..plain
    };
    assert_ne!(plain.state_identity(), pairs.state_identity());
    let draws = Relative {
        hand: HandSplit::parse("draws").unwrap(),
        ..plain
    };
    let draws_pairs = Relative {
        hand: HandSplit::parse("draws,pairs").unwrap(),
        ..plain
    };
    assert_ne!(draws.state_identity(), draws_pairs.state_identity());
}

#[test]
fn weak_draws_mark_backdoor_flush_draws_on_the_flop_and_gutshots() {
    let cards =
        |s: &str| -> Vec<Card> { s.split_whitespace().map(|c| c.parse().unwrap()).collect() };
    let w = |board: &str, hole: &str| {
        super::cards::weak_draws(&cards(board), cards(hole).try_into().unwrap())
    };
    // Backdoor flush draw: three of a suit with a hole card, on the flop only.
    assert_eq!(w("Kh 9c 2c", "Ah 5h"), 1);
    assert_eq!(w("Kh 9h 2c", "Ah 5d"), 1);
    assert_eq!(w("Kh 9h 2h", "As 5d"), 0);
    assert_eq!(w("Kh 9h 2c", "Ah 5h"), 0);
    assert_eq!(w("Kh 9c 2c 7s", "Ah 5h"), 0);
    // Gutshot: exactly one straight out; an OESD or a made straight is not one.
    assert_eq!(w("Ts Jh 2c", "7c 9d"), 2);
    assert_eq!(w("Ts Jh 2c", "8c 9d"), 0);
    assert_eq!(w("Ts Jh Qc", "8c 9d"), 0);
    assert_eq!(w("Ts Jh 2c 4d", "7c 9d"), 2);
    assert_eq!(w("Ts Jh 2c", "7h 9h"), 3);
}

#[test]
fn backdoor_and_gutshot_add_flop_draw_levels_only() {
    let source = Relative {
        strength: &Buckets,
        counts: [3; 3],
        levels: 1,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::parse("draws,pairs,backdoor,gutshot").unwrap(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    assert_eq!(source.hand_classes(Street::Flop), 18);
    assert_eq!(source.hand_classes(Street::Turn), 8);
    assert_eq!(source.hand_classes(Street::River), 1);
    assert!(HandSplit::parse("backdoor").is_err());
    assert!(HandSplit::parse("pairs,gutshot").is_err());
    let plain = Relative {
        hand: HandSplit::parse("draws,pairs").unwrap(),
        ..source
    };
    assert_ne!(plain.state_identity(), source.state_identity());
    let cards = fixed("Kh 9c 2c 7s 3d");
    let flop_cards = &cards[..3];
    let dead = cards.iter().fold(0_u64, |mask, c| mask | (1 << c.index()));
    let split = super::cards::Board::new(cards, &source);
    let base = super::cards::Board::new(cards, &plain);
    let (split_flop, base_flop) = (
        &split.relative.as_ref().unwrap()[0],
        &base.relative.as_ref().unwrap()[0],
    );
    let (split_turn, base_turn) = (
        &split.relative.as_ref().unwrap()[1],
        &base.relative.as_ref().unwrap()[1],
    );
    let catalog = Classes::get();
    for h in 0..NUM_COMBOS {
        if catalog.combo_mask(h) & dead != 0 {
            continue;
        }
        let hole = catalog.cards(h);
        let draws = super::cards::draws(flop_cards, hole);
        let weak = super::cards::weak_draws(flop_cards, hole);
        let pair = usize::from(hole[0].rank() == hole[1].rank());
        // Flop class = strength bucket x (flush 3 x straight 3) x pair 2.
        let flush = if draws & 1 != 0 {
            2
        } else {
            usize::from(weak & 1)
        };
        let straight = if draws & 2 != 0 {
            2
        } else {
            usize::from(weak & 2 != 0)
        };
        let class = usize::from(split_flop.class[h]);
        assert_eq!(class % 18, (flush * 3 + straight) * 2 + pair, "combo {h}");
        assert_eq!(class / 18, usize::from(base_flop.class[h]) / 8, "combo {h}");
        // The turn keeps the classes of draws,pairs.
        assert_eq!(split_turn.class[h], base_turn.class[h], "combo {h}");
    }
}

#[test]
fn category_split_classes_combos_by_made_hand_and_draw_on_every_street() {
    let plain = Relative {
        strength: &Buckets,
        counts: [1; 3],
        levels: 1,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::default(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let split = Relative {
        hand: HandSplit::parse("category:2").unwrap(),
        ..plain
    };
    assert_eq!(split.classes(Street::Flop), 120);
    assert_eq!(split.classes(Street::River), 20);
    assert_ne!(split.state_identity(), plain.state_identity());
    assert_ne!(
        split.state_identity(),
        Relative {
            hand: HandSplit::parse("category").unwrap(),
            ..plain
        }
        .state_identity()
    );
    let board = super::cards::Board::new(fixed("Kh 9h 2c 7s 3d"), &split);
    let relative = board.relative.as_ref().unwrap();
    let combo = |s: &str| {
        let cards: Vec<Card> = s.split_whitespace().map(|c| c.parse().unwrap()).collect();
        (0..NUM_COMBOS)
            .find(|&h| {
                let c = Classes::get().cards(h);
                (c[0] == cards[0] && c[1] == cards[1]) || (c[0] == cards[1] && c[1] == cards[0])
            })
            .unwrap()
    };
    let category = |street: usize, s: &str| relative[street].class[combo(s)] / 2;
    // Flop: set, flush draw, top pair with a backdoor flush draw.
    assert_eq!(category(0, "Ks Kd"), 6 * 6);
    assert_eq!(category(0, "Ah 5h"), 4);
    assert_eq!(category(0, "Kd Qh"), 3 * 6 + 1);
    // River: made hands only.
    assert_eq!(category(2, "Ks Kd"), 6);
    assert_eq!(category(2, "Ah 5h"), 0);
    // The two strength halves of a category: the top set above bottom set.
    assert!(relative[0].class[combo("Ks Kd")] % 2 >= relative[0].class[combo("2s 2d")] % 2);
    assert!(HandSplit::parse("category,draws").is_err());
    assert!(HandSplit::parse("category:0").is_err());
}

#[test]
fn runout_split_classes_the_turn_and_river_card_by_its_first_matching_kind() {
    let plain = Relative {
        strength: &Buckets,
        counts: [3; 3],
        levels: 1,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::default(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let all = Relative {
        runout: Runout::parse("flush,pair,straight,over").unwrap(),
        ..plain
    };
    assert_eq!(all.classes(Street::Flop), plain.classes(Street::Flop));
    assert_eq!(all.classes(Street::Turn), 5 * plain.classes(Street::Turn));
    assert_eq!(all.classes(Street::River), 5 * plain.classes(Street::River));
    assert_ne!(all.state_identity(), plain.state_identity());
    let class = |source: &Relative<'_>, board: &str| {
        let board: Vec<Card> = board
            .split_whitespace()
            .map(|c| c.parse().unwrap())
            .collect();
        source.class(&board, 0)
    };
    assert_eq!(class(&all, "Kh 9h 2c"), 0);
    // Flush beats pair; pair beats straight; straight beats over.
    assert_eq!(class(&all, "Kh 9h 2c 5h"), 1);
    assert_eq!(class(&all, "Kh 9h 2c 9d"), 2);
    assert_eq!(class(&all, "Kh 9d 2c Th"), 3);
    assert_eq!(class(&all, "Kh 3d 2c As"), 3, "the ace is also low");
    assert_eq!(class(&all, "Kh 9d 2c As"), 4);
    assert_eq!(class(&all, "Kh 9d 2c 7s"), 0);
    // On the river only the river card counts, and a straight already
    // possible on the turn is not new.
    assert_eq!(class(&all, "Kh 9h 2c 5h 7d"), 3);
    assert_eq!(class(&all, "Kh 9d Tc 2s Qd"), 0);
    let pair = Relative {
        runout: Runout::parse("pair").unwrap(),
        ..plain
    };
    assert_eq!(pair.classes(Street::Turn), 2 * plain.classes(Street::Turn));
    assert_eq!(class(&pair, "Kh 9h 2c 9d"), 1);
    assert_eq!(class(&pair, "Kh 9h 2c 5h"), 0);
    assert_eq!(
        Runout::parse("over,flush").unwrap(),
        Runout::parse("flush,over").unwrap()
    );
    assert_eq!(Runout::parse("none").unwrap(), Runout::default());
    assert!(Runout::parse("flsh").is_err());
}

/// Strength 0 on the flop; on later streets the combo's hearts if the last
/// card is a heart, else 0.
struct Hearts;

impl BucketSource for Hearts {
    fn count(&self, _street: Street) -> usize {
        3
    }
    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS] {
        let dead = board.iter().fold(0_u64, |b, c| b | (1 << c.index()));
        let heart = "2h".parse::<Card>().unwrap().suit();
        let last_heart = board.len() > 3 && board[board.len() - 1].suit() == heart;
        std::array::from_fn(|h| {
            if Classes::get().combo_mask(h) & dead != 0 {
                u16::MAX
            } else if last_heart {
                let cards = Classes::get().cards(h);
                cards.iter().filter(|c| c.suit() == heart).count() as u16
            } else {
                0
            }
        })
    }
}

#[test]
fn potential_split_ranks_combos_by_the_variance_of_their_next_card_strength() {
    let plain = Relative {
        strength: &Hearts,
        counts: [3; 3],
        levels: 1,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::default(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let split = Relative {
        hand: HandSplit::parse("potential:2").unwrap(),
        ..plain
    };
    assert_eq!(split.classes(Street::Flop), 2 * plain.classes(Street::Flop));
    assert_eq!(split.classes(Street::River), plain.classes(Street::River));
    assert_ne!(split.state_identity(), plain.state_identity());
    let three = Relative {
        hand: HandSplit::parse("potential:3").unwrap(),
        ..plain
    };
    assert_ne!(split.state_identity(), three.state_identity());
    let draws = Relative {
        hand: HandSplit::parse("draws").unwrap(),
        ..plain
    };
    let draws_potential = Relative {
        hand: HandSplit::parse("draws,potential:2").unwrap(),
        ..plain
    };
    assert_ne!(draws.state_identity(), draws_potential.state_identity());
    assert_eq!(
        draws_potential.classes(Street::Flop),
        8 * plain.classes(Street::Flop)
    );
    let board = super::cards::Board::new(fixed("Kh 9h 2c 7s 3d"), &split);
    let relative = board.relative.as_ref().unwrap();
    let combo = |s: &str| {
        let cards: Vec<Card> = s.split_whitespace().map(|c| c.parse().unwrap()).collect();
        (0..NUM_COMBOS)
            .find(|&h| {
                let c = Classes::get().cards(h);
                (c[0] == cards[0] && c[1] == cards[1]) || (c[0] == cards[1] && c[1] == cards[0])
            })
            .unwrap()
    };
    // 703 of the 1176 combos live on the flop hold no heart and never vary:
    // the lower half. One or two hearts vary with the turn card's suit.
    let flop = |s: &str| relative[0].class[combo(s)];
    assert_eq!(flop("4c 5d"), 0);
    assert_eq!(flop("Ah 4c"), 1);
    assert_eq!(flop("6h 5h"), 1);
    // The river has no potential split.
    assert_eq!(
        relative[2].class[combo("6h 5h")],
        relative[2].class[combo("4c 5d")]
    );
    assert!(HandSplit::parse("potential:0").is_err());
}

#[test]
fn flop_group_specs_load_the_bundled_groups_or_none() {
    let groups = FlopClusters::from_spec("recommended").unwrap().unwrap();
    assert_eq!(groups.count(), 49);
    assert!(!groups.flop_only);
    assert!(
        FlopClusters::from_spec("recommended:flop")
            .unwrap()
            .unwrap()
            .flop_only
    );
    assert!(FlopClusters::from_spec("none").unwrap().is_none());
    assert!(FlopClusters::from_spec("missing-groups.txt").is_err());
}

/// The recommended cards with two strength-table levels, the base these
/// tests need: a low-SPR mapping takes no opponent levels, and with four
/// levels a runout split has more turn classes than `u16` ids.
fn table_level_cards() -> CardSpec {
    CardSpec {
        levels: 2,
        opponent_levels: [false; 3],
        low_spr_levels: None,
        ..CardSpec::recommended()
    }
}

#[test]
fn low_spr_mapping_matches_finer_source_and_preserves_higher_spr_bits() {
    let spec = LowSprSpec::parse("2.5:16x4:recommended").unwrap();
    let cards = table_level_cards();
    let base = cards.source(&Buckets);
    let source = LowSprCards::new(base, &spec).unwrap();
    let (_, finer) = source.low_spr().unwrap();
    let runout = fixed("Kh 9h 2c 7s 3d");
    let plain = Board::new(runout, &base);
    let refined = Board::new(runout, &source);
    let fine = Board::new(runout, finer);
    assert_eq!(plain.ranks, refined.ranks);
    assert_eq!(plain.sorted, refined.sorted);
    assert_eq!(plain.buckets, refined.buckets);
    let reach = vec![0.01; NUM_COMBOS];
    for s in 0..3 {
        let mut expected = vec![u16::MAX; NUM_COMBOS];
        let mut actual = expected.clone();
        super::pass::relative_buckets(&fine.relative.as_ref().unwrap()[s], &reach, &mut expected);
        super::pass::relative_leaf_buckets(
            &refined.relative.as_ref().unwrap()[s],
            2.5,
            0,
            &reach,
            None,
            &mut actual,
        );
        assert_eq!(expected, actual);
        super::pass::relative_buckets(&plain.relative.as_ref().unwrap()[s], &reach, &mut expected);
        super::pass::relative_leaf_buckets(
            &refined.relative.as_ref().unwrap()[s],
            2.500001,
            0,
            &reach,
            None,
            &mut actual,
        );
        assert_eq!(expected, actual);
    }
    assert_ne!(base.state_identity(), source.state_identity());
    let changed = LowSprSpec::parse("2.4:16x4:recommended").unwrap();
    assert_ne!(
        source.state_identity(),
        LowSprCards::new(base, &changed).unwrap().state_identity()
    );
}

#[test]
fn low_spr_storage_refines_only_selected_leaves_at_the_inclusive_boundary() {
    let g = game(&config(3, false));
    let mut tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let cards = table_level_cards();
    let base = cards.source(&Buckets);
    let spec = LowSprSpec::parse("2.5:32x2:recommended").unwrap();
    let source = LowSprCards::new(base, &spec).unwrap();
    let baseline = Strategies::new(&tree, &base);
    assert!(baseline.leaves.len() >= 2);
    for (i, leaf) in baseline.leaves.iter().enumerate() {
        let subtree = tree.nodes[leaf.terminal]
            .terminal
            .as_mut()
            .unwrap()
            .l1
            .as_mut()
            .unwrap();
        assert!(subtree.flop_spr.is_finite() && subtree.flop_spr >= 0.0);
        subtree.flop_spr = if i % 2 == 0 { 2.5 } else { 2.500001 };
    }
    let refined = Strategies::new(&tree, &source);
    for (i, (plain, fine)) in baseline.leaves.iter().zip(&refined.leaves).enumerate() {
        let factor = if i % 2 == 0 { 2 } else { 1 };
        assert_eq!(fine.sums.len(), plain.sums.len() * factor);
        assert_eq!(
            fine.offsets,
            plain.offsets.iter().map(|n| n * factor).collect::<Vec<_>>()
        );
    }
    for invalid in [
        "NaN:8x2:none",
        "-1:8x2:none",
        "2.5:0x2:none",
        "2.5:8x0:none",
        "2.5:8x2",
        "2.5:65536x2:none",
    ] {
        assert!(LowSprSpec::parse(invalid).is_err(), "{invalid}");
    }
    let too_many = LowSprSpec::parse("2.5:128x128:recommended").unwrap();
    assert!(LowSprCards::new(base, &too_many).is_err());
}

/// The bundled groups with every group id moved by `shift` (mod 49), so every
/// flop is in another group than in the bundled ones.
fn moved_groups(shift: u16) -> FlopClusters {
    let text: String = RECOMMENDED_FLOP_GROUPS
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            let group: u16 = parts[3].parse().unwrap();
            format!(
                "{} {} {} {}\n",
                parts[0],
                parts[1],
                parts[2],
                (group + shift) % 49
            )
        })
        .collect();
    FlopClusters::parse(&text).unwrap()
}

#[test]
fn per_leaf_cards_train_and_evaluate_a_leaf_like_its_own_whole_tree_mapping() {
    let g = game(&config(3, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let profile = Profile::uniform(&t);
    let options = Options {
        trunk: SolveOptions {
            iterations: 4,
            ..Default::default()
        },
        l1_boards: 6,
        ..Default::default()
    };
    let cards = table_level_cards();
    let base = cards.source(&Buckets);
    let runout = Runout::parse("flush,pair,straight,over").unwrap();
    // Lower and higher group ids than the bundled ones on most flops: the
    // class shift of a board takes both signs.
    for shift in [7, 42] {
        let moved = moved_groups(shift);
        let own = Relative {
            flops: Some(&moved),
            runout,
            ..base
        };
        let z = Strategies::new(&t, &base).leaves[0].terminal;
        let spec = LeafCardSpec {
            flops: vec![(z, Some(&moved))],
            runout: Some((runout, usize::MAX)),
            low_spr_levels: None,
        };
        let source = LeafCards::new(&t, base, &spec).unwrap();
        assert!(source.leaf_variant(z) > 0);
        let solve = |source: &dyn BucketSource| {
            solve_postflop_fixed(
                &t,
                &m,
                source,
                &profile,
                options,
                Some(&[z]),
                None,
                |_, _| Ok(()),
            )
            .unwrap()
        };
        let (per_leaf, whole) = (solve(&source), solve(&own));
        let leaf = |s: &Strategies| s.leaves.iter().find(|l| l.terminal == z).unwrap().clone();
        let (a, b) = (leaf(&per_leaf), leaf(&whole));
        assert_eq!(a.offsets, b.offsets);
        assert_eq!(a.sums, b.sums);
        assert!(a.sums.iter().any(|&x| x != 0.0));
        let boards = |source: &dyn BucketSource| -> Vec<Board> {
            ["Kh 9h 2c 9s Ah", "Kh 9h 2c 7s 3d", "Qd Jd Tc 2d 9c"]
                .iter()
                .map(|s| Board::new(fixed(s), source))
                .collect()
        };
        let (per_leaf_boards, whole_boards) = (boards(&source), boards(&own));
        for &hero in &t.nodes[z].terminal.as_ref().unwrap().active {
            let x = leaf_strategy(&t, &profile, &m, &per_leaf, &per_leaf_boards, z, hero).unwrap();
            let y = leaf_strategy(&t, &profile, &m, &whole, &whole_boards, z, hero).unwrap();
            assert_eq!(
                serde_json::to_string(&x).unwrap(),
                serde_json::to_string(&y).unwrap()
            );
            let x =
                leaf_diagnostic(&t, &profile, &m, &per_leaf, &per_leaf_boards, z, hero).unwrap();
            let y = leaf_diagnostic(&t, &profile, &m, &whole, &whole_boards, z, hero).unwrap();
            assert_eq!((x.profile, x.best), (y.profile, y.best));
        }
    }
}

#[test]
fn per_leaf_runout_split_follows_the_slot_limit_at_the_inclusive_boundary() {
    let g = game(&config(3, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let cards = table_level_cards();
    let base = cards.source(&Buckets);
    let runout = Runout::parse("flush,pair").unwrap();
    let plain = Strategies::new(&t, &base);
    let split = Strategies::new(&t, &Relative { runout, ..base });
    let limit = split.leaves[0].sums.len();
    assert!(limit > plain.leaves[0].sums.len());
    let mapping = |max_slots| {
        let spec = LeafCardSpec {
            flops: Vec::new(),
            runout: Some((runout, max_slots)),
            low_spr_levels: None,
        };
        LeafCards::new(&t, base, &spec).unwrap()
    };
    let (inside, outside) = (mapping(limit), mapping(limit - 1));
    for (source, max_slots) in [(&inside, limit), (&outside, limit - 1)] {
        let storage = Strategies::new(&t, source);
        let mut splits = 0;
        for ((leaf, p), s) in storage.leaves.iter().zip(&plain.leaves).zip(&split.leaves) {
            let expected = if s.sums.len() <= max_slots { s } else { p };
            assert_eq!(leaf.offsets, expected.offsets, "leaf {}", leaf.terminal);
            assert_eq!(leaf.sums.len(), expected.sums.len());
            splits += usize::from(s.sums.len() <= max_slots);
        }
        assert_eq!(source.changed_leaves(), (0, splits, 0));
    }
    assert!(inside.leaf_variant(split.leaves[0].terminal) > 0);
    assert_eq!(outside.leaf_variant(split.leaves[0].terminal), 0);
    assert_ne!(inside.state_identity(), outside.state_identity());
    // The base must not split already, and a split needs kinds.
    let doubled = LeafCardSpec {
        flops: Vec::new(),
        runout: Some((runout, limit)),
        low_spr_levels: None,
    };
    assert!(LeafCards::new(&t, Relative { runout, ..base }, &doubled).is_err());
    let empty = LeafCardSpec {
        flops: Vec::new(),
        runout: Some((Runout::default(), limit)),
        low_spr_levels: None,
    };
    assert!(LeafCards::new(&t, base, &empty).is_err());
}

#[test]
fn low_spr_levels_train_and_evaluate_a_leaf_like_its_own_whole_tree_mapping() {
    let g = game(&config(3, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let profile = Profile::uniform(&t);
    let options = Options {
        trunk: SolveOptions {
            iterations: 4,
            ..Default::default()
        },
        l1_boards: 6,
        ..Default::default()
    };
    let cards = CardSpec::recommended();
    let base = Relative {
        levels: 4,
        opponent_levels: [true; 3],
        ..cards.source(&Buckets)
    };
    let spr = |z: usize| {
        t.nodes[z]
            .terminal
            .as_ref()
            .unwrap()
            .l1
            .as_ref()
            .unwrap()
            .flop_spr
    };
    let plain = Strategies::new(&t, &base);
    let z = plain.leaves[0].terminal;
    let rule = |buckets, levels| LeafCardSpec {
        flops: Vec::new(),
        runout: None,
        low_spr_levels: Some(LowSprLevels {
            max_spr: spr(z),
            buckets,
            levels,
        }),
    };
    // Fewer levels alone, and fewer percentile buckets with them.
    for (buckets, counts) in [(None, base.counts), (Some(8), [8; 3])] {
        let whole = Relative {
            levels: 2,
            counts,
            ..base
        };
        let source = LeafCards::new(&t, base, &rule(buckets, 2)).unwrap();
        let fewer = Strategies::new(&t, &whole);
        let storage = Strategies::new(&t, &source);
        let mut changed = 0;
        for ((leaf, p), f) in storage.leaves.iter().zip(&plain.leaves).zip(&fewer.leaves) {
            let low = spr(leaf.terminal) <= spr(z);
            assert_eq!(
                source.leaf_variant(leaf.terminal) > 0,
                low,
                "leaf {}",
                leaf.terminal
            );
            assert_eq!(leaf.sums.len(), if low { f } else { p }.sums.len());
            changed += usize::from(low);
        }
        assert_eq!(source.changed_leaves(), (0, 0, changed));
        let solve = |source: &dyn BucketSource| {
            solve_postflop_fixed(
                &t,
                &m,
                source,
                &profile,
                options,
                Some(&[z]),
                None,
                |_, _| Ok(()),
            )
            .unwrap()
        };
        let (per_leaf, whole_tree) = (solve(&source), solve(&whole));
        let leaf = |s: &Strategies| s.leaves.iter().find(|l| l.terminal == z).unwrap().clone();
        let (a, b) = (leaf(&per_leaf), leaf(&whole_tree));
        assert_eq!(a.offsets, b.offsets);
        assert_eq!(a.sums, b.sums);
        assert!(a.sums.iter().any(|&x| x != 0.0));
        let boards = |source: &dyn BucketSource| -> Vec<Board> {
            ["Kh 9h 2c 9s Ah", "Kh 9h 2c 7s 3d", "Qd Jd Tc 2d 9c"]
                .iter()
                .map(|s| Board::new(fixed(s), source))
                .collect()
        };
        let (per_leaf_boards, whole_boards) = (boards(&source), boards(&whole));
        for &hero in &t.nodes[z].terminal.as_ref().unwrap().active {
            let x = leaf_strategy(&t, &profile, &m, &per_leaf, &per_leaf_boards, z, hero).unwrap();
            let y = leaf_strategy(&t, &profile, &m, &whole_tree, &whole_boards, z, hero).unwrap();
            assert_eq!(
                serde_json::to_string(&x).unwrap(),
                serde_json::to_string(&y).unwrap()
            );
            let x =
                leaf_diagnostic(&t, &profile, &m, &per_leaf, &per_leaf_boards, z, hero).unwrap();
            let y = leaf_diagnostic(&t, &profile, &m, &whole_tree, &whole_boards, z, hero).unwrap();
            assert_eq!((x.profile, x.best), (y.profile, y.best));
        }
    }
    // The levels must divide the base's, and every street needs opponent levels.
    assert!(LeafCards::new(&t, base, &rule(None, 3)).is_err());
    let river = Relative {
        opponent_levels: [false, false, true],
        ..base
    };
    assert!(LeafCards::new(&t, river, &rule(None, 2)).is_err());
    let parsed = |max_spr, buckets, levels| {
        Some(LowSprLevels {
            max_spr,
            buckets,
            levels,
        })
    };
    assert_eq!(parse_low_spr_levels("2.5:2").unwrap(), parsed(2.5, None, 2));
    assert_eq!(
        parse_low_spr_levels("2.5:8x2").unwrap(),
        parsed(2.5, Some(8), 2)
    );
    assert_eq!(parse_low_spr_levels("none").unwrap(), None);
    for bad in ["2.5", "x:2", "-1:2", "inf:2", "2.5:0", "2.5:0x2", "2.5:x2"] {
        assert!(parse_low_spr_levels(bad).is_err(), "{bad}");
    }
}

#[test]
fn late_training_boards_start_at_their_first_iteration() {
    let late = Options {
        l1_boards_late: Some(LateBoards::parse("128:1001").unwrap()),
        ..Default::default()
    };
    assert_eq!(late.training_boards(1000), late.l1_boards);
    assert_eq!(late.training_boards(1001), 128);
    assert_eq!(late.training_boards(2000), 128);
    let plain = Options::default();
    assert_eq!(plain.training_boards(5000), plain.l1_boards);
    for bad in ["128", "0:5", "128:0", "x:5", "128:x", "128:5:6"] {
        assert!(LateBoards::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn leaf_flop_group_rows_resolve_files_beside_the_list_and_lines_to_leaves() {
    let dir = std::env::temp_dir().join(format!("p2-leaf-groups-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("moved.txt"), {
        let moved = moved_groups(7);
        let mut text = String::new();
        for l in RECOMMENDED_FLOP_GROUPS
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let parts: Vec<&str> = l.split_whitespace().collect();
            let flop: Vec<Card> = parts[..3].iter().map(|c| c.parse().unwrap()).collect();
            text += &format!(
                "{} {} {} {}\n",
                parts[0],
                parts[1],
                parts[2],
                moved.group(&flop)
            );
        }
        text
    })
    .unwrap();
    let rows = parse_leaf_flop_groups(
        "a/b moved.txt\n\n  c/d none\ne/f recommended:flop\ng/h moved.txt:flop\n",
        &dir,
    )
    .unwrap();
    let names: Vec<&str> = rows.iter().map(|(line, _)| line.as_str()).collect();
    assert_eq!(names, ["a/b", "c/d", "e/f", "g/h"]);
    let flop = fixed("Kh 9h 2c 7s 3d");
    let moved = rows[0].1.as_ref().unwrap();
    assert!(!moved.flop_only);
    assert_eq!(moved.group(&flop[..3]), moved_groups(7).group(&flop[..3]));
    assert!(rows[1].1.is_none());
    assert!(rows[2].1.as_ref().unwrap().flop_only);
    assert!(rows[3].1.as_ref().unwrap().flop_only);
    assert!(parse_leaf_flop_groups("a/b", &dir).is_err());
    assert!(parse_leaf_flop_groups("a/b moved.txt extra", &dir).is_err());
    assert!(parse_leaf_flop_groups("a/b missing.txt", &dir).is_err());
    std::fs::remove_dir_all(&dir).unwrap();

    let g = game(&config(3, false));
    let t = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let z = Strategies::new(&t, &Buckets).leaves[0].terminal;
    let mut line = Vec::new();
    let mut at = z;
    while at != 0 {
        let (parent, a) = (0..t.nodes.len())
            .find_map(|p| {
                t.nodes[p]
                    .children
                    .iter()
                    .position(|&c| c == at)
                    .map(|a| (p, a))
            })
            .unwrap();
        line.push(t.nodes[parent].labels[a].clone());
        at = parent;
    }
    line.reverse();
    assert_eq!(leaf_of_line(&t, &line.join("/")).unwrap(), z);
    assert!(leaf_of_line(&t, "").is_err());
    assert!(leaf_of_line(&t, "no-such-action").is_err());
}

#[test]
fn diagnostic_matchup_mass_preserves_heads_up_zero_sum_on_thin_ranges() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let model = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let profile = Profile::uniform(&tree);
    let reach = reaches(&tree, &profile, &model);
    let strategies = Strategies::new(&tree, &Buckets);
    let terminal = strategies.leaves[0].terminal;
    let boards = [Board::new(fixed("Kh 9h 2c 7s 3d"), &Buckets)];
    let mut total = 0.0;
    for &hero in &tree.nodes[terminal].terminal.as_ref().unwrap().active {
        let d = leaf_diagnostic(
            &tree,
            &profile,
            &model,
            &strategies,
            &boards,
            terminal,
            hero,
        )
        .unwrap();
        let mut value = 0.0;
        let mut mass = 0.0;
        for c in 0..169 {
            if d.mass[c] > 0.0 {
                let weight = Classes::get().n(c) as f64
                    * model.weights[hero][c]
                    * reach[terminal].pi(hero, c)
                    * d.matchup_mass[c];
                value += weight * d.profile[c] / d.mass[c];
                mass += weight;
            }
        }
        assert!(mass > 0.0);
        total += value / mass;
    }
    near(total, 0.0);
}

#[test]
fn informed_best_response_sits_between_the_strategy_and_the_runout_seeing_best() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let model = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let profile = Profile::uniform(&tree);
    let strategies = Strategies::new(&tree, &Buckets);
    let terminal = strategies.leaves[0].terminal;
    let hero = tree.nodes[terminal].terminal.as_ref().unwrap().active[0];
    let run = |boards: &[Board]| {
        let d =
            leaf_diagnostic(&tree, &profile, &model, &strategies, boards, terminal, hero).unwrap();
        let informed = leaf_informed_best_response(
            &tree,
            &profile,
            &model,
            &strategies,
            boards,
            terminal,
            hero,
        )
        .unwrap();
        (d, informed)
    };
    // One board: every decision sees the whole board, so it is the pass's best.
    let (d, informed) = run(&[Board::new(fixed("Kh 9h 2c 7s 3d"), &Buckets)]);
    for (&informed, &best) in informed.iter().zip(&d.best) {
        near(informed, best);
    }
    // Turns and rivers the flop decision cannot see: below the runout-seeing
    // best for some class, never below the saved strategy or above that best.
    let mut boards: Vec<Board> = [
        "Kh 9h 2c 7s 3d",
        "Kh 9h 2c 7s Ad",
        "Kh 9h 2c Qd 3d",
        "Kh 9h 2c Qd 8s",
    ]
    .iter()
    .map(|b| Board::new(fixed(b), &Buckets))
    .collect();
    let (d, informed) = run(&boards);
    let mut below = false;
    for (c, &value) in informed.iter().enumerate() {
        assert!(value >= d.profile[c] - 1e-9, "class {c}");
        assert!(value <= d.best[c] + 1e-9, "class {c}");
        below |= value < d.best[c] - 1e-6;
    }
    assert!(below);
    let other = Board::new(fixed("Ah 9h 2c 7s 3d"), &Buckets);
    assert!(
        leaf_informed_best_response(
            &tree,
            &profile,
            &model,
            &strategies,
            &[boards.remove(0), other],
            terminal,
            hero
        )
        .is_err()
    );
}

#[test]
fn relative_flop_recall_persists_after_draws_complete_or_miss() {
    let cards = table_level_cards();
    let plain = Relative {
        hand: HandSplit::parse("draws").unwrap(),
        ..cards.source(&Buckets)
    };
    let strength = Relative {
        refinement: Refinement::parse("recall:2").unwrap(),
        ..plain
    };
    let draws = Relative {
        hand: HandSplit::parse("draws,flop-draws").unwrap(),
        ..plain
    };
    let both = Relative {
        hand: draws.hand,
        ..strength
    };
    for variant in [&strength, &draws, &both] {
        assert_eq!(variant.count(Street::Flop), plain.count(Street::Flop));
        assert_ne!(variant.state_identity(), plain.state_identity());
    }
    assert_ne!(strength.state_identity(), draws.state_identity());
    assert_eq!(strength.count(Street::Turn), 2 * plain.count(Street::Turn));
    assert_eq!(draws.count(Street::River), 2 * plain.count(Street::River));
    assert_eq!(both.count(Street::Turn), 4 * plain.count(Street::Turn));
    for cards in [fixed("Kh 9h 2c 7s 3d"), fixed("Kh 9h 2c 7h 3d")] {
        let board = Board::new(cards, &both);
        let base = Board::new(cards, &plain);
        let later = board.relative.as_ref().unwrap();
        let base = base.relative.as_ref().unwrap();
        for &h in &board.sorted {
            let hole = Classes::get().cards(h);
            let expected = (board.buckets[0][h] / 2) * 2
                + u16::from(super::cards::draws(&cards[..3], hole) != 0);
            assert_eq!(later[0].class[h], base[0].class[h]);
            for s in [1, 2] {
                assert_eq!(later[s].class[h] / 4, base[s].class[h]);
                assert_eq!(later[s].class[h] % 4, expected);
                assert!(usize::from(later[s].class[h]) < later[s].classes);
            }
        }
    }
    assert!(HandSplit::parse("category,flop-draws").is_err());
}

#[test]
fn low_spr_mapping_preserves_persistent_flop_recall() {
    let spec = table_level_cards();
    let base = Relative {
        refinement: Refinement::parse("recall:2").unwrap(),
        hand: HandSplit::parse("draws,flop-draws").unwrap(),
        ..spec.source(&Buckets)
    };
    let fine = LowSprSpec::parse("2.5:16x2:none").unwrap();
    let source = LowSprCards::new(base, &fine).unwrap();
    let board = Board::new(fixed("Kh 9h 2c 7s 3d"), &source);
    for (s, row) in board.relative.as_ref().unwrap().iter().enumerate() {
        let width = source.hand_classes([Street::Flop, Street::Turn, Street::River][s])
            * source.flop_recall_classes([Street::Flop, Street::Turn, Street::River][s]);
        let finer = row.low_spr.as_ref().unwrap();
        for &h in &board.sorted {
            assert_eq!(row.class[h] % width as u16, finer.class[h] % width as u16);
            assert!(usize::from(finer.class[h]) < finer.classes);
        }
    }
}

#[test]
fn qre_prior_resumes_bit_identically_across_thread_counts() {
    check_finished_iteration_resume(
        &Relative {
            strength: &Buckets,
            counts: [4; 3],
            levels: 2,
            prior: 10.0,
            prior_fraction: 0.0,
            reach: RelativeReach::Current,
            refinement: Refinement::NONE,
            flops: None,
            hand: HandSplit::default(),
            runout: Runout::default(),
            opponent_levels: [false; 3],
        },
        Some(super::Qre::parse("0.5").unwrap()),
    );
}

#[test]
fn qre_prior_preserves_mass_and_changes_the_class_distribution() {
    let g = game(&config(2, false));
    let m = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let (a, b) = (m.support[0][0], m.support[0][1]);
    let mut path = [0.0; 169];
    path[a] = 2.0_f64.ln();
    let mut reach = [0.0; nlh::NUM_COMBOS];
    super::qre::add_prior(&m, 0, &path, 10.0, &mut reach);
    assert!((reach.iter().sum::<f64>() - 10.0).abs() < 1e-12);
    let catalog = crate::trunk::classes::Classes::get();
    let ratio = reach[catalog.representative(a)] / reach[catalog.representative(b)];
    let expected = 2.0 * m.weights[0][a] / m.weights[0][b];
    assert!((ratio - expected).abs() < 1e-12);
    for bad in ["-1", "NaN", "inf", "0.5:0.1"] {
        assert!(super::Qre::parse(bad).is_err());
    }
}

/// Strength: 3 per board card of a hole card's rank, plus 1 for a pocket
/// pair, so a class's mean strength on a flop follows the board's ranks.
struct BoardPairs;

impl BucketSource for BoardPairs {
    fn count(&self, _street: Street) -> usize {
        16
    }
    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS] {
        std::array::from_fn(|h| {
            let [a, b] = Classes::get().cards(h);
            let hits = board
                .iter()
                .filter(|c| c.rank() == a.rank() || c.rank() == b.rank())
                .count();
            (3 * hits + usize::from(a.rank() == b.rank())) as u16
        })
    }
}

fn class_named(name: &str) -> usize {
    (0..169).find(|&c| Classes::get().name(c) == name).unwrap()
}

#[test]
fn own_flop_groups_split_each_stratum_by_the_weighted_classes_only() {
    let features = FlopFeatures::new(&BoardPairs);
    let ranks = |flop: &[Card; 3], wanted: &[u8]| {
        flop.iter().filter(|c| wanted.contains(&c.rank())).count()
    };
    let distinct = |mut v: [u8; 3]| {
        v.sort_unstable();
        1 + usize::from(v[0] != v[1]) + usize::from(v[1] != v[2])
    };
    let mut only_aces = [0.0; 169];
    only_aces[class_named("AA")] = 1.0;
    let mut only_seven_deuce = [0.0; 169];
    only_seven_deuce[class_named("72o")] = 2.5;
    let cases: [(&[f64; 169], Option<&[u8]>); 3] = [
        (&only_aces, Some(&[12])),
        (&only_seven_deuce, Some(&[5, 0])),
        (&[1.0; 169], None),
    ];
    for (weights, key) in cases {
        let groups = features.groups(weights, OWN_FLOP_GROUPS).unwrap();
        let again = features.groups(weights, OWN_FLOP_GROUPS).unwrap();
        assert_eq!(groups.count(), OWN_FLOP_GROUPS);
        let mut stratum_of = std::collections::HashMap::new();
        let mut key_of = std::collections::HashMap::new();
        let mut per_stratum = std::collections::BTreeMap::<_, std::collections::BTreeSet<_>>::new();
        for a in 0..52_u8 {
            for b in a + 1..52 {
                for c in b + 1..52 {
                    let flop = [a, b, c].map(Card::from_index);
                    let g = groups.group(&flop);
                    assert_eq!(again.group(&flop), g, "equal weights give equal groups");
                    let stratum = (
                        distinct(flop.map(Card::suit)),
                        distinct(flop.map(Card::rank)),
                    );
                    assert_eq!(*stratum_of.entry(g).or_insert(stratum), stratum);
                    per_stratum.entry(stratum).or_default().insert(g);
                    // Each stratum but trips (one group) has groups enough
                    // for every strength of a lone weighted class.
                    if let Some(wanted) = key
                        && stratum != (3, 1)
                    {
                        let k = ranks(&flop, wanted);
                        assert_eq!(*key_of.entry(g).or_insert(k), k, "{flop:?}");
                    }
                }
            }
        }
        // One group per stratum plus its share of raw flops of the other 43.
        let counts: Vec<usize> = per_stratum.values().map(|s| s.len()).collect();
        assert_eq!(counts, [3, 5, 21, 1, 5, 14]);
    }
    assert!(features.groups(&only_aces, 5).is_err());
    assert!(features.groups(&[0.0; 169], OWN_FLOP_GROUPS).is_err());
}

#[test]
fn leaf_weights_average_the_reaching_seats_normalized_combos() {
    let g = game(&config(2, false));
    let tree = Tree::build_with(&g, FlopLeaves::L1).unwrap();
    let model = Model::new(&g, &Synthetic, EvaluationOptions::default()).unwrap();
    let mut profile = Profile::uniform(&tree);
    let range = ["AA", "KK", "AKs", "72o"].map(class_named);
    let share = |c: usize, out: Option<usize>| {
        let total: f64 = range
            .iter()
            .filter(|&&r| Some(r) != out)
            .map(|&r| Classes::get().n(r) as f64)
            .sum();
        if range.contains(&c) && Some(c) != out {
            Classes::get().n(c) as f64 / total
        } else {
            0.0
        }
    };
    let all = leaf_weights(&tree, &model, &profile);
    assert!(!all.is_empty());
    for (_, w) in &all {
        let w = w.unwrap();
        for (c, &x) in w.iter().enumerate() {
            close(x, share(c, None));
        }
    }
    // The first leaf's line and each seat's last decision on it.
    let z = all[0].0;
    let active = tree.nodes[z]
        .terminal
        .as_ref()
        .unwrap()
        .l1
        .as_ref()
        .unwrap()
        .active;
    let mut decisions = std::collections::HashMap::new();
    let mut at = z;
    while let Some((p, a)) = tree.nodes[at].parent {
        decisions
            .entry(tree.nodes[p].actor.unwrap())
            .or_insert((p, a));
        at = p;
    }
    let [(p0, a0), (p1, a1)] = active.map(|s| decisions[&s]);
    let set = |profile: &mut Profile, p: usize, a: usize, c: usize, x: f64| {
        let n = tree.nodes[p].labels.len();
        let row = &mut profile.rows[p][c * n..(c + 1) * n];
        row.fill((1.0 - x) / (n - 1) as f64);
        row[a] = x;
    };
    let aa = range[0];
    // AA never takes the first seat's action: that seat holds no AA at the
    // leaf, the other still does, and each seat counts half.
    set(&mut profile, p0, a0, aa, 0.0);
    let w = leaf_weights(&tree, &model, &profile)[0].1.unwrap();
    for (c, &x) in w.iter().enumerate() {
        close(x, 0.5 * (share(c, Some(aa)) + share(c, None)));
    }
    // The first seat never takes it: the second seat's range alone.
    for &c in &range {
        set(&mut profile, p0, a0, c, 0.0);
    }
    let w = leaf_weights(&tree, &model, &profile)[0].1.unwrap();
    for (c, &x) in w.iter().enumerate() {
        close(x, share(c, None));
    }
    // Neither seat reaches the leaf.
    for &c in &range {
        set(&mut profile, p1, a1, c, 0.0);
    }
    assert!(leaf_weights(&tree, &model, &profile)[0].1.is_none());
}

#[test]
fn leaf_flop_group_option_reads_counts_and_windows_profile_paths() {
    assert_eq!(LeafFlopGroups::parse("none").unwrap(), LeafFlopGroups::None);
    assert_eq!(
        LeafFlopGroups::parse("own:C:/runs/a.json").unwrap(),
        LeafFlopGroups::Own {
            groups: OWN_FLOP_GROUPS,
            profile: "C:/runs/a.json".into()
        }
    );
    assert_eq!(
        LeafFlopGroups::parse("own:25:C:/runs/a.json").unwrap(),
        LeafFlopGroups::Own {
            groups: 25,
            profile: "C:/runs/a.json".into()
        }
    );
    assert_eq!(
        LeafFlopGroups::parse("lists/own.txt").unwrap(),
        LeafFlopGroups::Rows("lists/own.txt".into())
    );
    assert!(LeafFlopGroups::parse("own:").is_err());
    assert!(LeafFlopGroups::parse("own:25:").is_err());
}

#[test]
fn opponent_levels_follow_showdown_equity_against_the_opponent_reach() {
    let catalog = Classes::get();
    let combo = |s: &str| {
        let cards: Vec<Card> = s.split_whitespace().map(|c| c.parse().unwrap()).collect();
        (0..NUM_COMBOS)
            .find(|&h| {
                let [a, b] = catalog.cards(h);
                (a, b) == (cards[0], cards[1]) || (b, a) == (cards[0], cards[1])
            })
            .unwrap()
    };
    let plain = Relative {
        strength: &Buckets,
        counts: [1; 3],
        levels: 4,
        prior: 0.0,
        prior_fraction: 0.0,
        reach: RelativeReach::Current,
        refinement: Refinement::NONE,
        flops: None,
        hand: HandSplit::default(),
        runout: Runout::default(),
        opponent_levels: [false; 3],
    };
    let river = Relative {
        opponent_levels: [false, false, true],
        ..plain
    };
    assert_ne!(plain.state_identity(), river.state_identity());
    let cards = fixed("Kh 9h 2c 7s 3d");
    let board = super::cards::Board::new(cards, &river);
    let strength = &board.relative.as_ref().unwrap()[2];
    let reach = [1.0; NUM_COMBOS];
    let mut opponent = [0.0; NUM_COMBOS];
    opponent[combo("As Ad")] = 1.0;
    opponent[combo("Qs Qh")] = 3.0;
    let mut out = [u16::MAX; NUM_COMBOS];
    super::pass::relative_leaf_buckets(
        strength,
        f64::INFINITY,
        0,
        &reach,
        Some(&opponent),
        &mut out,
    );
    // Equity 0, 3/4, 3/4 (with an ace), 1 (As removes the aces), 0 (Qs removes
    // the queens), and (0 + 3/2) / 4 tying the queens.
    for (hand, level) in [
        ("Jc Jd", 0),
        ("Kc Qc", 3),
        ("Ah Kc", 3),
        ("As Kc", 3),
        ("Kc Qs", 0),
        ("Qc Qd", 1),
    ] {
        assert_eq!(out[combo(hand)], level, "{hand}");
    }
    // No opponent combo meets one holding both a queen and an ace it lacks:
    // the strength-table level stays.
    let static_board = super::cards::Board::new(cards, &plain);
    let mut table = [u16::MAX; NUM_COMBOS];
    super::pass::relative_buckets(
        &static_board.relative.as_ref().unwrap()[2],
        &reach,
        &mut table,
    );
    let blocked = combo("As Qs");
    assert_eq!(out[blocked], table[blocked]);
    assert_eq!(
        strength.class,
        static_board.relative.as_ref().unwrap()[2].class
    );

    // On the turn the equity uses the four-card board: 3c 3h is an underpair
    // there, though the river 3d makes it a set. The pair split keeps its bit.
    let turn = Relative {
        hand: HandSplit::parse("pairs").unwrap(),
        opponent_levels: [false, true, false],
        ..plain
    };
    assert_eq!(turn.hand_classes(Street::Turn), 2);
    let board = super::cards::Board::new(cards, &turn);
    let strength = &board.relative.as_ref().unwrap()[1];
    let mut opponent = [0.0; NUM_COMBOS];
    opponent[combo("As Ad")] = 1.0;
    super::pass::relative_leaf_buckets(
        strength,
        f64::INFINITY,
        0,
        &reach,
        Some(&opponent),
        &mut out,
    );
    for (hand, level, pair) in [
        ("3c 3h", 0, 1),
        ("9c 9d", 3, 1),
        ("Ac Ah", 2, 1),
        ("Kc Qc", 0, 0),
    ] {
        assert_eq!(out[combo(hand)], level * 2 + pair, "{hand}");
    }
}
