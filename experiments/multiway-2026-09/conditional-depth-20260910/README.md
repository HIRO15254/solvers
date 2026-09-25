# Conditional diagnostics for rare multiway branches (2026-09-10)

Status: complete. Read-only conditional evaluation implemented and verified;
three retained cases passed validation. Strategy-learning work remains open.
This diagnostic measures the frozen profile; it is not a training improvement,
EV-error estimate, best response or convergence certificate.

## Method and interpretation

For each held-out physical world, force a requested action-index path and
multiply its baseline action probabilities to obtain `w`. Sample an ordinary
baseline continuation below the endpoint. The underlying configured ranges
include card removal from every seat, including players that folded.
The root chance distribution, abstraction and baseline fallbacks are unchanged.
The path probability mirrors the existing sampler's cumulative f32 intervals,
including clipping and the last-action residual. Forced actions consume the
same action-stream draws; prefix list order does not change a result.

Report root reach as `mean(w)`, and a conditional utility or trajectory rate as
`sum(w * Y) / sum(w)`. Decision source fractions instead divide source counts
by total decision counts: `sum(w * A) / sum(w * D)`. Every world/continuation is
one cluster even if it contributes multiple decisions. For numerator `X`,
denominator `Z`, ratio `r`, and N worlds, the delta standard error is
`sqrt(N/(N-1) * sum((X-r*Z)^2)) / sum(Z)`. Joint online Welford moments retain
the covariance; no decision-level binomial error is substituted.

These self-normalized ratios are not claimed finite-sample unbiased. The
standard errors are first-order approximations and may be optimistic at low
ESS. `sum(w)^2 / sum(w^2)` describes weight concentration, not a universal
variance equivalence. See Art Owen's [Monte Carlo, chapter 9, sections 9.2–9.3](https://artowen.su.domains/mc/Ch-var-is.pdf).
Decision-weight ESS uses `w*D`; prefix ESS alone cannot certify a rare later
street's precision. Maximum normalized prefix weight is also retained.

Zero-weight worlds do not contribute conditional evidence; zero denominators
are null. Positive reach whose square underflows fails explicitly. No later
street decisions are counted for all-in runouts. Whole-hand utility includes
prior commitments; it is not rebased at the prefix. Production Cash utilities
are in bb (betting-action labels use internal millibb amounts). Suffix source fractions
partition average/current/regret fallback/uniform fallback. Separate prefix
source fractions indicate whether each world used current/regret/uniform
anywhere along the forced path; those flags may overlap. Different nested
prefixes also overlap and must not be added as disjoint strata.

## Implementation and verification

- `crates/multiway/src/solver/conditioned.rs` adds a read-only API with 1–64
  unique legal nonterminal paths, at least two samples, early path validation,
  and indexed accumulation independent of thread count. Chunk storage is
  approximately 8 MiB, excluding allocator overhead and constant accumulators.
- `mw_checkpoint_audit` adds optional `--condition-prefix` and
  `--condition-samples`; disabled output and ordinary diagnostics are retained.
  No TOML/default, training algorithm, checkpoint fingerprint, state version,
  or artifact wire format changes.
- Independent weighted-residual calculations cover world/utility correlation
  and variable decision counts. Regression cases cover rare versus zero reach,
  source fallbacks, every street, invalid paths, root baseline identity,
  read-only state, 1/2/8 threads and prefix-order invariance.
- Final required checks: fmt, workspace clippy, workspace tests (762 passed,
  30 ignored across 48 suites). Feature example tests: 31 passed; targeted
  research average core tests: 6 passed. Release audit build passed.

## Retained experiment

Local run root: `runs/conditional-depth-20260910/`. Source manifest/archive,
config, immutable binary, literal job arguments, checkpoint hashes, stdout,
stderr, clock/peak-memory measurements and verification logs are retained.
The two checkpoint cases are 1,024 and 32,768 completed sweeps from the same
seed-0 Simple K32 current-street model. Both use evaluation seeds 101 and 202,
131,072 worlds per seed shared across nine prefixes, and eight threads.
The 900-second process timeout applies per case. Measurements run serially.
Ordinary baseline coverage uses the same 131,072 worlds; tiny ordinary
candidate diagnostics (128 worlds, one BR training traversal) are incidental
and are not evidence of strong deviations.

Prefixes include root, SB facing a 3bet, its called flop, BB facing a 4bet,
its called flop, SB facing a 5bet, and an actual UTG/HJ/CO three-player flop,
check-through turn and check-through river. These are aggregate physical-board
diagnostics, not board-specific GTO Wizard matches. Tree/rake/abstraction
mismatches with the partial Simple reference remain as previously documented.

No GCP resource was started. The user's total September bill constraint remains
strictly below USD 20; these local measurements do not establish cloud budget.

## Initial checkpoint comparison

Values below keep the two evaluation seeds 101 / 202 separate. Percentages
are decision-weighted average-policy use on river suffix decisions. Reaching
a three-player flop does not require that all later river decisions remain
three-player: intervening folds can produce HU continuations. The exact
check-through river prefix itself retains three players.

| Prefix | 1,024 sweeps: river average % | 32,768 sweeps: river average % | 32,768 river decision ESS |
|---|---:|---:|---:|
| Root | 51.24 / 49.66 | 94.55 / 94.25 | 11,762 / 11,555 |
| SB 3bet-call flop | 5.40 / 7.67 | 79.79 / 80.35 | 490 / 529 |
| BB 4bet-call flop | 0.003 / 0.826 | 45.17 / 60.00 | 37.8 / 30.0 |
| UTG/HJ/CO three-player flop | 8.27 / 7.61 | 33.63 / 56.19 | 13.6 / 16.3 |
| Exact three-player check-through turn | 0 / 0 | 11.86 / 3.58 | 4.9 / 9.5 |
| Exact three-player check-through river | 0 / 0 | 0 / 0 | 9.6 / 12.4 |

The two checkpoint profiles have different reach weights and continuation
strategies. This table is a coverage comparison, not a paired EV-improvement
or isolated learning-rate estimate. Ordinary baseline samples at the 32k
checkpoint reached the three-player flop only 1 / 2 times and its exact turn
and river zero times. Forced replay recovers a nonzero conditional diagnostic,
but the small effective counts still constrain quantitative conclusions.

At 32k, exact three-player river reach is `8.14e-9 / 3.43e-9`; prefix ESS is
`8.57 / 17.42`, and the largest world contributes `27.9% / 17.5%` of its
normalized reach. Observed suffix decisions use 100% uniform fallback, and
all prefix reach weight passes through at least one uniform fallback. This
is the implemented composite profile's reach, not a strictly observed-average
profile. A zero standard error for the constant observed source category does
not certify global accuracy or rule out unobserved categories.

Initial whole-process time was 170.29 s (1k) and 184.72 s (32k), including
construction, unrelated small candidate diagnostics, ordinary coverage,
conditional coverage, JSON and disposal. Constructor portions were 45.06 s
and 46.93 s. Observed peak memory was 1,550,704,640 / 1,950,134,272 bytes.
These are single measurements, not a conditional-only timing benchmark.

## Adaptive precision follow-up

Because 32k three-player prefix ESS was below 50, the follow-up fixes four
prefixes: 4bet-call flop and the three-player flop/turn/river. It uses
1,048,576 physical worlds per fresh seed 303 / 404, retains matching ordinary
coverage, and keeps the same 900-second whole-process timeout. This selection
was made after the pilot; the larger samples are not pooled with seeds
101 / 202 or presented as a pre-registered independent experiment.

Focused results keep seeds 303 / 404 separate. All percentage uncertainties
below are delta standard errors in percentage points, not confidence bounds.

| Prefix | Prefix ESS | River decision ESS | River average % ± SE | River uniform % |
|---|---:|---:|---:|---:|
| BB 4bet-call flop | 1,403 / 1,386 | 233 / 197 | 54.47 ± 2.45 / 51.15 ± 2.14 | 0.0001 / 0.0226 |
| Three-player flop | 437 / 275 | 83.2 / 32.4 | 51.56 ± 2.26 / 44.38 ± 4.30 | 42.25 / 42.85 |
| Exact three-player check-through turn | 46.2 / 10.5 | 16.4 / 2.3 | 6.69 ± 2.79 / 6.43 ± 5.05 | 93.30 / 93.57 |
| Exact three-player check-through river | 46.2 / 10.5 | 41.9 / 12.4 | 0 / 0 | 100 / 100 |

The ordinary baseline at the same million-world budgets produced 21 / 12
river decision trajectories below the 4bet-call flop, and 2 / 1 below the
three-player flop. It still observed zero exact three-player turn/river
reaches. The forced diagnostic materially improves observability for these
chosen branches, while its effective counts must still be read explicitly.

The exact three-player river reach estimates are `9.92e-9 ± 1.46e-9` and
`8.68e-9 ± 2.68e-9` (SE). Maximum normalized weight is 8.3% / 29.6%; one seed
therefore still concentrates almost 30% of the evidence in one physical world.
Increasing the nominal budget eightfold did not reliably fix the deepest
branch's precision. Do not infer that its 331,926 / 331,510 positive-weight
worlds are that many independent effective observations. Observed exact-river
suffix decisions remain entirely uniform fallback, and every positive reach
path includes uniform fallback; approximately 85% of reach weight also includes
regret fallback earlier in the forced path.

The focused process took 573.74 seconds, including 45.86 seconds of
construction and both ordinary/conditional diagnostics. Peak working set was
1,950,273,536 bytes. All three cases stayed below their 900-second timeout;
no concurrent benchmark or Cargo process ran during them.

## Decision and reproducibility

Keep this diagnostic available, and keep the production training defaults
unchanged. It identifies a persistent deep three-player support gap concealed
by aggregate river coverage, but does not establish better strategic EV or
balanced convergence. Further brute-force root-world sampling is a poor next
use of compute for the exact check-through branch. The next bounded candidate
is a production-specific preflop posterior proposal with support/rounding
correction, tested first against a small collision-aware enumeration oracle.
See the [next-experiment plan](../average-depth-20260910/plan.md).
Subsequent learning changes must track regret support as well as average mass.

The [machine-readable report](result.json)
retains 44 prefix/seed rows, all seat/street conditional quantities, ordinary
baseline counts, per-case hashes/times and verification metadata. Rebuild the
retained release source and rerun the literal `*-job.json` arguments through
`tools/run_average_sampling_measurement.ps1 -MeasurementKind CheckpointAudit`
with a fresh output directory. Validate with:

```text
python tools/summarize_conditional_depth.py --run runs/conditional-depth-20260910
```

Source identity is the uncommitted 160-file Rust/Cargo snapshot based on
`93c95533dbaca2e8388e82235af5519071fd880f`, not that base commit alone.



- Source manifest SHA-256: `91fe7d3aea8bf0ecf9497e994f3d25faaacd655526b461d90e7ac36b3200ffe4`.
- Source ZIP SHA-256: `e1475e231184505f7e8e69575bd025c18a874eb7fa7bc8c146a0aac05f19f1bc`.
- Audit binary SHA-256: `15b2051ec45289c37a1ff49d2c89a8285192b064777f6af881cf17de57b6b4f4`.
