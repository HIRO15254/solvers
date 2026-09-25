# Multiway initialization: next steps (2026-09-09)

The updated goal promotes initialization, resource efficiency, and reliable
long-run resume alongside algorithm and abstraction quality. This note records
candidate work. The tree cache is not implemented. Production new/resume now
uses the existing ordered parallel materializer after serial non-retaining resource admission,
through an explicitly sized construction pool. The parallel merge can still
temporarily retain source and destination node slots, so its node allowance
and policy-arena limit are not a whole-process RSS guarantee.
See the [scaling and long-run plan](../scaling-note.md).

## Current evidence and boundary

The Simple and General K32 screens both use preflop/flop/turn/river aggression caps
`4/1/1/1`. The Simple screen therefore does not gain its smaller tree from a
postflop-cap change. Its expected public-tree reduction comes from the observed
preflop menus: no SB limp, fewer cold-call branches, and fewer opening/3bet jam
branches. K32 reduces policy columns, slots, zeroing, and page commitment; it
does not by itself reduce the number of public betting states.

The completed Simple `none-b4` arm reports:

- process wall time: `255.51496980013326` seconds;
- research timed region: `191.6134826` seconds;
- outside timed region: `63.90148720013326` seconds;
- policy memory metric: `545,720,720` bytes;
- mean position-weighted GTOW frequency MAE: `0.14039131966190538`
  (`14.0391` percentage points).

The timed region includes solving, strategy materialization, fixed-candidate
evaluation, and solver disposal. The `63.901` second difference also contains
process startup, solver construction, and JSON output, so it is not a pure
initialization measurement. Source data is in the
[Simple screen summary](../multiway-convergence-round5-20260909/output/local-simple-algorithm-screen/summary.json).
The large strategy error keeps model and algorithm quality necessary. The
updated goal also requires efficient larger-machine and long-run execution, so
initialization work is now a parallel priority.

## Current repeated work

`DenseStorage::build` in `crates/multiway/src/solver/workers.rs` first runs the
non-retaining, resource-bounded preflight and then enumerates the same public
tree into memory. The recursive passes are `preflight_node` and
`enumerate_node` in `crates/multiway/src/tree.rs`. Arena layout construction,
zero initialization, and explicit page commitment follow those two walks.

The first pass must not simply be deleted. It provides the current guarantee
that an infeasible configuration fails on the arena byte, node, or depth bound
before a complete public tree and policy arena are retained.

## Longer-term candidate: checked PublicTree cache

After one successful preflight and enumeration, store a compact PublicTree in
the existing machine cache directory. The cache identity should include a new
internal tree-cache format/semantic version, the exact game fingerprint, and
the structural depth bound. The payload should be checksummed and published
atomically. A load must reject corrupt data, invalid node/child indices,
duplicate histories, invalid action-label shapes, and any identity mismatch;
`by_history` should be rebuilt from the validated node sequence.

Only immutable public-tree structure is cached. Every solver process must
still allocate, zero, and page-commit a fresh policy arena, and checkpoint and
algorithm fingerprints remain unchanged. This path has the best expected
return for paired seeds and algorithm arms because cache hits avoid both public
state/action walks without changing sampling or update order.

Required tests compare every PublicTree field for fresh and cached builds,
exercise corrupt and mismatched cache fallback, and compare uninterrupted
solver state/checkpoint bytes after a fixed number of sweeps. Cache use must
not change thread-count determinism.

## First implementation candidate: ordered-frontier parallel enumeration

On a cache miss, form a bounded deterministic DFS frontier serially, enumerate
each frontier subtree independently, and merge results in frontier DFS order.
Prefix offsets can rebase local parent/child node IDs while preserving the
existing preorder, HistoryKeys, action labels, and final node IDs. The number
of in-flight subtree results must be bounded so parallelism does not hold a
second complete public tree.

The resource preflight may use the same ordered frontier, but checked totals
must be reduced in DFS order. If a subtree crosses a byte/node limit, rerun
only that subtree serially from the preceding prefix total so the current
first-prefix error and fail-early behavior remain exact.

Required tests compare serial and parallel trees field-for-field, compare all
arena column/slot bases, cover exact and exceeded resource limits, and require
bit-identical solver/checkpoint state across thread counts. Phase timers should
first separate preflight, materialization, arena layout/zeroing, and page
commitment; page-touch parallelization is worthwhile only if that measurement
shows a material share.

## Direct session timing follow-up (2026-09-10)

The fresh Simple K32 average-depth pilot now records session construction
separately: 60.82 s for UniformOne and 60.45 s for EnumerateFirst on the same
source/binary. Their sweep-driver times are 17.14 s and 23.83 s at 4,096 sweeps.
This directly establishes construction as a large short-run cost; it does not
yet separate EHS loading, non-retaining preflight, retained tree materialization,
arena layout/zeroing and page commitment. Retained measurements are
`runs/average-depth-20260910/pilot-0-*/measurement.json` and `stdout.json`.
The [completed phase experiment](../tree-initialization-20260910/README.md)
now isolates the public-state walks. Two serial runs have medians of 29.531 s
preflight, 29.967 s materialization and 60.377 s process time. Two eight-thread
parallel runs have 13.940 s materialization and 44.158 s process time (26.86%
shorter), with identical full-tree and arena-layout digests. Arena layout/zero
allocation is below 0.082 s and explicit page commitment below 0.002 s. The
benchmark excludes EHS, deal sampler and solver wrapper construction.

A single 16-thread run gives only a small additional reduction. Prioritize
public-state traversal and inspect frontier work distribution before spending
more threads. The current planner expands the first DFS frontier up to depth
four; a shallower-first split is a hypothesis, not a demonstrated improvement.

The production integration below retains serial preflight, adopts the existing
parallel materializer and uses an explicitly sized pool for construction and
resume. An ambient Rayon pool cannot override the run thread count, and the
new parallel constructors do not strengthen every game adapter's state
contract. New/resume tests compare tree, arena and fixed-sweep/checkpoint state
at 1/2/8/16 threads, plus exact byte/depth errors; tree-level tests retain the
explicit node-limit boundary. The benchmark's small observed peak increase is
not a general resource guarantee.

Parallel preflight is a later opportunity for the remaining roughly 29.5 s.
Preserve the serial first-prefix failure payload/order: summing independently
rounded arena-byte estimates would duplicate sentinel and touched-bitset
rounding. A checked tree cache has a larger validation surface (semantic
identity, corruption, indices, and each run's resource/depth limits). Neither
is implemented or selected as the next production default.

The prototype's error-path lifetime hardening is now implemented: all parallel
state/results/partial merge buffers leave scope before canonical serial retry.
The merge destination also uses fallible reservation. The phase timings above
remain attached to their original pre-fix binary. This hardening does not by
itself install parallel construction in production or change successful tree
ordering.

## Production integration (2026-09-10)

The constructors now accept explicit operational thread counts through new
core entry points, with `State: Send` required only there. CLI new/resume,
research-session construction and formal `.mwsol` profile reconstruction use
them. Existing serial core constructors remain available. Materialization uses
the exact admitted node count rather than the representation limit; serial
admission and page-commit gates are retained. No algorithm fingerprint, state
version, bucket schedule or numerical update order changes.

Five focused real-Holdem tests compare new/resume at 1/2/8/16 threads, complete
Tree/arena/state/checkpoint bytes and continuation, inclusive memory and depth
errors, and actual requested pool size under a different ambient pool. The
current production measurements and final gates are recorded in
[production initialization evidence](README.md).

Formal `.mwsol` evaluation still builds a session and then reconstructs its
average profile into another arena. Threading that reconstruction reduces its
cost but does not remove the repeated work. A future consuming restore into an
already validated empty arena could reuse immutable structure; it requires
state validation and reset/commit invariants before it can replace this path.

Eight completed Simple K32 production-session measurements show median fresh
construction 59.936 → 45.930 s (23.37% shorter), and checkpoint reconstruction
61.440 → 46.071 s (25.01% shorter) at eight threads. All four outputs per path
match after excluding explicitly timed/operational fields. These gains are now
in the normal CLI; no algorithm/state identity changed. Peak increases are
observed case-specific values, not RAM bounds. All 756 workspace tests and
feature checks pass; see the linked evidence for exact commands and hashes.
