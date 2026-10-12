use super::*;

/// Plain `f32` backend: two flat arenas.
pub struct F32Storage {
    pub(super) regrets_released: bool,
    pub(super) regrets: Vec<f32>,
    pub(super) strategy_sum: Vec<f32>,
}

// Shared op bodies, parameterized by the arena slice and the *local* offset
// within it. `F32Storage` calls these with `r.offset` directly (global
// offsets); `F32View` rebases first. Keeping the logic here means the two
// backends can't drift apart.

fn regret_matching_impl(regrets: &[f32], offset: usize, r: StorageRef, out: &mut [f32]) {
    normalize_columns(&regrets[offset..offset + r.len()], r, out);
}

fn update_regrets_impl(
    regrets: &mut [f32],
    offset: usize,
    r: StorageRef,
    inst: &[f32],
    d: &Discounts,
) {
    let slice = &mut regrets[offset..offset + r.len()];
    debug_assert_eq!(inst.len(), slice.len());
    let (pos, neg) = (d.pos as f32, d.neg as f32);
    for (regret, &delta) in slice.iter_mut().zip(inst) {
        let factor = if *regret > 0.0 { pos } else { neg };
        let mut updated = *regret * factor + delta;
        if d.floor_neg && updated < 0.0 {
            updated = 0.0;
        }
        *regret = updated;
    }
}

pub(super) fn accumulate_strategy_impl(
    strategy_sum: &mut [f32],
    offset: usize,
    r: StorageRef,
    weighted: &[f32],
    d: &Discounts,
) {
    let slice = &mut strategy_sum[offset..offset + r.len()];
    debug_assert_eq!(weighted.len(), slice.len());
    let avg = d.avg as f32;
    if d.reset_avg {
        slice.copy_from_slice(weighted);
    } else {
        for (sum, &w) in slice.iter_mut().zip(weighted) {
            *sum = *sum * avg + w;
        }
    }
}

pub(super) fn average_strategy_impl(
    strategy_sum: &[f32],
    offset: usize,
    r: StorageRef,
    out: &mut [f32],
) {
    normalize_columns(&strategy_sum[offset..offset + r.len()], r, out);
}

impl StorageOps for F32Storage {
    fn regret_matching(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        self.assert_regrets_available();
        regret_matching_impl(&self.regrets, r.offset, r, out);
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

    fn update_regrets(&mut self, r: StorageRef, _ref_idx: u32, inst: &[f32], d: &Discounts) {
        self.assert_regrets_available();
        update_regrets_impl(&mut self.regrets, r.offset, r, inst, d);
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

impl Storage for F32Storage {
    type View<'a> = F32View<'a>;

    fn release_regrets(&mut self) {
        self.regrets = Vec::new();
        self.regrets_released = true;
    }

    fn regrets_released(&self) -> bool {
        self.regrets_released
    }

    fn arrays(&self) -> StorageArrays<'_> {
        self.assert_regrets_available();
        StorageArrays::F32 {
            regrets: &self.regrets,
            strategy_sum: &self.strategy_sum,
        }
    }
    fn arrays_mut(&mut self) -> StorageArraysMut<'_> {
        self.assert_regrets_available();
        StorageArraysMut::F32 {
            regrets: &mut self.regrets,
            strategy_sum: &mut self.strategy_sum,
        }
    }

    fn new(len: usize, _num_refs: usize) -> Self {
        let mut regrets = vec![0.0; len];
        let mut strategy_sum = vec![0.0; len];
        prefault_zeroed(&mut regrets);
        prefault_zeroed(&mut strategy_sum);
        F32Storage {
            regrets_released: false,
            regrets,
            strategy_sum,
        }
    }

    fn view_mut(&mut self) -> F32View<'_> {
        self.assert_regrets_available();
        F32View {
            regrets: &mut self.regrets,
            strategy_sum: &mut self.strategy_sum,
            base: 0,
        }
    }

    fn bytes_for(len: usize, _num_refs: usize) -> u64 {
        2 * len as u64 * 4
    }

    fn state(&self) -> StorageState {
        self.assert_regrets_available();
        StorageState::F32 {
            regrets: self.regrets.clone(),
            strategy_sum: self.strategy_sum.clone(),
        }
    }

    fn restore_state(&mut self, state: StorageState) -> Result<(), StateMismatch> {
        self.assert_regrets_available();
        match state {
            StorageState::F32 {
                regrets,
                strategy_sum,
            } => {
                check_len(self.regrets.len(), regrets.len())?;
                check_len(self.strategy_sum.len(), strategy_sum.len())?;
                self.regrets = regrets;
                self.strategy_sum = strategy_sum;
                Ok(())
            }
            _ => Err(StateMismatch::WrongVariant),
        }
    }
}

/// A borrowed range of an [`F32Storage`]'s two arenas, `[base, base +
/// regrets.len())` in global element coordinates.
pub struct F32View<'a> {
    pub(super) regrets: &'a mut [f32],
    pub(super) strategy_sum: &'a mut [f32],
    base: usize,
}

impl<'a> F32View<'a> {
    fn local_offset(&self, r: StorageRef) -> usize {
        debug_assert!(r.offset >= self.base, "storage ref starts before view base");
        let local = r.offset - self.base;
        debug_assert!(
            local + r.len() <= self.regrets.len(),
            "storage ref exceeds view bounds"
        );
        local
    }
}

impl<'a> StorageOps for F32View<'a> {
    fn regret_matching(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        let local = self.local_offset(r);
        regret_matching_impl(&*self.regrets, local, r, out);
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

    fn update_regrets(&mut self, r: StorageRef, _ref_idx: u32, inst: &[f32], d: &Discounts) {
        let local = self.local_offset(r);
        update_regrets_impl(&mut *self.regrets, local, r, inst, d);
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

impl<'a> StorageView for F32View<'a> {
    fn split(&mut self, spans: &[StorageSpan]) -> Vec<Self> {
        let base = self.base;
        let mut regrets_rest = std::mem::take(&mut self.regrets);
        let mut strategy_rest = std::mem::take(&mut self.strategy_sum);

        let mut out = Vec::with_capacity(spans.len());
        let mut consumed = 0usize;
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
            out.push(F32View {
                regrets: r_span,
                strategy_sum: s_span,
                base: span.start,
            });
            regrets_rest = r_after;
            strategy_rest = s_after;
            consumed = span.end - base;
        }
        out
    }
}
