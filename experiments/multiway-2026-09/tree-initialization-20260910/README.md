# Multiway initialization phase measurements (2026-09-10)

Status: five local measurements are complete. The existing parallel materializer
reduces this benchmark's median process time by 26.86% at eight threads,
with identical complete public-tree and arena-layout digests. At this measured
stage, production `DenseStorage::build` used serial preflight and materialization.
These numbers are prototype evidence, not a deployed solver speedup. The later
[production integration](../production-initialization-20260910/README.md)
records separate source/binary identities and real session measurements.

## Fixture and measurement scope

The fixture is the partial GTO Wizard Simple reference, six seats, 100 bb,
NL50, current-street K32 with exact preflop/postflop bucket schedules. The
benchmark uses the real cash Holdem public state machine and rake identity,
with a census-only abstraction. It does not build/load EHS tables, sample
physical worlds, create a solver, or write checkpoints/solutions. The game
fingerprint does not include abstraction; the structural schedule is recorded
and checked separately.

All cases use one immutable release binary/config/source package, a 1 GiB
policy-arena bound, 2,000,000-node limit, depth 512 and an external 900-second
timeout. Config run/stop settings do not control this diagnostic. Runs are
serial in the table order, without overlapping builds/solves. The eight-thread
pool is also created for serial mode, but that mode enumerates on one thread.
Only one 16-thread observation was taken; no scaling confidence interval can
be inferred from it. No GCP resources were started.

The phase timers exclude digest and disposal unless named. Whole-process
wall time includes all phases, executable hashing, output and disposal. Peak
working set uses Windows lifetime `PeakWorkingSet64` queried every 50 ms;
the final unsampled interval can be omitted. Arena bytes do not bound process
RAM, public-tree storage, allocator capacity or parallel merge temporaries.

## Results

| Case | Preflight s | Materialize s | Arena allocation/layout s | Page commit s | Process s | Peak MB |
|---|---:|---:|---:|---:|---:|---:|
| serial8-first | 29.737 | 30.026 | 0.0817 | 0.0017 | 60.696 | 829.198 |
| parallel8-first | 29.475 | 14.073 | 0.0733 | 0.0010 | 44.416 | 833.196 |
| parallel16 | 29.482 | 13.245 | 0.0737 | 0.0013 | 43.566 | 835.957 |
| parallel8-repeat | 29.242 | 13.806 | 0.0733 | 0.0011 | 43.899 | 833.229 |
| serial8-repeat | 29.324 | 29.908 | 0.0733 | 0.0011 | 60.059 | 828.854 |

MB is decimal. The JSON retains exact bytes and unrounded timing for every
phase, including pool construction, digest and release.

The two serial runs have median materialization 29.967 s and process
time 60.377 s. The two eight-thread parallel runs have 13.940 s and
44.158 s: materialization is 2.150x as fast (53.48% less time), while the
complete diagnostic is 26.86% shorter. Median observed peak increases by
4.186 MB. These are case-specific observations, not bounds for larger trees.

The 16-thread run reaches 13.245 s materialization and 43.566 s process time.
Its small additional improvement over eight threads does not establish enough
benefit to justify doubling allocated compute. The serial preflight still
takes roughly 29.5 s and dominates the parallel case. Arena allocation/layout
is below 0.082 s, and explicit page commitment below 0.002 s; optimizing those
phases is not the priority for this K32 case. Allocator zeroing may already
commit pages, so the explicit page timer alone is not total page-fault cost.

## Exact identity and retained evidence

All five cases have 966,141 decision nodes, 996,753 terminal edges,
31,854,277 columns, 64,818,941 action slots and 545,720,720 arena bytes.
Every node field, ordered action/child list and history mapping is bound by
the tree digest. Bucket counts, all contiguous bases/strides/extents, buffer
lengths and commitment status are bound by the arena-layout digest. These
digests do not certify policy values or solver state.

- Tree BLAKE3: `c28b5f82c6cf9de416f316d0af7fff656862b875163787ea90c81bf2620b2530`.
- Arena layout BLAKE3: `ccd7f7c611a11d546e0799ad11158fa290ab91f4936f2e9027b7132a9855c447`.
- Source manifest SHA256 (158 Rust/Cargo files): `f7ba2f6be2446daae15ef07286b669bd0ed94db47270b68b153d98180c1798ea`.
- Release binary SHA256: `5580b62e67fe15e6f14120f17a481456a80df6a5585d65d6a2c1eff13de89536`.

Retained inputs, source archive, five job JSON files, stdout/stderr and process
metadata are under `runs/tree-initialization-20260910/`. Each file hash is
checked when constructing the [evidence JSON](result.json).
The ZIP contents match the immutable source manifest. The average-depth
experiment uses its earlier 157-file source package and binary; do not relabel
those historical measurements with this 158-file source identity.

## Error-path hardening after measurement

The immutable benchmark binary above predates a focused prototype fix. The
old error paths could invoke serial retry while planning states, completed
subtrees or a partially merged destination remained in scope. The parallel
attempt now owns all of those values inside a helper and returns before the
serial oracle runs, releasing temporary data first. The extra merge destination
also uses fallible reservation; failure uses the same released-state retry.
The successful traversal order and production constructor remain unchanged.

A regression adapter checks that no planning-state references remain when the
serial retry constructs its root, and verifies canonical global label errors
and node-limit precedence. This directly tests planning-state lifetime; Rust
helper ownership provides the corresponding boundary for result/merge buffers.
A separate capacity-overflow test verifies typed merge-reservation failure
without requesting physical RAM. Existing serial/parallel identity tests cover
successful trees. This is not a new post-fix performance measurement or a
whole-process allocation-failure guarantee.

## Decision and verification

Retain the non-retaining byte/node/depth preflight: deleting it would allow an
infeasible model to retain the full tree before failing its arena budget.
The existing parallel prototype proves that public-state materialization can
benefit from concurrency. It uses a bounded shallow frontier and an ordered
merge, with a serial error fallback. Production adoption must still validate
resource behavior, temporary memory, and identical solver/checkpoint state.
The next bounded investigation should target the public-state walks and
frontier balance, or a checked immutable-tree cache that avoids repeated
walks. This evidence alone does not select a new production default.

After the error-path fix, all required format, workspace clippy and workspace
test gates passed (750 passed, 30 ignored, 48 suites). Feature-gated CLI clippy
and 29 example tests passed, including five initialization tests for complete
serial/parallel identity, hash/layout sensitivity, resource failure and
argument/configuration handling. Six focused average-sampling core tests and
12 Python summary tests also passed. The two new tree tests account for the
increase from the benchmark stage's 748 workspace tests. Logs, exact commands
and SHA256 hashes are retained under `final-verification/`, linked in the
evidence JSON. `hardening-source-manifest.json` and `hardening-source.zip`
identify the later tested source separately from the measured source.

See the [benchmark guide](../../../crates/cli/examples/mw_tree_initialization_bench.md)
for timing/resource scope and reproduction, and the
[initialization plan](../production-initialization-20260910/plan.md)
for remaining implementation gates.
