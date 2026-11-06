use crate::card_abstraction::{CardAbstraction, Ehs2Abstraction};
use crate::trunk::classes::Classes;
use nlh::{Card, NUM_COMBOS, Street, rank_of};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use serde::Serialize;
use std::sync::OnceLock;

/// How a set of sampled boards covers the deck.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sampling {
    /// Independent uniform boards.
    #[default]
    Random,
    /// Equally spaced points on the deal-ordered line of all boards, shifted
    /// by a golden-ratio sequence across iterations. Each point is uniform, so
    /// estimates stay unbiased; flops, turns and rivers are spread evenly.
    Stratified,
}

/// Experimental training game: one real flop per suit-isomorphism class,
/// sampled in proportion to its positive weight.
#[derive(Clone, Debug)]
pub struct WeightedFlopSubset {
    flops: Vec<[Card; 3]>,
    ends: Vec<f64>,
    identity: [u8; 32],
}

impl WeightedFlopSubset {
    /// Parses nonempty `card card card weight` lines. Card order and weight
    /// spelling are normalized; row order is retained for stratified sampling.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let mut flops = Vec::new();
        let mut ends = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut total = 0.0;
        let mut hash = blake3::Hasher::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let parts: Vec<_> = line.split_whitespace().collect();
            anyhow::ensure!(
                parts.len() == 4,
                "flop subset line {line} is not `card card card weight`"
            );
            let mut flop = parse_flop(&parts[..3], line)?;
            flop.sort_unstable_by_key(|c| c.index());
            anyhow::ensure!(
                seen.insert(super::canonical_flop(flop)),
                "flop {line} repeats an isomorphic flop"
            );
            let weight: f64 = parts[3].parse()?;
            anyhow::ensure!(
                weight.is_finite() && weight > 0.0,
                "flop {line} weight must be finite and positive"
            );
            let end = total + weight;
            anyhow::ensure!(
                end.is_finite() && end > total,
                "flop {line} weight overflows or is too small for the cumulative total"
            );
            total = end;
            hash.update(&flop.map(|c| c.index() as u8));
            hash.update(&weight.to_le_bytes());
            flops.push(flop);
            ends.push(end);
        }
        anyhow::ensure!(!flops.is_empty(), "no subset flops");
        Ok(Self {
            flops,
            ends,
            identity: *hash.finalize().as_bytes(),
        })
    }

    /// Requires exactly one representative of every strategy group.
    pub fn validate_groups(&self, groups: &super::FlopClusters) -> anyhow::Result<()> {
        let mut seen = vec![false; groups.count()];
        for flop in &self.flops {
            let group = groups.group(flop);
            anyhow::ensure!(
                !seen[group],
                "flop subset contains multiple representatives of group {group}"
            );
            seen[group] = true;
        }
        for (group, present) in seen.into_iter().enumerate() {
            anyhow::ensure!(
                present,
                "flop subset has no representative of group {group}"
            );
        }
        Ok(())
    }

    pub(super) fn state_identity(&self) -> [u8; 32] {
        self.identity
    }

    pub(super) fn board_at(&self, x: f64) -> [Card; 5] {
        let i = self
            .ends
            .partition_point(|&end| end <= x)
            .min(self.ends.len() - 1);
        let start = if i == 0 { 0.0 } else { self.ends[i - 1] };
        let fraction = ((x - start) / (self.ends[i] - start)).clamp(0.0, 1.0 - f64::EPSILON);
        board_runout(self.flops[i], fraction)
    }
}

/// Fixed-profile training boards. Unweighted inputs retain the historical
/// round-robin flop order and seeded random runouts.
#[derive(Clone, Debug)]
pub enum FixedFlops {
    RoundRobin(Vec<[Card; 3]>),
    Weighted(WeightedFlopSubset),
}

impl FixedFlops {
    /// Parses either three cards per line or three cards and a positive weight
    /// per line. Mixing the two formats is rejected.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let mut flops = Vec::new();
        let mut weighted = None;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let parts: Vec<_> = line.split_whitespace().collect();
            anyhow::ensure!(
                parts.len() == 3 || parts.len() == 4,
                "fixed flop line {line} is not `card card card [weight]`"
            );
            let has_weight = parts.len() == 4;
            anyhow::ensure!(
                weighted.is_none_or(|w| w == has_weight),
                "fixed flops mix weighted and unweighted lines"
            );
            weighted = Some(has_weight);
            if !has_weight {
                flops.push(parse_flop(&parts, line)?);
            }
        }
        anyhow::ensure!(weighted.is_some(), "no fixed flops");
        if weighted == Some(true) {
            Ok(Self::Weighted(WeightedFlopSubset::parse(text)?))
        } else {
            Ok(Self::RoundRobin(flops))
        }
    }
}

fn parse_flop(parts: &[&str], line: &str) -> anyhow::Result<[Card; 3]> {
    let mut flop = [Card::from_index(0); 3];
    for (card, text) in flop.iter_mut().zip(parts) {
        *card = text
            .parse()
            .map_err(|e| anyhow::anyhow!("card {text}: {e:?}"))?;
    }
    anyhow::ensure!(
        flop[0] != flop[1] && flop[0] != flop[2] && flop[1] != flop[2],
        "flop {line} repeats a card"
    );
    Ok(flop)
}

pub trait BucketSource: Sync {
    /// Stable identity of the mapping, not just its dimensions. Stateful
    /// custom sources must override this with their data/parameter identity.
    fn state_identity(&self) -> [u8; 32] {
        *blake3::hash(std::any::type_name::<Self>().as_bytes()).as_bytes()
    }

    /// Experimental finer relative mapping, used only at leaves at or below
    /// this flop effective-stack-to-pot ratio.
    fn low_spr(&self) -> Option<(f64, &dyn BucketSource)> {
        None
    }

    /// Experimental per-leaf mappings ([`super::LeafCards`]): the variant of
    /// the leaf at tree node `terminal`, 0 for this mapping and `v` for
    /// `variants()[v - 1]`.
    fn leaf_variant(&self, _terminal: usize) -> usize {
        0
    }

    /// The relative mappings of variants 1.. (`leaf_variant`). Each differs
    /// from this one only in the board-wide part of a class (the flop group
    /// and the runout class), the number of percentile buckets and, with
    /// opponent levels, the number of strength levels.
    fn variants(&self) -> &[super::Relative<'_>] {
        &[]
    }

    fn count(&self, street: Street) -> usize;
    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS];
    /// Relative buckets: `row` is then a strength table, and a pass buckets
    /// each combo by its percentile in the actor's reach at the node (ordered
    /// by showdown rank on the river) crossed with its `class`.
    fn relative(&self) -> bool {
        false
    }
    /// Relative buckets only: uniform prior mass in combos.
    fn relative_prior(&self) -> f64 {
        0.0
    }
    /// Relative buckets only: extra prior mass as a fraction of the reach total.
    fn relative_prior_fraction(&self) -> f64 {
        0.0
    }
    /// Relative buckets only: train with the average strategies' mapping reaches.
    fn relative_average(&self) -> bool {
        false
    }
    /// Relative buckets only: the strength levels in the least significant
    /// part of `class` (before the hand and recall splits).
    fn levels(&self) -> usize {
        1
    }
    /// Relative buckets only (experimental): on this street a pass replaces
    /// each combo's `levels` part with equal ranges of its showdown equity on
    /// the board so far (ties half) against the opponent's reach at the node.
    fn opponent_levels(&self, _street: Street) -> bool {
        false
    }
    /// Relative buckets only: classes per percentile bucket.
    fn classes(&self, _street: Street) -> usize {
        1
    }
    /// Relative buckets only: a live combo's class from the board and its
    /// strength bucket, among `classes / hand_classes`.
    fn class(&self, _board: &[Card], _strength: u16) -> u16 {
        0
    }
    /// Relative buckets only: splits of each `class` by the combo's own hand
    /// on the board so far (`hand_class`); 1 = none.
    fn hand_classes(&self, _street: Street) -> usize {
        1
    }
    /// Relative buckets only: a live combo's split from its `HandFeatures`.
    fn hand_class(&self, _street: Street, _features: &HandFeatures) -> u16 {
        0
    }
    /// Relative buckets only: whether `hand_class` uses the potential, the
    /// percentile among the combos live on the board so far (ties count half)
    /// of the variance of the combo's `row` bucket over the next card.
    fn hand_potential(&self, _street: Street) -> bool {
        false
    }
    /// Relative buckets only: whether `hand_class` uses the category and the
    /// percentile within it.
    fn hand_category(&self) -> bool {
        false
    }
    /// Persistent flop information appended to turn and river classes.
    fn flop_recall_classes(&self, _street: Street) -> usize {
        1
    }
    fn flop_recall_class(&self, _flop: &[Card], _hole: [Card; 2], _strength: u16) -> u16 {
        0
    }
}

/// What `BucketSource::hand_class` may split a live combo by.
pub struct HandFeatures {
    /// The made-hand percentile among the combos live on the board so far
    /// (ties count half).
    pub made: f64,
    /// `draws`.
    pub draws: u8,
    /// `weak_draws`: bit 0 a backdoor flush draw, bit 1 a gutshot.
    pub weak_draws: u8,
    /// The potential percentile (0 unless `hand_potential`).
    pub potential: f64,
    /// `category` and the percentile of the `row` bucket (on the river, the
    /// showdown rank) among the live combos of that category (0 unless
    /// `hand_category`).
    pub category: usize,
    pub within: f64,
    /// The two hole cards have the same rank.
    pub pocket_pair: bool,
}

/// Made-hand categories of `category`.
pub const MADE_CATEGORIES: usize = 10;
/// Draw categories of `category` on the flop and turn.
pub const DRAW_CATEGORIES: usize = 6;

/// Rank masks with the ace also below the deuce: bit r + 1 for rank r.
fn rank_mask<'a>(cards: impl IntoIterator<Item = &'a Card>) -> u16 {
    cards.into_iter().fold(0_u16, |m, c| {
        let r = c.rank();
        m | (1 << (r + 1)) | if r == 12 { 1 } else { 0 }
    })
}

fn straight(mask: u16) -> bool {
    (0..10).any(|low| (mask >> low) & 0x1f == 0x1f)
}

/// A combo's flush draw (four cards of a suit, at least one of them a hole
/// card, and no flush) and its straight outs (the ranks that complete a
/// straight that the board and that rank alone do not make; 0 with a
/// straight).
fn draw_parts(board: &[Card], hole: [Card; 2]) -> (bool, usize) {
    let mut suits = [0_u8; 4];
    for c in board.iter().chain(&hole) {
        suits[usize::from(c.suit())] += 1;
    }
    let flush = suits.iter().any(|&n| n >= 5);
    let flush_draw = !flush && hole.iter().any(|c| suits[usize::from(c.suit())] == 4);
    let all = rank_mask(board.iter().chain(&hole));
    let shared = rank_mask(board);
    if straight(all) {
        return (flush_draw, 0);
    }
    let outs = (0..13_u8)
        .filter(|&r| {
            let add = (1_u16 << (r + 1)) | if r == 12 { 1 } else { 0 };
            straight(all | add) && !straight(shared | add)
        })
        .count();
    (flush_draw, outs)
}

/// A combo's category on a flop, turn or river: its made hand (below) times
/// `DRAW_CATEGORIES` plus its draw on the flop and turn, its made hand alone
/// on the river.
///
/// Made hands, counting only what the hole cards add to the board: 0 nothing,
/// 1 weak pair (a pair below the second board rank, or a pocket pair below
/// it), 2 middle pair (pairing the second board rank, or a pocket pair between
/// the top two), 3 top pair, 4 overpair, 5 two pair (both hole cards pair the
/// board), 6 set or trips, 7 straight, 8 flush, 9 full house or better.
///
/// Draws: 0 none, 1 backdoor flush draw (flop only: three cards of a suit, at
/// least one a hole card), 2 gutshot (one straight out), 3 open-ended or
/// double gutshot (two or more), 4 flush draw, 5 flush draw with straight outs.
pub fn category(board: &[Card], hole: [Card; 2]) -> usize {
    let mut on_board = [0_u8; 13];
    let mut all = [0_u8; 13];
    let mut suits = [0_u8; 4];
    for c in board {
        on_board[usize::from(c.rank())] += 1;
    }
    all.copy_from_slice(&on_board);
    for c in board.iter().chain(&hole) {
        suits[usize::from(c.suit())] += 1;
    }
    for c in hole {
        all[usize::from(c.rank())] += 1;
    }
    let full = |n: &[u8; 13]| {
        n.iter().any(|&x| x >= 4) || (n.contains(&3) && n.iter().filter(|&&x| x >= 2).count() >= 2)
    };
    let [a, b] = hole.map(|c| usize::from(c.rank()));
    let mut ranks = (0..13).rev().filter(|&r| on_board[r] > 0);
    let top = ranks.next().expect("a board");
    let second = ranks.next();
    let made = if full(&all) && !full(&on_board) {
        9
    } else if hole.iter().any(|c| suits[usize::from(c.suit())] >= 5) {
        8
    } else if straight(rank_mask(board.iter().chain(&hole))) && !straight(rank_mask(board)) {
        7
    } else if all[a] >= 3 || all[b] >= 3 {
        6
    } else if a != b && on_board[a] > 0 && on_board[b] > 0 {
        5
    } else if a == b {
        if a > top {
            4
        } else if second.is_some_and(|s| a > s) {
            2
        } else {
            1
        }
    } else if let Some(r) = [a, b].into_iter().filter(|&r| on_board[r] > 0).max() {
        if r == top {
            3
        } else if Some(r) == second {
            2
        } else {
            1
        }
    } else {
        0
    };
    if board.len() == 5 {
        return made;
    }
    let (flush_draw, outs) = draw_parts(board, hole);
    let backdoor =
        board.len() == 3 && made != 8 && hole.iter().any(|c| suits[usize::from(c.suit())] == 3);
    let draw = match (flush_draw, outs) {
        (true, 1..) => 5,
        (true, 0) => 4,
        (false, 2..) => 3,
        (false, 1) => 2,
        (false, 0) => usize::from(backdoor),
    };
    made * DRAW_CATEGORIES + draw
}

/// Calls `f(item, percentile)` with each item's percentile by key among
/// `items`, ties counting half.
fn percentiles<K: Ord>(mut items: Vec<(K, usize)>, mut f: impl FnMut(usize, f64)) {
    items.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let n = items.len() as f64;
    let mut start = 0;
    while start < items.len() {
        let mut end = start + 1;
        while end < items.len() && items[end].0 == items[start].0 {
            end += 1;
        }
        let percentile = (start as f64 + 0.5 * (end - start) as f64) / n;
        for &(_, h) in &items[start..end] {
            f(h, percentile);
        }
        start = end;
    }
}

/// A combo's draws on a flop or turn: bit 0 a flush draw (four cards of a
/// suit, at least one of them a hole card, and no flush), bit 1 an
/// open-ended straight draw (at least two ranks complete a straight that the
/// board and that rank alone do not make, and no straight).
pub fn draws(board: &[Card], hole: [Card; 2]) -> u8 {
    let (flush_draw, outs) = draw_parts(board, hole);
    u8::from(flush_draw) | (u8::from(outs >= 2) << 1)
}

/// A combo's weak draws: bit 0 a backdoor flush draw (on the flop only: three
/// cards of a suit, at least one of them a hole card, and no flush draw), bit
/// 1 a gutshot (exactly one rank completes a straight that the board and that
/// rank alone do not make, and no straight). The same definitions as the
/// draw categories of `category`.
pub fn weak_draws(board: &[Card], hole: [Card; 2]) -> u8 {
    let (flush_draw, outs) = draw_parts(board, hole);
    let mut suits = [0_u8; 4];
    for c in board.iter().chain(&hole) {
        suits[usize::from(c.suit())] += 1;
    }
    let backdoor =
        board.len() == 3 && !flush_draw && hole.iter().any(|c| suits[usize::from(c.suit())] == 3);
    u8::from(backdoor) | (u8::from(outs == 1) << 1)
}

impl BucketSource for Ehs2Abstraction {
    fn state_identity(&self) -> [u8; 32] {
        self.content_hash()
    }

    fn count(&self, street: Street) -> usize {
        self.num_buckets(street) as usize
    }
    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS] {
        self.bucket_row(board)
    }
}

pub const Q: f64 = 1_712_304.0 / 2_598_960.0;

pub struct Board {
    pub cards: [Card; 5],
    pub buckets: [[u16; NUM_COMBOS]; 3],
    pub ranks: [u16; NUM_COMBOS],
    /// Live combos ordered by (rank, combo), independent of thread scheduling.
    pub sorted: Vec<usize>,
    pub(crate) groups: Vec<(usize, usize)>,
    /// Relative buckets only: per street, the bucket count and the live combos
    /// in ascending strength with the ranges of equal strength.
    pub(crate) relative: Option<[Strength; 3]>,
}

pub(crate) struct Strength {
    /// Percentile buckets, and `BucketSource::classes` per percentile bucket.
    pub count: usize,
    pub classes: usize,
    pub prior: f64,
    pub prior_fraction: f64,
    /// The live combo's `BucketSource::class`, `[combo]`.
    pub class: Vec<u16>,
    pub order: Vec<usize>,
    pub groups: Vec<(usize, usize)>,
    pub low_spr: Option<LowSprStrength>,
    /// Per-leaf variants 1.. (`BucketSource::variants`): the percentile
    /// buckets, the classes per bucket and the shift of every live combo's
    /// `class`.
    pub variants: Vec<(usize, usize, isize)>,
    /// Experimental `BucketSource::opponent_levels` on this street.
    pub opponent: Option<OpponentLevels>,
}

/// Where an equity level sits in a class: `class / stride % levels` of the
/// base mapping, and the live combos in ascending showdown rank on the board
/// so far with the ranges of equal rank. On the river that is the strength
/// order (`Strength::order`, `groups`), and `showdown` is `None`. A per-leaf
/// variant `v` has `variants[v - 1]` levels, a divisor of `levels`.
pub(crate) struct OpponentLevels {
    pub levels: usize,
    pub stride: usize,
    pub showdown: Option<Ranked>,
    pub variants: Vec<usize>,
}

/// Live combos in ascending order with the ranges of equal value, as
/// `Strength::order` and `Strength::groups`.
pub(crate) type Ranked = (Vec<usize>, Vec<(usize, usize)>);

pub(crate) struct LowSprStrength {
    pub max_spr: f64,
    pub count: usize,
    pub classes: usize,
    pub class: Vec<u16>,
}

impl Board {
    pub fn new(cards: [Card; 5], source: &dyn BucketSource) -> Self {
        let catalog = Classes::get();
        let dead = cards.iter().fold(0_u64, |b, c| b | (1 << c.index()));
        assert_eq!(dead.count_ones(), 5);
        let mut ranks = [0; NUM_COMBOS];
        let mut sorted = Vec::with_capacity(1081);
        for (h, rank) in ranks.iter_mut().enumerate() {
            if catalog.combo_mask(h) & dead == 0 {
                *rank = rank_of(cards.into_iter().chain(catalog.cards(h))).0;
                sorted.push(h);
            }
        }
        sorted.sort_unstable_by_key(|&h| (ranks[h], h));
        let groups = equal_runs(&sorted, |h| ranks[h]);
        let buckets: [[u16; NUM_COMBOS]; 3] = std::array::from_fn(|i| source.row(&cards[..i + 3]));
        let relative = source.relative().then(|| {
            let prior = source.relative_prior();
            assert!(prior.is_finite() && prior >= 0.0, "invalid relative prior");
            let prior_fraction = source.relative_prior_fraction();
            assert!(
                prior_fraction.is_finite() && prior_fraction >= 0.0,
                "invalid relative prior fraction"
            );
            // Compute the recalled flop class once, before either later street.
            // Default specifications do not allocate or classify recalled hands.
            let recall = (source.flop_recall_classes(Street::Turn) > 1).then(|| {
                let mut recalled = [0; NUM_COMBOS];
                for &h in &sorted {
                    recalled[h] =
                        source.flop_recall_class(&cards[..3], catalog.cards(h), buckets[0][h]);
                }
                recalled
            });
            std::array::from_fn(|s| {
                let key = |h: usize| {
                    if s == 2 {
                        u32::from(ranks[h])
                    } else {
                        u32::from(buckets[s][h])
                    }
                };
                let mut order = sorted.clone();
                order.sort_unstable_by_key(|&h| (key(h), h));
                let groups = equal_runs(&order, key);
                let street = [Street::Flop, Street::Turn, Street::River][s];
                let mut class = vec![0; NUM_COMBOS];
                for &h in &sorted {
                    class[h] = source.class(&cards[..s + 3], buckets[s][h]);
                }
                let hand = source.hand_classes(street);
                if hand > 1 {
                    let board = &cards[..s + 3];
                    let seen = board.iter().fold(0_u64, |b, c| b | (1 << c.index()));
                    let live: Vec<usize> = (0..NUM_COMBOS)
                        .filter(|&h| catalog.combo_mask(h) & seen == 0)
                        .collect();
                    let mut potential = vec![0.0; NUM_COMBOS];
                    if source.hand_potential(street) {
                        // Sums of the bucket and its square over the next
                        // cards; every live combo sees the same number of them.
                        let mut sums = vec![(0_u64, 0_u64); NUM_COMBOS];
                        let mut next = board.to_vec();
                        next.push(board[0]);
                        for x in (0..52_u8).filter(|&x| seen & (1 << x) == 0) {
                            next[board.len()] = Card::from_index(x);
                            let row = source.row(&next);
                            for &h in &live {
                                if catalog.combo_mask(h) & (1 << x) == 0 {
                                    let v = u64::from(row[h]);
                                    sums[h].0 += v;
                                    sums[h].1 += v * v;
                                }
                            }
                        }
                        let n = 52 - board.len() as u64 - 2;
                        percentiles(
                            live.iter()
                                .map(|&h| (n * sums[h].1 - sums[h].0 * sums[h].0, h))
                                .collect(),
                            |h, p| potential[h] = p,
                        );
                    }
                    let mut category_of = vec![0; NUM_COMBOS];
                    let mut within = vec![0.0; NUM_COMBOS];
                    if source.hand_category() {
                        let mut members = vec![Vec::new(); MADE_CATEGORIES * DRAW_CATEGORIES];
                        for &h in &live {
                            category_of[h] = category(board, catalog.cards(h));
                            members[category_of[h]].push((key(h), h));
                        }
                        for items in members {
                            percentiles(items, |h, p| within[h] = p);
                        }
                    }
                    percentiles(
                        live.iter()
                            .map(|&h| (rank_of(board.iter().copied().chain(catalog.cards(h))).0, h))
                            .collect(),
                        |h, made| {
                            if catalog.combo_mask(h) & dead == 0 {
                                let features = HandFeatures {
                                    made,
                                    draws: draws(board, catalog.cards(h)),
                                    weak_draws: weak_draws(board, catalog.cards(h)),
                                    potential: potential[h],
                                    category: category_of[h],
                                    within: within[h],
                                    pocket_pair: {
                                        let [a, b] = catalog.cards(h);
                                        a.rank() == b.rank()
                                    },
                                };
                                class[h] =
                                    class[h] * hand as u16 + source.hand_class(street, &features);
                            }
                        },
                    );
                }
                let recalled_classes = source.flop_recall_classes(street);
                if recalled_classes > 1 {
                    for &h in &sorted {
                        class[h] = class[h] * recalled_classes as u16 + recall.as_ref().unwrap()[h];
                    }
                }
                // The finer spec changes only percentiles, levels and flop
                // groups. Reuse strength order and hand features; neither
                // showdown ranks nor hand classification are computed twice.
                let low_spr = source.low_spr().map(|(max_spr, finer)| {
                    let hand = source.hand_classes(street) * recalled_classes;
                    assert_eq!(
                        hand,
                        finer.hand_classes(street) * finer.flop_recall_classes(street)
                    );
                    let mut finer_class = vec![0; NUM_COMBOS];
                    for &h in &sorted {
                        finer_class[h] = finer.class(&cards[..s + 3], buckets[s][h]) * hand as u16
                            + class[h] % hand as u16;
                    }
                    LowSprStrength {
                        max_spr,
                        count: finer.count(street) / finer.classes(street),
                        classes: finer.classes(street),
                        class: finer_class,
                    }
                });
                // A variant changes only the board-wide part of the class (the
                // same shift moves every live combo's class on this board) and
                // the number of percentile buckets. A variant with other levels
                // relies on opponent levels to replace each combo's level.
                let board = &cards[..s + 3];
                let variants = source
                    .variants()
                    .iter()
                    .map(|v| {
                        let hand = source.hand_classes(street) * recalled_classes;
                        assert_eq!(hand, v.hand_classes(street) * v.flop_recall_classes(street));
                        assert!(
                            v.levels == source.levels() || source.opponent_levels(street),
                            "a variant with other levels needs opponent levels on {street:?}"
                        );
                        let shift = v.class(board, 0) as isize - source.class(board, 0) as isize;
                        (
                            v.count(street) / v.classes(street),
                            v.classes(street),
                            shift * hand as isize,
                        )
                    })
                    .collect();
                let opponent = source.opponent_levels(street).then(|| {
                    assert!(
                        source.low_spr().is_none(),
                        "opponent levels do not support a low-SPR mapping"
                    );
                    let levels = source.levels();
                    let variants: Vec<usize> = source.variants().iter().map(|v| v.levels).collect();
                    assert!(
                        variants.iter().all(|&n| n > 0 && levels.is_multiple_of(n)),
                        "a variant's opponent levels must divide the base's"
                    );
                    let showdown = (s < 2).then(|| {
                        let board = &cards[..s + 3];
                        let mut rank = [0; NUM_COMBOS];
                        for &h in &sorted {
                            rank[h] = rank_of(board.iter().copied().chain(catalog.cards(h))).0;
                        }
                        let mut order = sorted.clone();
                        order.sort_unstable_by_key(|&h| (rank[h], h));
                        let groups = equal_runs(&order, |h| rank[h]);
                        (order, groups)
                    });
                    OpponentLevels {
                        levels,
                        stride: source.hand_classes(street) * recalled_classes,
                        showdown,
                        variants,
                    }
                });
                Strength {
                    count: source.count(street) / source.classes(street),
                    classes: source.classes(street),
                    prior,
                    prior_fraction,
                    class,
                    order,
                    groups,
                    low_spr,
                    variants,
                    opponent,
                }
            })
        });
        Self {
            cards,
            buckets,
            ranks,
            sorted,
            groups,
            relative,
        }
    }
}

/// The ranges of consecutive entries of `order` with equal `key`.
fn equal_runs<K: PartialEq>(order: &[usize], key: impl Fn(usize) -> K) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && key(order[start]) == key(order[end]) {
            end += 1;
        }
        runs.push((start, end));
        start = end;
    }
    runs
}

/// Canonical flops in a fixed order with their cumulative raw multiplicities.
struct Flops {
    cards: Vec<[Card; 3]>,
    ends: Vec<u32>,
}

fn flops() -> &'static Flops {
    static FLOPS: OnceLock<Flops> = OnceLock::new();
    FLOPS.get_or_init(|| {
        let mut end = 0;
        let (cards, ends) = nlh::iso::canonical_flops()
            .into_iter()
            .map(|(board, weight)| {
                end += weight;
                ([board.flop[0], board.flop[1], board.flop[2]], end)
            })
            .unzip();
        Flops { cards, ends }
    })
}

/// The board at `x` in [0, 22,100): the canonical flop whose multiplicity
/// interval holds `x`, then the turn and river from the fractional position,
/// so a uniform `x` deals a uniform board up to suit symmetry.
pub(super) fn board_at(x: f64) -> [Card; 5] {
    let f = flops();
    let i = f
        .ends
        .partition_point(|&e| f64::from(e) <= x)
        .min(f.ends.len() - 1);
    let start = if i == 0 { 0 } else { f.ends[i - 1] };
    let width = f64::from(f.ends[i] - start);
    let r = ((x - f64::from(start)) / width).clamp(0.0, 1.0 - f64::EPSILON);
    board_runout(f.cards[i], r)
}

fn board_runout(flop: [Card; 3], r: f64) -> [Card; 5] {
    let mut live = [Card::from_index(0); 49];
    let mut n = 0;
    for card in (0..52).map(Card::from_index) {
        if !flop.contains(&card) {
            live[n] = card;
            n += 1;
        }
    }
    let turn = ((r * 49.0) as usize).min(48);
    let r = r * 49.0 - turn as f64;
    let river = ((r * 48.0) as usize).min(47);
    let river = live[river + usize::from(river >= turn)];
    [flop[0], flop[1], flop[2], live[turn], river]
}

pub(crate) fn boards(
    source: &dyn BucketSource,
    domain: &[u8],
    seed: u64,
    iteration: Option<u64>,
    count: u32,
    sampling: Sampling,
) -> Vec<Board> {
    let mut output = Vec::new();
    boards_into(
        source,
        domain,
        seed,
        iteration,
        count,
        sampling,
        &mut output,
    );
    output
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn boards_into(
    source: &dyn BucketSource,
    domain: &[u8],
    seed: u64,
    iteration: Option<u64>,
    count: u32,
    sampling: Sampling,
    output: &mut Vec<Board>,
) {
    if sampling == Sampling::Stratified {
        let offset = stratified_offset(domain, seed, iteration);
        let total = f64::from(*flops().ends.last().unwrap());
        (0..count)
            .into_par_iter()
            .map(|k| {
                let x = (offset + f64::from(k)) / f64::from(count) * total;
                Board::new(board_at(x), source)
            })
            .collect_into_vec(output);
        return;
    }
    (0..count)
        .into_par_iter()
        .map(|j| {
            let mut hash = blake3::Hasher::new();
            hash.update(domain);
            hash.update(&seed.to_le_bytes());
            if let Some(t) = iteration {
                hash.update(&t.to_le_bytes());
            }
            hash.update(&j.to_le_bytes());
            let mut rng = ChaCha8Rng::from_seed(*hash.finalize().as_bytes());
            let mut used = 0_u64;
            let cards = std::array::from_fn(|_| {
                loop {
                    let i = rng.gen_range(0..52_u8);
                    if used & (1 << i) == 0 {
                        used |= 1 << i;
                        break Card::from_index(i);
                    }
                }
            });
            Board::new(cards, source)
        })
        .collect_into_vec(output);
}

fn stratified_offset(domain: &[u8], seed: u64, iteration: Option<u64>) -> f64 {
    let mut hash = blake3::Hasher::new();
    hash.update(domain);
    hash.update(&seed.to_le_bytes());
    let bits = u64::from_le_bytes(hash.finalize().as_bytes()[..8].try_into().unwrap());
    let shift = (bits >> 11) as f64 / (1_u64 << 53) as f64;
    (shift + iteration.unwrap_or(0) as f64 * 0.618_033_988_749_894_9).fract()
}

pub(crate) fn subset_boards_into(
    source: &dyn BucketSource,
    subset: &WeightedFlopSubset,
    seed: u64,
    iteration: u64,
    count: u32,
    output: &mut Vec<Board>,
) {
    let offset = stratified_offset(b"solvers.p2.trunk.l1.board.v1", seed, Some(iteration));
    let total = *subset.ends.last().unwrap();
    (0..count)
        .into_par_iter()
        .map(|k| {
            let x = (offset + f64::from(k)) / f64::from(count) * total;
            Board::new(subset.board_at(x), source)
        })
        .collect_into_vec(output);
}

/// `count` boards for one iteration of a solve restricted to `flops`: board
/// `k` takes flop `(iteration * count + k) % flops.len()` and a uniform turn
/// and river seeded by `seed`, the iteration and `k`.
pub(crate) fn runouts_into(
    source: &dyn BucketSource,
    flops: &[[Card; 3]],
    seed: u64,
    iteration: u64,
    count: u32,
    output: &mut Vec<Board>,
) {
    (0..count)
        .into_par_iter()
        .map(|k| {
            let index = (iteration * u64::from(count) + u64::from(k)) % flops.len() as u64;
            let flop = flops[index as usize];
            let mut hash = blake3::Hasher::new();
            hash.update(b"solvers.p2.trunk.l1.runout.v1");
            hash.update(&seed.to_le_bytes());
            hash.update(&iteration.to_le_bytes());
            hash.update(&k.to_le_bytes());
            let mut rng = ChaCha8Rng::from_seed(*hash.finalize().as_bytes());
            let mut live: Vec<Card> = (0..52)
                .map(Card::from_index)
                .filter(|c| !flop.contains(c))
                .collect();
            let turn = live.remove(rng.gen_range(0..live.len()));
            let river = live[rng.gen_range(0..live.len())];
            Board::new([flop[0], flop[1], flop[2], turn, river], source)
        })
        .collect_into_vec(output);
}
