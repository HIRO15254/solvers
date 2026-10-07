use crate::card_abstraction::{CardAbstraction, Ehs2Abstraction};
use crate::trunk::classes::Classes;
use nlh::{Card, NUM_COMBOS, Street, rank_of};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;

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

pub(crate) fn boards(
    source: &dyn BucketSource,
    domain: &[u8],
    seed: u64,
    iteration: Option<u64>,
    count: u32,
) -> Vec<Board> {
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
