//! Opponent mass after removing the two cards in a player's private hand.
//!
//! This is also the reporting denominator and root-deal normalizer primitive.
//! When f64 cannot represent all relevant sums exactly, use integer masses
//! before subtraction so a large blocked hand cannot erase a tiny legal one.

use cards::{Player, combo_cards};

use crate::hands::PostflopHands;
use crate::mass::{ExactMass, Mass, f64_mass_is_exact};

#[derive(Clone, Copy)]
pub(crate) struct ExactSums<M: ExactMass = Mass> {
    pub(crate) total: M,
    pub(crate) per_card: [M; 52],
}

impl<M: ExactMass> Default for ExactSums<M> {
    fn default() -> Self {
        Self {
            total: M::ZERO,
            per_card: [M::ZERO; 52],
        }
    }
}

impl<M: ExactMass> ExactSums<M> {
    pub(crate) fn add_mass(&mut self, cards: [usize; 2], mass: M) {
        self.total = self.total.add(mass);
        for card in cards {
            self.per_card[card] = self.per_card[card].add(mass);
        }
    }

    pub(crate) fn compatible_mass(&self, cards: [usize; 2], same_hand: M) -> M {
        // Add the intersection first: every intermediate remains nonnegative.
        self.total
            .add(same_hand)
            .sub(self.per_card[cards[0]])
            .sub(self.per_card[cards[1]])
    }
}

impl ExactSums {
    pub(crate) fn add(&mut self, cards: [usize; 2], reach: f32) {
        if reach != 0.0 {
            self.add_mass(cards, Mass::from_f32(reach));
        }
    }

    pub(crate) fn compatible(&self, cards: [usize; 2], same_hand: f32) -> Mass {
        self.compatible_mass(cards, Mass::from_f32(same_hand))
    }
}

fn cards_of(combo: usize) -> [usize; 2] {
    let (a, b) = combo_cards(combo);
    [a.index(), b.index()]
}

/// Opponent reach compatible with each seat-local hand of `player`.
///
/// Inputs must be finite, nonnegative f32 reaches in the fixed initial hand
/// domain; callers mask later public cards before calling. No positive reach
/// is discarded. Card-removal masses are exact before their single f64
/// rounding. Utility multiplication and subsequent f32/serialization rounding
/// are separate. Work is linear in the two retained ranges.
pub fn compatible_reach(hands: &PostflopHands, player: Player, opp_reach: &[f32]) -> Vec<f64> {
    let opponent = player.opponent();
    assert_eq!(opp_reach.len(), hands.len(opponent));
    if f64_mass_is_exact(opp_reach) {
        let total: f64 = opp_reach.iter().map(|&r| r as f64).sum();
        let mut per_card = [0.0; 52];
        for (local, &reach) in opp_reach.iter().enumerate() {
            if reach != 0.0 {
                for card in cards_of(hands.combo(opponent, local)) {
                    per_card[card] += reach as f64;
                }
            }
        }
        hands
            .combos(player)
            .iter()
            .map(|&combo| {
                let [a, b] = cards_of(combo as usize);
                let overlap = hands
                    .index(opponent, combo as usize)
                    .map_or(0.0, |i| opp_reach[i] as f64);
                total - per_card[a] - per_card[b] + overlap
            })
            .collect()
    } else {
        let mut sums = ExactSums::default();
        for (local, &reach) in opp_reach.iter().enumerate() {
            sums.add(cards_of(hands.combo(opponent, local)), reach);
        }
        hands
            .combos(player)
            .iter()
            .map(|&combo| {
                let overlap = hands
                    .index(opponent, combo as usize)
                    .map_or(0.0, |i| opp_reach[i]);
                sums.compatible(cards_of(combo as usize), overlap).to_f64()
            })
            .collect()
    }
}
