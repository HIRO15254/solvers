use super::*;

/// i16 regrets with the legacy per-node quantization; f32 strategy sums.
/// Split views own disjoint arenas/scales; updates lease worker-local scratch.
pub struct MixedStorage {
    pub(super) regrets_released: bool,
    pub(super) regrets: Vec<i16>,
    pub(super) strategy_sum: Vec<f32>,
    pub(super) regret_scales: Vec<f32>,
}

impl StorageOps for MixedStorage {
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
            inst.iter().copied(),
            d,
            &mut self.regret_scales[ref_idx as usize],
            &mut scratch.0,
        );
    }

    fn update_regrets_from_values(
        &mut self,
        r: StorageRef,
        ref_idx: u32,
        children: &[f32],
        values: &[f32],
        d: &Discounts,
    ) {
        self.assert_regrets_available();
        assert_eq!(children.len(), r.len());
        assert_eq!(values.len(), r.num_hands as usize);
        let mut scratch = QuantizedScratch::take();
        update_regrets_i16_impl(
            &mut self.regrets,
            r.offset,
            r,
            children
                .iter()
                .zip(values.iter().cycle())
                .map(|(&v, &cfv)| v - cfv),
            d,
            &mut self.regret_scales[ref_idx as usize],
            &mut scratch.0,
        );
    }

    fn accumulate_strategy(
        &mut self,
        r: StorageRef,
        _ref_idx: u32,
        weighted: &[f32],
        d: &Discounts,
    ) {
        accumulate_strategy_impl(&mut self.strategy_sum, r.offset, r, weighted, d);
    }

    fn average_strategy(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        average_strategy_impl(&self.strategy_sum, r.offset, r, out);
    }
}

impl Storage for MixedStorage {
    type View<'a> = MixedView<'a>;

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
        StorageArrays::Mixed {
            regrets: &self.regrets,
            strategy_sum: &self.strategy_sum,
            regret_scales: &self.regret_scales,
        }
    }
    fn arrays_mut(&mut self) -> StorageArraysMut<'_> {
        self.assert_regrets_available();
        StorageArraysMut::Mixed {
            regrets: &mut self.regrets,
            strategy_sum: &mut self.strategy_sum,
            regret_scales: &mut self.regret_scales,
        }
    }

    fn new(len: usize, num_refs: usize) -> Self {
        let mut regrets = vec![0i16; len];
        let mut strategy_sum = vec![0.0; len];
        let regret_scales = vec![1.0; num_refs];
        prefault_zeroed(&mut regrets);
        prefault_zeroed(&mut strategy_sum);
        MixedStorage {
            regrets_released: false,
            regrets,
            strategy_sum,
            regret_scales,
        }
    }

    fn view_mut(&mut self) -> MixedView<'_> {
        self.assert_regrets_available();
        MixedView {
            regrets: &mut self.regrets,
            strategy_sum: &mut self.strategy_sum,
            regret_scales: &mut self.regret_scales,
            base: 0,
            sref_base: 0,
        }
    }

    fn bytes_for(len: usize, num_refs: usize) -> u64 {
        len as u64 * 6 + num_refs as u64 * 4
    }

    fn state(&self) -> StorageState {
        self.assert_regrets_available();
        StorageState::Mixed {
            regrets: self.regrets.clone(),
            strategy_sum: self.strategy_sum.clone(),
            regret_scales: self.regret_scales.clone(),
        }
    }

    fn restore_state(&mut self, state: StorageState) -> Result<(), StateMismatch> {
        self.assert_regrets_available();
        match state {
            StorageState::Mixed {
                regrets,
                strategy_sum,
                regret_scales,
            } => {
                check_len(self.regrets.len(), regrets.len())?;
                check_len(self.strategy_sum.len(), strategy_sum.len())?;
                check_len(self.regret_scales.len(), regret_scales.len())?;
                self.regrets = regrets;
                self.strategy_sum = strategy_sum;
                self.regret_scales = regret_scales;
                Ok(())
            }
            _ => Err(StateMismatch::WrongVariant),
        }
    }
}

pub struct MixedView<'a> {
    pub(super) regrets: &'a mut [i16],
    pub(super) strategy_sum: &'a mut [f32],
    pub(super) regret_scales: &'a mut [f32],
    base: usize,
    sref_base: u32,
}

impl<'a> MixedView<'a> {
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

impl<'a> StorageOps for MixedView<'a> {
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
            inst.iter().copied(),
            d,
            &mut self.regret_scales[sidx],
            &mut scratch.0,
        );
    }

    fn update_regrets_from_values(
        &mut self,
        r: StorageRef,
        ref_idx: u32,
        children: &[f32],
        values: &[f32],
        d: &Discounts,
    ) {
        assert_eq!(children.len(), r.len());
        assert_eq!(values.len(), r.num_hands as usize);
        let local = self.local_offset(r);
        let sidx = self.local_ref(ref_idx);
        let mut scratch = QuantizedScratch::take();
        update_regrets_i16_impl(
            &mut *self.regrets,
            local,
            r,
            children
                .iter()
                .zip(values.iter().cycle())
                .map(|(&v, &cfv)| v - cfv),
            d,
            &mut self.regret_scales[sidx],
            &mut scratch.0,
        );
    }

    fn accumulate_strategy(
        &mut self,
        r: StorageRef,
        _ref_idx: u32,
        weighted: &[f32],
        d: &Discounts,
    ) {
        let local = self.local_offset(r);
        accumulate_strategy_impl(&mut *self.strategy_sum, local, r, weighted, d);
    }

    fn average_strategy(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        let local = self.local_offset(r);
        average_strategy_impl(&*self.strategy_sum, local, r, out);
    }
}

impl<'a> StorageView for MixedView<'a> {
    fn split(&mut self, spans: &[StorageSpan]) -> Vec<Self> {
        let base = self.base;
        let sref_base = self.sref_base;
        let mut regrets_rest = std::mem::take(&mut self.regrets);
        let mut strategy_rest = std::mem::take(&mut self.strategy_sum);
        let mut rscale_rest = std::mem::take(&mut self.regret_scales);

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
            let sref_len = (span.sref_end - span.sref_start) as usize;
            let (rscale_span, rscale_after) = rscale_r.split_at_mut(sref_len);

            out.push(MixedView {
                regrets: r_span,
                strategy_sum: s_span,
                regret_scales: rscale_span,
                base: span.start,
                sref_base: span.sref_start,
            });
            regrets_rest = r_after;
            strategy_rest = s_after;
            rscale_rest = rscale_after;
            consumed = span.end - base;
            sref_consumed = span.sref_end - sref_base;
        }
        out
    }
}
