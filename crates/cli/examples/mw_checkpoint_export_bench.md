# Multiway checkpoint export benchmark

`mw_checkpoint_export_bench` measures the cost around a frozen production
Multiway Preflop checkpoint. It performs no MCCFR training and no held-out
evaluation. The JSON on stdout contains independent timings for:

- `startupSecs`: a clean production session build, including EHS2 setup,
  public-tree preflight, arena allocation, and page commit;
- `restoreSecs`: a second production session build with the checkpoint
  restored;
- `snapshotSecs`: `snapshot_state()` on the restored solver;
- `makeSolutionSecs`: construction of the in-memory `.mwsol` object, including
  full public-tree export;
- `writeSecs`: `formats::write_mwsol_with`; and
- `verificationSecs`: file hashing and metadata reopen/consistency checks.

The clean startup session is dropped before restore, so the benchmark keeps
only one large policy arena resident at a time. This means startup and restore
are intentionally measured by two sequential builder calls. A run can be
expensive for a large current-street arena; use the same EHS2 cache, thread,
and memory settings when comparing configurations.

Run from the repository root with a fresh output path:

```text
cargo run --release -p cli --example mw_checkpoint_export_bench -- \
  --config runs/example/config.toml \
  --checkpoint runs/example/run/checkpoint.mwckpt \
  --output runs/example/bench/export.mwsol \
  --threads 8 \
  --memory 48GiB \
  --cache-dir .cache/bench-ehs
```

Diagnostics and phase timings go to stderr. The output artifact must not
already exist, and the tool refuses to treat the checkpoint as an output
path. The JSON reports the artifact byte size and BLAKE3 digest, counts for
the checkpoint snapshot's histories/policies and the export's histories,
public states, strategy blocks, and strategy weights, policy-arena allocation
facts, solver fingerprints, and a metadata verification flag. It also decodes
the first and last strategy frames as a bounded payload check.

`startupSecs` is not a solve-time metric: it includes all setup performed
before the solver can begin sweeps. `snapshotSecs`, `makeSolutionSecs`, and
`writeSecs` are likewise export diagnostics; they do not measure convergence
quality. The `.mwsol` writer is selected from the config's probability
encoding, while the restored checkpoint remains the source of the frozen
average policy.
