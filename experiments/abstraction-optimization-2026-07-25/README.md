# Abstraction optimization runner

`manifest.toml` is the experiment contract. The two rung runners execute one
process at a time and stop a process before the local 8 GiB ceiling.

Build the release tools, then run the solve and common-reference evaluation
rungs from the workspace root:

```bash
CARGO_TARGET_DIR=target/research-release \
cargo build --release -p cli --features research --bin solvers \
  --example generate_abstraction_optimization_configs \
  --example paired_reference_bootstrap

RESULT_ROOT=target/abstraction-optimization-2026-07-25
experiments/abstraction-optimization-2026-07-25/run-solve-rung.sh \
  s1 500 1011 "$RESULT_ROOT"
experiments/abstraction-optimization-2026-07-25/run-evaluation-rung.sh \
  s1 "$RESULT_ROOT"
```

The evaluation runner reads samples, deviator traversals, seed-pair count, RSS
limit, and reference routing from the generated experiment metadata. That
routing is derived from each reference's optional manifest `rungs` list, so a
more expensive future-rung reference is never run early. The reference class
can still be overridden explicitly for a diagnostic:

```bash
experiments/abstraction-optimization-2026-07-25/run-evaluation-rung.sh \
  s2 "$RESULT_ROOT" '^(T-R256|C-R256)$' --reference-set screening

experiments/abstraction-optimization-2026-07-25/run-evaluation-rung.sh \
  s3 "$RESULT_ROOT" '^(T-R256|C-R256)$' --reference-set final
```

`screening` selects every non-`final_only` reference. `final` selects the full
reference ensemble. Formal rung runs should omit the override and use the
manifest routing.

Use `--samples`, `--br-traversals`, `--seed-pairs`, or
`--evaluation-seed` only for an explicitly recorded override. `--plan`
validates the inputs and prints the serial jobs without running them.
`--reference-filter ERE` further restricts the references selected by the
rung/reference-set routing. An invalid expression or a filter matching zero
selected references exits with status 2 before any job starts.

A filtered run derives `rf-<first 16 hex digits of SHA-256(ERE)>` and uses it
in the job identity and artifact names, for example
`s3-rung-rf-...-evaluation-summary.csv` and
`s3-rung-rf-...-coverage-gates.json`. The exact expression and derived suffix
are also recorded in job metadata, filtered summary rows, and coverage JSON.
If the filter selects a proper subset of the manifest route, ranking records
both sets and the reason, and always emits screening-only decisions even when
all coverage gates pass. An explicit filter that selects the complete route is
still eligible for formal ranking.
Unfiltered runs retain the historical filenames and job identity. A
K512-EHS-only S3 pilot is therefore:

```bash
experiments/abstraction-optimization-2026-07-25/run-evaluation-rung.sh \
  s3 "$RESULT_ROOT" '^(T-K512|C-K512)$' \
  --reference-filter '^s3-ehs2-k512$' \
  --coverage-candidate-stored-min 0.995 \
  --coverage-candidate-postflop-stored-min 0.95
```

Each evaluation job has a content-derived directory below
`RESULT_ROOT/evaluations/`. A completed job is skipped only after its job
identity, report SHA-256, fingerprints, sample/seed values, and coverage-array
shape revalidate. `meta.json` records solver/config/checkpoint hashes, the
reference cache SHA-256 before and after the run, wall time, peak RSS, and
status. A failed or interrupted invocation can be rerun; validated jobs are
not repeated.

After the serial evaluations finish, the runner automatically executes the
coverage gate before candidate comparison. Pilot defaults are:

- candidate stored-policy fraction at least `0.95`;
- separately, each candidate postflop street with at least 200 pooled
  decision visits has stored-policy fraction at least `0.60`;
- held-out trained-action visits / decision visits at least `0.80`;
- separately from candidate coverage, each held-out reference postflop street
  with at least 200 visits has trained fraction at least `0.60`;
- candidate stored-fraction spread within a matched
  case/seed/reference cohort at most `0.05`.

The gate writes
`RESULT_ROOT/RUNG-rung-coverage-gates.json` and `.csv`, and exits nonzero if
any condition fails. Filtered runs use the derived suffix described above.
All thresholds are configurable on the standalone command:

```bash
experiments/abstraction-optimization-2026-07-25/aggregate-coverage-gates.py \
  "$RESULT_ROOT" s2 '^(T-R256|C-R256)$' \
  --candidate-stored-min 0.95 \
  --candidate-postflop-stored-min 0.60 \
  --candidate-postflop-min-visits 200 \
  --training-retained-min 0.80 \
  --heldout-trained-min 0.80 \
  --postflop-trained-min 0.60 \
  --postflop-min-visits 200 \
  --candidate-spread-max 0.05
```

`reference_training.retained_fraction` uses all counterfactual training
actions in its denominator. It is therefore retained as a diagnostic (with
the configurable `--training-retained-min 0.80` comparison), not a hard
coverage gate. In particular, a value around `0.59`-`0.62` does not by itself
invalidate a candidate. Candidate policy coverage and held-out reference
coverage are the hard gates.

When evaluation uses `--seed-pairs N` or `--evaluation-seed N`, the runner
passes the same selection to the coverage gate. For a standalone coverage or
ranking command, pass the same override explicitly. Both JSON artifacts bind
the decision to `inputs.seed_pairs`, the exact
`inputs.selected_seed_pairs`, and `inputs.evaluation_seed_override`; they do
not silently require unexecuted manifest seeds.

Formal `s3`/`s4` evaluation raises the aggregate candidate stored-policy
minimum to `0.995` and every applicable pooled postflop street minimum to
`0.95`; pass both overrides on the evaluation command so its automatic gate
uses the strict values:

```bash
experiments/abstraction-optimization-2026-07-25/run-evaluation-rung.sh \
  s3 "$RESULT_ROOT" '^(T-R256|C-R256)$' \
  --coverage-candidate-stored-min 0.995 \
  --coverage-candidate-postflop-stored-min 0.95
```

The coverage JSON retains aggregate candidate coverage and adds
`candidate_policy.by_street.{preflop,flop,turn,river}` plus
`candidate_policy.postflop_gate_passed`. The CSV exposes the same values as
`candidate_STREET_*` columns. Preflop remains part of the aggregate gate but
does not receive a separate street gate. A ranking is formal only when the
coverage artifact is bound to the current summary and uses at least `0.95`
candidate postflop coverage for `s3`/`s4`; otherwise every ranking decision is
screening-only.

After the formal coverage gate passes, build the non-inferiority and resource
Pareto tables with:

```bash
experiments/abstraction-optimization-2026-07-25/rank-reference-reports.py \
  "$RESULT_ROOT" s1 --seed-pairs 1
```

For a filtered run, pass the identical `--reference-filter` to the ranking
command; it derives the same summary and coverage paths and verifies their
filter provenance. A proper-subset filtered run remains screening-only; formal
elimination requires the complete manifest route. `--summary PATH` and
`--coverage-json PATH` are available for explicit artifact locations.

The ranking harness forms a complete cohort for each
case/reference/abstraction-seed/solver-seed/evaluation-seed tuple. Within each
cohort, the report with the lowest
`max_seat(max(0, mean_raw_gain))` is the quality champion. Every other
candidate is compared with that champion by the existing
`paired_reference_bootstrap` executable, in the `candidate - champion`
direction. Reports are never compared unless their reference/game
fingerprints, rung, sweeps, samples, evaluation and training seeds, BR
traversals, profile, purification threshold, seat shape, and sample IDs all
match.

The cash margin is `0.01`; the tournament margin is `0.002` in the report's
canonical raw prize units. The latter must equal the manifest prize-fraction
margin multiplied by the payout sum parsed from every selected tournament
config. The bootstrap replicate count comes from generated experiment
metadata. Comparison seeds are deterministically derived from the generated
manifest fingerprint and the cohort identity. The existing binary's two-sided
95% upper endpoint is recorded as a conservative 97.5th-percentile one-sided
95% UCB. Formal decisions additionally use the paired-bootstrap probability
of exceeding the margin, with plus-one bounds and Bonferroni control at family
alpha `0.05`. The family budgets every possible candidate pair in each
case/cohort, not only the comparisons with the data-selected champion. A
candidate is `passed` only when the simultaneous upper tail establishes
non-inferiority, `failed` only when the simultaneous lower tail establishes
inferiority, and otherwise remains `unresolved`. If the configured replicate
count cannot resolve the adjusted alpha, the output remains a screening table
and cannot eliminate candidates. These tail counts invert empirical
percentile bounds; they are not labelled or interpreted as frequentist
p-values.

A candidate is non-inferior only when every selected reference and seed cohort
passes. The config index must contain the complete candidate × selected-seed
Cartesian product. The coverage gate must use the formal thresholds and bind
the current summary, report, and metadata SHA-256 values; stale or looser
coverage can only produce screening output. No fixed top-N truncation is
applied: missing reports, mismatched provenance, insufficient multiplicity
resolution, failed coverage, and missing resource measurements remain
explicitly `unresolved`. The command writes
`RUNG-REFERENCE_SET-ranking.json` and `.csv`; exit `0` means a complete table,
`1` means unresolved candidates were retained, and `2` means an input contract
was invalid.

The Pareto frontier uses lower-is-better quality, the solver's warm-loop
elapsed time per sweep, peak RSS, solver memory, checkpoint bytes, and
abstraction-cache bytes. The process segment wall time is retained separately
as descriptive-only because it also contains unseparated build, restore, and
persist work. Warm-loop time is used only when its sweep denominator is
provable. The current summary does not record a resume segment's starting
sweep, so resumed rungs remain resource-unresolved rather than assuming the
previous rung was the exact start. Cold cache-build time is a separate
`not_recorded` column and is never folded into warm solve time or silently used
as a tie-break. Until that measurement exists, the available-metric frontier
is labelled `partial_frontier_cold_unresolved`, resource selection remains
unresolved, and the command exits `1`.

## Full-recall K64 resource ceiling

`run-resource-ceiling.sh` measures how far the existing seed-1011 `T-E64` and
`C-E64` full-recall checkpoints advance before a deliberately lower
solver-storage cap. It does not regenerate or write into the source experiment.
The initial plan is fixed by the defaults:

- `T-E64`: 3 GiB internal solver cap, 10,000 total sweeps;
- `C-E64`: 3.5 GiB internal solver cap, 10,000 total sweeps;
- both: a 7.5 GiB process-RSS watchdog, run strictly serially.

Build the release solver separately, inspect the validated plan, and then run
it into a sibling result root:

```bash
CARGO_TARGET_DIR=target/research-release \
cargo build --locked --release -p cli --features research --bin solvers

SOURCE_ROOT=target/abstraction-optimization-2026-07-25
CEILING_ROOT=target/abstraction-resource-ceiling-2026-07-25
experiments/abstraction-optimization-2026-07-25/run-resource-ceiling.sh \
  "$SOURCE_ROOT" "$CEILING_ROOT" --plan
experiments/abstraction-optimization-2026-07-25/run-resource-ceiling.sh \
  "$SOURCE_ROOT" "$CEILING_ROOT"
```

Each content-addressed job receives an atomic copy of the source checkpoint
and a dedicated config whose only raw-config mutation is
`[run].max_memory_bytes`. The source checkpoint, config, result, config index,
and warm EHS2 cache are SHA-256 checked before and after execution. The copied
checkpoint may be replaced by the solver; the source checkpoint never is.
Config, checkpoint-input/output, result, solver, watchdog, and runner hashes
are recorded in per-segment and per-job JSON provenance, and the CSV summary is
atomically published after every terminal segment.

This fork is resume-compatible by design, but its identity has two distinct
hash layers. Editing the copied config changes its raw config hash, so the
source index/result config hash is retained as source provenance and the new
hash is read back from the fork result. Solver checkpoint compatibility
excludes the operational memory budget while retaining every sampling,
algorithm, game, range, and abstraction setting. The runner also requires the
fork result's game, abstraction, and configuration fingerprints to equal the
source result and refuses a cap that is not above the checkpoint's currently
reported solver payload. Thus lowering 6 GiB to 3/3.5 GiB changes the
operational ceiling without mixing algorithms.

A solver result with `status = "resource_limit"` and child exit `75` is the
expected successful measurement, so two such results make the runner exit
`0`. If a job reaches 10,000 sweeps, it is recorded as `target_completed` and
the runner exits `1`: that cap did not locate the ceiling. External RSS
watchdog termination remains an error (`75`) and is distinguished from the
expected internal solver limit. Monitor failure is `70`; input/provenance
errors are `2`/`3`; HUP/INT/TERM are forwarded and a partial atomic summary is
published before returning `129`/`130`/`143`.

Run the lightweight harness check with:

```bash
experiments/abstraction-optimization-2026-07-25/test-evaluation-harness.sh
experiments/abstraction-optimization-2026-07-25/test-resource-ceiling-runner.sh
PYTHONDONTWRITEBYTECODE=1 \
  python3 experiments/abstraction-optimization-2026-07-25/test-coverage-gates.py
PYTHONDONTWRITEBYTECODE=1 \
  python3 experiments/abstraction-optimization-2026-07-25/test-ranking-harness.py
```

The check uses a mock solver only; it does not produce benchmark evidence.
