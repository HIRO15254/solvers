# Multiway initialization: next steps (2026-09-09)

The updated goal promotes initialization, resource efficiency, and reliable
long-run resume alongside algorithm and abstraction quality. This note records
candidate work; the tree cache and parallel enumeration are not implemented.
See the [scaling and long-run plan](../../validation/multiway-scaling-long-run-2026-09-09.md).

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
[Simple screen summary](../../../runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/summary.json).
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

## Candidate 1: checked PublicTree cache

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

## Candidate 2: ordered-frontier parallel enumeration

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
