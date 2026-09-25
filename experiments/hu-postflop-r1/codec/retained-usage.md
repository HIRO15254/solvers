# Relocated codec evidence verification

`verify-retained.py` verifies local collector archives after the VM is removed.
It does not execute a solver, compiler, or archived script. The one imported
validator is the local frozen `run_codec.py`, required to have SHA-256
`6c8ca8e49f6c20320d4140a8b1322e0c62b3d37de1b26968e4270970d6eba79f`.
Only its filesystem operations are mapped to retained bytes; its protocol,
plan, FileRefs, original Linux path strings and comparison are not modified.

Provide each archive and its independently retained SHA-256 using matching labels.
Its downloaded `.manifest.json` and `.sha256` sidecars must be adjacent. The final
codec archive must include all 96 samples, canonical files, rewritten SOL files,
plan, attestation, selection, inputs and runner. The build/checks archives supply
source archives, full source manifests, build logs, compiler and binaries. The
resolved Python, cargo and bash executable bytes must also be included. Missing
runtime bytes are rejected just like missing canonical bytes; a hash declaration
alone does not supply a file. Include a separate runtime collector bundle if needed.

```text
python experiments/hu-postflop-r1/codec/verify-retained.py --bundle checks=LOCAL-CHECKS.tar.gz --sha256 checks=ACTUAL64HEX --bundle build=LOCAL-BUILD.tar.gz --sha256 build=ACTUAL64HEX --bundle codec=LOCAL-CODEC.tar.gz --sha256 codec=ACTUAL64HEX --run-root /opt/r1/ACTUAL-CAMPAIGN-DIRECTORY --out NEW-REPORT.json
python experiments/hu-postflop-r1/codec/test_verify_retained.py -v
python experiments/hu-postflop-r1/codec/retain-compact.py --report VERIFIED-REPORT.json --out NEW-COMPACT-DIRECTORY
```

Use `--inventory-only` instead of `--run-root` before the last bundle arrives to
list direct FileRefs missing from the current inventory. That result is explicitly
an inventory report, never a successful campaign verification. The result file
must be new. Failed verification raises an error and publishes no success report.

Every regular archive payload is stream-hashed against its embedded and downloaded
manifest. Unlisted, duplicate, missing, nonregular or unsafe members fail. Original
paths are mapped to archive bytes, never used as extraction destinations. If two
bundles contain different versions at the same original path, an unqualified read
is ambiguous and fails; the verifier does not choose the latest version.

An anonymous temporary disk file holds the decoded bytes and is removed on close.
Canonical equality uses 1 MiB blocks, avoiding a multi-gigabyte in-memory dictionary.
Allow disk space for all decoded bundles (about 2 GiB for the anticipated campaign,
plus build/runtime files); the hard decoded bound is 8 GiB and the per-member bound
is 2 GiB. JSON and source-archive reads are separately bounded at 32 MiB. A spool
disk failure cannot produce a pass. No large canonical output is copied into Git.

The verifier rechecks the plan self-digest and protocol, every referenced source,
tool, binary, input and build-stage identity, all 96 supervised results, full/root
canonical equality and rewritten-original SOL byte equality. It additionally binds
source archives to the exact build source-file manifests, permitting only the
identical research example as an extra baseline file. It checks build → plan →
ordered, nonoverlapping samples → completion timestamps and recorded resource
limits, then independently recomputes all 12 three-pair medians, ratios and fixed
descriptive gates. The report lists every required reference's retained location.

A pass confirms retained evidence and codec byte equality within this protocol.
It does not independently prove compilation, estimate statistical confidence,
recompute saved strategy BR, establish end-to-end solve speed, or certify R1.
Process memory measurements include hashing/canonical work; no codec-phase memory
claim follows. The source06 input metadata is still an earlier pre-save snapshot.

`retain-compact.py` repeats full verification before copying selected original
plans, source tools and JSON/log records into a new compact directory. Its index
links every required byte to a local bundle and distinguishes selected compact
copies from bundle-only canonical/SOL/binary/archive/source bytes. It neither
deletes the bundles nor labels uncommitted compact files as Git-backed.
