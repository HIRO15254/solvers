use super::{Board, BucketSource, Subtree, cards::Q};
use crate::trunk::scratch::Reused;
use crate::trunk::{
    classes::{Classes, class},
    l0::{Model, Terminal, Tree, eval::Reach, solve::Discounts},
};
use anyhow::{Context, Result, ensure};
use nlh::NUM_COMBOS;
use std::{
    cell::RefCell,
    io::{BufReader, BufWriter, Read, Write},
};

thread_local! {
    static SCRATCH: RefCell<Vec<Scratch>> = const { RefCell::new(Vec::new()) };
    static ROWS: RefCell<Vec<Vec<f64>>> = const { RefCell::new(Vec::new()) };
    static INPUTS: RefCell<Vec<Combos>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn scratch() -> Reused<Scratch> {
    Reused::take(&SCRATCH)
}

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

    /// Save only the average accumulators, with a checksum over the header and payload.
    pub fn write_average(&self, tree: &Tree, out: &mut impl Write) -> Result<()> {
        let mut out = BufWriter::new(out);
        let mut hash = blake3::Hasher::new();
        let mut write = |bytes: &[u8]| -> Result<()> {
            out.write_all(bytes)?;
            hash.update(bytes);
            Ok(())
        };
        write(b"P2L1AVG1")?;
        write(&tree.game_fingerprint)?;
        write(&(self.leaves.len() as u64).to_le_bytes())?;
        write(&(self.slots as u64).to_le_bytes())?;
        for leaf in &self.leaves {
            write(&(leaf.terminal as u64).to_le_bytes())?;
            write(&(leaf.sums.len() as u64).to_le_bytes())?;
        }
        for leaf in &self.leaves {
            for value in &leaf.sums {
                write(&value.to_le_bytes())?;
            }
        }
        out.write_all(hash.finalize().as_bytes())?;
        out.flush()?;
        Ok(())
    }

    /// Load into storage built for the same tree and bucket source. Failed
    /// validation leaves all accumulators unchanged; regrets are never modified.
    pub fn read_average(&mut self, tree: &Tree, input: &mut impl Read) -> Result<()> {
        fn read<const N: usize>(
            input: &mut impl Read,
            hash: &mut blake3::Hasher,
        ) -> Result<[u8; N]> {
            let mut bytes = [0; N];
            input
                .read_exact(&mut bytes)
                .context("truncated L1 average or read error")?;
            hash.update(&bytes);
            Ok(bytes)
        }
        let mut input = BufReader::new(input);
        let mut hash = blake3::Hasher::new();
        ensure!(
            &read::<8>(&mut input, &mut hash)? == b"P2L1AVG1",
            "wrong L1 average magic"
        );
        ensure!(
            read::<32>(&mut input, &mut hash)? == tree.game_fingerprint,
            "L1 average game fingerprint mismatch"
        );
        ensure!(
            u64::from_le_bytes(read(&mut input, &mut hash)?) == self.leaves.len() as u64,
            "L1 average leaf count mismatch"
        );
        ensure!(
            u64::from_le_bytes(read(&mut input, &mut hash)?) == self.slots as u64,
            "L1 average slots mismatch"
        );
        for (i, leaf) in self.leaves.iter().enumerate() {
            ensure!(
                u64::from_le_bytes(read(&mut input, &mut hash)?) == leaf.terminal as u64,
                "L1 average leaf {i} terminal mismatch"
            );
            ensure!(
                u64::from_le_bytes(read(&mut input, &mut hash)?) == leaf.sums.len() as u64,
                "L1 average leaf {i} sums length mismatch"
            );
        }
        // Stage only decoded sums, rather than a second copy of the binary file.
        let mut sums = Vec::with_capacity(self.leaves.len());
        for leaf in &self.leaves {
            let mut values = Vec::with_capacity(leaf.sums.len());
            for _ in 0..leaf.sums.len() {
                values.push(f64::from_le_bytes(read(&mut input, &mut hash)?));
            }
            sums.push(values);
        }
        let mut checksum = [0; 32];
        input
            .read_exact(&mut checksum)
            .context("truncated L1 average checksum")?;
        ensure!(
            checksum == *hash.finalize().as_bytes(),
            "L1 average hash mismatch"
        );
        ensure!(
            input.read(&mut [0; 1])? == 0,
            "trailing bytes in L1 average"
        );
        for (leaf, values) in self.leaves.iter_mut().zip(sums) {
            leaf.sums = values;
        }
        Ok(())
    }

    pub fn storage_bytes(&self) -> usize {
        self.slots * 16
    }
}

impl LeafStrategy {
    pub(crate) fn profile(&self, tree: &Subtree, average: bool) -> Reused<Vec<f64>> {
        let mut rows = Reused::take(&ROWS);
        rows.resize(self.regrets.len(), 0.0);
        rows.fill(0.0);
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
/// leaves and jobs via a thread-local pool. A pass walks the tree depth first,
/// so it only touches the slabs along one path: a few hundred KB instead of every node's
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
    /// The opponent's total mass per depth, valid while `fresh`: terminals
    /// that share an opponent slab (a hero's fold and call, say) sum it once.
    totals: Vec<Mass>,
    fresh: Vec<bool>,
}

/// One pass's inputs and its slab layout.
struct Walk<'p, 'a> {
    pass: &'p Pass<'a>,
    local: usize,
    /// The most actions at any node of the tree: the stride of a depth's values.
    width: usize,
}

/// The opponent's reach summed over combos, in total and per card. Plain sums:
/// a compensated sum chains about four dependent operations per combo, and
/// over a board's 1,081 combos the rounding error of a plain sum is far below
/// what the Monte Carlo boards resolve.
#[derive(Clone, Copy)]
struct Mass {
    total: f64,
    cards: [f64; 52],
}

impl Default for Mass {
    fn default() -> Self {
        Self {
            total: 0.0,
            cards: [0.0; 52],
        }
    }
}

impl Mass {
    /// The opponent's reach summed over the board's live combos.
    fn of(board: &Board, catalog: &Classes, opponent: &[f64]) -> Self {
        let mut total = Self::default();
        for &h in &board.sorted {
            total.add(catalog, h, opponent[h]);
        }
        total
    }
    fn add(&mut self, catalog: &Classes, h: usize, value: f64) {
        self.total += value;
        for c in catalog.cards(h) {
            self.cards[c.index()] += value;
        }
    }
    fn disjoint(&self, catalog: &Classes, h: usize) -> f64 {
        let [a, b] = catalog.cards(h);
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
    let catalog = Classes::get();
    let total = Mass::of(board, catalog, opponent);
    settle(
        board, catalog, &total, opponent, payoffs, hero, local, output,
    );
}

/// [`terminal_values`] given the opponent's total mass.
#[allow(clippy::too_many_arguments)]
fn settle(
    board: &Board,
    catalog: &Classes,
    total: &Mass,
    opponent: &[f64],
    payoffs: &[Vec<f64>],
    hero: usize,
    local: usize,
    output: &mut [f64],
) {
    output.fill(0.0);
    if payoffs.len() == 1 {
        for &h in &board.sorted {
            output[h] = (total.disjoint(catalog, h) + opponent[h]) * payoffs[0][hero];
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
            equal.add(catalog, h, opponent[h]);
        }
        for &h in &board.sorted[start..end] {
            let w = lower.disjoint(catalog, h);
            let t = equal.disjoint(catalog, h) + opponent[h];
            let l = total.disjoint(catalog, h) + opponent[h] - w - t;
            output[h] = win * w + tie * t + lose * l;
        }
        lower.total += equal.total;
        for (sum, &value) in lower.cards.iter_mut().zip(&equal.cards) {
            *sum += value;
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
        // Roots are copied, child reaches and values are written before
        // recursion/reduction, and Mass reads only board.sorted. Blocked
        // root values are cleared below; other blocked slab entries are unused.
        self.grow(&walk, 0);
        self.opponent[..NUM_COMBOS].copy_from_slice(pass.opponent);
        self.own[..NUM_COMBOS].copy_from_slice(pass.own);
        self.fresh[0] = false;
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
            self.totals.resize(depth + 1, Mass::default());
            self.fresh.resize(depth + 1, false);
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
            let catalog = Classes::get();
            let slab = &self.opponent[opponent..opponent + NUM_COMBOS];
            let d = opponent / NUM_COMBOS;
            if !self.fresh[d] {
                self.totals[d] = Mass::of(board, catalog, slab);
                self.fresh[d] = true;
            }
            settle(
                board,
                catalog,
                &self.totals[d],
                slab,
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
            if !mine {
                self.fresh[depth + 1] = false;
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
) -> (Reused<Combos>, Reused<Combos>, Reused<Combos>) {
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
    let mut opponent_input = Reused::take_with(&INPUTS, || Box::new([0.0; NUM_COMBOS]));
    let mut own = Reused::take_with(&INPUTS, || Box::new([0.0; NUM_COMBOS]));
    let mut scale = Reused::take_with(&INPUTS, || Box::new([0.0; NUM_COMBOS]));
    for h in 0..NUM_COMBOS {
        opponent_input[h] = rho(opponent, h);
    }
    for h in 0..NUM_COMBOS {
        own[h] = rho(hero, h);
    }
    for h in 0..NUM_COMBOS {
        scale[h] = folded[class(h)];
    }
    (opponent_input, own, scale)
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
