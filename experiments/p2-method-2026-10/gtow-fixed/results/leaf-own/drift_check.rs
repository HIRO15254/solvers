// How far the ranges of the two-stage full solve (P2D12) moved from the stage-1 ranges its own flop
// groups were built from, and what that costs the groups (full-solve.md). It needs crate-private items, so
// it runs as a temporary test module: append this file to crates/mw-preflop/src/trunk/l1/own.rs (as of the
// commit that added full-solve.md), then from the workspace root
//   DRIFT_PROFILE=<stage-2 profile> cargo test -p mw-preflop --release --lib -- --ignored drift_check --nocapture
// Inputs: the it001000 (stage 1) and it002000 profiles of the r16g49q full solve, the stage-2 profile
// (DRIFT_PROFILE), reach-all-r16g49q.json (band of each leaf) and the 128-bucket EHS2 table.
// Output: runs/own-drift.tsv, one row per leaf that a seat reaches in all three profiles.
#[cfg(test)]
mod drift_check {
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

    /// Mean raw-weighted distance to each group's best medoid (the units of rust-check.md).
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

    fn tv(a: &[f64; 169], b: &[f64; 169]) -> f64 {
        0.5 * a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f64>()
    }

    #[test]
    #[ignore]
    fn drift_check() {
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
        let load = |path: &Path| {
            let document: ClassProfileDocument =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            let profile = Profile::from_json(&tree, &document).unwrap();
            leaf_weights(&tree, &model, &profile)
                .into_iter()
                .filter_map(|(z, w)| w.map(|w| (z, w)))
                .collect::<BTreeMap<_, _>>()
        };
        let snap = root.join(
            ".cache/p2-trunk/gtow-fixed/improve/im-d-r16g49q-sd0/snap-traise5+p10+g49+hdq+rel16x2",
        );
        let stage1 = load(&snap.join("it001000.profile.json"));
        let base = load(&snap.join("it002000.profile.json"));
        let stage2 = load(Path::new(&std::env::var("DRIFT_PROFILE").unwrap()));
        let reach: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join(".cache/p2-trunk/gtow-fixed/leaf-own/reach-all-r16g49q.json"))
                .unwrap(),
        )
        .unwrap();
        let leaves: Vec<(String, String, usize)> = reach["leaves"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                let line = l["line"].as_str().unwrap().to_string();
                let z = super::super::spec::leaf_of_line(&tree, &line).unwrap();
                (l["band"].as_str().unwrap().to_string(), line, z)
            })
            .filter(|(_, _, z)| stage1.contains_key(z) && base.contains_key(z) && stage2.contains_key(z))
            .collect();
        let bundled = assign_of(&features, &FlopClusters::recommended());
        let rows: Vec<String> = leaves
            .par_iter()
            .map(|(band, line, z)| {
                let (a1, a2, b2) = (&stage1[z], &stage2[z], &base[z]);
                let used = assign_of(&features, &features.groups(a1, 49).unwrap());
                let fresh = assign_of(&features, &features.groups(a2, 49).unwrap());
                let c = cost(&features, a2, &fresh);
                format!(
                    "{band}\t{:.4}\t{:.4}\t{:.4}\t{:.3}\t{:.4}\t{:.4}\t{line}",
                    tv(a1, a2),
                    tv(b2, a2),
                    tv(a1, b2),
                    c,
                    cost(&features, a2, &used) / c - 1.0,
                    cost(&features, a2, &bundled) / c - 1.0,
                )
            })
            .collect();
        let mut text = String::from(
            "band\ttv_stage1_stage2\ttv_base2000_stage2\ttv_stage1_base2000\tcost_fresh\tex_used\tex_bundled\tline\n",
        );
        for r in rows {
            text.push_str(&r);
            text.push('\n');
        }
        std::fs::write(root.join("runs/own-drift.tsv"), text).unwrap();
    }
}
