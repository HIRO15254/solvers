# Multiway checkpoint streaming load (2026-09-10)

The loader now restores owned state directly from individually verified chunks.
On the retained 262,144-sweep checkpoint, observed peak working set fell by
**19.73% (1.837 GB)**. The decoded checkpoint digest matched in every run.
Loading itself was slower in this experiment: median **9.727 → 11.462 seconds**.
This accepts a measured time/memory tradeoff for large-state resume; it is not
a convergence or throughput improvement claim.

## Implementation and compatibility

The previous loader accumulated the entire decompressed postcard payload in a
`Vec<u8>` before constructing `SolverState`. That payload coexisted with its
owned histories, policy rows, action-label strings, regrets, and average sums.
`checkpoint/stream.rs` replaces the payload-sized staging with a compressed
chunk, one decoded chunk (4 MiB plus an overflow-detection byte), and reusable
scratch for the largest single field crossing a chunk boundary. This is a
bound on decoder staging, not on the returned state, allocator overhead, or
the solver arena allocated when resuming.

The postcard adapter accepts owned DTOs and uses temporary slice visitors;
it contains no unsafe lifetime conversion. Framing limits and fingerprint
checks run before decoding. Each compressed chunk and its decoded length are
checked, and all remaining chunks plus the whole-payload checksum are checked
even if the codec stops early. Legacy container versions 5–7, current solver
state rejection rules, the writer, and checkpoint/fingerprint identities are
unchanged. The previous treatment of trailing decoded bytes is preserved.

## Real-data comparison

Input:
`runs/multiway-convergence-round5-20260909/cloud-k256-extension/k256-extension/sweeps-262144/run/checkpoint.mwckpt`.
It contains 22,368,024 checkpoint policy entries and 1,839,947,172 decompressed
bytes. These include policies absent from the average-only solution artifact;
this is not its `.mwsol` strategy-block count.

Input SHA-256:
`a54a47dabf9c3677a0fdffd8d7e5f65c9ac3134be4c57c73af9999c0cb3920f5`.
Complete decoded checkpoint JSON BLAKE3, including state/config/runtime/header:
`22cbcb11200cdd6d82e524dfd87a70e794e229971dc38335fde72562de3983a6`.

| Loader | Load seconds, both observations | Median observed peak, decimal GB |
|---|---|---:|
| Previous | 9.960, 9.494 | 9.308 |
| Streaming | 12.104, 10.820 | 7.471 |

Runs were serial in streaming/previous/streaming/previous order, after builds
and workspace tests had ended. An earlier baseline overlapped a debug build
and is retained but excluded. The filesystem was warm. Two measurements per
arm on one Windows machine do not provide a cross-machine speed guarantee.
Windows lifetime peak working set was queried every 50 ms; the final interval
may be omitted. Peak and process wall time include the streaming JSON digest
and disposal, whereas `loadSecs` times loading alone.

The [machine-readable record](result.json)
contains input/binary/source hashes, compiler identity, every retained
measurement, and exclusions. The input hashes remained identical. No cloud
compute was used.

## Reproduction and verification

Build `cargo build --release -p cli --example mw_checkpoint_load_bench`, then
use [the load benchmark](../../../crates/cli/examples/mw_checkpoint_load_bench.md)
and `tools/run_checkpoint_load_bench.ps1` with a fresh output directory for
each arm. Preserve the previous executable before changing the loader.

Seventeen checkpoint tests cover old versions, cross-chunk strings/numbers,
bounded staging for repeated small fields, corrupt/unused chunks, whole-payload
checksums, truncation, limits, metadata, and atomic overwrite. Existing solver
resume-equivalence tests passed as part of the workspace suite. A subagent
reviewed the streaming adapter independently and found no additional defect.

Required checks passed: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace` (744 passed, 30 ignored). The larger ignored acceptance
suite was not run. Log: `runs/checkpoint-streaming-20260910/workspace-tests.log`.

Remaining scale costs include owned per-policy checkpoint rows, solver-state
snapshot duplication, public-tree construction, and fresh arena allocation.
This change removes one full payload copy; it does not resolve those costs.
