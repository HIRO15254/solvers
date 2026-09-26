# Candidate01 gate diagnosis

[Observed call counts and retained proof](report01.jp.md): four diagnostic cases
matched candidate01 outputs exactly. These whole-process counts are not timing
shares or performance evidence.

`instrument.py` accepts only candidate01's SHA-pinned archive and manifest, checks
all 368 file identities, and creates a new research source directory. It does not
build or execute a solver. The production checkout and frozen timing protocol are
unchanged. The original gate implementation, validation and returned decision are
preserved; a second scan computes diagnostic bounds, then call-level relaxed
atomic counters aggregate them. There are no counters in integer add/subtract.

```sh
python3 -B instrument.py \
  --archive /opt/r1/exact-new01/source-candidate.tar.gz \
  --manifest /opt/r1/exact-new01/source-candidate-manifest.json \
  --out /opt/r1/exact-diagnostic01
```

Build `out/source` with a separate fresh Cargo target after the fixed timing run.
Run each of the same four cases once with the original target/cadence/cap and one
thread. Keep build/source/binary/supervisor identities. Compare final iterations,
the numerical stopping trajectory (excluding durations), quality, canonical.bin
and state.bin against uninstrumented candidate01. These comparisons are required
before interpreting counters. This script does not certify a build or equivalence.

The report-only `mass_diagnostics` object contains caller and field labels plus
their two-dimensional counts. Caller rows distinguish compact showdown, compact
fold, compatible reach, reporting equity and three test-only categories. Counts
cover the entire process: build, solve, convergence checks, final EV/BR and CFV
capture. They do not identify solve-only rates or time spent in a path.

For N positive represented reaches, `count_bits=bit_length(N)=ceil(log2(N+1))`.
Each float is `m * 2^(shift-149)`. The existing conservative bound is
`B=max_shift-min_shift+24+count_bits`. B<=53 is the existing floating gate;
54..64, 65..128 and >128 characterize possible exact integer tiers. The tighter
diagnostic bound removes unused high significand bits and common low zero bits:
`max_nonzero_bit-min_nonzero_bit+1+count_bits`. Both include one self add-back;
all-zero calls enter B<=53 with B=0. `original_fail_tight_gate_pass` counts only
calls rejected by the original gate but satisfying the tighter sufficient bound.
No diagnostic changes a numerical path. A large call count is not evidence of a
matching fraction of runtime. Instrumented durations and RSS are not comparable
performance evidence.

Outputs include the full copied source, original candidate manifest, patched
source manifest, exact unified patch and provenance with SHA-256 before/after
identities. Five Rust files change: mass.rs, kernel.rs, compatibility.rs, lib.rs,
and hu_scaling_bench.rs. No dependency or artifact format changes are made.

Lightweight tests (no Cargo/solver) use the pinned candidate archive. Override
`R1_CANDIDATE_ARCHIVE` and `R1_CANDIDATE_MANIFEST` if its local location differs:

```sh
python3 -B test_instrument.py
```

## Finite diagnostic driver

After the original 32-process campaign completes and verifies, run the prepared
copy under a separate systemd cgroup (memory maximum 12 GiB, swap zero, and an
absolute deadline). Keep the trusted sibling `exact-mass/run.py`, its protocol
and reference files, and `showdown-kernel/run.py` available beside these scripts:

```sh
python3 -B /opt/r1/exact-control/exact-mass/diagnostic/run_diagnostic.py \
  --root /opt/r1/exact-diagnostic01 \
  --target /opt/r1/target/exact-diagnostic01 \
  --baseline-proof /opt/r1/exact-proof01 \
  --deadline-utc 2026-09-26T19:35:00Z
```

The driver verifies the original campaign before starting. It uses the unchanged
candidate01 supervisor, checks Rust 1.97.0 (30 seconds), builds only the release
example in a fresh target with two Cargo jobs (1,200 seconds), then runs each
case once with one worker (300 seconds each). No fmt, workspace tests, retries,
performance ratio or measured-time reuse occurs. Every stage has sampled RSS
10 GiB, free-memory minimum 1 GiB and disk reserve 4 GiB, with 20 seconds of
additional deadline allowance before starting. A failure records a terminal
result and skips pending stages. All stages rehash the source and check the host.

`root/run` contains plan/result/verification plus `retention.json` and `payload/`
CAS. It retains original candidate archive and manifest, exact instrumented
source archive/manifest/patch/provenance, the new binary, build and process raw
logs/samples, each output artifact, and the selected original candidate01 b1
report/artifacts bound to its retained baseline plan/result. Tools are identity
pins rather than copied executable payloads. Root and target live files may be
discarded only after exported CAS verification. For a successful run, download
`root/run/{plan.json,result.json,verification.json,retention.json,payload/}`; the
duplicate stages and source/target directories are unnecessary for this check:

```sh
python3 -B run_diagnostic.py --root /path/to/retained-root --check
```

The trusted local checker reconstructs the expected patch from candidate01 and
checks all source bytes; it does not import evidence code. It also recomputes
selected-reference output equality and counter identities from retained raw
reports. Original full 32-process campaign verification remains a separate proof:
the diagnostic CAS holds its plan/result and selected references, not every old
campaign process payload. A preparation failure before plan completion retains
the available result/CAS but cannot claim this complete portable check.

`test_driver.py` checks frozen commands/counter identities, real candidate archive
and patch binding, and synthetic supervisor/sample composition with negative
baseline/counter/identity cases. The integration fixture mocks its source layer;
the separate real archive test covers that layer without a solver. Initial test
failure evidence records a fixture missing an input CAS payload; the final test
suite passes after fixing the fixture. Retained `*-test-evidence.json` and raw
logs distinguish this lightweight testing from the remote Rust/solver checks.
