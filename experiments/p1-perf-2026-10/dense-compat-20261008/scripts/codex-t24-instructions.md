# Task T24: vectorize the f32 card-removal sums and the f32 fold kernel with a dense combo layout (P1 postflop)

Repository: `C:\Users\PC_User\orca\workspaces\solvers\cisco-t22` (a git worktree, branch `s3-p1-t21-on-t22`,
HEAD `fadd0b0`, clean working tree). Work only in this directory. Do not commit, push, stash, reset, or switch
branches. Leave the changes uncommitted; the reviewer commits them.

## Background (16-core/32-thread AMD EPYC Milan VM, default `threads = "auto"` = 32)

HEAD contains T20 (fused fold/showdown siblings at the updating player's node), T21 (terminal children of the
opponent's node added straight into the node value) and T22 (vectorized per-hand loops in `cfr_pass`). After T22
the f32 terminal kernels in `crates/hu-postflop/src/kernel.rs` are about 60% of a CFR iteration at 32 threads.

What does and does not help at 32 threads (SMT) was measured repeatedly:
- latency tricks (independent accumulators, shorter dependency chains) and data-dependent branches that skip zero
  reaches did NOT help at 32 threads (rejected T17, T18), although they helped at 1 and 16 threads;
- fewer instructions per hand did help equally at 1 and 32 threads: T20 (fewer kernel calls and passes) and T22
  (scalar bounds-checked loops turned into AVX2 vector loops, 22% faster at every thread count).

The opponent card-removal sums ("compat sums": `total = sum r[j]`, `card[c] = sum of r[j] over hands j holding c`)
are computed per kernel call by `add_relaxed_f32`/`compat_sums_relaxed_f32` as a scalar loop with two scattered
read-modify-writes into a 52-entry array per opponent hand. They feed:
- the f32 fold kernel (`fold_relaxed_f32`, used by `fold_kernel_relaxed_f32` and `add_fold_kernel_relaxed_f32`),
  whose own-hand loop is `u * (total - card[a] - card[b] + r[same(h)])` with a board-blocked check;
- the initial "everyone loses" sums of the f32 showdown sweep (`showdown_relaxed_f32`: `all_total/all_card`, and
  the `OPP_FOLD` fold sums of T21).

The fold kernel was 10.6% of an iteration and the opponent passes about a fifth before T22.

## Key layout fact

`PostflopHands` (`crates/hu-postflop/src/hands.rs`) builds each seat's support by filtering the global combo index
`0..1326` in increasing order, so a reach slice indexed by a seat's local index is in global combo order. The global
combo index is `hi * (hi - 1) / 2 + lo` with `lo < hi` (`nlh::combo_cards`): combos are grouped by the higher card
`hi`, and inside group `hi` the lower card runs over `0..hi`. With a dense array `d[1326]` (`d[combo] = reach of
that combo`, zero where the seat has no such hand):
- `card[hi] += sum(d[seg(hi)])` (a contiguous segment of length `hi`), and
- `card[0..hi] += d[seg(hi)]` elementwise (a contiguous vector add),
- `total = sum of all d` (any fixed order),
so the whole compat sum is vector work over 1326 lanes plus one plain scatter `d[combo[i]] = r[i]` per support
hand (a store, no read-modify-write). Dead hands carry zero reach by the invariant at the top of `kernel.rs`, so a
dense array over the full seat support gives the same sums as the current lists.

The fold value of an own combo `g = (hi, lo)` is `u * (total - card[hi] - card[lo] + d_opp[g])` (the identical
combo of the opponent is the same dense index, so the `same` lookup disappears). Over a dense own array this is
again a per-segment vector expression: `out_d[seg(hi)] = u * ((total - card[hi]) - card[0..hi] + d_opp[seg(hi)])`.
The own support then gathers `out[i] = out_d[own_combo[i]]` (or adds it, for the add variant); own hands blocked by
the board must get the current behaviour (0 for the write variant, untouched for the add variant).

## What to implement (f32 CFR kernels only; f64 must not change at all)

1. Precompute what the dense layout needs once per seat (for example the local→combo map already in
   `PostflopHands::combos`, or `u16` combo indices stored next to `fold_combos`). No per-call heap allocation; a
   1326-entry `f32` array on the stack per dense buffer is fine.
2. A vectorizable dense compat-sum routine for f32 (zero the dense array, scatter the reach, then the segment sums
   and segment adds above). Write the loops as zipped equal-length slices / `chunks_exact` so LLVM emits AVX2
   without bounds checks (check the hot loops; see `mul_into` in `crates/hu-engine/src/solver.rs` for the style).
3. Use it in the f32 fold kernel (write and add variants), including a dense own-side computation as above if it
   measures faster than the current per-own-hand loop (keep whichever is faster; report both).
4. Use it for the compat sums of `showdown_relaxed_f32` (`all_total/all_card` and the `OPP_FOLD` sums). The sweep
   over rank groups and the own-hand loop of the showdown stay as they are unless you find a measured gain.
5. Comments: the invariant block at the top of `kernel.rs`, and `docs/hu-postflop.jp.md` where the f32 kernels are
   described (Japanese). `python tools/check_docs.py` must pass.

## Rules

- `cfr_precision = "f64"` must stay bit-identical to HEAD (same `.sol` payload, checkpoint arenas and progress).
  Do not touch the f64 kernels, `TerminalEvaluator::eval`, evaluation/EV/BR or the f64 CFR call path.
- The f32 path may change rounding (summation order) but must stay deterministic and bit-identical across thread
  counts. Keep the existing f32 accuracy tests (`precision_tests.rs`, `opponent_precision_tests.rs`) passing with
  their tolerances, and add a test that the dense compat sums match the list-based f64 sums within relative 1e-6
  on real rank tables / supports, both seats, dense, sparse, all-zero and `-0.0` reaches.
- A full solve must stay bit-identical at 1 and 4 threads in f32 for F32Storage, I16Storage and MixedStorage
  (existing tests cover this; make sure they still run).
- Safe Rust only. No new dependencies, no Cargo.toml changes, no file format changes. Do not touch
  `crates/mw-preflop` or `crates/cfr-ref`. No multi-accumulator/ILP tricks and no data-dependent branches added
  to inner loops (both measured useless at 32 threads).

## Benchmark (required)

Use the existing realistic benches in `crates/hu-postflop/benches/kernels.rs` (they replay reaches from a solved
river): `kernels_realistic/t18_fold`, `kernels_realistic/t18_showdown`, `kernels_realistic/t20_siblings`,
`kernels_realistic/t21_opponent_add`, plus `kernels/fold_f32`, `kernels_wide/fold_f32`, `kernels/showdown_f32`,
`kernels_wide/showdown_f32`. Do not rename existing benches; you may add one for a lone fold on a turn board if
none exists. Before changing kernel code, build the bench (`cargo bench -p hu-postflop --bench kernels --no-run`)
and copy the executable from `target/release/deps/` to
`C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad\t24-bench-base.exe`;
keep the final one as `t24-bench-new.exe` in the same folder. Run base/new alternately three times directly with
`--bench <filter>`, pinned to one logical CPU with `RAYON_NUM_THREADS=1`, and report medians. The machine is shared
with another heavy job, so timings are noisy; the reviewer measures end to end on the 32-thread VM.

## Build constraints (important)

The machine is close to its memory commit limit. Always set `CARGO_BUILD_JOBS=1` and `CARGO_INCREMENTAL=0`.
Never run two cargo commands at the same time. If rustc dies with `STATUS_STACK_BUFFER_OVERRUN` or an
out-of-memory error, wait a minute and retry.

## Verification (run all, report results)

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (save the full output to `runs/t24/tests.log`; report pass/fail/ignored totals)
4. `python tools/check_docs.py`
5. The benchmark above.

Do not create files under `experiments/` and do not edit `experiments/README.md`; put evidence files under
`runs/t24/` (ignored by git). The reviewer writes the experiment record.

## Report format

- Files changed; the dense layout and where it is precomputed; which kernels use it.
- Benchmark table: bench name, base median, new median, ratio.
- Variants tried and rejected, with numbers (for example dense own side vs per-own-hand loop for the fold).
- How f64 bit-identity, f32 determinism/thread invariance and f32 accuracy are tested; the largest relative error.
- Test, clippy, fmt and check_docs results.
- Anything you were unsure about.
