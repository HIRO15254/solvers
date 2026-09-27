# VM17 F32 fused update comparison

Research comparison of baseline and the F32 fused-update candidate; no production adoption yet.
The fixed [measurement protocol](../../flop-scaling/fused-update/timing/protocol.jp.md)
requires exact state/quality equality before interpreting performance. A candidate-specific
native storage fixture and engine/holdem/cfr-ref tests run before the 48-condition matrix.
Four smoke solves and two main canonicals make 54 solves in total.

One Spot VM, e2-standard-2 for dependencies and recovery, e2-highcpu-32 for fresh native
builds and measurement. N2 fallback is permitted only on the same stopped instance after
an explicit E2 failure and within the unchanged deadline. Maximum three starts.
The original cloud STOP is launch-request time plus35 minutes, never extended.
Work ends at min(dispatch+16 minutes, original STOP−15 minutes), with more than10 minutes
required at dispatch. Native binaries are never reused across measurement boots.

The workload has a12 GiB cgroup limit, swap0, full32 logical CPU affinity and no CPU quota.
The solver supervisor and fixed protocol provide tighter per-stage bounds. No automatic
retry or cross-boot resume. Incomplete and mismatched originals remain evidence, never
successful samples. Recovery verifies all original file bytes on GCP; archive plus sidecars
must fit256 MiB and all outbound traffic512 MiB. Local handling is limited to edits,
small checks and compressed-byte transfer hashes.

The [fresh price proposal](../preflight-vm17/cost-proposal.json) covers the entire35-minute
window plus120 seconds of pricing slack at the highest undiscounted rate,40 GiB disk24h,
IPv4,512 MiB egress and the original1 USD uncertainty reserve. It is below2 USD.
Reservation and launch are separate actions. Explicitly delete this VM and its sole
auto-delete disk after recovery, then verify absence. The receipt does not assert an invoice.
