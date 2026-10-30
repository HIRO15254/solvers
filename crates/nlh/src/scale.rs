//! How many table chips make one big blind, and exact conversion between
//! chip counts and big-blind decimals.

use serde::{Deserialize, Serialize};

use crate::table::{ChipAmountError, MwChips};

/// Table chips per big blind. One internal unit is one table chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChipScale(u64);

impl Default for ChipScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl ChipScale {
    /// One chip is 0.001 BB.
    pub const DEFAULT: Self = Self(1_000);
    /// Largest accepted chips per big blind.
    pub const MAX: u64 = 1_000_000;

    /// `None` unless `1 <= chips_per_bb <= MAX`.
    pub const fn new(chips_per_bb: u64) -> Option<Self> {
        if chips_per_bb == 0 || chips_per_bb > Self::MAX {
            None
        } else {
            Some(Self(chips_per_bb))
        }
    }

    pub const fn chips_per_bb(self) -> u64 {
        self.0
    }

    pub fn is_default(&self) -> bool {
        *self == Self::DEFAULT
    }

    /// One big blind in chips.
    pub const fn bb(self) -> MwChips {
        MwChips(self.0)
    }

    pub fn to_bb(self, chips: MwChips) -> f64 {
        chips.raw() as f64 / self.0 as f64
    }

    /// Nearest whole chip to a computed big-blind amount. Exact decimal input
    /// goes through [`Self::parse_bb`] or [`Self::from_decimal`] instead.
    pub fn from_bb(self, bb: f64) -> Result<MwChips, ChipAmountError> {
        if !bb.is_finite() {
            return Err(ChipAmountError::NotFinite);
        }
        if bb < 0.0 {
            return Err(ChipAmountError::Negative);
        }
        let chips = (bb * self.0 as f64).round();
        if chips > u64::MAX as f64 {
            return Err(ChipAmountError::Overflow);
        }
        Ok(MwChips(chips as u64))
    }

    /// Exact chips of `mantissa × 10^exponent` BB; fails unless whole.
    pub fn from_decimal(
        self,
        mut mantissa: u128,
        mut exponent: i32,
    ) -> Result<MwChips, ChipAmountError> {
        if mantissa == 0 {
            return Ok(MwChips::ZERO);
        }
        while mantissa.is_multiple_of(10) {
            mantissa /= 10;
            exponent += 1;
        }
        let scaled = mantissa
            .checked_mul(u128::from(self.0))
            .ok_or(ChipAmountError::Overflow)?;
        let chips = if exponent >= 0 {
            u32::try_from(exponent)
                .ok()
                .and_then(|e| 10u128.checked_pow(e))
                .and_then(|p| scaled.checked_mul(p))
                .ok_or(ChipAmountError::Overflow)?
        } else {
            // 10^39 exceeds u128; no scale can cancel that many tens.
            let divisor = u32::try_from(-exponent)
                .ok()
                .and_then(|e| 10u128.checked_pow(e))
                .ok_or(ChipAmountError::Fractional)?;
            if scaled % divisor != 0 {
                return Err(ChipAmountError::Fractional);
            }
            scaled / divisor
        };
        u64::try_from(chips)
            .map(MwChips)
            .map_err(|_| ChipAmountError::Overflow)
    }

    /// Exact chips of a plain unsigned decimal BB spelling (`12`, `2.25`).
    pub fn parse_bb(self, text: &str) -> Result<MwChips, ChipAmountError> {
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if whole.is_empty()
            || (text.contains('.') && fraction.is_empty())
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return Err(ChipAmountError::Syntax);
        }
        let fraction = fraction.trim_end_matches('0');
        let whole = whole.trim_start_matches('0');
        let digits = format!("{whole}{fraction}");
        let mantissa = if digits.is_empty() {
            0
        } else {
            digits
                .parse::<u128>()
                .map_err(|_| ChipAmountError::Overflow)?
        };
        let exponent = -i32::try_from(fraction.len()).map_err(|_| ChipAmountError::Fractional)?;
        self.from_decimal(mantissa, exponent)
    }

    /// Whether every chip amount has a terminating BB decimal.
    pub fn is_decimal(self) -> bool {
        without_twos_and_fives(self.0) == 1
    }

    /// Shortest exact BB decimal of `chips`, or `None` when it does not
    /// terminate.
    pub fn bb_text(self, chips: MwChips) -> Option<String> {
        let denominator = self.0 / gcd(chips.raw(), self.0);
        if without_twos_and_fives(denominator) != 1 {
            return None;
        }
        let digits = power_count(denominator, 2).max(power_count(denominator, 5));
        let ten = 10u128.pow(digits);
        let value = u128::from(chips.raw()) * ten / u128::from(self.0);
        let (whole, fraction) = (value / ten, value % ten);
        Some(if digits == 0 {
            whole.to_string()
        } else {
            format!("{whole}.{fraction:0width$}", width = digits as usize)
        })
    }

    /// Canonical spelling of an amount: the BB decimal on a table whose chips
    /// all have one, otherwise the chip count with a `c` suffix.
    pub fn amount_text(self, chips: MwChips) -> String {
        if self.is_decimal() {
            self.bb_text(chips)
                .expect("decimal tables spell every amount")
        } else {
            format!("{}c", chips.raw())
        }
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn power_count(mut n: u64, p: u64) -> u32 {
    let mut count = 0;
    while n.is_multiple_of(p) {
        n /= p;
        count += 1;
    }
    count
}

fn without_twos_and_fives(mut n: u64) -> u64 {
    for p in [2, 5] {
        while n.is_multiple_of(p) {
            n /= p;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale(n: u64) -> ChipScale {
        ChipScale::new(n).unwrap()
    }

    #[test]
    fn bb_decimals_convert_exactly_or_fail() {
        let milli = ChipScale::DEFAULT;
        assert_eq!(milli.parse_bb("2.5"), Ok(MwChips(2_500)));
        assert_eq!(milli.parse_bb("0.001"), Ok(MwChips(1)));
        assert_eq!(milli.parse_bb("1.2340000"), Ok(MwChips(1_234)));
        assert_eq!(milli.parse_bb("0.0005"), Err(ChipAmountError::Fractional));
        assert_eq!(scale(3).parse_bb("100"), Ok(MwChips(300)));
        assert_eq!(scale(3).parse_bb("0.5"), Err(ChipAmountError::Fractional));
        assert_eq!(scale(8).parse_bb("0.125"), Ok(MwChips(1)));
        for bad in ["", ".5", "1.", "-1", "1e3", "+2", "1.2.3"] {
            assert_eq!(milli.parse_bb(bad), Err(ChipAmountError::Syntax), "{bad}");
        }
        assert_eq!(milli.from_decimal(25, 2), Ok(MwChips(2_500_000)));
        assert_eq!(milli.from_decimal(1, -60), Err(ChipAmountError::Fractional));
        assert_eq!(
            milli.from_decimal(u128::MAX, 0),
            Err(ChipAmountError::Overflow)
        );
        assert_eq!(scale(3).from_bb(1.0 / 3.0), Ok(MwChips(1)));
    }

    #[test]
    fn spelling_is_bb_on_decimal_tables_and_chips_otherwise() {
        let milli = ChipScale::DEFAULT;
        assert_eq!(milli.amount_text(MwChips(2_500)), "2.5");
        assert_eq!(milli.amount_text(MwChips(3_000)), "3");
        assert_eq!(milli.amount_text(MwChips(1)), "0.001");
        assert_eq!(scale(8).amount_text(MwChips(1)), "0.125");
        assert_eq!(scale(3).amount_text(MwChips(10)), "10c");
        assert_eq!(scale(3).amount_text(MwChips(3)), "3c");
        assert_eq!(scale(3).bb_text(MwChips(3)).as_deref(), Some("1"));
        assert_eq!(scale(3).bb_text(MwChips(10)), None);
        assert_eq!(scale(6).bb_text(MwChips(3)).as_deref(), Some("0.5"));
        assert!(ChipScale::new(0).is_none());
        assert!(ChipScale::new(ChipScale::MAX + 1).is_none());
    }
}
