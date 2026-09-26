# HU compact-hand and worker scaling experiment

This prospective experiment measures the current F32 implementation with one
release `hu_scaling_bench` binary on one Linux x86-64 boot with 32 available
logical CPUs. It separates compact-hand memory changes from worker scaling.
It does not certify external poker-reference agreement, I16 equivalence, the
old codec matrix, or completion of R1. The user's cumulative cloud allowance
is USD 40; VM lifecycle, budget ledger, resize and deletion belong to the caller.

`protocol.json` freezes seven arms (compact 1/2/4/8/16/32 workers and dense 1 worker),
cases, limits, order, pilot rule and descriptive speed thresholds. The caller
keeps source, build and measurement phases separate and transfers evidence
between phases, without concurrent builds/transfers during measurements.

| Case | Source and role | Pilot/maximum iterations |
| --- | --- | ---: |
| River | HU-R0-002 diagnostic, same game/menu/rake assumptions; larger action tree | 1000 |
| Turn | Existing pipeline Turn, all river cards | 1000 |
| Flop | Existing pipeline limited betting and complete runouts | 50 |
| Narrow River | Pipeline River with OOP AsAh,AdAc and IP KsKh; overhead control | 10000 |

All inputs use F32/DCFR. Existing game/economics are preserved except the stated
narrow River ranges. Run controls become explicit fixed iterations without
early stopping. The benchmark retains configured chance thresholds. Each case
gets exactly one compact-1 pilot; its measured solve time can only reduce the
iteration count toward 10 seconds, never raise the fixed cap. The resulting
`frozen.json` is written before any multi-worker measurement.

Measurement has one warmup plus three measured blocks for every case/arm:
112 processes after four pilots. Each case uses three distinct measured cyclic
arm orders. Across four cases the twelve measured rotations put each arm in
each position once or twice, with imbalance at most one. Seven positions cannot
be exactly balanced with twelve measured blocks. Carryover effects are not fully balanced.
Warmups are retained and checked but excluded from time summaries. Every case
and every miss stays in the report. There are no automatic retries or case
substitutions.

For River/Turn/Flop whose compact-1 median solve time is at least one second,
the descriptive screen requires compact-2/compact-1 <= 0.95,
compact-4/compact-1 <= 0.90, compact-4/compact-2 <= 1.05, and at least two of
three paired wins over compact-1 for both parallel arms. These are prospective
practical thresholds, not confidence intervals. Short cases cannot certify
speedup. Narrow River is always an overhead control, even if its total run is
long. Logical CPUs can include SMT; the experiment does not assume 32 physical
cores or promise linear scaling. Additional 8/16/32-worker results report every
speedup T1/Tn, parallel efficiency T1/(n*Tn), adjacent-worker speedup, fastest
observed worker count and first adjacent median slowdown. They describe
saturation and regression for investigation; they add no acceptance threshold
or calibrated knee. A four-CPU validation host is not a formal timing host.

Every warm/measured invocation must have identical `canonical.bin`, `state.bin`,
root EV/BR/NashConv raw bits, retained combo IDs and topology across all seven arms.
The benchmark emits raw F32 strategy/CFV and regret/strategy-sum values in
ascending global combo order on initial positive, board-compatible support.
Dense arrays are projected onto that support. Compact state includes every
stored hand, including hands made dead by later public cards. This is a full
F32 compact-state check, not a comparison of inactive dense-hand state or an
I16 quantization claim. SHA-256 plus streaming byte equality bind the actual
files; emitted BLAKE3 is retained as benchmark metadata.

Solver `run_seconds` excludes tree construction, storage initialization,
EV/BR queries, all-node CFV capture and artifact writes. Reports retain those
separate durations. Full-process native peak RSS (`wait4.ru_maxrss`) includes
all phases. Summed process RSS sampled by the existing supervisor is a distinct
metric that may double-count shared pages and miss short peaks. `build`,
`solver_init`, `run` and CFV capture sample peaks use benchmark UNIX-millisecond
phase events matched to supervisor UTC samples. They are sampled observations,
not exact phase peak RSS; a phase without a sample records null, never zero.
Compare compact/dense full-process RSS and storage payload bytes alongside the
sampled phase memory. Research output bytes are not production SOL/CKPT sizes.

The caller builds and validates a frozen source archive with `validate.py`
before launching this runner. After changing the measurement CPU, use a new
target and `validate.py --build-only --name build32` to record toolchain and
fresh native release build without treating that phase as workspace validation.
`--build-record` must be the successful
`build32/stages/release-example/supervisor.json` whose identities include that
source manifest. Source inventory must exactly match the manifest's file list,
excluding reproducible cache/build directories. The runner copies and pins the
compiled example, and pins source, original binary, supervisor, protocol,
runner, Python, inputs and build record before/after every invocation.

Example paths (the caller supplies actual snapshot/output locations):

```sh
python3 -B scaling-run.py --phase pilot \
  --source /opt/r1/scaling/source \
  --source-manifest /opt/r1/scaling/source-candidate-manifest.json \
  --binary /opt/r1/target/scaling/release/examples/hu_scaling_bench \
  --build-record /opt/r1/scaling/run/stages/release-example/supervisor.json \
  --out /opt/r1/scaling/measurement \
  --deadline-utc 2026-09-26T07:00:00Z

# Collect build/pilot evidence and stop transfers before starting measurement.
python3 -B scaling-run.py --phase measure --out /opt/r1/scaling/measurement
python3 -B scaling-run.py --phase check --out /opt/r1/scaling/measurement
```

Use an outer cgroup with MemoryMax <= 12 GiB, swap disabled, control-group
cleanup, a finite service timeout and the existing VM expiry. CPU affinity must
allow all 32 logical CPUs; effective CPU quota must be unrestricted or at
least 32. CPU model/core topology, affinity, boot ID, cgroup limits and CPU
accounting are retained. The supervisor enforces 300 seconds per process,
10 GiB summed RSS, at least 1 GiB host available memory and 4 GiB free disk,
with five-second graceful and kill waits. The runner refuses to start a sample
unless its entire bound plus cleanup margin fits before the supplied deadline,
which must be no later than 07:00 UTC. The caller shortens the cloud STOP
deadline to at most 90 minutes after the 32-CPU resize, always before the
original 2026-09-26 07:15:19 UTC expiry, and leaves recovery time after the
supplied measurement deadline. Resizing does not extend the original window.

Failure, timeout, correctness mismatch or deadline exhaustion stops the run
and marks remaining invocations skipped with a machine-readable reason.
Incomplete campaigns never receive a passing screen. `check` re-reads and
verifies all source, records and output paths; it requires their original VM
locations and is not a relocatable offline checker. The caller must separately
retain the archive, source/binary identities, records, raw canonical outputs,
plan, frozen counts, protocol and their hashes before deleting the VM.

Small local checks (no Cargo or solver execution):

```sh
python3 -B -m unittest discover -s experiments/hu-postflop-r1/range-scaling -p test_scaling_run.py -v
```

Before the first scaling pilot, the success-record check was corrected against
retained source01 toolchain/fmt records: the supervisor's successful
`stop_reason` is `completed`, not null. The corrected runner was checked against
those actual records and a regression fixture; this prospective repair changes
neither case selection nor thresholds.

## Portable retained evidence

`verify-retained.py` verifies acquired evidence without the original VM or its
absolute filesystem paths. The existing codec `retain.py` container is reused
only for lossless gzip payload storage: its `r1.context-linux-retention/v1`
label does not certify this as a codec experiment. Every encoded blob and
decoded original payload is checked against its retained byte count and
SHA-256. Source archives are checked for the exact manifest file set and bytes,
then exposed virtually at their original source paths. Retained scripts are
read as evidence and never executed.

```sh
python3 -B experiments/hu-postflop-r1/range-scaling/verify-retained.py \
  --retained experiments/hu-postflop-r1/range-scaling/source01/failed-proof \
  --expect-validation failed

# Repeat --retained to combine matching validation/build and final measurement acquisitions.
python3 -B experiments/hu-postflop-r1/range-scaling/verify-retained.py \
  --retained /retained/full-validation \
  --retained /retained/native-build \
  --retained /retained/final-measurement \
  --expect-validation completed --expect-scaling completed

python3 -B -m unittest discover \
  -s experiments/hu-postflop-r1/range-scaling -p test_retained.py -v
```

Distinct acquisitions may share an original path only when its retained bytes
match. Do not combine a pilot-era `result.json` and its later final version under
the same original alias. A missing alias can be recovered only from another
retained payload with the exact same byte count and SHA-256; each recovery is
reported. Benchmark binary bytes, source archives, configs, runners, stage
records, raw stdout/stderr/samples and both original canonical files are
required. Compiler and Python executable bytes may be absent; their recorded
size/SHA identities remain required and are explicitly reported as identity-only.

Validation reports distinguish completed `full-validation`, completed
`release-build-only` and failed validation. Build-only success never means
workspace tests passed. Failed records retain child exit, stop reason, cleanup
state and the exact unexecuted suffix. The optional `--no-fail-fast` workspace
test flag is supported for the later snapshot. Source archive contents and any
separately retained source aliases are checked, while source-after inspection
remains the recorded runner assertion; it is not an independent retained
filesystem snapshot.

For a completed scaling campaign the verifier requires the retained runner to
match its trusted local adapter version. It routes that runner's checking code
to verified payloads, checks every pilot and all 112 scheduled samples, compares
the original canonical/state bytes, and recomputes the recorded summaries.
Resolved sample executables must match the pinned benchmark and the supervisor
must match its source archive member. Raw samples independently reconstruct
sample counts, sampled peak RSS and the final cleanup sample. The native OS
peak remains an OS-reported record, separate from that sampled metric.
Incomplete scaling campaigns cannot receive a portable performance pass.
The report lists available validation proofs separately; a scaling-only
acquisition does not establish full workspace validation. Preserve a matching
full-validation proof alongside the native build and measurement proof for
adoption review.
