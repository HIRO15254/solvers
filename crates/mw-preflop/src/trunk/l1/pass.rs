use super::{Board, BucketSource, Subtree, cards::Q};
use crate::trunk::{
    classes::{Classes, class},
    l0::{Model, Tree, eval::Reach, solve::Discounts},
};
use nlh::NUM_COMBOS;

#[derive(Clone)]
pub struct LeafStrategy {
    pub terminal: usize,
    pub offsets: Vec<usize>,
    pub regrets: Vec<f64>,
    pub sums: Vec<f64>,
}

#[derive(Clone)]
pub struct Strategies {
    pub leaves: Vec<LeafStrategy>,
    pub nodes: usize,
    pub slots: usize,
}

impl Strategies {
    pub fn new(tree: &Tree, source: &dyn BucketSource) -> Self {
        let mut nodes = 0;
        let mut slots = 0;
        let leaves = tree
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(terminal, n)| {
                let t = n.terminal.as_ref()?.l1.as_ref()?;
                let mut size = 0;
                let offsets = t
                    .nodes
                    .iter()
                    .map(|n| {
                        let offset = size;
                        if n.actor.is_some() {
                            nodes += 1;
                            size += source.count(n.street) * n.children.len();
                        }
                        offset
                    })
                    .collect();
                slots += size;
                Some(LeafStrategy {
                    terminal,
                    offsets,
                    regrets: vec![0.0; size],
                    sums: vec![0.0; size],
                })
            })
            .collect();
        Self {
            leaves,
            nodes,
            slots,
        }
    }

    pub fn storage_bytes(&self) -> usize {
        self.slots * 16
    }
}

impl LeafStrategy {
    pub(crate) fn profile(&self, tree: &Subtree, average: bool) -> Vec<f64> {
        let mut rows = vec![0.0; self.regrets.len()];
        for (z, n) in tree.nodes.iter().enumerate() {
            if n.actor.is_none() {
                continue;
            }
            let start = self.offsets[z];
            let end = self.offsets.get(z + 1).copied().unwrap_or(rows.len());
            for (out, input) in rows[start..end].chunks_mut(n.children.len()).zip(
                if average {
                    &self.sums[start..end]
                } else {
                    &self.regrets[start..end]
                }
                .chunks(n.children.len()),
            ) {
                let sum: f64 = input.iter().map(|&r| r.max(0.0)).sum();
                let uniform = 1.0 / out.len() as f64;
                for (a, &r) in out.iter_mut().zip(input) {
                    *a = if sum > 0.0 { r.max(0.0) / sum } else { uniform };
                }
            }
        }
        rows
    }
}

/// Arena slabs are resized once per leaf, then reused across boards/leaves
/// within a Rayon worker. There are no allocations inside the node loops.
#[derive(Default)]
pub(crate) struct Scratch {
    opponent: Vec<f64>,
    own: Vec<f64>,
    pub(crate) values: Vec<f64>,
    pub(crate) best: Vec<f64>,
}

#[derive(Clone, Copy)]
struct Mass {
    total: f64,
    cards: [f64; 52],
    correction: f64,
    card_corrections: [f64; 52],
}

impl Default for Mass {
    fn default() -> Self {
        Self {
            total: 0.0,
            cards: [0.0; 52],
            correction: 0.0,
            card_corrections: [0.0; 52],
        }
    }
}

fn compensated_add(sum: &mut f64, correction: &mut f64, value: f64) {
    let adjusted = value - *correction;
    let next = *sum + adjusted;
    *correction = (next - *sum) - adjusted;
    *sum = next;
}

impl Mass {
    fn add(&mut self, h: usize, value: f64) {
        compensated_add(&mut self.total, &mut self.correction, value);
        for c in Classes::get().cards(h) {
            compensated_add(
                &mut self.cards[c.index()],
                &mut self.card_corrections[c.index()],
                value,
            );
        }
    }
    fn disjoint(&self, h: usize) -> f64 {
        let [a, b] = Classes::get().cards(h);
        self.total - self.cards[a.index()] - self.cards[b.index()]
    }
}

pub(crate) fn terminal_values(
    board: &Board,
    opponent: &[f64],
    payoffs: &[Vec<f64>],
    hero: usize,
    local: usize,
    output: &mut [f64],
) {
    output.fill(0.0);
    let mut total = Mass::default();
    for &h in &board.sorted {
        total.add(h, opponent[h]);
    }
    if payoffs.len() == 1 {
        for &h in &board.sorted {
            output[h] = (total.disjoint(h) + opponent[h]) * payoffs[0][hero];
        }
        return;
    }
    let win = payoffs[if local == 0 { 0 } else { 2 }][hero];
    let tie = payoffs[1][hero];
    let lose = payoffs[if local == 0 { 2 } else { 0 }][hero];
    let mut lower = Mass::default();
    for &(start, end) in &board.groups {
        let mut equal = Mass::default();
        for &h in &board.sorted[start..end] {
            equal.add(h, opponent[h]);
        }
        for &h in &board.sorted[start..end] {
            let w = lower.disjoint(h);
            let t = equal.disjoint(h) + opponent[h];
            let l = total.disjoint(h) + opponent[h] - w - t;
            output[h] = win * w + tie * t + lose * l;
        }
        compensated_add(&mut lower.total, &mut lower.correction, equal.total);
        for i in 0..52 {
            compensated_add(
                &mut lower.cards[i],
                &mut lower.card_corrections[i],
                equal.cards[i],
            );
        }
    }
}

pub(crate) struct Pass<'a> {
    pub tree: &'a Subtree,
    pub storage: &'a LeafStrategy,
    pub rows: &'a [f64],
    pub board: &'a Board,
    pub hero: usize,
    pub opponent: &'a [f64; NUM_COMBOS],
    pub own: &'a [f64; NUM_COMBOS],
    pub scale: &'a [f64; NUM_COMBOS],
    pub auxiliary: bool,
}

impl Scratch {
    pub(crate) fn pass(
        &mut self,
        pass: &Pass<'_>,
        mut updates: Option<(&mut [f64], &mut [f64], f64)>,
    ) {
        let Pass {
            tree,
            storage,
            rows,
            board,
            hero,
            opponent,
            own,
            scale,
            auxiliary,
        } = *pass;
        let local = usize::from(tree.active[0] != hero);
        let size = tree.nodes.len() * NUM_COMBOS;
        self.opponent.resize(size, 0.0);
        self.own.resize(size, 0.0);
        self.values.resize(size, 0.0);
        if auxiliary {
            self.best.resize(size, 0.0);
        }
        self.opponent[..NUM_COMBOS].copy_from_slice(opponent);
        self.own[..NUM_COMBOS].copy_from_slice(own);
        for (z, n) in tree.nodes.iter().enumerate() {
            if let Some((parent, a)) = n.parent {
                let pn = &tree.nodes[parent];
                let buckets = &board.buckets[pn.street.index() - 1];
                for &h in &board.sorted {
                    let probability =
                        rows[storage.offsets[parent] + buckets[h] as usize * pn.children.len() + a];
                    self.opponent[z * NUM_COMBOS + h] = self.opponent[parent * NUM_COMBOS + h]
                        * if pn.actor == Some(local) {
                            1.0
                        } else {
                            probability
                        };
                    self.own[z * NUM_COMBOS + h] = self.own[parent * NUM_COMBOS + h]
                        * if pn.actor == Some(local) {
                            probability
                        } else {
                            1.0
                        };
                }
            }
        }
        for (z, n) in tree.nodes.iter().enumerate().rev() {
            let offset = z * NUM_COMBOS;
            if n.actor.is_none() {
                terminal_values(
                    board,
                    &self.opponent[offset..offset + NUM_COMBOS],
                    &n.payoffs,
                    hero,
                    local,
                    &mut self.values[offset..offset + NUM_COMBOS],
                );
                if auxiliary {
                    self.best[offset..offset + NUM_COMBOS]
                        .copy_from_slice(&self.values[offset..offset + NUM_COMBOS]);
                }
                continue;
            }
            let buckets = &board.buckets[n.street.index() - 1];
            for &h in &board.sorted {
                let row = storage.offsets[z] + buckets[h] as usize * n.children.len();
                let mut value = 0.0;
                let mut best = if n.actor == Some(local) {
                    f64::NEG_INFINITY
                } else {
                    0.0
                };
                for (a, &child) in n.children.iter().enumerate() {
                    let child = child * NUM_COMBOS + h;
                    value += self.values[child]
                        * if n.actor == Some(local) {
                            rows[row + a]
                        } else {
                            1.0
                        };
                    if auxiliary {
                        if n.actor == Some(local) {
                            best = best.max(self.best[child]);
                        } else {
                            best += self.best[child];
                        }
                    }
                }
                self.values[offset + h] = value;
                if auxiliary {
                    self.best[offset + h] = best;
                }
                if n.actor == Some(local)
                    && let Some((regrets, sums, weight)) = updates.as_mut()
                {
                    for (a, &child) in n.children.iter().enumerate() {
                        regrets[row + a] +=
                            *weight * scale[h] * (self.values[child * NUM_COMBOS + h] - value);
                        sums[row + a] += *weight * self.own[offset + h] * rows[row + a];
                    }
                }
            }
        }
        // Overlapping hero combos contribute zero even when scratch was reused.
        for h in 0..NUM_COMBOS {
            if board.ranks[h] == 0 {
                self.values[h] = 0.0;
                if auxiliary {
                    self.best[h] = 0.0;
                }
            }
        }
    }
}

pub(crate) fn inputs(
    tree: &Tree,
    model: &Model<'_>,
    reach: &Reach,
    terminal: usize,
    hero: usize,
) -> ([f64; NUM_COMBOS], [f64; NUM_COMBOS], [f64; NUM_COMBOS]) {
    let active = &tree.nodes[terminal].terminal.as_ref().unwrap().active;
    let opponent = *active.iter().find(|&&s| s != hero).unwrap();
    let rho = |seat: usize, h: usize| {
        let c = class(h);
        model.weights[seat][c] * reach.pi(seat, c)
    };
    let folded: [f64; 169] = std::array::from_fn(|c| {
        (0..tree.seats)
            .filter(|s| !active.contains(s))
            .map(|s| reach.mass(s, c))
            .product::<f64>()
            / Q
    });
    (
        std::array::from_fn(|h| rho(opponent, h)),
        std::array::from_fn(|h| rho(hero, h)),
        std::array::from_fn(|h| folded[class(h)]),
    )
}

pub(crate) fn class_values(values: &[f64], scale: &[f64; NUM_COMBOS]) -> [f64; 169] {
    std::array::from_fn(|c| {
        Classes::get()
            .combos(c)
            .iter()
            .map(|&h| values[h] * scale[h])
            .sum::<f64>()
            / Classes::get().n(c) as f64
    })
}

pub(crate) fn discount(
    storage: &mut LeafStrategy,
    tree: &Subtree,
    hero: usize,
    increments: &[f64],
    sums: &[f64],
    discount: &Discounts,
) {
    for (z, n) in tree.nodes.iter().enumerate() {
        if n.actor.map(|i| tree.active[i]) != Some(hero) {
            continue;
        }
        let start = storage.offsets[z];
        let end = storage
            .offsets
            .get(z + 1)
            .copied()
            .unwrap_or(storage.regrets.len());
        for i in start..end {
            storage.regrets[i] += increments[i];
            storage.regrets[i] *= if storage.regrets[i] > 0.0 {
                discount.positive
            } else {
                discount.nonpositive
            };
            storage.sums[i] += discount.average * sums[i];
        }
    }
}
