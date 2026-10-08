# Task T20: cut the per-own-hand work of the f32 CFR terminal kernels (P1 postflop solver)

Repository: `C:\Users\PC_User\orca\workspaces\solvers\cisco` (a git worktree, branch `s3-p1-multicore-perf`,
HEAD `fdd3618`, clean working tree). Work only in this directory. Do not commit, push, stash, reset, or switch
branches. Leave the changes uncommitted; the reviewer commits them.

## Background and measurements (16-core/32-thread AMD EPYC Milan VM, default `threads = "auto"` = 32)

The f32 terminal kernels are in `crates/hu-postflop/src/kernel.rs` (`showdown_kernel_relaxed_f32`,
`fold_kernel_relaxed_f32`, `compat_sums_relaxed_f32`, `add_relaxed_f32`), dispatched from
`PostflopEvaluator::eval_with_kernel` in `crates/hu-postflop/src/postflop.rs`. The engine calls them through
`TerminalEvaluator::eval_cfr` from `cfr_pass` in `crates/hu-engine/src/solver.rs` (terminal branch, and the
updating player's action-node branch around lines 798-914 that recurses into each child).

A throwaway diagnostic build switched parts off after warmup (Flop tree, 829k nodes; strategy frozen; time per
iteration, 32 threads / 16 threads):
- all terminal kernels: -56% / -59%
- showdown own-hand loop (`for &h in &own.hands[start..end]`): -16.5% / -12%
- fold own-hand loop (`for &h in own`): -12.5% / -10.5%
- both own-hand loops: -28% / -22%
- the opponent passes (`add_relaxed_f32` calls): -22% / -28%
- the 52-card merge of a tied group into the below sums: -2% / -1%
- regret updates: 0%; strategy-sum accumulation: -4%

Two earlier attempts were rejected because they did not help at 32 threads (two threads per core, SMT):
- T17: several independent accumulators to shorten dependency chains (faster at 1/16 threads, 2-3% slower at
  32 threads). Do not use multi-accumulator / ILP tricks.
- T18: skipping zero opponent reach with a per-hand branch in the opponent passes (kernel 20-25% faster on
  one thread, no gain at 32 threads). Avoid data-dependent branches in inner loops.
So the target is the per-own-hand work, which neither attempt touched: fewer loads, flops and stores per own hand
and fewer kernel calls, not lower latency.

## Part A: showdown own-hand loop with one combined per-card array (f32 only)

Today, per own hand: `win = below_total - below_card[a] - below_card[b]`,
`tie = group_total - group_card[a] - group_card[b] + same`, `compat = all_total - all_card[a] - all_card[b] + same`,
`lose = compat - win - tie`, `out = u_win*win + u_tie*tie + u_lose*lose` (6 card loads, the `same` lookup).
Algebraically `out = (u_win-u_lose)*win + (u_tie-u_lose)*tie + u_lose*compat = K - C[a] - C[b] + u_tie*same`, with
`K = (u_win-u_lose)*below_total + (u_tie-u_lose)*group_total + u_lose*all_total` (one scalar per own group) and
`C[c] = (u_win-u_lose)*below_card[c] + (u_tie-u_lose)*group_card[c] + u_lose*all_card[c]`.
Maintain `C` (and `K`) incrementally while the opponent rank groups are merged, so that the own-hand loop does
2 card loads instead of 6. Choose the cheapest correct maintenance (e.g. weighted adds into `C` per opponent hand
of a strictly-below or tied group, and converting a tied group's weight from tie to win when it moves below); do not
add per-own-group 52-element passes beyond what exists today. Measure; keep the variant that is fastest on the
benches below.

## Part B: one call for the fold and showdown children of the updating player's node (f32 only)

At an updating player's action node on the river that faces a bet, the `fold` child and the `call` child are both
terminals (fold, showdown) and receive exactly the same opponent reach (`cfr_pass` passes `opp_reach` unchanged to
every child of the updating player's node). The fold value is `u_fold * compat(h)` (0 for own hands dead on the
board), and the showdown kernel already computes `compat(h)` for every live own hand. Evaluate both in one kernel
call: one set of opponent passes and one own-hand loop that writes both outputs. This removes the separate fold
kernel's opponent pass and own-hand loop for about half of all fold terminal calls (the other half are the
opponent's folds, whose reach differs).

- Engine: add a defaulted method to `TerminalEvaluator` (crates/hu-engine/src/solver.rs) that evaluates several
  terminal children of one updating-player node that share `opp_reach` (for example
  `fn eval_cfr_siblings(&self, terminals: &[u32], p: Player, opp_reach: &[f32], outs: &mut [&mut [f32]])` whose
  default calls `eval_cfr` for each). In `cfr_pass`, at the updating player's node, find the terminal children,
  evaluate them through this method before or after the other children, and do not evaluate them again in the
  per-child recursion. Keep every existing invariant: each `out`/row starts at zero, zero-opponent-reach terminals
  are not evaluated when `PRUNE` is on (rows stay zero), both the sequential and the parallel (`parallel_actions`)
  branches, the `ActionViews` storage splitting, and thread-count independence of all results. Other evaluators
  (test evaluators in hu-engine, `ToyEvaluator`) must keep working unchanged through the default.
- `PostflopEvaluator` overrides it: when the precision is f32 and the siblings contain one fold and one showdown
  terminal on the same board, use the fused kernel; everything else (and every f64 call) goes through the existing
  per-terminal path. Live own hands of the fold output come from the showdown rank table's own list; every other
  entry of the fold output must be 0 (check that the rank table covers every own hand that is live on that board;
  add a test).
- `TerminalEvaluator::eval` (evaluation, EV, best response, saved EVs) must not change at all.

## Rules

- `cfr_precision = "f64"` must stay bit-identical to HEAD: same `.sol` payload, checkpoint arenas and progress
  (the reviewer checks this with old/new binaries). The f64 kernels and the f64 call path must not change.
- The f32 path may change rounding, but must stay deterministic and bit-identical across thread counts.
  Add tests: Part A kernel vs the HEAD f32 kernel and vs the f64 kernel within a relative tolerance on random
  reach (dense, sparse, with `-0.0`, real rank tables from several boards, asymmetric supports, utilities with
  mixed signs and zero); the fused output vs two separate calls within tolerance; fold entries of dead hands are
  exactly 0; a full solve gives bit-identical results at 1 and 4 threads in f32.
- Safe Rust only, unless `unsafe` gives a measured gain above 3% that safe code cannot reach (then document the
  safety argument). No new dependencies, no Cargo.toml changes, no file format changes. Do not touch
  `crates/mw-preflop` or `crates/cfr-ref`.
- Update comments (the invariant block at the top of `kernel.rs`), `docs/hu-postflop.jp.md` (around line 93-97,
  the f32 kernel description, Japanese) and `docs/architecture.md` if its `cfr_pass` description becomes wrong.
  `python tools/check_docs.py` must pass.

## Deliverables in two steps

1. Implement Part A alone first. When it passes the checks below, save its standalone diff against HEAD to
   `C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad\t20a.patch`
   (it must apply to `fdd3618` and build and pass `cargo test -p hu-postflop` by itself).
2. Then implement Part B on top. The final working tree holds A+B.

## Benchmark (required)

`crates/hu-postflop/benches/kernels.rs` already has f32 cases through `eval_cfr`, synthetic zero masks, and a
realistic set of terminal calls collected from a solved river (`bench_realistic`). Extend it:
- realistic sibling pairs: from the same solved river, collect (updating player, fold terminal, showdown terminal,
  opponent reach) for updating-player nodes whose children include both, and bench "two separate `eval_cfr` calls"
  vs "the sibling method" over the whole set;
- keep the existing bench names so old and new executables can be compared.
Before changing kernel code, build the bench with only the new bench cases
(`cargo bench -p hu-postflop --bench kernels --no-run`) and copy the executable from `target/release/deps/` to
`...\scratchpad\t20-bench-base.exe` (same scratchpad folder as above); also keep the Part A executable as
`t20-bench-a.exe`. Compare base / A / A+B alternately (A B C A B C A B C) by running the executables directly with
`--bench <filter>`, and report medians. This machine is shared with another heavy job, so timings are noisy; pin
to one logical CPU if that helps. The reviewer measures end to end on the 32-thread VM.

## Build constraints (important)

The machine is close to its memory commit limit. Always set `CARGO_BUILD_JOBS=2` and `CARGO_INCREMENTAL=0`.
Never run two cargo commands at the same time. If rustc dies with `STATUS_STACK_BUFFER_OVERRUN` or an
out-of-memory error, wait a minute and retry with `CARGO_BUILD_JOBS=1`.

## Verification (run all, report results)

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (save the full output to `runs/t20/tests.log`; report pass/fail/ignored totals)
4. `python tools/check_docs.py`
5. The benchmark above.

## Report format

- Files changed and a short description of Part A and Part B (API, where cfr_pass changed, how rows/zero
  invariants are kept).
- Benchmark table: bench name, base median, A median, A+B median, ratios; plus the number of sibling pairs in the
  realistic set.
- Variants tried and rejected, with numbers.
- How f64 bit-identity, f32 determinism/thread invariance and f32 accuracy are tested; the largest relative
  error seen against f64.
- Test, clippy, fmt and check_docs results.
- Anything you were unsure about.
