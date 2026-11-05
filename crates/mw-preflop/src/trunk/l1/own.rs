//! Per-leaf flop groups from each L1 leaf's own ranges
//! (docs/plans/p2-method-redesign.jp.md P2D12): the stratified weighted
//! k-medoids of the 1755 isomorphic flops that built the bundled groups
//! ([`super::RECOMMENDED_FLOP_GROUPS`]), with each hand class's strength
//! difference counted by its share of the two ranges at the leaf.
use super::BucketSource;
use super::refine::{FlopClass, FlopClusters, canonical_flop};
use crate::trunk::classes::{Classes, class};
use crate::trunk::l0::{Model, Profile, Tree};
use anyhow::{Result, ensure};
use nlh::{Card, combo_cards};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use std::collections::BTreeMap;

/// The recommended flop groups per leaf, as many as the bundled groups.
pub const OWN_FLOP_GROUPS: usize = 49;
/// k-medoids searches per stratum; the cheapest is kept.
const RESTARTS: usize = 8;
/// Bound on a search's rounds of assignment and medoid update.
const ROUNDS: usize = 100;

/// The 1755 suit-isomorphism classes of flops with their raw flops and the
/// mean flop strength of every hand class's live combos.
pub struct FlopFeatures {
    /// Ascending.
    classes: Vec<FlopClass>,
    raw: Vec<f64>,
    /// Flop indices by stratum, the flop's suits (monotone, two-tone,
    /// rainbow) and pairing (trips, paired, none), which the class means
    /// barely see.
    strata: Vec<Vec<usize>>,
    /// `[flop][hand class]` mean strength bucket. A class without live
    /// combos (the pair of a trips flop's rank) has its mean over the flops.
    strength: Vec<[f64; 169]>,
}

impl FlopFeatures {
    /// Over `strength`, the 128-bucket EHS2 table of the relative buckets.
    pub fn new(strength: &dyn BucketSource) -> Self {
        let mut flops: BTreeMap<FlopClass, ([Card; 3], u32)> = BTreeMap::new();
        for a in 0..52_u8 {
            for b in a + 1..52 {
                for c in b + 1..52 {
                    let flop = [a, b, c].map(Card::from_index);
                    flops.entry(canonical_flop(flop)).or_insert((flop, 0)).1 += 1;
                }
            }
        }
        let distinct = |mut values: [u8; 3]| {
            values.sort_unstable();
            1 + usize::from(values[0] != values[1]) + usize::from(values[1] != values[2])
        };
        let mut strata = BTreeMap::<(usize, usize), Vec<usize>>::new();
        let mut means = Vec::with_capacity(flops.len());
        for (i, (flop, _)) in flops.values().enumerate() {
            let key = (
                distinct(flop.map(Card::suit)),
                distinct(flop.map(Card::rank)),
            );
            strata.entry(key).or_default().push(i);
            let row = strength.row(flop);
            let mut sums = [0.0; 169];
            let mut counts = [0_u32; 169];
            for (combo, &bucket) in row.iter().enumerate() {
                let (x, y) = combo_cards(combo);
                if !flop.contains(&x) && !flop.contains(&y) {
                    sums[class(combo)] += f64::from(bucket);
                    counts[class(combo)] += 1;
                }
            }
            let mean: [Option<f64>; 169] =
                std::array::from_fn(|c| (counts[c] > 0).then(|| sums[c] / f64::from(counts[c])));
            means.push(mean);
        }
        let fill: [f64; 169] = std::array::from_fn(|c| {
            let (sum, n) = means
                .iter()
                .filter_map(|m| m[c])
                .fold((0.0, 0.0), |(s, n), x| (s + x, n + 1.0));
            sum / n
        });
        Self {
            classes: flops.keys().copied().collect(),
            raw: flops.values().map(|&(_, n)| f64::from(n)).collect(),
            strata: strata.into_values().collect(),
            strength: means
                .iter()
                .map(|m| std::array::from_fn(|c| m[c].unwrap_or(fill[c])))
                .collect(),
        }
    }

    /// `k` groups of the flops for a leaf whose ranges hold the hand classes
    /// in the proportions `weights`. Every stratum gets one group plus its
    /// share of the raw flops of the other `k` − 6 (largest remainders, ties
    /// to the earlier stratum), and its groups are the clusters of weighted
    /// k-medoids (raw flops as point weights) by the Euclidean distance of
    /// the class strengths, class `c` scaled by `sqrt(weights[c])`. Each leaf
    /// starts from the same seed, so equal weights give equal groups.
    pub fn groups(&self, weights: &[f64; 169], k: usize) -> Result<FlopClusters> {
        ensure!(
            (self.strata.len()..=self.classes.len()).contains(&k),
            "own flop groups must number {}..={}, not {k}",
            self.strata.len(),
            self.classes.len()
        );
        ensure!(
            weights.iter().all(|w| w.is_finite() && *w >= 0.0) && weights.iter().any(|&w| w > 0.0),
            "class weights must be finite, nonnegative and not all zero"
        );
        let scale: Vec<(usize, f64)> = (0..169)
            .filter(|&c| weights[c] > 0.0)
            .map(|c| (c, weights[c].sqrt()))
            .collect();
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        let mut by_class = BTreeMap::new();
        let mut next = 0;
        for (members, k) in self.strata.iter().zip(self.shares(k)) {
            let points: Vec<Vec<f64>> = members
                .iter()
                .map(|&i| {
                    scale
                        .iter()
                        .map(|&(c, s)| self.strength[i][c] * s)
                        .collect()
                })
                .collect();
            let n = members.len();
            let mut distance = vec![0.0; n * n];
            for i in 0..n {
                for j in i + 1..n {
                    let d = points[i]
                        .iter()
                        .zip(&points[j])
                        .map(|(a, b)| (a - b) * (a - b))
                        .sum::<f64>()
                        .sqrt();
                    distance[i * n + j] = d;
                    distance[j * n + i] = d;
                }
            }
            let weight: Vec<f64> = members.iter().map(|&i| self.raw[i]).collect();
            let (_, assign) = (0..RESTARTS)
                .map(|_| kmedoids(&distance, &weight, k, &mut rng))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
            for (&i, &j) in members.iter().zip(&assign) {
                by_class.insert(self.classes[i], u16::try_from(next + j)?);
            }
            next += k;
        }
        FlopClusters::from_classes(&by_class)
    }

    /// Groups per stratum for `k` groups in all, at most the stratum's flops.
    fn shares(&self, k: usize) -> Vec<usize> {
        let total: f64 = self.raw.iter().sum();
        let extra = (k - self.strata.len()) as f64;
        let share: Vec<f64> = self
            .strata
            .iter()
            .map(|m| 1.0 + extra * m.iter().map(|&i| self.raw[i]).sum::<f64>() / total)
            .collect();
        let mut count: Vec<usize> = share.iter().map(|&s| s as usize).collect();
        let mut order: Vec<usize> = (0..share.len()).collect();
        order.sort_by(|&a, &b| {
            (share[b] - count[b] as f64).total_cmp(&(share[a] - count[a] as f64))
        });
        let left = k.saturating_sub(count.iter().sum());
        for &s in order.iter().take(left) {
            count[s] += 1;
        }
        count
            .iter()
            .zip(&self.strata)
            .map(|(&c, m)| c.min(m.len()))
            .collect()
    }
}

/// An index drawn with probability proportional to the nonnegative `p`
/// (positive sum).
fn draw(p: &[f64], rng: &mut ChaCha8Rng) -> usize {
    let mut r = rng.r#gen::<f64>() * p.iter().sum::<f64>();
    for (i, &x) in p.iter().enumerate() {
        if r < x {
            return i;
        }
        r -= x;
    }
    p.iter().rposition(|&x| x > 0.0).unwrap()
}

/// Weighted k-medoids of the points of the row-major distance matrix `d`
/// with weights `w`: weighted k-means++ seeding, then rounds of assignment
/// to the nearest medoid (a medoid keeps itself) and of moving each medoid
/// to its cluster's point of least weighted distance, until none moves.
/// Returns the mean weighted distance to the medoid and each point's cluster.
fn kmedoids(d: &[f64], w: &[f64], k: usize, rng: &mut ChaCha8Rng) -> (f64, Vec<usize>) {
    let n = w.len();
    let mut medoids = vec![draw(w, rng)];
    let mut nearest = vec![f64::INFINITY; n];
    while medoids.len() < k {
        let last = medoids[medoids.len() - 1];
        for (i, x) in nearest.iter_mut().enumerate() {
            *x = x.min(d[i * n + last]);
        }
        let p: Vec<f64> = (0..n).map(|i| nearest[i] * nearest[i] * w[i]).collect();
        // Only points at distance 0 from every medoid are left: take the first.
        let next = if p.iter().any(|&x| x > 0.0) {
            draw(&p, rng)
        } else {
            (0..n).find(|i| !medoids.contains(i)).unwrap()
        };
        medoids.push(next);
    }
    let assign = |medoids: &[usize]| -> Vec<usize> {
        (0..n)
            .map(|i| {
                medoids.iter().position(|&m| m == i).unwrap_or_else(|| {
                    (0..k)
                        .min_by(|&a, &b| d[i * n + medoids[a]].total_cmp(&d[i * n + medoids[b]]))
                        .unwrap()
                })
            })
            .collect()
    };
    for _ in 0..ROUNDS {
        let cluster = assign(&medoids);
        let mut moved = false;
        for (j, medoid) in medoids.iter_mut().enumerate() {
            let members: Vec<usize> = (0..n).filter(|&i| cluster[i] == j).collect();
            let cost = |c: usize| members.iter().map(|&i| d[i * n + c] * w[i]).sum::<f64>();
            let mut best = (*medoid, cost(*medoid));
            for &c in &members {
                let x = cost(c);
                if x < best.1 || (x == best.1 && c < best.0) {
                    best = (c, x);
                }
            }
            if best.0 != *medoid {
                *medoid = best.0;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let cluster = assign(&medoids);
    let cost = (0..n)
        .map(|i| d[i * n + medoids[cluster[i]]] * w[i])
        .sum::<f64>()
        / w.iter().sum::<f64>();
    (cost, cluster)
}

/// Each L1 leaf's hand class weights under `profile`, by tree node: for each
/// of the leaf's two seats, the combos of every class that reach it (the
/// starting range weight × the seat's own action probabilities on the line ×
/// the class's combos) normalized to sum 1, averaged over the seats that
/// reach the leaf. `None` when neither does.
pub fn leaf_weights(
    tree: &Tree,
    model: &Model<'_>,
    profile: &Profile,
) -> Vec<(usize, Option<[f64; 169]>)> {
    let catalog = Classes::get();
    tree.nodes
        .iter()
        .enumerate()
        .filter_map(|(z, n)| {
            let subtree = n.terminal.as_ref()?.l1.as_ref()?;
            let mut weights = [0.0; 169];
            let mut seats = 0.0;
            for &seat in &subtree.active {
                let mut mass: [f64; 169] =
                    std::array::from_fn(|c| model.weights[seat][c] * catalog.n(c) as f64);
                let mut at = z;
                while let Some((p, a)) = tree.nodes[at].parent {
                    if tree.nodes[p].actor == Some(seat) {
                        for (c, m) in mass.iter_mut().enumerate() {
                            *m *= profile.row(tree, p, c)[a];
                        }
                    }
                    at = p;
                }
                let total: f64 = mass.iter().sum();
                if total > 0.0 {
                    for (w, m) in weights.iter_mut().zip(mass) {
                        *w += m / total;
                    }
                    seats += 1.0;
                }
            }
            Some((z, (seats > 0.0).then(|| weights.map(|w| w / seats))))
        })
        .collect()
}

/// `k` own flop groups ([`FlopFeatures::groups`]) for every L1 leaf that a
/// seat reaches under `profile` ([`leaf_weights`]), by tree node, built in
/// parallel. Unreached leaves are left out.
pub fn own_flop_groups(
    tree: &Tree,
    model: &Model<'_>,
    profile: &Profile,
    features: &FlopFeatures,
    k: usize,
) -> Result<Vec<(usize, FlopClusters)>> {
    let reached: Vec<(usize, [f64; 169])> = leaf_weights(tree, model, profile)
        .into_iter()
        .filter_map(|(z, w)| Some((z, w?)))
        .collect();
    reached
        .par_iter()
        .map(|(z, w)| Ok((*z, features.groups(w, k)?)))
        .collect()
}
