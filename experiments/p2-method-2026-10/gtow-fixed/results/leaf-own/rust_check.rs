// Cross-check of the Rust own-range flop groups (trunk::l1::own) against the Python
// groups of leaf_groups.py, and of how the groups move with the warm-up length
// (rust-check.md). It needs crate-private items, so it ran as a temporary test module:
// append this file to crates/mw-preflop/src/trunk/l1/own.rs (as of the commit that
// added rust-check.md), then from the workspace root
//   cargo test -p mw-preflop --release --lib -- --ignored rust_check --nocapture
// Inputs (hashes in rust-check.md): the it001000/it002000/it006000 profiles of the
// r16g49q full solve, reach-all-r16g49q.json and leafgroups/r16g49q-all (Python),
// ../_goal/fixed/flops.csv and the 128-bucket EHS2 table. Output: runs/own-scratch.tsv.
#[cfg(test)]
mod rust_check {
    use super::*;
    use crate::Street;
    use crate::card_abstraction::{Ehs2Abstraction, Ehs2Params};
    use crate::trunk::l0::{self, ClassProfileDocument, EvaluationOptions, FlopLeaves, Tables};
    use std::path::Path;

    struct NoTables;
    impl Tables for NoTables {
        fn t2(&self, _: usize, _: usize) -> Result<[f64; 3]> {
            unreachable!()
        }
        fn p3(&self, _: usize, _: usize, _: usize) -> Result<[f64; 13]> {
            unreachable!()
        }
    }

    fn assign_of(features: &FlopFeatures, groups: &FlopClusters) -> Vec<usize> {
        let mut out = vec![usize::MAX; features.classes.len()];
        for a in 0..52_u8 {
            for b in a + 1..52 {
                for c in b + 1..52 {
                    let flop = [a, b, c].map(Card::from_index);
                    let i = features.classes.binary_search(&canonical_flop(flop)).unwrap();
                    out[i] = groups.group(&flop);
                }
            }
        }
        out
    }

    /// `FlopFeatures::groups` with another seed.
    fn seeded(features: &FlopFeatures, weights: &[f64; 169], k: usize, seed: u64) -> Vec<usize> {
        let scale: Vec<(usize, f64)> = (0..169)
            .filter(|&c| weights[c] > 0.0)
            .map(|c| (c, weights[c].sqrt()))
            .collect();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut out = vec![0; features.classes.len()];
        let mut next = 0;
        for (members, k) in features.strata.iter().zip(features.shares(k)) {
            let n = members.len();
            let mut distance = vec![0.0; n * n];
            for i in 0..n {
                for j in i + 1..n {
                    let d = scale
                        .iter()
                        .map(|&(c, s)| {
                            let x = (features.strength[members[i]][c] - features.strength[members[j]][c]) * s;
                            x * x
                        })
                        .sum::<f64>()
                        .sqrt();
                    distance[i * n + j] = d;
                    distance[j * n + i] = d;
                }
            }
            let weight: Vec<f64> = members.iter().map(|&i| features.raw[i]).collect();
            let (_, assign) = (0..RESTARTS)
                .map(|_| kmedoids(&distance, &weight, k, &mut rng))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
            for (&i, &j) in members.iter().zip(&assign) {
                out[i] = next + j;
            }
            next += k;
        }
        out
    }

    /// Mean raw-weighted distance to each group's best medoid, in the units of
    /// flop_subset.py (features scaled by sqrt(169 w)).
    fn cost(features: &FlopFeatures, weights: &[f64; 169], assign: &[usize]) -> f64 {
        let scale: Vec<(usize, f64)> = (0..169)
            .filter(|&c| weights[c] > 0.0)
            .map(|c| (c, (169.0 * weights[c]).sqrt()))
            .collect();
        let d = |i: usize, j: usize| {
            scale
                .iter()
                .map(|&(c, s)| {
                    let x = (features.strength[i][c] - features.strength[j][c]) * s;
                    x * x
                })
                .sum::<f64>()
                .sqrt()
        };
        let groups = assign.iter().max().unwrap() + 1;
        let mut members = vec![Vec::new(); groups];
        for (i, &g) in assign.iter().enumerate() {
            members[g].push(i);
        }
        let total: f64 = members
            .iter()
            .map(|m| {
                m.iter()
                    .map(|&c| m.iter().map(|&i| features.raw[i] * d(i, c)).sum::<f64>())
                    .fold(f64::INFINITY, f64::min)
            })
            .sum();
        total / features.raw.iter().sum::<f64>()
    }

    /// Adjusted Rand index of two partitions of the 1755 flops.
    fn ari(a: &[usize], b: &[usize]) -> f64 {
        let mut table = std::collections::HashMap::new();
        let mut ra = std::collections::HashMap::new();
        let mut rb = std::collections::HashMap::new();
        for (&x, &y) in a.iter().zip(b) {
            *table.entry((x, y)).or_insert(0.0) += 1.0;
            *ra.entry(x).or_insert(0.0) += 1.0;
            *rb.entry(y).or_insert(0.0) += 1.0;
        }
        let c2 = |n: f64| n * (n - 1.0) / 2.0;
        let index: f64 = table.values().map(|&n| c2(n)).sum();
        let sa: f64 = ra.values().map(|&n| c2(n)).sum();
        let sb: f64 = rb.values().map(|&n| c2(n)).sum();
        let expected = sa * sb / c2(a.len() as f64);
        (index - expected) / (0.5 * (sa + sb) - expected)
    }

    fn tv(a: &[f64; 169], b: &[f64; 169]) -> f64 {
        0.5 * a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f64>()
    }

    #[test]
    #[ignore]
    fn rust_check() {
        let root = Path::new("../..");
        let config = root.join("experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml");
        let game =
            l0::game_from_config(&std::fs::read_to_string(&config).unwrap(), &config).unwrap();
        let tree = Tree::build_with(&game, FlopLeaves::L1).unwrap();
        let model = Model::new(&game, &NoTables, EvaluationOptions::default()).unwrap();
        let table = Path::new(&std::env::var("LOCALAPPDATA").unwrap())
            .join("solvers/ehs2/v2-f128-t128-r128.postcard");
        let strength = Ehs2Abstraction::load_or_build(
            Ehs2Params {
                flop_buckets: 128,
                turn_buckets: 128,
                river_buckets: 128,
            },
            &[Street::Flop, Street::Turn, Street::River],
            Some(&table),
        );
        let features = FlopFeatures::new(&strength);
        // The features flop_subset.py used (3 decimals; empty = no live combo).
        let csv_text = std::fs::read_to_string(root.join("../_goal/fixed/flops.csv")).unwrap();
        let mut csv: Vec<[Option<f64>; 169]> = vec![[None; 169]; features.classes.len()];
        for row in csv_text.lines().skip(1) {
            let cells: Vec<&str> = row.split(',').collect();
            let flop: Vec<Card> = cells[0].split(' ').map(|c| c.parse().unwrap()).collect();
            let i = features
                .classes
                .binary_search(&canonical_flop([flop[0], flop[1], flop[2]]))
                .unwrap();
            assert_eq!(cells[1].parse::<f64>().unwrap(), features.raw[i]);
            for c in 0..169 {
                csv[i][c] = cells[2 + c].parse().ok();
            }
        }
        let column: Vec<f64> = (0..169)
            .map(|c| {
                let v: Vec<f64> = csv.iter().filter_map(|r| r[c]).collect();
                v.iter().sum::<f64>() / v.len() as f64
            })
            .collect();
        let (mut max_diff, mut sum_diff, mut cells) = (0.0_f64, 0.0, 0);
        for (i, r) in csv.iter().enumerate() {
            for c in 0..169 {
                if let Some(x) = r[c] {
                    let d = (x - features.strength[i][c]).abs();
                    max_diff = max_diff.max(d);
                    sum_diff += d;
                    cells += 1;
                }
            }
        }
        println!(
            "csv features: {cells} cells, max |diff| {max_diff:.5}, mean {:.6}",
            sum_diff / cells as f64
        );
        let alt = FlopFeatures {
            classes: features.classes.clone(),
            raw: features.raw.clone(),
            strata: features.strata.clone(),
            strength: csv
                .iter()
                .map(|r| std::array::from_fn(|c| r[c].unwrap_or(column[c])))
                .collect(),
        };
        let snap = root.join(
            ".cache/p2-trunk/gtow-fixed/improve/im-d-r16g49q-sd0/snap-traise5+p10+g49+hdq+rel16x2",
        );
        let load = |it: u64| {
            let document: ClassProfileDocument = serde_json::from_slice(
                &std::fs::read(snap.join(format!("it{it:06}.profile.json"))).unwrap(),
            )
            .unwrap();
            let profile = Profile::from_json(&tree, &document).unwrap();
            leaf_weights(&tree, &model, &profile)
                .into_iter()
                .map(|(z, w)| (z, w.unwrap()))
                .collect::<BTreeMap<_, _>>()
        };
        let (w1, w2, w6) = (load(1000), load(2000), load(6000));
        let own = root.join(".cache/p2-trunk/gtow-fixed/leaf-own");
        let reach: serde_json::Value =
            serde_json::from_slice(&std::fs::read(own.join("reach-all-r16g49q.json")).unwrap())
                .unwrap();
        let list = std::fs::read_to_string(own.join("leafgroups/r16g49q-all/list.txt")).unwrap();
        let files: BTreeMap<String, String> = list
            .lines()
            .map(|l| {
                let (line, file) = l.split_once(' ').unwrap();
                (line.to_string(), file.to_string())
            })
            .collect();
        // Python's weights (leaf_groups.py) and groups for every leaf.
        let leaves: Vec<(String, String, usize, [f64; 169], FlopClusters)> = reach["leaves"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                let line = l["line"].as_str().unwrap().to_string();
                let z = super::super::spec::leaf_of_line(&tree, &line).unwrap();
                let mut w = [0.0; 169];
                for seat in l["seats"].as_object().unwrap().values() {
                    let mass: Vec<f64> = (0..169)
                        .map(|c| {
                            seat["reach_leaf"][c].as_f64().unwrap() * seat["n"][c].as_f64().unwrap()
                        })
                        .collect();
                    let total: f64 = mass.iter().sum();
                    for c in 0..169 {
                        w[c] += mass[c] / total;
                    }
                }
                let total: f64 = w.iter().sum();
                let w = w.map(|x| x / total);
                let groups = FlopClusters::parse(
                    &std::fs::read_to_string(own.join("leafgroups/r16g49q-all").join(&files[&line]))
                        .unwrap(),
                )
                .unwrap();
                (l["band"].as_str().unwrap().to_string(), line, z, w, groups)
            })
            .collect();
        let bundled = assign_of(&features, &FlopClusters::recommended());
        let rows: Vec<String> = leaves
            .par_iter()
            .map(|(band, line, z, wpy, py)| {
                let (a1, a2, a6) = (&w1[z], &w2[z], &w6[z]);
                let g2 = assign_of(&features, &features.groups(a2, 49).unwrap());
                let g1 = assign_of(&features, &features.groups(a1, 49).unwrap());
                let g6 = assign_of(&features, &features.groups(a6, 49).unwrap());
                let s1 = seeded(&features, a2, 49, 1);
                let gp = assign_of(&features, py);
                let ga = assign_of(&features, &alt.groups(a2, 49).unwrap());
                let c2 = cost(&features, a2, &g2);
                let c6 = cost(&features, a6, &g6);
                format!(
                    "{band}\t{:.2e}\t{:.4}\t{:.4}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{}\t{line}",
                    tv(a2, wpy),
                    tv(a1, a2),
                    tv(a6, a2),
                    c2,
                    cost(&features, a2, &gp) / c2 - 1.0,
                    cost(&features, a2, &s1) / c2 - 1.0,
                    cost(&features, a2, &g1) / c2 - 1.0,
                    cost(&features, a6, &g2) / c6 - 1.0,
                    cost(&features, a6, &g1) / c6 - 1.0,
                    cost(&features, a2, &bundled) / c2 - 1.0,
                    cost(&features, a6, &bundled) / c6 - 1.0,
                    ari(&g2, &gp),
                    ari(&g2, &s1),
                    ari(&g2, &g1),
                    ari(&g2, &g6),
                    cost(&features, a2, &ga) / c2 - 1.0,
                    cost(&alt, a2, &gp) / cost(&alt, a2, &ga) - 1.0,
                    (2..8)
                        .map(|s| format!(
                            "{:.4}",
                            cost(&features, a2, &seeded(&features, a2, 49, s)) / c2 - 1.0
                        ))
                        .collect::<Vec<_>>()
                        .join(","),
                )
            })
            .collect();
        let mut text = String::from(
            "band\ttv_py\ttv_1k\ttv_6k\tcost2\tex_py\tex_seed1\tex_1k\tex_2k_at6k\tex_1k_at6k\tex_bundled\tex_bundled_at6k\tari_py\tari_seed1\tari_1k\tari_6k\tex_csv\tpy_vs_csv\tex_seeds2to7\tline\n",
        );
        for r in rows {
            text.push_str(&r);
            text.push('\n');
        }
        std::fs::write(root.join("runs/own-scratch.tsv"), text).unwrap();
    }
}
