# Average-policy sampling research runner

This feature-gated example compares the existing one-action uniform opponent
walk with first-opponent enumeration on a fresh production Holdem game. It is
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

The expected average-walk cost increase is at most the largest action count at
a first opponent node in expectation. It is not a bound on one realized walk
or total process wall time. Stop the first fixed-sweep pilot if solver time is
more than twice the paired control.
