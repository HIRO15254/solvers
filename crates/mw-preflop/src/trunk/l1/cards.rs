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

pub trait BucketSource: Sync {
    fn count(&self, street: Street) -> usize;
    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS];
}

impl BucketSource for Ehs2Abstraction {
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
        let mut groups = Vec::new();
        let mut start = 0;
        while start < sorted.len() {
            let mut end = start + 1;
            while end < sorted.len() && ranks[sorted[start]] == ranks[sorted[end]] {
                end += 1;
            }
            groups.push((start, end));
            start = end;
        }
        Self {
            cards,
            buckets: std::array::from_fn(|i| source.row(&cards[..i + 3])),
            ranks,
            sorted,
            groups,
        }
    }
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
    let flop = f.cards[i];
    let mut live: Vec<Card> = (0..52)
        .map(Card::from_index)
        .filter(|c| !flop.contains(c))
        .collect();
    let turn = ((r * 49.0) as usize).min(48);
    let r = r * 49.0 - turn as f64;
    let turn = live.remove(turn);
    let river = live[((r * 48.0) as usize).min(47)];
    [flop[0], flop[1], flop[2], turn, river]
}

pub(crate) fn boards(
    source: &dyn BucketSource,
    domain: &[u8],
    seed: u64,
    iteration: Option<u64>,
    count: u32,
    sampling: Sampling,
) -> Vec<Board> {
    if sampling == Sampling::Stratified {
        let mut hash = blake3::Hasher::new();
        hash.update(domain);
        hash.update(&seed.to_le_bytes());
        let bits = u64::from_le_bytes(hash.finalize().as_bytes()[..8].try_into().unwrap());
        let shift = (bits >> 11) as f64 / (1_u64 << 53) as f64;
        let offset = (shift + iteration.unwrap_or(0) as f64 * 0.618_033_988_749_894_9).fract();
        let total = f64::from(*flops().ends.last().unwrap());
        return (0..count)
            .into_par_iter()
            .map(|k| {
                let x = (offset + f64::from(k)) / f64::from(count) * total;
                Board::new(board_at(x), source)
            })
            .collect();
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
        .collect()
}
