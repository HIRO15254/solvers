# Task T21: evaluate the terminal children of the opponent's node straight into the node value (f32, P1 postflop)

Repository: `C:\Users\PC_User\orca\workspaces\solvers\cisco` (a git worktree, branch `s3-p1-multicore-perf`,
HEAD `f918dc5`, clean working tree). Work only in this directory. Do not commit, push, stash, reset, or switch
branches. Leave the changes uncommitted; the reviewer commits them.

## Background (16-core/32-thread AMD EPYC Milan VM, default `threads = "auto"` = 32)

T20 (HEAD) did two things for `cfr_precision = "f32"`:
- Part A: the f32 showdown sweep keeps one utility-weighted total/card array (`showdown_relaxed_f32` in
  `crates/hu-postflop/src/kernel.rs`), so an own hand reads two card entries.
- Part B: at the *updating* player's action node, `cfr_pass` (`crates/hu-engine/src/solver.rs`) evaluates the
  terminal children up front through `TerminalEvaluator::eval_cfr_siblings`, skips them in the per-child
  recursion, and `PostflopEvaluator` fuses a same-board fold + showdown pair into one kernel
  (`showdown_fold_kernel_relaxed_f32`).

Measured per iteration at 32 threads: A alone 0-2% faster; A+B 8-9% faster on every tree, at 1, 16 and 32
threads. Earlier work showed that at 32 threads (SMT) latency tricks, multi-accumulator ILP and data-dependent
branches do not help (rejected T17/T18). What helps is fewer kernel calls, fewer passes over hand arrays and less
per-child bookkeeping. T21 applies the Part B idea to the *opponent's* action node, which Part B did not touch.

At the opponent's node (`NodeKind::Action` with `node.player != ctx.p`, the last branch of `cfr_pass`), each child
`a` gets the opponent reach `opp_reach[h] * sigma[a][h]`, and the node value is the sum of the children's values.
Today every terminal child costs: computing its reach, `child_out.fill(0.0)`, a recursive `cfr_pass` call, the
kernel writing into `child_out`, and `out[h] += child_out[h]`. On the river, when the updating player has bet, the
opponent's node has a fold child (fold terminal: the updating player wins) and a call child (showdown terminal) on
the same board, with different reaches. Their fold own-hand loop and the separate output buffer can be removed:
`u_fold * compat(h, r_fold)` is linear in `h`'s card sums, so it can be folded into the showdown sweep's combined
total/card array (start the sweep with `u_lose * all_c + u_fold * F` where `(T_f, F)` are the compat sums of
`r_fold`, and add `u_fold * r_fold[same]` next to the `u_tie * r_call[same]` correction). One own-hand loop then
adds both values into the node value.

## What to implement (f32 only; f64 must not change at all)

1. Engine (`crates/hu-engine/src/solver.rs`): add a defaulted `TerminalEvaluator` method, for example
   `fn add_cfr_opponent_terminals(&self, terminals: &[u32], p: Player, reaches: &[&[f32]], out: &mut [f32], tmp: &mut [f32])`
   that *adds* each terminal's CFR value (reach `reaches[i]`) into `out`, in the given order. The default uses
   `tmp` (zero it before each call) with `eval_cfr` and adds it into `out` exactly as the engine does today.
   Other evaluators (hu-engine test evaluators, `ToyEvaluator`, mw code) must keep working through the default.
2. In `cfr_pass`, at the opponent's node, only when `ctx.cfr_precision == CfrPrecision::F32` and `!zero_opp`:
   - compute the reaches of the terminal children, drop those whose reach is all zero when `PRUNE` is on (their
     contribution is skipped today), and call the new method once (small fixed-size stack batches, no heap
     allocation per node) before the non-terminal children;
   - skip terminal children in both the sequential loop and the parallel (`parallel_actions`) loop, and add the
     non-terminal children's values into `out` in child order in both loops, so the summation order is
     "terminals (child order), then non-terminal children (child order)" regardless of thread count;
   - keep `zero_opp`, `ActionViews`, the scratch discipline and every other existing invariant unchanged.
   The f64 path must keep the current code path and summation order exactly.
3. `PostflopEvaluator` (`crates/hu-postflop/src/postflop.rs`) overrides the method for f32:
   - a same-board fold + showdown pair: one fused kernel that adds `fold(r_fold) + showdown(r_call)` into `out`
     with one own-hand loop (see Background). Use the showdown rank table's opponent hands for the fold's compat
     sums (dead hands have zero reach by the invariant at the top of `kernel.rs`); prove it with a test.
   - any other terminal (a lone fold on the flop/turn, a lone showdown such as a river check-back): an "add"
     variant of the existing f32 kernel that adds into `out` directly (no temporary buffer, no separate add pass).
     Dead own hands must leave `out` unchanged.
   - every f64 call and `TerminalEvaluator::eval` (evaluation, EV, best response, saved EVs) unchanged.
4. Comments: the invariant block at the top of `kernel.rs`, the trait docs, `docs/hu-postflop.jp.md` (the f32
   kernel description near the T20 text, Japanese) and `docs/architecture.md` where it describes `cfr_pass`.
   `python tools/check_docs.py` must pass.

## Rules

- `cfr_precision = "f64"` must stay bit-identical to HEAD (same `.sol` payload, checkpoint arenas and progress;
  the reviewer checks with old/new binaries). The f64 kernels and the f64 call path must not change.
- The f32 path may change rounding but must stay deterministic and bit-identical across thread counts.
- Tests: fused add vs default add (two separate `eval_cfr` + adds) within relative tolerance on real rank
  tables, dense/sparse reaches, `-0.0`, zero reaches, both seats, mixed-sign/zero utilities; add-variants vs
  write-then-add; dead own hands leave `out` untouched; the rank table's opponent list covers every opponent hand
  that is live on the board; an engine test with an evaluator that records calls (terminal rows skipped in
  recursion, PRUNE on/off, zero reach, parallel and sequential branches, empty dimensions); a full solve
  bit-identical at 1 and 4 threads in f32 for F32Storage, I16Storage and MixedStorage.
- Safe Rust only, unless `unsafe` gives a measured gain above 3% that safe code cannot reach (document why).
  No new dependencies, no Cargo.toml changes, no file format changes. Do not touch `crates/mw-preflop` or
  `crates/cfr-ref`. No multi-accumulator/ILP tricks, no data-dependent branches added to inner loops.

## Benchmark (required)

Extend `crates/hu-postflop/benches/kernels.rs` (keep all existing bench names): from the same f64-solved river
(`bench_realistic`), collect opponent-node cases (updating player `p`, fold terminal, showdown terminal, the two
child reaches `opp_reach * sigma`), and bench "default: zero tmp, `eval_cfr`, add, twice" vs "the new method".
Before changing kernel code, build the bench with only the new bench cases
(`cargo bench -p hu-postflop --bench kernels --no-run`) and copy the executable from `target/release/deps/` to
`C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad\t21-bench-base.exe`;
keep the final one as `t21-bench-new.exe` in the same folder. Compare base/new alternately three times by running
the executables directly with `--bench <filter>`, pinned to one logical CPU, and report medians. This machine is
shared with another heavy job, so timings are noisy. The reviewer measures end to end on the 32-thread VM.

## Build constraints (important)

The machine is close to its memory commit limit. Always set `CARGO_BUILD_JOBS=2` and `CARGO_INCREMENTAL=0`.
Never run two cargo commands at the same time. If rustc dies with `STATUS_STACK_BUFFER_OVERRUN` or an
out-of-memory error, wait a minute and retry with `CARGO_BUILD_JOBS=1`.

## Verification (run all, report results)

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (save the full output to `runs/t21/tests.log`; report pass/fail/ignored totals)
4. `python tools/check_docs.py`
5. The benchmark above.

Do not create files under `experiments/` and do not edit `experiments/README.md`; put any evidence files under
`runs/t21/` (ignored by git). The reviewer writes the experiment record.

## Report format

- Files changed; the API; where `cfr_pass` changed; how the summation order and the zero/PRUNE invariants are kept.
- Benchmark table: bench name, base median, new median, ratio; number of opponent-node cases.
- Variants tried and rejected, with numbers.
- How f64 bit-identity, f32 determinism/thread invariance and f32 accuracy are tested; the largest relative error.
- Test, clippy, fmt and check_docs results.
- Anything you were unsure about.
