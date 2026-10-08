# Task T18: skip zero opponent reach in the f32 CFR terminal kernels (P1 postflop solver)

Repository: `C:\Users\PC_User\orca\workspaces\solvers\cisco` (a git worktree, branch `s3-p1-multicore-perf`).
Work only in this directory. Do not commit, push, stash, reset, or switch branches. Leave the changes
uncommitted; the reviewer commits them. The working tree already has one uncommitted change that you
must keep: `crates/hu-postflop/benches/kernels.rs` (f32 benches through `eval_cfr`, and a full-support
"kernels_wide" river case). Build on it.

## Background and measurements

`crates/hu-postflop/src/kernel.rs` has f64 kernels (`showdown_kernel`, `fold_kernel`, `compat_sums`,
`add`) and f32 kernels (`showdown_kernel_relaxed_f32`, `fold_kernel_relaxed_f32`,
`compat_sums_relaxed_f32`, `add_relaxed_f32`). The f32 ones run in every CFR pass by default
(`cfr_precision = "f32"`, dispatched in `PostflopEvaluator::eval_with_kernel`, `crates/hu-postflop/src/postflop.rs`).
On a 16-core/32-thread AMD EPYC Milan VM they take about 30% (showdown) + 16% (fold) of CFR time.

We counted, at every f32 terminal call that is actually evaluated (calls whose opponent reach is all
zero are already skipped by `cfr_pass`), how many hands of the opponent list the kernel walks have zero
reach: 64-86% depending on the tree (80% on a large GTO-Wizard-like flop tree, where half of all calls
have at least 90% zeros). The f64 `add` already skips zero reach with a branch; the f32 kernels add
every entry, including zeros.

A previous attempt (T17) used several independent accumulators to shorten dependency chains. It was
faster single-threaded but 2-3% slower end to end at 32 threads, because the default runs two threads
per core (SMT) and the sibling thread already hides the latency. So the goal here is to execute less
work, not to add instruction-level parallelism. Do not use multi-accumulator tricks.

## Goal

Make the f32 kernels skip opponent hands with zero reach, so that their cost scales with the number of
nonzero opponent hands, while producing bit-identical outputs to the current f32 kernels.

Why bit-identity is achievable: every f32 running sum in these kernels starts at `+0.0` and reach
values are non-negative (possibly `-0.0`). Adding `+0.0` or `-0.0` to such a sum never changes its bits,
so skipping every `r == 0.0` term (this test is also true for `-0.0`) gives exactly the same sums,
provided the nonzero terms are still added in the same order.

## Rules

- f32 outputs must be bit-identical to the current f32 kernels for every input. Keep the order of
  nonzero additions unchanged. Copy the current f32 kernels into the existing `#[cfg(test)] mod legacy`
  (or a new test-only module) as the reference, and add tests that compare `to_bits()` of the new and
  reference outputs on random inputs with many zero and some `-0.0` reach entries, on dense inputs, on
  all-zero inputs, on empty lists, and on real rank tables (see the existing tests for how they build
  `RankedHands` from a board).
- The f64 kernels (`add`, `compat_sums`, `same_reach`, `showdown_kernel`, `fold_kernel`) must not
  change. Evaluation (exploitability, EV, best response) always uses them.
- Dense inputs (no zeros) must not get more than about 3% slower in the kernel benches.
- Safe Rust only, unless `unsafe` gives a measured gain above 3% that safe code cannot reach (then
  document the safety argument). No new dependencies, no Cargo.toml changes, no public API or file
  format changes. Do not touch `crates/mw-preflop` or `crates/cfr-ref`. Only `crates/hu-postflop`
  (and docs if a description becomes wrong; `docs/hu-postflop.jp.md` around line 93 describes the f32
  kernels).
- Keep the invariant documented at the top of `kernel.rs` (dead hands have zero reach, hand lists may
  be supersets) and update comments to describe the new scheme.

## Ideas (measure, do not assume)

(a) A branch `if r != 0.0` in the f32 add loops, like the f64 `add`. Zeros tend to be clustered by hand
    strength in rank order (showdown lists), less so in combo order (fold lists), so prediction may differ.
(b) One compaction pass over the opponent list that collects the nonzero hands (and their reach) and,
    for showdown, the compacted end offset of every opponent rank group; then run the total/card-sum
    pass and the rank merge over the compacted list only. This reads each opponent hand once instead
    of twice in the showdown kernel. A small reusable scratch buffer (thread-local or a stack array of
    the maximum list length, at most 1,326 entries) avoids allocation; keep it deterministic.
(c) Anything else that skips work proportional to the zeros while keeping exact bits.
Pick the best by measurement. A combination (e.g. dense path when few zeros) is fine if it stays bit-identical.

## Benchmark (required)

Extend `crates/hu-postflop/benches/kernels.rs`:
1. Synthetic sparsity on the existing narrow and wide river cases: reach vectors with 0%, 50%, 80% and
   95% zeros (deterministic pseudo-random choice of which hands are zero), for f32 fold and showdown.
2. Realistic reach: build a river game with several bet sizes and raises (see `RiverConfig`), run CFR
   on it for a few hundred iterations with the public solver API, then collect the opponent reach
   vectors that real terminal calls would see (for example with `hu_engine::reach_at` or by walking the
   tree with the current strategy), for many fold and showdown terminals and both players. Bench the f32
   kernels summed over that whole set. Report the measured zero fraction of the set.
Measure on this machine (shared with another heavy job, so timings are noisy): before changing kernel
code, build the bench (`cargo bench -p hu-postflop --bench kernels --no-run`) and copy the produced
executable from `target/release/deps/` to
`C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad\t18-bench-base.exe`
(so add the new bench cases first, build the baseline, then change the kernels). Compare baseline and
new executables alternately (A B A B A B), running them directly with `--bench <filter>`, and report
medians. The reviewer will also measure on the EPYC VM.

## Build constraints (important)

The machine is close to its memory commit limit. Always set `CARGO_BUILD_JOBS=2` and
`CARGO_INCREMENTAL=0`. Never run two cargo commands at the same time. If rustc dies with
`STATUS_STACK_BUFFER_OVERRUN` or an out-of-memory error, wait a minute and retry with `CARGO_BUILD_JOBS=1`.

## Verification (run all, report results)

1. `cargo fmt --all --check`
2. `cargo clippy -p hu-postflop --all-targets -- -D warnings`
3. `cargo test -p hu-postflop` (save the full output to `runs/t18/tests.log`; report pass/fail/ignored totals)
4. `python tools/check_docs.py`
5. The A/B benchmark above.

## Report format

- Files changed and a short description of the scheme.
- Benchmark table: bench name, baseline median, new median, ratio; plus the realistic set's zero fraction.
- Variants tried and rejected, with numbers.
- How bit-identity is tested.
- Test, clippy, fmt and check_docs results.
- Anything you were unsure about.
