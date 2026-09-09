# Multiway convergence benchmark fixture

See the [benchmark catalog](../../docs/validation/multiway-benchmarks-2026-09-09.md)
for all retained configurations, historical inputs, executed and planned arms,
comparison boundaries, reproduction commands, and the current development priorities.

`3max_2bb.toml` and `6max_2bb.toml` are small, production-v1 push/fold
fixtures (one aggressive action per street) intended for reproducible local
runs and externally provisioned Spot workers. The runner keeps each generated TOML
file, solver logs, run directory, held-out evaluation output, and `inspect`
output together under `runs/benchmarks/` (or the selected `--output-root`).
`target/` is reserved for Cargo build output and must not hold retained runs.
The evaluator's raw mixed stdout is retained as `evaluation.stdout.log`; the
validated JSON document is written separately as `evaluation.json`.

The held-out `evaluate` phase uses its explicit evaluation seed and finite
deviator traversal count. The normative v1 contract limits viewed/exported
solution nodes to preflop. The current writer does not yet enforce that
boundary: public-tree metadata includes every street, and `make_solution`
exports every policy entry with positive average-strategy mass without a
street filter. Zero-mass and unvisited entries are omitted, raw regrets are
not stored, and probabilities may be quantized. This is an open specification
/ runtime disagreement; the extra stored blocks must not be described as a
complete or contract-supported postflop profile. Use a full checkpoint audit
when the original fallback/regret state matters.

The 2bb stacks force raises all-in, so there are no postflop decision policies
and this disagreement cannot change their evaluated profile. These fixtures
remain cheap convergence sanity checks and do not establish agreement with
GTO Wizard or a Nash guarantee.

Each plan/summary carries `evaluation_profile_equivalence`. The known fixture
is marked `verified-all-in-tree`; any other source config is marked
`unknown-preflop-only-artifact`, so a downstream report cannot silently rank
an unsupported postflop comparison. The latter string is a conservative
historical marker name, not a claim that the current writer actually filters
all postflop blocks.

The 6-max fixture was checked with one sweep, 8 held-out samples, and one
deviator traversal using the baseline release. Exported `tree` JSON contained
125 states and 124 edges: 62 public decision states, all at `street = 0`, and
63 terminal/no-action states, also at `street = 0`. Thus the exported tree had
zero non-preflop decision states.

The v1 parser resolves a relative `[game.tree] source` against the source TOML
directory. The runner copies that `.mwtree` file into each variant while
preserving its relative path, and records the copy in `external_dependencies`.
Missing, absolute, or escaping source paths fail before solve.

Comparisons across variants should use identical seeds and paired samples;
independent seeds are useful for variance reporting, while an unpaired
variant ranking confounds solver changes with deal noise.

Print a plan without launching a solve:

```text
python tools/multiway_convergence_bench.py examples/bench_multiway/3max_2bb.toml --dry-run
```

A small fixed-sweep smoke matrix is:

```text
python tools/multiway_convergence_bench.py examples/bench_multiway/3max_2bb.toml --seeds 0,11,29 --sweeps 4096 --timeout-seconds 900
```

Use `--max-time 2m` with a sufficiently large `--sweeps` value for a fixed
wall-time comparison. The runner raises `run.stop.check_every_sweeps` above
the sweep ceiling, because v1 has no stop-rule disable switch.

## 6-max 20bb GCP Spot experiment

`6max_20bb_checkdown.toml` is a larger performance and convergence anchor.
It has a real multiway preflop betting tree followed by an explicit postflop
checkdown. It is useful for comparing solver implementations under one fixed
game, but it is not a GTO Wizard equivalence claim.

Run the bounded one-process pilot first. It uses 256 sweeps, 128 evaluation
samples, and 512 trained-deviator traversals per seat. The pilot measures wall
time and memory before committing to a matrix; cloud speedup is unverified
until this finishes.

```text
python tools/gcp_multiway_experiment.py --output-root runs/gcp-pilot-YYYYMMDD
```

After reviewing the pilot, run the explicit matrix on a 32-vCPU worker. It
uses at most four independent processes with eight solver threads each, three
paired seeds, 30,000 sweeps, and live in-run evaluations at 15,000 and 30,000
sweeps. Each evaluation uses 4,096 samples and 20,000 trained-deviator
traversals per seat. This is screening strength; a final accuracy audit should
use more samples and at least two independent evaluation seeds.

```text
python tools/gcp_multiway_experiment.py --matrix --skip-build --output-root runs/gcp-matrix-YYYYMMDD
```

For a local screening run, keep `--jobs 1` (or at most `2`) so concurrent
processes do not distort throughput. The schedule and evaluator strength are
explicit overrides; for example:

```text
python tools/gcp_multiway_experiment.py --matrix --skip-build --jobs 1 --threads 8 --sweeps 4000 --evaluation-cadence 4000 --evaluation-samples 1024 --deviator-traversals 5000 --pruning none --output-root runs/local-4k-YYYYMMDD
```

`--pruning none` selects the unpruned vector/single-hand/batch variants;
`--pruning regret-based` selects only the pruning ablation; the default `all`
selects both groups. Sweep count must be divisible by evaluation cadence so
the final fixed-sweep quality row is present.

Use `--variants` to avoid running unrelated algorithms in a longer study.
The following pair runs three seeds for `vector-b4` with and without pruning.
Repeat it with `--discount-every 10000` to measure periodic discount under the
same fixed-sweep contract. A positive cadence writes a periodic discount whose
`until_sweeps` is one past the run ceiling; zero, the default, writes
`kind = "none"`. The selected discount is stored explicitly in `plan.json`.

```text
python tools/gcp_multiway_experiment.py --matrix --skip-build --jobs 1 --threads 8 --sweeps 65536 --evaluation-cadence 65536 --evaluation-samples 8192 --deviator-traversals 100000 --variants vector-b4,vector-b4-prune --output-root runs/local-65k-none-YYYYMMDD
```

`--variants` accepts only `vector-b1`, `single-b1`, `vector-b4`, and
`vector-b4-prune`. Its intersection with `--pruning` must contain at least one
variant; invalid or empty selections fail before creating the output root.

The runner requires a new or empty output directory, imposes a 20-minute
timeout on each solve and a 3-hour-20-minute global deadline, and fails closed
on validation errors, time limits, missing evaluation rows, or non-finite
confidence intervals. A `time-limit` result is incomplete rather than a
successful fixed-sweep result. It never resumes an existing directory.

If a Spot interruption leaves a valid checkpoint, recover it manually into a
new, explicitly named experiment after checking the saved config and hashes.
Do not combine a recovered run with the fixed-sweep matrix unless its sampling
stream and evaluation schedule still match the comparison contract. Quality
comes from the live solver's `progress.jsonl`; the runner does not reconstruct
the profile from `.mwsol`.
