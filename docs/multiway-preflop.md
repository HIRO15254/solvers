# Multiway preflop and blueprint solver

For a conceptual overview of what this solver computes and why (in
Japanese), see `docs/preflop-solver-overview.md`; this document is the
precise reference. A Japanese translation of this reference is maintained
at `docs/multiway-preflop.jp.md`.

A runnable 9-max BBA/ICM configuration is in
`examples/preflop_multiway_9max.toml`:

```sh
cargo run -p cli --release -- solve examples/preflop_multiway_9max.toml \
  --output result.json --metrics metrics.jsonl \
  --checkpoint solve.mwckpt --sol solve.mwsol
```

The multiway path solves two through nine dealt seats without changing the
exact heads-up vector engine.  It is a sampled, generative NLHE game: every
traversal draws one physical set of disjoint hole cards and a shared five-card
runout, reveals the board street by street, and stores policy only for the
abstract observations that were visited.

## Correctness boundary

- Heads-up `engine` / `preflop` remain the exact two-player implementation.
- Multiway uses external-sampling MCCFR with one traversal per dealt seat in a
  sweep.  It is a regret-minimized strategy profile, not a certified Nash/GTO
  solution for games with three or more players.
- Folded hole cards stay in the sampled world, so card removal and bunching are
  represented by the trajectory distribution.  Future board cards never enter
  an information-set key before they are public.
- Buckets compress strategy observations only.  Settlement always uses the
  sampled physical cards, exact seven-card ranks, refunds, and side pots.

## State and settlement

The betting state records each seat's remaining stack, per-street live
contribution, dead contribution, fold/all-in status, pending action, and the
bet level required to reopen raising.  This supports the big blind's option,
short all-ins that do not reopen, and several short all-ins whose cumulative
increase does reopen action for an affected seat.

Preflop configuration distinguishes an unopened raise-to menu
(`bet_sizes`), the raise-to menu after one or more limps
(`isolate_sizes`), and re-raise factors (`raise_sizes`).  Flop, turn, and
river each have independent bet sizes, raise sizes, aggressive-action caps,
and all-in switches.  A seat may replace the complete table betting profile.
If an older config omits `isolate_sizes`, the table reuses `bet_sizes` for
backward compatibility.

### Size vocabulary

Every `bet_sizes` / `isolate_sizes` / `raise_sizes` entry is a `SizeSpec`:

- `to-bb` (`value`): an absolute bet/raise-to size in big blinds.
- `pot-after-call` (`fraction`): `fraction` of the pot as it would stand right
  after the acting seat calls, added on top of that call.
- `previous-bet-multiple` (`factor`, must be `> 1.0`): `factor` times the
  current bet to match (a re-raise multiplier).
- `min-raise`: always resolves to the minimum legal full raise/bet target for
  the node, i.e. the smallest sizing that is not itself capped short by the
  stack.
- `stack-fraction` (`fraction`, must be positive): `fraction` of the acting
  seat's effective all-in (`current street wager + remaining stack`),
  independent of the pot or the current bet to match.

Every proposed target is still bumped up to the minimum full raise (or
dropped, for a voluntary sub-minimum size) and capped down to the seat's
all-in, exactly as before these two sizes were added.

`StreetBettingConfig.allin_threshold` (optional, `(0.0, 1.0]`) adds an
HRC-style raise-cap merge on top of that: once a size resolves to a target
`>= allin_threshold * maximum` (`maximum` = the seat's effective all-in), the
target is replaced by the all-in itself and flagged `all_in = true`. This is a
merge, not an addition — it still fires even when `include_allin = false`,
because it is folding an already-proposed sized target into the all-in
rather than proposing a new action. Duplicate targets (e.g. a merged size and
the native `include_allin` entry) are deduplicated, so the seat sees exactly
one all-in action. Configs that omit `allin_threshold` are unaffected.

At a terminal the implementation:

1. refunds unmatched top contribution;
2. constructs contribution-level main and side pots;
3. removes rake when the selected cash rule requires it;
4. ranks eligible live hands independently for every pot;
5. splits ties and assigns odd chips clockwise from the button; and
6. converts final stacks to chip EV or tournament utility.

A big-blind ante is common main-pot dead money rather than an individual
side-pot cap.  Tournament ICM and per-hand rake cannot be selected together.

## Tournament utility

Tournament inputs contain the dealt seats, any remaining off-table players,
and one payout entry per remaining player (zeroes included).  The baseline is
computed at the start of the hand and terminal utility is the change from that
baseline.  Same-hand busts are ordered by starting stack; equal starting stacks
split the affected payout slots.

Fields of at most 15 players use exact subset dynamic programming.  Fields of
16 through 100 use deterministic finish-order Monte Carlo and report confidence
intervals.  Larger fields are rejected.

## Abstraction and reproducibility

Preflop observations use the 169 conventional classes. Postflop observations
are clustered separately for one through eight active opponents using expected
pot share, its second moment, and scoop/tie probabilities.  Information sets
retain the complete bucket path.  The artifact seed, rollout parameters,
rules, and centroids form a fingerprint checked by caches and checkpoints.

### Recall mode and the policy memory model (`game.abstraction.recall`)

`recall` selects how private information keys the solver's policy storage.
It defaults to `"full"` and is omitted from a config's serialized identity
(and therefore its game fingerprint) whenever it is `"full"`, so every config
written before this option existed is unaffected byte-for-byte. Setting
`recall = "street"` **does** change the game fingerprint: a Street-recall
checkpoint/`.mwsol` is not interchangeable with the Full-recall run of the
same table, and resuming across the two is rejected the same way a changed
betting tree or bucket count would be.

- **`"full"` (default): sparse, full recall.** A `HashMap<InfoKey,
  PolicyColumn>` entry is created the first time a `(public history, player,
  bucket path through every already-reached street)` triple is visited.
  Memory therefore grows with the number of *distinct visited* information
  sets — unbounded in principle, and in practice proportional to sweep count
  until the abstraction/tree is exhausted (measured: ~122 MiB at 4,096
  sweeps growing to ~3.4 GiB by 196k sweeps on a 6-max 64-bucket table). This
  is today's behavior, unchanged.
- **`"street"`: dense, street (imperfect) recall.** Private information is
  keyed by *only* the current street's bucket — the Monker/Pluribus
  convention: earlier streets are never revisited (a speed bonus, since
  their buckets need not even be recomputed) and never appear in the key,
  trading finer strategy conditioning for a hard memory bound. At solver
  construction (or checkpoint resume), the *entire* public betting tree is
  enumerated once (no card dependence: chance is already sampled once,
  outside the public tree) and a single contiguous, node-major
  `[node][bucket][action]` arena of `f32` regrets and strategy sums is
  preallocated for every information set the tree can ever reach — touched
  or not. Memory is therefore **fixed** for the run's lifetime: it is
  computed and checked against `run.max_memory_bytes` *before* the (usually
  multi-gigabyte) arena is allocated, so an oversized tree/abstraction fails
  fast with a typed error naming the node/column counts and the estimated
  byte count, instead of growing until it hits (or blows through) the
  process's memory budget. `infosets` in the metrics stream reports the
  *touched* column count (a running counter, cheap to read), while
  `memory_bytes` reports the constant preflight estimate rather than a
  running total.
- **Tradeoff.** Street recall bounds memory and is faster per traversal (no
  earlier-street bucket recomputation, no per-node hash-map bookkeeping), at
  the cost of coarser, imperfect-recall strategy conditioning — the same
  simplification production solvers like Monker/Pluribus use. Whether that
  costs meaningfully more regret depends on the abstraction and betting tree;
  measure it for a given table rather than assuming either mode dominates.
  Because the arena is sized from the *full* enumerated public tree (not
  just its typical playout), a rich betting tree (many bet/raise sizes, high
  `max_aggressive_actions`, many seats) can make `"street"` mode's
  preallocation infeasible even when `"full"` mode comfortably fits in the
  same memory budget for a normal number of sweeps; the fix is the same one
  the preflight error suggests — shrink the betting tree (fewer sizes, lower
  aggressive-action caps) or bucket counts, or stay on `"full"`.

Deal, action, and evaluation random streams are derived independently from the
base seed, deterministic sample ID, traverser, and sample purpose. Checkpoints
resume without serializing an opaque process RNG. Changing `run.threads` does
not change the ordered sample stream or checkpoint result.

Parallel sweeps give every traverser the same immutable policy snapshot, then
merge local deltas in sample-ID/seat order.  A failed memory-limited sweep is
rolled back as a unit, so no partial sweep can enter a checkpoint.
The cancel token is checked at every complete-sweep boundary without rebuilding
the Rayon pool. `run.max_memory_bytes` is an operational limit rather than game
identity, so a resource-limited checkpoint can resume under a larger budget.

### Sweep batching (`run.sweep_batch`)

A single sweep only offers `num_players` (at most 9) parallel traversal tasks
against one strategy snapshot, which underuses a machine with many more
cores — and table seats are rarely balanced in per-seat traversal cost, so
even that fan-out is uneven. `run.sweep_batch = N` (default `1`) instead runs
`N` complete sweeps against the *same* snapshot as one `N * num_players`-wide
parallel batch, restoring parallel efficiency at the cost of later sweeps in
the batch reading a policy that is up to `N - 1` sweeps staler than the
sequential algorithm would have used — the standard mini-batch MCCFR
trade-off. Every task still gets its own sweep's linear CFR weight
(`completed_sweeps_at_batch_start + sweep_offset + 1`), and deltas are merged
one sweep at a time in strict sweep order, so `sweep_batch = 1` is exactly
the pre-batching schedule: bit-identical checkpoints and thread-count
invariance are unaffected. `sweep_batch > 1` is a deliberate,
algorithm-visible change — it produces a different but equally valid
sampled profile than `sweep_batch = 1` over the same sweep count — and is
recorded in `SolverConfig`, so it is part of a checkpoint's resume identity:
resuming with a different `sweep_batch` is rejected the same way changing
`exploration_epsilon` is. Cancellation is polled once per batch rather than
once per sweep, so `should_continue` granularity coarsens to whole batches.

## Output semantics

Multiway progress uses per-seat profile EV estimates, confidence intervals,
average positive-regret diagnostics, strategy drift, and a held-out unilateral
deviation-gain lower bound.  It deliberately does not reuse the heads-up
`exploitability` or `nash_conv` field names.

The multiway artifact contracts are separate from frozen HU v1:

- `.mwckpt` uses independently compressed 4 MiB frames with a checked chunk
  table and per-frame, table, and aggregate BLAKE3 integrity checks. Postcard
  serialization is streamed through a temporary file instead of duplicating
  the full raw state in memory.
- `.mwckpt` container version 4 adds `sweep_batch` to the serialized
  `SolverConfig`. Loading transparently accepts version 3 checkpoints
  (written before sweep batching existed), filling `sweep_batch = 1`; every
  checkpoint this process writes is always the current version.
- `.mwsol` stores metadata/public-history recall separately from a sorted
  strategy index.  Each strategy block is an independent checked frame, so a
  Bridge page query reads only the requested blocks.
- `.mwsol` format v3 adds an optional i16 fixed-point strategy encoding
  (`run.storage = "i16"`; denominator `i16::MAX` with largest-remainder
  rounding, so each block's quantized probabilities sum exactly to one).
  Readers accept v2 and v3 and always return f32 probabilities; the live
  MCCFR state and `.mwckpt` checkpoints stay f32 regardless of this knob.
- When the policy memory cap is reached, the solver does not evict policy.  It
  ends with `resource_limit` and writes the requested checkpoint; CLI runs
  without an explicit checkpoint derive a `.mwckpt` beside the result (or
  `multiway-resource-limit.mwckpt` when no result path was supplied).

The native GUI (`cargo run -p gui --release`, bin `solvers-gui`) embeds this
solver in-process: Setup (full config editing with live validation and
TOML preset save/load/import/export interchangeable with `solvers solve`
configs), Solve (live convergence charts: per-seat average positive regret
and strategy drift vs sweeps, per-seat EV ± CI at the evaluation cadence,
pause/resume/finish/cancel), and Results (GTO-Wizard-style 13×13 preflop
strategy matrix with per-action stacked frequency bars, public-history
navigation, and `.mwsol` file browsing).

Bridge v2 exposes health/capabilities, validation, create/status/cancel,
result, checkpoint, and paginated strategy endpoints alongside unchanged v1.
Checkpoint responses are streamed from the managed file, and overlapping
result/metrics/checkpoint/solution destinations are rejected before a run.
A browser resume accepts only a managed
`/v2/jobs/{id}/checkpoint` URL from the same Bridge session; arbitrary local
paths are never accepted from web input.  Once an atomic periodic checkpoint
exists, its URL is available even while the solve continues.

## References

- [Lanctot et al., *Monte Carlo Sampling for Regret Minimization in Extensive Games* (NeurIPS 2009)](https://papers.nips.cc/paper_files/paper/2009/hash/00411460f7c92d2124a67ea0f4cb5f85-Abstract.html)
  is the basis for the external-sampling estimator.
- [Gibson et al., *Regret Minimization in Games with Incomplete Information*](https://arxiv.org/abs/1305.0034)
  motivates the explicit product boundary: multiplayer/non-zero-sum profiles
  do not inherit the same Nash guarantee as two-player zero-sum CFR.
