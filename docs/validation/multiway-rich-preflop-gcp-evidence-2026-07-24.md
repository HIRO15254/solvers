# Canonical benchmark and historical GCP resource evidence (2026-07-23--24)

> **注記(2026-08)**: 本書が参照する `experiments/` 配下の設定・スクリプト・CSVは、
> 抽象化最適化の研究ラインを打ち切った際に削除した。以下のpath表記は当時の
> repository内位置であり、実体はGit履歴(tag `pre-gui-removal` 以前)から取得する。

This note supplements `multiway-abstraction-parameters-2026-07-23.md`. The
current results use the canonical benchmark Tree and a dense-arena byte
boundary. The former rich-preflop/one-size and GCP/checkdown results remain
below only as historical reproduction evidence. None of these short runs is
a convergence or equilibrium-quality result.
The card-only rollout, bucket, and k-means experiments are Tree-independent
and were not rerun.

## Current canonical benchmark Tree

The tracked canonical v1 configurations are:

- `experiments/abstraction-2026-07-23/tournament-6max-50bb-benchmark-v1.toml`;
- `experiments/abstraction-2026-07-23/cash-6max-100bb-benchmark-v1.toml`.

Tournament forbids limps, opens to 2bb or all-in, uses 2.5x or all-in at the
3bet and 2x or all-in at 4bet+, permits at most two non-BB callers of the
open, and excludes BB defence from that call cap. Cash forbids limps and open
jams, opens only to 2.5bb, uses 3x IP or 5x OOP relative to the immediately
preceding raiser plus all-in at 3bet+, and permits open calls only from
BTN/SB/BB. A Cash normal re-raise strictly above one third of the actor's
hand-start stack merges into all-in; at exact equality the normal target and
explicit all-in remain distinct. In both cases a first-time participant cannot
call after a 3bet, while a seat that previously called or raised may.

Both use at most six preflop aggressive actions. Every postflop street offers
one 50%-pot normal bet, one 2.5x-previous-bet normal raise, and a legal,
distinct all-in, with at most four aggressive actions and no player-count
checkdown.

### 6 GiB dense-arena byte preflight

`action_tree_preflight` defaults to the `benchmark` profile. The 44-case
source of truth is
`experiments/abstraction-2026-07-23/action-tree-benchmark-6gib-2026-07-24.csv`.
Each case ran in its own local process with no explicit node checkpoint and a
6 GiB (6,442,450,944 B) dense-arena payload limit.

| case | seats | completed tested depths | deepest complete: exact nodes / arena bytes | first MemoryLimit depth | first exceeding prefix N / columns / bytes |
|---|---:|---|---:|---:|---:|
| Tournament | 6 | 5/10/15/20/30/40bb | 1,317,762 / 5,824,769,232 | 50bb | 1,461,386 / 369,829,892 / 6,442,451,320 |
| Tournament | 7 | 5/10/15/20/30bb | 1,468,236 / 6,351,266,936 | 40bb | 1,455,102 / 370,319,976 / 6,442,451,248 |
| Tournament | 8 | 5/10/15/20bb | 776,429 / 3,349,725,256 | 30bb | 1,486,728 / 374,466,084 / 6,442,452,472 |
| Tournament | 9 | 5/10/15/20bb | 1,323,576 / 5,684,701,552 | 30bb | 1,487,741 / 374,724,542 / 6,442,454,048 |
| Cash | 6 | 100bb | 890,319 / 4,097,314,184 | 200bb | 1,359,230 / 347,911,028 / 6,442,452,024 |
| Cash | 7 | 100bb | 1,182,894 / 5,441,603,608 | 200bb | 1,359,231 / 347,911,197 / 6,442,454,776 |
| Cash | 8 | none | -- | 100bb | 1,399,020 / 358,055,856 / 6,442,452,264 |
| Cash | 9 | none | -- | 100bb | 1,399,021 / 358,056,025 / 6,442,455,016 |

Tournament completed 19/28 cases and Cash 2/16, for 21/44 complete Trees.
The remaining 23 results are byte-bound prefixes, not completed node counts:
prefix N is the first estimate above 6 GiB and N-1 nodes remain within the
configured arena limit. All deeper-stack rows and exact complete-case
columns are preserved in the CSV.

The count-only preflight retains neither the full public tree nor the dense
arena. Its peak RSS was 2,080,768--2,162,688 B and maximum wall time 0.519s,
which must not be interpreted as the RSS of an allocated dense solve.
`dense_arena_bytes` covers the two `f32` policy arrays, touched bitset, and
dense index tables. It excludes the materialized tree/history, abstraction
caches, worker scratch, evaluation, checkpoint staging, and allocator
overhead, so 6 GiB is not a full-process hard cap.

There is no fixed 50M-node production cap. `--node-limit` is an optional
benchmark checkpoint; `--node-limit 50000000` exists only to reproduce the
legacy results below.

### Sparse feasibility anchors

The source of truth is
`experiments/abstraction-2026-07-23/sparse-feasibility-100sweeps-2026-07-24.csv`.
Both runs use bucket-history/full recall, range-vector traversal, pruning
disabled, a 6 GiB solver accounting limit, 100 sweeps, and warm rollout
artifacts.

| case | sweeps / traversals | infosets | solver bytes | wall | process peak RSS |
|---|---:|---:|---:|---:|---:|
| Tournament 6-max/50bb, K256/R512 | 100 / 600 | 432,718 | 124,220,325 B | 11.63s | 397,492,224 B |
| Cash 6-max/100bb, K256/R2048 | 100 / 600 | 392,688 | 114,348,381 B | 23.02s | 371,589,120 B |

Both completed and each performed 399,600 hand updates. They establish
bounded sparse execution of the canonical Tree, not convergence or a
long-run memory bound. The current 44-case and sparse runs stayed local; no
GCP resource was started and the incremental cloud cost was USD 0.

## Historical rich-preflop/one-size Tree

Before the canonical preflop restrictions were implemented, the previous
rich-preflop Tree was combined with the same one-size postflop family.
Explicit `--node-limit 50000000` runs produced:

| case | historical outcome | enumerated decision nodes | elapsed | peak RSS |
|---|---|---:|---:|---:|
| Tournament 6-max/50bb | 50M checkpoint | 50,000,000 | 31.792s | 4.505630 GiB |
| Tournament 7-max/50bb | 50M checkpoint | 50,000,000 | 32.866s | 4.169632 GiB |
| Tournament 8-max/50bb | 50M checkpoint | 50,000,000 | 32.953s | 4.766449 GiB |
| Tournament 9-max/50bb | 50M checkpoint | 50,000,000 | 33.897s | 4.168686 GiB |
| Cash 6-max/100bb | 50M checkpoint | 50,000,000 | 34.535s | 4.702042 GiB |
| Cash 7-max/100bb | 50M checkpoint | 50,000,000 | 35.991s | 4.942764 GiB |
| Cash 8-max/100bb | 50M checkpoint | 50,000,000 | 34.248s | 4.574631 GiB |
| Cash 9-max/100bb | 50M checkpoint | 50,000,000 | 33.681s | 4.667252 GiB |

Historical endpoint runs were:

| case | outcome / nodes | dense arena | enumerate | peak RSS |
|---|---:|---:|---:|---:|
| Tournament 6-max/10bb | 1,899,688 | 7.308 GiB | 0.952s | 0.582 GiB |
| Tournament 7-max/10bb | 9,688,332 | 36.855 GiB | 4.405s | 2.972 GiB |
| Tournament 8-max/10bb | 48,801,984 | 184.051 GiB | 25.300s | 4.116 GiB |
| Tournament 9-max/10bb | >=50,000,000 / checkpoint | -- | 33.674s | 4.533 GiB |
| Cash 6-max/800bb | >=50,000,000 / checkpoint | -- | 41.526s | 4.334 GiB |
| Cash 7-max/800bb | >=50,000,000 / checkpoint | -- | 47.559s | 4.030 GiB |
| Cash 8-max/800bb | >=50,000,000 / checkpoint | -- | 43.646s | 5.161 GiB |
| Cash 9-max/800bb | >=50,000,000 / checkpoint | -- | 44.377s | 4.660 GiB |

These 50M rows are lower bounds on the old Tree only. They are not a
production policy and say nothing about the completed canonical Tree.

The old one-size sparse runs are also historical:

| case | sweeps / traversals | infosets | solver memory | wall | peak RSS |
|---|---:|---:|---:|---:|---:|
| Tournament 6-max/50bb, K256/R512 | 100 / 600 | 1,284,680 | 361,542,788 B | 15.20s | 1.255 GiB |
| Cash 6-max/100bb, K256/R2048 | 100 / 600 | 551,562 | 157,230,736 B | 49.85s | 0.707 GiB |

Everything after this section is the still older historical postflop-
checkdown baseline.

## Historical checkdown reproduction surface

- Source base revision: `a39b79fe53a6`, plus the uncommitted experiment
  harness/runtime changes listed in the main report.
- Tree harness:
  `crates/multiway/examples/action_tree_preflight.rs`.
- 6-max solve config:
  `experiments/abstraction-2026-07-23/tournament-6max-50bb-rich-preflop-gcp.toml`.
- VM: Compute Engine `e2-highmem-8`, 8 vCPU / 64 GiB,
  Ubuntu 24.04, Rust 1.97.1, `us-central1`.
- Tournament tree: equal 50bb stacks, 0.125bb per-seat ante, exact ICM for the
  solve, two open sizes, two isolation sizes, two re-raise multipliers, and
  all-in at every legal preflop decision.
- Cash tree: equal 100bb stacks, no ante, one open size, one isolation size,
  one re-raise multiplier, and all-in at every legal preflop decision.
- Both cases: at most four aggressive preflop actions and complete postflop
  checkdown. The count-only preflight uses 169 preflop and 256 postflop
  buckets; the postflop buckets are unreachable in these trees.
- One harness case ran per process. `arena bytes` is the solver's full
  current-street dense allocation estimate. `peak RSS` is GNU `time -v`
  maximum resident set size while enumerating and preflighting. These logs
  predate the current non-retaining byte-prefix preflight; the then-current
  harness enumerated first and deliberately rejected at a one-byte arena
  limit without allocating the dense regret/strategy buffers.

Representative commands:

```bash
cargo build --release -p multiway --example action_tree_preflight
/usr/bin/time -v target/release/examples/action_tree_preflight \
  --profile legacy-rich --case tournament --seats 6 --postflop checkdown \
  --max-memory-bytes 18446744073709551615
/usr/bin/time -v target/release/examples/action_tree_preflight \
  --profile legacy-rich --case cash --seats 9 --postflop checkdown \
  --node-limit 50000000 --max-memory-bytes 18446744073709551615
CARGO_TARGET_DIR=target/research-release \
  cargo build --release -p cli --features research --bin solvers
/usr/bin/time -v target/research-release/release/solvers solve \
  experiments/abstraction-2026-07-23/tournament-6max-50bb-rich-preflop-gcp.toml
```

## Historical checkdown public-tree and dense-arena census

| case | result | nodes | columns | arena bytes | arena GiB | enumerate | peak RSS |
|---|---|---:|---:|---:|---:|---:|---:|
| Tournament 6-max | measured | 2,625,052 | 443,633,788 | 7,763,100,168 | 7.229950 | 2.993s | 820,428 KiB |
| Tournament 7-max | measured | 21,013,626 | 3,551,302,794 | 61,033,714,368 | 56.842076 | 26.393s | 6,507,352 KiB |
| Tournament 8-max | 50M checkpoint | >=50,000,000 | -- | -- | -- | 64.092s | 14,075,060 KiB |
| Tournament 9-max | 50M checkpoint | >=50,000,000 | -- | -- | -- | 64.209s | 14,073,200 KiB |
| Cash 6-max | measured | 500,478 | 84,580,782 | 1,480,825,584 | 1.379126 | 0.528s | 162,568 KiB |
| Cash 7-max | measured | 4,113,606 | 695,199,414 | 11,954,366,000 | 11.133371 | 4.832s | 1,301,364 KiB |
| Cash 8-max | measured | 31,354,110 | 5,298,844,590 | 89,960,996,784 | 83.782707 | 45.447s | 9,907,700 KiB |
| Cash 9-max | 50M checkpoint | >=50,000,000 | -- | -- | -- | 77.142s | 13,807,404 KiB |

The checkpoint results are lower bounds, not estimates of the completed
historical checkdown tree. Card bucket counts do not change its public node
count. The explicit 50M stop was an experiment checkpoint, not a production
feasibility policy.

## Historical checkdown 6-max Tournament short solve

The rollout-kmeans K256/R512 artifact was generated from the same checkout
locally, copied to the VM, and accepted by the solve fingerprint check.
The current-street solve completed:

```text
sweeps=1000 traversals=6000 infosets=123898
regret_proxy=3.029e1 memory=7403MiB
status=completed
elapsed=13.56s
maximum resident set size=1,070,152 KiB
exit status=0
```

`memory=7403MiB` is the solver's dense-arena accounting. The Linux process RSS
after only 1,000 sweeps is much lower because most zero-initialized dense pages
have not yet been touched; this is not evidence that the current-street
backend is sparse. A longer solve can make more pages resident, and the
7.229950 GiB arena leaves insufficient process-level headroom for an 8 GiB
hard RSS guarantee.

## Historical GCP cost controls and cleanup

The current canonical benchmark did not use GCP and added USD 0. Everything
in this section concerns the earlier checkdown run.

- Dedicated project: `solvers-abstraction-20260723`.
- A project-filtered JPY 3,000 budget alert was created. Google Cloud budgets
  are monitoring alerts, not hard spending caps.
- Every VM was limited to a two-hour run duration. The initial standard VM
  used `DELETE`; after the user requested Spot, the standard VM was deleted.
- The first Spot VM used `DELETE` and was preempted during the build. The
  replacement Spot VM used `STOP` on preemption so its 30 GB boot disk could
  retain completed build outputs. It was restarted only to recover/finish the
  measurements.
- The logs were recovered before cleanup. The standard and Spot VMs, all boot
  disks, and the JPY 3,000 budget were deleted. The dedicated project remains
  empty because whole-project deletion was not approved; its billing account
  was unlinked and `billingEnabled: false` was verified.

Final readback, transcribed before the temporary gcloud credentials were
removed:

```text
billingAccountName: ''
billingEnabled: false
projectId: solvers-abstraction-20260723
compute instances list: empty
compute disks list: empty
```

Cloud Billing usage posting is delayed, so an exact invoice value was not
available at cleanup time. At the then-current official rates, a conservative
compute bound treats the one standard start and at most four Spot starts as
running their full two-hour limit:

```text
1 * 2h * $0.36159864 + 4 * 2h * $0.216976 = $2.45900528
```

Thus compute was below USD 2.50; 30 GB standard-disk retention, ephemeral
IPv4, and the sub-megabyte log transfers keep the total comfortably below
USD 3. The actual runtime was shorter. No billable resource remains attached
to the project.

Official control semantics:

- Budgets: <https://docs.cloud.google.com/billing/docs/how-to/budgets>
- Budget-notification latency:
  <https://docs.cloud.google.com/billing/docs/how-to/disable-billing-with-notifications>
- VM maximum run duration:
  <https://docs.cloud.google.com/compute/docs/instances/limit-vm-runtime>
- Spot behavior/pricing: <https://cloud.google.com/spot-vms/pricing>
