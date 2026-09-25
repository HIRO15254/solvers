# Checkpoint load benchmark

This example measures checkpoint decoding without constructing a solver,
building abstraction tables, or training. It hashes the complete decoded
checkpoint (state, header, embedded config, and runtime) through a streaming
JSON writer after the load timer. The digest is an equality check for this
benchmark, not a game/algorithm fingerprint or a quality measurement.

```text
cargo build --release -p cli --example mw_checkpoint_load_bench
target/release/examples/mw_checkpoint_load_bench --checkpoint PATH
```

On Windows, capture bounded execution, source/binary/input identity, exit
status, and peak working set with a fresh run directory:

```powershell
./experiments/multiway-2026-09/scripts/run_checkpoint_load_bench.ps1 -Binary target/release/examples/mw_checkpoint_load_bench.exe -Checkpoint PATH -OutDir runs/checkpoint-load/measurement-1
```

The default process timeout is 300 seconds. Memory observations query the
Windows lifetime peak every 50 ms and may miss the final interval. Wall time
and peak memory include the decoded-state digest and disposal; `loadSecs`
times only loading. Run one arm at a time without concurrent builds/solves,
alternate arm order, and retain repeated measurements and input hashes.

Loading enforces checkpoint framing/state-version checks but does not validate
game compatibility against a newly built solver. A successful load/digest
does not substitute for resume-equivalence tests.
