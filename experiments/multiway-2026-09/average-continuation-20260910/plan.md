# Multiway postflop: average-only continuation proposal

Status: implemented, required verification passed, seed-0 pilot measured.
Its 1.10077 driver-cost ratio passes the pilot gate. The user subsequently
prioritized **Preflop** solution quality; additional postflop-focused cohort
execution is deferred. Preserve the rule below as the original conditional
plan, not as work completed or the current next action.
2026-09-10. The broader solution-quality goal remains active.

The [pilot report](README.md)
retains its mixed results and exact learning identity. Next, extend independent
endpoint action-gain evaluation to preflop decision nodes and compare deep
3bet/4bet/5bet behavior directly. Preflop average sample changes from this
postflop proposal are not an expected-efficiency claim: its preflop proposal
distribution is unchanged, while later average RNG draws differ.

## Evidence and scope

The [balanced endpoint evidence](../balanced-endpoint-20260910/README.md)
finds about 1.4–1.8 bb of endpoint-only conditional gain in the frozen HU
3bet/4bet checked-through rivers. At the exact HU 4bet endpoint, all 32 buckets
have nonzero regrets but only nine have positive average mass. Its river suffix
uses average policy at about 35.5% of weighted decisions. The HU 3bet endpoint
has average mass in every bucket but its suffix still uses regret fallback at
about 14.6% of decisions. These are concrete averaging targets.

The actual three-player river remains a separate learning target: all columns
are missing. Creating average-only uniform columns there would not establish
regret learning or reduced strategic error. Preserve raw regret and average
support separately, and keep the three-player turn/river in every comparison.

## One bounded candidate

The implementation adds one research-only `PostflopContinuation` average-walk proposal to the
existing fresh, consuming average-sampling research path. Initially restrict
it explicitly to preallocated Street storage, where `dense_node_context` supplies public
street identity. Unsupported modes must fail before training. Do not alter
the production algorithm, TOML defaults, checkpoint identity or artifacts.

At an opponent node on a postflop street, let A be all legal actions and C be
the legal check/call continuations, identified from validated public labels
(`check`, `call:...`). With C nonempty use

```text
q(a | public node) = 0.5 / |A| + 0.5 * 1[a in C] / |C|.
```

With C empty, and at every preflop opponent node, retain uniform sampling.
At the averager's nodes retain existing enumeration and own-strategy reach.
This allocates more average samples to later streets while retaining at least
`0.5 / |A|` probability for every legal opponent action. The coefficients are
fixed before looking at new candidate results; no parameter grid is proposed.

The proposal must use only public state/menus, never private cards, bucket
values, current/average strategy or sweep number. For an exact public history,
the product of its opponent proposal probabilities is then a fixed positive
scalar, independent of own information and training time. The existing
average estimator's normalization can cancel that scalar. Establish this
argument with an independent finite-tree expectation test before using the
candidate. This is an averaging argument, not a new regret-update rule.
In particular, verify that the expected **unnormalized** accumulated vector
is `Q(history)` times the target vector, then normalize those expected vectors.
Do not assert that the expectation of a finite-sample normalized ratio is
exactly the target; the existing self-normalized estimator's finite-sample
ratio error remains. Here the proposal menus are card-independent, so the
same `Q(history)` must apply across all worlds merged into a column, including
different boards; merely being observable to players would not suffice.

Changing history-specific mass scales midway through an existing checkpoint
is outside that argument. Retain fresh-only, no-resume, no-solution-write
boundaries. Average RNG must remain independent from deal/regret RNG. Changes
to later draws can affect sibling average walks, so do not promise identical
preflop average samples or pairing merely because preflop q is unchanged.

## Correctness and measurement gates

1. Test full support, C-empty/preflop behavior, public-world invariance, and
   the expected-accumulated-vector identity above on a small tree with repeated actors,
   zero-probability own actions and several postflop streets. Keep sparse/full
   recall rejection explicit rather than guessing a public street from private
   bucket calls. Preserve the existing UniformOne and EnumerateFirst paths.
2. Across scalar/vector Street modes, prove fixed-sweep regret fingerprints
   remain identical to UniformOne and results are deterministic across thread
   counts. Test that unsupported use fails before training. Run required
   workspace fmt/clippy/tests plus affected research-feature checks.
3. Pilot seed 0 at 32,768 sweeps with the same no-pruning/no-discount, epsilon-0,
   K32 configuration, eight threads and 8GiB. Record driver-only time, whole
   process time and peak memory separately. Continue only if driver cost is no
   more than twice the matched baseline and no resource/invariant failure
   occurs. Pilot coverage is screening evidence, not a quality promotion.
4. If the pilot passes, use predeclared training seeds 0 / 11 / 29, fixed
   32,768-sweep pairs and separately calibrated UniformOne controls at the
   candidate's measured driver budget. Exact regret equality applies to the
   fixed-sweep pairs, not the longer calibrated controls.
5. Retain opener, 3bet/4bet/5bet preflop context plus HU 3bet/4bet and actual
   three-player postflop raw support, complete bucket denominators and suffix
   policy sources. Include held-out endpoint gain with separate fit/held-out
   seeds, fit ESS eligibility versus nonpositive-fit choices, candidate-table
   weight coverage and concentration. Low candidate coverage or a changed
   conditional population cannot establish reduced error.

Endpoint integration must remain read-only and bounded, reuse the same fresh
solver before it is consumed, and reject malformed requests before training.
Do not add checkpoint-writing solely to move research state between tools.
Specify exact per-endpoint evaluation budgets and fresh output directories in
an experiment manifest before running the candidate. Reuse stored evidence
where identities match; avoid paying for repeated construction unnecessarily.

## Actual multiplayer learning remains open

The existing opponent exploration parameter changes a sampling proposal with
importance correction. In `workers.rs`, an action with target probability zero
sets descendant `sample_importance` to zero. Visiting that branch can create
stored columns without a numerical regret update; the retained tests and
exploration screen demonstrate this behavior. Average-only sampling does not
change it.

After this bounded averaging experiment, evaluate a separate regret-learning
intervention. Any forced-prefix or targeted-world update needs its own
counterfactual reach and sampling-support derivation: the read-only preflop
proposal targets the all-player baseline reach, which is not automatically
the traverser's counterfactual training distribution. Do not reuse its weights
as a learning update without proof, or promote a perturbed target policy as
ordinary exploration. Keep existing epsilon 0.06 as a screened candidate,
with its known low per-key ESS and missing river support visible.

No GCP resource is required for this first candidate. The user's September
total-bill limit still requires account-wide cost/obligation verification
before any future allocation. Use one supporting agent only where an
independent bounded implementation review saves time or improves correctness.

## Implementation and pilot record

`averaging.rs` implements the fair integer-draw mixture only in the new dense
scalar/vector mode. UniformOne/EnumerateFirst keep their old draw paths.
`averaging_continuation_tests.rs` checks exact finite-tree expectations,
card independence, full support, C-empty/preflop draw identity and invalid
menu/recall rejection. The private research driver rejects sparse/Full use
before learning. Real Holdem tests compare scalar/vector regret fingerprints
to UniformOne and check deterministic output across one, two and four threads.

The consuming Holdem diagnostics wrapper in `research_diagnostics.rs` keeps
the trained solver privately owned until support, endpoint and optional root
passes finish. It returns raw regret vectors and normalized averages but no
experimental raw average masses or resumable solver handle. Malformed paths,
seed/gate/budget settings and duplicate requests fail before sweeps. Direct
evaluation and wrapper results are compared in tests, excluding clocks.

`runs/average-continuation-20260910/experiment.json` and both literal jobs
predeclare the pilot before execution: training seed 0, 32,768 sweeps,
8 threads / 8GiB, one serialized run per variant, 900 seconds per process.
Each includes 16 support/strategy nodes, eight ordinary coverage prefixes
(131,072 worlds per seed 101/202), and three river endpoint fits
(65,536 worlds, seed 602; held-out 131,072 per seed 702/703; minimum ESS 64).
Separate root reach uses 262,144 worlds per seed 801/802. Ordinary regret-greedy
evaluation uses only 128 worlds per seed and is not a quality measure.
The seed-0 pair can be reused in the fixed-sweep cohort if the pilot passes;
no new table/seed/budget is selected from held-out results. No production
promotion is allowed from this one training seed.

### Cohort execution rule (fixed before candidate output)

If the pilot gate passes, retain both seed-0 measurements without rerunning
them. Use the identical archived executable and diagnostic schedules for
training seeds 11 and 29. Their TOML files may differ from seed 0 only in
`solver.seed`; the configuration fingerprint includes this seed and therefore
must agree within each pair, not across different training seeds. Alternate
the fixed-pair order: continuation then uniform for seed 11, uniform then
continuation for seed 29. All measurements remain serialized and capped at
900 seconds per process.

For each seed, determine one longer UniformOne control from its completed
fixed pair using only driver clocks:

```text
control_sweeps = 4 * ceil(32768 * continuation_driver / uniform_driver / 4)
```

Save the calculated budget and its two source measurement hashes before
starting that control. Keep the configuration and every diagnostic budget
unchanged. This is a calibrated fixed-sweep control, not a wall-clock stop.
Report its actual driver-time difference. A difference above 5% is an
explicit mismatch, not an equal-compute claim; at most one timing-only
recalibration is allowed, retaining the first attempt and all its evidence.
Do not choose a control budget, endpoint, seed or fit gate from strategy or
held-out outcomes. Controls can run after their corresponding fixed pair;
they need not wait for all other training seeds.

The cohort summary must retain all three branches for every training seed,
including missing numerical regret support, fit eligibility, nonpositive-fit
choices, retained-table weight and the two held-out gains separately. Do not
pool conditional populations or call an across-training-seed change a paired
Monte Carlo estimate. Positive-average coverage is an implementation outcome;
balanced strategic improvement still needs adequate independent quality
evidence. A failure of the pilot cost/invariant gate stops this cohort rather
than silently changing its proposal coefficient.
