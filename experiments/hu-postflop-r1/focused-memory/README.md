# Focused native child RSS comparison

This separate protocol measures the kernel-reported `wait4.ru_maxrss` of the
solver child forked by a small native launcher. It does not modify final-pipeline
proof01/proof02, production code, quality targets or their recorded outcomes.
The prospective design and exact proof02 reference pins are in
[protocol.json](protocol.json). There is no time-performance screen here.

In proof02, every measured Flop process had only its root in the retained PID
samples. The three old native peaks were 337571840,337375232,337866752 bytes;
their respective sampled `/proc/PID/statm` peaks were
337960960,337981440,338046976. Thus all three violated the frozen `U>=L`
eligibility test by 389120,606208,180224 bytes. The new native peak was
100360192 bytes in every measured case, consistent with an inherited launcher
floor; the process allocation history was not independently recorded there.
Proof02's memory screen remains `null`. These observations do not establish
either physical-memory peaks or an additional-child cause.

The [Linux statm manual](https://man7.org/linux/man-pages/man5/proc_pid_statm.5.html)
documents approximate RSS accounting and directs accurate snapshot users to
smaps/smaps_rollup. The
[getrusage manual](https://man7.org/linux/man-pages/man2/getrusage.2.html)
documents KiB units, preservation across exec, and descendant-accounting limits.
Consequently this campaign does not combine the two counters. It reports only
the direct child's kernel counter, with any inherited small native startup
floor still included. Reaped descendants could contribute to that counter;
source review and PID observations supplement, but do not formally prove,
absence of arbitrary short-lived descendants. It is not physical resident peak,
per-phase memory, or a population estimate.

## Fixed work and guards

The original proof02 executables and source archives, three input files, one
worker, caps/cadences, and strict live/stored-profile NashConv below0.04 chips
are unchanged. One warmup and three measured old/new pairs for each case retain
the exact original solve ordering:24 solves. After all solves,24 summaries,
24 saved-profile audits and24 full canonical decodes verify each own-arm result
against the corresponding proof02 sample. Checkpoint bytes, config bytes,
live trajectory, summary, EV/BR/NC and canonical bytes must match; only the
already-defined wall-time metadata is excluded. No cross-version read or
cross-arm numerical equality is required.

The new screen is separate for each case:
`max(three new child ru_maxrss) / min(three old child ru_maxrss) <= 0.90`.
Warmups are excluded. Raw native values remain available even when a screen
fails. Neither this threshold nor a success certifies external-reference quality
or overall R1 acceptance.

Three preliminary supervised processes record GCC version, compile the native
launcher with warnings denied, and calibrate it. Calibration holds and touches
256MiB in a Python parent while the launcher forks allocation children of1MiB
and64MiB. Fixed gates require parent VmRSS at least200MiB, native launcher VmRSS
below16MiB, positive small-child peak below32MiB, large-child peak48–96MiB and
more than32MiB above the small child. The launcher's inherited getrusage peak is
recorded without a threshold. The portable checker independently verifies these
numeric gates and their raw report hashes; a `passed` string alone is insufficient.
No calibration retry, adjustment or sample replacement is permitted.

The calibration script must run through the campaign's pinned supervisor and
outer cgroup. Its Python timeout terminates the immediate native launcher, while
the outer supervisor owns the entire inherited process group and drains or
terminates surviving descendants. No child creates another session/group.
On root exit with descendants, the supervisor allows5seconds to drain, sends
SIGINT, then uses5second grace/kill bounds and verifies empty containment.
Its finalizer also cleans up on record/measurement exceptions. Direct standalone
calibration invocation is not a supported containment guarantee.

Every stage has the original300second timeout,10GiB sampled RSS limit,1GiB
free-memory and4GiB disk reserves,20ms polling and5second grace/kill limits.
The outer unit must provide12GiB/swap0,CPUWeight100 and the original four-CPU
boot. The fixed campaign deadline is no later than2026-09-26T21:20:00Z; each
launch requires its full timeout plus20seconds remaining. The existing VM STOP
deadline21:54:51Z and budget remain external constraints; these scripts do not
change cloud resources or reservations.

## Commands and retention

Install this directory beside the frozen `final-pipeline`, `showdown-kernel`
and `exact-mass` helper directories. Python and GCC identities are retained;
compiler executables are not copied. No Cargo build or production edit occurs.

```sh
python3 -B /opt/r1/final-control/focused-memory/run.py --phase run \
  --out /opt/r1/focused-memory03 --reference-proof /opt/r1/final-proof02 \
  --reference-archive /ABSOLUTE/final-proof02.tar.gz --cc /usr/bin/gcc \
  --deadline-utc 2026-09-26T21:20:00Z
python3 -B /TRUSTED/focused-memory/run.py --phase check \
  --out /LOCAL/focused-memory03 --reference-proof /LOCAL/final-proof02
python3 -B experiments/hu-postflop-r1/focused-memory/test_run.py
```

The checker executes only trusted checkout code. Proof02 is a separately retained
external dependency and is fully rechecked; its top-level original byte pins are
fixed in this protocol. Its source archives, binaries and inputs used here are
also preserved in the new CAS, but its entire historical raw CAS is not duplicated.
`--reference-archive` records an identity-only locator for the separately recovered
bundle; the focused checker validates the supplied proof directory and fixed
reference bytes, not that archive locator or an archive-to-directory linkage.
Verify the external bundle with its own recovery checker before extraction/use.
New reports retain all source/launcher identities, plans, command stdout/stderr,
samples, native reports, quality artifacts and failure suffixes. Failures remain
terminal and cannot carry a completed summary.
For an early preparation failure, the checker verifies all available retained CAS
bytes and returns `provenance_complete=false`; it does not assert complete source,
plan or measurement bindings. Recovery-bundle integrity is still checked separately.

Use [the recovery packer](../cloud/bundle-final-proof.py) after all writers stop.
This schema intentionally has no `build.json`: compiler stages and native binary
identity are in `result.json`, while original solver builds are in proof02.
The generic recovery manifest therefore reports only `build.json` as missing
on a complete focused campaign; do not manufacture a build record. Run this
campaign's checker, not the final-pipeline checker, on the recovered directory.
