use crate::CfrPrecision;
use crate::schedule::Discounts;
use rayon::prelude::*;
mod f32;
mod i16;
mod mixed;
mod normalize;
pub use f32::{F32Storage, F32View};
use f32::{accumulate_strategy_impl, average_strategy_impl};
pub use i16::{I16Storage, I16View};
#[cfg(test)]
use i16::{accumulate_strategy_i16_impl, quantize_block};
use i16::{regret_matching_i16_impl, update_regrets_i16_impl};
pub use mixed::{MixedStorage, MixedView};
use normalize::{normalize_columns, normalize_columns_f32};

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
            StorageArrays::I16 { strategy_sum, .. } => normalize_columns(
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
/// `F32Storage` but locates the quantized backends' per-node scales
/// without re-walking the tree.
#[derive(Clone, Copy, Debug, Default)]
pub struct StorageSpan {
    pub start: usize,
    pub end: usize,
    pub sref_start: u32,
    pub sref_end: u32,
}

/// Element-level operations shared by a full [`Storage`] backend
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

    /// Adds child counterfactual values minus the node value, applying the same
    /// f32 subtraction and discounted update as `update_regrets`. Child values
    /// are contiguous action-major rows; `values` contains one value per hand.
    fn update_regrets_from_values(
        &mut self,
        r: StorageRef,
        ref_idx: u32,
        children: &[f32],
        values: &[f32],
        d: &Discounts,
    );

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
/// hot loop). Quantized backends use the same traversal.
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

thread_local! {
    static QUANTIZED_SCRATCH: std::cell::RefCell<Vec<Vec<f32>>> = const { std::cell::RefCell::new(Vec::new()) };
}
struct QuantizedScratch(Vec<f32>);
impl QuantizedScratch {
    fn take() -> Self {
        Self(QUANTIZED_SCRATCH.with(|pool| pool.borrow_mut().pop().unwrap_or_default()))
    }
}
impl Drop for QuantizedScratch {
    fn drop(&mut self) {
        QUANTIZED_SCRATCH.with(|pool| pool.borrow_mut().push(std::mem::take(&mut self.0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage_bits(storage: &impl Storage) -> Vec<u32> {
        match storage.arrays() {
            StorageArrays::F32 {
                regrets,
                strategy_sum,
            } => regrets
                .iter()
                .chain(strategy_sum)
                .map(|v| v.to_bits())
                .collect(),
            StorageArrays::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            } => regrets
                .iter()
                .chain(strategy_sum)
                .map(|&v| v as u32)
                .chain(
                    regret_scales
                        .iter()
                        .chain(strategy_scales)
                        .map(|v| v.to_bits()),
                )
                .collect(),
            StorageArrays::Mixed {
                regrets,
                strategy_sum,
                regret_scales,
            } => regrets
                .iter()
                .map(|&v| v as u32)
                .chain(
                    strategy_sum
                        .iter()
                        .chain(regret_scales)
                        .map(|v| v.to_bits()),
                )
                .collect(),
        }
    }

    fn check_regret_deltas<S: Storage>() {
        for hands in [0, 1, 7, 8, 9, 257] {
            let r = make_ref(3, 3, hands, 0);
            let span = StorageSpan {
                start: r.offset,
                end: r.offset + r.len(),
                sref_start: 0,
                sref_end: 1,
            };
            let mut reference = S::new(r.offset + r.len() + 3, 1);
            let mut actual = S::new(r.offset + r.len() + 3, 1);
            let mut viewed = S::new(r.offset + r.len() + 3, 1);
            for t in 0..12 {
                let d = Discounts {
                    pos: 0.75,
                    neg: 0.25,
                    avg: 0.5,
                    floor_neg: t % 3 == 0,
                    reset_avg: t % 4 == 0,
                };
                let values: Vec<_> = (0..hands)
                    .map(|h| {
                        if h % 3 == 0 {
                            -0.0
                        } else {
                            (h % 11) as f32 - 5.0
                        }
                    })
                    .collect();
                let children: Vec<_> = (0..r.len())
                    .map(|i| {
                        if i % 7 == 0 {
                            -0.0
                        } else {
                            ((i * 17 + t) % 61) as f32 - 30.0
                        }
                    })
                    .collect();
                let deltas: Vec<_> = children
                    .iter()
                    .zip(values.iter().cycle())
                    .map(|(&v, &cfv)| v - cfv)
                    .collect();
                reference.update_regrets(r, 0, &deltas, &d);
                actual.update_regrets_from_values(r, 0, &children, &values, &d);
                viewed
                    .view_mut()
                    .split(&[span])
                    .pop()
                    .unwrap()
                    .update_regrets_from_values(r, 0, &children, &values, &d);
                assert_eq!(storage_bits(&actual), storage_bits(&reference));
                assert_eq!(storage_bits(&viewed), storage_bits(&reference));
            }
        }
    }

    #[test]
    fn regret_deltas_match_materialized_updates_bitwise_in_backends_and_views() {
        check_regret_deltas::<F32Storage>();
        check_regret_deltas::<I16Storage>();
        check_regret_deltas::<MixedStorage>();
    }

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
                                input.iter().copied(),
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
            assert_eq!(mixed.regrets, quantized.regrets);
            assert_eq!(bits(&mixed.regret_scales), bits(&quantized.regret_scales));
            assert_eq!(bits(&mixed.strategy_sum), bits(&float.strategy_sum));
            let mut a = vec![0.0; r.len()];
            let mut b = a.clone();
            mixed.average_strategy(r, 0, &mut a);
            float.average_strategy(r, 0, &mut b);
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
