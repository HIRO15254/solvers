# Multiway checkpoint capture memory

Status: completed. All seven build/test checks and byte/resume compatibility
regressions passed. All six fixed measurements were retained and revalidated.
2026-09-10.

The three-pair medians were 2.199 to 1.043 seconds for writing (52.56% lower),
and 1.960 to 1.423 GB sampled write-phase working set (27.41% lower).
Whole-process lifetime peak decreased only 0.69%, because restoration still
dominates that measurement. All six complete checkpoint files were identical.
See the [final report](plan.md)
for the full observations, exact timing scope, source identities and limits.
The fixed runner's console-only phase maximum is unavailable; the validated
Python aggregate uses all preserved timestamped working-set observations.

The existing writer already serializes postcard through a raw temporary file,
then compresses 4 MiB chunks. Its large remaining staging cost is earlier:
`MultiwayCheckpoint::capture` creates an owned `SolverState`, cloning every
touched policy's action labels, regrets and average sums beside the live arena.
The production finalizer also captures this state when it only needs the
stored-policy count and no solution is written.

Add a borrowed snapshot serializer and a direct atomic solver writer. Keep
the owned snapshot/capture API and the existing chunk writer as comparison
paths. Dense scratch stores touched-node IDs, ancestor IDs and node flags;
within sorted node contexts, emit touched buckets in ascending order. Sparse
scratch stores sorted references to the existing policies and histories.
No policy values or strings are cloned. Untouched and touched-all-zero columns
remain distinct, and all ancestors of touched dense columns remain present.
Allocation and count inconsistencies must fail explicitly before persistence.

State version 4, container version 7, postcard field order and stable key order
remain unchanged. Compare complete postcard/JSON and checkpoint bytes against
the owned path, including empty states, sparse full recall, dense multiple
streets, touched zeros, ancestor-only nodes, runtime metadata and resume.
The production checkpoint helper should use the direct writer. Build the final
solution snapshot only when a solution is actually produced; its block count
is already available from final metrics. Solution writing itself still stages
an owned snapshot and is a separate remaining scale cost.

## Validation and measurement

The learning seed-11 comparison continues using its immutable earlier binary
and source archive. Do not run builds or additional solvers concurrently with
its serialized timing processes. After both finish, run workspace formatting,
Clippy and tests plus affected research examples/features on the new source.
Keep a separate source manifest/archive, verification logs and immutable
checkpoint-writing benchmark executable.

`mw_checkpoint_write_bench` supports fresh fixed-budget training or restoring
one retained checkpoint, with separate owned/borrowed processes. Save both
literal jobs before running. Require identical model/config/runtime/counters
and full output-file bytes, using new output paths. Separate construction,
training and write time. The write interval includes snapshot/index setup and
disposal, common serialization/compression/fsync and atomic replacement.

Start with the retained K32 checkpoint at
`runs/simple-depth-coverage-20260910/extended-32768/checkpoint.mwckpt`, its matching
config, 8 threads, 8GiB and the existing warm abstraction cache. A restored
process can reach its lifetime memory peak during loading, before capture.
Therefore preserve both lifetime PeakWorkingSet64 and timestamped current
working-set samples within the write phase. Sampled phase peaks are approximate;
missing samples are unavailable. If initialization masks the reduction, use
a separate predeclared fresh-training pair after this compatibility gate.

The retained-input experiment is fixed to three pairs, each with a 300-second
whole-process timeout: owned-1, borrowed-1, borrowed-2, owned-2, owned-3,
borrowed-3. Preserve every completed trial and compare descriptive medians.

This change targets storage headroom, not strategy quality. No cloud resource
is needed for the initial comparison. The September whole-account USD 20 cap
continues to apply before any later cloud allocation.

## Separate remaining cost found during review

The production loop still retains its previous normalized profile in
`HashMap<InfoKey, Vec<f32>>` for drift. The core already offers
`strategy_drift_refresh_compact`, but production use and its exact dense
aggregation/refresh behavior need a separate review and measurement. Do not
attribute that still-retained map, owned solution staging, or restore-state
allocation to the borrowed checkpoint index. This experiment changes none of
those mechanisms.
