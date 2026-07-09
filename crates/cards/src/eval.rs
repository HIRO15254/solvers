use aya_poker::base::{CARDS, Hand};
use aya_poker::poker_rank;

use crate::card::Card;

/// Comparable strength of a 5-7 card hand under standard poker rules.
/// Higher is stronger. Only used at tree-build time (showdown tables),
/// never inside the solve loop.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct HandRank(pub u16);

/// Ranks a standard-poker hand of up to 7 cards.
pub fn rank_of(cards: impl IntoIterator<Item = Card>) -> HandRank {
    let mut hand = Hand::new();
    for card in cards {
        hand.insert_unchecked(&CARDS[card.index()]);
    }
    HandRank(poker_rank(&hand).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(s: &str) -> HandRank {
        rank_of(s.split_whitespace().map(|c| c.parse::<Card>().unwrap()))
    }

    #[test]
    fn hand_category_ordering() {
        let royal_flush = rank("As Ks Qs Js Ts");
        let quads = rank("Ac Ad Ah As Kc");
        let full_house = rank("Ac Ad Ah Kc Kd");
        let flush = rank("As Ks 9s 7s 3s");
        let straight = rank("9c 8d 7h 6s 5c");
        let trips = rank("Ac Ad Ah Kc Qd");
        let two_pair = rank("Ac Ad Kc Kd Qh");
        let pair = rank("Ac Ad Kc Qd Jh");
        let high_card = rank("Ac Kd Qh Js 9c");
        let hands = [
            high_card,
            pair,
            two_pair,
            trips,
            straight,
            flush,
            full_house,
            quads,
            royal_flush,
        ];
        for w in hands.windows(2) {
            assert!(w[0] < w[1], "{:?} should rank below {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn seven_card_uses_best_five() {
        // Board gives a straight; both hole-card sets play the board.
        let a = rank("9c 8d 7h 6s 5c 2c 2d");
        let b = rank("9c 8d 7h 6s 5c 3h 2h");
        assert_eq!(a, b);
    }

    #[test]
    fn kickers_matter() {
        assert!(rank("Ac Ad Kc Qd Jh") > rank("Ac Ad Kc Qd Th"));
    }
}
