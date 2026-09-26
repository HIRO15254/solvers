# Finite current-phase runner

`runner.py` implements the frozen prospective protocol without changing the
production source or the preparation controls. It launches no VM and changes no
budget. Actual Rust/C builds and Linux calibration remain execution prerequisites;
the fake Python tests do not establish their success.

The owner supplies a fresh Linux x86_64 boot with four available logical CPUs,
CPUWeight 100, MemoryMax 12 GiB, MemorySwapMax 0, KillMode=control-group and
SendSIGKILL=yes. The enclosing service and VM have absolute deadlines. No other
experiment, transfer, archiver, or build may overlap measured processes.

```sh
python3 -B phase-control/current-phases/runner.py \
  --source /opt/r1/phase-source \
  --workspace /opt/r1/current-phase-work01 \
  --out /opt/r1/current-phase-proof01 \
  --reference-proof /opt/r1/final-proof02 \
  --cargo /ABSOLUTE/1.97.0/bin/cargo --rustc /ABSOLUTE/1.97.0/bin/rustc \
  --cc /usr/bin/x86_64-linux-gnu-gcc-12 \
  --cargo-home /opt/r1/cargo-home --rustup-home /opt/r1/rustup-home \
  --supervisor /opt/r1/phase-control/tools/run_supervised.py \
  --launch-record /opt/r1/deployment/launch.json \
  --work-deadline-utc OWNER_FIXED_UTC --stop-deadline-utc OWNER_FIXED_UTC
```

The source, work root and proof root must be disjoint. Both output roots must be
new. Cargo intermediates stay under `workspace/targets/`, outside the recovered
proof. Both source copies, their complete manifests/patches, all helper sources,
and resulting executables are retained by original path and SHA-256 in the
proof's `retention.json` / `payload/` CAS. The existing 512 MiB compressed archive
bound belongs to the recovery packer; the runner does not archive while measuring.

The deployment includes sibling `current-phases/`, `final-pipeline/` (run.py,
protocol.json, hu_pipeline_probe.rs, three configs), `focused-memory/`
(native_rss.c, calibration.py, protocol.json), `showdown-kernel/run.py`, and
`exact-mass/run.py`. Include all files named by the preparation freeze, including
its retained test evidence. The supervisor location is an explicit CLI argument.
The exact proof02 plan/build/result pins come from the frozen focused-memory
protocol. Its already validated machine-independent observations are the
comparison reference; no historic compiler or solver is rerun.

## Launch record

The owner writes a `r1.current-phases-launch/v1` JSON object before invoking the
runner, retaining the real cloud/service evidence used to populate it:

```json
{
  "schema": "r1.current-phases-launch/v1",
  "instance_created_utc": "2026-01-01T00:00:00Z",
  "instance_id": "ACTUAL_METADATA_INSTANCE_ID",
  "boot_id": "ACTUAL_LINUX_BOOT_ID",
  "unit": "solvers-r1-current-phases.service",
  "unit_started_utc": "2026-01-01T00:05:00Z",
  "runtime_max_seconds": 2400,
  "runtime_max_systemd": "40min",
  "work_deadline_utc": "2026-01-01T00:45:00Z",
  "stop_deadline_utc": "2026-01-01T01:00:00Z"
}
```

These are examples, not dates or VM identifiers to reuse. VM creation to STOP is
at most 3600 seconds, creation to work deadline at most 2700, and the STOP tail
at least 900. The recorded numeric runtime must equal the actual systemd textual
duration and fit between unit start and work deadline. The runner reads live
systemd/cgroup containment, CPU topology/features, kernel and boot before/after
every stage. A process starts only if its full timeout plus 20 seconds fits.

## Fixed work and failure semantics

The plan creates all **297** stage entries before execution:

- Nine preparation stages: three tool versions, two native C compiles, two Cargo
  builds, native-child isolation calibration, and resettable-memory calibration.
- All 48 solves, in the protocol's four-arm warmup and three-block order.
- All 96 codec samples, case order then decode-all/read-root, each using that
  case's measured block 1 plain SOL as its immutable input.
- All 48 solve artifacts receive saved-profile audit, checkpoint decode, and
  stream-write/canonical verification: 144 additional quality processes. Their
  durations are not performance samples. Warmups are included.

Each copy has one locked/offline/release Cargo invocation, jobs 2, native CPU
flags, building CLI and the three pinned examples. `hu_pipeline_probe.rs` is the
only extra overlay beyond the frozen copy generator's output; the plan pins the
full resulting file closure. The measurement source is never changed afterward.

Native compilation/version stages have 30-second limits; each Rust build 900;
native calibration has a 60-second internal bound and 70-second supervisor
bound; reset calibration, samples and quality calls have 30-second bounds.
Every call uses the existing supervisor's 20 ms resource sampling, 10 GiB sampled
RSS stop, 1 GiB free-memory minimum, 4 GiB disk reserve, and 5+5 second shutdown
tail. A retained polling gap above one second invalidates the stage.

One failure stops execution: a passed prefix is followed by a failed stage and
skipped suffix. A final comparison failure can have all process stages passed but
the campaign remains failed. There is no resume, replacement, retry, adjustment
of quality targets or extra repetitions. Available original logs/artifacts are
retained on failure. A preparation failure before plan creation retains a failed
result without claiming any executed performance stage. Abrupt termination that
prevents finalization leaves nonterminal evidence; it must not be certified.

## Retained schemas and checker boundary

`plan.json` has schema `r1.current-phases-plan/v1`: original paths, immutable
protocols, launch/deadlines, both source closures, controls, reference pins,
inputs, compiler/supervisor identities, fixed environment, and initial host.
`result.json` has schema `r1.current-phases-result/v1`: terminal status, binaries,
native binaries and the exact `schedule()` rows. Each attempted stage records
environment, required input pins, before/after host/source identities, supervisor
record, and the sample derived from original bytes. Failed/skipped rows never
substitute zeros for absent observations.

`check_run.py` is a separate portable reader. It validates retained CAS contents,
trusted control/fixture/source identities, exact schedule and prefix semantics,
commands/environments/resource bounds/chronology, all native calibration records,
and reconstructs stage samples. It never executes/imports recovered source.
`verify_stage()` and `compare_completed()` are pure retained-byte helpers shared
with this trusted checker. Compiler/Python executables are identity-only; sources,
sample binaries and all outputs have retained original bytes.

All case/arm live trajectories, stopping iterations, checkpoint/config bytes,
pre-save metadata, saved quantized EV/BR/gains/NC and decoded canonical bytes must
match their own case and the current-source proof02 reference. Canonical equality
excludes only `wall_secs`. Measured full/root outputs are bridged to the fixed
plain input's verified stream-write result; every own SOL rewrite must equal its
original bytes. No external-library acceptance is implied.

Completed results contain separate timing calibration per case/operation and the
memory observer screen. Timing uses native external child wall durations;
memory-arm durations never enter timing calibration. Failed calibration or a
sub-10ms denominator keeps phase timing descriptive. Memory retains absolute
reset-window VmHWM, its raw entry/end counters, and unreset plain/off/time native
peaks. A passed observer screen permits descriptive counters only, never a
physical-memory bound or an old/new improvement claim.

```sh
python -B experiments/hu-postflop-r1/current-phases/test_runner.py
python -B experiments/hu-postflop-r1/current-phases/test_check_run.py
```

These tests use fake execution and tiny synthetic evidence only. No Cargo,
solver, Linux-native probe or cloud action runs during local tests.
