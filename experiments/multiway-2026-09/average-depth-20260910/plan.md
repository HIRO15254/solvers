# Multiway depth: next bounded experiments (2026-09-10)

## Current priority: preflop decision quality

The user has explicitly prioritized good **Preflop** solutions, including
deep 3bet/4bet/5bet decisions. The completed
[postflop continuation pilot](../average-continuation-20260910/README.md)
passes its cost/invariant gate but does not establish better preflop quality.
Its further postflop-focused cohort is deferred. Historical next-action text
below records the earlier investigation and is superseded by this priority.

The completed extension provides independent endpoint action-gain evaluation
at root and intermediate preflop decisions. It uses a corrected deal proposal built
from exactly the preceding preflop path, preserving own-information action
selection and independent fit/held-out evaluation. Keep the old postflop-only
conditional-profile API unchanged. The
[baseline](../preflop-endpoint-20260910/README.md) exactly
reproduces earlier training and identifies positive local gains at all three
deep preflop decisions. The current work compares additional sweeps and the
existing periodic discount. The separate
[preflop quality plan](../quality-plan.md) defines those
gates. Postflop policy quality matters through preflop action values; postflop
coverage alone is not the primary success metric.

This is a plan, not evidence of strategy improvement. Current implementation
and measurements are in the
[coverage report](../simple-depth-coverage-20260910/README.md) and
[checkpoint report](../checkpoint-streaming-20260910/README.md).

## Completed: coverage inside specified deep branches

The [deep-prefix audit](../deep-prefix-audit-20260910/README.md)
implements and validates baseline coverage at/below up to 64 public histories,
with separate street/seat counts and decision-trajectory counts. Ordinary
evaluation and checkpoint state remain unchanged. The baseline-only API makes
131,072-world diagnostics economical without repeating every deviation replay.

At 32,768 sweeps, global river average use is 94–95%, but the SB 3bet-call
branch is only 77–79% across the two evaluation seeds. Its remaining observed
river decisions all fall back to current regrets. This selects a concrete
late-street target for the next average-sampling experiment. The 4bet-call
branch reaches only one or two river decision trajectories per seed and cannot
be ranked reliably. These selected branches are heads-up continuations;
separately include a genuine three-player postflop prefix in the next audit.
Do not treat the preflop hand exporter as a board-specific postflop strategy
audit, or positive average mass as a low-variance/convergence certificate.

## Completed: paired average-sampling screen

The [average-depth experiment](README.md)
compares UniformOne and research-only EnumerateFirst across three training
seeds at 32,768 sweeps, then adds three calibrated UniformOne controls within
1.9% of the research variant's measured driver time. Fixed-sweep regret hashes
match exactly. The consuming runner now exports postflop rows and prefix
coverage while retaining its no-resume/no-artifact boundary.

EnumerateFirst improves 3bet-call river average use from roughly 77–88% to
92–95% at equal sweeps, at 1.33–1.37x driver time. Five of six calibrated
comparisons retain a coverage benefit. That benefit does not extend uniformly
to the exact three-player check-through river or opener/turn strategy
dispersion. The production default remains UniformOne. Positive average mass
and across-training-seed dispersion do not establish reduced EV error,
exploitability, or isolated averaging variance.

The next quality intervention should address sparse multi-player support and
separate measurement from training. An average-only RNG-seed experiment can
hold deal/regret updates fixed before any production promotion. For a change
to regret learning, compare balanced opener, 3bet/4bet/5bet and actual
multi-player postflop nodes at both fixed sweeps and calibrated compute.

If representation is limiting, repeat the existing draw-aware versus EHS2
comparison using at least three paired seeds and include late-street/deep
branch observations. The earlier single-seed opener improvement is a hypothesis
for this experiment, not sufficient evidence to promote the abstraction.
Keep the full tree/rake mismatch with GTO Wizard visible.

## Scale and cost

The 4,096-world audit lost effective sample size as deep-node reach weights
became more concentrated: the 5bet node fell to about three effective samples
at 32,768 sweeps.
The larger 131,072-world node audit is now inexpensive: validated public
states/menus and per-key normalization are prepared once. All node/profile
values match the old estimator; ordinary audit process time fell by 62–64% on
the two retained checkpoints, with each three-node sampling phase about 0.52 s.
The later [initialization phase benchmark](../tree-initialization-20260910/README.md)
measures roughly 29.5 s each for serial admission preflight and materialization
on the Simple K32 model. Its existing eight-thread materializer reduces total
benchmark time by 26.86%. The subsequent
[production integration](../production-initialization-20260910/README.md)
uses configured threads for new/resume materialization while preserving serial
resource admission. The phase benchmark omits EHS and the solver wrapper, so
it does not retrospectively partition the entire checkpoint-audit remainder.
Rare-branch baseline coverage may still need conditional/weighted sampling;
retain reach, effective samples and estimator uncertainty if adding it.

Streaming load removes the full decompressed payload staging buffer, but owned
policy rows, snapshots, fresh arenas, and tree initialization remain expensive.
Follow the [initialization plan](../production-initialization-20260910/plan.md):
use the completed phase evidence to prioritize public-state walks; preserve
non-retaining resource admission and exact construction/resume identity.

Prefer local read-only checkpoint reuse and serial measurements. No additional
cloud resource was started in the 2026-09-10 coverage/streaming experiments.
Before any future GCP allocation, independently verify September charges and
remaining obligations against the user's strict total-bill limit below USD 20;
these local results do not establish unused cloud budget. Use only the minimum
subagent count needed for the bounded experiment.

## Completed diagnostic and next rare-branch interventions

The three-player checked-through river has no common positive-average bucket
across the paired fixed-sweep seeds, and no held-out baseline trajectories at
that exact prefix. Longer calibrated controls touch additional buckets in two
seeds, without enough root-reach evidence to rank their quality.

The forced-prefix diagnostic is now implemented and verified. See the
[conditional-depth report](../conditional-depth-20260910/README.md)
for the retained checkpoint comparison and adaptive precision follow-up.
It exposes the difference between low root reach and missing positive-average
support while retaining weight ESS, maximum weight, source provenance and
trajectory-clustered uncertainty. It measures a composite baseline when any
path/continuation policy falls back; it is not a new training algorithm.

The 1k/32k comparison shows broad river coverage improvement but persistent
uniform-only decisions at the exact three-player check-through river. Root
coverage alone is insufficient. The next training experiment must include
this actual three-player endpoint and balanced HU 3bet/4bet branches, measure
regret-learning support as well as average mass, and retain fixed-sweep and
calibrated-compute controls. More positive average mass with uniform underlying
regrets must not count as a demonstrated strategy improvement.

The selected [average-only proposal](../average-continuation-20260910/plan.md)
uses a fixed 50/50 uniform/check-call mixture at each postflop opponent
decision; preflop and C-empty menus remain uniform. This specifies the earlier
broad continuation-proposal hypothesis. Every public-history probability must
stay positive and independent of private world, policy values and training
time; its history-constant factor cancels when expected accumulated vectors
are normalized. Finite-sample ratio error remains. Mixing a new history-specific
mass scale into an existing checkpoint is outside that argument, so the
candidate stays fresh-only. Separate regret-learning work is still necessary
where missing numerical support is the binding constraint.


### Completed: reduce conditional-deal weight concentration

The [corrected preflop proposal report](../preflop-proposal-20260910/README.md)
implements and validates the production-Holdem-specific factorization. The
proposal uses each seat's original actual sampler mass times its preflop
forced-action probability product. All seats, including folded seats, retain
their blockers through whole-tuple rejection. Exact f32 action intervals,
proposal rounding and a positive-support floor are explicitly corrected.
Postflop replay multiplies only the remaining postflop forced-action product;
absolute root reach is still a separate diagnostic. Independent full
enumeration, posterior sampling, board-card expectations and real Holdem
factor reconstruction cover the target-equality argument.

On the retained 32k checkpoint, 131,072 proposal worlds per seed per trunk
produce exact three-player river prefix ESS of 11,887 / 11,833 for seeds
303/404, versus 46.20 / 10.50 from 1,048,576 root worlds. UTG conditional EV
standard error falls from 7.13 / 3.68 bb to 0.652 / 0.660 bb. Whole-process
time falls from 573.738 to 124.103 seconds with the different stated budgets;
this is not an equal-time or isolated-kernel comparison. Seeds 101/202 give
similar proposal precision. Do not treat low-ESS root point estimates as
precise ground truth or pool errors from different samplers.

The exact check-through river still observes uniform-only continuations in
all four proposal seeds. The three-player-flop suffix uses average policy at
only 44.2–44.7% of river decisions and uniform fallback at roughly 42.3–42.6%;
the checked-through-turn suffix uses uniform fallback at roughly 92.3–92.7%.
Some flop/turn suffixes later become heads-up; the exact river endpoint does
not. Better evaluation now makes these selected rare branches measurable.
It does not supply a missing learning update.

### Completed: raw-support and existing exploration screen

The audit now supports fresh fixed-sweep runs and complete raw-support exports.
The [opponent exploration report](../opponent-exploration-20260910/plan.md)
compares the existing v1 parameter at 0 / 0.06 / 0.25 at 32,768 sweeps and
training seed 0. Its source passes required checks. A dense scalar/vector test
confirms that a sampled zero-target action can create touched state without
nonzero regrets, so the screen counts those separately. At the exact
three-player turn, stored support is 21 / 29 / 32 of 32 buckets, while
nonzero regret support is 0 / 29 / 0. Every setting still lacks all policy
columns at the exact three-player river. Both exploration candidates pass the
2x driver-cost gate, but conditional support does not improve uniformly.
The default stays at 0; 0.06 remains a candidate for further quality
evaluation. Its exact-river prefix ESS of 618 / 645 at 131,072 proposal
worlds requires an explicit per-key fit/held-out coverage gate. The stronger
quality test, additional training seeds and calibrated-compute comparisons
remain open.

### Completed: independent and balanced endpoint-deviation evidence

Before promoting a check/call-favoring average walk, inspect raw regret and
average columns at the actual three-player turn/river and balanced HU 3bet/
4bet endpoints. The measured `UniformFallback` source means that evaluation found no stored
policy column; dense lookup requires a touched column. A stored column with
zero average mass is instead `RegretFallback`, including when its regrets
normalize to a uniform policy. Preserve this existing distinction. For those
stored fallback columns, inspect raw all-zero versus nonzero/nonpositive
regrets, and separately identify missing/touched support. Do not infer an
update history solely from normalized uniform probabilities, and do not count
newly averaged uniform columns as strategic gain.

The [endpoint deviation report](../endpoint-deviation-20260910/plan.md)
implements conditional unilateral-deviation evidence using a fixed baseline
preflop proposal and separate fit/held-out worlds. At a fixed
public endpoint, keep the prefix policy fixed and restrict a candidate's
choices to its own legal information keys. Never select actions by the
opponents' hidden cards. Report conditional bb gain and root-reach context
separately; this is a local improvement opportunity, not global exploitability
or an equilibrium certificate in the multiplayer game. Holding the proposal's
baseline reach fixed is necessary when comparing candidate suffix policies.
A changed baseline checkpoint defines a different conditional population.

The first frozen 32k-baseline river experiment retains 27/32 legal bucket
actions and covers 97.264% / 97.282% of held-out prefix weight. The two seeds
show 27.920 ± 0.586 bb / 27.660 ± 0.590 bb gains (one SE) for the fixed table.
No learned checkpoint policy was changed. Older root-sampler evidence puts
this exact prefix around 1e-8 reach with low ESS, so a large conditional
weakness is not a large verified whole-game improvement. The
[balanced endpoint measurement](../balanced-endpoint-20260910/README.md)
now completes the selected HU 3bet/4bet comparison on the same checkpoint.
Their held-out endpoint-only gains are 1.516 / 1.777 bb and 1.467 / 1.409 bb,
with root reach about 1.8e-4 and 3.5–4.5e-6, respectively. The report retains
fit ESS eligibility, nonpositive-fit decisions and actual candidate-table
weight separately. It does not compare different conditional populations as
paired samples or declare a changed learned policy.

The next bounded implementation is the
[postflop average-continuation proposal](../average-continuation-20260910/plan.md).
HU 4bet has nonzero regrets in all 32 buckets but average mass in only nine;
its river suffix uses average policy at about 35.5% of weighted decisions.
HU 3bet and actual three-player turn/river remain balanced checks. This
average-only intervention must preserve regret fingerprints at fixed sweeps,
prove its fixed public-history proposal-factor argument and retain fresh-only
research use. Creating average-only uniform columns does not resolve missing
three-player regret learning.

Use this evidence to choose a regret-learning exploration or scheduling
intervention, then compare at fixed sweeps and calibrated compute across at
least three training seeds. Preserve paired regret fingerprints for any
average-only control. Maintain adequate diagnostics for opener, 3bet/4bet/5bet
and actual three-player postflop nodes. Keep the production algorithm and
checkpoint identity unchanged until a balanced quality benefit is established.
