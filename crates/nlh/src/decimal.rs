//! Decimal arithmetic for chip amounts derived from finite floating-point inputs.

use crate::Rounding;

/// Multiply `amount` by the shortest round-trip decimal representation of
/// `factor`, divide by `unit`, and round to whole units. Returns `None` on
/// overflow. Inputs are non-negative and `unit` must be nonzero.
pub fn rounded_product(amount: u64, factor: f64, unit: u64, rounding: Rounding) -> Option<u64> {
    if amount == 0 || factor <= 0.0 {
        return Some(0);
    }
    if !factor.is_finite() || unit == 0 {
        return None;
    }
    let text = factor.to_string();
    let (integer, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let digits = format!("{integer}{fraction}");
    let significant = digits.trim_start_matches('0').trim_end_matches('0');
    if significant.is_empty() {
        return Some(0);
    }
    let trailing = digits.trim_start_matches('0').len() - significant.len();
    let exponent = trailing as i32 - fraction.len() as i32;
    let mantissa: u128 = significant.parse().ok()?;
    let product = u128::from(amount).checked_mul(mantissa)?;
    let (whole, remainder, divisor) = if exponent >= 0 {
        let power = 10u128.checked_pow(exponent as u32)?;
        let numerator = product.checked_mul(power)?;
        let divisor = u128::from(unit);
        (numerator / divisor, numerator % divisor, divisor)
    } else if let Some(divisor) = 10u128
        .checked_pow(exponent.unsigned_abs())
        .and_then(|power| power.checked_mul(u128::from(unit)))
    {
        (product / divisor, product % divisor, divisor)
    } else {
        // A shortest f64 decimal has at most 17 significant digits; with a
        // u64 amount its product is far less than half an overflowing divisor.
        (0, product, 0)
    };
    let increment = remainder != 0
        && match rounding {
            Rounding::Down => false,
            Rounding::Up => true,
            Rounding::Nearest => divisor != 0 && remainder >= divisor - remainder,
        };
    u64::try_from(whole.checked_add(u128::from(increment))?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_tiny_factors_and_large_divisors() {
        assert_eq!(rounded_product(u64::MAX, 1e-100, 1, Rounding::Up), Some(1));
        assert_eq!(
            rounded_product(u64::MAX, 1e-100, 1, Rounding::Nearest),
            Some(0)
        );
        assert_eq!(
            rounded_product(u64::MAX, 0.1, u64::MAX, Rounding::Up),
            Some(1)
        );
        assert_eq!(
            rounded_product(u64::MAX, 1.0, 1, Rounding::Down),
            Some(u64::MAX)
        );
    }
}
