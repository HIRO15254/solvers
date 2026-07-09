use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign, Not, Sub};

use crate::card::Card;

/// A set of cards as a 52-bit bitset.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CardSet(u64);

const FULL_MASK: u64 = (1 << 52) - 1;

impl CardSet {
    pub const EMPTY: CardSet = CardSet(0);
    pub const FULL: CardSet = CardSet(FULL_MASK);

    pub fn insert(&mut self, card: Card) {
        self.0 |= 1 << card.index();
    }

    pub fn remove(&mut self, card: Card) {
        self.0 &= !(1 << card.index());
    }

    pub fn contains(self, card: Card) -> bool {
        self.0 & (1 << card.index()) != 0
    }

    pub fn is_disjoint(self, other: CardSet) -> bool {
        self.0 & other.0 == 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Card> {
        let mut bits = self.0;
        std::iter::from_fn(move || {
            if bits == 0 {
                None
            } else {
                let i = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                Some(Card::from_index(i))
            }
        })
    }
}

impl FromIterator<Card> for CardSet {
    fn from_iter<T: IntoIterator<Item = Card>>(iter: T) -> Self {
        let mut set = CardSet::EMPTY;
        for card in iter {
            set.insert(card);
        }
        set
    }
}

impl BitOr for CardSet {
    type Output = CardSet;
    fn bitor(self, rhs: CardSet) -> CardSet {
        CardSet(self.0 | rhs.0)
    }
}

impl BitOrAssign for CardSet {
    fn bitor_assign(&mut self, rhs: CardSet) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for CardSet {
    type Output = CardSet;
    fn bitand(self, rhs: CardSet) -> CardSet {
        CardSet(self.0 & rhs.0)
    }
}

impl Sub for CardSet {
    type Output = CardSet;
    fn sub(self, rhs: CardSet) -> CardSet {
        CardSet(self.0 & !rhs.0)
    }
}

impl Not for CardSet {
    type Output = CardSet;
    fn not(self) -> CardSet {
        CardSet(!self.0 & FULL_MASK)
    }
}

impl fmt::Debug for CardSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::ALL_CARDS;

    #[test]
    fn insert_contains_remove() {
        let mut set = CardSet::EMPTY;
        let card: Card = "Qh".parse().unwrap();
        assert!(!set.contains(card));
        set.insert(card);
        assert!(set.contains(card));
        assert_eq!(set.len(), 1);
        set.remove(card);
        assert!(set.is_empty());
    }

    #[test]
    fn full_deck() {
        let set: CardSet = ALL_CARDS.into_iter().collect();
        assert_eq!(set, CardSet::FULL);
        assert_eq!(set.len(), 52);
        assert_eq!((!set).len(), 0);
    }
}
