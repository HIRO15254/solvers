use crate::schedule::Discounts;

/// Location of one action node's data inside a storage buffer. Layout is
/// action-major: element `(a, h)` lives at `offset + a * num_hands + h`.
#[derive(Clone, Copy, Debug)]
pub struct StorageRef {
    pub offset: usize,
    pub num_actions: u16,
    pub num_hands: u32,
    /// Position of this ref in [`crate::tree::PublicTree::storage_refs`].
    pub index: u32,
}

impl StorageRef {
    pub fn len(&self) -> usize {
        self.num_actions as usize * self.num_hands as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A contiguous subtree's storage footprint: the element range `[start,
/// end)` shared by both flat arenas, plus the `storage_refs` index range
/// `[sref_start, sref_end)` the subtree owns. The latter isn't needed by
/// `F32Storage` but lets a future `I16Storage` locate its per-node
/// quantization scales without re-walking the tree.
#[derive(Clone, Copy, Debug, Default)]
pub struct StorageSpan {
    pub start: usize,
    pub end: usize,
    pub sref_start: u32,
    pub sref_end: u32,
}

/// The four element-level operations shared by a full [`Storage`] backend
/// and any [`StorageView`] split from it.
pub trait StorageOps {
    /// Writes the regret-matching (current) strategy into `out` (`A*H`,
    /// action-major). Hands whose positive regrets sum to zero get the
    /// uniform strategy.
    fn regret_matching(&self, r: StorageRef, out: &mut [f32]);

    /// Applies pre-add discounting to stored regrets, then adds `inst`.
    fn update_regrets(&mut self, r: StorageRef, inst: &[f32], d: &Discounts);

    /// Applies pre-add discounting (or reset) to the cumulative strategy,
    /// then adds the reach-weighted current strategy `weighted`.
    fn accumulate_strategy(&mut self, r: StorageRef, weighted: &[f32], d: &Discounts);

    /// Writes the normalized average strategy into `out`. Hands never
    /// reached get the uniform strategy.
    fn average_strategy(&self, r: StorageRef, out: &mut [f32]);
}

/// Backend holding cumulative regrets and the cumulative (average) strategy.
///
/// The solver is generic over this trait (monomorphized — no `dyn` in the
/// hot loop) so that a quantized `I16Storage` backend can slot in later
/// without touching the traversal.
pub trait Storage: StorageOps + Send + Sync {
    type View<'a>: StorageView
    where
        Self: 'a;

    fn new(len: usize) -> Self;

    /// Borrows the whole backend as a single view covering every element,
    /// ready to be [`StorageView::split`] into per-subtree views.
    fn view_mut(&mut self) -> Self::View<'_>;
}

/// A borrowed, possibly-split slice of a [`Storage`] backend. Elements are
/// addressed by the same [`StorageRef`]s used against the full backend —
/// implementations rebase offsets internally.
pub trait StorageView: StorageOps + Send + Sized {
    /// Splits into one sub-view per span (ascending, disjoint, within this
    /// view). Consumes the view's contents: `self` is empty afterwards.
    fn split(&mut self, spans: &[StorageSpan]) -> Vec<Self>;
}

/// Plain `f32` backend: two flat arenas.
pub struct F32Storage {
    regrets: Vec<f32>,
    strategy_sum: Vec<f32>,
}

impl F32Storage {
    pub fn snapshot(&self) -> (Vec<f32>, Vec<f32>) {
        (self.regrets.clone(), self.strategy_sum.clone())
    }

    pub fn restore(&mut self, snapshot: (Vec<f32>, Vec<f32>)) {
        assert_eq!(snapshot.0.len(), self.regrets.len());
        assert_eq!(snapshot.1.len(), self.strategy_sum.len());
        self.regrets = snapshot.0;
        self.strategy_sum = snapshot.1;
    }
}

fn normalize_columns(data: &[f32], r: StorageRef, out: &mut [f32]) {
    let (num_actions, num_hands) = (r.num_actions as usize, r.num_hands as usize);
    debug_assert_eq!(out.len(), r.len());
    for h in 0..num_hands {
        let mut total = 0.0f64;
        for a in 0..num_actions {
            total += data[a * num_hands + h].max(0.0) as f64;
        }
        if total > 0.0 {
            for a in 0..num_actions {
                out[a * num_hands + h] = (data[a * num_hands + h].max(0.0) as f64 / total) as f32;
            }
        } else {
            let uniform = 1.0 / num_actions as f32;
            for a in 0..num_actions {
                out[a * num_hands + h] = uniform;
            }
        }
    }
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

fn accumulate_strategy_impl(
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

fn average_strategy_impl(strategy_sum: &[f32], offset: usize, r: StorageRef, out: &mut [f32]) {
    normalize_columns(&strategy_sum[offset..offset + r.len()], r, out);
}

impl StorageOps for F32Storage {
    fn regret_matching(&self, r: StorageRef, out: &mut [f32]) {
        regret_matching_impl(&self.regrets, r.offset, r, out);
    }

    fn update_regrets(&mut self, r: StorageRef, inst: &[f32], d: &Discounts) {
        update_regrets_impl(&mut self.regrets, r.offset, r, inst, d);
    }

    fn accumulate_strategy(&mut self, r: StorageRef, weighted: &[f32], d: &Discounts) {
        accumulate_strategy_impl(&mut self.strategy_sum, r.offset, r, weighted, d);
    }

    fn average_strategy(&self, r: StorageRef, out: &mut [f32]) {
        average_strategy_impl(&self.strategy_sum, r.offset, r, out);
    }
}

impl Storage for F32Storage {
    type View<'a> = F32View<'a>;

    fn new(len: usize) -> Self {
        F32Storage {
            regrets: vec![0.0; len],
            strategy_sum: vec![0.0; len],
        }
    }

    fn view_mut(&mut self) -> F32View<'_> {
        F32View {
            regrets: &mut self.regrets,
            strategy_sum: &mut self.strategy_sum,
            base: 0,
        }
    }
}

/// A borrowed range of an [`F32Storage`]'s two arenas, `[base, base +
/// regrets.len())` in global element coordinates.
pub struct F32View<'a> {
    regrets: &'a mut [f32],
    strategy_sum: &'a mut [f32],
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
    fn regret_matching(&self, r: StorageRef, out: &mut [f32]) {
        let local = self.local_offset(r);
        regret_matching_impl(&*self.regrets, local, r, out);
    }

    fn update_regrets(&mut self, r: StorageRef, inst: &[f32], d: &Discounts) {
        let local = self.local_offset(r);
        update_regrets_impl(&mut *self.regrets, local, r, inst, d);
    }

    fn accumulate_strategy(&mut self, r: StorageRef, weighted: &[f32], d: &Discounts) {
        let local = self.local_offset(r);
        accumulate_strategy_impl(&mut *self.strategy_sum, local, r, weighted, d);
    }

    fn average_strategy(&self, r: StorageRef, out: &mut [f32]) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn discounts() -> Discounts {
        Discounts {
            pos: 1.0,
            neg: 1.0,
            avg: 1.0,
            floor_neg: false,
            reset_avg: false,
        }
    }

    fn make_ref(offset: usize, num_actions: u16, num_hands: u32, index: u32) -> StorageRef {
        StorageRef {
            offset,
            num_actions,
            num_hands,
            index,
        }
    }

    /// Runs all four ops for a given ref against a `StorageOps` impl and
    /// returns the resulting (current, average) strategies.
    fn run_all_ops<O: StorageOps>(
        ops: &mut O,
        r: StorageRef,
        inst: &[f32],
    ) -> (Vec<f32>, Vec<f32>) {
        let d = discounts();
        ops.update_regrets(r, inst, &d);
        let mut sigma = vec![0.0; r.len()];
        ops.regret_matching(r, &mut sigma);
        ops.accumulate_strategy(r, &sigma, &d);
        let mut avg = vec![0.0; r.len()];
        ops.average_strategy(r, &mut avg);
        (sigma, avg)
    }

    #[test]
    fn view_split_matches_direct_ops() {
        let len = 100;

        // Ref inside span [10, 30): offset 12, 2 actions x 3 hands -> len 6.
        let r1 = make_ref(12, 2, 3, 0);
        // Ref inside span [50, 80): offset 60, 3 actions x 4 hands -> len 12.
        let r2 = make_ref(60, 3, 4, 1);

        let inst1: Vec<f32> = (0..r1.len()).map(|i| i as f32 * 0.5 + 1.0).collect();
        let inst2: Vec<f32> = (0..r2.len()).map(|i| i as f32 * 0.25 - 0.5).collect();

        let mut direct = F32Storage::new(len);
        let (sigma1_direct, avg1_direct) = run_all_ops(&mut direct, r1, &inst1);
        let (sigma2_direct, avg2_direct) = run_all_ops(&mut direct, r2, &inst2);

        let mut split_target = F32Storage::new(len);
        let mut view = split_target.view_mut();
        let spans = [
            StorageSpan {
                start: 10,
                end: 30,
                sref_start: 0,
                sref_end: 1,
            },
            StorageSpan {
                start: 50,
                end: 80,
                sref_start: 1,
                sref_end: 2,
            },
        ];
        let mut subviews = view.split(&spans);
        assert_eq!(subviews.len(), 2);

        // Parent view slices are empty after split.
        assert!(view.regrets.is_empty());
        assert!(view.strategy_sum.is_empty());

        let (sigma1_split, avg1_split) = run_all_ops(&mut subviews[0], r1, &inst1);
        let (sigma2_split, avg2_split) = run_all_ops(&mut subviews[1], r2, &inst2);

        assert_eq!(sigma1_direct, sigma1_split);
        assert_eq!(sigma2_direct, sigma2_split);
        assert_eq!(avg1_direct, avg1_split);
        assert_eq!(avg2_direct, avg2_split);
    }
}
