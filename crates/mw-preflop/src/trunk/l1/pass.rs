use super::{Board, BucketSource, Subtree, cards::Q};
use crate::trunk::{
    classes::{Classes, class},
    l0::{Model, Terminal, Tree, eval::Reach, solve::Discounts},
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

/// Boards per parallel task. Chunks are fixed and summed in order, so results
/// do not depend on the thread count; up to one chunk keeps the sequential
/// summation order.
pub(crate) const CHUNK: usize = 8;

/// Per-depth slabs, grown to the deepest leaf tree and reused across boards and
/// leaves within a Rayon worker. A pass walks the tree depth first, so it only
/// touches the slabs along one path: a few hundred KB instead of every node's
/// combo arrays, which kept the pass waiting on memory.
#[derive(Default)]
pub(crate) struct Scratch {
    /// The opponent's and hero's reach per depth, `[depth][combo]`.
    opponent: Vec<f64>,
    own: Vec<f64>,
    /// Values per depth and action, `[depth][action][combo]`: a node's
    /// children leave theirs one depth below it. The root's are the first
    /// `NUM_COMBOS`, which is all callers read.
    pub(crate) values: Vec<f64>,
    pub(crate) best: Vec<f64>,
    checkdown: Vec<f64>,
}

/// One pass's inputs and its slab layout.
struct Walk<'p, 'a> {
    pass: &'p Pass<'a>,
    local: usize,
    /// The most actions at any node of the tree: the stride of a depth's values.
    width: usize,
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
    /// Class values of the leaf's checkdown line on one board: the control
    /// variate whose mean over all boards is the exact L0 (T2) value.
    pub(crate) fn checkdown(
        &mut self,
        board: &Board,
        terminal: &Terminal,
        hero: usize,
        opponent: &[f64; NUM_COMBOS],
        scale: &[f64; NUM_COMBOS],
    ) -> [f64; 169] {
        self.checkdown.resize(NUM_COMBOS, 0.0);
        terminal_values(
            board,
            opponent,
            &terminal.payoffs,
            hero,
            usize::from(terminal.active[0] != hero),
            &mut self.checkdown,
        );
        class_values(&self.checkdown, scale)
    }

    pub(crate) fn pass(
        &mut self,
        pass: &Pass<'_>,
        mut updates: Option<(&mut [f64], &mut [f64], f64)>,
    ) {
        let walk = Walk {
            pass,
            local: usize::from(pass.tree.active[0] != pass.hero),
            width: pass
                .tree
                .nodes
                .iter()
                .map(|n| n.children.len())
                .max()
                .unwrap_or(0)
                .max(1),
        };
        self.grow(&walk, 0);
        self.opponent[..NUM_COMBOS].copy_from_slice(pass.opponent);
        self.own[..NUM_COMBOS].copy_from_slice(pass.own);
        self.visit(&walk, &mut updates, 0, 0, [0, 0], 0);
        // Overlapping hero combos contribute zero even when scratch was reused.
        for h in 0..NUM_COMBOS {
            if pass.board.ranks[h] == 0 {
                self.values[h] = 0.0;
                if pass.auxiliary {
                    self.best[h] = 0.0;
                }
            }
        }
    }

    /// Makes room for a node at `depth` and its children's values.
    fn grow(&mut self, walk: &Walk<'_, '_>, depth: usize) {
        let reach = (depth + 1) * NUM_COMBOS;
        if self.opponent.len() < reach {
            self.opponent.resize(reach, 0.0);
            self.own.resize(reach, 0.0);
        }
        let values = (depth + 2) * walk.width * NUM_COMBOS;
        if self.values.len() < values {
            self.values.resize(values, 0.0);
        }
        if walk.pass.auxiliary && self.best.len() < values {
            self.best.resize(values, 0.0);
        }
    }

    /// Writes node `z`'s values at `out`, given the opponent's and hero's reach
    /// at the offsets `[opponent, own]`. An action leaves the other player's reach
    /// unchanged, so the child shares that slab instead of multiplying it by 1;
    /// a slab at `depth` is only rewritten after the subtrees that read it.
    /// The arithmetic and the order of every sum match a pass over all nodes
    /// at once, so results are bit-identical to it.
    fn visit(
        &mut self,
        walk: &Walk<'_, '_>,
        updates: &mut Option<(&mut [f64], &mut [f64], f64)>,
        z: usize,
        depth: usize,
        [opponent, own]: [usize; 2],
        out: usize,
    ) {
        let &Pass {
            tree,
            storage,
            rows,
            board,
            hero,
            scale,
            auxiliary,
            ..
        } = walk.pass;
        let local = walk.local;
        let n = &tree.nodes[z];
        if n.actor.is_none() {
            terminal_values(
                board,
                &self.opponent[opponent..opponent + NUM_COMBOS],
                &n.payoffs,
                hero,
                local,
                &mut self.values[out..out + NUM_COMBOS],
            );
            if auxiliary {
                self.best[out..out + NUM_COMBOS]
                    .copy_from_slice(&self.values[out..out + NUM_COMBOS]);
            }
            return;
        }
        self.grow(walk, depth + 1);
        let mine = n.actor == Some(local);
        let k = n.children.len();
        let buckets = &board.buckets[n.street.index() - 1];
        let offset = storage.offsets[z];
        let next = (depth + 1) * NUM_COMBOS;
        let children = (depth + 1) * walk.width * NUM_COMBOS;
        for (a, &child) in n.children.iter().enumerate() {
            let (slab, from) = if mine {
                (&mut self.own, own)
            } else {
                (&mut self.opponent, opponent)
            };
            for &h in &board.sorted {
                slab[next + h] = slab[from + h] * rows[offset + buckets[h] as usize * k + a];
            }
            let reach = if mine { [opponent, next] } else { [next, own] };
            self.visit(
                walk,
                updates,
                child,
                depth + 1,
                reach,
                children + a * NUM_COMBOS,
            );
        }
        for &h in &board.sorted {
            let row = offset + buckets[h] as usize * k;
            let mut value = 0.0;
            if mine {
                let mut best = f64::NEG_INFINITY;
                for a in 0..k {
                    let child = children + a * NUM_COMBOS + h;
                    value += self.values[child] * rows[row + a];
                    if auxiliary {
                        best = best.max(self.best[child]);
                    }
                }
                self.values[out + h] = value;
                if auxiliary {
                    self.best[out + h] = best;
                }
                if let Some((regrets, sums, weight)) = updates.as_mut() {
                    for a in 0..k {
                        regrets[row + a] += *weight
                            * scale[h]
                            * (self.values[children + a * NUM_COMBOS + h] - value);
                        sums[row + a] += *weight * self.own[own + h] * rows[row + a];
                    }
                }
            } else {
                let mut best = 0.0;
                for a in 0..k {
                    let child = children + a * NUM_COMBOS + h;
                    value += self.values[child];
                    if auxiliary {
                        best += self.best[child];
                    }
                }
                self.values[out + h] = value;
                if auxiliary {
                    self.best[out + h] = best;
                }
            }
        }
    }
}

/// A per-combo array on the heap. Callers hold these across nested parallel
/// loops, where rayon may stack several leaves' frames on one worker thread.
pub(crate) type Combos = Box<[f64; NUM_COMBOS]>;

/// The opponent's and hero's combo weights and the folded seats' mass per
/// combo.
pub(crate) fn inputs(
    tree: &Tree,
    model: &Model<'_>,
    reach: &Reach,
    terminal: usize,
    hero: usize,
) -> (Combos, Combos, Combos) {
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
        Box::new(std::array::from_fn(|h| rho(opponent, h))),
        Box::new(std::array::from_fn(|h| rho(hero, h))),
        Box::new(std::array::from_fn(|h| folded[class(h)])),
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
