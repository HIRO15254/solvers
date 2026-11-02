//! How derived amounts snap to whole table chips, and how tied winners divide
//! a pot. One internal unit is one table chip ([`crate::ChipScale`]).

use serde::{Deserialize, Serialize};

use crate::table::MwChips;

/// How an exact amount snaps to a whole number of chips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rounding {
    /// Toward zero.
    Down,
    /// To the closest whole chip; an exact half goes up.
    #[default]
    Nearest,
    /// Away from zero.
    Up,
}

impl Rounding {
    pub fn is_nearest(&self) -> bool {
        *self == Self::Nearest
    }

    /// Snap a non-negative exact amount in chips to a whole chip, saturating
    /// at `u64::MAX`.
    pub fn snap(self, exact: f64) -> u64 {
        let whole = match self {
            Self::Down => exact.floor(),
            Self::Nearest => exact.round(),
            Self::Up => exact.ceil(),
        };
        whole.max(0.0) as u64
    }
}

/// Divide `amount` among `winners` (positive) tied winners: each takes the
/// same number of chips and the odd chips go one at a time in seat order.
pub fn split_pot(amount: MwChips, winners: usize) -> PotSplit {
    let count = winners as u64;
    PotSplit {
        share: amount.raw() / count,
        odd: amount.raw() % count,
    }
}

/// One pot divided among tied winners; see [`split_pot`].
#[derive(Clone, Copy, Debug)]
pub struct PotSplit {
    share: u64,
    odd: u64,
}

impl PotSplit {
    /// Award of the winner at `order` (0 = first winner clockwise from the
    /// button).
    pub fn award(&self, order: usize) -> MwChips {
        MwChips(self.share + u64::from((order as u64) < self.odd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_follows_the_mode_but_keeps_whole_chips() {
        for (rounding, expected) in [
            (Rounding::Down, [108, 108, 109]),
            (Rounding::Nearest, [108, 109, 109]),
            (Rounding::Up, [109, 109, 109]),
        ] {
            assert_eq!(
                [
                    rounding.snap(108.4),
                    rounding.snap(108.5),
                    rounding.snap(109.0)
                ],
                expected
            );
        }
        assert_eq!(Rounding::Up.snap(1_000_000_000.4), 1_000_000_001);
        assert_eq!(Rounding::Down.snap(1_000_000_000.6), 1_000_000_000);
        assert_eq!(Rounding::Nearest.snap(f64::INFINITY), u64::MAX);
    }

    #[test]
    fn split_deals_odd_chips_in_order() {
        let awards = |amount, winners| {
            let split = split_pot(MwChips(amount), winners);
            (0..winners)
                .map(|order| split.award(order).raw())
                .collect::<Vec<_>>()
        };
        assert_eq!(awards(101, 3), [34, 34, 33]);
        assert_eq!(awards(100, 3), [34, 33, 33]);
        assert_eq!(awards(5, 9), [1, 1, 1, 1, 1, 0, 0, 0, 0]);
        assert_eq!(awards(1000, 2), [500, 500]);
    }
}
