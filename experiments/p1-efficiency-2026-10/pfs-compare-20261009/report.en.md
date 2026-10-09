# P1 vs b-inary/postflop-solver: matched-game benchmark (2026-10-09)

**Bottom line.** On three games with identical trees, P1 reaches 0.05% pot in **0.36–0.74× the iterations** of postflop-solver (pfs). Each P1 iteration is also **1.30–1.64× faster**. Excluding evaluation, P1 reaches 0.05% pot **2.1–3.6× sooner**. P1 has no headroom against pfs on iteration count. A test run shows pfs's larger iteration count comes from its DCFR schedule, not from some trick P1 lacks: P1 configured with pfs's schedule needs about the same number of iterations as pfs. There is a little headroom in three places:
- **Thread scaling:** at 8 threads pfs gets 5.0–5.1× over one thread; P1 gets 4.5–4.7×.
- **Evaluation cost:** one P1 exploitability evaluation costs about 1.8–2.3× a P1 iteration. One pfs evaluation costs about 0.86–0.88× a pfs iteration.
- **Memory:** P1's f32 peak is 7–51% above pfs f32. pfs i16 halves memory and still converges, though it needs more iterations.

## Setup

- pfs: `github.com/b-inary/postflop-solver` @ `9d1509fe5077d019825f833eed04b16d342dfda1` (2023-10-01). It is cloned to `.cache/pfs-bench/postflop-solver`, and the source was not modified.
- Bench harness: `.cache/pfs-bench/harness` (my own code; depends on pfs by path).
  - Built with features `rayon` only. `bincode` is off because the upstream crate does not build against bincode 2.0.1.
  - Flags: `-C target-cpu=native`, `-A dangerous_implicit_autorefs` (a lint added by newer rustc that rejects upstream code). Release profile with fat LTO and codegen-units 1. Rayon global pool of 8 threads.
  - A second build used nightly-2025-04-08 with pfs's `custom-alloc` bump allocator, to check how much per-node heap allocation costs. It made no measurable difference (see below).
- P1: the existing `target/release/solvers.exe`, not rebuilt. The workspace `.cargo/config.toml` already sets `target-cpu=native`. Run with `--threads 8`, default DCFR (alpha 1.25, beta 0.5, gamma 4, no reset), f32 storage.
- Host: Windows, 8 cores / 16 threads, shared. The two solvers ran alternately, never at the same time. Background load from other processes was logged on every run: 1.9–3.6 cores on average. Turn rep 1 ran while other work was heavier (its numbers are about 1.4–1.6× slower for both solvers), so medians are used.
- Metric: NashConv/2 as % of the starting pot (5.5 BB).
  - pfs `compute_exploitability` = (BR value OOP + BR value IP)/2 with the pot/2 bias removed, which is NashConv/2 for a zero-sum game. It is computed in f32.
  - P1 reports `nash_conv/2`, computed in f64.
  - Evidence that the two numbers measure the same thing: P1 run with pfs's schedule follows the pfs curve closely on the turn. Iteration 1030: 0.0838% vs 0.0883%. Iteration 1200: 0.0550% vs 0.0591%.
- Both solvers evaluate every 10 iterations.
  - pfs time is split by the harness into `solve_secs` (only `solve_step`) and evaluation time.
  - P1's `elapsed_secs` includes evaluation. To separate them, each P1 run that stops at the target (run A: `check_every = 10`) is paired with a run with no target (run D: `check_every = 50`, `max_iterations` = A's stopping iteration rounded down to a multiple of 50). At iteration L:
    - evaluation cost E = (A(L) − D(L)) / (L/10 − L/50)
    - per-iteration time = (D(L) − (L/50)·E) / L
  - Per-iteration cost was flat over the run for both solvers. Turn: P1 19.5–21.0 ms, pfs 26–28 ms per 50-iteration window. Flop: P1 278–290 ms, pfs 365–370 ms. So averages are representative.
- Peak memory is the process peak working set from psutil `peak_wset`.
  - P1: read when the `done:` line is printed, so the `.sol` write afterwards is not included.
  - pfs: read at process end. This includes `finalize`, which allocates nothing large.
- Repetitions:
  - river: 3 per arm.
  - turn: 4 per arm.
  - flop: 2 per arm, with one exception. pfs f32 to 0.05% takes 12 min, which is over the ~10-min limit, so it ran once; the second rep stopped at 0.1%. pfs i16 on the flop was stopped at 0.1% both times.
- Iteration counts are deterministic: every rep of a given arm stopped at the same iteration.

## Game definitions (verified equal)

The spot in every game: 6-max, 100 BB, `BTN r2.5, BB c`. Pot 5.5 BB, effective stack 97.5 BB, BB is OOP. Board Ks 7h 2d, turn 3c, river 8d. Ranges are exactly the strings given in the task. There is no rake; the comparison is zero-sum.

How the size menus were made identical:
- **Chip units.** pfs chips are set to **milli-BB** (pot 5500, stack 97500), not 1 BB = 100 chips. P1's internal grid is 0.001 BB (`Chips(1000)` = 1 BB). Both round pot-relative sizes as `f64::round(pot_after_call × fraction)` added to the amount already called. With the same unit, every amount matches exactly; 1 BB = 100 would have caused 0.005 BB rounding differences.
- **All-in options.** All-in is an explicit `a` in both size lists. P1 has no `include_allin` and no `allin_threshold`. pfs has `add_allin_threshold = 0`, `force_allin_threshold = 0` and `merging_threshold = 0`. With those settings, a size that is clamped to the stack becomes the all-in in both solvers and is deduplicated.
- **Raise caps.** pfs has no per-street raise caps, so P1's `max_aggressive_actions` are all set to 100. Raise wars end at all-in in both solvers. For example, the river raise sequence is 1.815 → 5.445 → 16.335 → 49.005 → all-in.
- **Bet sizing rules.**
  - The minimum raise is the previous full raise increment in both.
  - pfs's minimum bet is 1 chip and P1's is 1 BB, but no generated bet is below 1.8 BB, so this never matters.
  - The pfs donk options are `None`, so the default bet sizes apply.
  - Suit isomorphism plays no part: the board is rainbow and every turn card leaves no two suits interchangeable.

### P1 configs (`runs/pfs-compare/configs/{river,turn,flop}.toml`)

The common part:

```toml
schema = "solvers.nlh/v1"

[table]
players = 6
stack_bb = 100

[economics]
kind = "cash"

[spot]
line = "<per game>"
board = "<per game>"

[ranges]
BTN = "22+,A2s+,K2s+,Q5s+,J7s+,T7s+,97s+,86s+,75s+,65s,54s,A5o+,K9o+,Q9o+,J9o+,T9o"
BB = "99-22,AJs-A2s,KJs-K2s,Q4s+,J6s+,T6s+,96s+,85s+,74s+,64s+,53s+,43s,AJo-A2o,K8o+,Q9o+,J9o+,T8o+,98o"

[tree]
script = '''<per game>'''

[tree.max_aggressive_actions]
flop = 100
turn = 100
river = 100

[solver]
storage = "f32"

[solver.algorithm]
schedule = "dcfr"

[solver.stop]
target = "0.05%pot"
max_iterations = 100000
check_every = 10

[run]
checkpoint_interval = "10h"
final_checkpoint = false
```

| game | `line` | `board` | `script` |
|---|---|---|---|
| river | `BTN r2.5, BB c / BB x, BTN x / BB x, BTN x` | `Ks 7h 2d 3c 8d` | `river { replace bet [33, 75, a]` / `replace raise [3x, a] }` |
| turn | `BTN r2.5, BB c / BB x, BTN x` | `Ks 7h 2d 3c` | `turn, river { replace bet [33, 75, a]` / `replace raise [3x, a] }` |
| flop | `BTN r2.5, BB c` | `Ks 7h 2d` | `flop { replace bet [33, 75]` / `replace raise [a] }` and `turn, river { replace bet [75]` / `replace raise [a] }` |

(In the `script` column, `/` marks a line break inside the script.)

The flop game uses a smaller menu on purpose. The full 33/75/a + 3x/a menu on all three streets needs 47 GB in P1, which does not fit. The chosen flop game needs 1.8 GB.

### pfs config (harness `src/main.rs`)

```rust
CardConfig {
    range: [BB.parse()?, BTN.parse()?],          // [OOP, IP], same strings as above
    flop: flop_from_str("Ks7h2d")?,
    turn: /* river, turn games */ card_from_str("3c")?   /* flop game: NOT_DEALT */,
    river: /* river game */ card_from_str("8d")?         /* else NOT_DEALT */,
}
TreeConfig {
    initial_state: River | Turn | Flop,
    starting_pot: 5500, effective_stack: 97500,      // milli-BB
    rake_rate: 0.0, rake_cap: 0.0,
    // river game: river ("33%, 75%, a", "3x, a")
    // turn game:  turn & river ("33%, 75%, a", "3x, a")
    // flop game:  flop ("33%, 75%", "a"); turn & river ("75%", "a")
    // the same BetSizeOptions for OOP and IP; streets that are not played get empty options
    turn_donk_sizes: None, river_donk_sizes: None,
    add_allin_threshold: 0.0, force_allin_threshold: 0.0, merging_threshold: 0.0,
}
```

### Equality check

Both trees were dumped as rows of history (P1 history syntax, amounts in BB, cumulative from the spot start), street, actor, pot and the full action list with amounts.
- P1: `solvers export <sol> tree --node all --format csv`.
- pfs: the harness walks the `ActionTree`.

For P1, turn and river cards were replaced by a wildcard. The per-card rows were identical within each line, so they collapse to one abstract row.

| game | decision nodes P1 / pfs | abstract decision rows P1 / pfs | rows that differ | terminal nodes P1 / pfs harness count |
|---|---|---|---|---|
| river | 32 / 32 | 32 / 32 | 0 | 61 / 61 |
| turn | 15,008 / 15,008 | 344 / 344 | 0 | 28,590 / 27,838 |
| flop | 213,160 / 213,160 | 130 / 130 | 0 | 351,436 / 319,002 |

**Root menu**, identical in all three games: OOP `check | bet 1.815 | bet 4.125`, plus `| bet 97.5` on the river and turn games.

**The terminal-count difference is only how all-in runouts are counted.**
- P1 counts a called all-in before the river as a chance node plus one terminal per runout.
- My pfs tree walk counts each called all-in line once, with no chance node. Inside the built `PostFlopGame`, pfs also deals the remaining cards in these lines.

The differences are fully accounted for:
- Turn game: 16 abstract turn all-in-call lines. Each adds 1 chance node and 47 terminals. 16 × 47 = 752 = 28,590 − 27,838. Chance nodes: 15 + 16 = 31 (P1 total nodes 43,629 = 15,008 + 28,590 + 31).
- Flop game: 4 abstract flop all-in-call lines, each adding 50 chance nodes and 2,351 terminals. 10 turn all-in-call lines, each on 49 turn cards, adding 1 chance node and 47 terminals per card. This gives 4·50 + 490 = 690 extra chance nodes and 4·2,351 + 490·47 = 32,434 extra terminals, which are exactly the observed differences.
- Hands after board removal (pfs): 446/417 (river), 469/431 (turn), 479/436 (flop) OOP/IP.

**Converged EVs and root strategy** (EV in BB from spot start; the targets are 0.00275 BB NashConv/2):

| game | P1 EV OOP / IP | pfs f32 EV OOP / IP | P1 root | pfs f32 root |
|---|---|---|---|---|
| river | 2.2512 / 3.2488 | 2.2511 / 3.2489 | x .710, b1.815 .290, b4.125 .0002, AI 0 | x .720, b1.815 .280, b4.125 0, AI 0 |
| turn | 2.0500 / 3.4500 | 2.0501 / 3.4499 | x .9999, b1.815 .0001 | x 1.000 |
| flop | 1.7274 / 3.7726 | 1.7271 / 3.7729 (1,820 it) | x .858, b1.815 .140, b4.125 .0016 | x .846, b1.815 .152, b4.125 .0021 |

The EVs agree to within 0.0003 BB, which is about 10× smaller than the exploitability target.

On the flop, the split between check and small bet at the root is nearly indifferent and drifts while solving: pfs at 670 iterations had x .654 / b .332. So the root frequencies only agree to about ±0.01 there.

## Results (8 threads, medians)

Columns:
- **t excl** = time excluding evaluation.
- **t incl** = time including evaluations every 10 iterations.
- **eval** = cost of one evaluation.
- **peak** = peak working set.

### River (3 reps)

| solver | it to 0.3 / 0.1 / 0.05 % | t excl (s) | t incl (s) | ms / iter | eval | peak |
|---|---|---|---|---|---|---|
| P1 f32 | 200 / 440 / 610 | 0.039 / 0.081 / 0.111 | 0.044 / 0.092 / 0.126 | 0.185 | ~0.25 ms | 7.7 MiB |
| pfs f32 | 310 / 430 / 820 | 0.096 / 0.131 / 0.249 | 0.106 / 0.143 / 0.273 | 0.303 | 0.29 ms | 6.1 MiB |
| pfs i16 | 280 / 430 / 1030 | 0.096 / 0.140 / 0.331 | 0.104 / 0.152 / 0.360 | 0.321 | 0.29 ms | 5.9 MiB |

pfs does no parallel work on a river-start game, so it runs single-threaded here (see design note 4). These runs are so short that times are dominated by overhead.

### Turn (4 reps; rep 1 had heavy load)

| solver | it to 0.3 / 0.1 / 0.05 % | t excl (s) | t incl (s) | ms / iter | eval | peak |
|---|---|---|---|---|---|---|
| P1 f32 | 320 / 570 / 820 | 6.67 / 11.73 / 16.59 | 8.14 / 14.35 / 20.37 | 20.34 | 46 ms | 237 MiB |
| pfs f32 | 370 / 760 / 1320 | 9.64 / 19.95 / 34.81 | 10.46 / 21.67 / 37.78 | 26.37 | 22.6 ms | 157 MiB |
| pfs i16 | 370 / 1030 / 1320 | 9.51 / 26.89 / 34.42 | 10.33 / 29.20 / 37.35 | 26.07 | 22.3 ms | 86 MiB |

### Flop (2 reps; pfs f32 0.05% n = 1; pfs i16 stopped at 0.1%)

| solver | it to 0.3 / 0.1 / 0.05 % | t excl (s) | t incl (s) | ms / iter | eval | peak |
|---|---|---|---|---|---|---|
| P1 f32 | 170 / 350 / 660 | 48.4 / 98.9 / 186.4 | 56.9 / 116.3 / 219.2 | 282.4 | 0.497 s | 1,968 MiB |
| pfs f32 | 220 / 670 / 1820 | 79.8 / 245.0 / 666.5 | 86.8 / 266.6 / 724.5 | 366.0 | 0.321 s | 1,847 MiB |
| pfs i16 | 300 / 1110 / — | 108.0 / 403.4 / — | 117.4 / 438.7 / — | 363.4 | 0.318 s | 948 MiB |

Storage estimates for comparison:
- pfs `memory_usage()` f32 / i16: 0.46 / 0.31 MiB (river), 149 / 76 MiB (turn), 1,838 / 939 MiB (flop).
- P1 `validate --resources` f32Bytes: 0.3 MiB (river), 145 MiB (turn), 1,736 MiB (flop).

### Ratios (P1 relative to pfs f32)

| | river | turn | flop |
|---|---|---|---|
| iterations to 0.05% | 0.74× | 0.62× | 0.36× |
| iterations to 0.1% | 1.02× | 0.75× | 0.52× |
| time per iteration (8 threads) | 0.61× | 0.77× | 0.77× |
| time to 0.05%, excluding evaluation | 0.44× | 0.48× | 0.28× |
| time to 0.05%, including evaluation | 0.46× | 0.54× | 0.30× |
| peak memory | 1.28× | 1.51× | 1.07× |

### Single thread and scaling (fixed iteration count, 2 reps each)

The P1 per-iteration time is the difference between a 2N-iteration run and an N-iteration run, each with one evaluation, so the evaluations cancel.

| | P1 1 thread | pfs 1 thread | P1 speedup at 8 threads | pfs speedup at 8 threads |
|---|---|---|---|---|
| turn (N = 100) | 92.2 ms (92.7 / 90.8) | 133.8 ms | 4.53× | 5.07× |
| flop (N = 20) | 1.325 s (1.290 / 1.360) | 1.829 s | 4.69× | 5.00× |

The pfs build with the nightly `custom-alloc` bump allocator measured 26.8 / 27.1 ms on the turn, 365 / 369 ms on the flop and 0.30 / 0.32 ms on the river. That equals the default build, so per-node heap allocation is not a significant pfs cost.

### Schedule test: P1 with pfs's schedule

P1 was run with `alpha = 1.5, beta = 0, gamma = 3, pow4_reset = true`. In P1's formula, `beta = 0` gives a constant 0.5 factor on negative regrets, which matches pfs (see design note 1). These are single runs; the iteration counts are deterministic.

| iterations to 0.3 / 0.1 / 0.05 % | river | turn | flop |
|---|---|---|---|
| P1 default (1.25 / 0.5 / 4, no reset) | 200 / 440 / 610 | 320 / 570 / 820 | 170 / 350 / 660 |
| P1 with pfs schedule | 310 / 540 / 940 | 360 / 670 / 1250 | 180 / 450 / 1430 |
| P1 with pfs coefficients, no reset | 270 / 540 / 940 | 350 / 670 / 1040 | 180 / 460 / 1680 |
| pfs | 310 / 430 / 820 | 370 / 760 / 1320 | 220 / 670 / 1820 |

### P1 low-memory storage modes, for comparison (single runs)

| | iterations to 0.3 / 0.1 / 0.05 % | t incl at last target | peak |
|---|---|---|---|
| turn, P1 i16 | 370 / 1060 / 1160 | 38.7 s | 172 MiB |
| turn, P1 i16-f32avg | 350 / 610 / 860 | 26.4 s | 203 MiB |
| turn, pfs i16 | 370 / 1030 / 1320 | 37.4 s | 86 MiB |
| flop, P1 i16 (target 0.1%) | 200 / 360 / — | 171.9 s | 1,099 MiB |
| flop, P1 i16-f32avg (target 0.1%) | 200 / 350 / — | 151.1 s | 1,538 MiB |
| flop, pfs i16 (target 0.1%) | 300 / 1110 / — | 438.7 s | 948 MiB |

## Analysis

**Iterations: no headroom against pfs.** P1 needs fewer iterations at every target except the river 0.1% point, where 440 vs 430 is a tie. The gap grows with tree size and with tighter targets. When P1 uses pfs's schedule, its iteration counts land close to pfs's (turn 1250 vs 1320, flop 1430 vs 1820, river 940 vs 820), and the turn curves nearly overlap. So the gap comes from the schedule:
- pfs discounts positive regrets with alpha 1.5 instead of 1.25.
- It halves negative regrets every iteration, where P1 uses s^0.5/(s^0.5+1), which tends to 1.
- It weights the average with gamma 3 instead of 4.
- It resets the average at powers of 4.

The reset alone causes large spikes in pfs:
- flop: 0.33% at iteration 200 → 5.36% at 250 → 6.38% at 260; 0.071% at 1020 → 0.148% at 1030.
- turn: 0.062% at 1020 → 0.088% at 1030.

The flop spike at 1024 cost about 800 extra iterations to get back to 0.05%.

The remaining ±15–25% differences between "P1 with pfs schedule" and pfs go both ways, so there is no consistent residual effect. They plausibly come from three sources:
- pfs averages strategies without reach weighting (design note 2).
- pfs's alpha index is shifted by one (it uses t−1 with t counted from 0).
- Exploitability curves are non-monotone near the threshold, so when a 10-iteration check happens to land matters (P1 with pfs schedule had a 0.46% spike at iteration 800 on the flop).

There is nothing in pfs's iteration behaviour for P1 to adopt.

**Per-iteration cost: P1 is ahead; small headroom in thread scaling.**
- On one thread, P1's kernel is 1.38–1.46× faster.
- At 8 threads, pfs scales slightly better: 5.0–5.1× vs P1's 4.5–4.7×. That shrinks P1's lead to 1.30× on turn and flop.
- If P1 matched pfs's scaling, it would gain roughly 8–12% per iteration at 8 threads. [INFERENCE from the two speedup ratios]

**Evaluation cost: modest headroom.**
- P1's exact f64 evaluation costs 46 ms on the turn (2.3 iterations) and 0.50 s on the flop (1.8 iterations).
- pfs's f32 evaluation costs 0.86–0.88 of one of its iterations.
- With `check_every = 10`, evaluation takes 15% (flop) to 18% (turn) of P1's wall time, and 8% of pfs's.
- P1's default `check_every = "auto"` already evaluates less often. Making P1's evaluation as cheap relative to an iteration as pfs's would cut roughly 8–11% of wall time at this check interval. [INFERENCE]

**Memory.**
- P1 f32 peak minus P1 f32 storage: about 92 MiB on the turn (237 vs 145 MiB) and about 232 MiB on the flop (1,968 vs 1,736 MiB).
- pfs peak is about its storage plus 8 MiB. Its storage is about 6% larger than P1's, because it also keeps per-hand cfvalue arrays for IP and at chance nodes.
- Headroom 1: P1's fixed overhead outside storage — tree, tables, scratch, build transients. [INFERENCE: not broken down]
- Headroom 2: memory-saving mode. pfs i16 halves memory (948 vs 1,847 MiB on the flop) and still reached 0.05% on river and turn. On the flop it needs 1.66× more iterations to 0.1%.
- P1's i16 is 16% larger than pfs i16 on the flop (1,099 vs 948 MiB) and 2× larger on the turn (172 vs 86 MiB, mostly fixed overhead), but needs 3× fewer iterations to 0.1% on the flop.

## Notes on pfs design (read-only study, in my own words)

1. **DCFR as implemented** (`solver.rs`, `DiscountParams`). `t` is the 0-based iteration index passed to `solve_step`.
   - Positive cumulative regrets are multiplied by (t−1)^1.5 / ((t−1)^1.5 + 1), with t−1 floored at 0.
   - Negative cumulative regrets are multiplied by a constant 0.5. The sign is taken from the old cumulative value.
   - The cumulative strategy is multiplied by (t'/(t'+1))^3, where t' = t − (largest power of 4 ≤ t). That factor is 0 at t = 1, 4, 16, 64, 256, 1024, … (and at t = 0), so the average strategy restarts from the current strategy at each of those iterations.
   - The new regret is the action cfv minus the node cfv. It is added after the discount, so this is plain DCFR, not DCFR+ or a predictive variant.
   - Updates alternate: one traversal for OOP, then one for IP, per iteration. This is the same as P1.
2. **Averaging.**
   - The cumulative strategy adds the regret-matched current strategy per hand, *without* weighting by the player's own reach probability. The traversal only carries the opponent's reach.
   - The final strategy is the cumulative strategy normalized per hand, uniform if all zero.
   - P1 accumulates reach-weighted strategies.
3. **Exploitability.**
   - A best-response traversal in f32, with f64 only for sums over chance children.
   - It reuses the same terminal kernels as CFR, which is why it costs less than one iteration.
   - P1's evaluation is exact f64.
4. **Parallelism.**
   - pfs uses rayon `par_iter` over the children of every node *above the river* (decision and chance nodes alike, wherever the river card is not yet dealt).
   - River subtrees run sequentially.
   - So a river-start game gets no parallelism. Turn and flop games get coarse-grained parallelism with a lot of slack, which may explain the better 8-thread scaling.
5. **Terminal evaluation.**
   - One terminal at a time.
   - Fold: inclusion–exclusion over 52 card buckets.
   - Showdown, no rake: a two-pass sweep over hands sorted by strength (win pass, then lose pass). Accumulators are f64; opponent hands with zero reach are skipped inside the sweep.
   - No fusion of sibling terminals, no SIMD lanes, no pruning of zero-reach subtrees.
   - P1's fused, lane-batched f32 kernels and pruning plausibly explain most of its 1.4× single-thread advantage. [INFERENCE]
6. **Memory layout.**
   - Flat arenas: `storage1` (cumulative strategy), `storage2` (regrets), `storage_ip` (IP cfvalues), `storage_chance` (chance cfvalues).
   - Each node holds raw pointers into these arenas. Node structs are compact. There is almost nothing else at runtime.
   - Scratch vectors are allocated on the heap at each node visit unless `custom-alloc` (nightly bump allocator) is on. That made no difference here.
7. **i16 compression.**
   - Regrets are stored as i16 and the cumulative strategy as u16, each with one f32 scale per node.
   - Every update decodes, applies the discount, re-encodes and rescales to the maximum magnitude.
   - Combined with the power-of-4 average reset, this still reaches 0.05% on the river and turn games.

## Raw data

All under `runs/pfs-compare/` (gitignored):
- `configs/` — the P1 configs.
- `tree_{river,turn,flop}/` — P1 `tree_all.csv` and pfs `pfs_tree.csv` dumps.
- `{game}/p1_A*` — P1 runs to target; `{game}/p1_D*` — P1 runs at 50-iteration checks.
- `{game}/p1_pfsched*`, `{game}/p1_pfsched_noreset1` — the schedule test.
- `{game}/p1_i16*` — P1 low-memory storage runs.
- `{game}/p1_st_*` — single-thread runs.
- `{game}/pfs_*.jsonl`, `{game}/pfsCA_*.jsonl` — pfs runs; the matching `.stderr` file records wall time, other processes' CPU load and peak working set.
- `summary.json`.

The harness is in `.cache/pfs-bench/harness`; pfs is cloned to `.cache/pfs-bench/postflop-solver`. No tracked repository file was modified (`git status` shows only two untracked example files, which I did not create).
