use std::fmt;
use std::str::FromStr;

pub const NUM_CARDS: usize = 52;

const RANK_CHARS: [char; 13] = [
    '2', '3', '4', '5', '6', '7', '8', '9', 'T', 'J', 'Q', 'K', 'A',
];
const SUIT_CHARS: [char; 4] = ['c', 'd', 'h', 's'];

/// Card rank, `Two = 0` through `Ace = 12`.
pub type Rank = u8;
/// Card suit, `0 = clubs, 1 = diamonds, 2 = hearts, 3 = spades`.
pub type Suit = u8;

/// A card from a standard 52-card deck, encoded as `4 * rank + suit`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Card(u8);

/// All 52 cards in index order.
pub const ALL_CARDS: AllCards = AllCards;

#[derive(Clone, Copy)]
pub struct AllCards;

impl IntoIterator for AllCards {
    type Item = Card;
    type IntoIter = std::iter::Map<std::ops::Range<u8>, fn(u8) -> Card>;

    fn into_iter(self) -> Self::IntoIter {
        (0..NUM_CARDS as u8).map(Card)
    }
}

impl Card {
    /// Creates a card from its 0..52 index. Panics if out of range.
    pub fn from_index(index: u8) -> Self {
        assert!((index as usize) < NUM_CARDS, "card index out of range");
        Card(index)
    }

    pub fn new(rank: Rank, suit: Suit) -> Self {
        assert!(rank < 13 && suit < 4);
        Card(4 * rank + suit)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn rank(self) -> Rank {
        self.0 / 4
    }

    pub fn suit(self) -> Suit {
        self.0 % 4
    }

    /// Returns the same card with its suit replaced.
    pub fn with_suit(self, suit: Suit) -> Self {
        Card::new(self.rank(), suit)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid card string: {0:?}")]
pub struct ParseCardError(pub String);

impl FromStr for Card {
    type Err = ParseCardError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        let (Some(r), Some(su), None) = (chars.next(), chars.next(), chars.next()) else {
            return Err(ParseCardError(s.to_string()));
        };
        let rank = RANK_CHARS
            .iter()
            .position(|&c| c == r.to_ascii_uppercase())
            .ok_or_else(|| ParseCardError(s.to_string()))?;
        let suit = SUIT_CHARS
            .iter()
            .position(|&c| c == su.to_ascii_lowercase())
            .ok_or_else(|| ParseCardError(s.to_string()))?;
        Ok(Card::new(rank as u8, suit as u8))
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            RANK_CHARS[self.rank() as usize],
            SUIT_CHARS[self.suit() as usize]
        )
    }
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

pub(crate) fn rank_from_char(c: char) -> Option<Rank> {
    RANK_CHARS
        .iter()
        .position(|&r| r == c.to_ascii_uppercase())
        .map(|r| r as Rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_display_roundtrip() {
        for card in ALL_CARDS {
            let s = card.to_string();
            assert_eq!(s.parse::<Card>().unwrap(), card);
        }
    }

    #[test]
    fn known_cards() {
        let ace_of_spades: Card = "As".parse().unwrap();
        assert_eq!(ace_of_spades.index(), 51);
        assert_eq!(ace_of_spades.rank(), 12);
        assert_eq!(ace_of_spades.suit(), 3);
        let deuce_of_clubs: Card = "2c".parse().unwrap();
        assert_eq!(deuce_of_clubs.index(), 0);
    }

    #[test]
    fn rejects_garbage() {
        assert!("Xx".parse::<Card>().is_err());
        assert!("A".parse::<Card>().is_err());
        assert!("Ass".parse::<Card>().is_err());
    }
}
