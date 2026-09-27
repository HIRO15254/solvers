# VM16 CPU occupancy diagnostic

Baseline-only research; no production optimization or adoption decision.
Source, profile, native build and measurement boot are fixed. Compare16 workers,
32 workers, and16 workers restricted to one guest logical CPU per physical-core ID.
CPU time includes spin and scheduler work; this does not isolate SMT or bandwidth.

The maximum experiment window is16 minutes. The original35-minute cloud STOP
deadline is immutable, with at least15 minutes reserved for recovery. Bootstrap
and recovery use2 vCPUs; native build and comparisons use32. No automatic retry,
no resume across boots. Transfer at most256 MiB of archive plus small metadata,
within a512 MiB total egress budget. Recover evidence before deleting the VM and
its sole auto-delete40 GiB disk. All heavy work and original-state verification
stay on GCP.

The [protocol](../../flop-scaling/cpu-occupancy/protocol.jp.md) fixes diagnostics
and limitations; the resource reservation must be recorded before launch.

Completed evidence: [measurement report](report.jp.md), [compressed transfer check](download-check.json), and [VM/disk deletion reconciliation](reconciliation.json).
