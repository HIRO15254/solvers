# VM19 finite software CPU profile

One E2-only Spot instance: two CPUs for portable build, core tests and perf
preflight,32 logical CPUs for one measurement boot, then two CPUs for evidence
verification and recovery. Production sources are unchanged.

The original STOP is creation-request +45 minutes. The build deadline is
launch +20 minutes, but measurement requires a full15-minute window followed
by15 minutes of recovery; it cannot start after launch +15 minutes. An earlier
build deadline does not authorize a later measurement start. No automatic
retry, extra instance, N2 fallback or deadline extension is allowed.

The intended reservation is1.85 USD:47 minutes at the higher E2 regular-rate
ceiling0.80 USD/h, Spot IPv4,20GiB balanced disk for24h,512MiB total egress and
the original1 USD uncertainty reserve. A price proposal, current inventory,
and the shared40 USD ledger must validate that amount before any launch.
This document and research preparation do not reserve or spend funds.

N64 diagnostic profiles use one frame-pointer-enabled portable binary on
one32-logical-CPU boot. All heavyweight state verification, perf parsing and
archive compression run on GCP. Local work is limited to small source and
metadata checks and compressed transport hashes. Evidence is recovered and
verified before explicit instance/disk deletion and absence checks.

See the [prospective design](../../flop-scaling/cpu-profile/proposal.jp.md).
The executable protocol and preflight must be complete before launch.
