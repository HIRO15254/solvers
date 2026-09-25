# Multiway one-step endpoint deviation (2026-09-10)

Status: completed diagnostic implementation and frozen-baseline experiment.
The selected three-player river has a large measured local improvement
opportunity: a legal, independently fitted one-step action table gains
**27.920 ± 0.586 bb / 27.660 ± 0.590 bb** on held-out seeds 702/703
(mean ± one standard error), over **all** prefix weight. Its retained keys
cover **97.264% / 97.282%** of that weight. These are conditional gains
against one frozen profile. Older root-reach estimates put this exact branch
around 1e-8, with poor ESS; no large whole-game benefit or balanced convergence
improvement is established, and no learned policy/default was changed.

## Target and method

This follows the [endpoint design](plan.md)
and [exploration screen](../opponent-exploration-20260910/README.md). The
frozen seed-0, epsilon-0 checkpoint has 32,768 sweeps in the same partial
Simple K32 six-seat 100bb cash configuration. UTG/HJ/CO remain active after
the UTG open, HJ 3bet, CO cold call and UTG call, then six checks through
flop and turn. At river entry UTG (internal seat 3) acts with two opponents.
The history is `86599afe4e294e3e6fcd0d7b5a993b0a`; board and hole cards vary
across worlds. The menu is check, bet 10.5bb, or all-in 93.5bb. This is an
exact public action history across boards, not a board-specific hand chart.

Use the corrected preflop proposal with all folded-seat blockers retained.
For each world the weight is its proposal correction times the baseline
postflop prefix-action probability product. Preflop probabilities are already
in the proposal. The endpoint action probability is never included in the
weight, even when the baseline assigns that action zero probability. The
unknown proposal normalization cancels; relative weights are not root reach.

Fit enumerates all three legal first actions against the same baseline on
paired worlds and action RNG streams. It aggregates gains by the endpoint
actor's own legal information key, then chooses the first maximum with a
positive mean if that key has fit weight ESS >= 64. It never selects a
per-world best action using hidden opponent cards. Every later decision,
including another decision by UTG, follows the frozen baseline.

The fitted table is frozen before both held-out evaluations. Unsupported
keys keep the baseline and contribute zero gain while retaining their weight
in the denominator. Held-out moments include every accepted world, including
zero-weight worlds. Signed gains are not clamped; a zero denominator is null.
Joint numerator/denominator Welford moments give the trajectory-clustered
delta standard error. Fit per-key errors use that key's positive-weight worlds;
fewer than two such worlds produce null action estimates. ESS gates do not
provide confidence guarantees, and the measured table is not a complete best
response or a multiplayer equilibrium certificate.

## Predeclared budgets and cost

The initial timing pilot used fit seed 601 with 4,096 worlds and held-out
seed 701 with 8,192. Its zero retained keys and zero gain confirmed the
importance of reporting candidate coverage: zero here does not mean a strong
baseline. The successful 0.859-second evaluation phase admitted the already
planned main budget: fit seed 602 with 65,536 worlds, then 131,072 worlds
each for seeds 702 and 703. The gate and table were not retuned from held-out
results. Neither seed selection nor pooling occurred.

Runs were serial, local, eight threads, 8GiB arena budget, warm EHS cache,
with a 900-second external timeout per process. Incidental global diagnostics
used only 128 worlds per seed (101/202), one deviator-fit traversal per seat,
and no node-frequency sampling. Those tiny global diagnostics are not the
quality test reported here. The checkpoint was reused, with no new solver
checkpoint or solution written.

| Run | Construction (s) | Fit (s) | Held-out (s) | Full endpoint diagnostic (s) | Whole process (s) | Peak working set (bytes) |
|---|---:|---:|---:|---:|---:|---:|
| pilot | 46.083 | 0.417 | 0.439 | 0.859 | 47.435 | 1,950,101,504 |
| main | 46.924 | 6.549 | 8.602 / 8.251 | 23.405 | 70.775 | 1,950,019,584 |

Fit/held-out phase clocks include their sampling and aggregation. Proposal
preparation and thread-pool construction belong to the full endpoint clock.
Process time additionally includes restoration, other audits, JSON and
disposal. Each case was timed once; these are not performance confidence
intervals. Ordered sample accumulation preserves thread-count reproducibility;
world-result chunks are bounded at approximately 8MiB independently of the
world budget. Per-key fit/table storage and the existing solver are separate.

| Stage / seed | Accepted worlds | Deal attempts | Positive-weight worlds | Terminal replays | Prefix ESS | Maximum normalized weight |
|---|---:|---:|---:|---:|---:|---:|
| pilot fit / 601 | 4,096 | 16,092 | 706 | 2,824 | 340.126 | 0.007287621 |
| pilot held-out / 701 | 8,192 | 31,753 | 1,529 | 1,529 | 752.521 | 0.003203362 |
| main fit / 602 | 65,536 | 251,468 | 12,138 | 48,552 | 5971.343 | 0.000441335 |
| main held-out / 702 | 131,072 | 500,602 | 24,427 | 48,153 | 11875.498 | 0.000206191 |
| main held-out / 703 | 131,072 | 503,316 | 24,297 | 47,918 | 11848.498 | 0.000202927 |

Terminal counts include baseline and candidate continuations. Fit completes
four replays for each positive-weight world; held-out completes one baseline
plus one candidate only when its key was retained. Zero-weight suffixes skip
replay. They are still accepted-world observations in the held-out estimator.

## Frozen candidate and held-out result

All 32 policy columns at this exact endpoint are missing in the checkpoint.
Every exported baseline-source row therefore reports uniform fallback. This
does not describe the entire checkpoint or every unobserved descendant.
The observed baseline river continuations in these samples also use uniform
fallback only. The forced prefix is a composite stored/fallback baseline:
both held-out seeds observe uniform fallback somewhere in every positive-
weight prefix, and regret fallback in 77.625% / 78.943% of prefix weight.
These prefix flags overlap; suffix source fractions are a partition.

| Frozen action | Bucket IDs | Count |
|---|---|---:|
| Keep baseline (fit ESS < 64) | 0, 1, 2, 3, 18 | 5 |
| Bet 10.5bb | 4–16, 19 | 14 |
| All-in 93.5bb | 17, 20–31 | 13 |

The main fit retains 27/32 buckets and 97.447% of its prefix weight. Bucket
IDs are abstraction labels, not an independently verified strength ordering.
Individual fit-action estimates can be noisy; retention uses the predeclared
ESS/positive-mean rule rather than a per-key significance test. Held-out gains
evaluate the choices made by this one fit. Their standard errors condition on
that frozen table; variability across independent fits or training seeds is
unmeasured.

| Held-out seed | UTG baseline EV (bb) | Signed candidate gain (bb) | Retained-key weight | Prefix ESS |
|---:|---:|---:|---:|---:|
| 702 | 19.472 ± 0.650 | +27.920 ± 0.586 | 97.264% ± 0.148pp | 11875.498 |
| 703 | 20.089 ± 0.656 | +27.660 ± 0.590 | 97.282% ± 0.146pp | 11848.498 |

All ± values are one delta standard error, not exact confidence intervals.
The two held-out gains describe the same previously fitted candidate and are
reported separately. The baseline and gain estimates are correlated; do not
add their standard errors or infer a candidate EV error from these columns.
Future bets, including 93.5bb all-ins, contribute to whole-hand final-stack
differences. A gain larger than the current 21bb pot is therefore possible.
This is evidence of a substantial local weakness of this frozen composite
profile, not evidence that a production learning intervention has fixed it.

## Root reach and balanced follow-up

The [earlier root-sampler audit](../conditional-depth-20260910/README.md)
used the same checkpoint/config and exact prefix. These are separate older
estimates, not a remeasurement with the new proposal:

| Root seed | Root worlds | Reach probability (mean ± SE) | Weight ESS | Ordinary observed reaches |
|---:|---:|---:|---:|---:|
| 303 | 1,048,576 | 9.919e-09 ± 1.459e-09 | 46.20 | 0 / 1,048,576 |
| 404 | 1,048,576 | 8.679e-09 ± 2.678e-09 | 10.50 | 0 / 1,048,576 |

The small ESS and large maximum weights make those root estimates weak
precision evidence, but they show why the conditional +28bb must not be
advertised as +28bb per starting hand. No precise unconditional gain was
established. Proposal relative weights cannot replace absolute root reach.

The next bounded diagnostic should apply the same frozen-fit method to the
previously selected HU 3bet/4bet river endpoints, carrying root reach and
candidate coverage beside each local gain. This balances depth against
frequency before choosing a learning intervention. Then investigate regret-
learning support/scheduling with fixed-sweep and calibrated-compute controls
across at least three training seeds. The exploration default remains zero;
epsilon 0.06 is still only a follow-up candidate. Its low own-posterior ESS
needs adequate key support, and a changed profile changes the conditional
population. Common-reference comparisons need explicit fixed prefix weights.

## Verification and reproducibility

`cargo fmt --all --check`, workspace clippy with warnings denied, and
`cargo test --workspace` pass: **773 passed, 30 ignored, 48 suites**.
Feature example tests: **38 passed**. Targeted average-research tests:
**6 passed**. Release audit build passed. Seven endpoint tests also passed
in the focused run. The initial clippy-only test allocation warning was
fixed before the final checked source snapshot and all measurements.

Tests include zero-baseline-probability actions, exactly one overridden
decision even when the actor acts again, disabled-override RNG/source identity,
hidden-card argmax rejection, unsupported-key denominator retention, negative
held-out gain, independent schedules, deterministic ties and a weighted
residual variance oracle. Real Holdem baseline output matches the existing
conditional proposal evaluator exactly across sparse/dense storage, average/
current/purified profiles and 1/2/8 threads. Snapshots remain unchanged.

The [machine-readable report](result.json)
contains all fit rows, held-out results, clocks, source provenance and the
separate root-reach evidence. The validator checks complete key denominators,
fit-only retention, seed/budget identities, replay counts, weight/source
partitions, raw endpoint support and source/config/checkpoint/binary/log
hashes. Corrupted fixtures are rejected; signed negative gains remain legal.

| Retained input | SHA-256 |
|---|---|
| 165-file source manifest | `30f8661114a192c1dc1c9ec399dba6ce2a427757a67f973d35197c77d6bef52f` |
| Source ZIP | `59b57060d648f633d748cdafe3f3de14fc483bf0dc65242194a14550e7f28394` |
| Release audit executable | `f66dfce7ce5709dad80d9b2f8389e7f783cead036afa4f5dfc1d960c3176ec41` |
| Frozen checkpoint | `9a18c0927daf7d1b036558b7c87fa658a3055b6366e263385d776ca3bca0bd0a` |

The snapshot contains all crate Rust/Cargo files, workspace Cargo config/lock
and the three compile-time included contract documents. Its base commit is
`93c95533dbaca2e8388e82235af5519071fd880f`; the measured source is the recorded
uncommitted snapshot. The environment is Rust/Cargo 1.97.0, Windows MSVC,
`target-cpu=native`, with 16 logical CPUs. Each retained job records literal
arguments, config identity, timeout and successful completion.

```text
python -X utf8 tools/summarize_endpoint_deviation.py --run runs/endpoint-deviation-20260910 --output docs/validation/multiway-endpoint-deviation-2026-09-10.json
```

Repeat using the immutable retained audit executable and literal pilot/main
jobs through `tools/run_average_sampling_measurement.ps1` with
`-MeasurementKind CheckpointAudit`, fresh output directories and no concurrent Cargo work.
No GCP resource was started. This does not establish the account's September
bill or remaining budget. The broader solver-improvement goal remains active.
