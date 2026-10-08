# Task T25b: use the lane-batched terminal evaluation in the f32 CFR pass (stage 2), after removing T24

Continue in `C:\Users\PC_User\orca\workspaces\solvers\cisco-t25` (branch `s3-p1-t25`, HEAD `76089bc` = T24 as a WIP
commit on top of `b01d4a4`; your T25a changes are uncommitted). Do not commit, push, stash, reset, or switch
branches. Leave all changes uncommitted.

## Step 0: remove T24 from the working tree

T24 (dense card-removal sums, the HEAD WIP commit) was measured on the 32-thread VM and rejected: real trees got
1.06x (1 thread) to 1.12-1.14x (32 threads) slower per iteration although all kernel benches were faster. Its
profile showed `dense_compat_sums_f32` at 23-25% of an iteration, about three times the list sums it replaced.

Reverse-apply the T24 patch onto the working tree:
`C:\Users\PC_User\orca\workspaces\solvers\cisco\experiments\p1-perf-2026-10\dense-compat-20261008\scripts\t24.patch`
(`git apply -R` will conflict where T25a touched the same files; resolve by hand). Keep T25a working without any T24
code. Afterwards `git diff b01d4a4` must show only T25a/T25b changes: no `dense_compat_sums_f32`, no
`combos`/`opp_combos` kernel parameters, no `DENSE_MIN_HANDS`, the docs paragraph of T24 in `docs/hu-postflop.jp.md`
gone. Then re-run the T25a benchmark (`t25_lone`, `t25_current`, `t25_batch`, both workloads, three alternating runs
as before) so `t25_current` reflects the real current kernels (T23), and report it before going on.

## Step 1: the engine change (f32 CFR only)

Goal: every chance-free action subtree that cannot parallelize internally is processed by one non-recursive
three-phase pass that calls `eval_cfr_batch` once for all its terminals. f64 (`cfr_precision = "f64"`) must keep the
current `cfr_pass` path exactly (bit-identical `.sol` payload, checkpoint arenas and progress).

Facts about `PublicTree` (`crates/hu-engine/src/tree.rs`), check them yourself:
- `fill` reserves a node's children contiguously and then fills each child recursively, so all descendants of a
  node `R` occupy one contiguous id range starting at `R.first_child`, every child id is larger than its parent's,
  and the descendants of `R` do not include `R` itself (whose id is in its parent's child block).
- `subtree_has_chance[id]` and `storage_spans[id]` (a contiguous storage range covering `R` and all its descendants'
  storage refs) already exist.
- In `solver.rs`, `ActionViews::split_for` returns `Ambient` (no split) whenever
  `!subtree_has_chance[node] && subtree_elements(node) < 2 * ACTION_PAR_MIN_ELEMENTS`, and `parallel_actions` cannot
  be true inside such a subtree. Use exactly this condition (plus `cfr_precision == F32` and `node.kind == Action`)
  to enter the batched pass at the first such node the recursion reaches; inside it the recursion is never used.

The batched pass for root `R` (with `my_reach`, `opp_reach`, zeroed `out`, the ambient storage view), PRUNE semantics
identical to today's `cfr_pass`:
1. Top-down (`R`, then descendants in ascending id order): at each action node compute the current strategy with
   `regret_matching_cfr` (skip it at an opponent node whose opponent reach is all zero, as today) and the children's
   reaches: at the updating player's node `my_reach * sigma[a]` (opponent reach unchanged), at the opponent's node
   `opp_reach * sigma[a]` (own reach unchanged; all-zero reach stays shared zero). Share unchanged reaches by
   reference/index instead of copying. Record every terminal with its opponent reach; with PRUNE, a terminal whose
   opponent reach is all zero gets value 0 and is not sent to the batch (as today).
2. One `eval_cfr_batch` call for all recorded terminals (outputs zeroed, one row per terminal).
3. Bottom-up (descendants in descending id order, then `R`): an updating-player node computes
   `node_cfv = sum_a sigma[a] * v_a` in action order, the instantaneous regrets `v_a - node_cfv`, calls
   `update_regrets`, then `accumulate_strategy` with `my_reach * sigma` (the same calls, arguments and order per node
   as today), and stores `node_cfv` as its value. An opponent node's value is the sum of its children's values in
   child order (all-zero opponent reach: value 0, children still visited so own nodes below update as today).
   `R`'s value goes to `out`.

Memory: take the arenas from `scratch` (LIFO as elsewhere) or reused per-worker buffers; no per-call heap allocation
after warm-up. The sigma arena can mirror the storage span (`sref.offset - span.start`). Per-node value rows and the
reaches of the children of action nodes are needed; reach rows that equal the parent's should not be copied. Report
the arena sizes for the realistic trees below. The batch should keep the terminal kernels' cache footprint
proportional to the actual supports (T24 lesson).

Keep everything else as it is: the T20/T21 sibling paths stay for f32 nodes outside batched subtrees, the
evaluation/EV/BR passes and all f64 code are untouched, `TerminalEvaluator` default methods keep other evaluators
working (their results may change only by f32 rounding if they use the batch default, which calls `eval_cfr`, so
they should stay bit-identical; check the toy-game/engine tests).

## Rules

- f64 bit-identical to `b01d4a4` (`.sol` payload, checkpoint arenas, progress). The f32 path may change rounding but
  must be deterministic and bit-identical across thread counts (existing tests run F32Storage, I16Storage,
  MixedStorage at 1 and 4 threads; keep them running and passing).
- Safe Rust only. No new dependencies, no Cargo.toml changes, no file format changes. Do not touch
  `crates/mw-preflop` or `crates/cfr-ref`. No multi-accumulator/ILP tricks and no data-dependent branches inside
  per-hand loops.
- Comments where code changes; `docs/hu-postflop.jp.md` (Japanese) where the f32 CFR terminal handling is
  described: say that chance-free subtrees below the parallel threshold evaluate all their terminals in lane
  batches. If an engine doc describes `cfr_pass`, update it. `python tools/check_docs.py` must pass.

## Tests to add

- f32: the batched pass gives the same values/regrets/strategy sums as the recursive f32 pass with per-terminal
  `eval_cfr` up to f32 rounding (relative 1e-5 of the max-norm per node) on a real river tree and a turn tree, for
  both players, with and without PRUNE, and with all-zero opponent reach at some opponent nodes.
- f32 determinism: bit-identical state at 1 and 4 threads (existing tests may already cover it; make sure the
  batched path is exercised there, i.e. the test trees contain chance-free subtrees below the threshold).
- f64: unchanged (existing tests).

## Benchmarks (local, required; the reviewer measures at 32 threads on the VM)

A baseline executable of `p1_bench` built from `b01d4a4` (T23) will be at
`C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad\t23-p1_bench.exe`
(the reviewer builds it; if it is not there when you need it, wait; do not build it yourself). Build the new one with
`cargo build --release -p hu-postflop --example p1_bench` and copy it to `t25b-p1_bench.exe` in the same folder.
Configs (in this worktree): `experiments/p1-perf-2026-10/scratch-overwrite-20261008/configs/c_river.toml`,
`c_turn2.toml`, `c_flop1.toml`. Run baseline/new alternately three times each, `--evals 0 --json <file>`:
- `c_river.toml --threads 1 --warmup 20 --iters 200`
- `c_turn2.toml --threads 1 --warmup 5 --iters 50`
- `c_flop1.toml --threads 4 --warmup 2 --iters 6` (about 1 GB of memory; skip it and say so if memory is short)
Report `secsPerIter` medians and ratios.

## Build constraints (important)

The machine is close to its memory commit limit and is shared with another heavy job. Always set
`CARGO_BUILD_JOBS=1` and `CARGO_INCREMENTAL=0`. Never run two cargo commands at the same time. If rustc dies with
`STATUS_STACK_BUFFER_OVERRUN` or an out-of-memory error, wait a minute and retry.

## Verification (run all, report results)

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (full output to `runs/t25b/tests.log`; report pass/fail/ignored totals)
4. `python tools/check_docs.py`
5. f64 identity: a full f64 turn solve at 1 and 4 threads: `.sol` payload, checkpoint arenas and progress byte-equal
   to `b01d4a4` (your T24 method in `C:\Users\PC_User\orca\workspaces\solvers\cisco-t22\runs\t24\` can be reused).
6. The benchmarks above.

Evidence goes to `runs/t25b/` (ignored by git). Do not create files under `experiments/`.

## Report format

- Step 0 result (files, how conflicts were resolved) and the re-run T25a table against T23 kernels.
- Engine change: entry condition, phases, arenas and their sizes on the three trees, how PRUNE and zero reach are
  handled, what other evaluators see.
- Test, clippy, fmt, check_docs, f64 identity results; largest f32 relative difference against the recursive pass.
- The `p1_bench` table (baseline, new, ratio).
- Anything you were unsure about.
