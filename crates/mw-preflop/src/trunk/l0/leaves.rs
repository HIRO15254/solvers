use anyhow::{Result, ensure};
use nlh::{Card, rank_of};
use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use std::{collections::BTreeMap, time::Instant};

use super::eval::Reach;
use super::{Model, Terminal, Tree};
use crate::trunk::classes::{Classes, Ordering3};

pub(crate) struct LeafValues {
    pub values: Vec<Vec<[f64; 169]>>,
    pub t2: f64,
    pub t3: f64,
    pub k4: f64,
}

#[derive(Clone)]
pub(crate) struct ThreeItem {
    pub(crate) node: usize,
    pub(crate) hero: usize,
    pub(crate) q: Vec<(usize, f64)>,
    pub(crate) r: Vec<(usize, f64)>,
    #[cfg(test)]
    pub(crate) payoffs: Vec<f64>,
    pub(crate) terms: ThreeTerms,
}

/// Hero's fractional share of a three-way pot, two pairwise pots, and a constant.
fn three_basis() -> [[f64; 13]; 4] {
    let mut basis = [[0.0; 13]; 4];
    for o in Ordering3::ALL {
        let [h, x, y] = o.ranks();
        let top = h.max(x).max(y);
        basis[0][o.index()] = if h == top {
            1.0 / [h, x, y].iter().filter(|&&r| r == top).count() as f64
        } else {
            0.0
        };
        for (k, other) in [(1, x), (2, y)] {
            basis[k][o.index()] = if h > other {
                1.0
            } else if h == other {
                0.5
            } else {
                0.0
            };
        }
        basis[3][o.index()] = 1.0;
    }
    basis
}

#[derive(Clone)]
pub(crate) struct ThreeTerms {
    weights: [f64; 4],
    residuals: [f64; 13],
    pub(crate) mask: u16,
}

impl ThreeTerms {
    pub(crate) fn new(u: &[f64]) -> Self {
        let at = |h, x, y| u[Ordering3::from_ranks(h, x, y).index()];
        let constant = at(1, 3, 2); // X > Y > hero
        let a2 = at(2, 3, 1) - constant;
        let a1 = at(2, 1, 3) - constant;
        let a3 = at(3, 2, 1) - a1 - a2 - constant;
        let weights = [a3, a1, a2, constant];
        let basis = three_basis();
        let tolerance = 1e-12 * (1.0 + u.iter().copied().map(f64::abs).fold(0.0, f64::max));
        let residuals = std::array::from_fn(|o| {
            let delta = u[o] - (0..4).map(|k| weights[k] * basis[k][o]).sum::<f64>();
            if delta.abs() <= tolerance { 0.0 } else { delta }
        });
        let mask = (0..13).fold(0, |mask, o| mask | (u16::from(residuals[o] != 0.0) << o));
        Self {
            weights,
            residuals,
            mask,
        }
    }
}

/// Fixed-order bilinear forms, batched only with identical correction masks.
/// Independent item accumulators make grouping and class/thread order irrelevant.
pub(crate) fn three_values(
    c: usize,
    items: &[ThreeItem],
    tree: &Tree,
    model: &Model<'_>,
    reach: &[Reach],
) -> Result<Vec<(usize, usize, f64)>> {
    let mut selected: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let terminal = tree.nodes[item.node].terminal.as_ref().unwrap();
            let folded: f64 = (0..tree.seats)
                .filter(|j| !terminal.active.contains(j))
                .map(|j| reach[item.node].mass(j, c))
                .product();
            (model.weights[item.hero][c] > 0.0
                && !item.q.is_empty()
                && !item.r.is_empty()
                && folded != 0.0)
                .then_some((item, folded))
        })
        .collect();
    selected.sort_by_key(|(item, _)| (item.terms.mask, item.node, item.hero));
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let support: Vec<_> = (0..169)
        .filter(|&d| model.weights.iter().any(|w| w[d] > 0.0))
        .collect();
    let corrections = selected
        .iter()
        .fold(0, |mask, (item, _)| mask | item.terms.mask);
    let basis = three_basis();
    let catalog = Classes::get();
    let mut slab = vec![0.0; 17 * 169 * 169];
    for &d in &support {
        for &e in &support {
            let p = model.tables.p3(c, d, e)?;
            for o in 0..13 {
                let value = f64::from(catalog.k(c, d)) * f64::from(catalog.k(c, e)) * p[o];
                for k in 0..4 {
                    slab[(k * 169 + d) * 169 + e] += basis[k][o] * value;
                }
                if corrections & (1 << o) != 0 {
                    slab[((4 + o) * 169 + d) * 169 + e] = value;
                }
            }
        }
    }
    let mut output = Vec::with_capacity(selected.len());
    let mut remaining = selected.as_slice();
    while let Some((first, _)) = remaining.first() {
        let mask = first.terms.mask;
        let count = remaining.partition_point(|(item, _)| item.terms.mask == mask);
        let (group, rest) = remaining.split_at(count);
        remaining = rest;
        for block in group.chunks(16) {
            let mut q = [[0.0; 16]; 169];
            let mut r = [[0.0; 16]; 169];
            for (i, (item, _)) in block.iter().enumerate() {
                for &(d, v) in &item.q {
                    q[d][i] = v;
                }
                for &(e, v) in &item.r {
                    r[e][i] = v;
                }
            }
            let ds: Vec<_> = support
                .iter()
                .copied()
                .filter(|&d| q[d].iter().any(|&v| v != 0.0))
                .collect();
            let es: Vec<_> = support
                .iter()
                .copied()
                .filter(|&e| r[e].iter().any(|&v| v != 0.0))
                .collect();
            let mut total = [0.0; 16];
            for k in (0..4).chain((0..13).filter(|&o| mask & (1 << o) != 0).map(|o| 4 + o)) {
                let mut outcome = [0.0; 16];
                for &d in &ds {
                    let mut inner = [0.0; 16];
                    for &e in &es {
                        let value = slab[(k * 169 + d) * 169 + e];
                        for i in 0..16 {
                            inner[i] += value * r[e][i];
                        }
                    }
                    for i in 0..16 {
                        outcome[i] += q[d][i] * inner[i];
                    }
                }
                for (i, (item, _)) in block.iter().enumerate() {
                    let weight = if k < 4 {
                        item.terms.weights[k]
                    } else {
                        item.terms.residuals[k - 4]
                    };
                    total[i] += outcome[i] * weight;
                }
            }
            for (i, (item, folded)) in block.iter().enumerate() {
                output.push((item.node, item.hero, folded * total[i]));
            }
        }
    }
    Ok(output)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct K4Plan {
    pub samples: u64,
    pub min_samples: Option<u64>,
    pub seed: u64,
}

impl K4Plan {
    pub(crate) fn model(model: &Model<'_>) -> Self {
        Self {
            samples: model.options.k4_samples,
            min_samples: None,
            seed: model.options.seed,
        }
    }
    pub(crate) fn allocation(self, mass: f64, reference: f64) -> u64 {
        match self.min_samples {
            None => self.samples,
            Some(min) => {
                ((self.samples as f64 * mass / reference).ceil() as u64).clamp(min, self.samples)
            }
        }
    }
}

fn opponent_mass(reach: &Reach, seats: usize, hero: usize, c: usize) -> f64 {
    (0..seats)
        .filter(|&j| j != hero)
        .map(|j| reach.mass(j, c))
        .product()
}

/// One [o][d][e] slab per hero class, shared by every seat and terminal.
/// Sixteen items form the innermost loop so the table is streamed once per
/// cache-sized block, with independent fixed-order accumulations per item.
#[cfg(test)]
pub(crate) fn three_values_reference(
    c: usize,
    items: &[ThreeItem],
    tree: &Tree,
    model: &Model<'_>,
    reach: &[Reach],
) -> Result<Vec<(usize, usize, f64)>> {
    let catalog = Classes::get();
    let support: Vec<_> = (0..169)
        .filter(|&d| model.weights.iter().any(|w| w[d] > 0.0))
        .collect();
    let mut slab = vec![0.0; 13 * 169 * 169];
    for &d in &support {
        for &e in &support {
            let p = model.tables.p3(c, d, e)?;
            for o in 0..13 {
                slab[(o * 169 + d) * 169 + e] =
                    f64::from(catalog.k(c, d)) * f64::from(catalog.k(c, e)) * p[o];
            }
        }
    }
    let selected: Vec<_> = items
        .iter()
        .filter(|item| {
            model.weights[item.hero][c] > 0.0 && !item.q.is_empty() && !item.r.is_empty()
        })
        .collect();
    let mut output = Vec::with_capacity(selected.len());
    for block in selected.chunks(16) {
        let mut q = [[0.0; 16]; 169];
        let mut r = [[0.0; 16]; 169];
        for (i, item) in block.iter().enumerate() {
            for &(d, v) in &item.q {
                q[d][i] = v;
            }
            for &(e, v) in &item.r {
                r[e][i] = v;
            }
        }
        let ds: Vec<_> = support
            .iter()
            .copied()
            .filter(|&d| q[d].iter().any(|&v| v != 0.0))
            .collect();
        let es: Vec<_> = support
            .iter()
            .copied()
            .filter(|&e| r[e].iter().any(|&v| v != 0.0))
            .collect();
        let mut total = [0.0; 16];
        for o in 0..13 {
            let mut outcome = [0.0; 16];
            for &d in &ds {
                let mut inner = [0.0; 16];
                for &e in &es {
                    let t = slab[(o * 169 + d) * 169 + e];
                    for i in 0..16 {
                        inner[i] += t * r[e][i];
                    }
                }
                for i in 0..16 {
                    outcome[i] += q[d][i] * inner[i];
                }
            }
            for (i, item) in block.iter().enumerate() {
                total[i] += outcome[i] * item.payoffs[o];
            }
        }
        for (i, item) in block.iter().enumerate() {
            let t = tree.nodes[item.node].terminal.as_ref().unwrap();
            let folded: f64 = (0..tree.seats)
                .filter(|j| !t.active.contains(j))
                .map(|j| reach[item.node].mass(j, c))
                .product();
            output.push((item.node, item.hero, folded * total[i]));
        }
    }
    Ok(output)
}

fn sample_key(hero: usize, c: usize, active_mask: u64, seed: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.p2.trunk.l0.k4.v1");
    for x in [seed, hero as u64, c as u64, active_mask] {
        hasher.update(&x.to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

/// Replay a shared prefix of an exact ChaCha8 sample stream. Rejection draws
/// can consume arbitrarily many words; the fallback resumes the original RNG
/// at the exact word position. This changes neither draws nor their order.
struct SampleRng<'a> {
    words: &'a [u32],
    position: usize,
    key: [u8; 32],
    stream: u64,
    tail: Option<ChaCha8Rng>,
}

impl RngCore for SampleRng<'_> {
    fn next_u32(&mut self) -> u32 {
        if self.position < self.words.len() {
            let word = self.words[self.position];
            self.position += 1;
            word
        } else {
            self.tail
                .get_or_insert_with(|| {
                    let mut rng = ChaCha8Rng::from_seed(self.key);
                    rng.set_stream(self.stream);
                    rng.set_word_pos(self.position as u128);
                    rng
                })
                .next_u32()
        }
    }
    fn next_u64(&mut self) -> u64 {
        u64::from(self.next_u32()) | (u64::from(self.next_u32()) << 32)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(4) {
            chunk.copy_from_slice(&self.next_u32().to_le_bytes()[..chunk.len()]);
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

#[cfg(test)]
#[test]
fn shared_random_prefix_replays_chacha_including_tail_and_mixed_draws() {
    let key = sample_key(2, 14, 31, 92);
    for stream in [0, 1, 999] {
        let mut original = ChaCha8Rng::from_seed(key);
        original.set_stream(stream);
        let words: [u32; 7] = std::array::from_fn(|_| original.next_u32());
        original.set_word_pos(0);
        let mut replay = SampleRng {
            words: &words,
            position: 0,
            key,
            stream,
            tail: None,
        };
        for _ in 0..80 {
            assert_eq!(original.next_u32(), replay.next_u32());
            assert_eq!(original.next_u64(), replay.next_u64());
            assert_eq!(original.r#gen::<f64>(), replay.r#gen::<f64>());
            assert_eq!(original.gen_range(0..52_u8), replay.gen_range(0..52_u8));
            assert_eq!(
                original.gen_range(0..11_usize),
                replay.gen_range(0..11_usize)
            );
        }
    }
}

fn sample_value(
    t: &Terminal,
    hero: usize,
    c: usize,
    model: &Model<'_>,
    reach: &Reach,
    key: [u8; 32],
    random_words: &[[u32; 64]],
) -> Result<f64> {
    let mass: f64 = (0..model.weights.len())
        .filter(|&j| j != hero)
        .map(|j| reach.mass(j, c))
        .product();
    if mass == 0.0 {
        return Ok(0.0);
    }
    let catalog = Classes::get();
    let h = catalog.representative(c);
    let others: Vec<_> = t.active.iter().copied().filter(|&j| j != hero).collect();
    let cdfs: Vec<_> = others
        .iter()
        .map(|&j| {
            let mut sum = 0.0;
            reach
                .rho(model, j)
                .into_iter()
                .filter_map(|(d, r)| {
                    let w = f64::from(catalog.k(c, d)) * r;
                    if w == 0.0 {
                        None
                    } else {
                        sum += w;
                        Some((d, sum))
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();
    // Memoize only this item's patterns; publish shared misses once sampling ends.
    let mut cache = FxHashMap::default();
    let mut new_entries = Vec::new();
    let mut total = 0.0;
    for (s, words) in random_words.iter().enumerate() {
        let mut rng = SampleRng {
            words,
            position: 0,
            key,
            stream: s as u64,
            tail: None,
        };
        let mut combos = [h; 9];
        let mut dead = catalog.combo_mask(h);
        for (a, cdf) in cdfs.iter().enumerate() {
            let draw = rng.r#gen::<f64>() * cdf.last().unwrap().1;
            let d = cdf[cdf
                .partition_point(|&(_, end)| end <= draw)
                .min(cdf.len() - 1)]
            .0;
            let live = &model.live[c][d];
            let v = live[rng.gen_range(0..live.len())];
            combos[others[a]] = v;
            dead |= catalog.combo_mask(v);
        }
        let board: [Card; 5] = std::array::from_fn(|_| {
            loop {
                let i = rng.gen_range(0..52_u8);
                let bit = 1_u64 << i;
                if dead & bit == 0 {
                    dead |= bit;
                    break Card::from_index(i);
                }
            }
        });
        let mut ranks = [0; 9];
        for (i, &j) in t.active.iter().enumerate() {
            ranks[i] = rank_of(board.into_iter().chain(catalog.cards(combos[j]))).0;
        }
        let mut pattern = 0_u64;
        for (i, &rank) in ranks[..t.active.len()].iter().enumerate() {
            let order = ranks[..t.active.len()]
                .iter()
                .filter(|&&r| r < rank)
                .count() as u64
                + 1;
            pattern |= order << (4 * i);
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(pattern) {
            let shared = t
                .cache
                .read()
                .map_err(|_| anyhow::anyhow!("ordering cache poisoned"))?
                .get(&pattern)
                .copied();
            let payoff = if let Some(payoff) = shared {
                payoff
            } else {
                // The read guard is dropped before computing settlement.
                let ordering: Vec<_> = (0..t.active.len())
                    .map(|i| ((pattern >> (4 * i)) & 15) as u16)
                    .collect();
                let u = t.ranked(model.game(), &ordering)?;
                for (j, &utility) in u.iter().enumerate() {
                    if !t.active.contains(&j) {
                        ensure!(
                            utility == t.payoffs[0][j],
                            "folded payoff depends on ordering"
                        );
                    }
                }
                let mut payoff = [0.0; 9];
                payoff[..u.len()].copy_from_slice(&u);
                new_entries.push((pattern, payoff));
                payoff
            };
            entry.insert(payoff);
        }
        total += cache[&pattern][hero];
    }
    if !new_entries.is_empty() {
        let mut shared = t
            .cache
            .write()
            .map_err(|_| anyhow::anyhow!("ordering cache poisoned"))?;
        for (pattern, payoff) in new_entries {
            shared.entry(pattern).or_insert(payoff);
        }
    }
    Ok(mass * (total / random_words.len() as f64))
}

/// Hero-conditioned terminal values, excluding the hero's own reach/weight.
/// Filtering heroes preserves each contraction and sample stream's order.
pub(crate) fn leaf_values(
    tree: &Tree,
    model: &Model<'_>,
    reach: &[Reach],
    heroes: &[usize],
    plan: K4Plan,
) -> Result<LeafValues> {
    let n = tree.nodes.len();
    let mut leaves = vec![vec![[0.0; 169]; n]; tree.seats];
    let start = Instant::now();
    let mut three = Vec::new();
    let mut sampled = Vec::new();
    let support: Vec<_> = (0..169)
        .filter(|&c| model.weights.iter().any(|w| w[c] > 0.0))
        .collect();
    let mut t2_slab = vec![[0.0; 3]; 169 * 169];
    if tree.terminal_counts()[2] > 0 {
        for &c in &support {
            for &d in &support {
                t2_slab[c * 169 + d] = model.tables.t2(c, d)?;
            }
        }
    }
    for (z, node) in tree.nodes.iter().enumerate() {
        let Some(t) = &node.terminal else { continue };
        for (p, seat_leaves) in leaves.iter_mut().enumerate() {
            if !heroes.contains(&p) {
                continue;
            }
            // L1 leaves keep their exact checkdown value: the L1 solver and
            // evaluator replace it, or correct it with sampled boards.
            if (t.active.len() >= 4 || (t.active.len() == 3 && model.sample_three_way()))
                && t.active.contains(&p)
            {
                sampled.extend(model.support[p].iter().map(|&c| (z, p, c)));
                continue;
            }
            if t.active.len() == 3 && t.active.contains(&p) {
                let others: Vec<_> = t.active.iter().copied().filter(|&j| j != p).collect();
                three.push(ThreeItem {
                    node: z,
                    hero: p,
                    q: reach[z].rho(model, others[0]),
                    r: reach[z].rho(model, others[1]),
                    terms: ThreeTerms::new(&t.hero_payoffs(p)),
                    #[cfg(test)]
                    payoffs: t.hero_payoffs(p),
                });
                continue;
            }
            let payoff = if t.active.len() == 2 && t.active.contains(&p) {
                t.hero_payoffs(p)
            } else {
                Vec::new()
            };
            let other = t.active.iter().copied().find(|&j| j != p);
            let rho = other.map(|j| reach[z].rho(model, j)).unwrap_or_default();
            for &c in &model.support[p] {
                seat_leaves[z][c] = if payoff.is_empty() {
                    t.payoffs[0][p]
                        * (0..tree.seats)
                            .filter(|&j| j != p)
                            .map(|j| reach[z].mass(j, c))
                            .product::<f64>()
                } else {
                    let folded: f64 = (0..tree.seats)
                        .filter(|j| !t.active.contains(j))
                        .map(|j| reach[z].mass(j, c))
                        .product();
                    let mut outcomes = [0.0; 3];
                    for &(d, r) in &rho {
                        let t2 = t2_slab[c * 169 + d];
                        for o in 0..3 {
                            outcomes[o] += t2[o] * r;
                        }
                    }
                    folded * (0..3).map(|o| payoff[o] * outcomes[o]).sum::<f64>()
                };
            }
        }
    }
    let t2_seconds = start.elapsed().as_secs_f64();
    let start = Instant::now();
    if !three.is_empty() {
        let classes: Vec<_> = (0..169)
            .filter(|&c| model.weights.iter().any(|w| w[c] > 0.0))
            .collect();
        let outputs: Vec<_> = classes
            .par_iter()
            .map(|&c| three_values(c, &three, tree, model, reach))
            .collect::<Result<_>>()?;
        for (c, output) in classes.into_iter().zip(outputs) {
            for (z, p, v) in output {
                leaves[p][z][c] = v;
            }
        }
    }
    let t3_seconds = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let mut groups: BTreeMap<(usize, usize, u64), Vec<usize>> = BTreeMap::new();
    for (z, p, c) in sampled {
        if (0..tree.seats).any(|j| j != p && reach[z].mass(j, c) == 0.0) {
            continue;
        }
        let t = tree.nodes[z].terminal.as_ref().unwrap();
        let mask = t.active.iter().fold(0_u64, |mask, &j| mask | (1 << j));
        groups.entry((p, c, mask)).or_default().push(z);
    }
    let mut reference = vec![[0.0_f64; 169]; tree.seats];
    if plan.min_samples.is_some() {
        for (&(p, c, _), nodes) in &groups {
            for &z in nodes {
                reference[p][c] = reference[p][c].max(opponent_mass(&reach[z], tree.seats, p, c));
            }
        }
    }
    let groups: Vec<_> = groups.into_iter().collect();
    let samples: Vec<_> = groups
        .par_iter()
        .map(|((p, c, mask), nodes)| {
            let key = sample_key(*p, *c, *mask, plan.seed);
            let random_words: Vec<[u32; 64]> = (0..plan.samples)
                .map(|s| {
                    let mut rng = ChaCha8Rng::from_seed(key);
                    rng.set_stream(s);
                    std::array::from_fn(|_| rng.next_u32())
                })
                .collect();
            nodes
                .iter()
                .map(|&z| {
                    sample_value(
                        tree.nodes[z].terminal.as_ref().unwrap(),
                        *p,
                        *c,
                        model,
                        &reach[z],
                        key,
                        &random_words[..plan.allocation(
                            opponent_mass(&reach[z], tree.seats, *p, *c),
                            reference[*p][*c],
                        ) as usize],
                    )
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<_>>()?;
    for (((p, c, _), nodes), values) in groups.into_iter().zip(samples) {
        for (z, v) in nodes.into_iter().zip(values) {
            leaves[p][z][c] = v;
        }
    }
    let k4_seconds = start.elapsed().as_secs_f64();
    Ok(LeafValues {
        values: leaves,
        t2: t2_seconds,
        t3: t3_seconds,
        k4: k4_seconds,
    })
}

#[cfg(test)]
fn sample_value_reference(
    t: &Terminal,
    hero: usize,
    c: usize,
    model: &Model<'_>,
    reach: &Reach,
    key: [u8; 32],
    random_words: &[[u32; 64]],
) -> Result<f64> {
    let mass: f64 = (0..model.weights.len())
        .filter(|&j| j != hero)
        .map(|j| reach.mass(j, c))
        .product();
    if mass == 0.0 {
        return Ok(0.0);
    }
    let catalog = Classes::get();
    let h = catalog.representative(c);
    let others: Vec<_> = t.active.iter().copied().filter(|&j| j != hero).collect();
    let cdfs: Vec<_> = others
        .iter()
        .map(|&j| {
            let mut sum = 0.0;
            reach
                .rho(model, j)
                .into_iter()
                .filter_map(|(d, r)| {
                    let w = f64::from(catalog.k(c, d)) * r;
                    if w == 0.0 {
                        None
                    } else {
                        sum += w;
                        Some((d, sum))
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();
    // Item-local cache avoids serializing parallel sampling on a terminal lock.
    let mut cache = t
        .cache
        .read()
        .map_err(|_| anyhow::anyhow!("ordering cache poisoned"))?
        .clone();
    let mut total = 0.0;
    for (s, words) in random_words.iter().enumerate() {
        let mut rng = SampleRng {
            words,
            position: 0,
            key,
            stream: s as u64,
            tail: None,
        };
        let mut combos = [h; 9];
        let mut dead = catalog.combo_mask(h);
        for (a, cdf) in cdfs.iter().enumerate() {
            let draw = rng.r#gen::<f64>() * cdf.last().unwrap().1;
            let d = cdf[cdf
                .partition_point(|&(_, end)| end <= draw)
                .min(cdf.len() - 1)]
            .0;
            let live = &model.live[c][d];
            let v = live[rng.gen_range(0..live.len())];
            combos[others[a]] = v;
            dead |= catalog.combo_mask(v);
        }
        let board: [Card; 5] = std::array::from_fn(|_| {
            loop {
                let i = rng.gen_range(0..52_u8);
                let bit = 1_u64 << i;
                if dead & bit == 0 {
                    dead |= bit;
                    break Card::from_index(i);
                }
            }
        });
        let mut ranks = [0; 9];
        for (i, &j) in t.active.iter().enumerate() {
            ranks[i] = rank_of(board.into_iter().chain(catalog.cards(combos[j]))).0;
        }
        let mut pattern = 0_u64;
        for (i, &rank) in ranks[..t.active.len()].iter().enumerate() {
            let order = ranks[..t.active.len()]
                .iter()
                .filter(|&&r| r < rank)
                .count() as u64
                + 1;
            pattern |= order << (4 * i);
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(pattern) {
            let ordering: Vec<_> = (0..t.active.len())
                .map(|i| ((pattern >> (4 * i)) & 15) as u16)
                .collect();
            let u = t.ranked(model.game(), &ordering)?;
            for (j, &utility) in u.iter().enumerate() {
                if !t.active.contains(&j) {
                    ensure!(
                        utility == t.payoffs[0][j],
                        "folded payoff depends on ordering"
                    );
                }
            }
            let mut payoff = [0.0; 9];
            payoff[..u.len()].copy_from_slice(&u);
            entry.insert(payoff);
        }
        total += cache[&pattern][hero];
    }
    t.cache
        .write()
        .map_err(|_| anyhow::anyhow!("ordering cache poisoned"))?
        .extend(cache);
    // For the model-plan regression words.len() is exactly model K4 samples.
    Ok(mass * (total / random_words.len() as f64))
}

#[cfg(test)]
pub(crate) struct SampleReference {
    pub node: usize,
    pub hero: usize,
    pub class: usize,
    pub samples: u64,
    pub value: f64,
}

/// Independent item-by-item reference, without production groups or allocation.
/// A model plan runs exactly the original fixed-budget computation; scaled plans
/// independently select the stream prefix and its normalization.
#[cfg(test)]
pub(crate) fn k4_reference(
    tree: &Tree,
    model: &Model<'_>,
    reach: &[Reach],
    plan: K4Plan,
) -> Result<Vec<SampleReference>> {
    let mut output = Vec::new();
    for (z, node) in tree.nodes.iter().enumerate() {
        let Some(t) = &node.terminal else { continue };
        if t.active.len() < 4 {
            continue;
        }
        let mask = t.active.iter().fold(0, |mask, &j| mask | (1 << j));
        for &p in &t.active {
            for &c in &model.support[p] {
                let mass: f64 = (0..tree.seats)
                    .filter(|&j| j != p)
                    .map(|j| reach[z].mass(j, c))
                    .product();
                let samples = if let Some(min) = plan.min_samples {
                    let maximum = tree
                        .nodes
                        .iter()
                        .enumerate()
                        .filter(|(_, node)| {
                            node.terminal
                                .as_ref()
                                .is_some_and(|t| t.active.len() >= 4 && t.active.contains(&p))
                        })
                        .map(|(node, _)| {
                            (0..tree.seats)
                                .filter(|&j| j != p)
                                .map(|j| reach[node].mass(j, c))
                                .product::<f64>()
                        })
                        .fold(0.0, f64::max);
                    ((plan.samples as f64 * mass / maximum).ceil() as u64).clamp(min, plan.samples)
                } else {
                    plan.samples
                };
                let key = sample_key(p, c, mask, plan.seed);
                let words: Vec<_> = (0..samples)
                    .map(|sample| {
                        let mut rng = ChaCha8Rng::from_seed(key);
                        rng.set_stream(sample);
                        std::array::from_fn(|_| rng.next_u32())
                    })
                    .collect();
                output.push(SampleReference {
                    node: z,
                    hero: p,
                    class: c,
                    samples,
                    value: sample_value_reference(t, p, c, model, &reach[z], key, &words)?,
                });
            }
        }
    }
    Ok(output)
}
