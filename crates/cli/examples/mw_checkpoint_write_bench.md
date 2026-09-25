# Multiway checkpoint write benchmark

Build with `cargo build --release -p cli --example mw_checkpoint_write_bench`.
Use the same immutable executable, source, config, thread count, memory limit,
cache, input checkpoint or fresh sweep budget, and metadata for both modes.
Only `--mode owned|borrowed` and the new `--output PATH` differ. Each mode runs
in a separate process, in serialized order. Existing output files are rejected.

Required inputs are `--config PATH`, `--memory 8GiB`, `--source-revision ID`,
`--mode MODE`, `--output PATH`, and exactly one of `--checkpoint PATH` or
`--sweeps N`. `--threads` defaults to 8; `--cache-dir PATH` sets the existing
abstraction cache. Checkpoint mode restores without further training. Fresh
mode explicitly overrides the configuration's production sweep budget and
uses the normal training driver, without production stop/evaluation events.

The write timer includes owned capture or borrowed index preparation, shared
postcard serialization, chunk compression, fsync, atomic persist, and disposal
of the temporary DTO/index. Construction, optional fresh training, output-file
hashing and final solver disposal are separate. Persisted runtime metadata is
restored from the input or zero for fresh runs; benchmark clocks are never
inserted into the checkpoint. Matching full-file bytes are required, alongside
matching config, state counters and fingerprints. This is a storage experiment.

`experiments/multiway-2026-09/scripts/run_checkpoint_write_bench.ps1 -Binary EXE -Job JOB.json -OutDir NEW_DIR`
accepts a literal argument list and timeout in a retained job, with config hash,
source manifest hash and validation-report path. It records whole-process
lifetime peak working set and timestamped current working-set samples every
50 ms. The output write interval permits separate phase sampling; these
samples can miss peaks, and restored-state allocation can dominate whole-process
memory before writing starts. Missing phase samples are unavailable, never zero.
Preserve failures and timeout records. No input checkpoint is overwritten.
