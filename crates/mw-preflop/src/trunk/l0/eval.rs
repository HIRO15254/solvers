use anyhow::{Result, ensure};
use nlh::{Card, rank_of};
use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use serde::Serialize;
use std::{collections::BTreeMap, time::Instant};

use super::{Profile, Terminal, Tree};
use crate::trunk::{
    classes::Classes,
    tables::{HuShowdownTable, ThreeWayTable},
};
use crate::{ExternalSamplingGame, FeatureHashAbstraction, HoldemGame};

/// Lookups occur only while preparing class slabs, never in contraction loops.
/// T2 is unnormalized (sums to K); T3 is hero/first/second oriented, including
/// the opponent swap convention of `ThreeWayTable::p3`.
pub trait Tables: Sync {
    fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]>;
    fn p3(&self, c: usize, d: usize, e: usize) -> Result<[f64; 13]>;
}

impl Tables for (&HuShowdownTable, &ThreeWayTable) {
    fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
        self.0.t2(c, d)
    }
    fn p3(&self, c: usize, d: usize, e: usize) -> Result<[f64; 13]> {
        Ok(self.1.p3(c, d, e)?.map(f64::from))
    }
}

#[derive(Clone, Copy)]
pub struct EvaluationOptions {
    pub k4_samples: u64,
    pub seed: u64,
    pub top_infosets: usize,
    #[cfg(test)]
    pub(crate) sample_three_way: bool,
}

impl Default for EvaluationOptions {
    fn default() -> Self {
        Self {
            k4_samples: 2048,
            seed: 0,
            top_infosets: 20,
            #[cfg(test)]
            sample_three_way: false,
        }
    }
}

pub struct Model<'a> {
    game: &'a HoldemGame<FeatureHashAbstraction>,
    tables: &'a dyn Tables,
    options: EvaluationOptions,
    weights: Vec<[f64; 169]>,
    pub warnings: Vec<String>,
    support: Vec<Vec<usize>>,
    live: Vec<Vec<Vec<usize>>>,
    normalizers: Vec<f64>,
}

impl<'a> Model<'a> {
    pub fn weights(&self) -> &[[f64; 169]] {
        &self.weights
    }

    pub fn game(&self) -> &HoldemGame<FeatureHashAbstraction> {
        self.game
    }

    pub fn tables(&self) -> &dyn Tables {
        self.tables
    }

    pub fn new(
        game: &'a HoldemGame<FeatureHashAbstraction>,
        tables: &'a dyn Tables,
        options: EvaluationOptions,
    ) -> Result<Self> {
        game.require_l0_chip_ev()?;
        ensure!(options.k4_samples > 0, "k4 sample count must be positive");
        let catalog = Classes::get();
        let mut warnings = Vec::new();
        let weights: Vec<_> = game
            .config()
            .seats
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = catalog.weights(&s.range);
                warnings.extend(w.warnings.iter().map(|w| {
                    format!(
                        "seat {i}: suit-asymmetric {} ({}..{}), using class mean",
                        w.name, w.min, w.max
                    )
                }));
                w.weights
            })
            .collect();
        let support: Vec<Vec<usize>> = weights
            .iter()
            .map(|w| (0..169).filter(|&c| w[c] > 0.0).collect())
            .collect();
        let normalizers = deal_normalizers(&weights, &support)?;
        let live = (0..169)
            .map(|c| {
                let dead = catalog.combo_mask(catalog.representative(c));
                (0..169)
                    .map(|d| {
                        catalog
                            .combos(d)
                            .iter()
                            .copied()
                            .filter(|&v| catalog.combo_mask(v) & dead == 0)
                            .collect()
                    })
                    .collect()
            })
            .collect();
        Ok(Self {
            game,
            tables,
            options,
            weights,
            warnings,
            support,
            live,
            normalizers,
        })
    }

    fn sample_three_way(&self) -> bool {
        #[cfg(test)]
        {
            self.options.sample_three_way
        }
        #[cfg(not(test))]
        {
            false
        }
    }
}

pub(super) fn deal_normalizers(weights: &[[f64; 169]], support: &[Vec<usize>]) -> Result<Vec<f64>> {
    let catalog = Classes::get();
    let z: Vec<[f64; 169]> = weights
        .iter()
        .zip(support)
        .map(|(w, s)| {
            std::array::from_fn(|c| s.iter().map(|&d| f64::from(catalog.k(c, d)) * w[d]).sum())
        })
        .collect();
    let normalizers: Vec<_> = weights
        .iter()
        .enumerate()
        .map(|(p, w)| {
            support[p]
                .iter()
                .map(|&c| {
                    catalog.n(c) as f64
                        * w[c]
                        * (0..weights.len())
                            .filter(|&j| j != p)
                            .map(|j| z[j][c])
                            .product::<f64>()
                })
                .sum::<f64>()
        })
        .collect();
    ensure!(
        normalizers.iter().all(|&t| t.is_finite() && t > 0.0),
        "ranges have T_p = 0 (no hero-conditioned deal)"
    );
    Ok(normalizers)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LocalGain {
    pub path: Vec<String>,
    pub class: String,
    pub local_gain: f64,
    pub profile_row: Vec<f64>,
    pub best_action: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SeatEvaluation {
    pub seat: usize,
    pub name: String,
    pub value: f64,
    pub best_response: f64,
    pub gain: f64,
    /// Sum over defaulted (node,class) pairs. Nested defaulted decisions can
    /// count the same trajectory repeatedly, so this mass can exceed one.
    pub defaulted_mass: f64,
    /// Slot k contains reach with k active seats; slot zero is unused.
    pub reach_by_active_count: Vec<f64>,
    /// Contribution to value from terminals with k active seats.
    pub value_by_active_count: Vec<f64>,
    pub telescoping_residual: f64,
    pub top_infosets: Vec<LocalGain>,
}

#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub seats: Vec<SeatEvaluation>,
    /// Pure class best responses, indexed by node * 169 + class for each seat.
    #[serde(skip)]
    pub best_response_actions: Vec<Vec<u8>>,
    pub nash_conv: f64,
    pub warnings: Vec<String>,
    pub timings: PhaseTimings,
}

#[derive(Debug, Serialize)]
pub struct PhaseTimings {
    pub reaches: f64,
    pub t2: f64,
    pub t3: f64,
    pub k4: f64,
    pub backward: f64,
}

struct Reach {
    pi: Vec<f64>,
    mass: Vec<f64>,
}

impl Reach {
    fn pi(&self, seat: usize, c: usize) -> f64 {
        self.pi[seat * 169 + c]
    }
    fn mass(&self, seat: usize, c: usize) -> f64 {
        self.mass[seat * 169 + c]
    }
    fn rho(&self, model: &Model<'_>, seat: usize) -> Vec<(usize, f64)> {
        model.support[seat]
            .iter()
            .filter_map(|&d| {
                let r = model.weights[seat][d] * self.pi(seat, d);
                (r > 0.0).then_some((d, r))
            })
            .collect()
    }
}

fn reaches(tree: &Tree, profile: &Profile, model: &Model<'_>) -> Vec<Reach> {
    let mut reaches: Vec<Reach> = Vec::with_capacity(tree.nodes.len());
    for n in &tree.nodes {
        let mut pi = if let Some((p, _)) = n.parent {
            reaches[p].pi.clone()
        } else {
            vec![1.0; tree.seats * 169]
        };
        if let Some((p, a)) = n.parent {
            let actor = tree.nodes[p].actor.unwrap();
            for &d in &model.support[actor] {
                pi[actor * 169 + d] *= profile.row(tree, p, d)[a];
            }
        }
        reaches.push(Reach {
            pi,
            mass: vec![0.0; tree.seats * 169],
        });
    }
    let classes: Vec<_> = (0..169)
        .filter(|&c| model.weights.iter().any(|w| w[c] > 0.0))
        .collect();
    reaches.par_iter_mut().for_each(|reach| {
        for seat in 0..tree.seats {
            let rho = reach.rho(model, seat);
            for &c in &classes {
                reach.mass[seat * 169 + c] = rho
                    .iter()
                    .map(|&(d, r)| f64::from(Classes::get().k(c, d)) * r)
                    .sum();
            }
        }
    });
    reaches
}

#[derive(Clone)]
struct ThreeItem {
    node: usize,
    hero: usize,
    q: Vec<(usize, f64)>,
    r: Vec<(usize, f64)>,
    payoffs: Vec<f64>,
}

/// One [o][d][e] slab per hero class, shared by every seat and terminal.
/// Sixteen items form the innermost loop so the table is streamed once per
/// cache-sized block, with independent fixed-order accumulations per item.
fn three_values(
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
    // Item-local cache avoids serializing parallel sampling on a terminal lock.
    let mut cache = t
        .cache
        .lock()
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
            let u = t.ranked(model.game, &ordering)?;
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
        .lock()
        .map_err(|_| anyhow::anyhow!("ordering cache poisoned"))?
        .extend(cache);
    Ok(mass * (total / model.options.k4_samples as f64))
}

pub fn evaluate(tree: &Tree, profile: &Profile, model: &Model<'_>) -> Result<Evaluation> {
    profile.validate(tree)?;
    ensure!(
        tree.game_fingerprint == model.game.game_fingerprint(),
        "model/tree mismatch"
    );
    let start = Instant::now();
    let reach = reaches(tree, profile, model);
    let reach_seconds = start.elapsed().as_secs_f64();
    let n = tree.nodes.len();
    ensure!(
        tree.nodes.iter().all(|node| node.children.len() <= 254),
        "L0 nodes may have at most 254 actions"
    );
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
            .map(|&c| three_values(c, &three, tree, model, &reach))
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
    let groups: Vec<_> = groups.into_iter().collect();
    let samples: Vec<_> = groups
        .par_iter()
        .map(|((p, c, mask), nodes)| {
            let key = sample_key(*p, *c, *mask, model.options.seed);
            let random_words: Vec<[u32; 64]> = (0..model.options.k4_samples)
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
                        &random_words,
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
    let start = Instant::now();
    let mut seats = Vec::new();
    let mut best_response_actions = Vec::new();
    for (p, mut values) in leaves.into_iter().enumerate() {
        let mut actions = vec![u8::MAX; n * 169];
        for (z, node) in tree.nodes.iter().enumerate() {
            if node.actor == Some(p) {
                actions[z * 169..(z + 1) * 169].fill(0);
            }
        }
        let mut best = values.clone();
        let mut locals = Vec::new();
        let mut defaulted_mass = 0.0;
        let mut reach_by_active_count = vec![0.0; tree.seats + 1];
        let mut value_by_active_count = vec![0.0; tree.seats + 1];
        let mut local_total = 0.0;
        for z in (0..n).rev() {
            let node = &tree.nodes[z];
            for &c in &model.support[p] {
                let weight =
                    Classes::get().n(c) as f64 * model.weights[p][c] / model.normalizers[p];
                let prob = weight
                    * reach[z].pi(p, c)
                    * (0..tree.seats)
                        .filter(|&j| j != p)
                        .map(|j| reach[z].mass(j, c))
                        .product::<f64>();
                if let Some(t) = &node.terminal {
                    reach_by_active_count[t.active.len()] += prob;
                    value_by_active_count[t.active.len()] +=
                        weight * reach[z].pi(p, c) * values[z][c];
                    continue;
                }
                if node.actor == Some(p) {
                    if profile.is_defaulted(z, c) {
                        defaulted_mass += prob;
                    }
                    let row = profile.row(tree, z, c);
                    let mut maximum = f64::NEG_INFINITY;
                    let mut best_action = 0;
                    let mut mixed = 0.0;
                    let mut value = 0.0;
                    for (a, &child) in node.children.iter().enumerate() {
                        value += row[a] * values[child][c];
                        mixed += row[a] * best[child][c];
                        if best[child][c] > maximum {
                            maximum = best[child][c];
                            best_action = a;
                        }
                    }
                    values[z][c] = value;
                    best[z][c] = maximum;
                    actions[z * 169 + c] = best_action as u8;
                    let local = weight * reach[z].pi(p, c) * (maximum - mixed);
                    local_total += local;
                    if local > 0.0 {
                        locals.push((local, z, c, best_action));
                    }
                } else {
                    values[z][c] = node.children.iter().map(|&child| values[child][c]).sum();
                    best[z][c] = node.children.iter().map(|&child| best[child][c]).sum();
                }
            }
        }
        let aggregate = |v: &[f64; 169]| {
            model.support[p]
                .iter()
                .map(|&c| Classes::get().n(c) as f64 * model.weights[p][c] * v[c])
                .sum::<f64>()
                / model.normalizers[p]
        };
        let value = aggregate(&values[0]);
        ensure!(
            (value_by_active_count.iter().sum::<f64>() - value).abs() <= 1e-9 * (1.0 + value.abs()),
            "terminal values do not sum to value for seat {p}"
        );
        let best_response = aggregate(&best[0]);
        let gain = best_response - value;
        let residual = local_total - gain;
        ensure!(
            gain >= -1e-9 * (1.0 + value.abs()),
            "BR below profile value for seat {p}"
        );
        ensure!(
            residual.abs() <= 1e-9 * (1.0 + gain.abs()),
            "local gains do not telescope for seat {p}: {residual}"
        );
        let reach_sum: f64 = reach_by_active_count.iter().sum();
        ensure!(
            (reach_sum - 1.0).abs() <= 1e-9,
            "terminal reach does not sum to one for seat {p}: {reach_sum}"
        );
        locals.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        let top_infosets = locals
            .into_iter()
            .take(model.options.top_infosets)
            .map(|(local_gain, z, c, a)| LocalGain {
                path: tree.path(z),
                class: Classes::get().name(c),
                local_gain,
                profile_row: profile.row(tree, z, c).to_vec(),
                best_action: tree.nodes[z].labels[a].clone(),
            })
            .collect();
        seats.push(SeatEvaluation {
            seat: p,
            name: model.game.config().seats[crate::SeatId(p as u8)]
                .name
                .clone()
                .unwrap_or_else(|| format!("seat {p}")),
            value,
            best_response,
            gain: gain.max(0.0),
            defaulted_mass,
            reach_by_active_count,
            value_by_active_count,
            telescoping_residual: residual,
            top_infosets,
        });
        best_response_actions.push(actions);
    }
    Ok(Evaluation {
        nash_conv: seats.iter().map(|s| s.gain).sum(),
        seats,
        best_response_actions,
        warnings: model.warnings.clone(),
        timings: PhaseTimings {
            reaches: reach_seconds,
            t2: t2_seconds,
            t3: t3_seconds,
            k4: k4_seconds,
            backward: start.elapsed().as_secs_f64(),
        },
    })
}
