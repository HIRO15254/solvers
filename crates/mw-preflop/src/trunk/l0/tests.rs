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
    assert!(first.seats.iter().all(|s| s.best_response >= s.value));
    for s in first.seats {
        close(s.telescoping_residual, 0.0);
        close(s.reach_by_active_count.iter().sum(), 1.0);
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
