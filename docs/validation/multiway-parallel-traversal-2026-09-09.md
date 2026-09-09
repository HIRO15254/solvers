# Dense vector traversal: completed old/new A/B proof

Date: 2026-09-09. Four paired local comparisons completed with exact numerical
identity and shorter measured times. The measurement used a retained one-shot
research harness; this is not a committed runtime API.

## Boundary and guarantees

The dense `traverser_vector` branch runs regret and independent average-strategy
workers concurrently, then reduces outputs in the former serial order: regret
events first, average events second. See [solver/mod.rs](../../crates/multiway/src/solver/mod.rs).
Both workers read the same immutable world and arena and use separated RNG
streams. Error priority, event order, counters, fingerprints, and checkpoint
representation therefore remain unchanged by the A/B.

Focused tests in [solver/tests.rs](../../crates/multiway/src/solver/tests.rs)
covered thread determinism, checkpoint identity, and regret-before-average
event ordering. Applicable Clippy and workspace tests passed. These checks
validate behavior, not a speed guarantee.

## Fixed experiment and identity

Both arms used the Simple K32 seed-0 fixture, `uniform-one`, batch 1 or 4,
8 GiB arena budget, no discount/pruning, five unopened histories, and
`--skip-evaluation`. Stage 1 used 8 threads and 8,192 sweeps; stage 2 used
16 threads and 16,384 sweeps. Different sweep counts prevent treating stages
as a direct thread-scaling curve.

Old executable SHA-256:
`b1674011a6c338f0b1bb4e33e31c0fa490bbab88b1e560f8bec9c3c7b73c0769`.
New executable SHA-256:
`96ffca8f33e114f5041c19c5d7d0b89c0005917ab1619db29fc6cea5d16c6b66`.
Retained harness source archive SHA-256:
`b7268fbaed21e42da3a31a74286b238a7c77364b354ae1807597ca5cfdbed6b9`.

Every pair matched configuration/abstraction fingerprints, regret fingerprint,
all non-timing metrics, state version, and five histories of 169 rows.
Evaluation was disabled. A mismatch would reject the comparison regardless of
timing.

| threads | sweeps | batch | old timed s | new timed s | speedup | process wall s | sampled peak RSS MiB |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 8 | 8,192 | 1 | 83.674 | 67.937 | 1.232× | 145.907 → 130.466 | 1335.31 → 1335.95 |
| 8 | 8,192 | 4 | 46.913 | 43.935 | 1.068× | 108.710 → 105.710 | 1336.71 → 1336.62 |
| 16 | 16,384 | 1 | 166.938 | 126.021 | 1.325× | 230.846 → 190.281 | 1336.49 → 1338.49 |
| 16 | 16,384 | 4 | 71.709 | 63.523 | 1.129× | 134.209 → 126.054 | 1341.44 → 1342.30 |

The timed region includes training, selected strategy export, and solver
disposal; it excludes EHS preparation and session construction. Process wall
also includes process overhead and serialization. RSS values are periodic
samples, not proof that no unsampled peak occurred. Each row is one timing
observation, not a confidence interval or cross-machine guarantee.

Evidence is retained in [stage1 summary](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/stage1-summary.json),
[stage2 summary](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/stage2-summary.json),
and [monitor summary](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/monitor-summary.json).
The archive hash identifies the measurement harness source; it is not a claim
that a new staged subset or an uncommitted research tool is a public runtime.

This proves numerical preservation and records a bounded one-shot timing
observation. It does not prove convergence improvement, quality improvement,
or general scaling. Further repeated measurements were not launched.
