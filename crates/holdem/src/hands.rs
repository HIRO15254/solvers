//! Stable per-player private-hand indices for an exact postflop subgame.
//!
//! A positive root weight retains a combo regardless of its magnitude. Later
//! public deals mask these indices; they do not renumber the private states.

use cards::{Card, CardSet, NUM_COMBOS, PerPlayer, Player, Range, combo_cards};

const ABSENT: u16 = u16::MAX;

#[derive(Clone, Debug)]
pub struct PostflopHands {
    combos: PerPlayer<Vec<u16>>,
    indices: PerPlayer<[u16; NUM_COMBOS]>,
}

impl PostflopHands {
    /// Root support after removing starting-board conflicts, in global combo order.
    pub fn from_ranges(board: &[Card], ranges: &PerPlayer<Range>) -> Self {
        let board: CardSet = board.iter().copied().collect();
        let select = |p: Player| {
            (0..NUM_COMBOS)
                .filter(|&combo| {
                    let (a, b) = combo_cards(combo);
                    ranges[p].weight(combo) > 0.0 && !board.contains(a) && !board.contains(b)
                })
                .map(|combo| combo as u16)
                .collect()
        };
        Self::from_combos(PerPlayer::new(select(Player::P0), select(Player::P1)))
    }

    pub(crate) fn dense() -> Self {
        let all: Vec<_> = (0..NUM_COMBOS as u16).collect();
        Self::from_combos(PerPlayer::new(all.clone(), all))
    }

    fn from_combos(combos: PerPlayer<Vec<u16>>) -> Self {
        let inverse = |p: Player| {
            let mut indices = [ABSENT; NUM_COMBOS];
            for (local, &combo) in combos[p].iter().enumerate() {
                indices[combo as usize] = local as u16;
            }
            indices
        };
        let indices = PerPlayer::new(inverse(Player::P0), inverse(Player::P1));
        Self { combos, indices }
    }

    pub fn combos(&self, player: Player) -> &[u16] {
        &self.combos[player]
    }

    pub fn len(&self, player: Player) -> usize {
        self.combos[player].len()
    }

    pub fn is_empty(&self, player: Player) -> bool {
        self.combos[player].is_empty()
    }

    pub fn combo(&self, player: Player, index: usize) -> usize {
        self.combos[player][index] as usize
    }

    pub fn index(&self, player: Player, combo: usize) -> Option<usize> {
        let index = *self.indices[player].get(combo)?;
        (index != ABSENT).then_some(index as usize)
    }

    /// Expand only at reporting boundaries which use the global 1,326-combo API.
    pub fn expand(&self, player: Player, values: &[f32]) -> Vec<f32> {
        assert_eq!(values.len(), self.len(player));
        let mut full = vec![0.0; NUM_COMBOS];
        for (&combo, &value) in self.combos[player].iter().zip(values) {
            full[combo as usize] = value;
        }
        full
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cards::combo_index;

    #[test]
    fn support_is_seat_specific_exact_and_board_filtered() {
        let hand = |a: &str, b: &str| combo_index(a.parse().unwrap(), b.parse().unwrap());
        let aa = hand("As", "Ah");
        let kk = hand("Ks", "Kh");
        let blocked = hand("2c", "3c");
        let mut oop = Range::default();
        oop.set_weight(aa, f32::MIN_POSITIVE);
        oop.set_weight(blocked, 1.0);
        let mut ip = Range::default();
        ip.set_weight(kk, 0.25);
        let hands = PostflopHands::from_ranges(
            &[
                "2c".parse().unwrap(),
                "7d".parse().unwrap(),
                "9h".parse().unwrap(),
            ],
            &PerPlayer::new(oop, ip),
        );
        assert_eq!(hands.combos(Player::P0), &[aa as u16]);
        assert_eq!(hands.combos(Player::P1), &[kk as u16]);
        assert_eq!(hands.index(Player::P0, aa), Some(0));
        assert_eq!(hands.index(Player::P0, kk), None);
        assert_eq!(hands.index(Player::P0, blocked), None);
        let expanded = hands.expand(Player::P1, &[0.625]);
        assert_eq!(expanded[kk], 0.625);
        assert_eq!(expanded.iter().filter(|&&v| v != 0.0).count(), 1);
    }

    #[test]
    fn repeated_range_entries_use_final_positive_support_without_duplicates() {
        let ranges = PerPlayer::new(
            "AA:0,AsAh:0.25,AsAh:0.75,AcAd:0".parse().unwrap(),
            "KK:0,KsKh:1,KsKh:0,KcKd:0.5".parse().unwrap(),
        );
        let hands = PostflopHands::from_ranges(&[], &ranges);
        let aa = combo_index("As".parse().unwrap(), "Ah".parse().unwrap());
        let kk = combo_index("Kc".parse().unwrap(), "Kd".parse().unwrap());
        assert_eq!(hands.combos(Player::P0), &[aa as u16]);
        assert_eq!(hands.combos(Player::P1), &[kk as u16]);
        assert_eq!(ranges[Player::P0].weight(aa), 0.75);
        assert_eq!(ranges[Player::P1].weight(kk), 0.5);
    }
}
