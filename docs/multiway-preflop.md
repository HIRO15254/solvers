# Multiway preflop and blueprint solver

> **Current implementation reference.** The approved target specification for
> Multiway Preflop CLI v1 is `docs/multiway-preflop-cli-spec.jp.md`. During the
> migration, use this document to explain the existing binary and the v1 spec
> as the implementation target.

For a conceptual overview of what this solver computes and why (in
Japanese), see `docs/preflop-solver-overview.md`; this document is the
current-behavior reference. A Japanese translation of this reference is maintained
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

### Check-down thresholds (`max_betting_players`)

Commercial preflop solvers (HRC) avoid the exponential blowup of multiway
postflop betting trees by removing betting from streets that too many players
reach: dense-arena memory is roughly decision nodes × buckets × actions, and
multiway postflop betting sequences dominate the node count, so collapsing
them frees the budget for much finer buckets at the same memory footprint.

`StreetBettingConfig.max_betting_players` (optional `u8`, flop/turn/river
only) is an opt-in per-street check-down threshold. When a postflop street
begins and the number of non-folded seats at that moment — all-in seats
included, matching "players in the pot" rather than "seats able to act" —
is strictly greater than `max_betting_players`, that street has no betting
at all: no decision nodes are created, not even check nodes. Board cards
are still dealt and play proceeds exactly as if every remaining actor had
nothing to do, straight to the next street (which independently
re-evaluates its own threshold) or to showdown. This reuses the engine's
existing actionless fast-forward path (the one already used when every
remaining seat is all-in) rather than emitting check actions, so the
public tree simply has fewer decision nodes; settlement and showdown are
completely unchanged.

Because a checked-down street cannot fold anyone, a later street sees the
same non-folded count and checks down too iff its *own* threshold says so:
a stricter later threshold keeps collapsing, and a looser (or unset) later
threshold re-opens betting even though an earlier street collapsed.
Conversely, if an earlier street *has* betting and folds reduce the count,
a later street's threshold may newly apply where it previously would not
have.

Validation: `max_betting_players` is rejected on preflop (check-down never
applies there); `Some(0)` is rejected ("max_betting_players must be at
least 1"); `Some(1)` is legal and means "always check down whenever two or
more players see this street." Because check-down is a public-tree
property, it cannot legally differ by seat — a per-seat betting override
must repeat the table's `max_betting_players` for a given street exactly
(including agreeing that it is unset) or validation rejects the config,
naming the offending seat and street.

`max_betting_players` is `None` by default and is skipped from a config's
serialized identity (and therefore its game fingerprint) whenever unset, so
every config written before this option existed is unaffected byte-for-byte.

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

Fields of at most 15 players use exact subset dynamic programming. Fields of
16 through 10,000 use deterministic exponential-race Monte Carlo and report
confidence intervals. The off-table field is represented by at most 64
log-stack groups (identical stacks are grouped exactly); each group's total
chip mass is preserved. Only arrivals through the last non-zero payout are
prepared, and table-player ranks are found by binary search in those arrivals.
Prepared-race memory is approximately
`samples * (16 * table_players + 4 * min(outside_players, paid_places))`
bytes and is capped at 1 GiB; an oversized configuration fails before
allocation with guidance to reduce `samples` or paid places.
Larger fields are rejected.

## Abstraction and reproducibility

Preflop observations use the 169 conventional classes. Postflop observations
are clustered separately for one through eight active opponents using expected
pot share, its second moment, and scoop/tie probabilities.  Information sets
retain the complete bucket path.  The artifact seed, rollout parameters,
rules, and centroids form a fingerprint checked by caches and checkpoints.

### Abstraction backends (`game.abstraction.kind`)

`kind` selects the postflop card-abstraction backend. It defaults to
`"rollout-kmeans"` and is omitted from a config's serialized identity (and
therefore its game fingerprint) whenever it is `"rollout-kmeans"`, so every
config written before this option existed is unaffected byte-for-byte.

- **`"rollout-kmeans"` (default).** The trained rollout/k-means abstraction
  described below: opponent-count-aware, with solve-time Monte Carlo bucket
  assignment that is memoized (per canonical situation) and persisted to
  `artifact_cache` after a solve.
- **`"ehs2-table"`.** Precomputed exact E[HS²] percentile tables
  (`abstraction::Ehs2Abstraction`) over *every* canonical board of each
  postflop street, built once and disk-cached at `artifact_cache`. Bucket
  assignment is then an O(1) table lookup with zero solve-time Monte Carlo,
  at any solve scale. Trade-offs relative to the rollout backend:
  - The one-time build enumerates every canonical flop/turn/river board and
    takes on the order of minutes even in release mode; a valid cache at
    `artifact_cache` skips it (the same "load or rebuild and overwrite on any
    mismatch" recovery the rollout artifact uses). The built tables are
    several hundred MB resident in memory.
  - It **ignores** the active-opponent count (a config with `kind =
    "ehs2-table"` and any non-empty `active_opponent_buckets` is a validation
    error) and ignores `rollout_samples`/`seed` (they keep their defaults but
    do nothing).
  - Quality caveat: E[HS²] is a heads-up-vs-uniform-range hand-strength
    statistic with no multiway-specific features (no opponent-count
    conditioning, no scoop/tie modeling) -- it is a cheaper, zero-Monte-Carlo
    alternative, not a strictly better abstraction.

Switching `kind` changes the abstraction fingerprint (naturally -- the two
backends produce different buckets from the same board), so a checkpoint or
`.mwsol` built under one backend is not resumable under the other, the same
way a changed bucket count already isn't.

### v2 rollout: one sample stream per board

Every postflop bucket lookup canonicalizes `(street, active_opponents, hole,
board)` to a suit-isomorphism-minimal key. That key is now **board-major**:
`key.board` is a function of the physical board alone (the minimum over all
24 suit permutations), independent of which hole is being queried, and
`key.hole` is the hero's hole mapped into that same board-canonical suit
space. This lets every hole queried against one physical board share a
single Monte Carlo sample stream instead of drawing its own:

- The stream's RNG is seeded from `(seed, rollout_samples, street,
  active_opponents, board)` -- deliberately **not** the hole -- so it is
  identical for every hero on the same board.
- Samples (runout + opponent hands, drawn with one deck reset plus a
  partial Fisher-Yates shuffle of exactly the dealt cards per sample) are
  generated lazily, in a fixed order, and only
  ever appended to; a hero's query walks the stream from the start and
  extends it on demand, never regenerating or reordering earlier samples.
  This makes generation independent of which heroes are queried, and in
  what order, so a solo lookup and a batched lookup of many combos against
  the same board always agree.
- A hero's own two cards reject (skip) any sample whose dealt cards collide
  with them. Since a hero-conditioned deal is exactly "deal from a
  hero-excluded deck," each hero's own conditional sample distribution --
  and therefore its estimator's statistical quality -- is unaffected by
  sharing the stream; only cross-key correlation between different heroes'
  estimates on the same board is introduced, never bias in any one hero's
  estimate.

This is what backs `MultiwayAbstraction::bucket_batch` (see "Rollout-
abstraction batching" under vector-traverser sampling below): a whole
batch of combos against one board can share one stream and pay the Monte
Carlo cost once instead of once per combo.

The on-disk rollout artifact is version 3, bumped because both the
canonical key's layout and this rollout computation changed underneath it;
version 3 is the only version `read_artifact` accepts. A config pointing
`game.abstraction.artifact_cache` at an older or otherwise-unreadable
artifact is not a hard error: the CLI (and any caller going through
`build_multiway_session`) prints a warning to stderr, retrains from
scratch, and overwrites the file with a fresh version-3 artifact.

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

### Vector-traverser sampling (`algorithm.traverser_vector`)

Ordinary external sampling updates exactly one hand per traversal: the
traverser's own dealt combo. `algorithm.traverser_vector = true` (only valid
with `recall = "street"`; rejected with a typed error under `"full"`, since it
needs the dense arena's precomputed tree) instead updates *every feasible
hole combo* of the sampled traverser seat in one traversal, against the same
sampled opponents and board. "Feasible" means positive weight in the
traverser's configured range and disjoint from every other seat's sampled
hole cards and the sampled board — everything else about the deal stream
(including exactly which combo the sampler happened to deal the traverser)
is unchanged, so switching this on does not change the RNG consumption or
which worlds get sampled.

- **Why it's faster.** The tree is still walked exactly once per traversal;
  only the traverser's own decision nodes do more work (one action
  exploration per node, same as today, but every child value is now a vector
  over the feasible combos instead of a scalar). Terminal evaluation for a
  showdown reruns the same tested pot/rank/rake settlement machinery
  (`settle_ranked`) once per feasible combo instead of building an entire
  second traversal, so the speedup comes from amortizing the walk itself, not
  from a cheaper per-combo evaluation.
- **Bucket aggregation.** At a traverser decision node, each feasible combo
  `h` maps to a per-street abstraction bucket `B(h)` (preflop: the 169-class
  index). The regret matching strategy is looked up once per *distinct*
  bucket reached by the vector (not once per combo). The regret added to
  bucket `b`'s column for action `a` is the feasible-weighted mean over that
  bucket's members: `sum_{h: B(h)=b} weight(h) * (v_a(h) - n(h)) / sum_{h:
  B(h)=b} weight(h)`, where `v_a(h)` is combo `h`'s value under action `a`
  and `n(h)` is its node value under the node's regret-matched strategy.
  Buckets with no feasible member simply never appear and get no update.
  This matches the ordinary scalar external-sampling update in expectation
  over which combo actually gets dealt to that bucket.
- **Average strategy is dense too.** Unlike scalar external sampling (which
  only adds to `strategy_sum` at opponent nodes, on the seat's single sampled
  hand), vector mode accumulates the average strategy at *traverser* nodes,
  over every feasible combo: bucket `b`'s column gets
  `linear_weight * (sum_{h: B(h)=b} weight(h) * own_reach(h)) * sigma_b`,
  where `own_reach(h)` is combo `h`'s own-strategy reach product accumulated
  over the traverser decision nodes visited earlier in the same traversal.
  The vector opponent branch pushes no strategy update at all. This is
  necessary because vector mode runs far fewer sweeps than scalar mode at
  equal wall time (each sweep walks the whole tree once instead of sampling
  one hand); accumulating the average only on opponents' sampled lines, as
  scalar does, would leave it badly under-sampled relative to the (already
  dense) regret updates.
- **Honest approximation.** Every combo's value uses the *same* sampled
  opponents and board — which were themselves sampled from a joint deal that
  originally included a real (but now-unused) traverser hand. Opponent cards
  are therefore drawn from a distribution not conditioned on the specific
  hero combo being valued for other combos than the one actually dealt: a
  small card-removal bias relative to exact scalar external sampling. This is
  the same family of approximation range-based commercial solvers make (they
  do not repeat opponent sampling once per hero combo either); it is not
  claimed to be bias-free, only a documented, deliberate trade for the
  throughput gain.
- **`hand_updates`.** Every traversal's contribution to the new `hand_updates`
  counter (surfaced in metrics/CLI JSON as `handUpdates` /
  `handUpdatesPerSecond`) is `1` for the ordinary scalar algorithm and the
  feasible combo count for a vector traversal — the number to compare against
  a range-based solver's "hands/s".
- **ICM.** Terminal ICM utility is cached by the existing final-stack-vector
  keyed cache (`HoldemGame`'s ICM terminal cache): distinct combos that
  happen to produce the same final stack vector (e.g. many combos tying or
  losing the same way) already share one ICM evaluation for free, without any
  vector-specific bookkeeping. Fields of at most 15 players use exact subset
  DP. Larger fields use a reusable exponential-race approximation: an ICM
  finish order is sampled as `Exp(1) / stack`. Identical outside stacks are
  represented exactly as one group; more than 64 distinct stacks are
  compressed into logarithmic groups that preserve player count and total
  chip mass. The outside-field arrival orders are prepared once, and each
  terminal locates the at-most-nine changed table arrivals by binary search.
  Sampling stops after the last non-zero prize. The start and terminal stack
  vectors share the same races (common
  random numbers), so `ci95` measures the error of the utility difference
  actually consumed by MCCFR. `samples` controls this deterministic,
  seed-reproducible approximation.
- **Rollout-abstraction batching.** A vector traversal needs a bucket for
  every feasible combo instead of just one, so the vector-traverser path
  calls `MultiwayAbstraction::bucket_batch` (via
  `ExternalSamplingGame::buckets_for_combos`) once per node instead of
  `bucket` once per combo. `RolloutKMeansAbstraction`'s batch path looks up
  every combo's canonical key with one assignment-cache lock, and for
  whatever keys miss, builds a single shared Monte Carlo sample stream for
  the batch's one physical board instead of one stream per combo -- see "v2
  rollout: one sample stream per board" above. This is what makes
  `traverser_vector` mode's dominant cost (previously ~99% cold-rollout
  Monte Carlo) amortize across the ~hundreds of feasible combos sharing a
  board instead of re-paying it per combo.
- **Assignment cache growth and the persist cap.** The memoized
  `(RolloutKey -> BucketId)` assignment cache still grows with the number of
  distinct canonical keys visited, and a wide vector-traverser run visits
  many more of them per unit wall-clock time than scalar sampling would.
  Persisting it (`persist_assignment_cache`, called after a solve when
  `game.abstraction.artifact_cache` is set) is capped at 1 GiB rather than
  failing once the cache grows past that: if the serialized artifact would
  exceed the cap, the write drops the tail of the (key-sorted) cache
  deterministically until it fits, and always keeps the centroids/params
  intact. A run that grows the cache past 1 GiB therefore always finishes
  and persists successfully; it just starts the next run with an
  incomplete (but still consistent) warm cache instead of a fully warm one.

### Regret-based pruning (`algorithm.prune`)

Pluribus-style regret-based pruning (RBP), only meaningful in
`traverser_vector` mode: at a traverser decision node, a (bucket, action)
pair becomes a *pruning candidate* once its regret-matched probability is
exactly zero and its accumulated regret has fallen far below a threshold.
Rather than descending into every prunable action's subtree on every
traversal, a candidate is actually skipped (the combos whose bucket is
prunable for that action are dropped from the vector before recursing) with
probability `algorithm.prune_skip_probability` — so roughly 95% of
traversals shrink their combo set at that node, and the remaining ~5% still
explore it in full, keeping the estimate honest and letting a genuinely
recovering action climb back out of pruning.

Three `[algorithm]` keys control it, all optional:

- `prune` (bool, default `false`): enables the feature. Only valid together
  with `traverser_vector = true` — the CLI (`cli::session`) rejects `prune =
  true` with `traverser_vector = false` before ever building the game, and
  the engine (`SolverConfig::validate_setup`) rejects it too as a backstop.
- `prune_threshold` (float, no static default): the regret floor below which
  a zero-probability action becomes prunable. When `prune = true` and this
  key is omitted, it is derived from the game's stakes: `-10.0 *` the sum
  of every seat's starting stack (in bb) for `[utility] kind = "chip-ev"`, or
  `-10.0 *` the sum of the tournament payouts for `kind = "tournament-icm"`.
  The `-10x` scale was calibrated empirically on paired 200k-sweep 6-max
  runs: vector-mode bucket regrets are range-weighted *means* over combos,
  so they grow orders of magnitude slower than the raw per-hand regrets
  behind Pluribus's famously astronomical constant — a `-1000x` variant
  essentially never activated within realistic run lengths (and its
  bookkeeping made runs marginally slower), while `-10x` only admits a
  (bucket, action) after roughly 10k+ sweeps of persistent domination (the
  ratio is stack-depth-invariant, since per-sweep regret deltas scale with
  stack depth too) and measurably sped the paired runs up. Two safety nets
  keep the comparatively shallow default honest: the ~5% exploration below,
  and the batched early-discount events, which scale negative regrets back
  toward zero and so periodically lift borderline pairs above the threshold
  for a full re-check. Must be finite and strictly negative when set
  explicitly.
- `prune_skip_probability` (float, default `0.95`): probability that a
  prunable action is actually skipped on a given traversal, as above. Not
  exposed by the GUI.

Regret floor: whenever pruning is enabled, every regret update is clamped at
`1.05 * prune_threshold` — 5% more negative than the pruning threshold
itself, so a floored regret still satisfies the "below threshold" test
without growing unboundedly negative (which would otherwise both waste `f32`
headroom and slow a pruned action's eventual recovery once its true regret
improves).

Auto mode enables pruning unconditionally (`prune = true`, threshold derived
from stakes the same way as above) as part of materializing
`traverser_vector = true`. Advanced mode defaults new setups to `prune =
false` — the vector traverser is also off by default there, and the CLI
rejects the pruning-without-vector combination — with a "(recommended)"
checkbox for opting in; a config loaded from a TOML without the key likewise
parses as `prune = false` (the CLI's historical default).

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
- `.mwckpt` container version 7 embeds the effective v1 configuration and
  runtime stop state (confirmation count, next evaluation, sample count,
  sequence, and cumulative solve time), which enables configuration-free
  resume. Version 6 added regret-pruning fields and version 5 added the
  vector-traverser state. Readers temporarily accept versions 5-7 for the
  real-data migration gate, filling newer fields with safe defaults for old
  inputs; versions 3-4 fail with an unsupported-version error. Every
  checkpoint this process writes is version 7.
- `.mwsol` stores metadata/public-history recall separately from a sorted
  strategy index.  Each strategy block is an independent checked frame, so a
  Bridge page query reads only the requested blocks.
- `.mwsol` format v4 adds unsigned 16-bit fixed-point strategy encoding
  (denominator 65535 with stable largest-remainder rounding, so each block
  sums exactly to one). Explicit f32 storage remains available; the v3 i16
  form is legacy-only during the real-data migration gate. Readers accept
  v2-v4 and always expose f32 probabilities, while live MCCFR state and
  `.mwckpt` checkpoints remain f32.
- When the policy memory cap is reached, the solver does not evict policy.  It
  ends with `resource_limit` and writes the requested checkpoint; CLI runs
  without an explicit checkpoint derive a `.mwckpt` beside the result (or
  `multiway-resource-limit.mwckpt` when no result path was supplied).

The native egui GUI that previously embedded this solver in-process was
removed in 2026-07 together with the Next.js web workbench; the successor is
a single web-tech GUI bundled as a Tauri app that drives the solver through
the Bridge below (see `docs/app-structure.md`). Its preset TOMLs live on in
`examples/presets/`.

Bridge v2 exposes health/capabilities, validation, create/status/cancel,
result, checkpoint, and paginated strategy endpoints alongside unchanged v1.
Checkpoint responses are streamed from the managed file, and overlapping
result/metrics/checkpoint/solution destinations are rejected before a run.
A browser resume accepts only a managed
`/v2/jobs/{id}/checkpoint` URL from the same Bridge session; arbitrary local
paths are never accepted from web input.  Once an atomic periodic checkpoint
exists, its URL is available even while the solve continues.

## Auto mode: convergence stop rule (phase A)

`[run] stop_dev_gain` turns `run.sweeps` from a target into a safety cap: the
CLI drive loop additionally evaluates the held-out average profile every
`stop_eval_period_secs` of wall time (default `30.0`; independent of, and
layered on top of, the existing `evaluation_cadence`/`checkpoint_every`
boundaries -- it never changes when those fire, it only adds another check in
between). Each stop-rule evaluation takes `U = max` over seats of
`deviation_gain_lower_bound.ci95[1]`; once `U` stays below `stop_dev_gain` for
`stop_confirmations` consecutive evaluations (default `2`), the run stops
early with completion status `"converged"`. The evaluation sample count
starts at `run.evaluation_samples` and doubles (capped at `65_536`) whenever
a check's own max CI *width* still exceeds the threshold -- i.e. the check
could not possibly pass yet regardless of the true value -- logging each
doubling to the same progress stream as the ordinary cadence prints.
`stop_dev_gain`'s unit is the run's own utility unit: bb for chip-EV,
tournament-utility units (compared as-is, no conversion) for tournament ICM.

Each stop-rule check is additionally preceded by a **best-response burst**
(`run.stop_br_traversals`, default `2_000`; `0` disables): for every seat, a
dedicated deviator is trained with that many external-sampling traversals
against the *frozen* current average profile (local sparse regret table;
opponents sample the average strategy; final policy = argmax of the trained
regrets per visited infoset), and the stop-rule evaluation measures *that*
deviator's gain on the held-out samples instead of the default
regret-greedy-on-main-regrets heuristic (which remains the fallback at
infosets the burst never visited). Any fixed deviator evaluated on
independent samples yields a valid lower bound on the seat's best-response
gain, so a trained one only makes the bound tighter — the run stops when
the profile survives a *stronger* opponent, which is a strictly more honest
convergence certificate. The training stream is domain-separated from both
the solve and the evaluation streams and reseeded per check, so every check
retrains against the profile as it currently stands. Ordinary
`evaluation_cadence` metrics rows deliberately stay on the plain evaluator;
stop-rule numbers therefore read systematically higher (tighter) than
cadence rows.

Because the evaluation period is wall-clock rather than sweep-count based,
the exact sweep a converged run stops at is machine-dependent -- a faster
machine fits more sweeps into the same window before the first check, and
every check after that. The stopped sweep count is always recorded in the
run's metrics/result artifacts, so this is fully auditable after the fact,
but it means **bit-reproducible runs must set a fixed `run.sweeps` and leave
`stop_dev_gain` unset**; the two knobs are not meant to be combined when
exact reproducibility matters.

Two config-and-estimator-only helpers used to exist here in support of a
GUI "auto mode" -- `multiway::estimate_dense_arena` (a pre-training
dense-arena size estimate) and `cli::auto_run::derive_auto_run` (which picked
`sweep_batch` and a bucket count from that estimate plus the caller's
thread/memory-budget facts). Both were removed in 2026-07 as dormant
GUI-support code with no production consumer (recoverable from git history
if a future GUI needs them). The stop-rule half of "auto mode,"
`run.stop_dev_gain` (above), remains live in the CLI.

### Strategy purification measurement (`solvers mw-eval --purify`)

`solvers mw-eval <config.toml> --checkpoint <path.mwckpt> [--samples N]
[--seed S] [--purify 0.0,0.05,1.0] [--br-traversals 2000]` restores a
solver from a checkpoint and, for each listed threshold `delta`, measures
the deviation-gain lower bound of the THRESHOLDED average profile:
probabilities below `delta` are zeroed and the rest renormalized (`1.0`
degenerates to argmax = full purification; `0.0` is the unmodified
profile), applied consistently to both the evaluated profile and the
burst-trained deviators attacking it (Ganzfried, Sandholm & Waugh, AAMAS
2012: low-probability actions of an abstraction-derived, sampling-trained
strategy are largely noise, and removing them reduces exploitability).

Measured on a 6-max 100bb auto-shape 200k-sweep checkpoint (4096 samples,
2000-traversal bursts): max deviation-gain CI-upper fell monotonically-ish
from 0.543bb (raw) through 0.402bb (`delta = 0.15`) to 0.208bb at full
purification — 2.6x tighter, mirroring the paper's ACPC result where the
biggest improvement came from full purification. Caveat: purification also
deletes genuinely mixed equilibrium actions, so for range-study output a
moderate threshold preserves real mixes while removing noise; exported
`.mwsol` artifacts remain unpurified (this is a measurement tool).

`--current` additionally swaps the profile under evaluation (and the
deviators attacking it) from the linear average to the LAST-ITERATE
regret-matched current strategy. Plain regret matching carries no
last-iterate guarantee — the average is the object with the CCE-style
bound — but a replication sweep (run lengths 50k–500k, a second solve
seed, a 25bb config, adjacent 200k/210k checkpoints, two evaluation
seeds each) found:

- The PURIFIED last iterate (argmax) was the tightest profile at every
  single checkpoint measured (maxDevUp 0.00–0.18bb, vs 0.14–1.70bb for
  the raw average), including both configs and both solve seeds.
- The RAW last iterate beat the raw average everywhere up to ~200k
  sweeps (roughly 2–3x tighter), but at 500k the comparison became
  mixed (0.57/0.12bb across evaluation seeds vs the average's
  0.34/0.20bb): with enough sweeps the average catches up while the
  current iterate keeps wobbling.
- Adjacent checkpoints (200k vs 210k) gave similar current-iterate
  bounds, so the iterate is not oscillating wildly at that scale.

Caveat on the near-zero numbers: a deviation-gain LOWER bound of ~0.00
against a pure (argmax) profile only says our trained-plus-greedy
deviators found nothing — a true best response may well exploit the
determinism harder. Still, the pattern justifies the roadmap item of a
last-iterate output mode that drops the `strategy_sum` half of the dense
arena entirely — halving arena memory and doubling the affordable bucket
ladder (see the MMD/QRE literature for methods with an actual
last-iterate guarantee); for now the exported profile remains the
guaranteed average, and this stays a diagnostic.

### Auto-materialized sampling and discount settings

Besides the structural knobs above, the GUI's Auto mode also materializes
two convergence-calibrated `[algorithm]` values that differ from the
CLI-side serde defaults (which stay unchanged so existing hand-written TOMLs
keep reproducing byte-identically):

- `exploration_epsilon = 0.0` (CLI default `0.06`): pure on-policy opponent
  sampling. External sampling stays unbiased at `epsilon = 0`, and removing
  the `sigma/p` importance weights measurably reduced estimator variance in
  paired 200k-sweep runs (best final average-positive-regret of every
  variant tested). Trade-off: nodes reachable only through an opponent
  action the profile assigns zero probability stop receiving updates and
  stay uniform there; measured deviation-gain bounds kept improving anyway,
  but this is the knob to revisit if a convergence stop ever plateaus above
  its threshold.
- `discount_every = 10_000` (CLI default `100_000`): the batched
  linear-CFR discount only approximates true per-iteration linear weighting
  at the granularity of its cadence, and at `100_000` it fires just once or
  twice in a typical converged run -- early high-noise regrets barely decay.
  At `10_000` the paired runs reached the coarse-cadence run's final
  deviation-gain bound in roughly three quarters of the sweeps (about half,
  combined with `epsilon = 0`) for ~3.5% extra wall time (each discount
  event is a full arena scan).

## References

- [Lanctot et al., *Monte Carlo Sampling for Regret Minimization in Extensive Games* (NeurIPS 2009)](https://papers.nips.cc/paper_files/paper/2009/hash/00411460f7c92d2124a67ea0f4cb5f85-Abstract.html)
  is the basis for the external-sampling estimator.
- [Gibson et al., *Regret Minimization in Games with Incomplete Information*](https://arxiv.org/abs/1305.0034)
  motivates the explicit product boundary: multiplayer/non-zero-sum profiles
  do not inherit the same Nash guarantee as two-player zero-sum CFR.
