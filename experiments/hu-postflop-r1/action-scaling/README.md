# River action parallelism comparison

This independent prospective experiment compares source04 and the amended source06 candidate F32
compact-hand River solving. It leaves the original range-scaling runner and
protocol unchanged. The old campaign's actual `frozen.json` must bind River to
1000 iterations and its original source/binary/input from a 32-CPU host. The recovered baseline may come from the previous boot terminated by Spot eviction. Its original binary and exact source04 manifest/archive remain pinned and retained; rebuilding that same source on the new boot may change the binary hash. No new pilot or iteration selection is allowed, and prior-boot timings are not old/new timing pairs.

Both newly built binaries and all action measurements must share the new
32-CPU boot, with completed source-bound release build proofs. Build and measurement are separate. `run.py` imports trusted local
adjacent range-scaling parsing, sampling and storage helpers; their exact bytes
are pinned. Source archives are checked against their complete manifests.

```sh
python3 -B run.py --phase prepare --out /opt/r1/action06 \
  --old-source /opt/r1/range04/source \
  --old-manifest /opt/r1/range04/source-candidate-manifest.json \
  --old-binary /opt/r1/target/OLD32/release/examples/hu_scaling_bench \
  --old-build-record /opt/r1/range04/BUILD32/stages/release-example/supervisor.json \
  --new-source /opt/r1/range06/source \
  --new-manifest /opt/r1/range06/source-candidate-manifest.json \
  --new-binary /opt/r1/target/range06/release/examples/hu_scaling_bench \
  --new-build-record /opt/r1/range06/validation/stages/release-example/supervisor.json \
  --baseline-frozen /opt/r1/range04/MEASUREMENT/frozen.json \
  --deadline-utc 2026-09-26T04:15:56Z

# Stop builds/transfers before measurement.
python3 -B run.py --phase measure --out /opt/r1/action06
python3 -B run.py --phase check --out /opt/r1/action06
```

Replace the illustrative OLD32, BUILD32 and MEASUREMENT locations with the
actual retained paths. Each source archive is adjacent to its manifest as
`source-candidate.tar.gz`; each build result is three parents above its
`stages/release-example/supervisor.json` record. Its saved binary identity must
match the supplied binary, and all validation stages in that result must pass.
Prepare copies both binaries into the new output directory. It does no solving.

Six thread counts (1, 2, 4, 8, 16, 32) each receive one warmup pair and three
measured pairs: 48 processes. Pair order alternates by block and thread index;
all 48 must have byte-identical original strategy/CFV and F32 state files and
equal quality bits/support/tree metadata. Warmups remain evidence but are
excluded from summaries. Report every thread count, both timing curves,
parallel efficiency, adjacent slowdowns and three-pair win counts. The
descriptive adoption guards are new32/old32 <= 0.90 and new1/old1 <= 1.05.
Three pairs do not provide a confidence interval or remove machine noise.

The latest deadline is 2026-09-26 04:15:56 UTC. Every process has a 300-second
bound; starting requires that bound plus 20 seconds before the deadline. Use an
outer cgroup with at most 12 GiB memory, zero swap, at least 32 CPU quota and
control-group cleanup. The supervisor enforces 10 GiB summed sampled RSS,
1 GiB host available memory and 4 GiB disk reserve. Source/binary/host checks
surround every sample. Failures stop the campaign and retain the exact skipped
suffix. There is no retry or resume and no guard result for an incomplete run.

Portable verification uses the existing lossless SHA/gzip payload container,
with all original source archives, build results and their stage logs, binaries,
baseline plan/frozen file, original baseline binary/manifest/archive/input and source04 proof, action plan/result, runner/helpers/protocol, sample
logs, configs and canonical files retained. Compiler/Python executable bytes
may be identity-only; benchmark binaries and output bytes may not. Source-after
remains the validation runner's recorded assertion. Native OS peak RSS is
recorded separately from raw-sample counts/peaks that the checker recomputes.
No retained script is executed. Acquisitions can be combined only when shared
original paths have matching bytes; do not mix evolving result-file versions.

```sh
python3 -B verify-retained.py --retained /proof/old-build \
  --retained /proof/new-build --retained /proof/action-final --expect completed
python3 -B -m unittest discover \
  -s experiments/hu-postflop-r1/action-scaling -p test_action.py -v
```

The portable checker recomputes all 48 sample reports and summaries using the
trusted local runner. It also accepts a failed prefix with its exact skipped
suffix and reports `not_evaluated`, never an adoption guard pass. This experiment
does not certify I16, external poker-reference agreement or R1 completion.
