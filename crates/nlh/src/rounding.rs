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

/// Relative distance within which a computed amount counts as a whole chip.
/// Percentages such as `0.29` are not exact in binary, so a size whose exact
/// value is whole can land a few ulps on either side; `down` and `up` must not
/// move such a value by a whole chip.
const EXACT_TOLERANCE: f64 = 1e-9;

impl Rounding {
    pub fn is_nearest(&self) -> bool {
        *self == Self::Nearest
    }

    /// Snap a non-negative exact amount in chips to a whole chip, saturating
    /// at `u64::MAX`.
    pub fn snap(self, exact: f64) -> u64 {
        let nearest = exact.round();
        let whole = if (exact - nearest).abs() <= EXACT_TOLERANCE * nearest.abs().max(1.0) {
            nearest
        } else {
            match self {
                Self::Down => exact.floor(),
                Self::Nearest => nearest,
                Self::Up => exact.ceil(),
            }
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
        // 29% of 100 is 29 exactly, though 100.0 * 0.29 is not.
        for rounding in [Rounding::Down, Rounding::Nearest, Rounding::Up] {
            assert_eq!(rounding.snap(100.0 * 0.29), 29);
        }
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
