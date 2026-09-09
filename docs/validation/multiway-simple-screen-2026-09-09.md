# Simple Preflop algorithm screen (2026-09-09)

This is a separate research screen for the GTOW Wizard `Cash6m50zSimple`
reference. It must not be merged with the General 6-max partial-tree
measurements in round5. The reference contains five unopened decisions with
169 hand classes each (845 class rows total). The observed menu inventory is
five opening menus, fifteen no-caller response menus, and one SB-versus-BB
21bb/4bet menu (21 observed menus in total). The 21-menu count describes the
captured UI/fixture contract; it is not a claim that every later game node is
matched.

The reference artifact is
`docs/validation/gtowizard-simple-preflop-2026-09-09.json`, SHA-256
`606b8bfd5bf78d877606b53ecbdd318f8fc361aed021e85365f8983dbf2af151`.
It records the five opener class tables, with physical combo weights 6 for
pairs, 4 for suited classes, and 12 for offsuit classes. The captured weighted
raise-combo totals are UTG 262.05, HJ 324.17, CO 389.19, BTN 558.17, and SB
569.64; the corresponding nonzero physical-combo counts are 330, 406, 450,
618, and 626. These are reference observations, not training inputs.

The fixture is
`examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml`, SHA-256
`57279091cd769b2605336d53fc1cddd29a812ae22d9ff1a55b8955d29ec3e126`.
It uses six seats, 100bb stacks, 5% rake capped at 4bb, and `allow_limp=false`.
The five opener menus use 2bb/2bb/2.3bb/2.5bb/3bb for
UTG/HJ/CO/BTN/SB. The fixture includes the captured no-caller branches and an
explicit bounded postflop approximation. The latter and unobserved later
preflop branches are model limitations, so this is not a full GTOW tree.

The prepared plan is
`runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/manifest.json`,
SHA-256
`243fc00457d87e92e92257e87cccadd377e912f1f8badf65012c5f7f865e5362`.
It uses the frozen research source archive
`e6441362b61f101f3f8b8d5865f0529cccc1918ec01b6e7fb3c3de1c87c4efce` and the
research binary SHA-256
`b1674011a6c338f0b1bb4e33e31c0fa490bbab88b1e560f8bec9c3c7b73c0769`.
Each arm uses seed 0, 32,768 sweeps, eight solver threads, 8GiB, held-out
evaluation seeds 101 and 202 with 2,048 samples, and an 1,800-second external
timeout. Arms run sequentially in independent output directories:

| arm | batch sweeps | discount | config SHA-256 |
|---|---:|---|---|
| `seed0000-none-b4` | 4 | none | `5204ddfdc5f99b83599d1656ba215ee376884d085d5b8263071c47cb65b19bc3` |
| `seed0000-periodic10000-b4` | 4 | periodic, every_sweeps=10000, until_sweeps=32769 (exclusive; events 10000/20000/30000) | `17c3c88220c2870304365968768475ad9a6fcb47cefa5bc8e772c8368c67f258` |
| `seed0000-none-b1` | 1 | none | `394f728ef4f6842e3290a0f04b7605c34eb99c64dc4ad17b8b23040d26393555` |

The manifest was preflighted before execution. Parsed `tree.rules`,
`allow_limp=false`, and all non-algorithm fixture fields match across arms.
The driver was updated so old General manifests check `limp_raise_size` only
when declared, while Simple manifests check `allow_limp` when declared. The
General and Simple preflight tests cover both contracts. All three arms have
now completed successfully; their execution records and summary are retained
under the plan directory.

The completed records are in
`runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/summary.json`.
The baseline none-b4 arm reports wall time 255.514970 seconds, timed-region
time 191.613483 seconds, mean position combo-weighted MAE 0.14039132
(14.0391 percentage points), and pooled weighted RMSE 0.26083809. The
periodic10000-b4 arm reports wall time 261.791735 seconds (1.0246 times the
baseline), timed-region time 196.506022 seconds, mean MAE 0.13150562
(13.1506 percentage points, 6.33% below baseline), and pooled weighted RMSE
0.25763909. Its fixed-candidate held-out maximum gain means were 0.702661
bb/hand (seed 101) and 0.513762 bb/hand (seed 202), versus 0.322994 and
0.279600 for baseline; this does not show a consistent improvement.
The none-b1 arm reports wall time 412.924532 seconds (1.6160 times baseline),
timed-region time 347.008241 seconds, mean MAE 0.12100345 (12.1003
percentage points, 13.81% below baseline), and pooled weighted RMSE 0.25311502.
Its held-out maximum gain means were 0.387986 and 0.394856 bb/hand for seeds
101 and 202. Thus b1 improved this single-seed MAE while costing substantially
more wall time, and candidate-gain superiority was not established. These
comparisons are descriptive within one partial model.

The three-arm result table is:

| arm | status | wall s | timed s | mean weighted MAE | pooled weighted RMSE | held-out max gain means (101 / 202) |
|---|---|---:|---:|---:|---:|---:|
| none-b4 | complete | 255.514970 | 191.613483 | 0.14039132 | 0.26083809 | 0.322994 / 0.279600 |
| periodic10000-b4 | complete | 261.791735 | 196.506022 | 0.13150562 | 0.25763909 | 0.702661 / 0.513762 |
| none-b1 | complete | 412.924532 | 347.008241 | 0.12100345 | 0.25311502 | 0.387986 / 0.394856 |

Resource monitoring used 15-second target-process samples on a host with
16 logical processors and eight configured solver threads. The summary is:

| arm | samples | observed s | mean busy cores | host CPU % / 8-thread capacity % | max RSS GB | min available RAM GB |
|---|---:|---:|---:|---:|---:|---:|
| none-b4 | 16 | 225.338 | 4.972 | 31.074 / 62.148 | 1.402 | 10.865 |
| periodic10000-b4 | 17 | 240.327 | 4.671 | 29.192 / 58.383 | 1.402 | 10.825 |
| none-b1 | 28 | 405.501 | 2.718 | 16.986 / 33.972 | 1.401 | 10.852 |

These are sampled-window observations; they do not estimate unsampled startup
or finalization and do not justify a quality ranking. Full byte values and
sampling metadata are retained in the summary JSON and
`runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/host-observation.json`.

The wall and timed columns are seconds. The two held-out values are
maximum per-seat candidate-gain means in bb/hand for evaluation seeds 101 and
202; their confidence bounds remain in the summary JSON.

The timed region includes solving, strategy materialization, fixed-candidate
held-out evaluations, and solver disposal; the difference from wrapper wall
time also contains startup, construction, and output overhead. These values
are descriptive for the partial fixture and the single training seed.

The Python comparison tool checks the Rust class order by bucket index,
position/player identity, outer/key history identity, active-opponent count,
preflop bucket tail, row-specific action menus, normalized probabilities, and
state4/schema. Its weighted MAE/RMSE uses the physical 6/4/12 class-combo
weights. Eight comparison tests and ten algorithm-screen driver tests passed;
the Simple test includes distinct bucket values and position-specific
2bb/2bb/2.3bb/2.5bb/3bb mapping. The source/test evidence is diagnostic only.

The K128 sensitivity completed under
`runs/multiway-convergence-round5-20260909/local-simple-k128-screen/`.
It is a one-arm `seed0000-none-b4` run, keeping the completed K32 baseline's
fixture, seed, batch, discount, sweep, resource, and evaluation settings while
changing only flop/turn/river buckets from 32 to 128. The K128 manifest SHA-256
is `98553f2fa0c408ef8d8268b111f69d6d8369ce5f00af174993e22bbdaf44c116` and
the derived config SHA-256 is
`615b5b4a3cb97105d0a46e0f3cd3196d954718ba0a5ad9969a3e7e2dcd4cc1f6`.
The run started at `2026-09-09T06:12:45.136341Z`, reached 32,768 sweeps, and
returned successfully. Its wall time was 327.277028 seconds and timed-region
time 201.610879 seconds; mean combo-weighted MAE was 0.14106832 (14.1068
percentage points) and pooled weighted RMSE was 0.27473440. Relative to K32
none-b4, MAE increased 0.48% and RMSE increased 5.33%; timed-region time was
1.052 times the baseline. Raw wall time was 1.281 times the baseline, but
includes about 58.68 seconds of cold K128 EHS2 construction while K32 loaded a
warmed cache in about 0.64 seconds, so it is not a clean speed comparison.
The held-out maximum-gain means were 0.566790 and 0.281348 bb/hand for seeds
101 and 202, respectively; this single seed does not establish candidate-gain
superiority or a convergence improvement. The sampled K128 resource window
had 20 samples over 285.326 seconds, maximum working set 2.932 GB, sampled
peak 3.099 GB, and minimum available RAM 8.961 GB.

The paired-seed b1 reproducibility check completed at training seeds 11 and 29,
with b4 paired at each seed. The two independent driver processes started at
approximately `2026-09-09T06:32:16Z`; each arm used six solver threads, for 12
configured threads across the two processes on the shared 16-logical-CPU host.
The read-only monitors started after the solver processes, so startup and any
pre-monitor resource use are unsampled. The aggregate records are in
`runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/summary.json`.

Across seeds 0, 11, and 29, mean MAE was 0.12565914 for b4 and 0.12403591 for
b1, a paired difference of -0.00162322 (-0.1623 percentage points); the sample
standard deviation of the three paired differences was 0.02355558 (2.3556
percentage points), and b1 was lower in 2 of 3 seeds. This is weak evidence
for a seed-dependent tradeoff, not a reproducibility or default-promotion
result. The within-pair b1/b4 wall ratios were 1.6160 (seed 0), 1.3968 (seed
11), and 1.4139 (seed 29). Because seed 11 and 29 processes shared the host,
these timing ratios are descriptive only. Held-out fixed-candidate evaluations
are supplemental diagnostics and are not exploitability estimates.

The four new completed arm output hashes are: seed0011 b4
`66182bbf47e1f2adfd884a98248bccc9b14102f2dd8f9c10b2214533acef07a9`, seed0011
b1 `863c044b22e2a045e928c0bf3d09eae7cd6d4d540e1f767ab003b8f546cfe1e8`,
seed0029 b4 `7a4da523d6513001b37ed96bef8c351372f50ff34165f39576e596987ad81d12`,
and seed0029 b1 `c6f29c16d65a114a020bd0aaff9d94bf5e91ed06e5f70819412f7713abd67943`.
Their per-seed manifests are `fa119df44298692dc8c9ea3b24e7f8d17672ecb684194c4e7601e14e88ab41e0`
and `e7bdbabca34cb6b5f9b120dd16b72190ea8fd0009e2f0f0f6a2d2affa4aaecf5`.
No further b1 calibration, equal-wall comparison, or long run is planned now;
the next investigation is representation/model loss as described in the
abstraction-next note.

The reference metadata also records two prospective validation nodes:
SB open 3bb followed by BB responses, and BB 3bet 10bb followed by SB responses.
Their aggregate frequencies are not part of the five-unopened-node screen.
The broad library description mentions no 4bet all-in, but the observed SB
3bb to BB 10bb menu contains a 21bb raise and a 100bb all-in at 3.9%; the
fixture therefore preserves the observed menu rather than applying that
description uniformly. This remains a UI observation and a future validation
target, not an executed Simple comparison result.

This screen produces normalized research strategy rows and the fixed-candidate
held-out ProfileEvaluation. It does not produce checkpoint or `.mwsol`
artifacts, a trained deviator/best response, physical node-frequency estimates,
a Nash exploitability bound, or a convergence certificate. A single seed
cannot promote a default. The initial K32/K128 screens used eight solver
threads, while the paired seed-11/29 screen used six; the observed host has
16 logical processors. Host utilization and configured worker utilization must
remain separately labeled. The Simple results must
also remain separate from the General model because menus, rake/tree
coverage, and later-node equivalence differ.
