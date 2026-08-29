//! Card, chip, and range primitives shared by all solver crates.
//!
//! This crate has no dependency on the engine or any game variant. Card
//! encoding follows `index = 4 * rank + suit` with rank `0 = Two .. 12 = Ace`
//! and suit `0 = clubs, 1 = diamonds, 2 = hearts, 3 = spades`, which is the
//! same encoding as the `aya_poker` evaluator we wrap.

mod board;
mod card;
mod eval;
mod range;
mod set;
mod sizing;
mod types;

/// Tree-script front end: `.tree` source in, compiled rules out. Kept a
/// module rather than re-exported flat, because `Rule`, `Effect`, `Var` and
/// `Literal` are generic enough names to want the `script::` qualifier at
/// every use site.
pub mod script;

pub use board::BoardFacts;
pub use card::{ALL_CARDS, Card, NUM_CARDS, ParseCardError, Rank, Suit, rank_name};
pub use eval::{HandRank, rank_of};
pub use range::{
    NUM_CLASSES, NUM_COMBOS, ParseRangeError, Range, class_index, combo_cards, combo_index,
};
pub use set::CardSet;
pub use sizing::{ParseSizeError, SizeSpec, SizeUnit, geometric_allin_target};
pub use types::{Chips, PerPlayer, Player, Street};
