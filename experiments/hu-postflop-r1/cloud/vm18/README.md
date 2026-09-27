# VM18 portable build and Flop chance-depth comparison

[Completed comparison and retained evidence](report.jp.md): all38 solves and
full payload/quality checks passed, but the predeclared performance screen
rejected depth1. No production/default change. The VM and disk were deleted
after verified recovery; usage estimates remain separate from actual invoices.

One finite Spot VM, with a fixed 60-minute STOP measured from creation request.
Build and core tests use two logical CPUs and a fresh target with explicit
`RUSTFLAGS=-C target-cpu=x86-64-v3`; the build deadline is launch +20 minutes.
The same instance is stopped, resized to32 logical CPUs, and rebooted only after
the immutable successful build receipt exists. All38 solves use that binary on
one measurement boot. This does not compare absolute times with older native builds.

Measurement starts only with20 minutes of work and15 minutes of recovery left.
No automatic retry, resume, second instance, or deadline extension is permitted.
The small and large phases have separate pinned host, source, and binary records.
The experiment reader must verify full canonical streams and all38 state/quality
comparisons on GCP. A partial run does not establish performance acceptance.

Recovery uses two CPUs. Archive, manifest, checksum and split parts stay under
`/opt/r1` on the persistent boot disk. A verified compressed download precedes
explicit VM/disk deletion and resource absence checks. The local machine only
performs small source/metadata checks and compressed transport hashes.

The price proposal retains the complete62-minute exposure at the higher regular
32-CPU rate,40GiB disk for24h,512MiB outbound allowance and$1 uncertainty.
It is an estimate, not an invoice or a guaranteed bill ceiling. No launch is
authorized by this file alone: the shared$40 ledger must hold a$2.50 reservation.

Controls are linked to the predeclared
[comparison protocol](../../flop-scaling/chance-grain/protocol.jp.md).
