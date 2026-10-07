use super::*;
use crate::trunk::classes::{Classes, Ordering3};
use crate::{ExternalSamplingGame, FeatureHashAbstraction, HoldemGame, SeatVec};
use anyhow::Result;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::path::Path;

struct Synthetic;
impl Tables for Synthetic {
    fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
        let k = f64::from(Classes::get().k(c, d));
        let advantage = (d as f64 - c as f64) * 0.001;
        Ok([k * (0.45 + advantage), k * 0.1, k * (0.45 - advantage)])
    }
    fn p3(&self, c: usize, d: usize, e: usize) -> Result<[f64; 13]> {
        let mut p =
            std::array::from_fn(|o| (1 + (c + 3 * d.min(e) + 7 * d.max(e) + 11 * o) % 31) as f64);
        if d == e {
            let original = p;
            for o in 0..13 {
                p[o] = (original[o] + original[Ordering3::ALL[o].swap_opponents().index()]) / 2.0;
            }
        }
        let sum: f64 = p.iter().sum();
        p = p.map(|v| v / sum);
        Ok(if d > e {
            std::array::from_fn(|o| p[Ordering3::ALL[o].swap_opponents().index()])
        } else {
            p
        })
    }
}

#[test]
fn shared_leaves_and_solver_values_match_evaluator() {
    use super::{eval::reaches, leaves::leaf_values, solve::backward_values};
    for players in [3, 4] {
        let game = game(&config(players, true, false));
        let tree = Tree::build(&game).unwrap();
        let profile = random_profile(&tree, 17);
        let model = Model::new(
            &game,
            &Synthetic,
            EvaluationOptions {
                k4_samples: 64,
                ..Default::default()
            },
        )
        .unwrap();
        let reach = reaches(&tree, &profile, &model);
        let all = leaf_values(
            &tree,
            &model,
            &reach,
            &(0..players).collect::<Vec<_>>(),
            super::leaves::K4Plan::model(&model),
        )
        .unwrap();
        let evaluation = evaluate(&tree, &profile, &model).unwrap();
        for p in 0..players {
            let single = leaf_values(
                &tree,
                &model,
                &reach,
                &[p],
                super::leaves::K4Plan::model(&model),
            )
            .unwrap();
            for (a, b) in single.values[p]
                .iter()
                .flatten()
                .zip(all.values[p].iter().flatten())
            {
                assert_eq!(a.to_bits(), b.to_bits());
            }
            let mut values = single.values[p].clone();
            backward_values(&tree, &profile, &model.support[p], p, &mut values);
            let root = model.support[p]
                .iter()
                .map(|&c| Classes::get().n(c) as f64 * model.weights()[p][c] * values[0][c])
                .sum::<f64>()
                / model.normalizers[p];
            let x = evaluation.seats[p].value;
            assert!((root - x).abs() <= 1e-12 * (1.0 + x.abs()));
            for (z, node) in tree
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.actor == Some(p))
            {
                for &c in &model.support[p] {
                    let residual: f64 = node
                        .children
                        .iter()
                        .zip(profile.row(&tree, z, c))
                        .map(|(&child, &s)| s * (values[child][c] - values[z][c]))
                        .sum();
                    let max = node
                        .children
                        .iter()
                        .map(|&child| values[child][c].abs())
                        .fold(values[z][c].abs(), f64::max);
                    assert!(residual.abs() <= 1e-12 * (1.0 + max));
                }
            }
        }
    }
}

fn solver_convergence(players: usize) {
    let game = game(&config(players, false, false));
    let tree = Tree::build(&game).unwrap();
    let model = Model::new(
        &game,
        &Synthetic,
        EvaluationOptions {
            k4_samples: 64,
            ..Default::default()
        },
    )
    .unwrap();
    let solution = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            solve(
                &tree,
                &model,
                SolveOptions {
                    iterations: if players == 2 { 3000 } else { 1000 },
                    eval_every: 250,
                    target_nash_conv: if players == 2 { Some(1e-5) } else { None },
                    ..Default::default()
                },
                |_| {},
            )
            .unwrap()
        });
    println!(
        "{players}-player checkpoints: {:?}",
        solution
            .checkpoints
            .iter()
            .map(|c| (c.iteration, c.nash_conv))
            .collect::<Vec<_>>()
    );
    let final_checkpoint = solution.checkpoints.last().unwrap();
    if players == 2 {
        assert!(solution.reached_target, "{:?}", solution.checkpoints);
    } else {
        assert!(
            final_checkpoint.nash_conv <= 0.02 * solution.checkpoints[0].nash_conv,
            "{:?}",
            solution.checkpoints
        );
    }
    let document: ClassProfileDocument =
        serde_json::from_slice(&serde_json::to_vec(&solution.average.export(&tree)).unwrap())
            .unwrap();
    let restored = Profile::from_json(&tree, &document).unwrap();
    let x = final_checkpoint.nash_conv;
    assert!(
        (evaluate(&tree, &restored, &model).unwrap().nash_conv - x).abs()
            <= 1e-12 * (1.0 + x.abs())
    );
    for (z, node) in tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.actor.is_some())
    {
        let p = node.actor.unwrap();
        for c in 0..169 {
            assert!(!solution.average.is_defaulted(z, c));
            if model.weights()[p][c] == 0.0 {
                assert_eq!(
                    solution.average.row(&tree, z, c),
                    vec![1.0 / node.children.len() as f64; node.children.len()]
                );
            }
        }
    }
}

#[test]
fn trunk_solver_converges_two_players() {
    solver_convergence(2);
}
#[test]
fn trunk_solver_converges_three_players() {
    solver_convergence(3);
}
#[test]
fn trunk_solver_converges_four_players() {
    solver_convergence(4);
}

#[test]
fn trunk_solver_determinism_checkpoints_and_validation() {
    let game = game(&config(4, true, false).replace("remove call", ""));
    let tree = Tree::build(&game).unwrap();
    let model = Model::new(
        &game,
        &Synthetic,
        EvaluationOptions {
            k4_samples: 64,
            ..Default::default()
        },
    )
    .unwrap();
    let options = SolveOptions {
        iterations: 9,
        eval_every: 4,
        ..Default::default()
    };
    for options in [
        options,
        SolveOptions {
            k4_samples: Some(32),
            ..options
        },
        SolveOptions {
            k4_samples: Some(32),
            k4_min_samples: Some(4),
            ..options
        },
    ] {
        let run = |threads| {
            let mut observed = Vec::new();
            let result = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| {
                    solve(&tree, &model, options, |p| {
                        observed.push((p.iteration, p.checkpoint.map(|c| c.iteration)))
                    })
                    .unwrap()
                });
            assert_eq!(
                observed,
                vec![
                    (0, Some(0)),
                    (1, None),
                    (2, None),
                    (3, None),
                    (4, Some(4)),
                    (5, None),
                    (6, None),
                    (7, None),
                    (8, Some(8)),
                    (9, Some(9))
                ]
            );
            result
        };
        let one = run(1);
        for other in [run(4), run(1)] {
            for (z, _) in tree
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.actor.is_some())
            {
                for c in 0..169 {
                    assert_eq!(
                        one.average
                            .row(&tree, z, c)
                            .iter()
                            .map(|x| x.to_bits())
                            .collect::<Vec<_>>(),
                        other
                            .average
                            .row(&tree, z, c)
                            .iter()
                            .map(|x| x.to_bits())
                            .collect::<Vec<_>>()
                    );
                }
            }
            assert_eq!(one.checkpoints.len(), other.checkpoints.len());
            for (a, b) in one.checkpoints.iter().zip(other.checkpoints) {
                assert_eq!(a.iteration, b.iteration);
                assert_eq!(a.nash_conv.to_bits(), b.nash_conv.to_bits());
                for (a, b) in a.seats.iter().zip(b.seats) {
                    assert_eq!(a.seat, b.seat);
                    assert_eq!(a.value.to_bits(), b.value.to_bits());
                    assert_eq!(a.best_response.to_bits(), b.best_response.to_bits());
                    assert_eq!(a.gain.to_bits(), b.gain.to_bits());
                }
            }
        }
    }
    let end_only = solve(
        &tree,
        &model,
        SolveOptions {
            iterations: 2,
            eval_every: 0,
            ..options
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        end_only
            .checkpoints
            .iter()
            .map(|c| c.iteration)
            .collect::<Vec<_>>(),
        vec![2]
    );
    let initial_target = solve(
        &tree,
        &model,
        SolveOptions {
            target_nash_conv: Some(100.0),
            ..options
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(initial_target.iterations, 0);
    assert!(initial_target.reached_target);
    for invalid in [
        SolveOptions {
            k4_samples: Some(0),
            ..options
        },
        SolveOptions {
            k4_samples: Some(32),
            k4_min_samples: Some(0),
            ..options
        },
        SolveOptions {
            k4_samples: Some(32),
            k4_min_samples: Some(33),
            ..options
        },
        SolveOptions {
            k4_min_samples: Some(1),
            ..options
        },
        SolveOptions {
            iterations: 0,
            ..options
        },
        SolveOptions {
            alpha: f64::NAN,
            ..options
        },
        SolveOptions {
            beta: f64::INFINITY,
            ..options
        },
        SolveOptions {
            gamma: -1.0,
            ..options
        },
        SolveOptions {
            gamma: f64::NAN,
            ..options
        },
        SolveOptions {
            target_nash_conv: Some(-1.0),
            ..options
        },
        SolveOptions {
            target_nash_conv: Some(f64::NAN),
            ..options
        },
    ] {
        assert!(solve(&tree, &model, invalid, |_| {}).is_err());
    }
    let other = self::game(&config(3, false, false));
    assert!(solve(&Tree::build(&other).unwrap(), &model, options, |_| {}).is_err());
}

#[test]
#[ignore = "release acceptance: deterministic DCFR with real HU tables on B1"]
fn trunk_solver_meets_b1_target() {
    use crate::trunk::tables::HuShowdownTable;
    struct HuOnly(HuShowdownTable);
    impl Tables for HuOnly {
        fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
            self.0.t2(c, d)
        }
        fn p3(&self, _: usize, _: usize, _: usize) -> Result<[f64; 13]> {
            anyhow::bail!("heads-up cannot reach three-way terminals")
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tables = HuOnly(HuShowdownTable::load_or_build(&root.join(".cache/p2-trunk")).unwrap());
    for stack in [5, 10, 20] {
        let path = root.join(format!("examples/bench/hu_pushfold_{stack}bb.toml"));
        let game = game_from_config(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
        let tree = Tree::build(&game).unwrap();
        let model = Model::new(&game, &tables, EvaluationOptions::default()).unwrap();
        let solution = solve(
            &tree,
            &model,
            SolveOptions {
                iterations: 100_000,
                eval_every: 10,
                target_nash_conv: Some(1e-4),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let checkpoint = solution.checkpoints.last().unwrap();
        println!(
            "B1 {stack}bb: {} iterations, NashConv {}, {}s",
            solution.iterations, checkpoint.nash_conv, checkpoint.seconds
        );
        assert!(solution.reached_target, "B1 {stack}bb: {checkpoint:?}");
    }
}

fn config(players: usize, rake: bool, limp: bool) -> String {
    let mut raw = format!(
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = {players}\n[table.stacks_bb]\nBTN = 3\n"
    );
    if players == 2 {
        raw.push_str("BB = 2\n");
    } else {
        raw.push_str("SB = 2\nBB = 1\n");
    }
    if players >= 4 {
        raw.push_str("CO = 3\n");
    }
    raw.push_str("[ranges]\nBTN = 'AA,AKs,KK'\nBB = 'AA,KK,QQ'\n");
    if players >= 3 {
        raw.push_str("SB = 'AKs,KK,QQ'\n");
    }
    if players >= 4 {
        raw.push_str("CO = 'AA,QQ'\n");
    }
    if rake {
        raw.push_str("[economics.rake]\nrate = 0.05\n");
    }
    let preflop = if limp {
        "preflop { remove raise }"
    } else {
        "preflop { when unopened { replace raise [a] remove call } when aggressions >= 1 { remove raise } }"
    };
    raw.push_str(&format!("[tree]\nscript = '''\n{preflop}\nflop, turn, river when players >= 2 {{ checkdown }}\n'''\n"));
    raw
}

fn game(raw: &str) -> HoldemGame<FeatureHashAbstraction> {
    game_from_config(raw, Path::new("unit.toml")).unwrap()
}

fn random_profile(tree: &Tree, seed: u64) -> Profile {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut doc = Profile::uniform(tree).export(tree);
    for node in &mut doc.nodes {
        for row in &mut node.probabilities {
            for p in row {
                *p = rng.gen_range(0.01..1.0);
            }
        }
    }
    Profile::from_json(tree, &doc).unwrap()
}

/// Independent reference: enumerate complete opponent class tuples and walk
/// every root-to-leaf path to multiply each opponent's own action reach. No
/// factorized masses, batched contraction, or evaluator backward code is used.
/// Only the public tree/profile, table interface and game's settlement are shared.
fn reference(tree: &Tree, profile: &Profile, model: &Model<'_>) -> Vec<(f64, f64)> {
    fn tuples(
        seat: usize,
        p: usize,
        support: &[Vec<usize>],
        current: &mut Vec<usize>,
        out: &mut Vec<Vec<usize>>,
    ) {
        if seat == support.len() {
            out.push(current.clone());
            return;
        }
        if seat == p {
            current.push(0);
            tuples(seat + 1, p, support, current, out);
            current.pop();
        } else {
            for &d in &support[seat] {
                current.push(d);
                tuples(seat + 1, p, support, current, out);
                current.pop();
            }
        }
    }
    fn backup(
        tree: &Tree,
        profile: &Profile,
        leaf: &[f64],
        node: usize,
        p: usize,
        c: usize,
    ) -> (f64, f64) {
        let n = &tree.nodes[node];
        if n.terminal.is_some() {
            return (leaf[node], leaf[node]);
        }
        let children: Vec<_> = n
            .children
            .iter()
            .map(|&child| backup(tree, profile, leaf, child, p, c))
            .collect();
        if n.actor == Some(p) {
            (
                children
                    .iter()
                    .zip(profile.row(tree, node, c))
                    .map(|(v, &s)| s * v.0)
                    .sum(),
                children
                    .iter()
                    .map(|v| v.1)
                    .fold(f64::NEG_INFINITY, f64::max),
            )
        } else {
            (
                children.iter().map(|v| v.0).sum(),
                children.iter().map(|v| v.1).sum(),
            )
        }
    }
    let catalog = Classes::get();
    let support: Vec<Vec<_>> = model
        .weights()
        .iter()
        .map(|w| (0..169).filter(|&c| w[c] > 0.0).collect())
        .collect();
    (0..tree.seats)
        .map(|p| {
            let mut all = Vec::new();
            tuples(0, p, &support, &mut Vec::new(), &mut all);
            let mut u = 0.0;
            let mut br = 0.0;
            let mut normalizer = 0.0;
            for &c in &support[p] {
                let own = catalog.n(c) as f64 * model.weights()[p][c];
                let mut leaf = vec![0.0; tree.nodes.len()];
                for tuple in &all {
                    let weight = own
                        * (0..tree.seats)
                            .filter(|&j| j != p)
                            .map(|j| {
                                f64::from(catalog.k(c, tuple[j])) * model.weights()[j][tuple[j]]
                            })
                            .product::<f64>();
                    normalizer += weight;
                    for (z, node) in tree.nodes.iter().enumerate() {
                        let Some(t) = &node.terminal else { continue };
                        let mut reach = 1.0;
                        let mut cursor = z;
                        while let Some((parent, a)) = tree.nodes[cursor].parent {
                            let actor = tree.nodes[parent].actor.unwrap();
                            if actor != p {
                                reach *= profile.row(tree, parent, tuple[actor])[a];
                            }
                            cursor = parent;
                        }
                        if reach == 0.0 || weight == 0.0 {
                            continue;
                        }
                        let payoff = if !t.active.contains(&p) || t.active.len() == 1 {
                            t.payoffs[0][p]
                        } else {
                            let others: Vec<_> =
                                t.active.iter().copied().filter(|&j| j != p).collect();
                            let ranks: Vec<Vec<u16>>;
                            let probabilities: Vec<f64>;
                            if t.active.len() == 2 {
                                let d = tuple[others[0]];
                                probabilities = model
                                    .tables()
                                    .t2(c, d)
                                    .unwrap()
                                    .iter()
                                    .map(|&v| v / f64::from(catalog.k(c, d)))
                                    .collect();
                                ranks = vec![vec![2, 1], vec![1, 1], vec![1, 2]];
                            } else {
                                probabilities = model
                                    .tables()
                                    .p3(c, tuple[others[0]], tuple[others[1]])
                                    .unwrap()
                                    .to_vec();
                                ranks = Ordering3::ALL.iter().map(|o| o.ranks().to_vec()).collect();
                            }
                            ranks
                                .iter()
                                .zip(probabilities)
                                .map(|(r, prob)| {
                                    let seat_ranks = SeatVec::try_new(
                                        (0..tree.seats)
                                            .map(|j| {
                                                if j == p {
                                                    Some(r[0])
                                                } else {
                                                    others
                                                        .iter()
                                                        .position(|&s| s == j)
                                                        .map(|a| r[a + 1])
                                                }
                                            })
                                            .collect(),
                                    )
                                    .unwrap();
                                    prob * model
                                        .game()
                                        .l0_ranked_utilities(&t.state, &seat_ranks)
                                        .unwrap()[p]
                                })
                                .sum()
                        };
                        leaf[z] += weight * reach * payoff;
                    }
                }
                let values = backup(tree, profile, &leaf, 0, p, c);
                u += values.0;
                br += values.1;
            }
            (u / normalizer, br / normalizer)
        })
        .collect()
}

fn close(a: f64, b: f64) {
    assert!(
        (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs())),
        "{a} != {b}"
    );
}

#[test]
fn factorization_matches_tuple_enumeration_side_pots_rake_and_orientation() {
    for players in [2, 3] {
        for rake in [false, true] {
            for limp in [false, true] {
                let game = game(&config(players, rake, limp));
                let tree = Tree::build(&game).unwrap();
                let model = Model::new(&game, &Synthetic, EvaluationOptions::default()).unwrap();
                for profile in [Profile::uniform(&tree), random_profile(&tree, 17)] {
                    let actual = evaluate(&tree, &profile, &model).unwrap();
                    let expected = reference(&tree, &profile, &model);
                    for (a, (u, br)) in actual.seats.iter().zip(expected) {
                        close(a.value, u);
                        close(a.best_response, br);
                        close(a.gain, br - u);
                        assert!(a.gain >= 0.0);
                        close(a.telescoping_residual, 0.0);
                        close(a.reach_by_active_count.iter().sum(), 1.0);
                        close(a.value_by_active_count.iter().sum(), a.value);
                        assert_eq!(a.value_by_active_count.len(), players + 1);
                        assert_eq!(a.value_by_active_count[0], 0.0);
                    }
                    if players == 2 && !rake {
                        close(actual.seats.iter().map(|s| s.value).sum(), 0.0);
                    }
                }
            }
        }
    }
    let a = Synthetic.p3(0, 1, 14).unwrap();
    let b = Synthetic.p3(0, 14, 1).unwrap();
    assert_ne!(a, b);
    for o in 0..13 {
        close(a[o], b[Ordering3::ALL[o].swap_opponents().index()]);
    }
}

#[test]
fn sampled_four_way_br_and_thread_determinism() {
    let game = game(&config(4, true, false));
    let tree = Tree::build(&game).unwrap();
    assert!(tree.terminal_counts()[4] > 0);
    let profile = random_profile(&tree, 42);
    let model = Model::new(
        &game,
        &Synthetic,
        EvaluationOptions {
            k4_samples: 64,
            ..Default::default()
        },
    )
    .unwrap();
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| evaluate(&tree, &profile, &model).unwrap())
    };
    let first = run(1);
    assert_eq!(first.seats, run(1).seats);
    assert_eq!(first.seats, run(4).seats);
    assert_eq!(first.best_response_actions, run(4).best_response_actions);
    assert!(first.seats.iter().all(|s| s.best_response >= s.value));
    for s in first.seats {
        close(s.telescoping_residual, 0.0);
        close(s.reach_by_active_count.iter().sum(), 1.0);
        close(s.value_by_active_count.iter().sum(), s.value);
    }
}

#[test]
fn exported_pure_best_responses_attain_l0_values() {
    for players in [3, 4] {
        let game = game(&config(players, true, false));
        let tree = Tree::build(&game).unwrap();
        let mut document = random_profile(&tree, 72).export(&tree);
        // Include profile-unreachable nodes: their BR actions still matter.
        document.nodes[0].probabilities.iter_mut().for_each(|row| {
            row.fill(0.0);
            row[0] = 1.0;
        });
        let profile = Profile::from_json(&tree, &document).unwrap();
        let model = Model::new(
            &game,
            &Synthetic,
            EvaluationOptions {
                k4_samples: 64,
                ..Default::default()
            },
        )
        .unwrap();
        let original = evaluate(&tree, &profile, &model).unwrap();
        for i in 0..players {
            let pure = profile
                .with_pure_rows(&tree, i, &original.best_response_actions[i])
                .unwrap();
            close(
                evaluate(&tree, &pure, &model).unwrap().seats[i].value,
                original.seats[i].best_response,
            );
            for (z, node) in tree.nodes.iter().enumerate() {
                for c in 0..169 {
                    let a = original.best_response_actions[i][z * 169 + c];
                    if node.actor == Some(i) {
                        assert!(!pure.is_defaulted(z, c));
                        assert_eq!(pure.row(&tree, z, c)[usize::from(a)], 1.0);
                        if model.weights()[i][c] == 0.0 {
                            assert_eq!(a, 0);
                        }
                    } else {
                        assert_eq!(a, u8::MAX);
                    }
                }
            }
        }
    }
}

/// Independent physical reference: recursively walk every public path, keep
/// each seat's own profile/BR factors separately, and invoke card-based game
/// settlement at every leaf. No prepared awards, arena pruning, rank reuse,
/// joint-reach recurrence, or statistical accumulators from real.rs are used.
fn real_deal_reference(
    tree: &Tree,
    profile: &Profile,
    responses: &[Vec<u8>],
    game: &HoldemGame<FeatureHashAbstraction>,
    world: &crate::SampledWorld,
) -> (Vec<f64>, Vec<Vec<f64>>, Vec<f64>) {
    struct Reference<'a> {
        tree: &'a Tree,
        profile: &'a Profile,
        responses: &'a [Vec<u8>],
        game: &'a HoldemGame<FeatureHashAbstraction>,
        world: &'a crate::SampledWorld,
        values: Vec<f64>,
        by_count: Vec<Vec<f64>>,
        best: Vec<f64>,
    }
    impl Reference<'_> {
        fn visit(&mut self, z: usize, pi: &[f64], beta: &[f64]) {
            let node = &self.tree.nodes[z];
            if let Some(t) = &node.terminal {
                let payoff = self.game.real_reference_utilities(
                    &self.game.settle_terminal(&t.state, self.world).unwrap(),
                );
                let reach: f64 = pi.iter().product();
                for i in 0..self.tree.seats {
                    let value = reach * payoff[i];
                    self.values[i] += value;
                    self.by_count[i][t.active.len()] += value;
                    self.best[i] += beta[i]
                        * pi.iter()
                            .enumerate()
                            .filter(|&(j, _)| j != i)
                            .map(|(_, p)| p)
                            .product::<f64>()
                        * payoff[i];
                }
                return;
            }
            let actor = node.actor.unwrap();
            let c = crate::trunk::classes::class(self.world.hole_combo(actor));
            for (a, &child) in node.children.iter().enumerate() {
                let mut pi = pi.to_vec();
                let mut beta = beta.to_vec();
                pi[actor] *= self.profile.row(self.tree, z, c)[a];
                let action = self.responses[actor][z * 169 + c];
                beta[actor] *= if action == FOLLOW {
                    self.profile.row(self.tree, z, c)[a]
                } else {
                    f64::from(a == usize::from(action))
                };
                self.visit(child, &pi, &beta);
            }
        }
    }
    let mut reference = Reference {
        tree,
        profile,
        responses,
        game,
        world,
        values: vec![0.0; tree.seats],
        by_count: vec![vec![0.0; tree.seats + 1]; tree.seats],
        best: vec![0.0; tree.seats],
    };
    reference.visit(0, &vec![1.0; tree.seats], &vec![1.0; tree.seats]);
    (reference.values, reference.by_count, reference.best)
}

fn random_responses(tree: &Tree, rng: &mut ChaCha8Rng) -> Vec<Vec<u8>> {
    (0..tree.seats)
        .map(|i| {
            let mut actions = vec![u8::MAX; tree.nodes.len() * 169];
            for (z, node) in tree
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.actor == Some(i))
            {
                for c in 0..169 {
                    actions[z * 169 + c] = rng.gen_range(0..node.children.len()) as u8;
                }
            }
            actions
        })
        .collect()
}

#[test]
fn physical_deal_values_match_independent_recursive_settlement() {
    let strict = |a: f64, b: f64| {
        assert!((a - b).abs() <= 1e-12 * (1.0 + b.abs()), "{a} != {b}");
    };
    for players in [3, 4] {
        let game = game(&config(players, players == 4, false));
        let tree = Tree::build(&game).unwrap();
        let profile = random_profile(&tree, 97);
        let mut rng = ChaCha8Rng::seed_from_u64(104);
        let mut responses = vec![
            random_responses(&tree, &mut rng),
            random_responses(&tree, &mut rng),
        ];
        let mut following = random_responses(&tree, &mut rng);
        for (i, actions) in following.iter_mut().enumerate() {
            for (z, _) in tree
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.actor == Some(i))
            {
                for c in 0..169 {
                    if rng.gen_bool(0.3) {
                        actions[z * 169 + c] = FOLLOW;
                    }
                }
            }
        }
        responses.push(following);
        let sampler = game.deal_sampler().unwrap();
        let prepared = super::real::Prepared::new(&tree, &game).unwrap();
        let mut scratch = super::real::Scratch::new(&tree, responses.len());
        let mut doc = profile.export(&tree);
        for node in &mut doc.nodes {
            for row in &mut node.probabilities {
                let a = rng.gen_range(0..row.len());
                row.fill(0.0);
                row[a] = 1.0;
            }
        }
        let pure = Profile::from_json(&tree, &doc).unwrap();
        for d in 0..200 {
            let world = sampler.sample(&mut rng).unwrap();
            // Dense random profile on all 200 worlds; also exercise zero-reach
            // subtree pruning and off-profile BR paths on the first 20.
            for profile in std::iter::once(&profile).chain((d < 20).then_some(&pure)) {
                scratch.deal(&tree, profile, &responses, &prepared, &world);
                for (set, responses) in responses.iter().enumerate() {
                    let (v, k, b) = real_deal_reference(&tree, profile, responses, &game, &world);
                    for i in 0..players {
                        strict(scratch.values[i], v[i]);
                        strict(scratch.best[set][i], b[i]);
                        for (active, &expected) in k[i].iter().enumerate() {
                            strict(scratch.by_count[i][active], expected);
                        }
                        strict(scratch.by_count[i].iter().sum(), scratch.values[i]);
                    }
                }
                if players == 3 {
                    strict(scratch.values[..players].iter().sum(), 0.0);
                }
            }
        }
    }
}

#[test]
fn physical_deal_estimates_are_thread_independent_and_validate_inputs() {
    let game = game(&config(4, true, false));
    let tree = Tree::build(&game).unwrap();
    let profile = random_profile(&tree, 75);
    let responses = random_responses(&tree, &mut ChaCha8Rng::seed_from_u64(97));
    let options = RealOptions {
        deals: 20_000,
        seed: 18,
    };
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                evaluate_real(
                    &tree,
                    &profile,
                    std::slice::from_ref(&responses),
                    &game,
                    options,
                )
                .unwrap()
            })
    };
    let one = run(1);
    assert_eq!(one, run(4));
    assert_eq!(one.deals, options.deals);
    assert_eq!(one.seed, options.seed);
    assert!(one.mean_deal_attempts >= 1.0);
    for s in &one.seats {
        close(
            s.value_by_active_count.iter().map(|v| v.mean).sum(),
            s.value.mean,
        );
        close(
            s.responses[0].value.mean - s.value.mean,
            s.responses[0].gain.mean,
        );
        assert!(s.value.stderr > 0.0);
        assert_eq!(
            s.value_by_active_count[0],
            Estimate {
                mean: 0.0,
                stderr: 0.0
            }
        );
    }
    let small = RealOptions {
        deals: 23,
        ..options
    };
    let zero = evaluate_real(&tree, &profile, &[], &game, small).unwrap();
    assert!(zero.response_gain_sums.is_empty());
    assert!(zero.seats.iter().all(|s| s.responses.is_empty()));
    let four = vec![responses.clone(); 6];
    let repeated = evaluate_real(&tree, &profile, &four, &game, small).unwrap();
    assert_eq!(zero.value_sum, repeated.value_sum);
    for (z, r) in zero.seats.iter().zip(&repeated.seats) {
        assert_eq!(z.value, r.value);
        assert_eq!(z.value_by_active_count, r.value_by_active_count);
        assert!(r.responses.iter().all(|s| *s == r.responses[0]));
    }
    assert!(evaluate_real(&tree, &profile, &vec![responses.clone(); 7], &game, small).is_err());
    let mut bad_second = vec![responses.clone(); 2];
    bad_second[1][0].pop();
    assert!(evaluate_real(&tree, &profile, &bad_second, &game, small).is_err());
    assert!(
        evaluate_real(
            &tree,
            &profile,
            std::slice::from_ref(&responses),
            &game,
            RealOptions {
                deals: 1,
                ..options
            }
        )
        .is_err()
    );
    assert!(evaluate_real(&tree, &profile, &[responses[..3].to_vec()], &game, options).is_err());
    let mut bad = responses.clone();
    bad[0].pop();
    assert!(evaluate_real(&tree, &profile, std::slice::from_ref(&bad), &game, options).is_err());
    let actor = tree.nodes[0].actor.unwrap();
    let mut bad = responses.clone();
    bad[actor][0] = u8::MAX;
    assert!(evaluate_real(&tree, &profile, std::slice::from_ref(&bad), &game, options).is_err());
    assert!(profile.with_pure_rows(&tree, actor, &bad[actor]).is_err());
    bad[actor][0] = tree.nodes[0].children.len() as u8;
    assert!(evaluate_real(&tree, &profile, std::slice::from_ref(&bad), &game, small).is_err());
    bad[actor][0] = FOLLOW;
    assert!(evaluate_real(&tree, &profile, std::slice::from_ref(&bad), &game, small).is_ok());
    assert!(
        profile
            .with_pure_rows(&tree, tree.seats, &responses[0])
            .is_err()
    );
    assert!(
        profile
            .with_pure_rows(&tree, 0, &responses[0][..169])
            .is_err()
    );
}

#[test]
fn physical_estimates_match_direct_sample_statistics_and_seed_stream() {
    let game = game(&config(4, true, false));
    let tree = Tree::build(&game).unwrap();
    let profile = random_profile(&tree, 39);
    let responses = random_responses(&tree, &mut ChaCha8Rng::seed_from_u64(87));
    let options = RealOptions {
        deals: 23,
        seed: 52,
    };
    let actual = evaluate_real(
        &tree,
        &profile,
        std::slice::from_ref(&responses),
        &game,
        options,
    )
    .unwrap();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.p2.trunk.l0.real.v1");
    hasher.update(&options.seed.to_le_bytes());
    let sampler = game.deal_sampler().unwrap();
    let mut samples = Vec::new();
    let mut attempts = 0_u64;
    for d in 0..options.deals {
        let mut rng = ChaCha8Rng::from_seed(*hasher.finalize().as_bytes());
        rng.set_stream(d);
        let sample = sampler.sample_counted(&mut rng).unwrap();
        attempts += u64::from(sample.attempts);
        samples.push(real_deal_reference(
            &tree,
            &profile,
            &responses,
            &game,
            &sample.world,
        ));
    }
    let check = |estimate: &Estimate, values: Vec<f64>| {
        let mean = values.iter().sum::<f64>() / options.deals as f64;
        let stderr = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
            / (options.deals - 1) as f64
            / options.deals as f64)
            .sqrt();
        assert!((estimate.mean - mean).abs() <= 1e-12 * (1.0 + mean.abs()));
        assert!((estimate.stderr - stderr).abs() <= 1e-12 * (1.0 + stderr));
    };
    for i in 0..tree.seats {
        check(
            &actual.seats[i].value,
            samples.iter().map(|s| s.0[i]).collect(),
        );
        check(
            &actual.seats[i].responses[0].value,
            samples.iter().map(|s| s.2[i]).collect(),
        );
        check(
            &actual.seats[i].responses[0].gain,
            samples.iter().map(|s| s.2[i] - s.0[i]).collect(),
        );
        for k in 0..=tree.seats {
            check(
                &actual.seats[i].value_by_active_count[k],
                samples.iter().map(|s| s.1[i][k]).collect(),
            );
        }
    }
    check(
        &actual.value_sum,
        samples.iter().map(|s| s.0.iter().sum()).collect(),
    );
    check(
        &actual.response_gain_sums[0],
        samples
            .iter()
            .map(|s| (0..tree.seats).map(|i| s.2[i] - s.0[i]).sum())
            .collect(),
    );
    assert_eq!(
        actual.mean_deal_attempts,
        attempts as f64 / options.deals as f64
    );
}

#[test]
#[ignore = "release acceptance: real HU tables and 2^20 physical deals"]
fn real_matches_l0_in_heads_up() {
    use crate::trunk::tables::HuShowdownTable;
    struct HuOnly(HuShowdownTable);
    impl Tables for HuOnly {
        fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
            self.0.t2(c, d)
        }
        fn p3(&self, _: usize, _: usize, _: usize) -> Result<[f64; 13]> {
            anyhow::bail!("heads-up cannot reach three-way terminals")
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/bench/hu_pushfold_10bb.toml");
    let game = game_from_config(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    let tree = Tree::build(&game).unwrap();
    let tables = HuOnly(HuShowdownTable::load_or_build(&root.join(".cache/p2-trunk")).unwrap());
    let profile = random_profile(&tree, 101);
    let model = Model::new(&game, &tables, EvaluationOptions::default()).unwrap();
    let l0 = evaluate(&tree, &profile, &model).unwrap();
    let real = evaluate_real(
        &tree,
        &profile,
        std::slice::from_ref(&l0.best_response_actions),
        &game,
        RealOptions {
            deals: 1 << 20,
            seed: 31,
        },
    )
    .unwrap();
    for (l, r) in l0.seats.iter().zip(&real.seats) {
        assert!((r.value.mean - l.value).abs() <= 5.0 * r.value.stderr);
        assert!((r.responses[0].gain.mean - l.gain).abs() <= 5.0 * r.responses[0].gain.stderr);
        println!(
            "seat {}: L0 value {}, real {:?}; L0 gain {}, real {:?}",
            l.seat, l.value, r.value, l.gain, r.responses[0].gain
        );
        assert_eq!(r.value_by_active_count.len(), 3);
        close(
            r.value_by_active_count.iter().map(|v| v.mean).sum(),
            r.value.mean,
        );
    }
}

#[test]
fn fit_matches_evaluation_on_the_same_deals() {
    for players in [3, 4] {
        // Give BB a decision instead of posting its entire stack as a blind.
        let game = game(&config(players, players == 4, false).replace("BB = 1", "BB = 2"));
        let tree = Tree::build(&game).unwrap();
        let profile = random_profile(&tree, 97);
        let options = RealOptions {
            deals: 3_000,
            seed: 13,
        };
        let fit = fit_real_responses(&tree, &profile, &game, options, &[]).unwrap();
        let real = evaluate_real(
            &tree,
            &profile,
            std::slice::from_ref(&fit.actions),
            &game,
            options,
        )
        .unwrap();
        for (f, r) in fit.seats.iter().zip(&real.seats) {
            close(r.responses[0].value.mean, f.best_response);
            close(r.value.mean, f.value);
            close(r.responses[0].gain.mean, f.gain);
            assert!(f.gain >= -1e-12);
        }
        // Absent classes choose the first action at every own decision, and
        // all other cells keep the non-actor sentinel (including terminals).
        let model = Model::new(&game, &Synthetic, EvaluationOptions::default()).unwrap();
        for (i, actions) in fit.actions.iter().enumerate() {
            for (z, node) in tree.nodes.iter().enumerate() {
                for c in 0..169 {
                    if node.actor != Some(i) {
                        assert_eq!(actions[z * 169 + c], u8::MAX);
                    } else if model.weights()[i][c] == 0.0 {
                        assert_eq!(actions[z * 169 + c], 0);
                    }
                }
            }
        }
        let json = serde_json::to_value(&fit).unwrap();
        assert!(json.get("actions").is_none());
        assert_eq!(json["deals"], options.deals);
    }
}

#[test]
fn all_follow_has_exactly_zero_gain() {
    let game = game(&config(4, true, false));
    let tree = Tree::build(&game).unwrap();
    let profile = random_profile(&tree, 39);
    let actions = vec![vec![FOLLOW; tree.nodes.len() * 169]; tree.seats];
    let real = evaluate_real(
        &tree,
        &profile,
        &[actions],
        &game,
        RealOptions {
            deals: 8192,
            seed: 52,
        },
    )
    .unwrap();
    let zero = Estimate {
        mean: 0.0,
        stderr: 0.0,
    };
    for seat in &real.seats {
        assert_eq!(seat.responses[0].value, seat.value);
        assert_eq!(seat.responses[0].gain, zero);
    }
    assert_eq!(real.response_gain_sums[0], zero);
}

#[test]
fn gated_pass_matches_recursive_reference() {
    struct Reference<'a> {
        tree: &'a Tree,
        profile: &'a Profile,
        indices: &'a [usize],
        total: &'a [f64],
        lanes: &'a [Vec<f64>],
        seat: usize,
        class: usize,
        threshold: f64,
        actions: Vec<u8>,
    }
    impl Reference<'_> {
        fn visit(&mut self, z: usize) -> (f64, [f64; 16]) {
            let node = &self.tree.nodes[z];
            if node.terminal.is_some() {
                let t = self.indices[z];
                return (self.total[t], std::array::from_fn(|l| self.lanes[l][t]));
            }
            let children: Vec<_> = node
                .children
                .iter()
                .map(|&child| self.visit(child))
                .collect();
            if node.actor != Some(self.seat) {
                return (
                    children.iter().map(|v| v.0).sum(),
                    std::array::from_fn(|l| children.iter().map(|v| v.1[l]).sum()),
                );
            }
            let mut action = 0;
            let mut maximum = f64::NEG_INFINITY;
            for (a, value) in children.iter().enumerate() {
                if value.0 > maximum {
                    maximum = value.0;
                    action = a;
                }
            }
            let row = self.profile.row(self.tree, z, self.class);
            let follow: f64 = children.iter().zip(row).map(|(v, p)| p * v.0).sum();
            let lane_follow: [f64; 16] =
                std::array::from_fn(|l| children.iter().zip(row).map(|(v, p)| p * v.1[l]).sum());
            let advantages: Vec<_> = (0..16)
                .map(|l| children[action].1[l] - lane_follow[l])
                .collect();
            let mean = advantages.iter().sum::<f64>() / 16.0;
            let se =
                (16.0 / 15.0 * advantages.iter().map(|a| (a - mean).powi(2)).sum::<f64>()).sqrt();
            if maximum - follow > self.threshold * se {
                self.actions[z * 169 + self.class] = action as u8;
                children[action]
            } else {
                self.actions[z * 169 + self.class] = FOLLOW;
                (follow, lane_follow)
            }
        }
    }
    let game = game(&config(3, false, false).replace("BB = 1", "BB = 2"));
    let tree = Tree::build(&game).unwrap();
    let profile = Profile::uniform(&tree);
    let random = random_profile(&tree, 712);
    let mut terminals = 0;
    let indices: Vec<_> = tree
        .nodes
        .iter()
        .map(|n| {
            if n.terminal.is_some() {
                let t = terminals;
                terminals += 1;
                t
            } else {
                usize::MAX
            }
        })
        .collect();
    let mut rng = ChaCha8Rng::seed_from_u64(881);
    for case in 0..3 {
        let mut lanes = vec![vec![0.0; terminals]; 16];
        if case == 1 {
            for (t, x) in lanes[7].iter_mut().enumerate() {
                *x = (t % 11) as f64 - 5.0;
            }
        } else if case == 2 {
            for lane in &mut lanes {
                for x in lane {
                    *x = rng.gen_range(-5.0..5.0);
                }
            }
        }
        let mut total = lanes[0].clone();
        for lane in &lanes[1..] {
            for (x, y) in total.iter_mut().zip(lane) {
                *x += y;
            }
        }
        let profile = if case == 2 { &random } else { &profile };
        let mut pass = super::real::GatedPass::new(&tree, profile, &indices);
        for seat in 0..tree.seats {
            for class in [0, 73, 168] {
                for threshold in [0.0, 1.0, 2.0, 1e9] {
                    let mut actions = vec![u8::MAX; tree.nodes.len() * 169];
                    let (value, deviations) = pass.run(
                        seat,
                        class,
                        &total,
                        std::array::from_fn(|l| lanes[l].as_slice()),
                        threshold,
                        &mut actions,
                    );
                    let mut reference = Reference {
                        tree: &tree,
                        profile,
                        indices: &indices,
                        total: &total,
                        lanes: &lanes,
                        seat,
                        class,
                        threshold,
                        actions: vec![u8::MAX; tree.nodes.len() * 169],
                    };
                    let expected = reference.visit(0).0;
                    assert_eq!(actions, reference.actions);
                    assert!((value - expected).abs() <= 1e-12 * (1.0 + expected.abs()));
                    assert_eq!(
                        deviations,
                        actions
                            .iter()
                            .filter(|&&a| a != FOLLOW && a != u8::MAX)
                            .count() as u64
                    );
                    if case == 0 || (case == 1 && threshold >= 1.0) {
                        assert_eq!(deviations, 0);
                    }
                }
            }
        }
    }
}

#[test]
fn gated_responses_match_evaluation_on_the_same_deals() {
    for players in [3, 4] {
        let game = game(&config(players, players == 4, false).replace("BB = 1", "BB = 2"));
        let tree = Tree::build(&game).unwrap();
        let profile = random_profile(&tree, if players == 3 { 9 } else { 3 });
        let options = RealOptions {
            deals: 16 * 4096,
            seed: 13,
        };
        let fit =
            fit_real_responses(&tree, &profile, &game, options, &[0.0, 1.0, 2.0, 1e9]).unwrap();
        let responses: Vec<_> = fit.gated.iter().map(|g| g.actions.clone()).collect();
        let real = evaluate_real(&tree, &profile, &responses, &game, options).unwrap();
        for (g, gated) in fit.gated.iter().enumerate() {
            for seat in &gated.seats {
                let pure = &fit.seats[seat.seat];
                let tol = 1e-9 * (1.0 + pure.best_response.abs().max(pure.value.abs()));
                assert!(
                    (real.seats[seat.seat].responses[g].value.mean - seat.best_response).abs()
                        <= tol
                );
                assert!(
                    pure.value - tol <= seat.best_response
                        && seat.best_response <= pure.best_response + tol
                );
                if g == 0 {
                    assert!((seat.best_response - pure.best_response).abs() <= tol);
                }
                if g == 3 {
                    assert_eq!(seat.deviations, 0);
                    assert!((seat.best_response - pure.value).abs() <= tol);
                }
                for (z, node) in tree.nodes.iter().enumerate() {
                    for c in 0..169 {
                        let a = gated.actions[seat.seat][z * 169 + c];
                        if node.actor == Some(seat.seat) {
                            assert!(a == FOLLOW || usize::from(a) < node.children.len());
                        } else {
                            assert_eq!(a, u8::MAX);
                        }
                    }
                }
            }
            assert!(
                serde_json::to_value(gated)
                    .unwrap()
                    .get("actions")
                    .is_none()
            );
        }
        let count = |g: usize| fit.gated[g].seats.iter().map(|s| s.deviations).sum::<u64>();
        assert!(
            count(1) > 0 && count(1) < count(0),
            "players {players}: z0 {}, z1 {}",
            count(0),
            count(1)
        );
    }
}

#[test]
fn fitted_responses_are_optimal_on_the_fitting_deals() {
    for players in [3, 4] {
        // Give BB a decision instead of posting its entire stack as a blind.
        let game = game(&config(players, players == 4, false).replace("BB = 1", "BB = 2"));
        let tree = Tree::build(&game).unwrap();
        let profile = random_profile(&tree, 97);
        let options = RealOptions {
            deals: 3_000,
            seed: 13,
        };
        let fit = fit_real_responses(&tree, &profile, &game, options, &[]).unwrap();
        let model = Model::new(
            &game,
            &Synthetic,
            EvaluationOptions {
                k4_samples: 64,
                ..Default::default()
            },
        )
        .unwrap();
        let l0 = evaluate(&tree, &profile, &model).unwrap();
        let mut rng = ChaCha8Rng::seed_from_u64(921);
        let mut responses = vec![
            l0.best_response_actions,
            random_responses(&tree, &mut rng),
            random_responses(&tree, &mut rng),
        ];
        // Each candidate perturbs exactly one cell per seat. The evaluator
        // evaluates each seat's unilateral deviation separately, so every seat
        // is tested against 30 single-cell changes of its own fitted response.
        for _ in 0..30 {
            let mut candidate = fit.actions.clone();
            for (i, actions) in candidate.iter_mut().enumerate() {
                let nodes: Vec<_> = tree
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.actor == Some(i) && n.children.len() > 1)
                    .collect();
                let &(z, node) = &nodes[rng.gen_range(0..nodes.len())];
                let support: Vec<_> = (0..169).filter(|&c| model.weights()[i][c] > 0.0).collect();
                let c = support[rng.gen_range(0..support.len())];
                let action = &mut actions[z * 169 + c];
                *action = ((*action as usize + rng.gen_range(1..node.children.len()))
                    % node.children.len()) as u8;
            }
            responses.push(candidate);
        }
        for sets in responses.chunks(4) {
            let real = evaluate_real(&tree, &profile, sets, &game, options).unwrap();
            for (f, r) in fit.seats.iter().zip(&real.seats) {
                for response in &r.responses {
                    assert!(
                        response.value.mean
                            <= f.best_response + 1e-9 * (1.0 + f.best_response.abs()),
                        "seat {}: {} > {}",
                        f.seat,
                        response.value.mean,
                        f.best_response
                    );
                }
            }
        }
    }
}

#[test]
fn fitted_responses_are_thread_independent_and_validate_inputs() {
    let game = game(&config(4, true, false));
    let tree = Tree::build(&game).unwrap();
    let profile = random_profile(&tree, 97);
    // Cover all lanes, a second chunk in lane zero, and a partial last chunk.
    let options = RealOptions {
        deals: 16 * 4096 + 17,
        seed: 13,
    };
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| fit_real_responses(&tree, &profile, &game, options, &[1.0, 2.0]).unwrap())
    };
    let one = run(1);
    let four = run(4);
    assert_eq!(one.actions, four.actions);
    assert_eq!(one.seats, four.seats);
    assert_eq!(one.deals, four.deals);
    assert_eq!(one.seed, four.seed);
    for (a, b) in one.gated.iter().zip(&four.gated) {
        assert_eq!(a.threshold, b.threshold);
        assert_eq!(a.actions, b.actions);
        assert_eq!(a.seats, b.seats);
    }
    for thresholds in [
        vec![-1.0],
        vec![f64::NAN],
        vec![f64::INFINITY],
        vec![0.0; 5],
    ] {
        assert!(fit_real_responses(&tree, &profile, &game, options, &thresholds).is_err());
    }
    assert!(
        fit_real_responses(
            &tree,
            &profile,
            &game,
            RealOptions {
                deals: 1,
                ..options
            },
            &[]
        )
        .is_err()
    );
    let other_game = self::game(&config(3, false, false));
    assert!(fit_real_responses(&tree, &profile, &other_game, options, &[]).is_err());
    let other_tree = Tree::build(&other_game).unwrap();
    assert!(
        fit_real_responses(&tree, &Profile::uniform(&other_tree), &game, options, &[]).is_err()
    );
}

#[test]
#[ignore = "release acceptance: real HU tables and independent 2^20-deal fit/evaluation"]
fn fitted_responses_match_l0_in_heads_up() {
    use crate::trunk::tables::HuShowdownTable;
    struct HuOnly(HuShowdownTable);
    impl Tables for HuOnly {
        fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
            self.0.t2(c, d)
        }
        fn p3(&self, _: usize, _: usize, _: usize) -> Result<[f64; 13]> {
            anyhow::bail!("heads-up cannot reach three-way terminals")
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/bench/hu_pushfold_10bb.toml");
    let game = game_from_config(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    let tree = Tree::build(&game).unwrap();
    let tables = HuOnly(HuShowdownTable::load_or_build(&root.join(".cache/p2-trunk")).unwrap());
    let profile = random_profile(&tree, 101);
    let model = Model::new(&game, &tables, EvaluationOptions::default()).unwrap();
    let l0 = evaluate(&tree, &profile, &model).unwrap();
    let fit = fit_real_responses(
        &tree,
        &profile,
        &game,
        RealOptions {
            deals: 1 << 20,
            seed: 1,
        },
        &[2.0],
    )
    .unwrap();
    let real = evaluate_real(
        &tree,
        &profile,
        &[fit.actions.clone(), fit.gated[0].actions.clone()],
        &game,
        RealOptions {
            deals: 1 << 20,
            seed: 2,
        },
    )
    .unwrap();
    for ((l, f), r) in l0.seats.iter().zip(&fit.seats).zip(&real.seats) {
        let gain = &r.responses[0].gain;
        let gated = &r.responses[1].gain;
        assert!(gated.mean <= l.gain + 5.0 * gated.stderr);
        println!("seat {}: gated-z2 held-out {:?}", l.seat, gated);
        assert!(f.gain >= l.gain - 5.0 * gain.stderr);
        assert!(gain.mean <= l.gain + 5.0 * gain.stderr);
        println!(
            "seat {}: L0 gain {}, in-sample {}, out-of-sample {:?}",
            l.seat, l.gain, f.gain, gain
        );
    }
}

#[test]
fn json_loading_roundtrip_defaults_and_errors() {
    let game = game(&config(3, false, false));
    let tree = Tree::build(&game).unwrap();
    let original = random_profile(&tree, 5);
    let doc: ClassProfileDocument =
        serde_json::from_str(&serde_json::to_string(&original.export(&tree)).unwrap()).unwrap();
    let profile = Profile::from_json(&tree, &doc).unwrap();
    for (i, n) in tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.actor.is_some())
    {
        for c in 0..169 {
            for (&a, &b) in original
                .row(&tree, i, c)
                .iter()
                .zip(profile.row(&tree, i, c))
            {
                close(a, b);
            }
        }
        assert!(!n.labels.is_empty());
    }
    let model = Model::new(&game, &Synthetic, EvaluationOptions::default()).unwrap();
    let missing = Profile::missing(&tree);
    let result = evaluate(&tree, &missing, &model).unwrap();
    // Root is certainly reached; defaulted mass includes that seat's later nodes.
    assert!(result.seats[tree.nodes[0].actor.unwrap()].defaulted_mass >= 1.0 - 1e-9);
    let mut bad = doc.clone();
    bad.nodes[0].actions[0] = "unknown".into();
    assert!(Profile::from_json(&tree, &bad).is_err());
    let mut bad = doc.clone();
    bad.nodes[0].path = vec!["unknown".into()];
    assert!(Profile::from_json(&tree, &bad).is_err());
    let mut bad = doc.clone();
    bad.nodes[0].actions.pop();
    assert!(Profile::from_json(&tree, &bad).is_err());
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut bad = doc.clone();
        bad.nodes[0].probabilities[0].fill(invalid);
        assert!(Profile::from_json(&tree, &bad).is_err());
    }
    let mut permuted = doc;
    for n in &mut permuted.nodes {
        n.actions.reverse();
        for row in &mut n.probabilities {
            row.reverse();
        }
    }
    let reordered = Profile::from_json(&tree, &permuted).unwrap();
    close(
        evaluate(&tree, &reordered, &model).unwrap().nash_conv,
        evaluate(&tree, &original, &model).unwrap().nash_conv,
    );
}

#[test]
fn mwsol_paged_import_without_running_solver() {
    use crate::mwsol::*;
    let raw = config(2, false, false);
    let game = game(&raw);
    let tree = Tree::build(&game).unwrap();
    let root = &tree.nodes[0];
    let actor = root.actor.unwrap();
    let key = MultiwayStrategyKey {
        history: root.history.0,
        actor: actor as u8,
        street: 0,
        active_opponents: 1,
        bucket_path: [0, u32::MAX, u32::MAX, u32::MAX],
    };
    let solution = MultiwaySolution {
        schema_version: crate::metrics::MULTIWAY_SCHEMA_VERSION,
        config_toml: raw.clone(),
        config_fingerprint: runfiles::config_hash(raw.as_bytes()),
        game_fingerprint: game.game_fingerprint(),
        algorithm_fingerprint: [1; 32],
        abstraction_fingerprint: [2; 32],
        configuration_fingerprint: [3; 32],
        stop_status: "completed".into(),
        chip_unit_bb: 0.001,
        sweeps: 0,
        approximate_profile: true,
        seats: Vec::new(),
        histories: Vec::new(),
        public_states: vec![MultiwayPublicState {
            history: root.history.0,
            street: 0,
            actor: Some(actor as u8),
            pot_millibb: 1500,
            remaining_stacks_millibb: vec![2500, 1000],
            legal_actions: game
                .node_actions(&game.root_state())
                .iter()
                .map(|a| match a {
                    crate::Action::Fold => MultiwayPublicAction::Fold,
                    crate::Action::RaiseTo {
                        to,
                        all_in,
                        full_raise,
                    } => MultiwayPublicAction::RaiseTo {
                        amount_millibb: to.raw(),
                        all_in: *all_in,
                        full_raise: *full_raise,
                    },
                    other => panic!("unexpected {other:?}"),
                })
                .collect(),
        }],
        strategy_weights: vec![MultiwayStrategyWeight { key, weight: 1.0 }],
        strategies: vec![MultiwayStrategyBlock {
            key,
            actions: root.labels.clone(),
            probabilities: vec![0.2, 0.8],
        }],
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.mwsol");
    write_mwsol_with(&path, &solution, MwsolStorage::U16).unwrap();
    let mut reader = MwSolReader::open(&path).unwrap();
    let restored = game_from_solution(reader.metadata()).unwrap();
    assert_eq!(restored.game_fingerprint(), tree.game_fingerprint);
    let mut profile = Profile::missing(&tree);
    let page = reader.read_strategy_page(0, 1).unwrap();
    profile.apply_blocks(&tree, &page.strategies).unwrap();
    assert!(!profile.is_defaulted(0, 0));
    assert!(profile.is_defaulted(0, 1));
    close(profile.row(&tree, 0, 0).iter().sum(), 1.0);
    assert!((profile.row(&tree, 0, 0)[0] - 0.2).abs() < 1e-4);
    let model = Model::new(&game, &Synthetic, EvaluationOptions::default()).unwrap();
    let result = evaluate(&tree, &profile, &model).unwrap();
    // Independently enumerate physical disjoint pairs for defaulted mass.
    let catalog = Classes::get();
    let combos: Vec<Vec<_>> = model
        .weights()
        .iter()
        .map(|w| {
            (0..169)
                .filter(|&c| w[c] > 0.0)
                .flat_map(|c| catalog.combos(c).iter().copied())
                .collect()
        })
        .collect();
    let mut total = 0.0;
    let mut defaulted = 0.0;
    let mut response = 0.0;
    for &h in &combos[0] {
        for &v in &combos[1] {
            if catalog.combo_mask(h) & catalog.combo_mask(v) != 0 {
                continue;
            }
            let c = crate::trunk::classes::class(h);
            let d = crate::trunk::classes::class(v);
            let weight = model.weights()[0][c] * model.weights()[1][d];
            total += weight;
            if c != 0 {
                defaulted += weight;
            }
            response += weight * profile.row(&tree, 0, c)[1];
        }
    }
    close(result.seats[0].defaulted_mass, defaulted / total);
    close(result.seats[1].defaulted_mass, response / total);
    let mut wrong = reader.metadata().clone();
    wrong.game_fingerprint[0] ^= 1;
    assert!(game_from_solution(&wrong).is_err());
    for change in 0..3 {
        let mut block = page.strategies[0].clone();
        match change {
            0 => block.key.history = [9; 16],
            1 => block.key.actor ^= 1,
            _ => block.actions[0] = "unknown".into(),
        }
        assert!(
            Profile::missing(&tree)
                .apply_blocks(&tree, &[block])
                .is_err()
        );
    }
    let mut block = page.strategies[0].clone();
    block.actions.truncate(1);
    block.probabilities = vec![1.0];
    let mut partial = Profile::missing(&tree);
    partial.apply_blocks(&tree, &[block]).unwrap();
    assert_eq!(partial.row(&tree, 0, 0), &[1.0, 0.0]);
}

#[test]
fn rejects_icm_postflop_and_zero_deal_and_warns_on_suits() {
    let prepared =
        crate::prepare::prepare(&config(2, false, false), Path::new("unit.toml")).unwrap();
    let valid_game = game(&config(2, false, false));
    let mut root = valid_game.root_state();
    root.to_act = None;
    assert!(
        Tree::build_root(&valid_game, root)
            .err()
            .unwrap()
            .to_string()
            .contains("without an actor")
    );
    let icm = economics::UtilityConfig::TournamentIcm {
        outside_field: Vec::new(),
        payouts: vec![1.0, 0.0],
        samples: 8,
        seed: 0,
    };
    let icm_game = HoldemGame::new(
        &prepared.lowered.game,
        &icm,
        &prepared.lowered.rake,
        FeatureHashAbstraction::default(),
    )
    .unwrap();
    assert!(
        Tree::build(&icm_game)
            .err()
            .unwrap()
            .to_string()
            .contains("ICM is handled in S4-1b")
    );
    let raw =
        config(2, false, true).replace("flop, turn, river when players >= 2 { checkdown }", "");
    assert!(
        Tree::build(&game(&raw))
            .err()
            .unwrap()
            .to_string()
            .contains("postflop decision")
    );
    let mut config = prepared.lowered.game;
    config.seats[0].range = "AsAh".into();
    config.seats[1].range = "AsAh".into();
    // Class averaging means AA/AA still has disjoint combos; use zero weights.
    let suit_game = HoldemGame::new(
        &config,
        &economics::UtilityConfig::ChipEv,
        &economics::RakeConfig::None,
        FeatureHashAbstraction::default(),
    )
    .unwrap();
    assert!(
        !Model::new(&suit_game, &Synthetic, EvaluationOptions::default())
            .unwrap()
            .warnings
            .is_empty()
    );
    assert!(
        super::eval::deal_normalizers(&[[0.0; 169], [1.0; 169]], &[Vec::new(), (0..169).collect()])
            .is_err()
    );
}

#[test]
fn sampler_three_way_matches_fixed_table_within_five_standard_errors() {
    struct RealThree(crate::trunk::tables::ThreeWayTable);
    impl Tables for RealThree {
        fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
            Synthetic.t2(c, d)
        }
        fn p3(&self, c: usize, d: usize, e: usize) -> Result<[f64; 13]> {
            Ok(self.0.p3(c, d, e)?.map(f64::from))
        }
    }
    let game = game(&config(3, false, true));
    let tree = Tree::build(&game).unwrap();
    let profile = random_profile(&tree, 91);
    let classes = [0, 1, 14, 28];
    let tables =
        RealThree(crate::trunk::tables::ThreeWayTable::build_subset(&classes, 65536, 14).unwrap());
    let table = evaluate(
        &tree,
        &profile,
        &Model::new(&game, &tables, EvaluationOptions::default()).unwrap(),
    )
    .unwrap();
    let sampled: Vec<_> = (0..8)
        .map(|seed| {
            evaluate(
                &tree,
                &profile,
                &Model::new(
                    &game,
                    &tables,
                    EvaluationOptions {
                        k4_samples: 4096,
                        seed,
                        sample_three_way: true,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
            .unwrap()
        })
        .collect();
    for p in 0..3 {
        for best in [false, true] {
            let get = |e: &Evaluation| {
                if best {
                    e.seats[p].best_response
                } else {
                    e.seats[p].value
                }
            };
            let mean = sampled.iter().map(get).sum::<f64>() / sampled.len() as f64;
            let variance = sampled.iter().map(|e| (get(e) - mean).powi(2)).sum::<f64>() / 7.0;
            let se = (variance / 8.0 + 36.0 / 65536.0).sqrt();
            assert!((mean - get(&table)).abs() <= 5.0 * se);
        }
    }
}

#[test]
fn incremental_reaches_match_full_rebuild_bitwise() {
    use super::eval::{reaches, solver_reaches, update_reach};
    for players in [3, 4] {
        for limp in [false, true] {
            let game = game(&config(players, true, limp));
            let tree = Tree::build(&game).unwrap();
            let model = Model::new(&game, &Synthetic, EvaluationOptions::default()).unwrap();
            let mut profile = random_profile(&tree, 21);
            let mut incremental = solver_reaches(&tree, &profile, &model);
            for step in 0..=players * 4 {
                let full = reaches(&tree, &profile, &model);
                for z in 0..tree.nodes.len() {
                    for p in 0..players {
                        for c in 0..169 {
                            assert_eq!(
                                incremental[z].pi(p, c).to_bits(),
                                full[z].pi(p, c).to_bits(),
                                "pi: step {step}, node {z}, seat {p}, class {c}"
                            );
                            if tree.nodes[z].terminal.is_some() {
                                assert_eq!(
                                    incremental[z].mass(p, c).to_bits(),
                                    full[z].mass(p, c).to_bits(),
                                    "mass: step {step}, node {z}, seat {p}, class {c}"
                                );
                            }
                        }
                    }
                }
                if step == players * 4 {
                    break;
                }
                let p = step % players;
                let replacement = random_profile(&tree, step as u64 + 50);
                for (z, node) in tree.nodes.iter().enumerate() {
                    if node.actor == Some(p) {
                        for c in 0..169 {
                            // Alternate dense random and pure rows, covering zero reaches.
                            let row = profile.row_mut(&tree, z, c);
                            row.copy_from_slice(replacement.row(&tree, z, c));
                            if step % 2 == 1 {
                                row.fill(0.0);
                                row[c % row.len()] = 1.0;
                            }
                        }
                    }
                }
                update_reach(&tree, &profile, &model, &mut incremental, p);
            }
        }
    }
}

#[test]
fn three_way_basis_matches_old_contraction_with_residuals_and_zero_mass() {
    use super::eval::reaches;
    use super::leaves::{ThreeItem, ThreeTerms, three_values, three_values_reference};
    let mut saw_corrections = false;
    for players in [3, 4] {
        for rake in [false, true] {
            for limp in [false, true] {
                let game = game(&config(players, rake, limp));
                let tree = Tree::build(&game).unwrap();
                let model = Model::new(&game, &Synthetic, EvaluationOptions::default()).unwrap();
                let random = random_profile(&tree, 39);
                let mut pure = random.clone();
                for (z, node) in tree.nodes.iter().enumerate() {
                    if node.actor.is_some() {
                        for c in 0..169 {
                            let row = pure.row_mut(&tree, z, c);
                            row.fill(0.0);
                            row[c % row.len()] = 1.0;
                        }
                    }
                }
                for profile in [random, pure] {
                    let reach = reaches(&tree, &profile, &model);
                    let mut items = Vec::new();
                    for (z, node) in tree.nodes.iter().enumerate() {
                        let Some(t) = &node.terminal else { continue };
                        if t.active.len() != 3 {
                            continue;
                        }
                        for &hero in &t.active {
                            let others: Vec<_> =
                                t.active.iter().copied().filter(|&p| p != hero).collect();
                            let payoffs = t.hero_payoffs(hero);
                            let terms = ThreeTerms::new(&payoffs);
                            saw_corrections |= terms.mask != 0;
                            items.push(ThreeItem {
                                node: z,
                                hero,
                                q: reach[z].rho(&model, others[0]),
                                r: reach[z].rho(&model, others[1]),
                                terms,
                                payoffs,
                            });
                        }
                    }
                    // Arbitrary out-of-span payoffs, with both tie and strict perturbations.
                    let mut perturbed = items[0].clone();
                    perturbed.payoffs[Ordering3::from_ranks(2, 2, 1).index()] += 0.137;
                    perturbed.payoffs[Ordering3::from_ranks(3, 1, 2).index()] -= 0.271;
                    perturbed.terms = ThreeTerms::new(&perturbed.payoffs);
                    assert!(perturbed.terms.mask != 0);
                    for items in [items, vec![perturbed]] {
                        for c in 0..169 {
                            if !model.weights.iter().any(|w| w[c] > 0.0) {
                                continue;
                            }
                            let actual = three_values(c, &items, &tree, &model, &reach).unwrap();
                            let expected =
                                three_values_reference(c, &items, &tree, &model, &reach).unwrap();
                            for (z, p, v) in expected {
                                let new = actual
                                    .iter()
                                    .find(|&&(node, hero, _)| node == z && hero == p)
                                    .map_or(0.0, |item| item.2);
                                let item = items
                                    .iter()
                                    .find(|item| item.node == z && item.hero == p)
                                    .unwrap();
                                let mass: f64 = (0..players)
                                    .filter(|&j| j != p)
                                    .map(|j| reach[z].mass(j, c))
                                    .product();
                                let scale = item
                                    .payoffs
                                    .iter()
                                    .copied()
                                    .map(f64::abs)
                                    .fold(0.0, f64::max)
                                    * mass;
                                assert!(
                                    (new - v).abs() <= 1e-12 * scale,
                                    "{new} != {v}, scale {scale}"
                                );
                            }
                            // Reverse item traversal must not change any accumulator.
                            let reversed: Vec<_> = items.iter().rev().cloned().collect();
                            assert_eq!(
                                actual,
                                three_values(c, &reversed, &tree, &model, &reach).unwrap()
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(
        saw_corrections,
        "settlement trees must exercise odd-chip corrections"
    );
}

#[test]
fn k4_leaf_values_and_cache_patterns_match_cold_warm_and_thread_counts() {
    use super::eval::reaches;
    use super::leaves::{K4Plan, leaf_values};

    let game = game(&config(4, true, false).replace("remove call", ""));
    let tree = Tree::build(&game).unwrap();
    let parallel_tree = Tree::build(&game).unwrap();
    assert!(tree.terminal_counts()[4] > 0);
    let profile = random_profile(&tree, 92);
    let model = Model::new(
        &game,
        &Synthetic,
        EvaluationOptions {
            k4_samples: 64,
            seed: 17,
            ..Default::default()
        },
    )
    .unwrap();
    let reach = reaches(&tree, &profile, &model);
    let heroes: Vec<_> = (0..tree.seats).collect();
    let run = |tree: &Tree, threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| leaf_values(tree, &model, &reach, &heroes, K4Plan::model(&model)).unwrap())
    };
    let patterns = |tree: &Tree| {
        tree.nodes
            .iter()
            .map(|node| {
                let mut keys: Vec<_> = node
                    .terminal
                    .as_ref()
                    .map(|t| t.cache.read().unwrap().keys().copied().collect())
                    .unwrap_or_default();
                keys.sort_unstable();
                keys
            })
            .collect::<Vec<_>>()
    };
    assert!(patterns(&tree).iter().all(Vec::is_empty));
    assert!(patterns(&parallel_tree).iter().all(Vec::is_empty));
    let cold = run(&tree, 1);
    let expected_patterns = patterns(&tree);
    assert!(expected_patterns.iter().any(|keys| !keys.is_empty()));
    let warm = run(&tree, 1);
    assert_eq!(patterns(&tree), expected_patterns);
    let parallel_cold = run(&parallel_tree, 4);
    assert_eq!(patterns(&parallel_tree), expected_patterns);
    let parallel_warm = run(&parallel_tree, 4);
    assert_eq!(patterns(&parallel_tree), expected_patterns);

    let mut saw_nonzero = false;
    for (z, node) in tree.nodes.iter().enumerate() {
        if !node.terminal.as_ref().is_some_and(|t| t.active.len() >= 4) {
            continue;
        }
        for p in 0..tree.seats {
            for c in 0..169 {
                let expected = cold.values[p][z][c].to_bits();
                saw_nonzero |= cold.values[p][z][c] != 0.0;
                for other in [&warm, &parallel_cold, &parallel_warm] {
                    assert_eq!(other.values[p][z][c].to_bits(), expected, "{z}/{p}/{c}");
                }
            }
        }
    }
    assert!(saw_nonzero);
}

#[test]
fn k4_model_plan_is_bit_identical_and_iteration_plans_change_streams() {
    use super::eval::reaches;
    use super::leaves::{K4Plan, SampleReference, k4_reference, leaf_values};
    for players in [4, 5] {
        let mut raw = config(4, true, false).replace("remove call", "");
        if players == 5 {
            raw = raw
                .replace("players = 4", "players = 5")
                .replace("[ranges]", "HJ = 3\n[ranges]\nHJ = 'AA,QQ'");
        }
        let game = game(&raw);
        let tree = Tree::build(&game).unwrap();
        let model = Model::new(
            &game,
            &Synthetic,
            EvaluationOptions {
                k4_samples: 64,
                seed: 17,
                ..Default::default()
            },
        )
        .unwrap();
        let profile = random_profile(&tree, 92);
        let reach = reaches(&tree, &profile, &model);
        let heroes: Vec<_> = (0..tree.seats).collect();
        let model_values =
            leaf_values(&tree, &model, &reach, &heroes, K4Plan::model(&model)).unwrap();
        for SampleReference {
            node: z,
            hero: p,
            class: c,
            samples: count,
            value: v,
        } in k4_reference(&tree, &model, &reach, K4Plan::model(&model)).unwrap()
        {
            assert_eq!(count, model.options.k4_samples);
            assert_eq!(v.to_bits(), model_values.values[p][z][c].to_bits());
        }
        let mut pure = profile.clone();
        for (z, node) in tree.nodes.iter().enumerate() {
            if node.actor.is_some() {
                for c in 0..169 {
                    let row = pure.row_mut(&tree, z, c);
                    row.fill(0.0);
                    row[0] = 1.0;
                }
            }
        }
        let zero_reach = reaches(&tree, &pure, &model);
        let scaled = SolveOptions {
            k4_samples: Some(32),
            k4_min_samples: Some(4),
            ..Default::default()
        }
        .k4_plan(&model, 1);
        let zero_leaves = leaf_values(&tree, &model, &zero_reach, &heroes, scaled).unwrap();
        for item in k4_reference(&tree, &model, &zero_reach, scaled).unwrap() {
            assert_eq!(item.value, 0.0);
            assert_eq!(zero_leaves.values[item.hero][item.node][item.class], 0.0);
        }
        for minimum in [None, Some(4)] {
            let options = SolveOptions {
                k4_samples: Some(32),
                k4_min_samples: minimum,
                ..Default::default()
            };
            let one = options.k4_plan(&model, 1);
            let two = options.k4_plan(&model, 2);
            assert_ne!(one.seed, two.seed);
            assert_ne!(one.seed, model.options.seed);
            assert_ne!(two.seed, model.options.seed);
            let a = leaf_values(&tree, &model, &reach, &heroes, one).unwrap();
            let b = leaf_values(&tree, &model, &reach, &heroes, two).unwrap();
            let mut saw_full_budget = false;
            let mut saw_reduced_budget = false;
            for SampleReference {
                node: z,
                hero: p,
                class: c,
                samples: count,
                value,
            } in k4_reference(&tree, &model, &reach, one).unwrap()
            {
                assert!((minimum.unwrap_or(32)..=32).contains(&count));
                saw_full_budget |= count == 32;
                saw_reduced_budget |= count < 32;
                assert_eq!(
                    value.to_bits(),
                    a.values[p][z][c].to_bits(),
                    "prefix at {z}/{p}/{c}, K={count}"
                );
            }
            assert!(saw_full_budget);
            if minimum.is_some() {
                assert!(saw_reduced_budget);
            }

            assert!(
                tree.nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| node.terminal.as_ref().is_some_and(|t| t.active.len() >= 4))
                    .any(|(z, t)| t
                        .terminal
                        .as_ref()
                        .unwrap()
                        .active
                        .iter()
                        .any(|&p| model.support[p]
                            .iter()
                            .any(|&c| a.values[p][z][c].to_bits() != b.values[p][z][c].to_bits())))
            );
            for mass in [1e-20, 0.01, 0.25, 0.999, 1.0] {
                let count = one.allocation(mass, 1.0);
                assert!((minimum.unwrap_or(32)..=32).contains(&count));
                let expected =
                    minimum.map_or(32, |min| ((32.0 * mass).ceil() as u64).clamp(min, 32));
                assert_eq!(count, expected);
            }
            assert_eq!(one.allocation(1.0, 1.0), 32);
        }
    }
    let encoded = serde_json::to_value(SolveOptions::default()).unwrap();
    assert!(encoded.get("k4_samples").unwrap().is_null());
    assert!(encoded.get("k4_min_samples").unwrap().is_null());
}

#[test]
fn trunk_solver_converges_four_players_with_solver_k4_approximation() {
    let game = game(&config(4, false, false));
    let tree = Tree::build(&game).unwrap();
    let model = Model::new(
        &game,
        &Synthetic,
        EvaluationOptions {
            k4_samples: 64,
            ..Default::default()
        },
    )
    .unwrap();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    pool.install(|| {
        let options = SolveOptions {
            iterations: 1000,
            eval_every: 0,
            ..Default::default()
        };
        let uniform = evaluate(&tree, &Profile::uniform(&tree), &model)
            .unwrap()
            .nash_conv;
        let exact = solve(&tree, &model, options, |_| {})
            .unwrap()
            .checkpoints
            .last()
            .unwrap()
            .nash_conv;
        let approximate = solve(
            &tree,
            &model,
            SolveOptions {
                k4_samples: Some(64),
                ..options
            },
            |_| {},
        )
        .unwrap()
        .checkpoints
        .last()
        .unwrap()
        .nash_conv;
        println!(
            "K4 convergence: uniform={uniform:.15}, exact={exact:.15}, solver64={approximate:.15}"
        );
        assert!(
            approximate <= 10.0 * exact + 1e-3 * uniform,
            "exact={exact}, approximate={approximate}, uniform={uniform}"
        );
    });
}

#[test]
#[ignore = "report three-way payoff correction statistics on uniform B3/B4 profiles"]
fn three_way_correction_statistics_b3_b4() {
    use super::leaves::ThreeTerms;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (name, file) in [
        ("B3", "6max_20bb_checkdown.toml"),
        (
            "B4 Simple",
            "6max_100bb_nl50_partial_simple_reference_checkdown.toml",
        ),
        (
            "B4 General",
            "6max_100bb_nl50_partial_reference_checkdown.toml",
        ),
    ] {
        let path = root.join("examples/bench").join(file);
        let game = game_from_config(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
        let tree = Tree::build(&game).unwrap();
        let mut items = 0;
        let mut corrected = 0;
        let mut terms = 0;
        // Uniform actions and full benchmark ranges have positive reaches.
        for t in tree
            .nodes
            .iter()
            .filter_map(|node| node.terminal.as_ref())
            .filter(|t| t.active.len() == 3)
        {
            for &hero in &t.active {
                let mask = ThreeTerms::new(&t.hero_payoffs(hero)).mask;
                items += 1;
                corrected += usize::from(mask != 0);
                terms += mask.count_ones() as usize;
            }
        }
        println!(
            "{name}: items={items}, corrected={corrected}, correction_terms={terms}, mean={:.9}, corrected_mean={:.9}",
            terms as f64 / items as f64,
            terms as f64 / corrected.max(1) as f64
        );
    }
}
