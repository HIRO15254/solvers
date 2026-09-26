# Final CLI / artifact pipeline comparison

The measurement design is frozen before candidate execution; see
[protocol.json](protocol.json) and [freeze.json](freeze.json). The freeze changes
only the reviewed draft's status and repeats the lightweight checks. It is not
evidence that the Linux build or measurements have completed.
No cloud resource is created by these scripts. The related
[research proposal](../../../docs/research/2026-09-27-r1-final-pipeline-proposal.jp.md)
does not supersede the final frozen protocol.

The [first deployment evidence](proof01/README.jp.md) records a successful old
release compilation followed by a host-identity guard failure, before any
measurement process. The separately recorded
[second deployment setting](../cloud/vm11/deployment-correction02.jp.md) enables
the CPU controller before prepare and uses fresh targets/output with unchanged
controls, sources, quality targets and the original absolute deadline.

The comparison uses production Git bytes at old `88ffa5d` (SOL3/CKPT1) and new
`11e4062` (SOL4/CKPT2). The three retained pipeline fixtures change only the worker
count from eight to one. Native CLI stopping retains each original cap/cadence
and requires NashConv strictly below 0.04 chips. The independently re-evaluated
stored quantized profile must also meet that target. Each deviation gain and NC
must be at least −1e−6 chips; original values are retained without clamping.
These no-rake, zero-sum fixtures do not establish general-sum or external-reference
quality. Old/new numeric equality is not assumed.

## Fixed work and evidence

Nine build stages check Rust1.97.0 and build each arm's CLI, saved-profile audit,
probe and codec executables, with targeted Clippy for both research overlays.
Targets must be fresh, separate and outside source/output directories.

The 156-process measurement schedule is six semantic censuses, then all24 solves
(one warmup pair and three measured pairs per case), then24 summaries,24 saved
profile audits,72 codec processes and six warmup checkpoint decodes in the
corresponding sample order. The solve pair order reverses with case/block parity.
Every codec operation reads its own completed solve's immutable SOL. No warmup
enters statistics; no retry or replacement is allowed. A later saved-profile or
artifact failure invalidates the comparison even if all solves already ran.

Whole solve time includes initialization, CFR, quality checks, checkpoint/SOL
writing, reporting and destruction. Internal final-source phase values are
`null`. Codec operation timers and saved-profile load/evaluation timers are
separate observations, not decompositions of the solve process.

Repeated same-arm live results, trajectories, checkpoint state bytes and decoded
SOL canonical bytes must agree; only identified wall-time metadata is excluded.
Full/root/rewritten SOL views are connected by original-byte SHA-256 checks.
Python binds reported BLAKE3 strings to native output but does not recompute
BLAKE3. Cross-arm input/census bytes and normalizer bits must agree. The census
does not enumerate evaluator internals or public chance-card IDs, so it is a
necessary semantic check alongside source review and independent oracle tests,
not a complete proof of terminal equivalence.

The unchanged current production input closure in [source-pins.json](source-pins.json)
is compared with the historical exact-mass source04 archive. Its original eight
validation stages are rechecked, including958 workspace passes,31 ignored tests,
56 summaries, three release oracle tests and one release river-resolve test.
This reuses source-identical validation; it does not claim a new full-workspace
execution on this host. The two overlay examples receive fresh build/Clippy
checks. Original source files, Cargo inputs and crate file sets are pinned.

## Memory interpretation

Repository-owned CLI/config/game/engine/formats execution paths and the research
examples were inspected for process spawning in both revisions: they use Rayon
threads and no process-spawn call was found. This is not a formal proof for every
third-party dependency. All sampled PIDs must show only the root or an empty
post-exit tree before the direct-process RSS screen is eligible.

Linux wait4 peak may inherit a floor from the Python parent. Census, source,
SOL/CKPT and other retained-byte reads can raise that floor. All solves precede
large codec canonical retention, but this does not remove the effect. Therefore
native-new/native-old is not the memory comparison. With valid root-only samples,
`max(native_peak_new) / min(sampled_peak_old)` is a conservative comparison of
OS-accounted peaks for these recorded runs. It is not exact physical memory,
phase memory or a population estimate. An unmet bound is inconclusive. Raw native
and sampled counters remain separately available.

## Deployment and commands

Keep this directory and sibling `showdown-kernel/run.py` and `exact-mass/run.py`
in the same trusted checkout layout. Those are the only imported campaign
helpers; no retained evidence code is executed. Supply each arm root with
`source/`, `source-candidate-manifest.json` and `source-candidate.tar.gz`.
Both sources receive byte-identical overlays at:

- `crates/cli/examples/hu_pipeline_probe.rs`
- `crates/formats/examples/sol_codec_bench.rs`

The manifest's file set, lengths and SHA-256 must match its archive exactly;
directory entries, when present, must match the manifest. The packer preserves
the respective Git `tools/run_supervised.py` bytes. Cargo/rustc arguments below
are absolute real toolchain binaries, not rustup proxies. Historical proof is
the extracted original `exact-mass/exact-proof04.tar.gz` payload directory.

Run within the owner's single systemd unit on one4-vCPU x86_64 boot,12GiB memory
limit, swap0, all-process cleanup and an absolute lifetime at most three hours.
Reserve at least20 minutes for export/local verification before VM shutdown.
The runner separately enforces10GiB sampled RSS,1GiB free-memory and4GiB free-disk
reserves,1200-second build stages,300-second solve/audit/probe stages and120-second
query/codec stages. Every launch must fit timeout+20 seconds before its deadline.

```sh
python3 -B experiments/hu-postflop-r1/final-pipeline/run.py --phase prepare \
  --old-root /opt/r1/final-old --new-root /opt/r1/final-new \
  --old-target /opt/r1/target/final-old --new-target /opt/r1/target/final-new \
  --cargo /ABSOLUTE/1.97.0/bin/cargo --rustc /ABSOLUTE/1.97.0/bin/rustc \
  --validation-proof /opt/r1/exact-proof04 --out /opt/r1/final-proof \
  --deadline-utc YYYY-MM-DDTHH:MM:SSZ
python3 -B experiments/hu-postflop-r1/final-pipeline/run.py --phase build --out /opt/r1/final-proof
python3 -B experiments/hu-postflop-r1/final-pipeline/run.py --phase measure --out /opt/r1/final-proof
python3 -B experiments/hu-postflop-r1/final-pipeline/verify.py --out /opt/r1/final-proof --expect completed > /opt/r1/final-proof/verification.json
```

For portable retention, collect `plan.json`, **`build.json`**, `result.json`,
`retention.json`, `verification.json`, any `prepare-failure.json`, and the complete
deduplicated `payload/`. The old exact-mass collector omits `build.json`, so do not
use it unchanged. Raw stage directories are redundant only after every available
file has been retained and portable checking succeeds. On failure preserve
retention errors and unmatched original bytes, without claiming successful
identity binding. The same trusted checker runs after relocating this proof;
original VM paths are aliases, not required live paths. Toolchain executables
are recorded by identity rather than copied into the proof.

Lightweight checks (no Rust build, solver or cloud operation):

```sh
python3 -B experiments/hu-postflop-r1/final-pipeline/test_run.py
python3 -B tools/check_docs.py
```

The recovery packer and readable report have [21 separate lightweight checks](support-tests/report.json).
After recovery, `report.py --out PROOF --json-target NEW.json --markdown-target NEW.md`
revalidates original bytes with the trusted runner before rendering. A failed
proof reports only its failure and stage counts, without performance claims.
