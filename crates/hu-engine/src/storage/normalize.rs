use super::StorageRef;

pub(super) trait Positive: Copy {
    fn positive_f64(self) -> f64;
    fn positive_f32(self) -> f32;
}
impl Positive for f32 {
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
    const BLOCK: usize = 256;
    let uniform = 1.0 / num_actions as f32;
    for first in (0..num_hands).step_by(BLOCK) {
        let len = BLOCK.min(num_hands - first);
        let mut totals = [0.0f32; BLOCK];
        for a in 0..num_actions {
            let row = &data[a * num_hands + first..a * num_hands + first + len];
            for (total, &value) in totals[..len].iter_mut().zip(row) {
                *total += value.positive_f32();
            }
        }
        let mut inverses = [0.0f32; BLOCK];
        for (inverse, &total) in inverses[..len].iter_mut().zip(&totals[..len]) {
            *inverse = if total > 0.0 { total.recip() } else { 0.0 };
        }
        for a in 0..num_actions {
            let row = &data[a * num_hands + first..a * num_hands + first + len];
            let dst = &mut out[a * num_hands + first..a * num_hands + first + len];
            for (((dst, &value), &total), &inverse) in dst
                .iter_mut()
                .zip(row)
                .zip(&totals[..len])
                .zip(&inverses[..len])
            {
                *dst = if total > 0.0 {
                    value.positive_f32() * inverse
                } else {
                    uniform
                };
            }
        }
    }
}
