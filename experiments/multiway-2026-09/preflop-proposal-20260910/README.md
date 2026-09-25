# Corrected preflop proposals for rare postflop evaluation (2026-09-10)

Status: complete for this evaluation change; strategic learning improvements remain open.
This changes read-only evaluation, not the learned checkpoint or training ranges.

## Motivation and estimator

The [previous conditional audit](../conditional-depth-20260910/README.md)
used root-distributed worlds and forced public paths. Even 1,048,576 worlds
left the exact three-player check-through river with prefix ESS 46 / 10 and
maximum normalized weight 8% / 30%. More root worlds were not an efficient
way to measure that branch. Aggregate river average coverage concealed a
uniform-only continuation there.

For production Holdem, the forced preflop probability factorizes by acting
seat: `A(H)=product_i A_i(h_i)`. BucketContext exposes only that seat's combo,
an empty preflop board and public opponent count. Public betting transitions
do not depend on hidden cards. Let `r_i` be the original sampler's actual
unnormalized draw intervals. The desired preflop posterior has joint mass
proportional to `product_i r_i(h_i)*A_i(h_i)` on collision-free tuples.
All seats, including folded seats, remain present. Whole-tuple rejection
conditions the product distribution on card disjointness; accepted runouts
exclude every dealt hole card.

The implementation proposes `q_i` by scaling `r_i*A_i` by its seat maximum,
rounding to f32, and flooring every positive target to 1e-7 of that maximum.
It then uses the *actual cumulative draw interval* after these changes.
Per-world correction is
`C(H)=product_i [(r_i*A_i/scale_i)/q_i_actual]`.
For a postflop forced-action product `B(W)`, use relative weight `C(H)*B(W)`.
Unknown product normalizers and seat scales cancel in every conditional ratio.
The positive floor changes the proposal, not the target, because it is corrected.
Lost positive support, non-finite weights or underflow fail explicitly.

Do not multiply preflop `A` a second time during replay. Do retain every forced
action's RNG draw and policy-source flag. As before, conditional source
fractions and utility errors use world-level joint numerator/denominator
moments, not binomial decision counts. Self-normalized finite-sample ratios
are not claimed unbiased. The general background is Art Owen's
[Monte Carlo, chapter 9](https://artowen.su.domains/mc/Ch-var-is.pdf).

Proposal `relative_weight_mean` is deliberately not called root reach.
Absolute root reach remains a separate ordinary/weighted-root diagnostic.
Different preflop trunks receive separate world budgets; prefixes within a
trunk share their proposed worlds. Common seed ids across different samplers
are not treated as independent replicates or as a paired error estimate.

## Implementation and tests

`solver/preflop_proposal.rs` prepares the corrected proposal and exposes
`evaluate_profile_conditioned_preflop` for Holdem adapters. A call accepts
postflop decision prefixes with one shared preflop trunk. The checkpoint-audit
example groups different trunks and adds `--condition-sampler preflop-proposal`
and `preflopConditionalEvaluations`. Default root output, ordinary sampling,
training algorithm/ranges, TOML and checkpoint/artifact identities are retained.

The sampler's new crate-private read helper exposes actual draw-interval mass;
it does not alter any draw or RNG consumption. The existing full-deck uniform
fast path reports uniform mass independently of an equivalent range's scale.
Preparation uses the baseline f32 policy normalization, fallback and cumulative
interval behavior, including the residual last action.

Verification includes an independent 3-seat/3-combo enumeration with collisions,
nonuniform priors and a tiny target that activates the floor. Both utility and
source ratios agree with the original target after correction. A 50,000-world
sampling check verifies tuple frequencies and that all three seats' cards
remain absent from the runout. A board-card expectation uses all dealt blockers.
Real Holdem tests separately compare reconstructed prefix factors with baseline
replay, test postflop weight multiplication and identical utilities/source flags,
and cover sparse/dense storage, average/current/purified variants, read-only
state, thread counts 1/2/8, prefix ordering and invalid/mixed trunks.

Required fmt and workspace clippy passed. Workspace tests: 765 passed,
30 ignored across 48 suites. Feature example tests: 33 passed. Targeted
average-research tests: 6 passed. Release audit build passed.

The repeated default-root audit matches every previous output field except
the three timing fields and the top-level configuration file location. Both
configuration files have the same verified SHA-256. All numeric evaluations,
prefix contexts, ordinary coverage, node frequencies and solver identities
are exactly equal. This regression job takes 178.113 seconds, with 46.023
seconds of session construction and a peak working set of 1,950,138,368 bytes.

## Retained design and budget

Run root: `runs/preflop-proposal-20260910/`. The uncommitted 161-file Rust/Cargo
snapshot, immutable binary, source archive, configuration, job arguments,
checkpoint hashes, logs and measurement files are retained. Base revision:
`93c95533dbaca2e8388e82235af5519071fd880f`; that commit alone is not the measured
source. Every job has a 900-second timeout, runs locally and is measured
serially after Cargo verification finishes. No cloud resource is started.

The unchanged seed-0 Simple K32 checkpoint contains 32,768 sweeps. One root
regression job repeats the previous 9-prefix, 131,072-world seeds 101/202 audit.
Two proposal jobs use four postflop prefixes: BB 4bet-call flop, UTG/HJ/CO
three-player flop, and its exact check-through turn and river. They use
131,072 accepted worlds **per seed per trunk**, with two trunks, keeping
101/202 and 303/404 separate. Ordinary coverage also uses 131,072 worlds per
seed. Tiny incidental candidate diagnostics are not a meaningful BR test.

For seeds 303/404 the retained root reference used the same four suffixes but
1,048,576 root worlds per seed. Thus the proposal uses one eighth the nominal
world budget per prefix and one quarter as many accepted physical worlds across
its two trunks, before accounting for rejection attempts. Zero-weight root
worlds skip the suffix, so this is not a count of executed suffix rollouts. This is not an equal-time
comparison. Report actual whole-process cost and individual standard errors.

Both samplers target the same frozen composite profile, including fallback
policies. Cash utility is whole-hand bb; action labels use internal millibb.
A three-player flop's suffix can become HU after later folds; the exact
check-through river itself retains three players. Model/board/abstraction/rake
mismatches with the partial GTO Wizard Simple reference remain unchanged.

## Conditional precision on the frozen 32k checkpoint

The following comparisons keep seeds 303 and 404 separate. Each cell lists
`303 / 404`. The root reference allocates 1,048,576 worlds per prefix and seed;
the proposal allocates 131,072 per preflop trunk and seed. These are weight
concentration diagnostics, not counts of independent decisions or a universal
variance-equivalent sample size.

| Prefix | Root prefix ESS | Proposal prefix ESS | Root river decision ESS | Proposal river decision ESS |
|---|---:|---:|---:|---:|
| SB–BB 4bet-call flop | 1,403 / 1,386 | 131,072 / 131,072 | 233 / 197 | 19,224 / 19,378 |
| UTG/HJ/CO three-player flop | 437 / 275 | 131,072 / 131,072 | 83 / 32 | 19,005 / 19,007 |
| Same three-player check-through turn | 46.20 / 10.50 | 11,887 / 11,833 | 16.43 / 2.33 | 3,567 / 3,588 |
| Same three-player check-through river | 46.20 / 10.50 | 11,887 / 11,833 | 41.91 / 12.37 | 11,486 / 11,459 |

The exact three-player river's prefix ESS increases by 257x / 1,127x. Its
largest normalized weight falls from 8.28% / 29.57% to 0.0230% / 0.0216%.
Postflop checks still concentrate the weights: the preflop-only proposal does
not eliminate the remaining board/action selection. There are 24,238 / 24,208
positive-weight worlds at this endpoint, fewer than the nominal world budget.

Uncertainty improves for actual measured quantities as well. Below, the EV
standard error is for the actor at the prefix: SB for the 4bet-call flop and
UTG for the three-player prefixes. EV is whole-hand bb, including sunk chips;
it is not an action advantage or the gain from this implementation change.

| Prefix | Root actor EV SE (bb) | Proposal actor EV SE (bb) | Root river average-use SE (pp) | Proposal river average-use SE (pp) |
|---|---:|---:|---:|---:|
| 4bet-call flop | 1.530 / 1.576 | 0.161 / 0.160 | 2.449 / 2.137 | 0.257 / 0.255 |
| Three-player flop | 4.754 / 5.703 | 0.255 / 0.254 | 2.260 / 4.300 | 0.235 / 0.233 |
| Three-player check-through turn | 8.830 / 6.220 | 0.697 / 0.694 | 2.788 / 5.048 | 0.292 / 0.283 |
| Three-player check-through river | 7.133 / 3.679 | 0.652 / 0.660 | 0 / 0* | 0 / 0* |

`*` Every sampled positive-weight continuation at the last endpoint uses
uniform fallback. Zero empirical error for an all-equal indicator does not
bound the probability of an unseen nonuniform bucket. It does not prove good
strategy quality or exact coverage of every possible world.

The first proposal job, with seeds 101/202, independently gives prefix ESS
12,065 / 11,904 at the exact river, maximum weight 0.0220% / 0.0231%, and UTG
EV standard error 0.645 / 0.648 bb. Each seed uses the same 131,072-world
budget per trunk; it is not pooled with the other seed pair.

Across proposal seeds 101, 202, 303 and 404, observed river average use is:

| Prefix | 101 | 202 | 303 | 404 |
|---|---:|---:|---:|---:|
| 4bet-call flop | 54.85% | 54.85% | 54.93% | 54.74% |
| Three-player flop | 44.64% | 44.32% | 44.17% | 44.65% |
| Three-player check-through turn | 7.58% | 7.17% | 7.53% | 7.49% |
| Three-player check-through river | 0% | 0% | 0% | 0% |

Here `UniformFallback` specifically means no stored evaluation column was
found (dense lookup requires a touched column). A stored zero-average column
uses `RegretFallback`, even if regret matching also gives uniform probabilities.
Thus the source categories distinguish missing columns from merely uniform
normalized strategies; neither records a detailed regret-update count.

For the three-player flop suffix, uniform fallback remains about 42.3–42.6%
of river decisions, and current-regret fallback accounts for the remaining
part outside positive average use. From the checked-through turn, river
uniform fallback is about 92.3–92.7%. These suffixes may become HU after
folds; the exact check-through river itself still has three active players.

The old low-ESS root estimates are not precise ground truth. For example,
seed 303's three-player-flop river average use was 51.56% ± 2.26 pp, compared
with 44.17% ± 0.235 pp under the proposal (each ± is one reported SE). The
gap exceeds three of the old reported errors. Very rare source categories
also differ: the same seed's 4bet-call river uniform-use estimate rises from
0.000111% to 0.0382%. Concentrated samples can miss tails and understate their
own uncertainty. Neither shared seed ids nor empirical agreement proves
target equality; that relies on the corrected density argument and independent
enumeration tests above. No pooled test or paired standard error is claimed.

## Measured cost and decision

The two proposal jobs finish in 127.269 / 124.103 seconds, including session
construction (46.094 / 47.029 seconds), incidental candidate diagnostics,
ordinary coverage, conditional evaluation, output and disposal. Their peak
working sets are 1,950,203,904 / 1,950,306,304 bytes. The earlier million-world
root job took 573.738 seconds and peaked at 1,950,273,536 bytes. Thus the
303/404 proposal job's observed whole-process time is 78.4% lower while the
reported conditional errors above are substantially smaller. This is one
measurement per seed-pair job with different budgets, not an isolated kernel
benchmark, an equal-time experiment or a demonstrated universal speedup.

For 131,072 accepted worlds, proposal whole-tuple attempts are 443,794–444,595
for the 4bet trunk (about 29.5% acceptance), and 500,574–504,027 for the
three-player trunk (about 26.0–26.2%). Preparation's deterministic 4,096-world
pilots accept 1,221 and 1,055, respectively. Per-seat positive-support counts,
floor-adjusted counts, range fingerprints and target scales are retained in
the JSON. Rejection work remains bounded by the sampler's explicit limit;
the improvement does not assume collision-free independent seats.

**Decision:** use the optional corrected proposal for subsequent measurements
of these rare postflop branches, retaining ordinary/root-weighted reach
separately. The learned profile, default sampler and training algorithm remain
unchanged. This is a verified evaluation-efficiency improvement. It does not
yet establish improved equilibrium accuracy, reduced exploitability, or
balanced strategic convergence. The next learning intervention must address
the persistent uniform fallback and assess regret support as well as average
mass, with fixed-sweep and calibrated-compute controls across deep HU and
actual three-player branches.

## Reproduction and identities

The [machine-readable report](result.json) retains
all 16 proposal prefix/seed rows, full root-reference rows, contexts, budgets,
proposal metadata, process measurements and verification records. The
summarizer checks expected seeds, groups and prefixes as well as file identity,
so a missing row cannot be presented as a completed experiment.

| Retained input | SHA-256 |
|---|---|
| 161-file source manifest | `4530f49cb07722328de19ea682e66d7523e00997ad07bb9bb86d04f9886200e9` |
| Source ZIP | `72d2ad8786065b3b7781dd9d1ae49e934b1208b49775a602c43694481e422002` |
| Release audit binary | `23dce011ebdb8526ec267cbdef66e4b012703045c95e50941f2808bea24c8bcd` |
| Audit config | `3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a` |
| 32k checkpoint | `9a18c0927daf7d1b036558b7c87fa658a3055b6366e263385d776ca3bca0bd0a` |

The source archive matches both its manifest and all current Rust/Cargo files.
The retained verification manifest binds all six successful command logs to
that source. Job files and measurements retain exact arguments and hashes;
`experiment.json` binds the summarizer and measurement runner. Measurements
use release MSVC code with `target-cpu=native`, eight solver threads and a
warm EHS cache on the recorded local Windows machine.

Regenerate the checked report from repository root:

```text
python -X utf8 tools/summarize_preflop_proposal.py --run runs/preflop-proposal-20260910 --output docs/validation/multiway-preflop-proposal-2026-09-10.json
```

To repeat measurements, use the immutable `audit.exe` and the corresponding
`*-job.json` with `tools/run_average_sampling_measurement.ps1`,
`-MeasurementKind CheckpointAudit`, and fresh output directories. Keep jobs
serial and avoid concurrent Cargo work. A different machine or cold cache
requires new timing evidence. No GCP resource was launched for this experiment;
this statement does not determine the account's September charges or remaining
budget.
