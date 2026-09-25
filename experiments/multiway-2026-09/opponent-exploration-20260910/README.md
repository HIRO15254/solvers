# Multiway opponent exploration: raw-support pilot (2026-09-10)

Status: completed fixed-seed pilot; no parameter promoted.
At epsilon 0.06, the selected three-player turn gains nonzero regret support
at almost the same measured driver cost. Epsilon 0.25 creates more stored
columns there but all their regrets remain zero. Every setting still lacks
columns at the exact three-player river. This is a parameter screen, not
evidence of balanced strategic convergence.

## Why this experiment

The [corrected conditional audit](../preflop-proposal-20260910/README.md)
made a rare three-player river measurable, but found only missing-column
uniform fallback along its sampled continuations. The production v1 config
already exposes `solver.opponent_exploration`, with default zero. Its epsilon
mixture and importance corrections already exist in all three regret workers;
no new sampling algorithm or default is introduced here.

At an opponent node `q(a)=(1-epsilon)*sigma(a)+epsilon/K` and
`rho=sigma(a)/q(a)`. The prefix weight passed to descendants is multiplied by
rho and the returned utility is also multiplied by rho, accounting for
different path segments around an updated information set. The range-vector
update retains the existing feasible-combo weights, without bucket-wise
renormalization. An exploration action with sigma=0 cannot contribute a
nonzero descendant regret update. The current merge can nevertheless mark a
column touched after that all-zero event. Touched counts alone therefore
cannot demonstrate learning progress.

## Implementation and safeguards

The audit example accepts `--fresh-sweeps N` in place of `--checkpoint PATH`.
It constructs a fresh production solver, uses the unchanged ordinary sweep
driver and freezes the result for evaluation. No solver artifact is written.
It records actual solver config, final metrics, effective-config hash and a
sweep-only clock in `freshTraining`. The research budget is this exact sweep
count plus an external timeout; config run stop/output schedules do not drive
the loop. Existing checkpoint mode retains its old output when new options
are absent.

Optional `--support-node PATH` reports every expected current-street bucket,
not just stored/average-positive rows. It obtains bucket cardinality from the
street-start opponent context, while using current opponents for InfoKey
identity. Raw regret and strategy-sum arrays retain action-label order.
Missing columns have null values. Stored zero-average columns have null
average strategy, not a substituted current-regret strategy.

Statuses distinguish missing, stored all-zero, stored nonpositive nonzero,
and stored positive regrets. Counts separately track positive average mass
and its intersection with nonzero regret support. Equal positive regrets,
all-zero regrets and nonpositive regrets can each produce uniform normalized
strategies. Zero raw regrets do not prove no updates occurred; nonzero raw
regrets do not count updates or prove accuracy. Average mass contains a fixed
public-history proposal factor and is not comparable across different histories.

Tests exercise that distinction, invalid/overflowed columns, all-street
export versus snapshot columns without mutation, and mutually exclusive
fresh/checkpoint budgets. A separate dense scalar/vector regression samples
a zero-target opponent branch and verifies zero descendant updates, then
shows that merging them creates touched fallback state without numeric regret
support. The same random stream with a positive target is a nonzero control;
an average traversal is deliberately absent from this test.

## Predeclared measurement design

The [pilot plan](plan.md)
fixes seed 0, 32,768 sweeps and epsilon 0 / 0.06 / 0.25. The game is the same
partial Simple K32 6-max 100bb configuration, range-vector, batch 4, no
discount/pruning, eight threads, 8 GiB arena budget and warm EHS cache.
Changing epsilon changes the normal solver fingerprint, so each run is fresh.

Fourteen support nodes balance opener and facing-3bet/4bet/5bet decisions,
HU 3bet/4bet flop/turn/river, and UTG/HJ/CO three-player flop/check-through
turn/river. Conditional diagnostics use five prefixes and three preflop trunks,
131,072 accepted worlds per seed per trunk, with held-out seeds 101/202.
Ordinary coverage separately uses 131,072 worlds per seed; preflop node
frequencies also use 131,072 root worlds. Tiny incidental candidate diagnostics
(128 worlds, one fit traversal per seat) are not a meaningful BR test.

Every job has a 900-second timeout. Jobs run locally, serially after Cargo
checks/build, with retained config, literal arguments, immutable source/binary,
raw output, process wall time and peak working set under
`runs/opponent-exploration-20260910/`. A candidate exceeding 2x the baseline
sweep-driver time fails the pilot cost gate before additional training seeds
are assigned. This gate says nothing about accuracy.

## Interpretation boundary

Each learned profile changes preflop posterior and suffix behavior. Its
conditional EV or source coverage is a different conditional population;
shared evaluation seed ids do not make different proposal draws paired.
Report seeds separately. Additional numeric regret support is useful for
choosing a follow-up, but promotion requires held-out unilateral-deviation
evidence, at least three training seeds and calibrated-compute controls across
the balanced depths. The production default remains zero exploration.

No GCP resource is used in this pilot. This does not establish the account's
September bill or unused budget.

## Measured cost and exact baseline regression

| Exploration | Sweep driver (s) | Driver ratio | Construction (s) | Whole process (s) | Peak working set (bytes) |
|---:|---:|---:|---:|---:|---:|
| 0 | 131.685 | 1.0000x | 45.787 | 274.649 | 1,416,806,400 |
| 0.06 | 132.661 | 1.0074x | 45.443 | 282.080 | 1,417,252,864 |
| 0.25 | 132.561 | 1.0067x | 46.128 | 283.609 | 1,415,970,816 |

Both candidates pass the predeclared 2x cost gate. This is one timed run per
setting, not a statistically precise claim that either changes driver cost by
less than one percent. Whole-process time includes all audits; this is not an
isolated sampling-kernel comparison. No calibrated-compute controls were run.

Fresh epsilon 0 exactly matches all eight overlapping conditional prefix/seed
rows from the retained 32k checkpoint audit, including full contexts, proposal
metadata, utility/error estimates, source fractions and weights. The extra HU
3bet prefix has no counterpart in that particular reference. This validates
the fresh driver against an actual retained state without creating another
large checkpoint.

## Raw policy support across depths

Every cell is **nonzero-regret buckets / positive-average buckets**, with the
common total bucket count shown separately. These are counts at exact nodes;
they are not fractions of root trajectories. All nonzero-regret columns at
these selected nodes also have positive regret in at least one action.

| Exact decision history | Total buckets | epsilon 0 | epsilon 0.06 | epsilon 0.25 |
|---|---:|---:|---:|---:|
| UTG unopened | 169 | 169 / 169 | 169 / 169 | 169 / 169 |
| SB unopened | 169 | 169 / 169 | 169 / 169 | 169 / 169 |
| SB facing 3bet | 169 | 169 / 167 | 169 / 169 | 169 / 166 |
| BB facing 4bet | 169 | 169 / 120 | 169 / 120 | 169 / 146 |
| SB facing 5bet | 169 | 169 / 90 | 169 / 139 | 169 / 121 |
| HU 3bet-call flop | 32 | 32 / 32 | 32 / 32 | 32 / 32 |
| HU 3bet check-through turn | 32 | 32 / 32 | 32 / 32 | 32 / 32 |
| HU 3bet check-through river | 32 | 32 / 32 | 32 / 32 | 32 / 32 |
| HU 4bet-call flop | 32 | 32 / 32 | 32 / 32 | 32 / 32 |
| HU 4bet check-through turn | 32 | 32 / 27 | 32 / 30 | 32 / 29 |
| HU 4bet check-through river | 32 | 32 / 9 | 32 / 19 | 32 / 16 |
| Three-player flop | 32 | 32 / 32 | 32 / 30 | 32 / 30 |
| Three-player check-through turn | 32 | 0 / 21 | 29 / 18 | 0 / 27 |
| Three-player check-through river | 32 | 0 / 0 | 0 / 0 | 0 / 0 |

At the three-player turn, stored-column counts are 21 / 29 / 32 for epsilon
0 / 0.06 / 0.25. Thus epsilon 0.25 would look best by touched coverage or
average-column count, even though none of its 32 columns has a nonzero regret.
Epsilon 0.06 has 29 nonzero positive-regret columns with nonuniform current
strategies, but only 18 positive-average columns. The exact three-player
river has no stored columns under any of the three settings.

The HU 4bet river has a different limitation: all settings have nonzero
regrets across all 32 buckets, while average support is only 9 / 19 / 16.
The full 3bet node support does not assert coverage of all later 3bet branches.
At the three-player flop, the exploration candidates reduce positive-average
counts from 32 to 30. Support benefits therefore do not extend uniformly
across the requested depths, and numeric support is not a strategy-quality
metric by itself.

## Within-profile conditional source observations

Each cell below lists evaluation seeds **101 / 202**, separately. These are
river average-use percentages in decisions at/below each prefix. A suffix
starting with three players may later become HU; the exact checked-through
river itself retains three active players. Prefix populations differ across
learned profiles, so differences are not paired treatment effects.

| Prefix | epsilon 0 | epsilon 0.06 | epsilon 0.25 |
|---|---:|---:|---:|
| HU 3bet-call flop | 79.86% / 79.94% | 78.45% / 78.36% | 80.24% / 80.14% |
| HU 4bet-call flop | 54.85% / 54.85% | 60.46% / 60.53% | 62.59% / 62.55% |
| Three-player flop | 44.64% / 44.32% | 43.07% / 43.09% | 33.62% / 33.84% |
| Three-player check-through turn | 7.58% / 7.17% | 16.42% / 16.85% | 14.81% / 14.50% |
| Three-player check-through river | 0.00% / 0.00% | 0.00% / 0.00% | 0.00% / 0.00% |

From the three-player flop, missing-column river fallback is 42.42% / 42.60%
at epsilon 0, 20.13% / 20.29% at 0.06, and 46.37% / 46.07% at 0.25.
Epsilon 0.06's current-regret fallback increases to 36.80% / 36.62%.
Stored current-regret fallback can still be a normalized uniform strategy;
suffix-wide source counts do not prove positive regret at every visited key.

The exact three-player-river prefix ESS is 12,065 / 11,904 at epsilon 0,
618 / 645 at 0.06, and 38,215 / 38,281 at 0.25. Equal nominal conditional
budgets therefore give very different precision once postflop check weights
change. Every setting observes uniform-only continuations at this endpoint.
Larger ESS is an estimator property, not stronger solver strategy. The full
JSON retains errors and maximum weights. Relative weight means must not be
read as absolute root-reach ratios across settings.

## Decision and next quality gate

Keep the production default at zero. Epsilon 0.06 is a candidate for further
quality checks because it creates numerical three-player-turn support at
acceptable cost in this seed. Do not promote it based on this screen. Give
epsilon 0.25 lower priority for this target: its additional stored/average
columns do not come with numerical regret support here. This is a single-seed
screening decision, not a statistical ranking of the settings.

First establish the [one-step endpoint deviation diagnostic](../endpoint-deviation-20260910/plan.md)
on the frozen epsilon-0 reference. It must fit legal own-information actions
on separate worlds, retain unsupported keys as baseline, and measure a signed
held-out gain over all prefix weight. Low retained-key coverage must not be
misread as a strong baseline. Epsilon 0.06's small river ESS reinforces that
requirement. Later compare candidate settings across at least three training
seeds and calibrated compute, separating a fixed-reference population from
each profile's own conditional population. No global EV, exploitability or
balanced convergence improvement has been established by this pilot.

## Verification and reproducibility

Fmt and workspace clippy pass. Workspace tests: 766 passed, 30 ignored,
48 suites. Feature example tests: 36 passed. Targeted average-research tests:
6 passed. Release audit build passed. The new zero-target exploration test
also passed independently. The immutable Rust/Cargo source snapshot is the
final checked source; the measurements ran after all Cargo work completed.

The [machine-readable report](result.json)
contains all raw support rows and conditional outputs, preflop frequency
estimates, budgets, clocks and verification records. The summary validator
checks f32 reconstruction before comparing raw mass/normalized policies,
missing bucket retention, full contexts, separate seed/trunk budgets,
config-only-parameter differences, source/config/binary/log hashes and the
retained baseline equality. It refuses incomplete cases.

| Retained input | SHA-256 |
|---|---|
| 161-file Rust/Cargo source manifest | `cfd99488c314717a85d16d9c302efbfb033c02ff3890b4827d3c42a19c7ab4cb` |
| Source ZIP | `ba9da6459404db2a47d9fc8268c30d12ed9b2434b0da353c8afd22361d2a2bab` |
| Release audit executable | `c2e42453954e676baeb4d5375f237726dfcabe9d6b602f373a18df1058e1345d` |

All three config hashes, job arguments, timeout/status records and raw output
hashes are in the retained run. The base commit is
`93c95533dbaca2e8388e82235af5519071fd880f`; the measured source is the recorded
uncommitted snapshot rather than that commit alone. The local environment
uses release MSVC `target-cpu=native` on the recorded Windows machine.

Regenerate the report from repository root:

```text
python -X utf8 tools/summarize_opponent_exploration.py --run runs/opponent-exploration-20260910 --output docs/validation/multiway-opponent-exploration-2026-09-10.json
```

Use the retained immutable audit executable, literal jobs and
`tools/run_average_sampling_measurement.ps1 -MeasurementKind CheckpointAudit`
with fresh output directories to repeat measurements. Keep cases serial and
avoid concurrent Cargo work. No additional GCP resource was launched.
