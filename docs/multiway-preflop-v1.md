# Multiway Preflop v1 implementation guide

Audience: solver users, maintainers, and AI coding agents  
Normative source: [multiway-preflop-cli-spec.jp.md](multiway-preflop-cli-spec.jp.md)  
Schema: `solvers.multiway-preflop/v1`
Complete TOML reference: [multiway-preflop-toml-reference.jp.md](multiway-preflop-toml-reference.jp.md)

This guide is the compact operational map of the approved specification. The
normative document wins if wording differs. The specification, this guide, and
the TOML reference must be updated together whenever the contract changes; the repository-level
AI instructions in `AGENTS.md` and `CLAUDE.md` make that synchronization
mandatory.

## What v1 means

Multiway Preflop solves 2–9 seat NLHE from preflop through the river with
External Sampling MCCFR. The formal output is the linear average strategy.
For three or more players it is a regret-minimized approximate profile, not a
certified Nash equilibrium, GTO solution, exploitability bound, or convergence
proof. Settlement uses sampled physical cards, exact hand ranks, refunds,
main/side pots, rake, and odd-chip rules rather than abstract bucket equity.

The parser is selected only by the top-level schema:

```toml
schema = "solvers.multiway-preflop/v1"
```

Unknown fields, irrelevant combinations, invalid seats, duplicate overrides,
non-finite values, and BB amounts that cannot be represented in exactly
`.001 BB` are errors.

## Minimal configuration

```toml
schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0

[game.defaults]
stack_bb = 100.0
range = "random"
```

Seat IDs are `0..seat_count-1` clockwise. Defaults apply first and
`[[game.players]]` entries sparsely override them. Player names are not part
of v1 identity. `random` means every legal combo at weight 1.

With standard blinds, three-handed or larger tables put SB and BB immediately
left of the button; heads-up puts the SB on the button. A player-level
`blind_bb` replaces the derived value, including explicit zero. Arbitrary
live blinds and straddles use the same field. Individual antes, common ante,
and live blinds are posted in that order. A short poster goes all-in, while
the nominal maximum blind still determines the preflop call price and minimum
full raise. `preflop_first_to_act` is independent of forced bets and accepts
`"utg"` or an explicit seat.

## Stable defaults

| Area | v1 default |
|---|---|
| Tree | standard: limp/call; 2.5× open/isolation; 3× reraise; all-in |
| Postflop tree | 0.5-pot bet; 0.75-pot raise; all-in; donk allowed |
| Buckets | flop/turn/river = 64/64/64 |
| Abstraction | multiway rollout, 512 samples, seed 0 |
| Recall | current-street |
| Solver | range-vector External Sampling MCCFR |
| Opponent exploration | 0 |
| Batch sweeps | 1 |
| Discount | every 10,000 sweeps through 10,000,000 |
| Pruning | regret-based for range-vector |
| Evaluation | every 10,000 sweeps, 4,096 initial samples |
| Confirmation count | 3 |
| Probability encoding | u16 |

Opponent bucket keys are the non-folded opponent count at street start,
excluding the hero. `ehs2-percentile` is the explicit fast approximation and
rejects rollout samples, rollout seed, and opponent-specific overrides.

Cash is chipEV and has no rake unless `[economics.rake]` exists. Tournament
ICM pads omitted trailing payouts with zero, treats `outside_field_bb` as an
unordered multiset for identity, uses exact subset DP for fields up to 15,
and deterministic Monte Carlo above that. ICM and rake cannot be combined.

## Commands and run directory

Validate before solving:

```text
solvers config new --template minimal
solvers config new --template full --out config.toml
solvers validate config.toml
solvers validate config.toml --format json
solvers validate config.toml --show-effective
solvers validate config.toml --write-effective effective.toml
solvers solve config.toml --out runs/my-run
solvers resume runs/my-run/checkpoint.mwckpt
solvers inspect runs/my-run/solution.mwsol
solvers inspect runs/my-run/solution.mwsol --node call:1000 --view strategy
solvers inspect runs/my-run/solution.mwsol --node 1/0 --view range
solvers inspect runs/my-run/solution.mwsol --node call:1000 --view ev
solvers evaluate runs/my-run/solution.mwsol
solvers export runs/my-run/solution.mwsol strategy --format csv
solvers compare left.mwsol right.mwsol
solvers experiment profile ...
solvers experiment compare ...
solvers experiment benchmark ...
```

A v1 solve accepts one new, empty run directory and reserves:

```text
run.json
progress.jsonl
solution.mwsol
checkpoint.mwckpt
```

Legacy per-file output flags are rejected for schema v1. The run and artifacts
must describe multiway results as approximate profiles and must not label a
sweep/time limit as convergence.

`inspect --node` accepts `root`, a 32-hex-digit public-history key, or a
slash-separated sequence of action labels/action indices. `--view node`
(default) returns the public state plus 13×13 strategy and conditional-range
grids; `strategy`, `range`, `summary`, and `ev` select focused views.
Every strategy cell carries its persisted sampling weight and an explicit
`visited`/`unvisited` status. Missing infosets remain null and are never
reconstructed as uniform. Root EV uses the same trained-deviator evaluation as
`evaluate`. A preflop child-node EV conditions each seat range on the selected
prefix, forces that prefix, evaluates the continuation, and reports sampling
CI. Results are cached beside (not inside) the solution as
`*.mwsol.inspect-cache.json`; the cache key includes solution fingerprint,
node, sample budget, seed, and deviation budget.

`compare` rejects different game fingerprints unless `--cross-game` is
explicit, and cross-game comparison still requires identical seat mapping and
utility units. Solutions with the same abstraction are compared by persisted
infoset. When abstractions differ, the command deterministically samples 1,024
shared real-card worlds and maps each world through both abstractions; bucket
IDs from different abstractions are never compared directly. The result also
reports per-seat EV uncertainty and quality-metric deltas plus both stop
statuses.

## Implementation map

| Contract | Primary implementation |
|---|---|
| Schema routing, defaults, strict validation, lowering | `app/cli/src/multiway_v1.rs` |
| CLI validate | `app/cli/src/validate.rs` |
| v1 run-directory routing | `app/cli/src/solve.rs` |
| Artifact inspect/evaluate/export/compare | `app/cli/src/multiway_artifact.rs` |
| Self-contained resume | `app/cli/src/resume.rs` |
| Fixed chip unit and seats | `crates/multiway/src/types.rs` |
| Forced contributions and first actor | `crates/multiway/src/config.rs`, `betting.rs` |
| Physical deals and abstraction | `crates/multiway/src/sampler.rs`, `abstraction.rs` |
| Pots, refund, rake, awards | `crates/multiway/src/settlement.rs` |
| ICM | `crates/multiway/src/icm.rs` |
| MCCFR, evaluation, deterministic merge | `crates/multiway/src/solver/` |
| Checkpoint and solution formats | `crates/multiway/src/checkpoint.rs`, `crates/formats/src/mwsol.rs` |

## Implemented contract and migration gate

The dedicated v1 parser never silently falls back to legacy behavior. The
standard typed-rule frontend and the deterministic `.mwtree` frontend compile
to the same priority/source-ordered rule IR. Selectors cover position, IP/OOP,
player/limper/flat/aggression counts, unopened, squeeze, c-bet, donk, and SPR.
`add`, `remove`, `replace`, `force`, and actionless `checkdown` execute in the
public tree. Supported size literals are BB targets, pot fractions, current-bet
multiples, minimum, all-in, effective/actor-stack fractions, and geometric
all-in sizing. Script paths are resolved relative to the config directory.
During normalization/solve, an external script is deterministically expanded
into standard typed rules in the effective config, so checkpoints and solutions
do not depend on the original `.mwtree` file.

Generic rake supports the complete public-state `when` expression, optional
cap, down/nearest/up `.001 BB` rounding, and main-first or proportional
side-pot allocation. Tournament ICM and rake remain mutually exclusive.
`range-vector` works with both current-street and bucket-history recall; the
bucket-history implementation uses sparse weighted per-combo updates.

The v1 run directory writes JSON schema v3 with normalized effective config,
fingerprints, guarantee and unit metadata, quality, and timestamps.
`progress.jsonl` has a monotonic sequence across resumes. `.mwsol v4`
contains the effective config, config/game/algorithm/abstraction/configuration
fingerprints, successful stop status, chip unit, seat EV/CI, the complete
public tree and public states, one typed legal-action table per decision node,
and only visited average-strategy blocks with their strategy weights. Strategy
frames contain keys and probabilities only, so action strings are not repeated
per private bucket. Full-recall bucket paths retain an explicit unreached
sentinel. The default unsigned-u16 encoding has denominator 65,535 and uses
largest remainder so every distribution sums exactly; explicit f32 is also
available, while signed i16 is rejected. Result JSON, solution, and checkpoint
writes are atomic. `.mwckpt v7` embeds the source config, stop/evaluation state, and
cumulative solve time; `solvers resume checkpoint.mwckpt` needs no config and
accepts thread, memory, cumulative-time, maximum-sweep, stop-target,
evaluation-budget/cadence, checkpoint-cadence, and output-directory overrides.
Changing the stop target resets the consecutive-confirmation counter.

The first SIGINT sets a cooperative cancellation token. The solver commits
only complete sweeps, writes the cancellation checkpoint/run state, omits the
formal solution, and exits 130. A second SIGINT force-exits immediately.
Successful target/sweep/time stops exit 0; runtime/I/O failures 1;
configuration/validation failures 2; artifact version/fingerprint failures 3;
resource preflight/runtime limits 75; and user cancellation 130.

The compatibility readers for `.mwsol v2/v3` and `.mwckpt v5/v6` remain behind
the normative real-data migration gate. They must be deleted once that gate is
recorded as complete; unsupported versions must never be guessed or silently
converted.

## Change checklist

When the contract changes:

1. edit the normative specification first;
2. update the TOML reference and the corresponding section/defaults table in this guide;
3. update the v1 typed parser and semantic validation;
4. update runtime behavior rather than only accepting the key;
5. add rejection, normalization, and end-to-end behavior tests;
6. update examples, help, fingerprints, checkpoints, and artifacts if
   observable;
7. run formatting, clippy, and workspace tests;
8. verify that removed options are absent rather than deprecated aliases.

