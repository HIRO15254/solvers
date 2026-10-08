# Task T17: faster f32 CFR terminal kernels (P1 postflop solver)

Repository: `C:\Users\PC_User\orca\workspaces\solvers\cisco` (a git worktree, branch `s3-p1-multicore-perf`).
Work only in this directory. Do not commit, push, stash, reset, or switch branches. Leave the changes
uncommitted in the working tree; the reviewer commits them.

## Goal

Make the f32 CFR terminal kernels in `crates/hu-postflop/src/kernel.rs` faster:
`showdown_kernel_relaxed_f32`, `fold_kernel_relaxed_f32`, and their helpers
(`add_relaxed_f32`, `compat_sums_relaxed_f32`, `same_reach_relaxed_f32`).
They are called from `PostflopEvaluator::eval_with_kernel` in `crates/hu-postflop/src/postflop.rs`
when `cfr_precision = "f32"` (the default). A GCP profile (AMD EPYC Milan, 32 threads) of a
real flop solve attributes about 30% of CFR time to the showdown kernel and 16% to the fold kernel,
so a 1.3-1.5x faster kernel is worth roughly 10-15% of every iteration.

Read first: `AGENTS.md`, `crates/hu-postflop/src/kernel.rs` (whole file, including tests),
`crates/hu-postflop/src/postflop.rs` around `eval_with_kernel` and `fold_combos`,
`crates/hu-postflop/src/equity.rs` (how `RankedHands` groups are built),
`crates/nlh/src/range.rs` (`combo_index`: `hi*(hi-1)/2 + lo`),
`crates/hu-postflop/benches/kernels.rs`, `docs/hu-postflop.jp.md` lines 85-100.

## Suspected costs (verify by measurement, do not assume)

1. Store-to-load forwarding chains in the card sums. Hand lists are in global combo order
   (whole list for fold, within each rank group for showdown), so consecutive hands often share
   `cards[0]` (the higher card). `card[h.cards[0]] += r` then forms a serial
   load-add-store chain of roughly 7-8 cycles per hand. `*total += r` is a second serial chain
   (3-4 cycles per hand).
2. In `showdown_kernel_relaxed_f32`, every own rank group zeroes a 52-entry `group_card`, and
   every tied group merges 52 entries into `below_card`. River boards have hundreds of groups of
   1-6 hands, so this fixed per-group work may rival the per-hand work. When an own group has no
   tied opponent group, `tie` is exactly 0: `same` is then 0 too, because an opponent combo
   identical to the own combo has the same rank and would make the group tied.
3. Bounds checks: `card[h.cards[i] as usize]` on `[f32; 52]` cannot be proven in range. A
   `[f32; 64]` array indexed with `& 63` (or similar safe trick) removes the checks.
4. `fold_kernel_relaxed_f32` tests `board & mask` per own hand.

Ideas you may use (all must be measured): several independent accumulators (for example 4 card
arrays and 4 totals assigned by hand index modulo 4, combined at the end in a fixed order);
keeping the running sum of a run of equal `cards[0]` in a register; skipping `group_card` work for
untied groups; handling tied groups with per-hand add/subtract of only the touched entries instead
of 52-wide loops; any other restructuring that keeps the result exact up to f32 rounding.

## Rules

- The f64 kernels (`add`, `compat_sums`, `same_reach`, `showdown_kernel`, `fold_kernel`) and the
  `legacy` test module must not change. `cfr_precision = "f64"` must stay bit-identical to the
  current binary, and evaluation (exploitability, EV, best response) always uses f64.
- The f32 kernels may change their summation order and algebraic form (decision PF5: the f32 path
  need not be bit-identical to the previous f32 binary). They must stay deterministic: the same
  input always gives the same output bits, independent of thread count and of call history.
- Accuracy: relative infinity error against the f64 kernel must stay below 1e-5 in
  `f32_synthetic_kernels_close` (report the old and new maximum it prints). Do not loosen that
  threshold.
- Keep the invariant documented at the top of `kernel.rs` (dead hands have zero reach, hand lists
  may be supersets). Update the module and function comments to describe the new f32 scheme.
- Safe Rust only. `unsafe` is allowed only if it gives a measured kernel gain above 3% that safe
  code cannot reach; then document the safety argument next to it. No new dependencies, no changes
  to workspace or crate features, no changes to `Cargo.toml` files except adding a bench target if
  really needed (prefer extending the existing `benches/kernels.rs`).
- Do not touch `crates/mw-preflop`, `crates/cfr-ref`, or anything outside `crates/hu-postflop`
  except docs if a description becomes wrong. Do not change file formats, configs, or public APIs.

## Benchmark (required)

Extend `crates/hu-postflop/benches/kernels.rs` with f32 variants that go through the real
dispatch (`set_cfr_precision(CfrPrecision::F32)` then `eval_cfr`), for both the showdown and the
fold terminal, and add a second river case with wide ranges (close to the full 1,326-combo table on
both sides, e.g. a typical button vs big blind single-raised-pot river). Keep the existing f64
benches.

The machine is shared with another heavy job, so timings are noisy:
- Before editing any kernel, build the bench (`cargo bench -p hu-postflop --bench kernels --no-run`)
  and copy the produced bench executable (path printed by cargo, under `target/release/deps/`)
  to `C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad\t17-bench-base.exe`.
  Note the f32 bench names must already exist in this baseline, so add the bench cases first,
  build the baseline, then change the kernels.
- Compare baseline and new executables alternately (A B A B A B, at least 3 rounds each), running
  them directly with `--bench <filter>`; report the median time per bench for each side.
- Try more than one variant if the first one does not help; report what you tried and the numbers,
  including variants you rejected.

## Build constraints (important)

The machine is close to its memory commit limit. Always set `CARGO_BUILD_JOBS=2` and
`CARGO_INCREMENTAL=0`. Never run two cargo commands at the same time. If rustc dies with
`STATUS_STACK_BUFFER_OVERRUN`, an out-of-memory error, or a zstd "not enough memory" error, wait a
minute and retry with `CARGO_BUILD_JOBS=1`.

## Verification (run all, report results)

1. `cargo fmt --all --check`
2. `cargo clippy -p hu-postflop --all-targets -- -D warnings`
3. `cargo test -p hu-postflop` (save the full output to a file and report the pass/fail counts)
4. `python tools/check_docs.py`
5. The A/B benchmark above.

## Report format

- Files changed and a short description of the new f32 kernel scheme.
- Benchmark table: bench name, baseline median, new median, ratio.
- Variants tried and rejected, with numbers.
- `f32_synthetic_kernels_close` maximum error before and after.
- Test, clippy, fmt, and check_docs results.
- Anything you were unsure about.
