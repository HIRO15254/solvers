use super::{
    Board, BucketSource, Subtree,
    cards::{Q, Strength},
};
use crate::trunk::scratch::Reused;
use crate::trunk::{
    classes::{Classes, class},
    l0::{Model, Terminal, Tree, eval::Reach, solve::Discounts},
};
use anyhow::{Context, Result, ensure};
use nlh::NUM_COMBOS;
use rayon::prelude::*;
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
    /// The leaf's card mapping (`BucketSource::leaf_variant`; 0 = the source's).
    pub variant: usize,
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
        Self::new_for(tree, source, None)
    }

    /// `new` with accumulators only at the leaves in `keep` (trunk terminal
    /// ids; `None` = every leaf). The other leaves keep their layout but hold
    /// no accumulators: they cannot be trained or played, `slots` counts only
    /// the kept leaves, and a saved average records them with no sums.
    pub fn new_for(tree: &Tree, source: &dyn BucketSource, keep: Option<&[usize]>) -> Self {
        let mut nodes = 0;
        let mut slots = 0;
        let leaves = tree
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(terminal, n)| {
                let t = n.terminal.as_ref()?.l1.as_ref()?;
                let variant = source.leaf_variant(terminal);
                let source: &dyn BucketSource = if variant > 0 {
                    &source.variants()[variant - 1]
                } else {
                    source.low_spr().map_or(source, |(max_spr, finer)| {
                        if t.flop_spr <= max_spr { finer } else { source }
                    })
                };
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
                if !keep.is_none_or(|k| k.contains(&terminal)) {
                    size = 0;
                }
                slots += size;
                Some(LeafStrategy {
                    terminal,
                    variant,
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

    /// The leaf count, postflop decisions and slots of `new`'s full layout,
    /// without allocating it.
    pub fn layout(tree: &Tree, source: &dyn BucketSource) -> (usize, usize, usize) {
        let mut leaves = 0;
        let mut nodes = 0;
        let mut slots = 0;
        for (terminal, n) in tree.nodes.iter().enumerate() {
            let Some(t) = n.terminal.as_ref().and_then(|t| t.l1.as_ref()) else {
                continue;
            };
            let variant = source.leaf_variant(terminal);
            let source: &dyn BucketSource =
                if variant > 0 {
                    &source.variants()[variant - 1]
                } else {
                    source.low_spr().map_or(source, |(max_spr, finer)| {
                        if t.flop_spr <= max_spr { finer } else { source }
                    })
                };
            leaves += 1;
            for n in t.nodes.iter().filter(|n| n.actor.is_some()) {
                nodes += 1;
                slots += source.count(n.street) * n.children.len();
            }
        }
        (leaves, nodes, slots)
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
        self.read_average_of(tree, None, input)
    }

    /// `read_average` of only leaf `terminal`, into storage built by `new_for`
    /// with that leaf kept. The saved average's other leaves may hold sums or
    /// none (an average of `new_for` storage); they are checked and skipped.
    pub fn read_average_leaf(
        &mut self,
        tree: &Tree,
        terminal: usize,
        input: &mut impl Read,
    ) -> Result<()> {
        ensure!(
            self.leaves.iter().any(|l| l.terminal == terminal),
            "node {terminal} is not an L1 leaf"
        );
        self.read_average_of(tree, Some(terminal), input)
    }

    fn read_average_of(
        &mut self,
        tree: &Tree,
        only: Option<usize>,
        input: &mut impl Read,
    ) -> Result<()> {
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
        let slots = u64::from_le_bytes(read(&mut input, &mut hash)?);
        ensure!(
            only.is_some() || slots == self.slots as u64,
            "L1 average slots mismatch"
        );
        let mut lengths = Vec::with_capacity(self.leaves.len());
        for (i, leaf) in self.leaves.iter().enumerate() {
            ensure!(
                u64::from_le_bytes(read(&mut input, &mut hash)?) == leaf.terminal as u64,
                "L1 average leaf {i} terminal mismatch"
            );
            let length = u64::from_le_bytes(read(&mut input, &mut hash)?);
            ensure!(
                only.is_some_and(|z| z != leaf.terminal) || length == leaf.sums.len() as u64,
                "L1 average leaf {i} sums length mismatch"
            );
            lengths.push(length);
        }
        ensure!(
            lengths.iter().sum::<u64>() == slots,
            "L1 average slots mismatch"
        );
        // Stage only decoded sums, rather than a second copy of the binary file.
        let mut sums = Vec::with_capacity(self.leaves.len());
        for (leaf, &length) in self.leaves.iter().zip(&lengths) {
            let keep = only.is_none_or(|z| z == leaf.terminal);
            let mut values = Vec::with_capacity(if keep { leaf.sums.len() } else { 0 });
            for _ in 0..length {
                let value = f64::from_le_bytes(read(&mut input, &mut hash)?);
                if keep {
                    values.push(value);
                }
            }
            sums.push(keep.then_some(values));
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
            if let Some(values) = values {
                leaf.sums = values;
            }
        }
        Ok(())
    }

    /// Copy a saved average whose only difference from one of `tree` is the
    /// game fingerprint (a tree written under an older fingerprint scheme),
    /// stamping `tree`'s fingerprint and a new checksum. Everything else is
    /// validated as in `read_average`; an error may leave `out` partial.
    /// Returns the replaced fingerprint. The caller must establish that the
    /// trees are equal: the leaf and slot layout is the only check here.
    pub fn rebind_average(
        &self,
        tree: &Tree,
        input: &mut impl Read,
        out: &mut impl Write,
    ) -> Result<[u8; 32]> {
        let mut input = BufReader::new(input);
        let mut out = BufWriter::new(out);
        let mut old_hash = blake3::Hasher::new();
        let mut new_hash = blake3::Hasher::new();
        let mut copy = |bytes: &[u8], written: &[u8]| -> Result<()> {
            old_hash.update(bytes);
            new_hash.update(written);
            out.write_all(written)?;
            Ok(())
        };
        let mut header = vec![0; 56 + 16 * self.leaves.len()];
        input
            .read_exact(&mut header)
            .context("truncated L1 average or read error")?;
        ensure!(&header[..8] == b"P2L1AVG1", "wrong L1 average magic");
        let old: [u8; 32] = header[8..40].try_into().unwrap();
        let word = |at: usize| u64::from_le_bytes(header[at..at + 8].try_into().unwrap());
        ensure!(
            word(40) == self.leaves.len() as u64,
            "L1 average leaf count mismatch"
        );
        ensure!(word(48) == self.slots as u64, "L1 average slots mismatch");
        for (i, leaf) in self.leaves.iter().enumerate() {
            ensure!(
                word(56 + 16 * i) == leaf.terminal as u64,
                "L1 average leaf {i} terminal mismatch"
            );
            ensure!(
                word(64 + 16 * i) == leaf.sums.len() as u64,
                "L1 average leaf {i} sums length mismatch"
            );
        }
        let mut stamped = header.clone();
        stamped[8..40].copy_from_slice(&tree.game_fingerprint);
        copy(&header, &stamped)?;
        let mut left = self.slots * 8;
        let mut chunk = vec![0; left.min(1 << 20)];
        while left > 0 {
            let n = left.min(chunk.len());
            input
                .read_exact(&mut chunk[..n])
                .context("truncated L1 average or read error")?;
            copy(&chunk[..n], &chunk[..n])?;
            left -= n;
        }
        let mut checksum = [0; 32];
        input
            .read_exact(&mut checksum)
            .context("truncated L1 average checksum")?;
        ensure!(
            checksum == *old_hash.finalize().as_bytes(),
            "L1 average hash mismatch"
        );
        ensure!(
            input.read(&mut [0; 1])? == 0,
            "trailing bytes in L1 average"
        );
        out.write_all(new_hash.finalize().as_bytes())?;
        out.flush()?;
        Ok(old)
    }

    pub fn storage_bytes(&self) -> usize {
        self.slots * 16
    }
}

impl LeafStrategy {
    pub(crate) fn profile(&self, tree: &Subtree, average: bool) -> Reused<Vec<f64>> {
        let mut rows = Reused::take_bounded(&ROWS, LEAF_POOL, Vec::new);
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

/// Values a worker's pool keeps of the buffers sized like a leaf's strategy.
pub(crate) const LEAF_POOL: usize = 4;

/// Per-depth slabs, grown to the deepest leaf tree and reused across boards and
/// leaves and jobs via a thread-local pool. A pass walks the tree depth first,
/// so it only touches the slabs along one path: a few hundred KB instead of every node's
/// combo arrays, which kept the pass waiting on memory.
#[derive(Default)]
pub(crate) struct Scratch {
    /// The opponent's and hero's reach per depth, `[depth][combo]`.
    opponent: Vec<f64>,
    own: Vec<f64>,
    /// Optional mapping reaches, with the same depth offsets and sharing.
    map_opponent: Vec<f64>,
    map_own: Vec<f64>,
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
    /// Relative buckets per depth, `[depth][combo]`, for the node being visited.
    mapped: Vec<u16>,
    /// The hero's own reach and live combo count per bucket, `[bucket][2]`,
    /// at the node whose updates are being written.
    bucket_reach: Vec<f64>,
}

/// Buckets each live combo by the mid-point of its equal-strength group in the
/// actor's reach-weighted strength distribution (with no reach, by position),
/// crossed with the combo's class.
#[cfg(test)]
pub(crate) fn relative_buckets(strength: &Strength, reach: &[f64], out: &mut [u16]) {
    relative_leaf_buckets(strength, f64::INFINITY, 0, reach, None, out);
}

/// `relative_buckets` at a leaf with this flop SPR and card variant
/// (`LeafStrategy::variant`). Where the street has opponent levels
/// (`Strength::opponent`), `opponent` is the reach of the actor's opponent
/// at the node, and each combo's level is its showdown equity against it.
pub(crate) fn relative_leaf_buckets(
    strength: &Strength,
    spr: f64,
    variant: usize,
    reach: &[f64],
    opponent: Option<&[f64]>,
    out: &mut [u16],
) {
    percentile_buckets(strength, spr, variant, reach, out);
    if let Some(levels) = &strength.opponent {
        let opponent = opponent.expect("opponent levels need the opponent's reach at the node");
        equity_levels(strength, levels, variant, opponent, out);
    }
}

/// Replaces each live combo's strength level by equal ranges of its showdown
/// equity on the board so far (ties half) against `opponent`, among the
/// opponent combos sharing no card with it, with the variant's number of
/// levels; a combo no opponent combo can meet keeps its strength-table level
/// on that scale.
fn equity_levels(
    strength: &Strength,
    levels: &super::cards::OpponentLevels,
    variant: usize,
    opponent: &[f64],
    out: &mut [u16],
) {
    let catalog = Classes::get();
    let base = levels.levels;
    let n = if variant > 0 {
        levels.variants[variant - 1]
    } else {
        base
    };
    let (order, groups) = levels
        .showdown
        .as_ref()
        .map_or((&strength.order, &strength.groups), |(order, groups)| {
            (order, groups)
        });
    let mut all = Mass::default();
    for &h in order {
        all.add(catalog, h, opponent[h]);
    }
    let mut below = Mass::default();
    for &(start, end) in groups {
        let mut tie = Mass::default();
        for &h in &order[start..end] {
            tie.add(catalog, h, opponent[h]);
        }
        for &h in &order[start..end] {
            // `disjoint` removes the combo's own weight twice; add it back
            // where the sum holds it.
            let met = all.disjoint(catalog, h) + opponent[h];
            let old = usize::from(strength.class[h]) / levels.stride % base;
            let level = if met > 0.0 {
                let tied = tie.disjoint(catalog, h) + opponent[h];
                let equity = (below.disjoint(catalog, h) + 0.5 * tied) / met;
                ((equity * n as f64) as usize).min(n - 1)
            } else {
                old * n / base
            };
            // A variant's bucket holds the base level until here, so the
            // intermediate may leave the u16 range; the result does not.
            out[h] = out[h]
                .wrapping_add((level * levels.stride) as u16)
                .wrapping_sub((old * levels.stride) as u16);
        }
        below.absorb(&tie);
    }
}

fn percentile_buckets(
    strength: &Strength,
    spr: f64,
    variant: usize,
    reach: &[f64],
    out: &mut [u16],
) {
    let (n, classes, class, shift) = if variant > 0 {
        let (count, classes, shift) = strength.variants[variant - 1];
        (count, classes, &strength.class, shift)
    } else {
        strength
            .low_spr
            .as_ref()
            .filter(|f| spr <= f.max_spr)
            .map_or(
                (strength.count, strength.classes, &strength.class, 0),
                |f| (f.count, f.classes, &f.class, 0),
            )
    };
    let total: f64 = strength.order.iter().map(|&h| reach[h]).sum();
    let count = n as f64;
    let live = strength.order.len() as f64;
    let mass = strength.prior + strength.prior_fraction * total;
    if mass > 0.0 && total > 0.0 {
        let prior = mass / live;
        let total = total + mass;
        let mut below = 0.0;
        for &(start, end) in &strength.groups {
            let group: f64 = strength.order[start..end]
                .iter()
                .map(|&h| reach[h] + prior)
                .sum();
            let x = (below + 0.5 * group) / total;
            let bucket = ((x * count) as usize).min(n - 1) * classes;
            for &h in &strength.order[start..end] {
                out[h] = (bucket + usize::from(class[h])).wrapping_add_signed(shift) as u16;
            }
            below += group;
        }
        return;
    }
    // Keep the original arithmetic at prior 0. With zero reach, the uniform
    // prior has exactly these positional mid-points; avoid rounding their sums.
    let mut below = 0.0;
    for &(start, end) in &strength.groups {
        let group: f64 = strength.order[start..end].iter().map(|&h| reach[h]).sum();
        let x = if total > 0.0 {
            (below + 0.5 * group) / total
        } else {
            (start + end) as f64 * 0.5 / live
        };
        let bucket = ((x * count) as usize).min(n - 1) * classes;
        for &h in &strength.order[start..end] {
            out[h] = (bucket + usize::from(class[h])).wrapping_add_signed(shift) as u16;
        }
        below += group;
    }
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
    fn absorb(&mut self, other: &Mass) {
        self.total += other.total;
        for (a, b) in self.cards.iter_mut().zip(&other.cards) {
            *a += b;
        }
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
    pub mapping: Option<Mapping<'a>>,
    /// Weight each combo's regret by its own reach relative to the mean own
    /// reach of the combos sharing its bucket, instead of equally. With perfect
    /// recall every combo in a bucket has the same own reach and this is plain
    /// counterfactual regret; abstract buckets also hold combos the hero's
    /// range does not contain, which otherwise shape the shared strategy.
    pub own_reach_regret: bool,
}

pub(crate) struct Mapping<'a> {
    pub opponent: &'a [f64; NUM_COMBOS],
    pub own: &'a [f64; NUM_COMBOS],
    pub rows: &'a [f64],
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

    pub(crate) fn pass(&mut self, pass: &Pass<'_>, updates: Option<(&mut [f64], &mut [f64], f64)>) {
        // Specialize once at the root, keeping mapping branches and slab work
        // out of the default traversal's hot loops.
        if pass.mapping.is_some() {
            self.run::<true>(pass, updates);
        } else {
            self.run::<false>(pass, updates);
        }
    }

    fn run<const MAPPING: bool>(
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
        self.grow::<MAPPING>(&walk, 0);
        self.opponent[..NUM_COMBOS].copy_from_slice(pass.opponent);
        self.own[..NUM_COMBOS].copy_from_slice(pass.own);
        if MAPPING {
            let mapping = pass.mapping.as_ref().unwrap();
            self.map_opponent[..NUM_COMBOS].copy_from_slice(mapping.opponent);
            self.map_own[..NUM_COMBOS].copy_from_slice(mapping.own);
        }
        self.fresh[0] = false;
        self.visit::<MAPPING>(&walk, &mut updates, 0, 0, [0; 2], 0);
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
    fn grow<const MAPPING: bool>(&mut self, walk: &Walk<'_, '_>, depth: usize) {
        let reach = (depth + 1) * NUM_COMBOS;
        if self.opponent.len() < reach {
            self.opponent.resize(reach, 0.0);
            self.own.resize(reach, 0.0);
            self.totals.resize(depth + 1, Mass::default());
            self.fresh.resize(depth + 1, false);
        }
        if walk.pass.board.relative.is_some() && self.mapped.len() < reach {
            self.mapped.resize(reach, 0);
        }
        if MAPPING && self.map_opponent.len() < reach {
            self.map_opponent.resize(reach, 0.0);
            self.map_own.resize(reach, 0.0);
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
    /// at the offsets `[opponent, own]`. An action leaves the other player's
    /// reach unchanged, so the child shares that slab instead of multiplying
    /// it by 1; a slab at `depth` is only rewritten after the subtrees that
    /// read it. The arithmetic and the order of every sum match a pass over
    /// all nodes at once, so results are bit-identical to it.
    fn visit<const MAPPING: bool>(
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
            ref mapping,
            own_reach_regret,
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
        self.grow::<MAPPING>(walk, depth + 1);
        let mine = n.actor == Some(local);
        let k = n.children.len();
        let street = n.street.index() - 1;
        let at = depth * NUM_COMBOS;
        let relative = if let Some(strength) = &board.relative {
            let (own_slab, opponent_slab) = if MAPPING {
                (&self.map_own, &self.map_opponent)
            } else {
                (&self.own, &self.opponent)
            };
            // The actor's and the opponent's reach at the node.
            let (reach, other) = if mine {
                (
                    &own_slab[own..own + NUM_COMBOS],
                    &opponent_slab[opponent..opponent + NUM_COMBOS],
                )
            } else {
                (
                    &opponent_slab[opponent..opponent + NUM_COMBOS],
                    &own_slab[own..own + NUM_COMBOS],
                )
            };
            relative_leaf_buckets(
                &strength[street],
                tree.flop_spr,
                storage.variant,
                reach,
                Some(other),
                &mut self.mapped[at..at + NUM_COMBOS],
            );
            true
        } else {
            false
        };
        let buckets = &board.buckets[street];
        let offset = storage.offsets[z];
        let next = (depth + 1) * NUM_COMBOS;
        let children = (depth + 1) * walk.width * NUM_COMBOS;
        for (a, &child) in n.children.iter().enumerate() {
            let (slab, from) = if mine {
                (&mut self.own, own)
            } else {
                (&mut self.opponent, opponent)
            };
            let map = if relative {
                &self.mapped[at..at + NUM_COMBOS]
            } else {
                &buckets[..]
            };
            for &h in &board.sorted {
                slab[next + h] = slab[from + h] * rows[offset + map[h] as usize * k + a];
            }
            if MAPPING {
                let mapping = mapping.as_ref().unwrap();
                let (slab, from) = if mine {
                    (&mut self.map_own, own)
                } else {
                    (&mut self.map_opponent, opponent)
                };
                for &h in &board.sorted {
                    slab[next + h] =
                        slab[from + h] * mapping.rows[offset + map[h] as usize * k + a];
                }
            }
            if !mine {
                self.fresh[depth + 1] = false;
            }
            let reach = if mine { [opponent, next] } else { [next, own] };
            self.visit::<MAPPING>(
                walk,
                updates,
                child,
                depth + 1,
                reach,
                children + a * NUM_COMBOS,
            );
        }
        let map = if relative {
            &self.mapped[at..at + NUM_COMBOS]
        } else {
            &buckets[..]
        };
        let weighted = mine && own_reach_regret && updates.is_some();
        if weighted {
            let end = storage
                .offsets
                .get(z + 1)
                .copied()
                .unwrap_or(storage.regrets.len());
            self.bucket_reach.clear();
            self.bucket_reach.resize(2 * ((end - offset) / k), 0.0);
            for &h in &board.sorted {
                let b = 2 * map[h] as usize;
                self.bucket_reach[b] += self.own[own + h];
                self.bucket_reach[b + 1] += 1.0;
            }
        }
        for &h in &board.sorted {
            let row = offset + map[h] as usize * k;
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
                    // The combo's own reach over its bucket's mean; a bucket
                    // nobody reaches keeps plain counterfactual regret. At 1
                    // the product is exact, so unweighted updates are unchanged.
                    let share = if weighted {
                        let b = 2 * map[h] as usize;
                        let (total, count) = (self.bucket_reach[b], self.bucket_reach[b + 1]);
                        if total > 0.0 {
                            self.own[own + h] * count / total
                        } else {
                            1.0
                        }
                    } else {
                        1.0
                    };
                    for a in 0..k {
                        regrets[row + a] += *weight
                            * share
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

/// The hero's best response to the saved opponent strategy on one flop's
/// `boards`, per combo and summed over the boards, unscaled like the pass's
/// values. Unlike the pass's auxiliary `best`, which picks every action
/// knowing the whole runout, a flop decision sees only the flop and a turn
/// decision only the turn: the action maximises the value summed over the
/// boards that share those cards. River decisions and terminals are per board.
pub(crate) fn informed_best_response(
    tree: &Subtree,
    storage: &LeafStrategy,
    rows: &[f64],
    boards: &[Board],
    hero: usize,
    opponent: &[f64; NUM_COMBOS],
) -> Vec<f64> {
    let walk = Informed {
        tree,
        storage,
        rows,
        boards,
        hero,
        local: usize::from(tree.active[0] != hero),
    };
    let group: Vec<usize> = (0..boards.len()).collect();
    let reach: Vec<Vec<f64>> = group.iter().map(|_| opponent.to_vec()).collect();
    walk.value(0, &group, &reach)
}

struct Informed<'a> {
    tree: &'a Subtree,
    storage: &'a LeafStrategy,
    rows: &'a [f64],
    boards: &'a [Board],
    hero: usize,
    local: usize,
}

impl Informed<'_> {
    /// The cards a decision at `z` sees: 3 on the flop, 4 on the turn and the
    /// whole board on the river and at terminals.
    fn seen(&self, z: usize) -> usize {
        let n = &self.tree.nodes[z];
        if n.actor.is_none() {
            5
        } else {
            2 + n.street.index()
        }
    }

    /// Node `z`'s value summed over `group`, boards that agree on the cards
    /// `z` sees, with the opponent's reach per board in `reach`.
    fn value(&self, z: usize, group: &[usize], reach: &[Vec<f64>]) -> Vec<f64> {
        let catalog = Classes::get();
        let n = &self.tree.nodes[z];
        let mut out = vec![0.0; NUM_COMBOS];
        if n.actor.is_none() {
            let mut values = vec![0.0; NUM_COMBOS];
            for (&b, slab) in group.iter().zip(reach) {
                let board = &self.boards[b];
                let total = Mass::of(board, catalog, slab);
                settle(
                    board,
                    catalog,
                    &total,
                    slab,
                    &n.payoffs,
                    self.hero,
                    self.local,
                    &mut values,
                );
                for &h in &board.sorted {
                    out[h] += values[h];
                }
            }
            return out;
        }
        let mine = n.actor == Some(self.local);
        let k = n.children.len();
        let street = n.street.index() - 1;
        let offset = self.storage.offsets[z];
        let maps: Vec<Vec<u16>> = if mine {
            Vec::new()
        } else {
            group
                .iter()
                .zip(reach)
                .map(|(&b, slab)| {
                    let board = &self.boards[b];
                    match &board.relative {
                        Some(strength) => {
                            let mut map = vec![0; NUM_COMBOS];
                            // `leaf_informed_best_response` rejects opponent
                            // levels: the hero's reach is not tracked here.
                            relative_leaf_buckets(
                                &strength[street],
                                self.tree.flop_spr,
                                self.storage.variant,
                                slab,
                                None,
                                &mut map,
                            );
                            map
                        }
                        None => board.buckets[street].to_vec(),
                    }
                })
                .collect()
        };
        let children: Vec<Vec<f64>> = n
            .children
            .par_iter()
            .enumerate()
            .map(|(a, &child)| {
                let child_reach: Vec<Vec<f64>> = if mine {
                    reach.to_vec()
                } else {
                    group
                        .iter()
                        .zip(reach)
                        .zip(&maps)
                        .map(|((&b, slab), map)| {
                            let mut next = vec![0.0; NUM_COMBOS];
                            for &h in &self.boards[b].sorted {
                                next[h] = slab[h] * self.rows[offset + map[h] as usize * k + a];
                            }
                            next
                        })
                        .collect()
                };
                let seen = self.seen(child);
                let mut parts: Vec<(Vec<usize>, Vec<Vec<f64>>)> = Vec::new();
                for (i, &b) in group.iter().enumerate() {
                    let cards = &self.boards[b].cards[..seen];
                    let part = parts
                        .iter()
                        .position(|(p, _)| self.boards[p[0]].cards[..seen] == *cards);
                    let part = part.unwrap_or_else(|| {
                        parts.push((Vec::new(), Vec::new()));
                        parts.len() - 1
                    });
                    parts[part].0.push(b);
                    parts[part].1.push(child_reach[i].clone());
                }
                parts
                    .par_iter()
                    .map(|(boards, slabs)| self.value(child, boards, slabs))
                    .reduce(
                        || vec![0.0; NUM_COMBOS],
                        |mut a, b| {
                            for (a, b) in a.iter_mut().zip(&b) {
                                *a += b;
                            }
                            a
                        },
                    )
            })
            .collect();
        for h in 0..NUM_COMBOS {
            out[h] = if mine {
                children
                    .iter()
                    .map(|c| c[h])
                    .fold(f64::NEG_INFINITY, f64::max)
            } else {
                children.iter().map(|c| c[h]).sum()
            };
        }
        out
    }
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
