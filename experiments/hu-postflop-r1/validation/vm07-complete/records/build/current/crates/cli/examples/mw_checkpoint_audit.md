# Full-policy checkpoint and fresh-run audit

`mw_checkpoint_audit` restores a frozen Multiway Preflop `.mwckpt` through
the production session builder, trains one fixed deviation candidate per
seat, and evaluates that same candidate set on multiple independent held-out
seeds. It reads the full solver state directly, including postflop policies,
current regrets, and zero-average-mass fallback state. `.mwsol` also stores
observed average policies across streets, but does not preserve this raw
regret state and can quantize probabilities. The checkpoint therefore allows
the audit to reproduce the solver's deviation and fallback behavior.

Run it from the repository root:

```text
cargo run --release -p cli --example mw_checkpoint_audit -- \
  --config runs/example/config.toml \
  --checkpoint runs/example/run/checkpoint.mwckpt \
  --evaluation-seeds 101,202,303 \
  --samples 16384 \
  --br-traversals 100000 \
  --br-seed 404 \
  --node-frequency-samples 8192 \
  --node-frequency-seed 505 \
  --threads 8 \
  --memory 48GiB \
  --node root \
  --node fold/fold
```

The config must describe the checkpoint's exact game, ranges, abstraction,
and solver algorithm. `--threads` controls restoration, parallel deviator
training, and held-out profile evaluation; `--memory` is an operational
override. All compatibility fingerprints are still checked during restore. Use
`--cache-dir` when the EHS2 cache is outside its normal machine-local path.
Diagnostics and EHS2 cache messages go to stderr. The complete audit document
is the only stdout output, so it can be redirected to JSON.

For a fixed-sweep parameter experiment, replace `--checkpoint PATH` with
`--fresh-sweeps N` (N >= 1). The two options are mutually exclusive and one
is required. This constructs a fresh production solver, runs the ordinary
sweep driver, freezes the result and performs the same diagnostics. It neither
resumes nor writes a checkpoint/solution. `freshTraining` records requested
sweeps, the sweep-driver clock, effective-config hash, actual solver config
and final metrics; `checkpoint` is omitted. `constructionElapsedSecs` retains
its session-only scope. Config `run.max_sweeps`, `max_time`, stop/evaluation
cadence and output schedules do not control this research loop; use the exact
`--fresh-sweeps` budget and an external timeout. Game/algorithm options such
as `solver.opponent_exploration` still come from the validated v1 config and
remain part of its normal fingerprint. Changing that parameter requires a
fresh experiment rather than editing an existing checkpoint's identity.

`--enumerate-raised-preflop` selects a separate experimental regret walk and
requires `--fresh-sweeps` plus a build with `--features research-regret-sampling`.
Without that feature it fails before loading the config. The core operation
requires dense range-vector, zero opponent exploration and pruning disabled.
It enumerates the first opponent decision after at least one preflop raise,
at most once on each root-to-leaf path, at every position. Descendant regrets
receive the opponent action probability and returned values are probability
weighted separately. Zero-probability branches do not supply numeric support.
The average-policy walk stays unchanged. `freshTraining.regretSamplingResearch`
records the variant, completed sweeps, eligible/public preflop node counts,
eligibility-map bytes and expansion limit. Its training clock includes map
preparation. The flag is not a v1 option or checkpoint/resume identity; this
fresh-only example still writes no checkpoint or solution. Omission preserves
the ordinary production route and omits the extra JSON field.

`--preflop-support-census` adds `preflopSupportCensus` with a separate clock
and compact records for **every** materialized preflop decision, including
untouched nodes. It records actor, public aggressions/limpers/flats and player
counts, full expected bucket denominators, touched and numeric-regret/average
support, and exact raw-state fingerprints. It reads borrowed arena columns
without cloning all policies or keeping every per-bucket vector in the output.
This exposes neglected regions across the entire Preflop Tree. Touched and
nonzero-regret counts are not visit counts, ESS, exploitability or evidence
that a strategy is strong; independent quality evaluation is still required.

`--support-node PATH` adds an optional `policySupport` array for up to 64
unique public decision histories on any street. This uses the usual node
path/hex syntax and validates requests before fresh training. It reports every
expected current-street bucket, including missing columns, actual action labels,
the recorded street opponent count used for bucket cardinality, raw regrets and
strategy sums, and normalized current/positive-average strategies. Missing
columns have null values, and absent positive average mass has null average
strategy rather than a substituted fallback. These postflop bucket rows are
not board-specific hand strategies.

Statuses distinguish `missing`, `stored-zero-regrets`,
`stored-nonpositive-regrets` and `stored-positive-regrets`. The summary counts
stored, nonzero-regret, positive-regret, average and average-with-nonzero-regret
buckets separately, all against the same complete bucket denominator. A stored
column can be touched by a zero regret event or an average-only event. All-zero
regrets can also result from cancellation; nonzero regrets do not count visits
or establish convergence. Equal positive regrets and nonpositive regrets can
both normalize to uniform probabilities. Raw average mass contains a
history-dependent proposal factor and is not a reach probability or a quantity
to sum/compare across different histories. Default checkpoint output omits
both new fields and retains the previous numerical behavior.

To fit changes across a seat's entire preflop policy, explicitly provide all
four options below. This diagnostic is disabled by default and needs no
research feature or endpoint path.

| Option | Required value |
|---|---|
| `--preflop-deviation-fit-traversals N` | Positive fitting traversals per seat |
| `--preflop-deviation-fit-seed S` | Seed used once for fitting |
| `--preflop-deviation-samples M` | At least two held-out physical worlds per seed |
| `--preflop-deviation-seeds T,U` | 1..64 unique seeds, each different from S |

An additional optional `--preflop-deviation-retention-gate` requires all four
options above. It selects `PreflopDeviationFitMode::RetentionGated` through
`evaluate_preflop_deviation_with_fit_mode`. `fitMode` records
`local-regret-matching` (default) or `retention-gated`. The gate uses the candidate
baseline to value an own preflop key until its eighth visit, then activates local
regret matching. All own actions are enumerated and updated from the first visit,
including those behind zero own reach. Baseline expectations use the same
cumulative f32 probability intervals and final-action remainder as replay.
Final pure argmax extraction and the visit threshold are unchanged; finite fit,
later policy changes and pure extraction can still produce negative gains.

The optional `preflopDeviation` object has schema
`solvers.multiway-preflop-deviation/v1` and scope
`all-preflop-decisions-with-frozen-postflop`. It calls
`evaluate_preflop_deviation_with_fit_mode(variant, threads, config, mode)` with the unpurified average
variant. Each seat fits a separate unilateral table against the same frozen
profile; tables are not combined into a joint deviation. The table may change
multiple own preflop decisions, including those behind zero own baseline
reach. Every postflop decision and unretained preflop key uses the specified
candidate baseline, without regret-greedy substitution. Only keys with at least
8 fitting visits enter the table; this is a visit threshold, not an ESS gate.
Fit and replay use reference keys only for preflop actions and candidate keys
for frozen baseline lookup. Fit visit counters use checked u64 arithmetic,
including the existing all-street fitting path; overflow is an error rather
than silent saturation. Ordinary-budget arithmetic and RNG remain unchanged.

Fit is independent of the held-out worlds and happens once. Each `heldOut`
entry reports per-seat `baseline`, `deviating` and signed paired `gains` with
standard errors and approximate 95% CIs over **all** worlds, including
unsupported-key fallback. Negative gains are retained. `coverage` separates
trained actions from baseline fallbacks by seat/street; `candidatePolicyCoverage`
counts sources on the unmodified baseline only. Intervals are per seat, not a
simultaneous bound across seats/seeds. Small gain or low fitting coverage does
not certify a full best response, exploitability, or equilibrium.

Held-out results are accumulated in sample-id order with at most 4096 sample
results buffered (`maxBufferedSamples`). Fitted tables grow with visited
preflop keys; this buffer bound does not cap their memory, the solver, or thread
stacks. `fitElapsedSecs` and each held-out `elapsedSecs` report diagnostic time.
`fitPolicyFingerprint` hashes sorted fitted actions only, not baseline identity;
retain checkpoint/config/source identity separately. Without these options the
new JSON field is omitted. Ordinary evaluations, stopping, training defaults and
checkpoint/solution formats are unchanged; other audit budgets still apply.

For a local strategy-quality diagnostic, add `--endpoint-prefix PATH`
with `--endpoint-fit-samples N`, `--endpoint-fit-seed S`,
`--endpoint-samples M` and `--endpoint-seeds T,U`. Both sample budgets must
be at least 2. The 1..64 held-out seeds must be unique and differ from the
fit seed. `--endpoint-min-fit-ess` defaults to 64 and must be finite and at
least 2. Explicit endpoint options require an endpoint; no partial schedule
is silently ignored. A preflop or postflop decision with 1..8 legal actions and
current-street recall is accepted, including `root`. Terminal paths remain
invalid. Requests are checked before fresh training.

Repeat `--endpoint-prefix` for up to eight unique decision histories. All share
one restored solver, with separate fits and held-out evaluations in request
order; each endpoint receives the full declared sample budget. Duplicate
histories (including aliases) and excessive counts fail. One endpoint retains
the existing singular output shape. Multiple endpoints use `endpointDeviations`
or `endpointCounterfactualDeviations` instead of the singular field. They do not
compose several changes into one candidate strategy. Batching avoids repeated
session construction when comparing root, 3bet, 4bet and 5bet decisions.

`--endpoint-target both` evaluates actual-prefix and opponents-prefix
independently on the same frozen preflop profile. It runs actual then
opponents for each requested endpoint, giving each target the full declared
fit and held-out budgets (up to eight paths / sixteen fits). Both output
families are retained; populations, denominators and fitted action tables
remain separate. It does not pool gains, and it rejects postflop endpoints
before fresh learning. Default and single-target output shapes stay unchanged.

At a preflop endpoint the proposal incorporates exactly the preceding partial
preflop path; at root that path is empty. At a postflop endpoint it uses the
complete preflop trunk and weights the remaining forced postflop actions.
It never conditions the deal proposal on the endpoint's own action or any
future action. The separate baseline-only `--condition-sampler preflop-proposal`
mode remains restricted to postflop endpoints.

The optional `endpointDeviation` result fits one frozen action table using
the corrected preflop proposal. At each endpoint actor's legal information
key it compares all actions on fit worlds, retains the first maximum only
when its estimated gain is positive and its fit weight ESS meets the gate,
and otherwise keeps the baseline. The same table is evaluated on every
held-out seed, with no held-out selection or refitting. All configured
buckets remain in the output, including unsupported/missing keys.
Fit action estimates use the positive-weight worlds for that key; with fewer
than two such worlds their mean/error fields are null and no action is retained.
Held-out moments include every accepted world, including zero-weight worlds.

Only the first action at the specified endpoint may change. Earlier actions
remain forced with baseline reach; all later decisions by every seat, including
the endpoint actor, use the baseline. The endpoint action's baseline probability
is never included in the prefix weight, even when it is zero. Each baseline /
candidate pair shares its physical world and continuation random stream.
Action choice uses the actor's own information key, never an opponent's cards
or a per-world maximum utility. Payoff evaluation still uses the full world.

Held-out signed gain and delta standard error use **all** prefix weight in
the denominator. Unsupported keys keep the baseline and contribute zero gain;
they are not removed from the denominator. Zero total weight gives a null
estimate. Read retained-key weight coverage, ESS, maximum normalized weight,
and source provenance beside the gain. Little retained coverage and a small
gain cannot establish a strong strategy. These are conditional whole-hand
utility differences (bb for cash), not root reach, a complete best response,
global exploitability or a multiplayer equilibrium guarantee. Prefix populations
change with the baseline profile and are not paired across different profiles.

`--endpoint-target actual-prefix|opponents-prefix` selects the population for
this endpoint diagnostic and requires `--endpoint-prefix` when explicit.
The default `actual-prefix` preserves the existing output and calculations
above. `opponents-prefix` accepts only preflop decisions and emits the separate
optional `endpointCounterfactualDeviation` field for a single endpoint; it
omits `endpointDeviation` and uses the plural form for multiple endpoints.
The new result records target, excluded actor, proposal kind, the number of
verified class contexts, all 169 own-prefix probabilities, and the complete
fit/held-out evaluation. Without this option and with at most one endpoint,
no new JSON field is emitted.

The new proposal weights each physical seat's original range by its prefix
action probabilities, except the endpoint actor, whose original range is kept.
Folded seats still participate in card removal. Actual f32/CDF probabilities
and proposal floors are corrected. All 1,326 combos must match the 169 preflop
classes at every path/endpoint context, and own reach must be constant within
each class. Unsupported mapping, postflop and terminal contexts fail explicitly.
Zero-own-reach hands still replay every fitted endpoint action, including folds
and jams. A zero opponent target or incompatible ranges remain errors.

The new API independently fits its own table. All its weights, fit ESS, signed
held-out gains and unsupported-key denominators refer to the opponents-prefix
target. Read the nested `evaluation` with that target; it is not an actual-prefix
evaluation of the old fitted table. Raw relative-weight means and aggregate
gains from the two targets must not be directly ranked. This read-only normalized
diagnostic is neither an unbiased CFR update nor root reach or an equilibrium
certificate. The original target's fit/seed/serialized behavior stays unchanged.

For example, an initial timing pilot can use:

```text
--endpoint-prefix raise-to:2000/raise-to:6500/call:6500/fold/fold/fold/call:4500/check/check/check/check/check/check
--endpoint-fit-samples 4096 --endpoint-fit-seed 601
--endpoint-samples 8192 --endpoint-seeds 701 --endpoint-min-fit-ess 64
```

Those labels describe one particular six-seat tree; the requested path must
exist in the supplied config. Other audit budgets still apply independently.
For an endpoint experiment, explicitly reduce incidental global candidate
budgets if they are not part of the experiment. This diagnostic does not
write a checkpoint or change any learned policy or normal stopping rule.

Held-out worlds are evaluated in parallel, then accumulated in sample-id order,
so changing `--threads` does not change any reported value or deal-attempt
count. The parallel evaluator retains at most 4096 samples of temporary scalar
results at a time; its extra memory is therefore bounded independently of
`--samples`.

The ordinary `--br-traversals` deviators are trained once with `--br-seed` and reused unchanged for every
evaluation seed. Results remain separate by seed; the tool does not select,
average, or pool them. The reported deviation gain covers the solver's two
fixed candidates and the no-deviation option. The trained candidate is used
at retained information sets and otherwise falls back to the solver's main
regret-greedy candidate. The reported deviator coverage describes training;
the ordinary `evaluations` output does not expose held-out replay fallback counts.
The result is not a full best response, exploitability measurement, or Nash
certificate.

`--node` accepts `root`, a 32-digit history key, or slash-separated exact
action labels or zero-based indices. Node export deliberately accepts only
preflop decision nodes. At preflop the production EHS2 adapter maps buckets
directly to the standard 169 hand classes, so each row is suitable for an
action-frequency comparison after aligning action sizes and labels with the
reference solver. Postflop hand export needs an explicit board and physical
world/blocker contract and therefore fails instead of inventing one.

An `average-observed` row has positive raw average-strategy mass and contains
the normalized average policy. An `unvisited` row has no touched checkpoint
policy entry (even if its dense storage column was preallocated).
A touched column with zero average-strategy mass is marked
`current-regret-fallback-omitted` and has a null strategy: the solver can
derive a current regret-matched fallback for such a column, but that fallback
is not evidence of an observed average policy and is intentionally excluded
from comparison output.

For each exported node, `--node-frequency-samples` draws complete physical
card worlds from all configured seat ranges. It multiplies the sampled hands'
average-policy probabilities along the unique public action path to obtain a
reach weight, then reports each target action's reach-weighted conditional
rate. This accounts for folded players' dead cards and card removal; directly
weighting the 169 target-seat classes does not. Set the sample count to zero
to omit this estimate. A positive count must be at least two.

The JSON includes the estimated public-node reach probability, effective
sample size, first-order delta-method standard errors, and the fraction of
target reach weight whose prefix or target strategy used a current-regret or
uniform fallback. The fallback categories can overlap. A positive fallback
fraction means the reported rate is for that composite fallback profile and
is unsafe as a strict observed-average-policy comparison. Zero estimated
reach produces null conditional rates. The conditional rate is a
self-normalized ratio estimate; it is not claimed to be finite-sample
unbiased. Physical worlds use independent, domain-separated random streams
for every `(seed, sample id)`, and worlds are accumulated online rather than
retained in memory.

Public betting states, action menus and normalized per-key policies are
prepared once for a node-frequency estimate. Card-dependent bucket lookup,
physical-world RNG and sample-ordered moment updates still run for every world.
`nodes[i].frequencyElapsedSecs` times preparation and frequency sampling only,
excluding solver construction, hand-table export and other evaluation phases.
It is omitted when frequency estimation is disabled.

To examine postflop coverage inside a deep branch, add repeatable
`--coverage-prefix` paths and optionally `--coverage-samples`, for example:

```text
--coverage-prefix fold/fold/fold/fold/raise-to:3000/raise-to:10000 \
--coverage-prefix fold/fold/fold/fold/raise-to:3000/raise-to:10000/call:7000 \
--coverage-samples 131072
```

These paths must identify known public decision nodes; unlike `--node` hand
export, prefixes can be on any street. Up to 64 unique histories are accepted;
aliases that resolve to the same history fail. `--coverage-samples` requires a
prefix and must be at least two; otherwise the count defaults to `--samples`.
The same evaluation seeds are retained separately. Prefix collection does not
change ordinary evaluation, training, checkpoints or solve defaults.

`coverageEvaluations` is present only when prefixes are requested. Its `result`
is a separate baseline-only evaluation: it uses the ordinary physical-world and
action RNG streams but skips all deviation replays and sets
`deviation_gain_lower_bound` to null. Its unconditional coverage can be compared
with each `prefixes[i].candidatePolicyCoverage` on the same samples.
`reachedSamples / result.samples` is the observed prefix reach fraction.
`trajectoryVisitsByStreet` counts trajectories with at least one subsequent
decision on each street; all-in runouts are excluded. Per-seat decision counts
can exceed this trajectory count. The decision at the prefix itself is included.
Nested prefixes overlap and must not be pooled as disjoint strata. Zero visits
mean unmeasured coverage; positive average mass alone does not imply convergence.

Prefix counters are reduced in sample order. Parallel chunks shrink with the
number of prefixes to keep their result buffers near 8 MiB, excluding allocator
overhead and solver memory. This remains bounded as total samples grow. Prefix
reach is ordinary baseline trajectory sampling, not forced branch exploration;
rare branches still need enough worlds. This diagnostic does not export a
board-specific strategy or establish a strong best response.

For branches too rare for ordinary trajectory coverage, request separate
forced-prefix baseline diagnostics with `--condition-prefix` and
`--condition-samples`, for example:

```text
--condition-prefix raise-to:2000/raise-to:6500/call:6500/fold/fold/fold/call:4500 \
--condition-prefix raise-to:2000/raise-to:6500/call:6500/fold/fold/fold/call:4500/check/check/check/check/check/check \
--condition-samples 131072
```

Conditional diagnostics are disabled by default (`--condition-samples 0` and
no prefixes). Enabling them requires at least two worlds and 1–64 prefixes.
`--condition-sampler` defaults to `root`, the original root-world estimator
described below. Explicitly setting either sampler requires both
`--condition-prefix` and `--condition-samples`.
A prefix without a positive sample budget, or a positive budget without a
prefix, fails before restoration. The same root/hex/action-label/action-index
syntax is accepted on every street. Endpoints must be known decision nodes;
duplicate resolved histories and terminal endpoints fail before diagnostics.
Nested prefixes are allowed and their results overlap.

`conditionalEvaluations` is present only when root sampling is enabled. Each
entry retains an evaluation `seed`, a `prefixes` context list and the serialized
core `result`.
Contexts include `requested`, hex `history`, `actionIndices`, `actionLabels`,
`actor`, `street` and `activeOpponents`, in request order. The numeric results
use the same prefix order. Evaluation seeds are kept separate, with
`--condition-samples` physical worlds per seed shared across prefixes;
`result.total_deal_attempts` counts the deals once. The core result retains
snake_case field names. No new timing field is added, and this work occurs
after the existing construction, training and evaluation timers.

Physical worlds still come from the configured joint seat ranges, including
folded players' dead cards. The evaluator follows each requested public path
and uses its baseline action probabilities as a reach weight before rolling
out the baseline from the endpoint. Conditional utilities and coverage must
therefore be interpreted together with estimated reach and effective sample
size. Forcing a path does not make its worlds equally likely under the policy,
and a large nominal sample count can still have low effective sample size.
Zero reach cannot establish a conditional utility or coverage estimate.

For each `result.prefixes[i]`, `reach_probability` reports a mean and standard
error; `effective_sample_size`, `max_normalized_weight` and
`positive_weight_samples` describe weight concentration. `seats` contains
whole-hand utility conditional on the prefix, without rebasing invested chips
at that node. `coverage_by_street` and `coverage_by_seat` describe decisions at
or below the endpoint; street arrays are preflop/flop/turn/river. The source
fractions weight decisions by prefix reach, while `trajectory_probability`
and `decisions_per_prefix_trajectory` use reach weight as their denominator.
Raw positive-weight visit counts are forced continuations, not ordinary reach
counts or independent decision-level sample sizes. Standard errors use
trajectory-level numerator/denominator covariance and a first-order delta
approximation; they are not confidence intervals or a low-ESS accuracy
guarantee. Zero denominators produce null estimates.

These results describe the frozen baseline, including its policy fallbacks.
Coverage source fractions describe the continuation only. The separate
`prefix_current_fraction`, `prefix_regret_fallback_fraction` and
`prefix_uniform_fallback_fraction` report the reach-weight fraction of worlds
using each source anywhere in the forced path. These flags can overlap.
An average continuation fraction of one therefore does not prove that the
entire path used observed average policies. Tiny positive reach whose squared
weight underflows fails explicitly rather than silently becoming zero evidence.
They do not train a deviator, alter the checkpoint, export a board-specific
strategy, or certify convergence or EV loss on an unobserved branch. Ordinary
evaluations and optional `--coverage-prefix` diagnostics still run separately
with their existing seeds and budgets; enabling conditional diagnostics does
not replace those checks or turn forced paths into ordinary observed reach.

For production Holdem postflop prefixes, `--condition-sampler preflop-proposal`
instead proposes worlds from the preflop-conditioned joint seat ranges:

```text
--condition-sampler preflop-proposal \
--condition-prefix raise-to:2000/raise-to:6500/call:6500/fold/fold/fold/call:4500 \
--condition-prefix raise-to:2000/raise-to:6500/call:6500/fold/fold/fold/call:4500/check/check/check/check/check/check \
--condition-samples 131072
```

This mode requires postflop decision endpoints; root, preflop decisions and
terminal endpoints are rejected before diagnostics. The CLI replays the public
action indices and groups prefixes by the exact preflop trunk ending at the
first postflop state. It evaluates groups in first-request order, retaining
request order within each group. There are `--condition-samples` physical
worlds **per seed per trunk**, so adding another trunk adds a full sampling
budget. Prefixes sharing a trunk share their proposed worlds and overlap.
Different trunks use seed/sample-id keyed streams but different proposal
distributions; they are not a paired comparison or independent replicates and
must not be pooled as such.

Proposal output appears under `preflopConditionalEvaluations`, with one
`{seed, prefixes, result}` entry per seed/trunk. Prefix contexts have the same
fields as root sampling; each group's numeric results follow its context
order. `conditionalEvaluations` is omitted in this mode. No conditional timing
field is added. The root-mode JSON is unchanged when this option is absent.

The proposal incorporates each seat's preflop action probabilities into its
own range and retains the joint card-removal constraints, including folded
seats. Positive target weights are scaled per seat, rounded to f32 and floored
at the recorded `proposal_floor_fraction`; the evaluator corrects against the
actual cumulative draw intervals. Postflop forced actions and this proposal
correction supply the remaining relative importance weights.
**`result.prefixes[i].relative_weight_mean` is not an absolute
root-reach probability.** The preflop normalization constant cancels from
conditional utility and coverage ratios; it does not make absolute reach
available. Use the separate ordinary `--coverage-prefix` evaluation for
observed root reach. Its sample budget is still controlled independently by
`--coverage-samples` (or `--samples`), and it is not reweighted by the proposal.

`result.proposal` records `preflop_actions`, `preflop_history`, the root and
proposal range fingerprints, `positive_target_combos_by_seat`,
`floor_adjusted_combos_by_seat`, `target_scale_by_seat`,
`proposal_floor_fraction`, and the sampler's `pilot_samples`/`pilot_accepted`.
These identify the prepared proposal and its collision-rejection feasibility.
An empty target range, incompatible joint support or failed acceptance pilot
is an error, not a zero conditional estimate. `result.total_deal_attempts`
counts whole-tuple rejection attempts for that group once per physical world.

Conditional utilities remain whole-hand utilities, and suffix source fractions
still describe the composite frozen profile. Prefix source flags may overlap.
Proposal ESS and maximum relative weight describe this proposal's concentration;
they are not directly comparable sample counts to the root-world estimator and
do not certify precision. This research diagnostic neither changes the solver's
configured training ranges nor trains or resumes the checkpoint.

An active solve can be audited from its atomically published periodic
checkpoint. The JSON records the restored sweep count, so each audit remains
a frozen comparison even if training continues and later replaces the live
checkpoint.

`constructionElapsedSecs` times the production-session construction call after
CLI config overrides: lowering, abstraction/cache preparation, checkpoint read,
resource admission, tree materialization, arena validation/commit and state
restore. It excludes initial argument/config-file reads, node exports,
deviation training, held-out evaluation, JSON output and solver disposal.
Construction uses the resolved `--threads`/config thread count, including for
checkpoint restoration; its serial resource preflight remains unchanged.
