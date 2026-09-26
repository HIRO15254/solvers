# VM12 cleanup guard

`cleanup.py` is restricted to project `solvers-abstraction-20260723`, zone
`us-central1-b`, and instance `solvers-r1-20260926-12`. The default is read-only
inspection. No cloud operations run during its mocked local tests.

The instance ID is read from the preserved **original gcloud create JSON**, not
supplied freely. `--launch-record` must contain a single-instance array with the
expected name, a positive numeric ID and the exact expected boot-disk source
link. The launch projection may omit `selfLink` and `zone`; if present they must
match. Its disk source must identify this project, zone and instance-named disk.
Original launch bytes and SHA-256 are copied into the new receipt directory.

```powershell
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/cleanup-vm12/cleanup.py --mode inspect --launch-record E:/RECOVERY/launch-create.stdout.json --out E:/codex-work/solvers/r1-vm12-inspect
```

Before deletion the caller must confirm that all VM services and evidence writers
are quiesced, intended work is finished, and **every attempt, including the latest
failure**, has been recovered. Local evidence does not establish the live service
state or discover unsupplied attempts. Keep recovered directories unchanged and
write new verification results outside them.

```powershell
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/cleanup-vm12/cleanup.py --mode delete --launch-record E:/RECOVERY/launch-create.stdout.json --foundation-proof E:/EXACT04/proof --evidence E:/CURRENT32/proof E:/CURRENT32/current32.tar.gz --out E:/codex-work/solvers/r1-vm12-cleanup
```

Repeat `--evidence EXTRACTED_CURRENT32 ARCHIVE` for every retained attempt. Each
archive requires adjacent `.manifest.json` and `.sha256`. The trusted recovery
checker verifies all members and the extracted directory's exact file set and
bytes. The trusted checkout's `current-scaling32/run.py --phase check` must return
schema `r1.current-scaling32-verification/v1`, terminal `completed` or `failed`,
and `payload_integrity=verified`. Retained code is never executed. A verified
failure permits cleanup without implying performance success; incomplete
provenance remains recorded as such.

The original extracted exact-mass04 foundation is required. Its plan SHA-256 is
fixed to `8763df23d4c86a295ffb4728409d19311f36c0dcb38aaa58e9d77fb3b823a2eb`;
the current-scaling checker verifies the original eight validation stages when
the campaign has a complete plan. An early prepare failure can only establish
available retained-byte integrity, which the cleanup receipt distinguishes.
The foundation plan is also pinned independently by this guard, including that
early failure path. All local checks finish before any cloud operation.

Fresh instance describe must match the launch ID and exact project/zone/name.
Exactly one persistent40GiB boot disk with `autoDelete=true` is required. A fresh
disk describe must show the expected link and this instance as its sole user.
Only that named instance is deleted, using the verified automatic disk policy;
there are no disk-policy overrides, separate disk deletes or reservation changes.
The command intent, raw output, exit/timeout and selected-field readbacks are
preserved. An uncertain deletion is never retried automatically.

Reconciled success requires **all four** conditions: empty instance list, empty
disk list, empty reserved-address list for this VM's name and observed IPs, and
a matching delete operation with the exact instance ID/link/zone, `DONE` and no
error. Every readback is attempted even when another fails. Anything less is
`deletion_not_fully_reconciled`; it requires read-only investigation.

Python/SDK paths and UTF-8 encoding are fixed. Receipt output must be a new
directory outside proofs, the foundation and archive files. The script performs
no filesystem deletion, releases no held budget and makes no billing-zero claim.

```powershell
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/cleanup-vm12/test_cleanup.py
```

Tests use synthetic archives and mocked cloud/process calls; their captured
command, stdout/stderr and result are retained in `local-checks.json`.
