use super::*;

// --- I16Storage: quantized backend -----------------------------------

/// Re-quantizes `vals` into `out_q` (same length), choosing a fresh scale
/// with headroom below `i16::MAX` so future in-place updates rarely
/// saturate, and returns that scale. An all-zero block gets scale `1.0`
/// (arbitrary — dequantizing zeros is scale-invariant) rather than a
/// division by zero.
pub(super) fn quantize_block(vals: &[f32], max_abs: f32, out_q: &mut [i16]) -> f32 {
    debug_assert_eq!(vals.len(), out_q.len());
    if max_abs == 0.0 {
        out_q.fill(0);
        return 1.0;
    }
    let scale = max_abs / 32_000.0;
    // Use a double-precision reciprocal/product to avoid creating artificial
    // half-integer ties by rounding the product to f32 before rounding to i16.
    let inverse = f64::from(scale).recip();
    if !inverse.is_finite() || inverse > f64::from(f32::MAX) {
        // Very small subnormal blocks can underflow the scale or overflow
        // its reciprocal. Preserve saturating conversion in that rare case.
        for (o, &v) in out_q.iter_mut().zip(vals) {
            *o = (v / scale).round_ties_even() as i16;
        }
        return scale;
    }
    for (dst, &src) in out_q.iter_mut().zip(vals) {
        // The block maximum bounds the rounded result to +/-32000, well
        // inside i16. The helper avoids saturating conversion checks so LLVM
        // can use packed float-to-i32 conversion followed by narrowing.
        *dst = quantize_value(src, inverse);
    }
    scale
}

#[inline]
fn quantize_value(value: f32, inverse: f64) -> i16 {
    let rounded = (f64::from(value) * inverse).round_ties_even();
    let rounded = if rounded.is_nan() { 0.0 } else { rounded };
    // SAFETY: only called by quantize_block with a finite reciprocal of
    // max_abs/32000, no larger than f32::MAX. For a finite maximum the
    // magnitude is <=32001 (allowing
    // rounding error), safely in i32's range. An infinite maximum gives
    // reciprocal zero: finite values become zero, infinities/NaNs become
    // NaN, explicitly replaced by zero above. Underflow/overflow of the
    // reciprocal takes the saturating fallback before reaching this helper.
    unsafe { rounded.to_int_unchecked::<i32>() as i16 }
}

/// Fuse dequantization/update and max tracking. Independent lane maxima
/// allow SIMD without reassociating any of the per-element update math.
fn update_i16_block(
    data: &mut [i16],
    input: &[f32],
    scratch: &mut Vec<f32>,
    update: impl Fn(i16, f32) -> f32,
) -> f32 {
    let len = data.len();
    debug_assert_eq!(len, input.len());
    if scratch.len() < len {
        scratch.resize(len, 0.0);
    }
    let scratch = &mut scratch[..len];
    let (dst, dst_tail) = scratch.as_chunks_mut::<8>();
    let (src, src_tail) = data.as_chunks::<8>();
    let (input, input_tail) = input.as_chunks::<8>();
    let mut maxima = [0.0f32; 8];
    for ((dst, src), input) in dst.iter_mut().zip(src).zip(input) {
        for lane in 0..8 {
            let value = update(src[lane], input[lane]);
            dst[lane] = value;
            let abs = value.abs();
            if abs > maxima[lane] {
                maxima[lane] = abs;
            }
        }
    }
    let mut max_abs = maxima.into_iter().fold(0.0f32, f32::max);
    for ((dst, &src), &input) in dst_tail.iter_mut().zip(src_tail).zip(input_tail) {
        let value = update(src, input);
        *dst = value;
        max_abs = max_abs.max(value.abs());
    }
    quantize_block(scratch, max_abs, data)
}

// Shared op bodies for the i16 backend, mirroring `*_impl` above. Unlike
// the f32 versions, `update_regrets`/`accumulate_strategy` need a scratch
// `f32` buffer for the fused dequantize/discount/add results, and a
// `&mut f32` to the node's single scale (see `I16Storage`/`I16View` doc
// comments for why the scratch buffer isn't allocated per call).

pub(super) fn regret_matching_i16_impl(
    regrets: &[i16],
    offset: usize,
    r: StorageRef,
    out: &mut [f32],
) {
    normalize_columns(&regrets[offset..offset + r.len()], r, out);
}

pub(super) fn update_regrets_i16_impl(
    regrets: &mut [i16],
    offset: usize,
    r: StorageRef,
    inst: &[f32],
    d: &Discounts,
    scale: &mut f32,
    scratch: &mut Vec<f32>,
) {
    let len = r.len();
    let slice = &mut regrets[offset..offset + len];
    debug_assert_eq!(inst.len(), len);
    let old_scale = *scale;
    let (pos, neg) = (d.pos as f32, d.neg as f32);
    let update = |raw: i16, delta| {
        let value = raw as f32 * old_scale;
        let factor = if value > 0.0 { pos } else { neg };
        value * factor + delta
    };
    *scale = if d.floor_neg {
        update_i16_block(slice, inst, scratch, |raw, delta| {
            let updated = update(raw, delta);
            if updated < 0.0 { 0.0 } else { updated }
        })
    } else {
        update_i16_block(slice, inst, scratch, update)
    };
}

pub(super) fn accumulate_strategy_i16_impl(
    strategy_sum: &mut [i16],
    offset: usize,
    r: StorageRef,
    weighted: &[f32],
    d: &Discounts,
    scale: &mut f32,
    scratch: &mut Vec<f32>,
) {
    let len = r.len();
    let slice = &mut strategy_sum[offset..offset + len];
    debug_assert_eq!(weighted.len(), len);
    let old_scale = *scale;
    let avg = d.avg as f32;
    if d.reset_avg {
        let (chunks, tail) = weighted.as_chunks::<8>();
        let mut maxima = [0.0f32; 8];
        for chunk in chunks {
            for lane in 0..8 {
                let abs = chunk[lane].abs();
                if abs > maxima[lane] {
                    maxima[lane] = abs;
                }
            }
        }
        let max_abs = tail
            .iter()
            .fold(maxima.into_iter().fold(0.0f32, f32::max), |m, v| {
                m.max(v.abs())
            });
        *scale = quantize_block(weighted, max_abs, slice);
    } else {
        *scale = update_i16_block(slice, weighted, scratch, |raw, w| {
            (raw as f32 * old_scale) * avg + w
        });
    }
}

pub(super) fn average_strategy_i16_impl(
    strategy_sum: &[i16],
    offset: usize,
    r: StorageRef,
    out: &mut [f32],
) {
    normalize_columns(&strategy_sum[offset..offset + r.len()], r, out);
}

/// Quantized `i16` backend: two flat `i16` arenas plus one `f32` scale per
/// action node (`storage_refs` entry) per arena. `value` is approximately
/// `stored_i16 as f32` times `scale`, with `scale` re-chosen on every write
/// to keep the block's largest-magnitude element at `32000` (headroom below
/// `i16::MAX`, so a slightly larger successor value next iteration doesn't
/// immediately saturate).
///
/// `regret_matching`/`average_strategy` only need the current sign/ratio of
/// values within a node, which a shared per-node scale doesn't change, so
/// they normalize the raw `i16`s without ever dequantizing (see
/// `normalize_columns`). `update_regrets`/`accumulate_strategy` apply
/// the same discount math `F32Storage` does, but on a dequantized `f32`
/// copy of the node's block, then re-quantize.
///
/// Update scratch is leased from the current worker's reusable pool for each
/// write. It survives passes and view splits, grows only for larger blocks,
/// and returns to the pool on scope exit. No borrow of the pool crosses an
/// update, so reentrant work can obtain a separate buffer safely.
pub struct I16Storage {
    pub(super) regrets_released: bool,
    pub(super) regrets: Vec<i16>,
    pub(super) strategy_sum: Vec<i16>,
    pub(super) regret_scales: Vec<f32>,
    pub(super) strategy_scales: Vec<f32>,
}

impl StorageOps for I16Storage {
    fn regret_matching(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        self.assert_regrets_available();
        regret_matching_i16_impl(&self.regrets, r.offset, r, out);
    }
    fn regret_matching_cfr(
        &self,
        r: StorageRef,
        ref_idx: u32,
        out: &mut [f32],
        norm: CfrPrecision,
    ) {
        self.assert_regrets_available();
        match norm {
            CfrPrecision::F64 => self.regret_matching(r, ref_idx, out),
            CfrPrecision::F32 => {
                normalize_columns_f32(&self.regrets[r.offset..r.offset + r.len()], r, out);
            }
        }
    }

    fn update_regrets(&mut self, r: StorageRef, ref_idx: u32, inst: &[f32], d: &Discounts) {
        self.assert_regrets_available();
        let mut scratch = QuantizedScratch::take();
        update_regrets_i16_impl(
            &mut self.regrets,
            r.offset,
            r,
            inst,
            d,
            &mut self.regret_scales[ref_idx as usize],
            &mut scratch.0,
        );
    }

    fn accumulate_strategy(
        &mut self,
        r: StorageRef,
        ref_idx: u32,
        weighted: &[f32],
        d: &Discounts,
    ) {
        let mut scratch = QuantizedScratch::take();
        accumulate_strategy_i16_impl(
            &mut self.strategy_sum,
            r.offset,
            r,
            weighted,
            d,
            &mut self.strategy_scales[ref_idx as usize],
            &mut scratch.0,
        );
    }

    fn average_strategy(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        average_strategy_i16_impl(&self.strategy_sum, r.offset, r, out);
    }
}

impl Storage for I16Storage {
    type View<'a> = I16View<'a>;

    fn release_regrets(&mut self) {
        self.regrets = Vec::new();
        self.regret_scales = Vec::new();
        self.regrets_released = true;
    }

    fn regrets_released(&self) -> bool {
        self.regrets_released
    }

    fn arrays(&self) -> StorageArrays<'_> {
        self.assert_regrets_available();
        StorageArrays::I16 {
            regrets: &self.regrets,
            strategy_sum: &self.strategy_sum,
            regret_scales: &self.regret_scales,
            strategy_scales: &self.strategy_scales,
        }
    }
    fn arrays_mut(&mut self) -> StorageArraysMut<'_> {
        self.assert_regrets_available();
        StorageArraysMut::I16 {
            regrets: &mut self.regrets,
            strategy_sum: &mut self.strategy_sum,
            regret_scales: &mut self.regret_scales,
            strategy_scales: &mut self.strategy_scales,
        }
    }

    fn new(len: usize, num_refs: usize) -> Self {
        let mut regrets = vec![0i16; len];
        let mut strategy_sum = vec![0i16; len];
        let regret_scales = vec![1.0; num_refs];
        let strategy_scales = vec![1.0; num_refs];
        prefault_zeroed(&mut regrets);
        prefault_zeroed(&mut strategy_sum);
        I16Storage {
            regrets_released: false,
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        }
    }

    fn view_mut(&mut self) -> I16View<'_> {
        self.assert_regrets_available();
        I16View {
            regrets: &mut self.regrets,
            strategy_sum: &mut self.strategy_sum,
            regret_scales: &mut self.regret_scales,
            strategy_scales: &mut self.strategy_scales,
            base: 0,
            sref_base: 0,
        }
    }

    fn bytes_for(len: usize, num_refs: usize) -> u64 {
        2 * len as u64 * 2 + num_refs as u64 * 2 * 4
    }

    fn state(&self) -> StorageState {
        self.assert_regrets_available();
        StorageState::I16 {
            regrets: self.regrets.clone(),
            strategy_sum: self.strategy_sum.clone(),
            regret_scales: self.regret_scales.clone(),
            strategy_scales: self.strategy_scales.clone(),
        }
    }

    fn restore_state(&mut self, state: StorageState) -> Result<(), StateMismatch> {
        self.assert_regrets_available();
        match state {
            StorageState::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            } => {
                check_len(self.regrets.len(), regrets.len())?;
                check_len(self.strategy_sum.len(), strategy_sum.len())?;
                check_len(self.regret_scales.len(), regret_scales.len())?;
                check_len(self.strategy_scales.len(), strategy_scales.len())?;
                self.regrets = regrets;
                self.strategy_sum = strategy_sum;
                self.regret_scales = regret_scales;
                self.strategy_scales = strategy_scales;
                Ok(())
            }
            _ => Err(StateMismatch::WrongVariant),
        }
    }
}

/// A borrowed range of an [`I16Storage`]'s arenas, `[base, base +
/// regrets.len())` in global element coordinates, with a matching
/// `[sref_base, sref_base + regret_scales.len())` range of per-node scales.
pub struct I16View<'a> {
    pub(super) regrets: &'a mut [i16],
    pub(super) strategy_sum: &'a mut [i16],
    pub(super) regret_scales: &'a mut [f32],
    pub(super) strategy_scales: &'a mut [f32],
    base: usize,
    sref_base: u32,
}

impl<'a> I16View<'a> {
    fn local_offset(&self, r: StorageRef) -> usize {
        debug_assert!(r.offset >= self.base, "storage ref starts before view base");
        let local = r.offset - self.base;
        debug_assert!(
            local + r.len() <= self.regrets.len(),
            "storage ref exceeds view bounds"
        );
        local
    }

    fn local_ref(&self, ref_idx: u32) -> usize {
        debug_assert!(
            ref_idx >= self.sref_base,
            "storage ref index starts before view sref base"
        );
        let local = (ref_idx - self.sref_base) as usize;
        debug_assert!(
            local < self.regret_scales.len(),
            "storage ref index exceeds view sref bounds"
        );
        local
    }
}

impl<'a> StorageOps for I16View<'a> {
    fn regret_matching(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        let local = self.local_offset(r);
        regret_matching_i16_impl(&*self.regrets, local, r, out);
    }
    fn regret_matching_cfr(
        &self,
        r: StorageRef,
        ref_idx: u32,
        out: &mut [f32],
        norm: CfrPrecision,
    ) {
        match norm {
            CfrPrecision::F64 => self.regret_matching(r, ref_idx, out),
            CfrPrecision::F32 => {
                let local = self.local_offset(r);
                normalize_columns_f32(&self.regrets[local..local + r.len()], r, out);
            }
        }
    }

    fn update_regrets(&mut self, r: StorageRef, ref_idx: u32, inst: &[f32], d: &Discounts) {
        let local = self.local_offset(r);
        let sidx = self.local_ref(ref_idx);
        let mut scratch = QuantizedScratch::take();
        update_regrets_i16_impl(
            &mut *self.regrets,
            local,
            r,
            inst,
            d,
            &mut self.regret_scales[sidx],
            &mut scratch.0,
        );
    }

    fn accumulate_strategy(
        &mut self,
        r: StorageRef,
        ref_idx: u32,
        weighted: &[f32],
        d: &Discounts,
    ) {
        let local = self.local_offset(r);
        let sidx = self.local_ref(ref_idx);
        let mut scratch = QuantizedScratch::take();
        accumulate_strategy_i16_impl(
            &mut *self.strategy_sum,
            local,
            r,
            weighted,
            d,
            &mut self.strategy_scales[sidx],
            &mut scratch.0,
        );
    }

    fn average_strategy(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        let local = self.local_offset(r);
        average_strategy_i16_impl(&*self.strategy_sum, local, r, out);
    }
}

impl<'a> StorageView for I16View<'a> {
    fn split(&mut self, spans: &[StorageSpan]) -> Vec<Self> {
        let base = self.base;
        let sref_base = self.sref_base;
        let mut regrets_rest = std::mem::take(&mut self.regrets);
        let mut strategy_rest = std::mem::take(&mut self.strategy_sum);
        let mut rscale_rest = std::mem::take(&mut self.regret_scales);
        let mut sscale_rest = std::mem::take(&mut self.strategy_scales);

        let mut out = Vec::with_capacity(spans.len());
        let mut consumed = 0usize;
        let mut sref_consumed = 0u32;
        for span in spans {
            debug_assert!(
                span.start >= base + consumed && span.end >= span.start,
                "storage spans must be ascending, disjoint, and within view bounds"
            );
            let gap = span.start - (base + consumed);
            let (_, r_rest) = regrets_rest.split_at_mut(gap);
            let (_, s_rest) = strategy_rest.split_at_mut(gap);
            let len = span.end - span.start;
            let (r_span, r_after) = r_rest.split_at_mut(len);
            let (s_span, s_after) = s_rest.split_at_mut(len);

            debug_assert!(
                span.sref_start >= sref_base + sref_consumed && span.sref_end >= span.sref_start,
                "storage spans must carry ascending, disjoint sref ranges within view bounds"
            );
            let sref_gap = (span.sref_start - (sref_base + sref_consumed)) as usize;
            let (_, rscale_r) = rscale_rest.split_at_mut(sref_gap);
            let (_, sscale_r) = sscale_rest.split_at_mut(sref_gap);
            let sref_len = (span.sref_end - span.sref_start) as usize;
            let (rscale_span, rscale_after) = rscale_r.split_at_mut(sref_len);
            let (sscale_span, sscale_after) = sscale_r.split_at_mut(sref_len);

            out.push(I16View {
                regrets: r_span,
                strategy_sum: s_span,
                regret_scales: rscale_span,
                strategy_scales: sscale_span,
                base: span.start,
                sref_base: span.sref_start,
            });
            regrets_rest = r_after;
            strategy_rest = s_after;
            rscale_rest = rscale_after;
            sscale_rest = sscale_after;
            consumed = span.end - base;
            sref_consumed = span.sref_end - sref_base;
        }
        out
    }
}
