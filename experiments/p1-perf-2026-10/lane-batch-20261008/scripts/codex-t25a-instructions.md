# Task T25a: a lane-batched f32 terminal kernel for whole river subtrees (P1 postflop) — prototype and benchmark

Repository: `C:\Users\PC_User\orca\workspaces\solvers\cisco-t25` (a git worktree, branch `s3-p1-t25`, HEAD
`76089bc`, clean working tree). Work only in this directory. Do not commit, push, stash, reset, or switch
branches. Leave the changes uncommitted; the reviewer commits them.

This is stage 1 of a two-stage change. Stage 1 (this task) adds the kernel, the evaluator entry point and a
benchmark, and answers one question: how much faster is evaluating all terminals of a river subtree in one
lane-batched sweep than the calls the CFR pass makes today? Stage 2 (a later task, only if stage 1 shows a large
gain) will change the engine's f32 CFR pass to collect the terminals of each chance-free subtree first and call
the new entry point once. Design the API for that use.

## Background (16-core/32-thread AMD EPYC Milan VM, default `threads = "auto"` = 32)

HEAD contains T20 (`eval_cfr_siblings`: fused fold/showdown terminal children at the updating player's node), T21
(`add_cfr_opponent_terminals`: terminal children of the opponent's node added straight into the node value, with
the fold sums seeded into the showdown sweep), T22/T23 (vectorized `cfr_pass` loops) and T24 (dense combo-order
card-removal sums and a dense f32 fold kernel; `dense_compat_sums_f32` in `crates/hu-postflop/src/kernel.rs`).

A sampling profile at 32 threads (Flop1, before T24): `add_cfr_opponent_terminals` 35%, `eval_cfr_siblings` 24%,
`cfr_pass` 13%, regret matching 13%, `showdown_kernel_relaxed_f32` 6%. Inside the two terminal functions most
samples are in the showdown rank sweep (`showdown_relaxed_f32`: per opponent hand a reach gather and two to four
scattered read-modify-writes into 52-entry card arrays; per own hand two card loads, an identical-combo lookup and
a scattered store) and in the card-removal sums.

What helps at 32 threads (SMT) was measured repeatedly: fewer instructions per hand helps equally at 1 and 32
threads (T20, T22: AVX2 vector loops instead of scalar ones). Latency tricks (independent accumulators) and
data-dependent branches that skip zero reaches did not help at 32 threads (rejected T17, T18).

## Idea

All terminals below one river street root share one board (one rank table). Today each terminal costs a full
sweep. Evaluate up to `LANES = 8` terminals in one sweep instead, with every per-hand quantity an `[f32; 8]` lane
vector (one lane per terminal): the hand loads, rank-group control flow and card indices are shared, and the
arithmetic becomes one AVX2 operation for eight terminals.

- Interleave the opponent reaches once per batch: `buf[i] = [reach_0[i], ..., reach_7[i]]` over the opponent's
  local index `i` (unused lanes 0). Reserve one extra all-zero row so the identical-combo lookup can be
  branch-free (map `ABSENT` to that row, for example with `min`).
- Initial "everyone loses" sums: `total` and `card: [[f32; 8]; 52]` over the opponent ranked list, lane-wise,
  then scaled lane-wise by `u_lose`.
- The rank sweep exactly as `showdown_relaxed_f32` does it, with lane vectors and per-lane utilities
  (`u_win - u_lose`, `u_tie - u_lose`, `u_win - u_tie`). Avoid re-zeroing a full `[[f32; 8]; 52]` group array
  per own group if it costs much (for example only when the group is tied, or by undoing the touched entries);
  measure.
- Own hands: `value = total - card[a] - card[b] + u_tie * buf[same]` lane-wise, then store lane `t` to
  `outs[t][h.local]` (or via an interleaved output buffer if that measures faster).
- Fold terminals fit the same sweep: a fold has the same utility for every outcome, so a fold lane uses
  `u_win = u_tie = u_lose = u_fold` (all sweep increments are zero and the value is
  `u_fold * (total - card[a] - card[b] + same)`). On a complete 5-card board the rank table covers every
  board-live hand and board-dead hands carry zero reach (see the invariant at the top of `kernel.rs`), so this
  gives the fold value for every live own hand. Board-dead own hands must stay 0. Fold terminals do not record a
  rank table today (`PostflopTerminal::table` is `u32::MAX`); precompute, at evaluator construction, the rank
  table of each fold terminal whose board is a complete 5-card board that has a table (for example from the
  showdown terminals' `board_mask -> table`). Folds on earlier streets keep their current kernel.

Per-lane arithmetic does not depend on the other lanes, so a terminal's result must not depend on which batch or
lane it lands in. Make that a tested property (bitwise).

## What to implement

1. `crates/hu-engine/src/solver.rs`: a new `TerminalEvaluator` method for f32 CFR, for example

   ```rust
   /// CFR terminals for the updating player `p`, each with its own opponent reach; each output starts at
   /// zero and receives that terminal's values (as `eval_cfr` would write them). Default: one `eval_cfr`
   /// per terminal. Overrides may batch terminals with relaxed f32 rounding; results must not depend on
   /// how the caller groups terminals or on thread count.
   fn eval_cfr_batch(&self, terminals: &[u32], p: Player, opp_reaches: &[&[f32]], outs: &mut [&mut [f32]]);
   ```

   Do not change `cfr_pass` or any other engine code in this stage.
2. `crates/hu-postflop`: the lane-batched kernel in `kernel.rs` (f32 only), and the `PostflopEvaluator` override:
   group the given terminals by rank table (showdowns and river folds), run batches of up to 8 lanes, and fall
   back to `eval_cfr` for anything else (folds on non-river boards, all-in runout terminals that are not a
   single rank table, f64 precision). No per-call heap allocation in the kernel; stack buffers of a few tens of
   KB are fine (the rayon worker threads have the default stack size; say what you use). Grouping may use a
   small fixed-size stack structure or reuse a thread-local buffer; say what you chose.
3. Tests: accuracy of every lane against the f64 kernels (`eval`) on real rank tables with the existing relative
   tolerance style (1e-5 of the max-norm), fold lanes included; board-dead own entries stay 0; bitwise
   independence of the grouping (each terminal alone vs in a batch of 8 vs mixed batches); all-zero and `-0.0`
   reaches.
4. Benchmark, in `crates/hu-postflop/benches/kernels.rs` (do not rename existing benches): a new
   `kernels_realistic` workload built like the existing realistic ones (the wide river with bet fractions
   0.33/0.75/1.5 and `max_raises = 3`, f64-solved for 300 + 50 + 50 iterations, reaches from `reach_at` and
   `current_strategy_at`). For each player `p` and each snapshot, take every terminal of the tree with the
   opponent reach at that terminal, and measure three ways of producing all those terminal values:
   - `t25_lone`: one `eval_cfr` per terminal;
   - `t25_current`: exactly the calls `cfr_pass` makes today in f32: at each `p` node one `eval_cfr_siblings`
     over its terminal children (pairs, in child order, as `cfr_pass` groups them) with the node's opponent
     reach; at each opponent node one `add_cfr_opponent_terminals` over its terminal children (pairs) with the
     child reaches (opponent reach times that action's current strategy);
   - `t25_batch`: one `eval_cfr_batch` over all terminals.
   Also report the number of terminals per player and the lane fill (terminals / (batches * 8)).

## Rules

- `cfr_precision = "f64"` and all existing f32 paths must stay bit-identical to HEAD: this stage adds code but
  changes no existing behavior (no existing kernel, `cfr_pass`, evaluation/EV/BR or format change).
- Safe Rust only. No new dependencies, no Cargo.toml changes. Do not touch `crates/mw-preflop` or
  `crates/cfr-ref`. No multi-accumulator/ILP tricks and no data-dependent branches inside the per-hand loops.
- Write hot loops as zipped equal-length slices / `chunks_exact` / fixed-size arrays so LLVM emits AVX2 without
  bounds checks; check the hot loops in the linked executable as in T24
  (`C:\Users\PC_User\orca\workspaces\solvers\cisco-t22\runs\t24\` has your earlier scripts; read-only).

## Build constraints (important)

The machine is close to its memory commit limit and is shared with another heavy job. Always set
`CARGO_BUILD_JOBS=1` and `CARGO_INCREMENTAL=0`. Never run two cargo commands at the same time. If rustc dies with
`STATUS_STACK_BUFFER_OVERRUN` or an out-of-memory error, wait a minute and retry.

## Benchmark procedure

Build the bench executable, run `t25_lone`, `t25_current` and `t25_batch` alternately three times, pinned to one
logical CPU with `RAYON_NUM_THREADS=1`, and report medians. Keep the final executable as
`C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad\t25a-bench.exe`.
Timings are noisy on this machine; the ratio between the three in the same run matters.

## Verification (run all, report results)

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (save the full output to `runs/t25a/tests.log`; report pass/fail/ignored totals)
4. `python tools/check_docs.py`
5. The benchmark above.

Do not create files under `experiments/` and do not edit `experiments/README.md`; put evidence files under
`runs/t25a/` (ignored by git). Update comments where you add code; docs (`docs/hu-postflop.jp.md`) only if you
change described behavior (this stage should not).

## Report format

- Files changed; the kernel layout (lane buffers, sizes, where they live); how terminals are grouped.
- Benchmark table: workload, terminals per player, lane fill, `t25_lone`, `t25_current`, `t25_batch` medians,
  ratios batch/current and batch/lone.
- Variants tried and rejected, with numbers (group-array reset strategy, output store strategy, lanes 4 vs 8 if
  you try it).
- How accuracy and grouping independence are tested; the largest relative error.
- Test, clippy, fmt and check_docs results.
- Anything you were unsure about, and what stage 2 (the engine side) would need from this API.
