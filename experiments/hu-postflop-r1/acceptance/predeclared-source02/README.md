# R1 HU pipeline comparison

[R0 measurement protocol](../../../docs/plans/hu-postflop-r0/measurement-protocol.md) and
[R1 execution plan](../../../docs/plans/r1-execution-plan.jp.md) define the acceptance scope.
This campaign measures synthetic, finite, no-rake chip-EV games. These three cases are
**not** the 24 external reference fixtures and cannot certify their quality.

## Fixed inputs and pilot

| Case | Board | Remaining tree | Pilot cap/check cadence |
|---|---|---|---|
| [river](configs/river.toml) | 2c 7d 9h Js Qs | bet 50/100% pot, raise 100%, at most two aggressive actions | 400 / 25 |
| [turn](configs/turn.toml) | 2c 7d 9h Js | one 50% pot bet per street, no raises, all river runouts | 400 / 25 |
| [flop](configs/flop.toml) | Ks 7h 2d | one 50% pot flop bet, later streets check down, all runouts | 100 / 10 |

All use pot 20 chips, remaining stack 60 chips, DCFR, F32, 8 threads, no suit-isomorphism
merging, Full artifact storage, and the literal ranges in each file. Narrow Flop ranges
keep exported rows small; they do **not** reduce the kernel's 1,326-hand arena dimensions.
The restricted later-street action tree is the principal memory bound. This does not
represent a normal unrestricted Flop solve.

Before observing candidate results the internal target is fixed at
`NashConv < 0.04 chips`, equivalent to `Exploitability < 0.1%` of the 20-chip starting pot
for these constant-sum games. This is an experiment-specific internal BR target,
not a calibrated external-EV tolerance or overall R1 acceptance. `run.json` is the
live-average source; `.sol` summary is checked against it as a presave snapshot.

Run the baseline-only pilot first. The `--baseline-source` argument names a retained
source manifest or source archive; its SHA-256 is recorded with its path and size.
When this is an archive, its hash is an archive identity, **not** the R0 source-manifest
`source_id`. Retain the actual build logs/source manifest separately to connect source,
compiler, flags, target, and executable. The expected baseline revision is `9632d8b`.

```sh
python experiments/hu-postflop-r1/pipeline/run_campaign.py pilot \
  --baseline /opt/r1/target/baseline/release/solvers \
  --baseline-source /opt/r1/baseline-source.tar.gz \
  --out runs/r1-pilot
```

`--cases river turn` selects a subset explicitly. Each command has an external 600-second
timeout, 5-second graceful window, and 5-second kill/verification window. Defaults for
the 64-GiB VM are 40 GiB sampled process-tree RSS, 8 GiB host free RAM, and 10 GiB disk
reserve. Pilot flags can fix different bounds before any measurements; freeze copies
them into the formal plan. The resource triggers are sampled, not hard kernel limits.
The caller must run this entire campaign in its finite systemd/cgroup wrapper and
ensure another build or solver is not sharing the VM. Linux process groups alone
do not survive supervisor SIGKILL safely; see the [supervisor contract](../../../docs/plans/r1-supervisor.jp.md).
The runner does not create a VM, transfer files, install tools, build, or retry.
Pilot `--campaign-seconds` defaults to 1,800 seconds; freeze records a separate comparison
window, default 3,600 seconds. Before each stage, the runner reserves its complete timeout
and cleanup windows; if these no longer fit, it records interruption and refuses to start
another process. It never shortens a later stage's timeout to obtain a favorable result.
This is a dispatch bound, not a hard deadline for hashing or filesystem stalls; the parent
systemd/cgroup limit and VM lifetime remain the hard outer bounds. Supervisor signal or
containment/metric error terminates the campaign instead of proceeding to another run.

## Freeze before comparison

Only completed pilot cases whose live BR reaches the original target and whose saved
summary agrees can enter the plan. An unreached target is not relaxed. A timeout or
resource stop is retained as such. The frozen config changes **only** the iteration cap
to the observed baseline stopping iteration. It preserves target, cadence, finite game,
storage, and thread settings. This supplies a fixed-iteration comparison and a target
check for these IO/workspace changes without choosing a favorable candidate result.

```sh
python experiments/hu-postflop-r1/pipeline/run_campaign.py freeze \
  --pilot runs/r1-pilot/pilot.json \
  --candidate /opt/r1/target/candidate/release/solvers \
  --candidate-source /opt/r1/candidate-source.tar.gz \
  --out experiments/hu-postflop-r1/pipeline/frozen

python experiments/hu-postflop-r1/pipeline/run_campaign.py run \
  --plan experiments/hu-postflop-r1/pipeline/frozen/plan.json \
  --out runs/r1-paired
```

The plan records both binary/source identities, config identities, runner/supervisor
hashes, pilot evidence, host/boot/CPU information, bounds, and the exact run order.
Paths are absolute and intended for the same VM. Preserve the plan and source snapshot
before `run`; do not edit the runner, binary, supervisor, or frozen configs after freeze.
`run` verifies these identities before each solve and refuses a different host/boot.
If the runner changes between pilot and freeze, both versions' hashes remain recorded;
no candidate performance observations are consulted by freeze.

For each case, the order is baseline, candidate, baseline, candidate, baseline, candidate.
Each solve is fresh. Export uses the binary that wrote the artifact, allowing baseline
v1 and candidate v2 to be compared without cross-version reads. The artifact version,
embedded config hash and iteration are recorded from the fixed header.

## Measurements and checks

- Every solver/export/resume process runs through `tools/run_supervised.py`, with binary,
  config, source evidence, runner, stdout/stderr, and resource-record identities. No failed
  stage is retried. Unverified tree cleanup stops the whole campaign immediately.
- Solve time is the external monotonic process interval including initialization,
  CFR/checks, final BR, checkpoint and `.sol` generation, and process exit. Phase spans
  without instrumentation remain `null`; `.sol` `wall_secs` is only the reported loop
  interval and must not be relabeled total or pure CFR time.
- Summary query is a separate process, timed before profile exports, once per run.
  It uses a 1-ms supervisor poll target; other stages use 50 ms. Actual sample gaps and
  OS peak sources remain in the supervisor record. Very short timings include process
  startup, scheduler delay and monitor overhead; do not infer sub-millisecond latency.
- Process RSS uses the supervisor's native root high-water mark with its OS metric name,
  plus sampled tree RSS as a separate lower-bound observation. Checkpoint and `.sol`
  byte sizes are recorded separately. No static arena estimate is presented as RSS.
- Both binaries export `tree`, `strategy`, and `ev` at `--node all`. Exact stdout SHA-256
  equality is required, including row order and numeric JSON representation. Strategy
  rows cover saved nodes with **positive own reach**; omitted zero-reach hands are not
  certified by this public export comparison. Repository tests cover raw block equality.
- Summary comparison excludes only the time field. Iterations, root EV, both gains,
  NashConv, game fields, and node counts must match. A pair is performance-eligible only
  when both executions complete, hit the fixed internal target at the frozen iteration,
  pass all export comparisons, and preserve the intended artifact versions.
- For repetition 1 of each binary/case, checkpoint resume forks a new run at its already
  completed iteration cap. It must reproduce summary and all exported rows with **zero
  additional CFR iterations**. This checks load/restore/republish and is explicitly not
  an interrupted-run continuation test or a saved-quantized-profile BR evaluation.

OS caches are warm/uncontrolled: each artifact was just written, and baseline/candidate
alternate on one VM. There is no cold-cache claim. No other benchmark runs concurrently.
`analysis.json` reports individual measurements and median/min/max, never treats
timeout/resource stops as fast successes, and leaves overall `quality_status` and
`saved_profile_br` as `not_evaluated`. Saved EV is `presave_snapshot`; strategy is
`stored_quantized`. The current CLI does not evaluate BR against the saved profile.

Raw outputs are in `runs/`. Before removing the VM, retain the frozen plan, source/build
identities, supervisor records, compact result/analysis JSON, validation result, and
artifact/export hashes under a dated evidence directory here. For large files retain
their location, availability and hash; a hash of a deleted file alone is not recoverable
evidence. VM lifecycle, transfer caps and budget are controlled by the parent campaign.
Per-run results are written before export, profile records after each view, and campaign
results before resume checks so completed solve evidence survives a later export failure.
The final `run` exit code is 0 only if every selected case passes comparison eligibility;
1 indicates an incomplete/non-equivalent comparison, and 2 indicates an orchestration
error or dispatch-deadline interruption. Detailed solver/resource statuses remain separate.

Orchestration tests use synthetic files and mocked processes; they do not run a solver:

```sh
python -m unittest discover -s experiments/hu-postflop-r1/pipeline -p test_run_campaign.py -v
```
