# Task T26: cheaper lane-batched terminals: output store and fold-only batches (P1 postflop, f32 CFR)

Continue in `C:\Users\PC_User\orca\workspaces\solvers\cisco-t25` (branch `s3-p1-t25`). Your T25a/T25b changes are
uncommitted there and are the baseline for this task ("T25"). The reviewer is committing exactly that state to the
main P1 branch in another worktree; do not commit, push, stash, reset, or switch branches. Leave all changes
uncommitted. Keep `runs/t25b/` as it is; put new evidence under `runs/t26/`.

## What the 32-thread VM measured for T25 (AMD EPYC Milan, 16 cores / 32 threads)

T25 was adopted: one CFR iteration got 0.82x (Flop1, 32 threads), 0.81x (16), 0.85x (1), gtow_b 0.83x; solves to
0.1% pot 0.81-0.90x; f64 bit-identical. The new sampling profile (Flop1, 32 threads, `p1_bench`):

| symbol | share |
|---|---:|
| `terminal_batch_lanes_f32::<8>` | 31.6% |
| `cfr_pass` (with the inlined batch pass) | 18.0% |
| `normalize_columns_f32` | 16.9% |
| `terminal_batch_lanes_f32::<1>` | 10.2% |
| `terminal_batch_lanes_f32::<5>` | 6.0% |
| `terminal_batch_lanes_f32::<7>` / `<6>` / `<4>` / ... | 3.7% / 1.2% / 0.7% / ... |

gtow_b is similar (all lane widths 46%, `<8>` 27%). Inside `<8>` (line numbers of the current `kernel.rs`):
`kernel.rs:357` (`out[local] = values[local][lane]`, the per-lane output copy) 16.5% plus `non_null.rs:1714`
(the slice-iterator loop compares of that copy loop, unrolled with a pointer reload per store) 12.8%, i.e. about
30% of the kernel is the output de-interleave. The card-array read-modify-writes (`kernel.rs:180`, `309`, `342`)
are about 22% and the own-hand value line (`kernel.rs:328`) 8.4%. In `<1>` and `<5>` the same copy is 2.6% / 14%.

Batch sizes (your `runs/t25b/arenas.jsonl`): in Flop1 every batched root is a whole river subtree; 84% of them have
15 nodes / 9 terminals and 16% have 9 nodes / 5 terminals. With one 75% bet, one 3x raise and two aggressive
actions per street, the 9 terminals are 5 showdowns and 4 folds, so today they become one 8-lane sweep plus one
1-lane sweep (that is where `<1>` comes from); the 5-terminal roots (3 showdowns + 2 folds) become one `<5>` sweep.
`<1>` costs about a third of `<8>` per call, so the per-lane parts dominate the sweep cost.

## Changes to make (f32 CFR terminal batches only)

### A. Output store

Make the transfer of lane values into the per-terminal output rows cheap. Try at least these and keep the fastest
on the realistic workloads (report all, with numbers):
1. Copy in local-index order instead of `own.hands` order: process the own support in blocks of 8 consecutive
   local indices and transpose each 8 x LANES block into the LANES output rows with fixed-size arrays (so LLVM can
   emit shuffles and full-width stores instead of scalar scatters); board-dead own entries must stay `+0.0`
   (for example by keeping the dead rows of the value buffer zero, from a per-table list of dead locals, or by
   zeroing them in the outputs after the copy; do not reintroduce a full `fill` of every output first if the copy
   writes every entry anyway).
2. Store directly from the own-hand loop (the `kernel.rs:328` computation) into the LANES output rows, dropping the
   value buffer, with the output row slices fetched once per call (an array of LANES `&mut [f32]`).
3. Anything better you find (check the disassembly of the hot loop as before).
Check the input interleave the same way (it did not show up as hot, but say what it costs).

### B. Fold-only lane batches

Fold terminals that have a rank table (river folds; `batch_tables != u32::MAX`) must no longer take showdown sweep
lanes. Route them, always (whatever the grouping, so a terminal's result never depends on which other terminals
share the call), to a new fold-only lane kernel: interleave up to 8 opponent reaches as today, one lane-wise pass
over the opponent hands for the card-removal sums (total and the 52 card sums, like `add_lane_hands`), then one
pass over the own hands computing `u_fold * (total - card[a] - card[b] + same)` lane-wise (or the algebraically
equal form that matches the existing f32 fold kernels' accuracy; justify the choice), with the output store from A.
No rank-group control flow. Showdowns of a table keep the sweep kernel, in batches of up to 8 lanes. Folds and
showdowns of the same table are grouped separately; keep the batch order deterministic.

Measure B on top of A. If some other split measures faster (for example letting folds fill otherwise unused sweep
lanes), say so with numbers, but only keep a scheme where each terminal kind always uses the same kernel and lane
arithmetic, so grouping independence stays bitwise.

## Rules

- f64 (`cfr_precision = "f64"`) bit-identical to T25 (and therefore to `b01d4a4`): `.sol` payload, checkpoint
  arenas and progress. f32 may change rounding but must be deterministic, bitwise independent of grouping, lane
  position and thread count (the existing tests at 1 and 4 threads must keep passing and keep exercising batches).
- Safe Rust only. No new dependencies, no Cargo.toml changes, no file format changes. Do not touch
  `crates/mw-preflop` or `crates/cfr-ref`. No multi-accumulator/ILP tricks and no data-dependent branches inside
  per-hand loops. Keep fixed per-call costs and stack/buffer footprints proportional to the actual supports and lane
  counts (the T24 lesson; two SMT threads share a 32 KiB L1D on the target).
- Engine (`crates/hu-engine`) changes only if needed; the batch API stays.
- Comments where code changes; update `docs/hu-postflop.jp.md` (Japanese) where the batched terminal evaluation is
  described (folds now use their own lane kernel). `python tools/check_docs.py` must pass.

## Tests

- Accuracy of every fold and showdown lane against the f64 kernels (relative 1e-5 of the max-norm), board-dead own
  entries `+0.0`, all-zero and `-0.0` reaches, every lane width 1..=8, both players, realistic and wide ranges.
- Bitwise grouping independence: each terminal alone vs in full batches vs mixed fold/showdown calls in different
  orders.
- The existing batched-pass engine tests and determinism tests keep passing.

## Benchmarks (local, required; the reviewer measures at 32 threads on the VM)

1. Kernel bench (`crates/hu-postflop/benches/kernels.rs`): add a third realistic workload shaped like the Flop1 river
   subtrees: the BTN/BB ranges and board of the existing realistic workload, river bet 0.75 only, raise 3x, two
   aggressive actions (9 terminals: 5 showdowns, 4 folds), f64-solved like the others. For all three workloads
   (that one, BTN/BB, wide) report `t25_batch` with the T25 kernels (keep a copy of the old path callable from the
   bench, or build the T25 bench executable from a saved copy of the sources; say which) against the new kernels:
   A alone and A+B, three alternating runs, `RAYON_NUM_THREADS=1`, pinned to one logical CPU, medians.
2. Whole iterations: baseline `p1_bench` = `C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad\t25b-p1_bench.exe`
   (your T25 build); build the new one and copy it to `t26-p1_bench.exe` in the same folder. Configs in this
   worktree: `experiments/p1-perf-2026-10/scratch-overwrite-20261008/configs/c_river.toml`, `c_turn2.toml`,
   `c_flop1.toml`. Alternate baseline/new three times each, `--evals 0 --json <file>`:
   - `c_river.toml --threads 1 --warmup 20 --iters 200`
   - `c_turn2.toml --threads 1 --warmup 5 --iters 50`
   - `c_flop1.toml --threads 4 --warmup 2 --iters 6`
   Report `secsPerIter` medians and ratios.

## Build constraints (important)

The machine is close to its memory commit limit and is shared with another heavy job. Always set
`CARGO_BUILD_JOBS=1` and `CARGO_INCREMENTAL=0`. Never run two cargo commands at the same time. If rustc dies with
`STATUS_STACK_BUFFER_OVERRUN` or an out-of-memory error, wait a minute and retry.

## Verification (run all, report results)

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (full output to `runs/t26/tests.log`; report pass/fail/ignored totals)
4. `python tools/check_docs.py`
5. f64 identity against T25 (your T25b method; the f64 path should not change at all).
6. The benchmarks above.

## Report format

- What was changed in A (the variants tried with numbers, the chosen one, disassembly notes) and in B (kernel layout,
  formula, buffers and sizes, grouping).
- Kernel bench table (workload, terminals, T25 batch, A, A+B, ratios) and the `p1_bench` table.
- Test, clippy, fmt, check_docs and f64 identity results; largest relative error against f64.
- Anything you were unsure about.
