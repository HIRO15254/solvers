# VM15 worker scratch experiment

Research only. One small2-vCPU Spot VM for bootstrap and dependencies; resize the same stopped VM to32 CPUs for fresh native builds, candidate unit/integration tests, and64 matrix stages.
Absolute75-minute cloud STOP is never extended; the experiment has at most40 minutes and ends at least15 minutes before STOP. Recover and delete the VM and auto-delete disk after a terminal outcome.
No automatic retries, no resume across boots. Linux file/directory fsync and independent completed-case checkpoints protect finished blocks; incomplete blocks remain incomplete.
The workload and performance guard are fixed in ../../flop-scaling/worker-scratch/protocol.jp.md. CPU-intensive verification stays on GCP.

The [completed comparison](report.jp.md) rejects the candidate under the precommitted performance guard. All64 stages and the state/quality comparison passed; native candidate tests passed138 with13 ignored. Fixed iterations do not establish convergence or external quality acceptance.

Original evidence is retained in three `flop-worker-cloud32-proof01.part00..02` files, concatenated in numeric order into a tar.gz. See the [transfer verification](download-check.json) and [archive manifest](flop-worker-cloud32-proof01.tar.gz.manifest.json). Local verification hashed compressed bytes only. The VM and sole boot disk were deleted at2026-09-27T04:29:22.259Z; [absence and operation completion](reconciliation.json) were verified. No solver code was adopted.
