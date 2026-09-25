# Production compact strategy-drift integration

Status: completed implementation, seven verification commands and all six
predeclared measurements passed. 2026-09-10. The broader solution-quality goal
remains active; this is a storage/evaluation-cost improvement.

On one restored dense K32 state with 1,994,775 observed columns, the median
initial capture fell from 0.6433555 to 0.1534594 seconds (76.15%), and an
unchanged-profile refresh fell from 0.5959598 to 0.1473973 seconds (75.27%).
Retained capacity payload fell from 237,627,888 to 49,512,632 bytes (79.16%,
188,115,256 bytes). All six written checkpoints match byte-for-byte, and the
five new core regressions match changing-profile drift values bit-for-bit.
Whole-process lifetime peak stayed essentially unchanged at about 1.95GB;
capacity payload is not working-set memory.

## Implementation and correctness

Production now uses the existing `StrategyDriftTracker` and
`strategy_drift_refresh_compact` at initial capture and every evaluation
boundary, including the final evaluation. Errors receive context and propagate.
The previous profile is still released before final solution staging. Dense
storage keeps column IDs/action counts plus contiguous f32 probabilities;
sparse storage continues to use the legacy map. This phase changes no metric
definition, learning update, RNG sequence, configuration default or artifact
wire format. The frozen `cfr-ref` oracle is untouched.

The five tests in
[`drift_tests.rs`](../../../crates/multiway/src/solver/drift_tests.rs) cover dense
sampled/range-vector changing and stable profiles; earlier column insertions
across all four streets; stored-zero, positive-regret and average-policy
fallbacks; sparse full recall; resumed training and thread equivalence; and
configuration mismatch until explicit reset. Each refresh comparison checks
exact per-seat f64 bits, observed columns/action slots and unchanged solver
state. Stable refresh also retains capacity. First-seen columns contribute
zero drift while remaining in the denominator. These tests cover changes in
state; the performance experiment below measures an unchanged restored state.

## Fixed experiment and all results

The immutable preexecution snapshot and all six literal jobs were written
before the first process. Order was legacy-1, compact-1, compact-2, legacy-2,
legacy-3, compact-3. Every case ran serially in a fresh process with a
300-second timeout, 8 threads, 8GiB and the existing warm `.cache/bench-ehs`
cache. No additional solver training, Cargo build or other solver benchmark
ran during this cohort. Read-only inspection, light file validation and
report editing continued. No GCP resource was started.

The config is the retained 6max 100bb, K32 current-street EHS2 partial Simple
reference model. The input has 32,768 sweeps and 196,608 traversals. Each refresh
observes all 1,994,775 columns and 4,356,732 action slots. Both methods first
capture the profile and then refresh it without training. Their two six-seat
result vectors contain exactly zero f64 bits. After tracker disposal they use
the same borrowed checkpoint writer, isolating representation from output.

| Case, in execution order | Construction s | Initial capture s | Stable refresh s | Retained payload bytes | Lifetime peak bytes | Whole process s |
|---|---:|---:|---:|---:|---:|---:|
| restored-legacy-1 | 45.9234085 | 0.6435979 | 0.5959598 | 237,627,888 | 1,949,724,672 | 48.9282420 |
| restored-compact-1 | 46.5609751 | 0.1625931 | 0.1475049 | 49,512,632 | 1,950,068,736 | 48.2759850 |
| restored-compact-2 | 47.3193296 | 0.1527914 | 0.1469900 | 49,512,632 | 1,949,851,648 | 49.1306038 |
| restored-legacy-2 | 46.5657248 | 0.6399430 | 0.5948963 | 237,627,888 | 1,950,027,776 | 49.5712313 |
| restored-legacy-3 | 46.4785998 | 0.6433555 | 0.5978108 | 237,627,888 | 1,950,134,272 | 49.4490766 |
| restored-compact-3 | 46.4353472 | 0.1534594 | 0.1473973 | 49,512,632 | 1,949,925,376 | 48.1561626 |

All three repeats per method are retained; the following are descriptive
medians, without confidence intervals or a statistical significance claim.

| Measurement | Legacy median | Compact median | Change |
|---|---:|---:|---:|
| Initial capture (seconds) | 0.6433555 | 0.1534594 | -76.1470% |
| Stable refresh (seconds) | 0.5959598 | 0.1473973 | -75.2672% |
| Retained capacity payload (bytes) | 237,627,888 | 49,512,632 | -79.1638% |
| Whole-process lifetime peak (bytes) | 1,950,027,776 | 1,949,925,376 | -0.0053% |
| Session construction (seconds) | 46.4785998 | 46.5609751 | +0.1772% |
| Whole process (seconds) | 49.4490766 | 48.275985 | -2.3723% |

`firstRefreshSeconds` includes capture, while `stableRefreshSeconds` times the
second unchanged refresh. Both timers exclude tracker disposal, shape/payload
accounting, checkpoint I/O and hashing. Shape counts are inspected between the
two calls; the legacy map scan can affect cache warmth before its stable call.
The stable ratios for corresponding pairs are 0.247508, 0.247085 and 0.246562.
This is not a measurement of dense growth during active learning.

Retained payload uses actual map/vector capacities: legacy counts
`map.capacity * (sizeof(InfoKey) + sizeof(Vec<f32>))` plus probability-vector
capacities; compact counts its column/count and probability capacities. Both
omit allocator bookkeeping; legacy also omits hash control bytes. Dense growth
can temporarily keep old and new buffers. Temporary per-column normalization
vectors and infallible allocations remain. This is not a hard process-memory
cap. Lifetime peak is Windows `PeakWorkingSet64` polled every 50ms; the final
unsampled interval may be missed. It covers session restoration, both
refreshes, output and disposal. There is no phase RSS measurement here, and
no claim that the 79.16% payload saving lowers whole-process peak by that amount.

The common session constructor took about 46 seconds and dominates the full
process. Its load/construction allocations can dominate lifetime peak; the
unchanged peak is not evidence that the smaller persistent tracker is free.
Whole-process time medians differ by 2.37%, but three static repetitions do
not establish a learning-throughput or solution-quality improvement.

## Provenance, equality and validation

Retained experiment: `runs/strategy-drift-20260910`. Base revision is
`93c95533dbaca2e8388e82235af5519071fd880f`, with an uncommitted 172-file
source archive. This includes crate Rust/Cargo files, workspace Cargo config/
lock and three compile-time contract documents. It is distinct from the
170-file [borrowed-writer cohort](../checkpoint-write-20260910/README.md) and
167-file [seed-29 learning evidence](../preflop-discount-seed29-20260910/README.md).
The current 172 source files still match the archived manifest after all runs.

| Artifact | SHA-256 |
|---|---|
| Final experiment.json | `5547618e77acb8299a26b7ee32a91c5f5ca8448b6c7ce2d7d71998da7ae6ed37` |
| Immutable experiment-preexecution.json | `ab7f540ac499b85b201726b02bfeeac387e08854c6ee8902e028b23f552054a3` |
| Source manifest, 172 files | `7d461c77f8a34d71eecae514889f232dadda9447104aeae6b21d4e24e788b41f` |
| Source ZIP | `3a4c09686257413ed817ee6a17849fee67bde2d28d20d481cb0c4e02bdc5d227` |
| Immutable release benchmark executable | `db15a9f927fe311e4dbd0bf13fc3cb36e79f08bc96328240a53caf635d6a7767` |
| Verification record and its hashed logs | `fcc6670e753748176371745d22a9d5995a5bda5cb4703941ac3f64d49c47ea75` |
| Input config | `3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a` |
| Input checkpoint | `9a18c0927daf7d1b036558b7c87fa658a3055b6366e263385d776ca3bca0bd0a` |
| All output checkpoints | `2409c23bece81f9f4f2a3ed37278314a79e4ec31a24ec15c784856f8d74bc86f` |
| Summary, regenerated summary and tracked JSON | `bad3312cadae6fc69291ccc851f6e298c43dfc8ef2e1faf72d0aa3a4ff9846ed` |
| Validator | `de0f8d7cddb6aab30225b090617e556a8e2736bf3fdc6b104beb8d7fcc67358f` |
| Validator test script | `e936ded27746b673bdaddf1ac53e173fd98f1aec054869e47edadb9490216a6e` |
| Frozen measurement runner | `1cd3e737e0821f2760ad601fbb67a4d3996a64fedfe311b4e08b3fc687f6c7e7` |

Every output checkpoint is 28,522,138 bytes; direct byte comparison and
independently recomputed SHA-256 agree across all six files. Reported output
BLAKE3 is `a5d8ac24cc715f3f5abb5014c26ff0da292f905b3795876b57a36629fc631711`.
The validator checks its format/agreement, but does not independently recompute
BLAKE3. The input checkpoint is 28,522,123 bytes; no input/output byte-identity
claim is made. Common runtime counters, model/abstraction fingerprints and
metrics match exactly. The complete raw-field comparison excludes only mode,
output path, the three elapsed clocks and retained payload bytes.

The frozen runner records declared input identities but does not itself hash
inputs before and after each process. Inputs were checked before this cohort;
final validation hashes the actual retained files and checks their literal
argument binding. This is a weaker time-of-use record than the newer writer
runner and is not described as that runner's before/after guarantee. The raw
benchmark `sourceRevision` is the manifest SHA; the measurement record's
`sourceRevision` is the base Git revision, intentionally distinct identities.

| Verification command | Result |
|---|---|
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo clippy -p cli --examples --features research-draw-abstraction -- -D warnings` | Passed |
| `cargo test --workspace` | 795 passed, 0 failed, 30 ignored; 48 suites |
| `cargo test -p cli --examples --features research-draw-abstraction` | 39 passed, 0 failed, 0 ignored; 9 suites |
| `cargo test -p multiway --features research-average-sampling --lib` | 274 passed, 0 failed, 1 ignored; 1 suite |
| `cargo build --release -p cli --example mw_strategy_drift_bench` | Passed |

The Rust suites overlap; these counts are not added as unique tests. Compiler:
`rustc 1.97.0 (2d8144b78 2026-07-07)`, x86_64-pc-windows-msvc, LLVM 22.1.6.
The separate Python validator passed 15 tests, including corrupted/incomplete
schedule, input/source/build identity, time ordering, shape/drift mismatch,
output mismatch and payload-versus-peak separation. The validator rechecks the
complete source ZIP, required checks/log hashes, actual input/output identities
and all six serial process intervals. The 36,499-byte full summary was
regenerated byte-identically and copied to the
[tracked JSON](result.json).

Reproduction uses the retained executable and literal jobs with
`tools/run_average_sampling_measurement.ps1 -MeasurementKind CheckpointAudit`;
choose fresh output directories and freeze their job identities before new
measurements. Timer details are in
[`mw_strategy_drift_bench.md`](../../../crates/cli/examples/mw_strategy_drift_bench.md).
To validate retained evidence without running another solver:

```text
python -X utf8 tools/summarize_strategy_drift.py runs/strategy-drift-20260910 --output SUMMARY.json
python -X utf8 -m unittest discover -s tools/tests -p test_summarize_strategy_drift.py -v
```

The production integration is validated. Deep-tree learning quality, runtime
allocation failures, growth costs and larger-state scaling remain separate
work; no tuning default or equilibrium claim follows from this experiment.
