# Public-tree initialization phase benchmark

This research example measures the existing core initialization phases on a
cash Multiway v1 config. It constructs the real Holdem public state machine,
including the configured tree rules and rake identity, with a census backend
that supplies the config's exact bucket counts, including active-opponent
overrides. It never samples cards or builds/loads EHS tables. Tournament ICM is
explicitly unsupported because preparing its utility runtime is a separate
resource task. No strategy, checkpoint or solution is created.

```text
cargo build --release -p cli --example mw_tree_initialization_bench
target/release/examples/mw_tree_initialization_bench \
  --config experiments/multiway-2026-09/average-depth-20260910/output/config-seed0.toml \
  --mode serial --threads 8 \
  --arena-limit-bytes 1073741824 --max-nodes 2000000 --max-depth 512 \
  --source-revision <immutable-source-id>
```

Repeat with `--mode parallel` using the same executable, config and limits.
On Windows the executable has the `.exe` suffix. Run one process at a time,
after all builds/solves finish, and retain commands, hashes, stdout, exit code,
external process timeout, process wall time and peak working set/RSS. The
example has no internal cancellation clock; use an external timeout for the
whole process (900 seconds is the initial Simple K32 experiment ceiling).

Defaults are serial, one thread, a 1 GiB arena payload limit, 2,000,000 decision
nodes and a depth limit of 512. Threads must be in 1–64. These explicit benchmark
limits apply instead of the config's run/stop/resource settings. They bound
the retained arena/node work; they are not a whole-process memory guarantee.
The public tree, action strings, maps, allocator overhead and parallel temporary
storage require additional RAM.

The JSON has schema `solvers.multiway-tree-initialization-bench/v1` and records
the raw config BLAKE3, actual game fingerprint, executable BLAKE3, caller-supplied
source identifier and structural abstraction config. The census backend does
not claim a production EHS abstraction or solver configuration fingerprint.
`abstractionBackend` is explicitly `census-counts-only`;
`gameFingerprintIncludesAbstraction` is false because the core Holdem game
fingerprint excludes the abstraction config/backend, while retaining betting,
range and economics identity. Compare the structural schedule separately.
Referenced tree scripts affect the lowered game fingerprint; retain the source
package as well as the raw config to reproduce them.

`configAndGameSecs` covers contract validation, lowering and the public-game
constructor. `measurement.timings` reports:

- `poolBuildSecs`: construction of the private Rayon pool.
- `preflightSecs`: the actual serial, non-retaining core byte/node/depth check.
  An infeasible model fails before retained tree or arena construction.
- `materializeSecs`: the selected existing serial or parallel core tree
  enumerator, installed on that pool.
- `buildArenaSecs`: actual core layout arrays and zero-initialized policy
  buffers, including allocation. This is distinct from page commitment even
  when the allocator already faults in pages while zeroing.
- `commitPagesSecs`: actual core page commitment of regrets, strategy sums
  and the touched bitset, without a duplicated benchmark page-touch routine.
- `digestSecs`: identity calculation after commitment, outside every phase
  being compared. It does not warm the policy payload before commitment.
- `releaseSecs`: destruction of arena, tree and pool after identity checks.

These phases omit EHS setup, deal-sampler setup, solver wrapper construction,
strategy export, process startup, executable hashing and JSON output. They
must not be presented as the complete production session's construction time.

The parallel mode measures `tree::enumerate_tree_with_limits_parallel`, also
used by production new/resume after serial resource admission. Production
uses the configured private-pool thread count and exact admitted node count;
this standalone diagnostic uses its explicit benchmark limits. Its bounded
frontier has at most 64 tasks and depth four, and a one-thread pool delegates
to serial. Ordered merge may temporarily retain up to twice the node slots,
plus bounded prefix and active worker capacity. Action/child buffers are moved
rather than cloned. A prototype error releases the complete parallel attempt before rerunning the
serial oracle to preserve error priority and payload; elapsed time can therefore
include that fallback. The merge destination uses fallible reservation and
follows the same released-state retry path if it cannot be reserved.
Neither the arena limit nor this prototype establishes a process-RSS ceiling.

For a successful pair require `measurement.complete = true`, matching game,
config/source/executable identities, counts and both digests:

- `treeBlake3` binds every node in preorder: history, parent and incoming action,
  actor, street, both opponent counts, every action label and child marker/ID.
  It also validates and hashes every history-to-node mapping, without sorting
  or copying the map.
- `arenaLayoutBlake3` binds every node's actual bucket count, column start,
  first/last slot ranges (including action stride), final extents, buffer
  lengths, estimated bytes, touched count and commitment flag. Those bases,
  strides and extents describe the complete contiguous arena layout. It is
  not a checksum of strategy values or allocator capacity.

A failed measured phase emits `complete = false`, `failedPhase` and `error`,
leaves unavailable timing/digest fields null and exits nonzero. A preflight
failure cannot be mistaken for a complete-tree result. Configuration or argument
errors fail before phase measurements and may have only stderr output. Errors
after allocations can release partial data while unwinding; error-phase timing
is diagnostic and is not comparable to successful phase timing.
