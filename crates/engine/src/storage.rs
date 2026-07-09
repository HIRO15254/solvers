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
///
/// `ref_idx` is the node's position in [`crate::tree::PublicTree::storage_refs`]
/// (equal to `r.index`, and to the action node's `node.aux`) — a quantized
/// backend needs it to locate its per-node scale state. `F32Storage`/`F32View`
/// ignore it entirely.
pub trait StorageOps {
    /// Writes the regret-matching (current) strategy into `out` (`A*H`,
    /// action-major). Hands whose positive regrets sum to zero get the
    /// uniform strategy.
    fn regret_matching(&self, r: StorageRef, ref_idx: u32, out: &mut [f32]);

    /// Applies pre-add discounting to stored regrets, then adds `inst`.
    fn update_regrets(&mut self, r: StorageRef, ref_idx: u32, inst: &[f32], d: &Discounts);

    /// Applies pre-add discounting (or reset) to the cumulative strategy,
    /// then adds the reach-weighted current strategy `weighted`.
    fn accumulate_strategy(&mut self, r: StorageRef, ref_idx: u32, weighted: &[f32], d: &Discounts);

    /// Writes the normalized average strategy into `out`. Hands never
    /// reached get the uniform strategy.
    fn average_strategy(&self, r: StorageRef, ref_idx: u32, out: &mut [f32]);
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

    /// `num_refs` is the number of action nodes (`PublicTree::storage_refs.len()`);
    /// `F32Storage` ignores it, `I16Storage` uses it to size its per-node
    /// scale arrays.
    fn new(len: usize, num_refs: usize) -> Self;

    /// Borrows the whole backend as a single view covering every element,
    /// ready to be [`StorageView::split`] into per-subtree views.
    fn view_mut(&mut self) -> Self::View<'_>;

    /// Bytes this backend allocates for a tree with `len` elements and
    /// `num_refs` action nodes — callable before allocation.
    fn bytes_for(len: usize, num_refs: usize) -> u64;

    /// Owned, backend-tagged snapshot of this backend's contents, for
    /// checkpointing.
    fn state(&self) -> StorageState;

    /// Restores this backend's contents from a snapshot previously produced
    /// by [`Storage::state`]. Fails if `state` is the wrong backend variant
    /// or its vector lengths don't match this backend's.
    fn restore_state(&mut self, state: StorageState) -> Result<(), StateMismatch>;
}

/// Owned, backend-tagged copy of a storage backend's contents, for
/// checkpointing. Serde impls are behind the "serde" feature.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StorageState {
    F32 {
        regrets: Vec<f32>,
        strategy_sum: Vec<f32>,
    },
    I16 {
        regrets: Vec<i16>,
        strategy_sum: Vec<i16>,
        regret_scales: Vec<f32>,
        strategy_scales: Vec<f32>,
    },
}

/// Error returned by [`Storage::restore_state`] when the supplied
/// [`StorageState`] doesn't match the backend: wrong enum variant, or a
/// vector whose length disagrees with the backend's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateMismatch {
    WrongVariant,
    WrongLength { expected: usize, actual: usize },
}

impl std::fmt::Display for StateMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateMismatch::WrongVariant => {
                write!(f, "storage state variant does not match this backend")
            }
            StateMismatch::WrongLength { expected, actual } => write!(
                f,
                "storage state length mismatch: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for StateMismatch {}

fn check_len(expected: usize, actual: usize) -> Result<(), StateMismatch> {
    if expected == actual {
        Ok(())
    } else {
        Err(StateMismatch::WrongLength { expected, actual })
    }
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
    fn regret_matching(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        regret_matching_impl(&self.regrets, r.offset, r, out);
    }

    fn update_regrets(&mut self, r: StorageRef, _ref_idx: u32, inst: &[f32], d: &Discounts) {
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

    fn new(len: usize, _num_refs: usize) -> Self {
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

    fn bytes_for(len: usize, _num_refs: usize) -> u64 {
        2 * len as u64 * 4
    }

    fn state(&self) -> StorageState {
        StorageState::F32 {
            regrets: self.regrets.clone(),
            strategy_sum: self.strategy_sum.clone(),
        }
    }

    fn restore_state(&mut self, state: StorageState) -> Result<(), StateMismatch> {
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
    fn regret_matching(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        let local = self.local_offset(r);
        regret_matching_impl(&*self.regrets, local, r, out);
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

// --- I16Storage: quantized backend -----------------------------------

/// Column normalization for a raw `i16` block. A node's positive/negative
/// split is invariant to a single shared per-node scale (every element of
/// the block — every action, every hand — is quantized against the same
/// scale), so `regret_matching`/`average_strategy` normalize the raw
/// quantized values directly instead of dequantizing first.
fn normalize_columns_i16(data: &[i16], r: StorageRef, out: &mut [f32]) {
    let (num_actions, num_hands) = (r.num_actions as usize, r.num_hands as usize);
    debug_assert_eq!(out.len(), r.len());
    for h in 0..num_hands {
        let mut total = 0.0f64;
        for a in 0..num_actions {
            total += data[a * num_hands + h].max(0) as f64;
        }
        if total > 0.0 {
            for a in 0..num_actions {
                out[a * num_hands + h] = (data[a * num_hands + h].max(0) as f64 / total) as f32;
            }
        } else {
            let uniform = 1.0 / num_actions as f32;
            for a in 0..num_actions {
                out[a * num_hands + h] = uniform;
            }
        }
    }
}

/// Dequantizes a block of `i16`s (`value = stored_i16 as f32 * scale`) into
/// `out`, which must already be sized to `q.len()`.
fn dequantize_block(q: &[i16], scale: f32, out: &mut [f32]) {
    for (o, &v) in out.iter_mut().zip(q) {
        *o = v as f32 * scale;
    }
}

/// Re-quantizes `vals` into `out_q` (same length), choosing a fresh scale
/// with headroom below `i16::MAX` so future in-place updates rarely
/// saturate, and returns that scale. An all-zero block gets scale `1.0`
/// (arbitrary — dequantizing zeros is scale-invariant) rather than a
/// division by zero.
fn quantize_block(vals: &[f32], out_q: &mut [i16]) -> f32 {
    debug_assert_eq!(vals.len(), out_q.len());
    let max_abs = vals.iter().fold(0.0f32, |acc, &v| acc.max(v.abs()));
    if max_abs == 0.0 {
        out_q.fill(0);
        return 1.0;
    }
    let scale = max_abs / 32_000.0;
    for (o, &v) in out_q.iter_mut().zip(vals) {
        *o = (v / scale).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    }
    scale
}

// Shared op bodies for the i16 backend, mirroring `*_impl` above. Unlike
// the f32 versions, `update_regrets`/`accumulate_strategy` need a scratch
// `f32` buffer to dequantize into before applying the discount math, and a
// `&mut f32` to the node's single scale (see `I16Storage`/`I16View` doc
// comments for why the scratch buffer isn't allocated per call).

fn regret_matching_i16_impl(regrets: &[i16], offset: usize, r: StorageRef, out: &mut [f32]) {
    normalize_columns_i16(&regrets[offset..offset + r.len()], r, out);
}

fn update_regrets_i16_impl(
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
    scratch.resize(len, 0.0);
    dequantize_block(slice, *scale, scratch);
    let (pos, neg) = (d.pos as f32, d.neg as f32);
    for (v, &delta) in scratch.iter_mut().zip(inst) {
        let factor = if *v > 0.0 { pos } else { neg };
        let mut updated = *v * factor + delta;
        if d.floor_neg && updated < 0.0 {
            updated = 0.0;
        }
        *v = updated;
    }
    *scale = quantize_block(scratch, slice);
}

fn accumulate_strategy_i16_impl(
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
    scratch.resize(len, 0.0);
    dequantize_block(slice, *scale, scratch);
    let avg = d.avg as f32;
    if d.reset_avg {
        scratch.copy_from_slice(weighted);
    } else {
        for (v, &w) in scratch.iter_mut().zip(weighted) {
            *v = *v * avg + w;
        }
    }
    *scale = quantize_block(scratch, slice);
}

fn average_strategy_i16_impl(strategy_sum: &[i16], offset: usize, r: StorageRef, out: &mut [f32]) {
    normalize_columns_i16(&strategy_sum[offset..offset + r.len()], r, out);
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
/// `normalize_columns_i16`). `update_regrets`/`accumulate_strategy` apply
/// the same discount math `F32Storage` does, but on a dequantized `f32`
/// copy of the node's block, then re-quantize.
///
/// That dequantized copy needs a scratch buffer sized to the node's `A*H`
/// block. Rather than allocate one per call (this runs once per action
/// node per player per iteration), `scratch` is a reusable `Vec<f32>` that
/// only grows (`resize`, never shrinks) — amortized to one allocation per
/// solve for the largest node visited, at the cost of holding that much
/// memory for the backend's lifetime. Each [`I16View`] split off gets its
/// own empty `scratch`, grown lazily by whatever nodes that view's rayon
/// task visits.
pub struct I16Storage {
    regrets: Vec<i16>,
    strategy_sum: Vec<i16>,
    regret_scales: Vec<f32>,
    strategy_scales: Vec<f32>,
    scratch: Vec<f32>,
}

impl I16Storage {
    #[allow(clippy::type_complexity)]
    pub fn snapshot(&self) -> (Vec<i16>, Vec<i16>, Vec<f32>, Vec<f32>) {
        (
            self.regrets.clone(),
            self.strategy_sum.clone(),
            self.regret_scales.clone(),
            self.strategy_scales.clone(),
        )
    }

    pub fn restore(&mut self, snapshot: (Vec<i16>, Vec<i16>, Vec<f32>, Vec<f32>)) {
        assert_eq!(snapshot.0.len(), self.regrets.len());
        assert_eq!(snapshot.1.len(), self.strategy_sum.len());
        assert_eq!(snapshot.2.len(), self.regret_scales.len());
        assert_eq!(snapshot.3.len(), self.strategy_scales.len());
        self.regrets = snapshot.0;
        self.strategy_sum = snapshot.1;
        self.regret_scales = snapshot.2;
        self.strategy_scales = snapshot.3;
    }
}

impl StorageOps for I16Storage {
    fn regret_matching(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        regret_matching_i16_impl(&self.regrets, r.offset, r, out);
    }

    fn update_regrets(&mut self, r: StorageRef, ref_idx: u32, inst: &[f32], d: &Discounts) {
        update_regrets_i16_impl(
            &mut self.regrets,
            r.offset,
            r,
            inst,
            d,
            &mut self.regret_scales[ref_idx as usize],
            &mut self.scratch,
        );
    }

    fn accumulate_strategy(
        &mut self,
        r: StorageRef,
        ref_idx: u32,
        weighted: &[f32],
        d: &Discounts,
    ) {
        accumulate_strategy_i16_impl(
            &mut self.strategy_sum,
            r.offset,
            r,
            weighted,
            d,
            &mut self.strategy_scales[ref_idx as usize],
            &mut self.scratch,
        );
    }

    fn average_strategy(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        average_strategy_i16_impl(&self.strategy_sum, r.offset, r, out);
    }
}

impl Storage for I16Storage {
    type View<'a> = I16View<'a>;

    fn new(len: usize, num_refs: usize) -> Self {
        I16Storage {
            regrets: vec![0i16; len],
            strategy_sum: vec![0i16; len],
            regret_scales: vec![1.0; num_refs],
            strategy_scales: vec![1.0; num_refs],
            scratch: Vec::new(),
        }
    }

    fn view_mut(&mut self) -> I16View<'_> {
        I16View {
            regrets: &mut self.regrets,
            strategy_sum: &mut self.strategy_sum,
            regret_scales: &mut self.regret_scales,
            strategy_scales: &mut self.strategy_scales,
            base: 0,
            sref_base: 0,
            scratch: Vec::new(),
        }
    }

    fn bytes_for(len: usize, num_refs: usize) -> u64 {
        2 * len as u64 * 2 + num_refs as u64 * 2 * 4
    }

    fn state(&self) -> StorageState {
        StorageState::I16 {
            regrets: self.regrets.clone(),
            strategy_sum: self.strategy_sum.clone(),
            regret_scales: self.regret_scales.clone(),
            strategy_scales: self.strategy_scales.clone(),
        }
    }

    fn restore_state(&mut self, state: StorageState) -> Result<(), StateMismatch> {
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
/// `scratch` is this view's own dequantization buffer (see the
/// [`I16Storage`] doc comment): empty until this view's first
/// `update_regrets`/`accumulate_strategy` call, then grown on demand.
pub struct I16View<'a> {
    regrets: &'a mut [i16],
    strategy_sum: &'a mut [i16],
    regret_scales: &'a mut [f32],
    strategy_scales: &'a mut [f32],
    base: usize,
    sref_base: u32,
    scratch: Vec<f32>,
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

    fn update_regrets(&mut self, r: StorageRef, ref_idx: u32, inst: &[f32], d: &Discounts) {
        let local = self.local_offset(r);
        let sidx = self.local_ref(ref_idx);
        update_regrets_i16_impl(
            &mut *self.regrets,
            local,
            r,
            inst,
            d,
            &mut self.regret_scales[sidx],
            &mut self.scratch,
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
        accumulate_strategy_i16_impl(
            &mut *self.strategy_sum,
            local,
            r,
            weighted,
            d,
            &mut self.strategy_scales[sidx],
            &mut self.scratch,
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
                scratch: Vec::new(),
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
    /// returns the resulting (current, average) strategies. Uses `r.index`
    /// as the `ref_idx` threaded through every op — exactly what the solver
    /// does at every call site (see `crate::solver`).
    fn run_all_ops<O: StorageOps>(
        ops: &mut O,
        r: StorageRef,
        inst: &[f32],
    ) -> (Vec<f32>, Vec<f32>) {
        let d = discounts();
        ops.update_regrets(r, r.index, inst, &d);
        let mut sigma = vec![0.0; r.len()];
        ops.regret_matching(r, r.index, &mut sigma);
        ops.accumulate_strategy(r, r.index, &sigma, &d);
        let mut avg = vec![0.0; r.len()];
        ops.average_strategy(r, r.index, &mut avg);
        (sigma, avg)
    }

    /// Two refs plus the spans they live in, shared by the `F32` and `I16`
    /// split tests: ref 0 inside span `[10, 30)` (sref range `[0, 1)`), ref
    /// 1 inside span `[50, 80)` (sref range `[1, 2)`).
    fn split_fixture() -> (StorageRef, StorageRef, [StorageSpan; 2]) {
        let r1 = make_ref(12, 2, 3, 0);
        let r2 = make_ref(60, 3, 4, 1);
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
        (r1, r2, spans)
    }

    #[test]
    fn view_split_matches_direct_ops() {
        let len = 100;
        let (r1, r2, spans) = split_fixture();

        let inst1: Vec<f32> = (0..r1.len()).map(|i| i as f32 * 0.5 + 1.0).collect();
        let inst2: Vec<f32> = (0..r2.len()).map(|i| i as f32 * 0.25 - 0.5).collect();

        let mut direct = F32Storage::new(len, 2);
        let (sigma1_direct, avg1_direct) = run_all_ops(&mut direct, r1, &inst1);
        let (sigma2_direct, avg2_direct) = run_all_ops(&mut direct, r2, &inst2);

        let mut split_target = F32Storage::new(len, 2);
        let mut view = split_target.view_mut();
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

    #[test]
    fn i16_view_split_matches_direct_ops() {
        let len = 100;
        let (r1, r2, spans) = split_fixture();

        let inst1: Vec<f32> = (0..r1.len()).map(|i| i as f32 * 0.5 + 1.0).collect();
        let inst2: Vec<f32> = (0..r2.len()).map(|i| i as f32 * 0.25 - 0.5).collect();

        let mut direct = I16Storage::new(len, 2);
        let (sigma1_direct, avg1_direct) = run_all_ops(&mut direct, r1, &inst1);
        let (sigma2_direct, avg2_direct) = run_all_ops(&mut direct, r2, &inst2);

        let mut split_target = I16Storage::new(len, 2);
        let mut view = split_target.view_mut();
        let mut subviews = view.split(&spans);
        assert_eq!(subviews.len(), 2);

        // Parent view slices are empty after split.
        assert!(view.regrets.is_empty());
        assert!(view.strategy_sum.is_empty());
        assert!(view.regret_scales.is_empty());
        assert!(view.strategy_scales.is_empty());

        let (sigma1_split, avg1_split) = run_all_ops(&mut subviews[0], r1, &inst1);
        let (sigma2_split, avg2_split) = run_all_ops(&mut subviews[1], r2, &inst2);

        // i16 quantization is deterministic, so a split view must match the
        // unsplit backend bit-for-bit, exactly like F32.
        assert_eq!(sigma1_direct, sigma1_split);
        assert_eq!(sigma2_direct, sigma2_split);
        assert_eq!(avg1_direct, avg1_split);
        assert_eq!(avg2_direct, avg2_split);
    }

    #[test]
    fn i16_quantization_round_trips_approximately() {
        // A handful of update_regrets/accumulate_strategy rounds should
        // reproduce F32Storage's results up to i16 quantization noise
        // (headroom is 32000/32767, so relative error per element is on
        // the order of 1/32000).
        let r = make_ref(0, 2, 3, 0);
        let d = discounts();

        let mut f32_backend = F32Storage::new(r.len(), 1);
        let mut i16_backend = I16Storage::new(r.len(), 1);

        for round in 0..5 {
            let inst: Vec<f32> = (0..r.len())
                .map(|i| (i as f32 + 1.0) * (round as f32 + 1.0) * 0.1 - 0.3)
                .collect();
            f32_backend.update_regrets(r, r.index, &inst, &d);
            i16_backend.update_regrets(r, r.index, &inst, &d);

            let mut sigma_f32 = vec![0.0; r.len()];
            let mut sigma_i16 = vec![0.0; r.len()];
            f32_backend.regret_matching(r, r.index, &mut sigma_f32);
            i16_backend.regret_matching(r, r.index, &mut sigma_i16);
            for (a, b) in sigma_f32.iter().zip(&sigma_i16) {
                assert!((a - b).abs() < 1e-3, "sigma {a} vs {b}");
            }

            f32_backend.accumulate_strategy(r, r.index, &sigma_f32, &d);
            i16_backend.accumulate_strategy(r, r.index, &sigma_i16, &d);
        }

        let mut avg_f32 = vec![0.0; r.len()];
        let mut avg_i16 = vec![0.0; r.len()];
        f32_backend.average_strategy(r, r.index, &mut avg_f32);
        i16_backend.average_strategy(r, r.index, &mut avg_i16);
        for (a, b) in avg_f32.iter().zip(&avg_i16) {
            assert!((a - b).abs() < 1e-3, "avg {a} vs {b}");
        }
    }

    #[test]
    fn i16_state_round_trip() {
        let r = make_ref(0, 2, 3, 0);
        let d = discounts();
        let inst: Vec<f32> = (0..r.len()).map(|i| i as f32 * 0.5 + 1.0).collect();

        let mut backend = I16Storage::new(r.len(), 1);
        backend.update_regrets(r, r.index, &inst, &d);
        let mut sigma = vec![0.0; r.len()];
        backend.regret_matching(r, r.index, &mut sigma);
        backend.accumulate_strategy(r, r.index, &sigma, &d);

        let state = backend.state();
        let mut restored = I16Storage::new(r.len(), 1);
        restored.restore_state(state.clone()).unwrap();
        assert_eq!(restored.state(), state);

        // Wrong variant.
        let f32_state = F32Storage::new(r.len(), 1).state();
        assert_eq!(
            restored.restore_state(f32_state),
            Err(StateMismatch::WrongVariant)
        );

        // Wrong length.
        let short = StorageState::I16 {
            regrets: vec![0; r.len() - 1],
            strategy_sum: vec![0; r.len()],
            regret_scales: vec![1.0; 1],
            strategy_scales: vec![1.0; 1],
        };
        assert!(matches!(
            restored.restore_state(short),
            Err(StateMismatch::WrongLength { .. })
        ));
    }

    #[test]
    fn bytes_for_i16_smaller_than_f32() {
        let (len, num_refs) = (10_000usize, 200usize);
        let f32_bytes = F32Storage::bytes_for(len, num_refs);
        let i16_bytes = I16Storage::bytes_for(len, num_refs);
        assert!(
            i16_bytes < f32_bytes,
            "i16 ({i16_bytes}) should be smaller than f32 ({f32_bytes})"
        );
        // Scale overhead is negligible next to the element arenas at this
        // size, so i16 should land close to half of f32.
        let ratio = i16_bytes as f64 / f32_bytes as f64;
        assert!(
            (0.45..0.55).contains(&ratio),
            "expected i16 to be roughly half of f32 for large len, ratio = {ratio}"
        );
    }
}
