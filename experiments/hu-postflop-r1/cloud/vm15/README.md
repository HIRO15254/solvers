# VM15 worker scratch experiment

Research only. One small2-vCPU Spot VM for bootstrap and dependencies; resize the same stopped VM to32 CPUs for fresh native builds, candidate unit/integration tests, and64 matrix stages.
Absolute75-minute cloud STOP is never extended; the experiment has at most40 minutes and ends at least15 minutes before STOP. Recover and delete the VM and auto-delete disk after a terminal outcome.
No automatic retries, no resume across boots. Linux file/directory fsync and independent completed-case checkpoints protect finished blocks; incomplete blocks remain incomplete.
The workload and performance guard are fixed in ../../flop-scaling/worker-scratch/protocol.jp.md. CPU-intensive verification stays on GCP.
