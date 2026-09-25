# Multiway production initialization (2026-09-10)

Status: production integration, required verification and all eight local
measurements are complete. On this Simple K32 fixture, eight construction
threads reduce median fresh-session construction by 23.37% and checkpoint
reconstruction by 25.01%, with identical compared non-timing results. The
normal CLI uses this implementation; no new experimental flag is required.

Production new/resume, research-session construction and formal `.mwsol`
profile reconstruction use explicitly sized private pools. Serial non-retaining
resource admission remains first, and its exact node count bounds retained
parallel workers. Ordered materialization and arena allocation retain serial
node IDs, histories and layout. Parallel errors release all temporary data
before canonical serial retry. Existing serial core constructors remain
available without adding `Send` to every game adapter's state.

No algorithm identity, state/wire version, chance distribution, bucket schedule
or sweep update order changes. Thread count and memory remain operational
settings. Peak process RAM still includes public tree, merge temporaries,
EHS tables, checkpoint DTOs and solver scratch outside the policy-arena limit.

The earlier [phase benchmark](../tree-initialization-20260910/README.md)
measured 26.86% less diagnostic process time at eight threads. Those historical
results are not substituted for the production-session measurements below.

## Measurement protocol

Retained root: `runs/production-initialization-20260910/`. A 159-file immutable
Rust/Cargo source manifest and ZIP include `.cargo/config.toml`; release
binaries, literal job arguments, config/checkpoint hashes, toolchain and machine
metadata are retained. The source package is distinct from the earlier phase
and average-depth experiments. Runs are sequential with a warm EHS cache, no
concurrent build or solver, a 900-second external per-process timeout and
Windows lifetime peak-working-set queries every 50 ms. The final unsampled
interval can be omitted. No GCP resource is launched for these measurements.

The fixture is the same partial Simple reference: six seats, 100 bb, NL50,
current-street K32, batch 4, no discount/pruning, seed 0, 8 GiB policy-arena
limit. Explicit thread overrides select 1 or 8. The game has 966,141 public
decision nodes and a 545,720,720-byte policy arena. This is a scaling test on a
bounded approximate game, not a quality comparison to a fully matched GTO
Wizard postflop solution.

Fresh cases use the consuming research runner with UniformOne and exactly
1,024 CLI-controlled sweeps, no held-out evaluation, and 16 exported histories
covering openers, 3bet/4bet/5bet, heads-up postflop and the explicit UTG/HJ/CO
three-player check-through path. The research runner uses the same production
session constructor as a normal solve. `constructionElapsedSecs` excludes
subsequent solving, exports, output and disposal. Process time also changes
because `--threads` affects the existing sweep driver; it cannot be attributed
entirely to the new construction path.

Restore cases use the production checkpoint-audit session loader on the same
retained 32,768-sweep checkpoint. The restore config changes only the operational
`run.max_sweeps` ceiling from 4,096 to 65,536, so the session accepts that saved
state. The audit resumes no training. Construction timing includes lowering,
EHS/cache preparation, checkpoint decoding, admission, materialization,
validation, page commitment and policy replay. It excludes later diagnostics
and disposal. Held-out seeds 101/202 use 128 profile worlds, 4,096 node-frequency
worlds and 8,192 prefix-coverage worlds; one traversal trains each deviator only
to exercise the path. These tiny deviator budgets are not BR quality evidence.

The executed order is fresh 1/8, restore 8/1, fresh 8/1, restore 1/8. Two repeats
per kind/thread setting screen repeatability; they are not a confidence
interval. All configuration/abstraction identities and complete non-timing
outputs match within each kind, including the full raw regret hash and exported
fresh rows, or restored node/evaluation/coverage outputs. Timings and explicitly
checked operational thread/config fields are excluded from identity checks.

## Verification completed before measurement

Required fmt and workspace clippy passed. Workspace tests: 756 passed,
30 ignored, 48 suites. Feature-gated CLI clippy and 29 example tests passed;
six focused average-sampling API tests passed. Five new real-Holdem core tests
cover new/resume at 1/2/8/16 threads, every tree/arena field, raw f32/touched
state, actual checkpoint bytes and continued learning; exact/below arena
limits, depth errors, zero threads and pool sizes under a different ambient
pool are included. A CLI session roundtrip test covers construction, save,
restore and continuation across thread settings. Logs/hashes and exact commands
are retained under `verification/verification.json`.

## Completed measurements

| Case | Construction s | Sweep driver s | Process s | Observed peak MB |
|---|---:|---:|---:|---:|
| fresh1-first | 60.289 | 19.053 | 86.283 | 1397.318 |
| fresh8-first | 45.925 | 3.792 | 56.638 | 1404.752 |
| restore8-first | 46.062 | — | 47.508 | 1949.962 |
| restore1-first | 61.353 | — | 66.405 | 1946.223 |
| fresh8-repeat | 45.935 | 3.952 | 56.786 | 1404.940 |
| fresh1-repeat | 59.583 | 18.857 | 85.335 | 1397.375 |
| restore1-repeat | 61.526 | — | 66.504 | 1946.169 |
| restore8-repeat | 46.079 | — | 47.370 | 1949.848 |

MB is decimal. Peak values cover the whole process, not an isolated phase.
Stderr confirms an EHS table cache load in every case (reported as 0.64–0.88 s).

| Construction path | 1-thread median s | 8-thread median s | Time reduction | Median peak increase MB |
|---|---:|---:|---:|---:|
| Fresh production session | 59.936 | 45.930 | 23.37% | 7.500 |
| Checkpoint reconstruction | 61.440 | 46.071 | 25.01% | 3.709 |

The median observed peak increase is below 0.6% in both paths. This does not
bound another tree, smaller bucket schedule, allocator or machine. In the
fresh runs, existing sweep parallelism also reduces driver time from about
19 s to 3.9 s. Therefore the process-time change is deliberately not reported
as an initialization-only speedup.

## Equality and provenance

All four fresh outputs match after removing only their operational thread
field and solve timer; the exported node contexts also match. The raw regret
fingerprint is
`017c15d608eea38398824545c5cb1608d5e0c1723f26690af60873a5970a0fba`.
This covers the complete dense regret array. The 16 exported histories include
1,479 reported rows with identical source status and normalized strategies.
It does not by itself hash unexported average columns; the real-Holdem core
tests separately compare every arena value and serialized checkpoint byte
after construction and continuation.

All four restored outputs match after removing construction/diagnostic
timers, including node rates and SE/ESS/fallback weights, per-seat evaluations,
deviator coverage and deep-prefix coverage. Each restores the same 32,768-sweep
checkpoint and a committed 966,141-node / 31,854,277-column / 64,818,941-slot
arena. Full raw output stays in each retained case directory. The
[evidence JSON](result.json) records
whole-output equality hashes, summarized per-hand rows and exact input/output/
log hashes without duplicating every raw strategy row in the tracked report.

Source manifest SHA256: `fcc52d588112b95c1bb3f7223812a60861e59bea865e4af1b6cab2e4b73a9560`.
The research output calls this immutable source-package identifier
`sourceRevision`; process metadata uses that field name for the Git base
revision. The manifest and ZIP, not the base revision alone, identify this
uncommitted implementation. All 159 archived source/config files match the
measured source. Historical phase/average-depth binaries retain their original
identities.

## Adoption and remaining work

The production CLI now uses the configured construction pool for new solves,
checkpoint restoration and formal `.mwsol` average-profile reconstruction.
Serial non-retaining admission, canonical resource errors and complete page
commitment are retained. A separate merge reservation may fail and retry
serially only after all parallel temporary data leaves scope. Existing serial
core constructors remain usable without a universal `State: Send` restriction.

This change makes the same learned state available sooner; it does not prove
better poker strategies, solve the sparse three-player river-support problem,
or change average-sampling/abstraction defaults. The next quality work should
use the deep-prefix evidence to address those branches. Remaining scaling work
includes the roughly 30-second serial admission walk, frontier balance, and
avoiding duplicate formal-profile reconstruction. Larger machines and other
tree/bucket shapes require separate measurements. No GCP resource was added.
