use crate::CfrPrecision;
use crate::schedule::Discounts;
use rayon::prelude::*;

/// Arenas smaller than this are left to the allocator's lazy zero pages.
const PREFAULT_MIN_BYTES: usize = 64 << 20;
const PREFAULT_PAGE_BYTES: usize = 4096;
const PREFAULT_CHUNK_BYTES: usize = 1 << 20;

/// Touches every page of a freshly allocated, all-zero arena by writing zero,
/// in parallel on the current rayon pool.
///
/// Large `vec![0; n]` arenas come from untouched zero pages. The first CFR
/// iteration reads an element before writing it, so the read maps the shared
/// zero page and the later write takes a copy-on-write fault with a TLB
/// shootdown to every CPU of the process. Writing first, without reading,
/// avoids that. Only for arenas that are all zero (`T::default()`).
fn prefault_zeroed<T: Copy + Default + Send>(arena: &mut [T]) {
    prefault_zeroed_above(arena, PREFAULT_MIN_BYTES);
}

fn prefault_zeroed_above<T: Copy + Default + Send>(arena: &mut [T], min_bytes: usize) {
    let size = std::mem::size_of::<T>();
    if size == 0 || std::mem::size_of_val(arena) < min_bytes {
        return;
    }
    let page = (PREFAULT_PAGE_BYTES / size).max(1);
    let chunk = (PREFAULT_CHUNK_BYTES / size).max(page);
    arena.par_chunks_mut(chunk).for_each(|part| {
        for i in (0..part.len()).step_by(page) {
            // SAFETY: `i < part.len()`, so the pointer is in bounds, valid and
            // aligned for `T`. The arena is all `T::default()`, so the store
            // leaves it unchanged; `volatile` keeps the store from being elided.
            unsafe { std::ptr::write_volatile(part.as_mut_ptr().add(i), T::default()) }
        }
    });
}

#[cfg(test)]
mod precision_tests {
    use super::*;

    fn check<S: Storage>() {
        // Cross a block boundary; cover negative regrets and zero columns.
        let r = StorageRef {
            offset: 7,
            num_actions: 7,
            num_hands: 259,
            index: 0,
        };
        let mut storage = S::new(r.offset + r.len(), 1);
        let data: Vec<f32> = (0..r.len())
            .map(|i| {
                if (i % 259).is_multiple_of(11) {
                    -1.0
                } else {
                    ((i * 37 % 101) as f32 - 20.0) / 103.0
                }
            })
            .collect();
        let d = Discounts {
            pos: 1.0,
            neg: 1.0,
            avg: 1.0,
            floor_neg: false,
            reset_avg: false,
        };
        storage.update_regrets(r, 0, &data, &d);
        storage.accumulate_strategy(r, 0, &data, &d);
        let mut exact = vec![0.0; r.len()];
        let mut relaxed = exact.clone();
        storage.regret_matching(r, 0, &mut exact);
        storage.regret_matching_cfr(r, 0, &mut relaxed, CfrPrecision::F64);
        assert_eq!(
            exact.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            relaxed.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        storage.regret_matching_cfr(r, 0, &mut relaxed, CfrPrecision::F32);
        let max = exact
            .iter()
            .zip(&relaxed)
            .map(|(&a, &b)| f64::from((a - b).abs()))
            .fold(0.0, f64::max);
        assert!(max < 1e-6);
        assert!(
            exact
                .iter()
                .zip(&relaxed)
                .any(|(a, b)| a.to_bits() != b.to_bits())
        );
        for h in 0..259 {
            let sum: f32 = (0..7).map(|a| relaxed[a * 259 + h]).sum();
            assert!((sum - 1.0).abs() < 1e-6);
            if h.is_multiple_of(11) {
                for a in 0..7 {
                    assert_eq!(relaxed[a * 259 + h], 1.0 / 7.0);
                }
            }
        }
        // Full backend and rebased view use the identical selected normalization.
        {
            let mut view = storage.view_mut();
            let span = StorageSpan {
                start: r.offset,
                end: r.offset + r.len(),
                sref_start: 0,
                sref_end: 1,
            };
            let view = view.split(&[span]).pop().unwrap();
            let mut through_view = vec![0.0; r.len()];
            view.regret_matching_cfr(r, 0, &mut through_view, CfrPrecision::F32);
            assert_eq!(through_view, relaxed);
        }
        println!(
            "{} norm f32 max absolute error={max:e}",
            std::any::type_name::<S>()
        );
        let mut average = vec![0.0; r.len()];
        let mut expected = average.clone();
        storage.average_strategy(r, 0, &mut average);
        match storage.arrays() {
            StorageArrays::F32 { strategy_sum, .. } | StorageArrays::Mixed { strategy_sum, .. } => {
                normalize_columns(
                    &strategy_sum[r.offset..r.offset + r.len()],
                    r,
                    &mut expected,
                )
            }
            StorageArrays::I16 { strategy_sum, .. } => normalize_columns_i16(
                &strategy_sum[r.offset..r.offset + r.len()],
                r,
                &mut expected,
            ),
        }
        assert_eq!(
            average.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
    #[test]
    fn f32_normalization_all_backends_and_views() {
        check::<F32Storage>();
        check::<I16Storage>();
        check::<MixedStorage>();
    }
}

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

    /// CFR-only hook; other implementations retain their exact behavior.
    fn regret_matching_cfr(
        &self,
        r: StorageRef,
        ref_idx: u32,
        out: &mut [f32],
        _norm: CfrPrecision,
    ) {
        self.regret_matching(r, ref_idx, out);
    }

    /// Applies pre-add discounting to stored regrets, then adds `inst`.
    fn update_regrets(&mut self, r: StorageRef, ref_idx: u32, inst: &[f32], d: &Discounts);

    /// Applies pre-add discounting (or reset) to the cumulative strategy,
    /// then adds the reach-weighted current strategy `weighted`.
    fn accumulate_strategy(&mut self, r: StorageRef, ref_idx: u32, weighted: &[f32], d: &Discounts);

    /// Writes the normalized average strategy into `out`. Hands never
    /// reached get the uniform strategy.
    fn average_strategy(&self, r: StorageRef, ref_idx: u32, out: &mut [f32]);

    /// Writes the raw accumulated regrets (dequantized for a quantized
    /// backend, unnormalized — unlike [`StorageOps::regret_matching`]) into
    /// `out` (`A*H`, action-major). Used by negative-regret pruning, which
    /// needs the actual regret magnitude (to compare against a threshold),
    /// not the positive-part-normalized strategy.
    fn raw_regrets(&self, r: StorageRef, ref_idx: u32, out: &mut [f32]);
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

    /// Permanently frees the regret arena and its node scales (zero capacity).
    /// Average strategy and evaluation remain available. Regret operations,
    /// mutable views, snapshots, checkpoint arrays and restoration panic after
    /// release, including for empty arenas. Calling release again is harmless.
    fn release_regrets(&mut self);

    fn regrets_released(&self) -> bool;

    /// Panics if a regret-dependent operation follows release.
    fn assert_regrets_available(&self) {
        assert!(!self.regrets_released(), "regrets have been released");
    }

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

    /// Borrow the raw arenas without allocating a snapshot.
    fn arrays(&self) -> StorageArrays<'_>;

    /// Borrow raw arenas for streaming restoration. Callers must discard the
    /// backend if an I/O or integrity check fails after writing any elements.
    fn arrays_mut(&mut self) -> StorageArraysMut<'_>;

    /// Scales every accumulated regret by `regret` and every accumulated
    /// strategy-sum by `strategy` (batched early discounting, applied
    /// between iterations — never during a pass, so unlike [`StorageOps`]
    /// this lives on the full backend, not [`StorageView`]). A quantized
    /// backend only needs to touch its O(num_refs) per-node scale arrays
    /// (dequantized value = raw_i16 * scale, so scaling the value is
    /// scaling the scale) rather than the O(len) element arrays — the whole
    /// point of the per-node-scale representation.
    fn scale_all(&mut self, regret: f32, strategy: f32);
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
    Mixed {
        regrets: Vec<i16>,
        strategy_sum: Vec<f32>,
        regret_scales: Vec<f32>,
    },
}

/// Raw checkpoint arenas in stable order: regrets, strategy sum, then scales.
pub enum StorageArrays<'a> {
    F32 {
        regrets: &'a [f32],
        strategy_sum: &'a [f32],
    },
    I16 {
        regrets: &'a [i16],
        strategy_sum: &'a [i16],
        regret_scales: &'a [f32],
        strategy_scales: &'a [f32],
    },
    Mixed {
        regrets: &'a [i16],
        strategy_sum: &'a [f32],
        regret_scales: &'a [f32],
    },
}

pub enum StorageArraysMut<'a> {
    F32 {
        regrets: &'a mut [f32],
        strategy_sum: &'a mut [f32],
    },
    I16 {
        regrets: &'a mut [i16],
        strategy_sum: &'a mut [i16],
        regret_scales: &'a mut [f32],
        strategy_scales: &'a mut [f32],
    },
    Mixed {
        regrets: &'a mut [i16],
        strategy_sum: &'a mut [f32],
        regret_scales: &'a mut [f32],
    },
}

impl StorageState {
    pub fn arrays(&self) -> StorageArrays<'_> {
        match self {
            Self::F32 {
                regrets,
                strategy_sum,
            } => StorageArrays::F32 {
                regrets,
                strategy_sum,
            },
            Self::Mixed {
                regrets,
                strategy_sum,
                regret_scales,
            } => StorageArrays::Mixed {
                regrets,
                strategy_sum,
                regret_scales,
            },
            Self::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            } => StorageArrays::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            },
        }
    }
    pub fn arrays_mut(&mut self) -> StorageArraysMut<'_> {
        match self {
            Self::F32 {
                regrets,
                strategy_sum,
            } => StorageArraysMut::F32 {
                regrets,
                strategy_sum,
            },
            Self::Mixed {
                regrets,
                strategy_sum,
                regret_scales,
            } => StorageArraysMut::Mixed {
                regrets,
                strategy_sum,
                regret_scales,
            },
            Self::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            } => StorageArraysMut::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            },
        }
    }
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
    regrets_released: bool,
    regrets: Vec<f32>,
    strategy_sum: Vec<f32>,
}

impl F32Storage {
    pub fn snapshot(&self) -> (Vec<f32>, Vec<f32>) {
        self.assert_regrets_available();
        (self.regrets.clone(), self.strategy_sum.clone())
    }

    pub fn restore(&mut self, snapshot: (Vec<f32>, Vec<f32>)) {
        self.assert_regrets_available();
        assert_eq!(snapshot.0.len(), self.regrets.len());
        assert_eq!(snapshot.1.len(), self.strategy_sum.len());
        self.regrets = snapshot.0;
        self.strategy_sum = snapshot.1;
    }
}

fn normalize_columns(data: &[f32], r: StorageRef, out: &mut [f32]) {
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
                *total += value.max(0.0) as f64;
            }
        }
        for a in 0..num_actions {
            let row = &data[a * num_hands + first..a * num_hands + first + len];
            let dst = &mut out[a * num_hands + first..a * num_hands + first + len];
            for ((dst, &value), &total) in dst.iter_mut().zip(row).zip(&totals[..len]) {
                *dst = if total > 0.0 {
                    (value.max(0.0) as f64 / total) as f32
                } else {
                    uniform
                };
            }
        }
    }
}

fn normalize_columns_f32(data: &[f32], r: StorageRef, out: &mut [f32]) {
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
                *total += value.max(0.0);
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
                    value.max(0.0) * inverse
                } else {
                    uniform
                };
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

fn raw_regrets_impl(regrets: &[f32], offset: usize, r: StorageRef, out: &mut [f32]) {
    out.copy_from_slice(&regrets[offset..offset + r.len()]);
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

    fn raw_regrets(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        self.assert_regrets_available();
        raw_regrets_impl(&self.regrets, r.offset, r, out);
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

    fn scale_all(&mut self, regret: f32, strategy: f32) {
        self.assert_regrets_available();
        for v in &mut self.regrets {
            *v *= regret;
        }
        for v in &mut self.strategy_sum {
            *v *= strategy;
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

    fn raw_regrets(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        let local = self.local_offset(r);
        raw_regrets_impl(&*self.regrets, local, r, out);
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
                *total += value.max(0) as f64;
            }
        }
        for a in 0..num_actions {
            let row = &data[a * num_hands + first..a * num_hands + first + len];
            let dst = &mut out[a * num_hands + first..a * num_hands + first + len];
            for ((dst, &value), &total) in dst.iter_mut().zip(row).zip(&totals[..len]) {
                *dst = if total > 0.0 {
                    (value.max(0) as f64 / total) as f32
                } else {
                    uniform
                };
            }
        }
    }
}

fn normalize_columns_i16_f32(data: &[i16], r: StorageRef, out: &mut [f32]) {
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
                *total += value.max(0) as f32;
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
                    value.max(0) as f32 * inverse
                } else {
                    uniform
                };
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
fn quantize_block(vals: &[f32], max_abs: f32, out_q: &mut [i16]) -> f32 {
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
    let (add, add_tail) = input.as_chunks::<8>();
    let mut maxima = [0.0f32; 8];
    for ((dst, src), add) in dst.iter_mut().zip(src).zip(add) {
        for lane in 0..8 {
            let value = update(src[lane], add[lane]);
            dst[lane] = value;
            let abs = value.abs();
            if abs > maxima[lane] {
                maxima[lane] = abs;
            }
        }
    }
    let mut max_abs = maxima.into_iter().fold(0.0f32, f32::max);
    for ((dst, &src), &add) in dst_tail.iter_mut().zip(src_tail).zip(add_tail) {
        let value = update(src, add);
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

fn average_strategy_i16_impl(strategy_sum: &[i16], offset: usize, r: StorageRef, out: &mut [f32]) {
    normalize_columns_i16(&strategy_sum[offset..offset + r.len()], r, out);
}

fn raw_regrets_i16_impl(
    regrets: &[i16],
    offset: usize,
    r: StorageRef,
    scale: f32,
    out: &mut [f32],
) {
    dequantize_block(&regrets[offset..offset + r.len()], scale, out);
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
    regrets_released: bool,
    regrets: Vec<i16>,
    strategy_sum: Vec<i16>,
    regret_scales: Vec<f32>,
    strategy_scales: Vec<f32>,
    scratch: Vec<f32>,
}

impl I16Storage {
    #[allow(clippy::type_complexity)]
    pub fn snapshot(&self) -> (Vec<i16>, Vec<i16>, Vec<f32>, Vec<f32>) {
        self.assert_regrets_available();
        (
            self.regrets.clone(),
            self.strategy_sum.clone(),
            self.regret_scales.clone(),
            self.strategy_scales.clone(),
        )
    }

    pub fn restore(&mut self, snapshot: (Vec<i16>, Vec<i16>, Vec<f32>, Vec<f32>)) {
        self.assert_regrets_available();
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
                normalize_columns_i16_f32(&self.regrets[r.offset..r.offset + r.len()], r, out);
            }
        }
    }

    fn update_regrets(&mut self, r: StorageRef, ref_idx: u32, inst: &[f32], d: &Discounts) {
        self.assert_regrets_available();
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

    fn raw_regrets(&self, r: StorageRef, ref_idx: u32, out: &mut [f32]) {
        self.assert_regrets_available();
        raw_regrets_i16_impl(
            &self.regrets,
            r.offset,
            r,
            self.regret_scales[ref_idx as usize],
            out,
        );
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
            scratch: Vec::new(),
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
            scratch: Vec::new(),
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

    fn scale_all(&mut self, regret: f32, strategy: f32) {
        self.assert_regrets_available();
        for v in &mut self.regret_scales {
            *v *= regret;
        }
        for v in &mut self.strategy_scales {
            *v *= strategy;
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
                normalize_columns_i16_f32(&self.regrets[local..local + r.len()], r, out);
            }
        }
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

    fn raw_regrets(&self, r: StorageRef, ref_idx: u32, out: &mut [f32]) {
        let local = self.local_offset(r);
        let sidx = self.local_ref(ref_idx);
        raw_regrets_i16_impl(&*self.regrets, local, r, self.regret_scales[sidx], out);
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

/// i16 regrets with the legacy per-node quantization; f32 strategy sums.
/// Split views own disjoint arenas/scales and a private regret scratch buffer.
pub struct MixedStorage {
    regrets_released: bool,
    regrets: Vec<i16>,
    strategy_sum: Vec<f32>,
    regret_scales: Vec<f32>,
    scratch: Vec<f32>,
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
                normalize_columns_i16_f32(&self.regrets[r.offset..r.offset + r.len()], r, out);
            }
        }
    }

    fn update_regrets(&mut self, r: StorageRef, ref_idx: u32, inst: &[f32], d: &Discounts) {
        self.assert_regrets_available();
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
        _ref_idx: u32,
        weighted: &[f32],
        d: &Discounts,
    ) {
        accumulate_strategy_impl(&mut self.strategy_sum, r.offset, r, weighted, d);
    }

    fn average_strategy(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {
        average_strategy_impl(&self.strategy_sum, r.offset, r, out);
    }

    fn raw_regrets(&self, r: StorageRef, ref_idx: u32, out: &mut [f32]) {
        self.assert_regrets_available();
        raw_regrets_i16_impl(
            &self.regrets,
            r.offset,
            r,
            self.regret_scales[ref_idx as usize],
            out,
        );
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
            scratch: Vec::new(),
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
            scratch: Vec::new(),
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

    fn scale_all(&mut self, regret: f32, strategy: f32) {
        self.assert_regrets_available();
        for v in &mut self.regret_scales {
            *v *= regret;
        }
        for v in &mut self.strategy_sum {
            *v *= strategy;
        }
    }
}

pub struct MixedView<'a> {
    regrets: &'a mut [i16],
    strategy_sum: &'a mut [f32],
    regret_scales: &'a mut [f32],
    base: usize,
    sref_base: u32,
    scratch: Vec<f32>,
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
                normalize_columns_i16_f32(&self.regrets[local..local + r.len()], r, out);
            }
        }
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

    fn raw_regrets(&self, r: StorageRef, ref_idx: u32, out: &mut [f32]) {
        let local = self.local_offset(r);
        let sidx = self.local_ref(ref_idx);
        raw_regrets_i16_impl(&*self.regrets, local, r, self.regret_scales[sidx], out);
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
                scratch: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i16_fused_updates_preserve_discount_floor_reset_and_headroom() {
        // Exercise full SIMD chunks, tails, empty nodes, rebased offsets,
        // and reuse of a scratch buffer larger than the current node.
        let mut scratch = vec![f32::NAN; 300];
        for len in [0usize, 1, 7, 8, 9, 257] {
            let r = make_ref(3, 1, len as u32, 0);
            let initial: Vec<i16> = (0..len).map(|i| (i as i16 % 7 - 3) * 9000).collect();
            let input: Vec<f32> = (0..len).map(|i| (i % 11) as f32 - 5.0).collect();
            for floor_neg in [false, true] {
                for reset_avg in [false, true] {
                    let d = Discounts {
                        pos: 0.75,
                        neg: 0.25,
                        avg: 0.5,
                        floor_neg,
                        reset_avg,
                    };
                    for strategy in [false, true] {
                        let mut arena = vec![123i16; len + 6];
                        arena[3..3 + len].copy_from_slice(&initial);
                        let mut scale = 0.125;
                        let expected: Vec<f32> = initial
                            .iter()
                            .zip(&input)
                            .map(|(&q, &add)| {
                                let v = q as f32 * scale;
                                if strategy {
                                    if reset_avg {
                                        add
                                    } else {
                                        v * d.avg as f32 + add
                                    }
                                } else {
                                    let updated =
                                        v * if v > 0.0 { d.pos as f32 } else { d.neg as f32 } + add;
                                    if floor_neg { updated.max(0.0) } else { updated }
                                }
                            })
                            .collect();
                        if strategy {
                            accumulate_strategy_i16_impl(
                                &mut arena,
                                3,
                                r,
                                &input,
                                &d,
                                &mut scale,
                                &mut scratch,
                            );
                        } else {
                            update_regrets_i16_impl(
                                &mut arena,
                                3,
                                r,
                                &input,
                                &d,
                                &mut scale,
                                &mut scratch,
                            );
                        }
                        assert_eq!(&arena[..3], &[123; 3]);
                        assert_eq!(&arena[3 + len..], &[123; 3]);
                        assert_eq!(scratch.len(), 300);
                        let max_abs = expected.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                        assert_eq!(
                            scale,
                            if max_abs == 0.0 {
                                1.0
                            } else {
                                max_abs / 32_000.0
                            }
                        );
                        for (&q, &v) in arena[3..3 + len].iter().zip(&expected) {
                            assert!(q.abs() <= 32_000);
                            assert!((q as f32 * scale - v).abs() <= scale * 0.501);
                            if !strategy && floor_neg {
                                assert!(q >= 0);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn i16_zero_blocks_and_subnormal_quantization() {
        let mut q = [17; 9];
        assert_eq!(quantize_block(&[0.0; 9], 0.0, &mut q), 1.0);
        assert_eq!(q, [0; 9]);
        // A reset must discard both the old arena and its scale.
        let r = make_ref(0, 1, 9, 0);
        let mut scratch = Vec::new();
        let mut scale = 100.0;
        let mut d = discounts();
        d.reset_avg = true;
        q.fill(32_000);
        accumulate_strategy_i16_impl(&mut q, 0, r, &[0.0; 9], &d, &mut scale, &mut scratch);
        assert_eq!(q, [0; 9]);
        assert_eq!(scale, 1.0);
        assert!(scratch.is_empty());
        let mut tiny = [0; 2];
        let values = [f32::from_bits(1), -f32::from_bits(1)];
        assert_eq!(quantize_block(&values, values[0], &mut tiny), 0.0);
        assert_eq!(tiny, [i16::MAX, i16::MIN]);
    }

    #[test]
    fn i16_bounded_conversion_handles_extreme_and_nonfinite_blocks() {
        for max_abs in [1e-30f32, 1.0, 1e30, f32::MAX] {
            let values = [max_abs, -max_abs, max_abs * 0.25, f32::NAN, 0.0];
            let mut q = [0; 5];
            let scale = quantize_block(&values, max_abs, &mut q);
            assert_eq!(q[0], 32_000);
            assert_eq!(q[1], -32_000);
            assert_eq!(q[2], 8000);
            assert_eq!(&q[3..], &[0, 0]);
            assert!(scale.is_finite() && scale > 0.0);
        }
        let mut q = [1; 3];
        assert_eq!(
            quantize_block(
                &[f32::INFINITY, f32::NEG_INFINITY, 1.0],
                f32::INFINITY,
                &mut q
            ),
            f32::INFINITY
        );
        assert_eq!(q, [0; 3]);
    }

    #[test]
    fn prefault_keeps_arena_zero() {
        // A few pages, threshold bypassed; lengths not page-aligned.
        let mut a = vec![0.0f32; 5 * 1024 + 7];
        prefault_zeroed_above(&mut a, 0);
        assert!(a.iter().all(|&x| x.to_bits() == 0));
        let mut b = vec![0i16; 3 * 2048 + 5];
        prefault_zeroed_above(&mut b, 0);
        assert!(b.iter().all(|&x| x == 0));
        // Below the threshold nothing happens.
        prefault_zeroed(&mut b);
        let mut empty: Vec<f32> = Vec::new();
        prefault_zeroed_above(&mut empty, 0);
    }

    #[test]
    fn mixed_ops_match_i16_regrets_and_f32_sums_bitwise() {
        let r = make_ref(0, 3, 257, 0);
        let mut mixed = MixedStorage::new(r.len(), 1);
        let mut quantized = I16Storage::new(r.len(), 1);
        let mut float = F32Storage::new(r.len(), 1);
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        for t in 1..=100 {
            let d = Discounts {
                pos: 0.97,
                neg: 0.5,
                avg: 0.83,
                floor_neg: t % 3 == 0,
                reset_avg: t % 17 == 0,
            };
            let inst: Vec<_> = (0..r.len())
                .map(|i| ((i * 19 + t) % 61) as f32 - 30.0)
                .collect();
            mixed.update_regrets(r, 0, &inst, &d);
            quantized.update_regrets(r, 0, &inst, &d);
            let mut sigma = vec![0.0; r.len()];
            mixed.regret_matching(r, 0, &mut sigma);
            let weighted: Vec<_> = sigma
                .iter()
                .enumerate()
                .map(|(i, v)| v * (i % 13) as f32 / 13.0)
                .collect();
            mixed.accumulate_strategy(r, 0, &weighted, &d);
            float.accumulate_strategy(r, 0, &weighted, &d);
            if t % 11 == 0 {
                mixed.scale_all(0.71, 0.63);
                quantized.scale_all(0.71, 0.63);
                float.scale_all(0.71, 0.63);
            }
            assert_eq!(mixed.regrets, quantized.regrets);
            assert_eq!(bits(&mixed.regret_scales), bits(&quantized.regret_scales));
            assert_eq!(bits(&mixed.strategy_sum), bits(&float.strategy_sum));
            let mut a = vec![0.0; r.len()];
            let mut b = a.clone();
            mixed.average_strategy(r, 0, &mut a);
            float.average_strategy(r, 0, &mut b);
            assert_eq!(bits(&a), bits(&b));
            mixed.raw_regrets(r, 0, &mut a);
            quantized.raw_regrets(r, 0, &mut b);
            assert_eq!(bits(&a), bits(&b));
        }
        let saved = mixed.state();
        let mut restored = MixedStorage::new(r.len(), 1);
        restored.restore_state(saved.clone()).unwrap();
        assert_eq!(restored.state(), saved);
        assert_eq!(
            restored.restore_state(float.state()),
            Err(StateMismatch::WrongVariant)
        );
        assert!(matches!(
            restored.restore_state(MixedStorage::new(r.len() - 1, 1).state()),
            Err(StateMismatch::WrongLength { .. })
        ));
        assert_eq!(restored.state(), saved);
        assert_eq!(MixedStorage::bytes_for(r.len(), 1), r.len() as u64 * 6 + 4);
    }

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
    fn mixed_view_split_matches_direct_ops() {
        let len = 100;
        let (r1, r2, spans) = split_fixture();

        let inst1: Vec<f32> = (0..r1.len()).map(|i| i as f32 * 0.5 + 1.0).collect();
        let inst2: Vec<f32> = (0..r2.len()).map(|i| i as f32 * 0.25 - 0.5).collect();

        let mut direct = MixedStorage::new(len, 2);
        let (sigma1_direct, avg1_direct) = run_all_ops(&mut direct, r1, &inst1);
        let (sigma2_direct, avg2_direct) = run_all_ops(&mut direct, r2, &inst2);

        let mut split_target = MixedStorage::new(len, 2);
        let mut view = split_target.view_mut();
        let mut subviews = view.split(&spans);
        assert_eq!(subviews.len(), 2);

        // Parent view slices are empty after split.
        assert!(view.regrets.is_empty());
        assert!(view.strategy_sum.is_empty());
        assert!(view.regret_scales.is_empty());

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

    #[test]
    fn f32_scale_all_scales_arrays() {
        let r = make_ref(0, 2, 3, 0);
        let d = discounts();
        let inst: Vec<f32> = (0..r.len()).map(|i| i as f32 * 0.5 + 1.0).collect();

        let mut backend = F32Storage::new(r.len(), 1);
        backend.update_regrets(r, r.index, &inst, &d);
        let mut sigma = vec![0.0; r.len()];
        backend.regret_matching(r, r.index, &mut sigma);
        backend.accumulate_strategy(r, r.index, &sigma, &d);

        let mut raw_before = vec![0.0; r.len()];
        backend.raw_regrets(r, r.index, &mut raw_before);
        let mut avg_before = vec![0.0; r.len()];
        backend.average_strategy(r, r.index, &mut avg_before);

        backend.scale_all(0.5, 0.25);

        let mut raw_after = vec![0.0; r.len()];
        backend.raw_regrets(r, r.index, &mut raw_after);
        for (b, a) in raw_before.iter().zip(&raw_after) {
            assert!((a - b * 0.5).abs() < 1e-6, "{a} vs {}", b * 0.5);
        }

        // Strategy sums scaled uniformly per column, so the normalized
        // average strategy (a ratio) is unaffected by the strategy scale.
        let mut avg_after = vec![0.0; r.len()];
        backend.average_strategy(r, r.index, &mut avg_after);
        for (b, a) in avg_before.iter().zip(&avg_after) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn i16_scale_all_scales_dequantized_values() {
        let r = make_ref(0, 2, 3, 0);
        let d = discounts();
        let inst: Vec<f32> = (0..r.len()).map(|i| i as f32 * 0.5 + 1.0).collect();

        let mut backend = I16Storage::new(r.len(), 1);
        backend.update_regrets(r, r.index, &inst, &d);
        let mut sigma = vec![0.0; r.len()];
        backend.regret_matching(r, r.index, &mut sigma);
        backend.accumulate_strategy(r, r.index, &sigma, &d);

        let mut raw_before = vec![0.0; r.len()];
        backend.raw_regrets(r, r.index, &mut raw_before);

        backend.scale_all(0.5, 0.25);

        let mut raw_after = vec![0.0; r.len()];
        backend.raw_regrets(r, r.index, &mut raw_after);
        for (b, a) in raw_before.iter().zip(&raw_after) {
            assert!((a - b * 0.5).abs() < 1e-3, "{a} vs {}", b * 0.5);
        }

        // regret_matching/average_strategy only read raw i16 sign/ratio, so
        // a per-node scale change (as opposed to touching the i16 payload)
        // must leave them unaffected.
        let mut sigma_after = vec![0.0; r.len()];
        backend.regret_matching(r, r.index, &mut sigma_after);
        assert_eq!(sigma, sigma_after);
    }
}

#[cfg(test)]
mod release_tests {
    use super::*;

    #[test]
    fn release_returns_all_regret_capacity_including_empty_storage() {
        for (len, refs) in [(100, 4), (0, 0)] {
            let mut f32 = F32Storage::new(len, refs);
            let mut i16 = I16Storage::new(len, refs);
            let mut mixed = MixedStorage::new(len, refs);
            f32.release_regrets();
            i16.release_regrets();
            mixed.release_regrets();
            assert_eq!((f32.regrets.len(), f32.regrets.capacity()), (0, 0));
            assert_eq!((i16.regrets.len(), i16.regrets.capacity()), (0, 0));
            assert_eq!((mixed.regrets.len(), mixed.regrets.capacity()), (0, 0));
            assert_eq!(
                (i16.regret_scales.len(), i16.regret_scales.capacity()),
                (0, 0)
            );
            assert_eq!(
                (mixed.regret_scales.len(), mixed.regret_scales.capacity()),
                (0, 0)
            );
            assert_eq!(f32.strategy_sum.len(), len);
            assert_eq!(i16.strategy_sum.len(), len);
            assert_eq!(i16.strategy_scales.len(), refs);
            assert_eq!(mixed.strategy_sum.len(), len);
        }
    }
}
