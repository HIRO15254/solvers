# Average-policy sampling research runner

This feature-gated example compares the existing one-action uniform opponent
walk with first-opponent enumeration or a postflop continuation proposal on a fresh production Holdem game. It is
one-shot by construction: the core API consumes the solver and returns only
normalized positive-mass strategy rows, explicit zero-mass omission rows,
metrics, a raw-regret fingerprint, and optional
held-out evaluation. It cannot resume, write a checkpoint, or write `.mwsol`,
so experimental raw strategy masses cannot be mistaken for production state.

```text
cargo run --release -p cli \
  --features research-average-sampling \
  --example mw_average_sampling_research -- \
  --config examples/bench_multiway/6max_100bb_nl50_partial_reference.toml \
  --variant enumerate-first-opponent \
  --sweeps 4096 \
  --threads 8 \
  --memory 48GiB \
  --evaluation-seeds 101,202 \
  --evaluation-samples 2048 \
  --node root \
  --node fold \
  --node fold/fold \
  --node fold/fold/fold \
  --node fold/fold/fold/fold \
  --source-revision <git-commit-or-source-package-sha256>
```

Run `uniform-one` with the same source, config, seeds, sweeps, threads, and
machine for the paired control. Require identical `sourceRevision`,
`executableBlake3`, effective config/configuration/abstraction fingerprints,
solver-state version, sweep counts, progress counters, and seed schedule before
comparing `currentRegretFingerprint`. The fingerprint itself binds the latter
solver identities, counters, and raw regrets; equality then establishes that
the average-only proposal did not perturb regret learning. Compare normalized
rows and held-out results; do not compare raw strategy mass, which this API
deliberately does not return. A row with status
`zero-average-mass-omitted` has `actions: null`: its current-regret fallback is
not an observed average strategy. The held-out gain uses only the solver's
fixed regret-greedy candidate and is not exploitability.

The JSON records the effective config hash, executable hash, caller-supplied
immutable source identifier, normal configuration/abstraction fingerprints,
and the explicit research variant and thread count. `elapsedSecs` covers
sweeps, result materialization/fingerprinting, and held-out evaluation after
game construction; measure process wall time separately when startup cost
matters.

For first-opponent enumeration, the expected average-walk cost increase is at most the largest action count at
a first opponent node in expectation. It is not a bound on one realized walk
or total process wall time. Stop the first fixed-sweep pilot if solver time is
more than twice the paired control.

## Deep postflop coverage and timing

`--node` accepts decision nodes on every street, including paths crossing a
street boundary. Output `nodes` maps each requested path to its exact history,
actor, street, and active opponent count. Postflop rows refer to abstraction
buckets at that exact history; they are not board-specific 169-class hands.

Repeat `--coverage-prefix PATH` (at most 64 unique resolved histories) to add a
separate baseline-only held-out pass. `--coverage-samples N` requires prefixes,
is at least two, and defaults to `--evaluation-samples`. The pass uses every
`--evaluation-seeds` value; prefixes conflict with `--skip-evaluation`.
`coveragePrefixes` records the corresponding node context, and
`result.coverage_evaluations[].result` contains unconditional baseline EV and
policy-source coverage plus ordered prefix coverage. Deviation gain is null
for this separate pass. Normal fixed-candidate evaluation remains separate and
can use a much smaller sample budget. Unknown/duplicate prefixes fail before
sweeps begin. Neither this diagnostic nor normalized strategy output changes
the one-shot/no-checkpoint boundary.

A prefix counts decisions at and after that history on baseline trajectories,
with separate average/current-regret/uniform counts per seat and street.
`trajectory_visits_by_street` counts each reached decision street at most once
per world; an all-in runout is not a later decision. Prefixes may overlap and
must not be summed as disjoint populations. Rare branches need adequate
trajectory counts; 100% observed average coverage from two decisions is not
strong evidence. Positive average mass alone establishes neither stability
nor low strategy error. The baseline policy itself can change between variants,
so the set of worlds that reaches a prefix can change too.

`result.solve_elapsed_secs` times only the sweep driver. Top-level
`constructionElapsedSecs` times production session construction, while
`elapsedSecs` keeps its earlier research-call scope (solve, materialization,
fingerprint, held-out passes and consumed-solver disposal). Process wall time
additionally includes config loading, path resolution and JSON. Use the sweep-only timer for the
first-pilot 2x cost gate, and record end-to-end time and peak memory separately.
The added metadata is diagnostic-only; ordinary solver configuration,
checkpoint identity and defaults remain unchanged.

Execution is governed by this example's `--sweeps` and evaluation arguments,
plus the measurement runner's external process timeout. Production config
`run.max_sweeps`, `max_time`, stop criteria, checkpoint cadence and output
encoding do not drive the one-shot research call. Read actual sweep counts
from `result.metrics.sweeps` and retained literal CLI arguments.

## Postflop continuation proposal

`--variant postflop-continuation` is research-only and requires preallocated
Street-recall storage (the production session default). Sparse/Full use fails
before training.
At each postflop opponent node, let A be all legal actions and C the actions
whose validated public labels are `check` or begin with `call:`. If C is
nonempty, sample a fair mixture of uniform A and uniform C. Thus every action
has probability `q(a)=0.5/|A|+0.5*1[a in C]/|C|`. Preflop and C-empty nodes
keep the uniform proposal. At the averager's own decisions, enumerate actions
with existing own-strategy reach. The production default remains UniformOne.

This proposal depends only on card-independent public menus, so its product
is a constant for each exact history across all worlds and training steps.
Expected unnormalized average vectors acquire that constant, which cancels
on normalization. Finite-sample normalized ratios still have sampling error;
the proposal does not make those ratios unbiased. Regret learning and its RNG
remain independent and must match the fixed-sweep UniformOne control exactly.
Do not compare raw average mass across variants or mix it into a checkpoint.
No first-opponent-enumeration cost bound applies to this proposal: measure
its driver cost and use the explicit pilot gate.

## Support and one-step deviation diagnostics

The optional diagnostics run on the same freshly trained solver before it is
consumed, avoiding repeated construction/training or research checkpoint
writes. The consuming Holdem API is
`run_average_sampling_research_with_diagnostics(training, diagnostics)`.
Both the normal API and the extended API return no resumable solver handle.

- Repeat `--support-node PATH` up to 64 times for complete bucket rows at any
  decision street. `supportNodes` records requested public context and
  `diagnostics.support` includes ordered keys, raw regret vectors and normalized
  average strategies. A null regret vector means missing; an all-zero vector
  means stored zero regrets. A null average means missing or zero average
  mass. No raw experimental average mass is exposed.
- Repeat `--endpoint-prefix PATH` up to eight times. All paths must be unique
  preflop or postflop decisions, including `root`. Explicit `--endpoint-fit-samples N`,
  `--endpoint-fit-seed S`, `--endpoint-samples M` and
  `--endpoint-seeds T,U` are required. N and M are at least two; held-out seeds
  number 1–64, are unique and differ from S. Optional
  `--endpoint-min-fit-ess E` defaults to 64 and must be finite and at least two.
  Each endpoint fits a separate own-information action table before evaluating
  the same table on all its held-out seeds. The prefix and all later decisions
  remain baseline. Unsupported keys keep baseline and all prefix weight stays
  in the signed gain denominator. These are local conditional gains, not
  whole-game exploitability. `endpointPrefixes` maps the public requests to
  `diagnostics.endpoints` in order.
- Optionally pair `--root-samples N` with `--root-seeds S,T` to estimate root
  reach for those same endpoint paths. N is at least two; 1–64 unique root
  seeds must differ from endpoint fit/held-out seeds. The root pass shares each
  seed's physical worlds across endpoints and records
  `diagnostics.root_evaluations`. Proposal relative weight means are not root
  probabilities. Matching numeric seeds across changed profiles do not imply
  paired conditional populations.

These diagnostics can be used with `--skip-evaluation` to omit the old ordinary
regret-greedy pass. Ordinary `--coverage-prefix` still requires ordinary
evaluation. Missing budgets, malformed/duplicate paths, unsupported recall
and invalid seed/gate settings fail before training. Resource or numerical
failures during sampling remain runtime errors.

`diagnostics.config` records literal core requests and budgets.
`diagnostics.elapsed_secs` covers support and the endpoint/root passes, while
`result.solve_elapsed_secs` still covers only sweeps. Top-level `elapsedSecs`
includes all research results and disposal. Absent diagnostics omit the new
top-level optional fields, preserving earlier output structure. Production
TOML contracts, state version and solution/checkpoint formats are unchanged.
