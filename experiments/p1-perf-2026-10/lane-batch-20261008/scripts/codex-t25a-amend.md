Continue task T25a in `C:\Users\PC_User\orca\workspaces\solvers\cisco-t25` from where you were interrupted (your
uncommitted changes are still there). The reviewer interrupted you to add one requirement that comes from a
measurement that just finished; everything in the original task still applies.

New evidence (T24, the dense combo-order card-removal sums you wrote earlier, which is HEAD here): on the
32-thread VM it made whole CFR iterations SLOWER although every kernel bench was faster. One iteration
(`p1_bench`, real trees with realistic preflop ranges): Flop1 1.06x at 1 thread, 1.08x at 16, 1.12x at 32;
gtow_b 1.12x and a single river tree 1.14x at 32 threads. The kernel benches that showed 0.52-0.91x all use the
wide river config, where every one of the 1,081 combos is in both supports. Real supports are much narrower
(about 400-700 hands per seat, fewer live on a river board), so a fixed per-call cost proportional to 1326 lanes
(zeroing and a 51-segment pass over a 5.3 KB stack array, twice per showdown with the T21 fold sums) outweighed
the saved per-hand work, and the extra stack footprint per call also competes for the L1 cache shared by two SMT
threads. T24 will therefore be rejected; you do not need to undo it in this worktree (keep HEAD as it is).

What this means for T25a:
1. Add realistic-range workloads to the new `t25_*` benches and treat them as the decisive ones: build the river
   game with the preflop ranges of the repository's P1 test spots instead of full ranges, at least
   - BTN `22+,A2s+,K2s+,Q5s+,J7s+,T7s+,97s+,86s+,75s+,65s,54s,A5o+,K9o+,Q9o+,J9o+,T9o` vs
     BB `99-22,AJs-A2s,KJs-K2s,Q4s+,J6s+,T6s+,96s+,85s+,74s+,64s+,53s+,43s,AJo-A2o,K8o+,Q9o+,J9o+,T8o+,98o`
     on board `Ks 7h 2d 3c 9s`, bet fractions 0.33/0.75/1.5 plus all-in if the river config supports it,
     `max_raises = 3` (the same tree shape as the existing realistic bench), f64-solved like the others;
   - keep the wide workload as a second, secondary case.
   Report `t25_lone`, `t25_current` and `t25_batch` for both, with the support sizes.
2. Keep every per-call fixed cost and the stack footprint proportional to the actual support and lane count,
   not to 1326 or to 1327 x 8: size interleaved buffers to the opponent support length (+1 zero row), only touch
   the rows that are used, and avoid zero-filling whole fixed-size arrays per call. You reported about 87 KiB of
   stack arrays for the current kernel; bring that down (for example a reused per-worker buffer sized to the
   support, or stack arrays bounded by the actual length) and measure the effect on the realistic workload.
   Two SMT threads share a 32 KiB L1D on the target machine.
3. In the report, add a short estimate of the per-call fixed cost (time for a batch with all-zero reaches or with
   one lane) next to the per-terminal cost, for both workloads.

Same rules, build constraints (`CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, one cargo command at a time),
verification list and report format as the original task.
