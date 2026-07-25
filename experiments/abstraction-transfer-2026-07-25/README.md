# Representative-envelope transfer configs

This directory is independent of the active abstraction-optimization manifest
and solve runner. Use it only after one Tournament finalist and one Cash
finalist have been selected.

1. Copy `manifest.example.toml` and replace both example finalists with the
   selected IDs, abstraction parameters, and seeds.
2. Generate the fixed ten-scenario envelope:

   ```bash
   CARGO_TARGET_DIR=target/research-release \
   cargo run --release -p cli --features research \
     --example generate_abstraction_transfer_configs -- \
     TRANSFER_MANIFEST.toml OUTPUT_DIR CACHE_DIR
   ```

The envelope is fixed in code:

- Tournament: 6-max/5bb, 6-max/50bb, 8-max/20bb, 9-max/5bb, 9-max/50bb
- Cash: 6-max/100bb, 6-max/800bb, 8-max/400bb, 9-max/100bb, 9-max/800bb

Each row in `transfer-configs.csv` has two verified twins:

- `canonical_config`: canonical `solvers.multiway-preflop/v1` source of truth.
- `compatibility_config`: the same lowered game/tree/abstraction spec, plus a
  content-addressed warm-artifact cache path.

Generation fails before writing a scenario when the canonical Tree contract,
seat/button/blind/ante layout, stack, ICM payouts, cash rake, or hard-coded
game fingerprint differs from the expected contract. `transfer-metadata.json`
records the manifest hash and every config, game, Tree, and abstraction-spec
fingerprint.

## Dense-arena payload 8 GiB estimate matrix

The finalist-independent dense preflight uses the same fixed ten scenarios
and their pinned game/Tree fingerprints. The preflight binary constructs the
canonical Tournament ICM or Cash raked game for each scenario, emits its
runtime game fingerprint, and the runner rejects any disagreement with the
plan. It estimates the `current-street` arena payload for equal
flop/turn/river bucket counts
`1, 2, 16, 64, 128, 256`, giving 60 independent processes. The canonical
matrix is `dense-preflight-manifest.json`; changing a scenario, fingerprint,
bucket count, benchmark profile, one-size postflop contract, or the exact
8 GiB arena-estimate limit makes plan generation fail.

Build the count-only executable, validate the complete plan, then run it:

```bash
CARGO_TARGET_DIR=target/research-release \
  cargo build --locked --release -p multiway \
  --features research-abstractions \
  --example action_tree_preflight

DENSE_ROOT=target/abstraction-transfer-dense-preflight-2026-07-25
experiments/abstraction-transfer-2026-07-25/run-dense-preflight.sh \
  "$DENSE_ROOT" --plan
experiments/abstraction-transfer-2026-07-25/run-dense-preflight.sh \
  "$DENSE_ROOT"
```

The runner never performs card abstraction training or a solve, and the
preflight walk does not retain the public tree or allocate the dense arena.
Its 8 GiB (`8589934592` byte) limit therefore applies only to the estimated
dense-arena payload. It is not an 8 GiB process-feasibility result: the
materialized public tree, abstraction cache, worker scratch, evaluator,
allocator overhead, and artifact staging are outside that estimate.

Each count-only preflight process is separately wrapped in a 7.5 GiB
(`8053063680` byte) sampled RSS watchdog. That watchdog protects only the
preflight process and leaves 0.5 GiB below the local process ceiling; it does
not predict or guarantee the RSS of a future materialized solve. The CLI may
lower the watchdog or raise it only as far as 8 GiB. A typed `MemoryLimit` is
an expected arena-estimate result: its first exceeding prefix is recorded and
the remaining jobs continue. A NodeId representation checkpoint is handled
the same way. An actual RSS watchdog hit is also retained in the 60-row
summary, but makes the final runner exit `75`.

Production feasibility remains a separate acceptance gate: run the
materialized solver with its 6 GiB (`6442450944` byte) dense-arena cap under
an 8 GiB process limit and require the representative solve to complete.
Neither a `completed` preflight row nor a 7.5 GiB watchdog pass substitutes
for that run. Plan metadata, every job sidecar, and the summary columns label
this boundary explicitly as `arena-payload-estimate-only` and
`production_process_feasibility=not-established`.

`dense-preflight-plan.csv` and `dense-preflight-metadata.json` bind all 60
jobs to the manifest, the fixed transfer generator source, the preflight
source, and their SHA-256 values. Each content-addressed job binds the plan
metadata, runner, generator, preflight, and watchdog hashes and records hashed
stdout, stderr, and watchdog metadata. The summary also binds each JSON
sidecar by SHA-256. The runner atomically
publishes `dense-preflight-summary.csv` after every job, so expected
memory-limit rows never truncate the matrix.

Validate the runner without performing the real matrix:

```bash
experiments/abstraction-transfer-2026-07-25/test-dense-preflight-runner.sh
```

The fixture covers all 60 jobs, including successful completion, typed
memory limits with child exits `0` and `75`, a NodeId checkpoint, an RSS
wrapper exit `75` distinct from the child exit, provenance sidecars, runtime
game-fingerprint mismatch rejection, malformed typed-result rejection, and
rejection of a changed bucket grid. `--plan` and the fixture are not
benchmark evidence.

## Serial solve runner

After replacing the example finalists, keep `run.max_sweeps` and
`run.check_every_sweeps` equal. The benchmark compatibility config inherits a
very loose convergence stop rule; using a smaller check cadence could
otherwise produce an early `converged` result before the requested fixed
sweep budget.

Validate the ten generated jobs without solving:

```bash
RESULT_ROOT=target/abstraction-transfer-2026-07-25
experiments/abstraction-transfer-2026-07-25/run-transfer-solves.sh \
  500 "$RESULT_ROOT" \
  --manifest experiments/abstraction-transfer-2026-07-25/manifest.toml \
  --plan
```

Run all ten jobs serially:

```bash
experiments/abstraction-transfer-2026-07-25/run-transfer-solves.sh \
  500 "$RESULT_ROOT" \
  --manifest experiments/abstraction-transfer-2026-07-25/manifest.toml
```

Filters only narrow execution; they never change or cherry-pick the generated
ten-case envelope:

```bash
# One selected finalist and seed, Cash cases only.
experiments/abstraction-transfer-2026-07-25/run-transfer-solves.sh \
  500 "$RESULT_ROOT" \
  --manifest experiments/abstraction-transfer-2026-07-25/manifest.toml \
  --case cash \
  --solver-seed 1011 \
  --finalist-regex '^C-R256$'
```

`TARGET_SWEEPS` is a fail-closed confirmation of the generated manifest, not a
CLI override. It must equal `run.max_sweeps`; the runner passes neither
`--iterations` nor `--max-sweeps` to the solver. This keeps the executed
budget inside the config identity. The manifest `evaluation_seed` is retained
as provenance for the later evaluation phase and is not labelled as the
solver's internal held-out evaluation seed.

The portable canonical v1 config remains the specification source of truth.
The runner executes its verified compatibility twin because that twin
contains the content-addressed abstraction-cache path. Consequently, a
result's `configHash` must equal `compatibilityConfigFingerprint`, while the
canonical fingerprint is recorded separately.

The default process RSS watchdog limit is 7.5 GiB
(`8053063680` bytes), with an absolute runner ceiling of 8 GiB
(`8589934592` bytes). The config generator separately verifies the solver's
own memory budget is at most 8 GiB. `--rss-limit-bytes` may lower the watchdog
limit for a diagnostic, but cannot raise it past 8 GiB. The watchdog is a
sampled guard, not an operating-system hard cap; on a Linux/Spot host, place
the runner under an 8 GiB cgroup as the outer hard guard.

Every invocation regenerates and validates all ten configs before selecting
jobs. It rejects:

- a changed fixed envelope or duplicate/missing scenario;
- CSV/metadata disagreement;
- config/cache paths outside the generated directories or symlinks;
- malformed config/game/Tree/abstraction-spec fingerprints;
- raw config, manifest, metadata, index, solver, result, checkpoint, or
  progress SHA drift;
- early terminal results or results past the exact sweep target.

Only one runner may own a result root. Each content-addressed job records
attempt segments, watchdog output, logs, checkpoint/result/cache SHA values,
and resource-limit source. On `resource_limit`, `cancelled`, or another
checkpointed partial result, rerun the exact same command. Automatic resume is
allowed only when the checkpoint and its runner sidecar still match. Signals
received by the runner are forwarded through the watchdog so the solver can
write its final checkpoint.

The runner atomically replaces a filter-specific
`transfer-...-solve-summary.csv`. It records exact target/actual sweeps,
executed versus reused state, solver memory/time, process wall time and peak
RSS, internal versus watchdog resource-limit source, raw SHA-256 values, and
all expected/runtime fingerprints. Evaluation is intentionally not run here.

The rollout assignment cache is shared by scenarios using the same finalist.
That is valid memoization for solve correctness, but later cold-start resource
comparisons must use separately controlled cache state rather than treating
these serial warm timings as identical cold runs.

Run the fixture-only harness check with:

```bash
experiments/abstraction-transfer-2026-07-25/test-transfer-solve-runner.sh
```

The smoke test uses a mock generator and solver. It covers filtering, serial
execution, internal resource-limit recording, checkpointed resume,
exact-target `converged`, completed-job reuse, checkpoint SHA rejection,
CSV/metadata mismatch, an EHS2 finalist with no abstraction seed, input drift,
early termination, atomic summary publication, and zero-match failure. It
does not produce benchmark evidence.
