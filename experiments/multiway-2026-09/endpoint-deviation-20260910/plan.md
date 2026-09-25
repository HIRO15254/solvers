# Conditional one-step endpoint deviation

Status: implementation, verification and the first frozen-baseline experiment
completed. The [validation report](plan.md)
records the evidence. The original design below remains as the predeclared
method; learned policies, defaults, checkpoint identity and artifact format
are unchanged.

The main fit retains 27/32 buckets, covering 97.264% / 97.282% of held-out
prefix weight. Its signed gains are 27.920 ± 0.586 bb / 27.660 ± 0.590 bb
(one standard error) for seeds 702/703. The pilot retained no keys and had
zero gain; its purpose was timing/invariants. This is a local weakness of the
frozen composite baseline. Older root-reach estimates around 1e-8 have low
ESS, so it is not a large measured gain per starting hand. No production
learning improvement or default promotion follows from these numbers.
Next apply the same diagnostic to the already selected HU 3bet/4bet river
endpoints with root reach and candidate coverage beside each conditional gain.
Keep the broader goal active until balanced learning improvements are verified.

The [corrected preflop proposal evidence](../preflop-proposal-20260910/README.md)
made the selected rare three-player river population measurable while retaining
folded-seat blockers and correcting proposal rounding/floor mass. Its source
coverage measures which stored/fallback policies were used, not their strategic
quality. The [opponent-exploration screen](../opponent-exploration-20260910/plan.md)
therefore needs independent deviation evidence before a parameter is promoted.
This plan narrows the next step from the broader
[depth-improvement plan](../average-depth-20260910/plan.md).

## First bounded target

Start with one frozen 32,768-sweep baseline and the genuine three-player
check-through river public endpoint. Only that endpoint's acting seat may
change its first action. Every earlier action is forced under the frozen
baseline reach, and every later decision by every seat follows the baseline.
Validate the endpoint actor, street, active-opponent count, history and menu
before sampling. The exact endpoint means the specified action history;
public boards and private cards still vary across sampled physical worlds.

The candidate chooses by the endpoint actor's legal `InfoKey`, obtained from
that actor's own hand and public context. It must not choose by an opponent's
hole cards or by inspecting a sampled continuation's utility. Payoffs may use
the full physical world; action selection may use only the frozen key/action
table fitted from independent worlds.

This measures a restricted local improvement opportunity in conditional bb.
It is not a full best response, global exploitability, a Nash certificate or a
complete ranking of multiplayer profiles.

## Fixed target and paired estimates

Let `P` stop immediately before the endpoint action, `C(x)` be the current
corrected preflop proposal weight, and `sigma` be the frozen baseline profile.
For each accepted physical world `x`, use

```text
W(x) = C(x) * product(sigma(a_h | I_h), h in the postflop part of P).
```

Use the same actual action-draw intervals as the existing conditional replay.
The preflop proposal's unknown common normalization constant cancels in the
ratios. `W` is not absolute root reach. Never multiply it by the candidate
endpoint action's probability, including when that baseline probability is
zero. Folded seats remain in the joint deal sampler as blockers.

The exploration pilot reinforces the need to start with this single reference
baseline: epsilon 0.06 leaves exact-river prefix ESS at only 618 / 645 from
131,072 proposal worlds, versus about 12,000 for epsilon 0. A 65,536-world fit
under the former population may retain few keys at ESS >= 64. A near-zero
gain caused by low candidate coverage is not evidence of a strong baseline.
Keep retained reach-weight coverage beside the gain; fixed-reference suffix
comparison and each profile's own conditional population are distinct targets.

In fit worlds, enumerate every legal endpoint action. Replay the baseline and
each action with the same world and cloned action RNG. For each endpoint key
`I`, accumulate

```text
fit_gain(I, a) = sum(W * 1[key=I] * (U_a - U_baseline))
                 / sum(W * 1[key=I]).
```

Choose at most one action per key after aggregation, with a deterministic
action-order tie break. Never average a per-world maximum: that would let the
choice depend on hidden opponent information. The proposed initial retention
gate is fit weight ESS >= 64 for a key. This is a fixed evidence threshold,
not a confidence guarantee. Keep baseline behavior for unsupported keys or
when the best estimated gain is nonpositive. Freeze the action table and all
retention rules before held-out evaluation.

On held-out worlds, evaluate that frozen table against the same baseline:

```text
gain = sum(W * (U_frozen_candidate - U_baseline)) / sum(W).
```

The denominator includes all prefix weight, not just keys retained in fit.
Unsupported keys contribute zero gain because they keep the baseline action.
Zero-weight worlds contribute zeros to the moments and no conditional support.
A zero total denominator produces an unavailable estimate, not a zero gain.

Use joint numerator/denominator Welford moments, as in the existing conditional
evaluator. One observation is one accepted physical world, including its
paired continuation difference. If `d` is the per-world utility difference,
the corresponding delta-method variance estimate is

```text
SE^2 = n/(n-1) * sum((W * (d - gain))^2) / sum(W)^2.
```

Report the signed gain and standard error, including negative results. Do not
select an action, retention rule, seed or profile by its held-out result and
then present that same result as independent validation. Low ESS can still
hide important tails; the delta standard error is not an exact finite-sample
interval or a guarantee that every relevant world was observed.

## Minimal replay integration

The proposed insertion point is the internal `ForcedPrefixReplay` used by
`solver/conditioned.rs` and `solver/eval.rs`. Add an optional endpoint action
override that applies only when `depth == actions.len()`. Existing calls would
pass no override. Keep `actions` ending before the endpoint and keep
`skip_weight_actions` equal to the preflop trunk length.

Do not append the candidate action to the forced prefix: the existing replay
would then multiply its baseline probability into reach and change the target.
Do not reuse `evaluate_world`'s general `deviator` argument: it can change the
same actor's later turns and falls back to regret-greedy actions at keys absent
from the trained table. Unsupported endpoint keys must instead preserve the
baseline exactly.

The override still goes through `paired_profile_action`, consuming one draw
even when the endpoint action is fixed. Baseline and candidates share the
same action stream up to their divergence. Subsequent branch lengths can
differ; common random numbers preserve each marginal distribution but do not
guarantee a variance reduction for every comparison. Assert that every replay
of a world obtains the identical pre-endpoint weight.

Use distinct, predeclared fit and held-out seeds or RNG domains. Preserve
indexed sample-order accumulation and bounded chunks so thread count does not
change results and memory does not grow with the number of worlds. Fit stores
per-key/action moments; it does not retain all physical worlds. Record the
frozen table, fit settings and baseline/proposal identities with the result.

## Sampling budget and exact-river option

The initial proposed pilot is 4,096 fit plus 8,192 held-out worlds at one
endpoint. Time construction, fit and held-out replay separately and retain
actual terminal evaluations, action count, accepted worlds and deal attempts.
Use at most eight endpoint actions in this first implementation; reject a
larger requested menu rather than silently omit actions.

After checking pilot cost and output invariants, the proposed main diagnostic
is 65,536 fit worlds and 131,072 held-out worlds for each of two distinct
held-out seeds. Both held-out runs use the same previously frozen fit table.
Declare seeds and an external timeout before running. This is a proposed
budget, not authorization for an unbounded search or repeated precision runs.

The first implementation experiment predeclares pilot fit seed 601 and held-out
seed 701, then main fit seed 602 and held-out seeds 702/703, with the respective
sample budgets above, ESS gate 64, eight threads and 900 seconds per process.
The main fit table is frozen once for both held-out seeds. Local runs use the
retained epsilon-0 32,768-sweep checkpoint and create no additional checkpoint.
The ledger is `runs/endpoint-deviation-20260910/experiment.json`. The pilot
selects only whether the fixed main budget is affordable and valid; it is not
used to choose candidate actions or thresholds for the main fit.

Initially reuse sampled suffix replay. A later exact-river option can
enumerate the baseline action tail for each sampled physical world if a
public-tree preflight admits, for example, at most 64 terminal leaves. Sum
terminal utilities with actual baseline action probabilities; later decisions
by the endpoint actor also remain baseline decisions. Select exact versus
sampled mode once using public structure, not observed utilities or partial
results. Fail an exceeded bound rather than reporting a truncated exact sum.
Exact action enumeration removes continuation-action noise, not uncertainty
from sampled boards and private cards.

## Required output and tests

Retain prefix context, action labels, frozen candidate actions, fit counts and
key-level ESS, held-out signed gain/SE, prefix ESS and maximum normalized
weight, retained-key prefix-weight fraction, and baseline source coverage.
Keep the full configured bucket denominator, including missing/unsupported
keys. Keep absolute root reach and unconditional diagnostics separate.

The first implementation must test:

- A zero-baseline-probability endpoint action remains a legal deviation and
  does not alter prefix weight; a zero-reach prefix remains unavailable.
- Only the endpoint changes, even when the same actor has later decisions.
- Worlds with the same actor `InfoKey` but different opponent cards choose
  the same frozen action. A hidden-card toy oracle must reject a per-world
  argmax masquerading as a legal policy.
- Unsupported keys keep baseline behavior and remain in the main denominator.
- Independent fit/held-out schedules, deterministic ties and thread-count
  equivalence; changing held-out payoffs cannot change the frozen table.
- Disabled override preserves the existing baseline replay, RNG behavior,
  source coverage and weight exactly.
- Paired ratio moments match an independently computed weighted oracle with
  correlated weight and payoff. Tiny exact river tails match enumeration if
  that optional mode is implemented.

## Comparing exploration settings

Changing a learned profile changes both its preflop posterior and suffix
policies. Fitting and evaluating under each profile's own proposal describes
different conditional populations. The same numeric seed does not make those
different samplers' physical worlds paired. A smaller local gain in that
comparison is not, by itself, evidence of better overall strategy.

First establish this diagnostic against one frozen baseline. A later common-
population comparison must prepare the proposal and all prefix reach weights
from one reference profile, then hold those weights fixed while evaluating
each candidate suffix. Replaying the prefix with the candidate's probabilities
would undo that control. Require compatible game/range/abstraction/menu
identities, identify the reference population explicitly, and preserve root-
reach context. Broader promotion still needs balanced endpoints, independent
training seeds and calibrated-compute controls from the exploration plan.
