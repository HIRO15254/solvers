//! Card, chip, and range primitives shared by all solver crates.
//!
//! This crate has no dependency on the engine or any game variant. Card
//! encoding follows `index = 4 * rank + suit` with rank `0 = Two .. 12 = Ace`
//! and suit `0 = clubs, 1 = diamonds, 2 = hearts, 3 = spades`, which is the
//! same encoding as the `aya_poker` evaluator we wrap.

mod card;
mod eval;
mod range;
mod set;
mod types;

pub use card::{ALL_CARDS, Card, NUM_CARDS, ParseCardError, Rank, Suit};
pub use eval::{HandRank, rank_of};
pub use range::{
    NUM_CLASSES, NUM_COMBOS, ParseRangeError, Range, class_index, combo_cards, combo_index,
};
pub use set::CardSet;
pub use types::{Chips, PerPlayer, Player, Street};
