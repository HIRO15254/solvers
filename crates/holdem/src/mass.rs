//! Exact nonnegative sums of binary32 reach, in units of 2^-149.
//!
//! At most 1,326 finite f32 terms plus one self-combo add-back fit in
//! 288 bits. Five limbs also leave room for checked intermediate sums.
//! This does not make utility products or the final f32 CFV exact.

const MAX_HANDS: usize = 1326;
const FRACTION_MASK: u32 = (1 << 23) - 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mass([u64; 5]);

impl Mass {
    pub(crate) const ZERO: Self = Self([0; 5]);

    /// Decode the represented f32 value exactly, including subnormals.
    /// Negative zero is accepted as zero; other negative/nonfinite values
    /// are not valid reach and must not be silently clamped.
    #[inline]
    pub(crate) fn from_f32(value: f32) -> Self {
        assert!(value.is_finite() && value >= 0.0, "invalid reach mass");
        if value == 0.0 {
            return Self::ZERO;
        }
        let bits = value.to_bits();
        let exponent = (bits >> 23) & 0xff;
        let fraction = bits & FRACTION_MASK;
        let (significand, shift) = if exponent == 0 {
            (fraction, 0)
        } else {
            (fraction | (1 << 23), exponent - 1)
        };
        let limb = (shift / 64) as usize;
        let offset = shift % 64;
        let mut result = [0; 5];
        result[limb] = u64::from(significand) << offset;
        if offset != 0 && limb + 1 < result.len() {
            result[limb + 1] = u64::from(significand) >> (64 - offset);
        }
        Self(result)
    }

    /// Exact addition, rejecting overflow rather than wrapping mass.
    #[inline]
    pub(crate) fn add(self, other: Self) -> Self {
        let mut result = [0; 5];
        let mut carry = false;
        for (index, slot) in result.iter_mut().enumerate() {
            let (sum, first) = self.0[index].overflowing_add(other.0[index]);
            let (sum, second) = sum.overflowing_add(u64::from(carry));
            *slot = sum;
            carry = first || second;
        }
        assert!(!carry, "mass addition overflow");
        Self(result)
    }

    /// Exact nonnegative subtraction, rejecting an invalid negative result.
    #[inline]
    pub(crate) fn sub(self, other: Self) -> Self {
        let mut result = [0; 5];
        let mut borrow = false;
        for (index, slot) in result.iter_mut().enumerate() {
            let (difference, first) = self.0[index].overflowing_sub(other.0[index]);
            let (difference, second) = difference.overflowing_sub(u64::from(borrow));
            *slot = difference;
            borrow = first || second;
        }
        assert!(!borrow, "mass subtraction underflow");
        Self(result)
    }

    /// Round once to binary64, nearest with ties to even.
    /// Every nonzero 320-bit value times 2^-149 is a normal, finite f64.
    #[inline]
    pub(crate) fn to_f64(self) -> f64 {
        let Some(limb) = self.0.iter().rposition(|&word| word != 0) else {
            return 0.0;
        };
        let high_bit = limb * 64 + (63 - self.0[limb].leading_zeros() as usize);
        let mut exponent = (high_bit + (1023 - 149)) as u64;
        let mut significand;
        if high_bit <= 52 {
            significand = self.0[0] << (52 - high_bit);
        } else {
            let shift = high_bit - 52;
            let word = shift / 64;
            let offset = shift % 64;
            significand = self.0[word] >> offset;
            if offset != 0 && word + 1 < self.0.len() {
                significand |= self.0[word + 1] << (64 - offset);
            }
            let guard_index = shift - 1;
            let guard = self.0[guard_index / 64] & (1u64 << (guard_index % 64)) != 0;
            let whole_words = guard_index / 64;
            let partial_bits = guard_index % 64;
            let sticky = self.0[..whole_words].iter().any(|&word| word != 0)
                || (self.0[whole_words] & ((1u64 << partial_bits) - 1) != 0);
            if guard && (sticky || significand & 1 != 0) {
                significand += 1;
                if significand == 1u64 << 53 {
                    significand >>= 1;
                    exponent += 1;
                }
            }
        }
        f64::from_bits((exponent << 52) | (significand & ((1u64 << 52) - 1)))
    }
}

/// Validated reach bounds shared by the f64 gate and integer fallback.
/// The floating-point scale is constructed only when the fallback needs it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MassAnalysis {
    bound: u32,
    min_shift: u32,
}

impl MassAnalysis {
    /// Covers all subset sums and one self-combo add-back. Each represented
    /// opponent hand must occur at most once per sum.
    #[inline]
    pub(crate) fn is_f64_exact(self) -> bool {
        self.bound <= 53
    }

    /// If B = D + 24 + bit_length(N), total plus self is strictly below 2^B:
    /// each term is below 2^(D+24), and N+1 <= 2^bit_length(N).
    /// All subsets, cards and rank groups must share the returned scale.
    #[inline]
    pub(crate) fn integer(self) -> (MassWidth, MassScale) {
        let width = if self.bound <= 64 {
            MassWidth::U64
        } else if self.bound <= 128 {
            MassWidth::U128
        } else {
            MassWidth::Wide
        };
        (width, MassScale::new(self.min_shift))
    }
}

/// Analyze the unchanged conservative B bound once, validating every term
/// even after the prefix already exceeds a narrower accumulator's bound.
pub(crate) fn analyze_mass(reach: &[f32]) -> MassAnalysis {
    assert!(reach.len() <= MAX_HANDS, "too many reach terms");
    let mut count = 0usize;
    let mut minimum = u32::MAX;
    let mut maximum = 0u32;
    for &value in reach {
        let bits = value.to_bits();
        // Normalize both signed zeros; every other negative or nonfinite
        // encoding exceeds f32::MAX's bits and is rejected after the scan.
        let bits = if bits << 1 == 0 { 0 } else { bits };
        count += usize::from(bits != 0);
        minimum = minimum.min(if bits == 0 { u32::MAX } else { bits });
        maximum = maximum.max(bits);
    }
    assert!(maximum <= 0x7f7f_ffff, "invalid reach mass");
    if count == 0 {
        return MassAnalysis {
            bound: 0,
            min_shift: 0,
        };
    }
    // Positive finite binary32 encodings and their exponents have the same
    // order, so only the two extrema need exponent extraction.
    let minimum = ((minimum >> 23) & 0xff).saturating_sub(1);
    let maximum = ((maximum >> 23) & 0xff).saturating_sub(1);
    // bit_length(N) == ceil(log2(N+1)); u32 avoids wrapping at B=288.
    let count_bits = usize::BITS - count.leading_zeros();
    MassAnalysis {
        bound: maximum - minimum + 24 + count_bits,
        min_shift: minimum,
    }
}

/// A sufficient, conservative condition for the existing f64 mass sweep
/// to be exact. It covers all subset sums and one self-combo add-back.
/// The caller must use each represented opponent hand at most once per sum.
#[inline]
pub(crate) fn f64_mass_is_exact(reach: &[f32]) -> bool {
    analyze_mass(reach).is_f64_exact()
}

/// One common quantum for every sum in a sweep, including self add-back.
/// Construction is restricted to validated reach classification.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MassScale {
    min_shift: u32,
    factor: f64,
}

impl MassScale {
    /// The fixed binary32 quantum; wide accumulators ignore the scale.
    pub(crate) const WIDE: Self = Self {
        min_shift: 0,
        factor: f64::from_bits(874u64 << 52),
    };

    fn new(min_shift: u32) -> Self {
        assert!(min_shift <= 253, "invalid mass scale");
        Self {
            min_shift,
            factor: f64::from_bits(u64::from(min_shift + 874) << 52),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MassWidth {
    U64,
    U128,
    Wide,
}

/// Classify reach when the caller has not retained its f64 gate analysis.
#[cfg(test)]
#[inline]
pub(crate) fn classify_integer_mass(reach: &[f32]) -> (MassWidth, MassScale) {
    analyze_mass(reach).integer()
}

/// Exact nonnegative mass arithmetic, selected once per sweep.
pub(crate) trait ExactMass: Copy {
    const ZERO: Self;
    fn from_f32_scaled(value: f32, scale: MassScale) -> Self;
    fn add(self, other: Self) -> Self;
    fn sub(self, other: Self) -> Self;
    fn to_f64_scaled(self, scale: MassScale) -> f64;
}

/// Decode a represented value without discarding any low bits.
#[inline]
fn scaled_significand(value: f32, scale: MassScale) -> (u32, u32) {
    assert!(value.is_finite() && value >= 0.0, "invalid reach mass");
    if value == 0.0 {
        return (0, 0);
    }
    let bits = value.to_bits();
    let exponent = (bits >> 23) & 0xff;
    let fraction = bits & FRACTION_MASK;
    let (significand, shift) = if exponent == 0 {
        (fraction, 0)
    } else {
        (fraction | (1 << 23), exponent - 1)
    };
    let delta = shift
        .checked_sub(scale.min_shift)
        .expect("reach below mass scale");
    (significand, delta)
}

macro_rules! impl_integer_mass {
    ($word:ty) => {
        impl ExactMass for $word {
            const ZERO: Self = 0;

            #[inline]
            fn from_f32_scaled(value: f32, scale: MassScale) -> Self {
                let (significand, delta) = scaled_significand(value, scale);
                let word = Self::from(significand);
                // checked_shl alone would allow significant bits to fall off.
                assert!(
                    delta < Self::BITS && word <= Self::MAX >> delta,
                    "mass conversion overflow"
                );
                word << delta
            }

            #[inline]
            fn add(self, other: Self) -> Self {
                self.checked_add(other).expect("mass addition overflow")
            }

            #[inline]
            fn sub(self, other: Self) -> Self {
                self.checked_sub(other).expect("mass subtraction underflow")
            }

            #[inline]
            fn to_f64_scaled(self, scale: MassScale) -> f64 {
                // Integer-to-f64 rounds nearest-even. Multiplication by this
                // exact power of two only changes its exponent: all possible
                // nonzero values remain normal and finite in binary64.
                self as f64 * scale.factor
            }
        }
    };
}

impl_integer_mass!(u64);
impl_integer_mass!(u128);

impl ExactMass for Mass {
    const ZERO: Self = Mass::ZERO;

    #[inline]
    fn from_f32_scaled(value: f32, _scale: MassScale) -> Self {
        Mass::from_f32(value)
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        Mass::add(self, other)
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        Mass::sub(self, other)
    }

    #[inline]
    fn to_f64_scaled(self, _scale: MassScale) -> f64 {
        Mass::to_f64(self)
    }
}

#[cfg(test)]
#[path = "mass_tests.rs"]
mod tests;
