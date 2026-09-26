# Current HU worker endpoints on 32 logical CPUs

This prospective comparison covers the production closure at
`11e4062ba1735e58b60d12999cb23ed10fd1a163`, after prepared terminal tables and
exact compatible mass arithmetic. It does not reuse historical scaling times.
The [source06 River experiment](../action-scaling/source06/report.jp.md) used an
earlier kernel; its 32-worker median was slower than its 16-worker median.
The current [exact-mass comparison](../exact-mass/report04.jp.md) and
[CLI pipeline comparison](../final-pipeline/proof02/report.jp.md) used one worker.

[protocol.json](protocol.json) fixes **36 runs**: compact/F32, workers1/16/32,
River/Turn/Flop, one warmup and three measured blocks. Within each case, measured
worker orders rotate so each worker occupies each position once. All warmups
must pass correctness checks and are excluded from performance statistics.
There is no pilot, retry, replacement, outcome-dependent selection or guarantee
that32 workers will beat16. This is an endpoint refresh, not the full six-worker
curve or a search for an optimal worker count.

| Case | Internal NashConv target, inclusive | Maximum iterations | Check cadence |
|---|---:|---:|---:|
| River | 0.439 | 1000 | 100 |
| Turn | 0.00228 | 1000 | 100 |
| Flop | 0.0367 | 50 | 5 |

Inputs, targets, caps and cadences are reused unchanged from exact-mass04;
target/cadence arguments are explicit because the research benchmark otherwise
ignores configured stopping. The first passing check stops execution. Every run
must reach its target and agree across workers/repeats in complete stopping
trajectory values and f64 bits, final EV/BR/NashConv bits, all retained strategy
and CFV canonical bytes, F32 regret/strategy-sum state bytes, game metadata and
normalized config. Original bytes are compared directly as well as by SHA-256.
Reported BLAKE3 is syntax-checked and retained, not independently recomputed.

The River input contains nonzero rake and unconfirmed external-reference
assumptions. These are internal quality checks, not external accuracy or
zero-sum-equilibrium certification. I16, arbitrary ranges and full R1 acceptance
are outside this campaign.

## Source and validation

Reuse the exact-mass04 source archive and manifest **without repacking or editing**.
The manifest correctly records dirty base `db9b874`; the actual production,
Cargo and contract-file bytes are bound to Git11e4062 by
[source-pins.json](source-pins.json). All211 entries, including
`hu_scaling_bench.rs`, match that Git revision. The archive contains368 files and
two declared empty directories, all checked against its manifest.

The source archive can be recovered from
[exact-proof04.tar.gz](../exact-mass/exact-proof04.tar.gz), member
`payload/51068bb8464b57b91d11fe1ce1be848f14cd83e26865661afeeee3c70a3e9cd7`
(1,400,466 bytes). Its manifest is member
`payload/1a84947f6daa9ca1c57d5f48e4914e176643dbe9c787262ded95dcf6aba042d6`
(68,137 bytes), also available as
[source-candidate-manifest.json](../exact-mass/source-new04/source-candidate-manifest.json).
Place these at ROOT/source-candidate.tar.gz and ROOT/source-candidate-manifest.json,
and extract the unchanged source into ROOT/source/ before preparation.

The trusted checker rereads all eight historical validation stages and their
raw logs/source/tool/binary bindings:958 workspace passes,31 ignored tests,
56 summaries, plus separately three release oracle passes and one river-resolve
pass. It requires the original fixed foundation plan and exact source hashes.
This is source-identical historical validation, not a new workspace execution.
The foundation's original payload remains a separate required evidence input;
it is not redundantly copied into this proof. Retained code is never imported.

Only two new build stages run: actual Rust1.97.0 version inspection and a fresh
offline, locked, native release build of `hu_scaling_bench`. Source, output and
target directories must be disjoint; the target must not exist at preparation
or build entry. The one resulting binary is retained and used for every worker.
Build and measurement must share the same32-logical-CPU boot, topology, full
affinity and cgroup. Compiler/Python executables are identity-only; source,
measured binary, raw build output and measurement artifacts are retained.

## Timing, resources and limits

The primary timer includes CFR and all stopping EV/BR checks. Construction,
initialization, final reporting queries and canonical capture remain separate.
Report all three worker medians, speedups over1, efficiencies,32/16 ratio and
any slowdown. Three observations are descriptive, not confidence intervals;
short cases do not establish broad practical speedup.

Native `wait4` RSS can inherit the Python parent's highwater. Native full-process
RSS and sampled process-tree/run-phase RSS remain separate counters; this
campaign makes no memory-reduction claim. It also does not infer32 physical
cores from32 logical CPUs; retain core/socket topology and CPU model.

Each measurement is bounded at300 seconds, the release build at1200 seconds,
with an additional20-second launch margin. The absolute deadline is fixed at
preparation, at most one hour later. The caller reserves a separate recovery
margin before VM expiry and owns cloud budgets, VM start/stop and deletion.
There is no deadline extension. An outer unit must use MemoryMax<=12GiB,
MemorySwapMax=0, CPUWeight=100 and full control-group cleanup. CPU quota must be
unlimited or at least32; sampled process RSS is limited to10GiB, free RAM must
remain at least1GiB, disk reserve4GiB, polling20ms and grace/kill waits5 seconds.
No concurrent build, transfer or other experiment belongs in the measurement
interval. Run prepare/build/measure in the same initialized unit so its CPU
controller fingerprint does not change between stages.

## Interface and recovery

Keep this directory beside the trusted `exact-mass/run.py` and
`showdown-kernel/run.py`. Reuse the extracted original exact-proof04 payload as
FOUNDATION. The runner pins its controls before invoking source-owned supervisor
code; the portable checker only imports trusted checkout helpers.

```sh
python3 -B current-scaling32/run.py --phase prepare \
  --root /opt/r1/current32 --target /opt/r1/target/current32 \
  --foundation-proof /opt/r1/exact-proof04 \
  --cargo /opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/cargo \
  --rustc /opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/rustc \
  --out /opt/r1/current32-proof --deadline-utc FIXED_UTC
python3 -B current-scaling32/run.py --phase build --out /opt/r1/current32-proof
python3 -B current-scaling32/run.py --phase measure --out /opt/r1/current32-proof
python3 -B current-scaling32/run.py --phase check --out /opt/r1/current32-proof \
  --foundation-proof /opt/r1/exact-proof04 > /opt/r1/current32-verification.json
```

Retain plan.json, build.json, result.json, retention.json, the full deduplicated
payload directory, external verification and deployment/termination metadata.
The existing [recovery packer](../cloud/bundle-final-proof.py) can preserve this
layout after quiescence, including unmatched raw failure outputs. Portable
checking accepts relocated proof and foundation directories; original VM paths
are aliases. Failed suffixes remain failed; no summary is accepted for incomplete
measurements. An early prepare failure verifies only available CAS bytes and
explicitly reports incomplete provenance. A recovery bundle check proves retained
bytes, not a successful benchmark or a stopped VM.

Lightweight tests use in-memory fixtures only:

```sh
python3 -B experiments/hu-postflop-r1/current-scaling32/test_run.py
```
