//! Fixed, seat-specific root range support, ordered by global combo number.
use nlh::{Card, CardSet, NUM_COMBOS, PerPlayer, Player, Range, combo_cards};
pub(crate) const ABSENT: u16 = u16::MAX;
#[derive(Clone, Debug)]
pub struct PostflopHands {
    combos: PerPlayer<Vec<u16>>,
    local: PerPlayer<Vec<u16>>,
    pub(crate) same: PerPlayer<Vec<u16>>,
}
impl PostflopHands {
    pub fn new(board: &[Card], ranges: &PerPlayer<Range>) -> Self {
        let board: CardSet = board.iter().copied().collect();
        let support = |p: Player| {
            (0..NUM_COMBOS)
                .filter(|&h| {
                    let (a, b) = combo_cards(h);
                    ranges[p].weight(h) > 0.0 && !board.contains(a) && !board.contains(b)
                })
                .map(|h| h as u16)
                .collect::<Vec<_>>()
        };
        let combos = PerPlayer::new(support(Player::P0), support(Player::P1));
        let inverse = |p: Player| {
            let mut map = vec![ABSENT; NUM_COMBOS];
            for (i, &h) in combos[p].iter().enumerate() {
                map[h as usize] = i as u16;
            }
            map
        };
        let local = PerPlayer::new(inverse(Player::P0), inverse(Player::P1));
        let same = |p: Player| {
            combos[p]
                .iter()
                .map(|&h| local[p.opponent()][h as usize])
                .collect()
        };
        let same = PerPlayer::new(same(Player::P0), same(Player::P1));
        Self {
            combos,
            local,
            same,
        }
    }
    pub fn combos(&self, p: Player) -> &[u16] {
        &self.combos[p]
    }
    pub fn len(&self, p: Player) -> usize {
        self.combos[p].len()
    }
    pub fn is_empty(&self, p: Player) -> bool {
        self.combos[p].is_empty()
    }
    pub fn local(&self, p: Player, global: usize) -> Option<usize> {
        self.local[p]
            .get(global)
            .copied()
            .filter(|&i| i != ABSENT)
            .map(usize::from)
    }
    pub fn expand(&self, p: Player, compact: &[f32]) -> Vec<f32> {
        assert_eq!(compact.len(), self.len(p));
        let mut global = vec![0.0; NUM_COMBOS];
        for (&h, &v) in self.combos[p].iter().zip(compact) {
            global[h as usize] = v;
        }
        global
    }
    pub fn compact(&self, p: Player, global: &[f32]) -> Vec<f32> {
        assert_eq!(global.len(), NUM_COMBOS);
        self.combos[p].iter().map(|&h| global[h as usize]).collect()
    }
}

/// `[hi, lo]` card indices of every combo, higher card first.
const COMBO_CARD_TABLE: [[u8; 2]; NUM_COMBOS] = {
    let mut table = [[0u8; 2]; NUM_COMBOS];
    let mut hi = 1;
    while hi < 52 {
        let mut lo = 0;
        while lo < hi {
            table[hi * (hi - 1) / 2 + lo] = [hi as u8, lo as u8];
            lo += 1;
        }
        hi += 1;
    }
    table
};

/// O(1) equivalent of [`nlh::combo_cards`], returning card indices.
pub(crate) fn combo_card_indices(combo: usize) -> [usize; 2] {
    let [hi, lo] = COMBO_CARD_TABLE[combo];
    [usize::from(hi), usize::from(lo)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_card_table_matches_nlh() {
        for combo in 0..NUM_COMBOS {
            let (hi, lo) = combo_cards(combo);
            assert_eq!(combo_card_indices(combo), [hi.index(), lo.index()]);
        }
    }
}
