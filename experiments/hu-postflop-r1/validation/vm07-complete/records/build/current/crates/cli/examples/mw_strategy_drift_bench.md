# Strategy-drift storage benchmark

Build with `cargo build --release -p cli --example mw_strategy_drift_bench`.
The example restores a production Multiway checkpoint, captures the previous
profile once and refreshes the unchanged profile once. It performs no training.

Required arguments are `--config FILE --checkpoint FILE --output NEW_FILE
--mode legacy|compact --memory 8GiB --source-revision SOURCE_MANIFEST_SHA`.
Use `--threads 8` (default 8; accepted range 1..64) and `--cache-dir DIR` to
fix the production constructor resources and abstraction cache. All configured
game, abstraction and algorithm compatibility checks still apply on restore.

Run each mode in a fresh process. Save literal arguments and input/source/binary
identities before execution. The existing `tools/run_average_sampling_measurement.ps1`
can capture the whole-process observation with `-MeasurementKind CheckpointAudit`.
Its lifetime memory peak includes restore, both refreshes, output and disposal;
it is not a measurement of the drift phase alone.

`firstRefreshSeconds` includes initial profile capture. `stableRefreshSeconds`
measures a second refresh at unchanged state. Both exclude tracker destruction,
payload accounting, checkpoint I/O and output hashes. The column/slot counts
are inspected between the two timed refresh calls.
`retainedPayloadBytes` counts actual retained capacities of keys/vectors/probabilities, but omits
allocator overhead and HashMap control bytes. It is not current working set or
a total memory bound. Dense compact growth can still retain old/new buffers
temporarily, and normalization has per-column temporary allocations.

Both modes require exact zero per-seat drift bits, matching observed column
and action-slot counts. After dropping the tracker, both save an identical
checkpoint through the borrowed writer; compare complete bytes, not just
configuration identities. Dynamic drift and resume equality are checked in
the core regression suite. This storage benchmark cannot establish strategy
quality, convergence, or an equal-time learning improvement.
