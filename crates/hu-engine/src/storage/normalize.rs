use super::StorageRef;

pub(super) trait Positive: Copy {
    const CACHE_POSITIVE: bool;
    fn positive_f64(self) -> f64;
    fn positive_f32(self) -> f32;
}
impl Positive for f32 {
    const CACHE_POSITIVE: bool = false;

    #[inline]
    fn positive_f64(self) -> f64 {
        self.max(0.0) as f64
    }
    #[inline]
    fn positive_f32(self) -> f32 {
        self.max(0.0)
    }
}
impl Positive for i16 {
    const CACHE_POSITIVE: bool = true;

    #[inline]
    fn positive_f64(self) -> f64 {
        self.max(0) as f64
    }
    #[inline]
    fn positive_f32(self) -> f32 {
        self.max(0) as f32
    }
}

pub(super) fn normalize_columns<T: Positive>(data: &[T], r: StorageRef, out: &mut [f32]) {
    let (num_actions, num_hands) = (r.num_actions as usize, r.num_hands as usize);
    debug_assert_eq!(out.len(), r.len());
    // 2 KiB on the stack, independent of hand count; each hand still sums
    // action 0, 1, ... in f64 and divides (never multiplies a reciprocal).
    const BLOCK: usize = 256;
    let uniform = 1.0 / num_actions as f32;
    for first in (0..num_hands).step_by(BLOCK) {
        let len = BLOCK.min(num_hands - first);
        let mut totals = [0.0f64; BLOCK];
        for a in 0..num_actions {
            let row = &data[a * num_hands + first..a * num_hands + first + len];
            for (total, &value) in totals[..len].iter_mut().zip(row) {
                *total += value.positive_f64();
            }
        }
        for a in 0..num_actions {
            let row = &data[a * num_hands + first..a * num_hands + first + len];
            let dst = &mut out[a * num_hands + first..a * num_hands + first + len];
            for ((dst, &value), &total) in dst.iter_mut().zip(row).zip(&totals[..len]) {
                *dst = if total > 0.0 {
                    (value.positive_f64() / total) as f32
                } else {
                    uniform
                };
            }
        }
    }
}

pub(super) fn normalize_columns_f32<T: Positive>(data: &[T], r: StorageRef, out: &mut [f32]) {
    let (num_actions, num_hands) = (r.num_actions as usize, r.num_hands as usize);
    debug_assert_eq!(out.len(), r.len());
    // CFR f32: f32 positive sums, then one reciprocal per hand.
    // i16 conversion is expensive in both passes; cache positive values in
    // the output block. For f32, the extra writes are slower than reloading.
    const BLOCK: usize = 256;
    let uniform = 1.0 / num_actions as f32;
    for first in (0..num_hands).step_by(BLOCK) {
        let len = BLOCK.min(num_hands - first);
        let mut totals = [0.0f32; BLOCK];
        for a in 0..num_actions {
            let row = &data[a * num_hands + first..a * num_hands + first + len];
            if T::CACHE_POSITIVE {
                let dst = &mut out[a * num_hands + first..a * num_hands + first + len];
                for ((total, dst), &value) in totals[..len].iter_mut().zip(dst).zip(row) {
                    let positive = value.positive_f32();
                    *dst = positive;
                    *total += positive;
                }
            } else {
                for (total, &value) in totals[..len].iter_mut().zip(row) {
                    *total += value.positive_f32();
                }
            }
        }
        // Reuse totals: positive totals have nonnegative reciprocals (including
        // zero for infinity); -1 marks a uniform column.
        for total in &mut totals[..len] {
            *total = if *total > 0.0 { total.recip() } else { -1.0 };
        }
        for a in 0..num_actions {
            let dst = &mut out[a * num_hands + first..a * num_hands + first + len];
            if T::CACHE_POSITIVE {
                for (dst, &inverse) in dst.iter_mut().zip(&totals[..len]) {
                    *dst = if inverse >= 0.0 {
                        *dst * inverse
                    } else {
                        uniform
                    };
                }
            } else {
                let row = &data[a * num_hands + first..a * num_hands + first + len];
                for ((dst, &value), &inverse) in dst.iter_mut().zip(row).zip(&totals[..len]) {
                    *dst = if inverse >= 0.0 {
                        value.positive_f32() * inverse
                    } else {
                        uniform
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_normalization_preserves_extreme_column_bits() {
        let r = StorageRef {
            offset: 0,
            num_actions: 2,
            num_hands: 5,
            index: 0,
        };
        let data = [
            0.0,
            -0.0,
            f32::INFINITY,
            f32::from_bits(1),
            f32::NAN,
            -1.0,
            0.0,
            1.0,
            0.0,
            2.0,
        ];
        let mut out = [0.0; 10];
        normalize_columns_f32(&data, r, &mut out);
        for hand in 0..5 {
            let total = 0.0 + data[hand].max(0.0) + data[5 + hand].max(0.0);
            let inverse = if total > 0.0 { total.recip() } else { 0.0 };
            for action in 0..2 {
                let expected = if total > 0.0 {
                    data[action * 5 + hand].max(0.0) * inverse
                } else {
                    0.5
                };
                let actual = out[action * 5 + hand];
                if expected.is_nan() {
                    assert!(actual.is_nan());
                } else {
                    assert_eq!(actual.to_bits(), expected.to_bits());
                }
            }
        }
    }
}
