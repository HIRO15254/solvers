# VM13 cleanup guard

`cleanup.py` defaults to read-only inspection. The fixed scope is
`solvers-abstraction-20260723 / us-central1-b / solvers-r1-20260926-13` and its
single 40 GiB persistent boot disk. It does not launch, stop, resize, alter disk
policies, remove addresses separately, or release budget reservations.

The caller first stops all writers and recovers **every attempt**. Deletion
requires the original single-instance JSON array returned by `gcloud create`,
not a typed ID or a reconstructed replacement. Each repeated `--evidence` pair
contains the extracted archive and the original archive with adjacent manifest
and checksum sidecars.

```text
python -B experiments/hu-postflop-r1/cloud/cleanup-vm13/cleanup.py \
  --mode inspect --launch-record ORIGINAL_CREATE.stdout.log --out NEW_RECEIPT

python -B experiments/hu-postflop-r1/cloud/cleanup-vm13/cleanup.py \
  --mode delete --launch-record ORIGINAL_CREATE.stdout.log --out NEW_RECEIPT \
  --evidence EXTRACTED_PROOF ORIGINAL_ARCHIVE.tar.gz
```

Before any cloud call, the guard verifies the archive checksum, every member,
the exact extracted file set and every byte. It invokes the **trusted checkout**
`current-phases/check_run.py --out EXTRACTED_PROOF` and requires a terminal
`completed` or `failed` result with verified payload integrity. It never imports
or executes recovered code. A failed campaign may be deleted after complete raw
recovery; this does not turn failure into a quality or performance success.

The old generic bundler always expects `build.json`. Current-phases stores build
records in its result, so exactly that absent legacy filename is expected. A
plan-generation failure may additionally lack `plan.json` only when the trusted
checker explicitly returns `failed_prepare`, failed status and incomplete
provenance. Other missing required files or any retention issue stop deletion.

Every archive must retain `metadata/current-phase-launch01.json` through its
recovery-manifest alias. Its instance ID must equal the original create ID and
its unit must be `solvers-r1-vm13-current-phases`; boot identity is retained. If a
plan exists, its launch object must equal these raw metadata bytes semantically.
Thus an unrelated successful proof cannot authorize deleting this VM.

Fresh cloud reads must match the original instance ID, exact project/zone/name,
one boot disk with `autoDelete=true`, and a separately described 40 GiB disk whose
sole owner is that instance. The guard issues one named-instance delete without
disk-policy overrides. It records intent before dispatch and never retries an
uncertain delete. Every absence readback is attempted: instance, disk, reserved
addresses matching the observed IP/name, and a DONE delete operation bound to the
same instance ID/link/zone with no error. Anything less stays unreconciled and
requires read-only investigation.

Receipt output is a new local directory outside the evidence. Original command
outputs, exit/timeout records, the create JSON and reconciliation are preserved.
Resource absence is not a billing-zero assertion; `billing_usd` stays null and
`reservation_released` stays false.

Missing original evidence blocks this automated delete path. It must not be used
to extend the fixed budget STOP or leave resources until the 24-hour operational
deadline: the owner must promptly inspect the failure, preserve remaining raw
evidence, and choose a documented manual cleanup if necessary. The script has no
force-delete bypass and performs no automatic retry or deadline extension.

```text
python -B experiments/hu-postflop-r1/cloud/cleanup-vm13/test_cleanup.py
```

Tests use tiny synthetic archives and mocked subprocess/cloud calls only.
