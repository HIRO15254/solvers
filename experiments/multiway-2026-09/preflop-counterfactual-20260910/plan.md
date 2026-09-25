# Preflop endpoint sampling coverage: counterfactual target

Status: implementation and seven workspace/research/build checks passed.
Workspace tests: 803 passed, 30 ignored; research examples: 39 passed; research
core: 282 passed, 1 ignored (overlapping suites). Both retained-state targets
passed validation and byte-identical summary regeneration. The amended Python
validator passed 21 tests. Deep fit ESS eligibility improved from 110/60/25 to
169/169/169 keys; full-process time was 199.873 versus 213.916 seconds. This is
diagnostic coverage evidence, not additional training or a quality promotion.
See the [completed report](README.md).
The fixed seed-29 comparison and production-drift validation are complete.
2026-09-10.

## Observed limitation

The existing endpoint diagnostic draws from all players' reaching ranges.
Its signed held-out gain uses all actual prefix weight, including unselected
keys. That remains the correct definition of the existing conditional metric.
The seed-0/11 reports also retain many deep keys below fit ESS 64. Changing
the metric silently or interpreting a negative fitted-candidate gain as a
convergence certificate would conceal this missing evidence.

The retained [32,768-sweep baseline](../preflop-endpoint-20260910/README.md)
has fit ESS at least 64 in 169/169 opener keys, 169/169 SB-unopened keys,
110/169 keys facing the 3bet, 60/169 facing the 4bet and 25/169 facing the
5bet. The low-ESS keys carry only 1.363%, 0.938% and 0.444% of the respective
actual fit weight. Thus the limitation is per-hand/off-path coverage, not a
claim that the existing aggregate reaching-population estimate discards most
of its weight. A different target must not be presented as a correction to
that existing metric or as a demonstrated root-EV gain.

## Mathematical distinction

CFR's counterfactual value uses chance and opponents' prefix probabilities,
excluding the acting player's own prefix probability. MCCFR corrects sampling
with the actual sampling probability; those two probabilities are distinct.
The original theorem's equilibrium setting is two-player zero-sum with perfect
recall, so it does not establish convergence for this multiway abstraction.
See Lanctot et al., [MCCFR, equations 4 and 6](https://www.cs.cmu.edu/~kwaugh/publications/nips09b.pdf).

The following is this repository's diagnostic application, not a result from
that paper. Let a legal physical deal be `d`, including folded seats, let
`P(d)` be its original chance density, and let `L_j(d)` be seat j's action
probability product along a forced preflop path. For endpoint actor i:

```text
actual-prefix target(d)        = P(d) * product_j L_j(d)
counterfactual-prefix target(d)= P(d) * product_{j != i} L_j(d)
actual-prefix target(d)        = counterfactual-prefix target(d) * L_i(d)
```

At a fixed own information set, the own-prefix product is constant when the
information representation preserves those earlier own decisions. For the
production 169-class preflop abstraction, verify this across every physical
combo assigned to that key and every prior own action, rather than extending
the assumption to postflop current-street recall. If this check holds and the
product is positive, the factor cancels from a per-key conditional action-gain
ratio. Aggregate key weights still differ. Zero-own-reach keys have no mass in
the existing metric but may have counterfactual mass.

## Bounded prototype after the current gates

Use a retained production checkpoint with no new learning. Compare the current
proposal against a pure counterfactual proposal at the same five preflop
endpoints, fixed fit/held-out budgets and multiple evaluation seeds. The latter
uses the actor's original range and each other seat's range times its own
prefix factors. Retain all folded cards and whole-tuple collision rejection.
Correct actual f32/CDF probabilities and floor adjustments, as the existing
`PreparedPreflopProposal` does. Multiplying by `L_i(d)` then recovers weights
for the existing actual-prefix diagnostic from the same physical sample.

That recovery means proportional target weights within the new proposal,
not equality to the old proposal's raw weight values. Per-seat max scales and
card-removal rejection normalizers differ between the two samplers. Never
compare their relative-weight means directly. Even the ratio
`E[W_CF * L_i] / E[W_CF]` under one proposal is a ratio of the two target
normalizers, not absolute root reach.

Implementation would need a separate preparation path that leaves the actor's
factor at one; the existing preparation always includes every actor. Force the
whole preflop path with `skip_weight_actions = path.len()` so replay does not
multiply those prefix factors again. A world with `L_i = 0` must still reach
the suffix for the counterfactual target; only its actual-prefix contribution
is zero.

Keep the two target identities explicit in metadata and in separate reported
denominators. The retained-key fraction, signed gains and every own key must
remain visible for each target. Relative proposal weight is not absolute root
reach. Neither self-normalized ratios nor their per-key finite-sample estimates
should be called unbiased raw CFR updates. Root reach stays independently
estimated. A broader proposal may spend more work on low-value branches; an
ESS increase alone is not a success criterion.

Before empirical comparison, verify exact small-game/card enumeration with
unequal own reaches, an own zero-reach key, a zero opponent-reach path, folded
blockers and nonuniform ranges. Check both weighted targets, per-key agreement
where own reach cancels, actual CDF correction, zero denominators, deterministic
threads and unchanged solver state. Reject unsupported abstraction/context
instead of applying the factorization approximately. Keep independent fit and
held-out action randomness and signed gains over complete target weight.
An identically zero opponent target has undefined conditional utility; the
current proposal rejects it with `EmptyRange`. Missing legal joint tuples or
failed collision sampling also remain explicit errors, never successful zero
gain observations. Preserve these boundaries in the prototype tests.

Do not reuse these normalized diagnostic weights as learning updates. A
learning proposal needs its own inclusion-probability and counterfactual
unbiasedness argument, including traversal and average-strategy estimators.

## Reuse of expensive learned states

The current one-shot research runner consumes the trained solver and returns
normalized policy observations; it does not save a complete state. Thus the
seed-0/11/29 131,072-sweep profiles cannot be reevaluated from their JSON alone.
Use the existing 32,768-sweep retained checkpoint for an initial diagnostic
prototype. Before larger new production-compatible learning cohorts, consider
a separate train/checkpoint/evaluate workflow that uses the borrowed writer.
Preserve experimental-average isolation: variants whose average update is not
the production algorithm must not be persisted as ordinary compatible state.
No such persistence API or additional cohort is implemented by this note.

## Smallest implementation boundary from the audit

Preserve `evaluate_endpoint_deviation_preflop` and its existing output as the
actual-prefix API. A new explicitly preflop-only counterfactual method can
reuse the endpoint fit/replay machinery with a separately prepared proposal.
Its result must identify the target, excluded endpoint actor, proposal kind,
path, profile variant and config/abstraction fingerprints. Calling both methods
on the same retained solver permits an initial comparison without changing
the old API's fit procedure, random schedule or serialized output.

`prepare_preflop_proposal` currently multiplies every seat's prefix factors.
A separate preparation path can replace only the endpoint actor's proposal
factors with one and retain that actor's original factor table separately.
Reuse `PreparedPreflopProposal::from_factors` for actual f32/CDF correction and
whole-tuple card rejection. An actual zero-own-reach combo is currently removed
there; an entirely empty actor range returns `EmptyRange`.

The replay implementation in `eval.rs` also stops after a forced action when
its accumulated weight is zero, and `endpoint_sample` stops candidate replay
on a zero-weight baseline. Therefore counterfactual replay must start with
`W_CF` and skip all preflop path weighting, even when `L_i` is zero. If a later
extension reports actual-prefix quantities from those same worlds, multiply
by `L_i` only in the separate aggregate. A counterfactual-fitted table evaluated
with actual weights is a distinct quantity from the existing actual-fitted
table. Independent target fits need two ESS gates/tables and, for shared
held-out worlds, the union of selected action replays. Count physical terminal
replays once. These extra dual-target features are not prerequisites for the
smallest separate-method prototype.

The abstraction trait exposes a fingerprint and bucket count, but neither
proves 169-class mapping. The production table adapter maps preflop combos to
`cards::class_index`. A conservative new-method gate can enumerate all 1,326
combos at every path/endpoint context, require exactly 169 buckets and exact
class-index mapping, then check that each endpoint class has a constant own
prefix factor across its physical combos. Class-index identity is a bounded
implementation restriction; factorization and own-factor constancy are the
mathematical conditions behind the per-key cancellation claim. Reject a custom
169-bucket abstraction that mixes those classes, and reject postflop contexts.

Alongside the finite enumeration tests above, preserve a regression for every
old actual diagnostic field, excluding elapsed clocks. Check zero-own-reach
counterfactual fold/jam candidates, unsupported-key and signed-gain handling,
custom-abstraction rejection, thread equality and unchanged solver state. The
new method would remain a read-only normalized diagnostic, not a CFR learning
update or a multiway equilibrium certificate.

## Fixed retained-state pilot

After the seven workspace/research/build gates pass, compare exactly two
serialized processes: `actual-prefix`, then `opponents-prefix`, with the new
`mw_checkpoint_audit` binary. Each process restores the same existing
32,768-sweep K32 checkpoint once and evaluates the five already fixed decisions
in order: root, SB unopened after four folds, SB facing the 10bb 3bet, BB facing
the 21bb 4bet, and SB facing the 100bb 5bet jam. Repeatable `--endpoint-prefix`
shares the restored solver; it does not combine their deviations. No main-profile
training or cloud resource is included.

Keep the prior schedule: 65,536 fit worlds at seed 602, minimum fit ESS 64;
131,072 held-out worlds for each seed 702 and 703. Use 8 threads, 8GiB, the
existing warm EHS2 cache and a 900-second timeout per process. Incidental
ordinary evaluation is 128 worlds at seeds 101/202 with one incidental deviator
candidate traversal per seat; this read-only operation does not update the main
profile. Node-frequency sampling is disabled. Save both literal jobs, source/archive/
binary/config/input hashes and an immutable preexecution snapshot before the
first process. Preserve actual elapsed time and lifetime process peak. The
source is the new 174-file archive, separate from all earlier cohorts.

Both jobs also export all 169 raw policy-support rows at the five endpoints
and at BB's decision facing SB's 3bb open. That extra node was absent from the
old sixteen-node support fixture and is needed to independently reconstruct
BB's own 3bet prefix factor for the 4bet endpoint. Compare the five existing
support nodes to the archived baseline, and require identical raw support in
both new processes. Reconstruct every own-prefix factor using the actual f32
action CDF, including clipped cumulative intervals and final residual.

The actual-prefix results must reproduce every field of the old verified
baseline's five endpoint evaluations except elapsed clocks. At root and SB
unopened, the endpoint actor has not acted previously, so own prefix factors
are one and the new target must reproduce the actual result exactly. Preserve
all deep-node differences, full 169-key fit eligibility/weight partitions,
held-out signed gain with standard error, candidate-table coverage and cost.
No ranking from aggregate gains across the two targets is allowed. More
eligible keys at comparable cost is diagnostic coverage evidence, not a new
trained profile or a convergence certificate. Precision and per-key action
estimates must remain visible even if aggregate coverage improves.

The independent finite-world tests already cover both weighting laws and
positive-own-class cancellation. This empirical pilot evaluates each target's
own fitted table; it does not claim actual-prefix evaluation of the CF-fitted
table or root reach from relative proposal weights. Those are separate possible
extensions. Retain every case and do not tune the schedule after seeing results.

### Startup correction, before any endpoint observations

The original `runs/preflop-counterfactual-20260910` actual-prefix job incorrectly
requested `--br-traversals 0`. The existing CLI requires a positive budget and
rejected that job before config loading or solver construction (exit 1,
0.1027462 seconds, empty stdout). No endpoint observation or training resulted.
Keep its original jobs, preexecution manifest, validator archive, measurement
and `startup-failure.json` intact. The opponents-prefix job was never started.

The replacement `runs/preflop-counterfactual-br1-20260910` changes only this
incidental budget to `--br-traversals 1` in both jobs, and checks the six coverage
records accordingly. It reuses the exact verified 174-file source archive,
binary and seven successful verification logs. Record the startup-failure hash,
new validator/test hashes and both corrected literal jobs before retrying.
The checkpoint, five endpoint fit/held-out budgets, seeds and acceptance
conditions above remain fixed. This is a preflight protocol correction, not
selection based on endpoint results.

### Validator context-label correction

The replacement actual-prefix process completed successfully in 199.8725554
seconds. The first validator then rejected the output because it incorrectly
equated `context.actionLabels` (the prefix path) with the support node's
`actionLabels` (the endpoint's legal menu). The CLI has always exported those
as different fields; its root context has an empty prefix label list.
An independent direct comparison already reproduces all five historical
endpoint payloads except clocks and all five historical normalized support
nodes exactly. The six exported context paths also match their prefix labels.

Preserve this successful output and the original br1 validator/test archive
as `validator-v1.zip`. Correct the schema interpretation and its synthetic
fixtures, add a regression for distinct prefix/menu labels, and record an
explicit hash-bound `validator-amendment.json` before the opponents-prefix
process. Keep the original experiment's validator hashes and preexecution
snapshot unchanged; the amendment separately identifies the corrected scripts,
the old archive and the already observed actual output. Neither solver process
nor endpoint result is repeated or selected because of this validator repair.
