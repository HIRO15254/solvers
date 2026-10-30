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
    pub fn from_decimal(self, mantissa: u128, exponent: i32) -> Result<MwChips, ChipAmountError> {
        let (chips, changed) =
            self.from_decimal_rounded(mantissa, exponent, crate::Rounding::Down)?;
        if changed {
            Err(ChipAmountError::Fractional)
        } else {
            Ok(chips)
        }
    }

    /// Round an exact decimal BB value to chips. The flag reports a fractional
    /// chip in the input, even when rounding down produces zero.
    pub fn from_decimal_rounded(
        self,
        mantissa: u128,
        exponent: i32,
        rounding: crate::Rounding,
    ) -> Result<(MwChips, bool), ChipAmountError> {
        if mantissa == 0 {
            return Ok((MwChips::ZERO, false));
        }
        if exponent < 0 {
            let digits = mantissa.to_string();
            let places = exponent.unsigned_abs() as usize;
            if places > digits.len() + 7 {
                return Ok((MwChips(u64::from(rounding == crate::Rounding::Up)), true));
            }
            let text = if places >= digits.len() {
                format!("0.{}{digits}", "0".repeat(places - digits.len()))
            } else {
                let split = digits.len() - places;
                format!("{}.{}", &digits[..split], &digits[split..])
            };
            return self.parse_bb_rounded(&text, rounding);
        }
        let scaled = mantissa
            .checked_mul(u128::from(self.0))
            .ok_or(ChipAmountError::Overflow)?;
        let (whole, remainder, divisor) = if exponent >= 0 {
            let power = u32::try_from(exponent)
                .ok()
                .and_then(|e| 10u128.checked_pow(e))
                .ok_or(ChipAmountError::Overflow)?;
            (
                scaled.checked_mul(power).ok_or(ChipAmountError::Overflow)?,
                0,
                1,
            )
        } else if let Some(divisor) = exponent
            .checked_neg()
            .and_then(|e| u32::try_from(e).ok())
            .and_then(|e| 10u128.checked_pow(e))
        {
            (scaled / divisor, scaled % divisor, divisor)
        } else {
            // The divisor exceeds u128, so the scaled value is less than one
            // tenth of a chip and cannot reach the nearest-mode half.
            (0, scaled, 0)
        };
        let changed = remainder != 0;
        let increment = changed
            && match rounding {
                crate::Rounding::Down => false,
                crate::Rounding::Up => true,
                crate::Rounding::Nearest => divisor != 0 && remainder >= divisor - remainder,
            };
        let chips = whole
            .checked_add(u128::from(increment))
            .ok_or(ChipAmountError::Overflow)?;
        Ok((
            MwChips(u64::try_from(chips).map_err(|_| ChipAmountError::Overflow)?),
            changed,
        ))
    }

    /// Parse and round a plain unsigned decimal BB spelling.
    pub fn parse_bb_rounded(
        self,
        text: &str,
        rounding: crate::Rounding,
    ) -> Result<(MwChips, bool), ChipAmountError> {
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
        let whole = whole.bytes().try_fold(0u64, |n, digit| {
            n.checked_mul(10)
                .and_then(|n| n.checked_add(u64::from(digit - b'0')))
                .ok_or(ChipAmountError::Overflow)
        })?;
        let mut carry = 0u64;
        let mut fractional_nonzero = false;
        let mut first_digit = 0;
        for digit in fraction.bytes().rev() {
            let product = u64::from(digit - b'0') * self.0 + carry;
            first_digit = product % 10;
            fractional_nonzero |= first_digit != 0;
            carry = product / 10;
        }
        let changed = fractional_nonzero;
        let increment = changed
            && match rounding {
                crate::Rounding::Down => false,
                crate::Rounding::Nearest => first_digit >= 5,
                crate::Rounding::Up => true,
            };
        let chips = whole
            .checked_mul(self.0)
            .and_then(|n| n.checked_add(carry))
            .and_then(|n| n.checked_add(u64::from(increment)))
            .ok_or(ChipAmountError::Overflow)?;
        Ok((MwChips(chips), changed))
    }

    /// Exact chips of a plain unsigned decimal BB spelling (`12`, `2.25`).
    pub fn parse_bb(self, text: &str) -> Result<MwChips, ChipAmountError> {
        let (chips, changed) = self.parse_bb_rounded(text, crate::Rounding::Down)?;
        if changed {
            Err(ChipAmountError::Fractional)
        } else {
            Ok(chips)
        }
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
    fn exact_decimal_rounding_obeys_modes_and_reports_changes() {
        for (mode, expected) in [
            (crate::Rounding::Down, [1, 1, 1]),
            (crate::Rounding::Nearest, [1, 2, 2]),
            (crate::Rounding::Up, [2, 2, 2]),
        ] {
            for (text, chips) in ["0.49", "0.5", "0.51"].into_iter().zip(expected) {
                assert_eq!(
                    scale(3).parse_bb_rounded(text, mode),
                    Ok((MwChips(chips), true))
                );
            }
            assert_eq!(
                scale(3).parse_bb_rounded("2", mode),
                Ok((MwChips(6), false))
            );
        }
        assert_eq!(
            scale(3).from_decimal_rounded(1, -60, crate::Rounding::Up),
            Ok((MwChips(1), true))
        );
        assert_eq!(
            scale(3).parse_bb_rounded("1e2", crate::Rounding::Nearest),
            Err(ChipAmountError::Syntax)
        );
    }

    #[test]
    fn long_decimals_round_without_mantissa_overflow() {
        let s = scale(3);
        let below = "0.16666666666666666666666666666666666666666666666666";
        let above = "0.16666666666666666666666666666666666666666666666667";
        assert_eq!(
            s.parse_bb_rounded(below, crate::Rounding::Nearest),
            Ok((MwChips(0), true))
        );
        assert_eq!(
            s.parse_bb_rounded(above, crate::Rounding::Nearest),
            Ok((MwChips(1), true))
        );
        assert_eq!(
            s.parse_bb_rounded(below, crate::Rounding::Up),
            Ok((MwChips(1), true))
        );
        assert_eq!(
            s.parse_bb("1.00000000000000000000000000000000000000000000000000"),
            Ok(MwChips(3))
        );
        assert_eq!(s.parse_bb(below), Err(ChipAmountError::Fractional));
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
