# VM20: fresh attempt with the fixed CPU profile package

Use the exact [VM19 deployment](../vm19/pack-receipt.json) on a new
`solvers-r1-20260928-20` Spot instance and disk. The package's internal
`vm19` paths, unit names and schema labels identify the reused control
template. Metadata supplies the new numeric instance ID; no VM19 runtime
plan, binary, tests or measurement proof is transplanted.

The [fixed protocol](../../flop-scaling/cpu-profile/protocol.jp.md) remains
unchanged: two CPUs for a fresh portable release build and core tests,
one32-logical-CPU boot for two canonical and eight profiled N64 solves,
and two CPUs for proof verification and recovery. Original creation request
+45 minutes is the absolute STOP. Measurement dispatch must occur within
15 minutes, with its own15-minute limit and15-minute recovery margin.
Individual stage timeouts are upper limits, not an assurance all stages fit;
an incomplete diagnostic stays incomplete without retry or deadline extension.

The proposed reservation is1.85USD, subject to reviewed usage returns,
fresh official prices and resource inventory. Keep the original1USD
uncertainty reserve and512MiB transfer allowance. No additional budget is
authorized by this document. No local native build, solve or state expansion.
