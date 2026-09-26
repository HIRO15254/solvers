# VM11 cleanup guard

`cleanup.py` is restricted to project `solvers-abstraction-20260723`, zone
`us-central1-b`, instance `solvers-r1-20260926-11`, ID `570856080701499920`.
Its default mode only describes that instance and disk with selected fields.
No cloud operation was performed while implementing or testing this script.

```powershell
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/cleanup-vm11/cleanup.py --mode inspect --out E:/codex-work/solvers/r1-vm11-inspect
```

Before deletion, the caller must separately confirm that the current VM service
and all evidence writers are quiesced, recover **every attempt including the
latest one**, and verify that the intended work is finished. Local proof bytes
cannot establish the live service state. Keep extraction directories unchanged;
save any new verification output outside them.

```powershell
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/cleanup-vm11/cleanup.py --mode delete --out E:/codex-work/solvers/r1-vm11-cleanup --evidence E:/RECOVERED01/proof E:/RECOVERED01/final-proof01.tar.gz --evidence E:/RECOVERED02/proof E:/RECOVERED02/final-proof02.tar.gz
```

When focused-memory attempts also exist, append one
`--focused-evidence EXTRACTED_DIR ARCHIVE REFERENCE_PROOF_DIR` for **each** retained
focused attempt. For example, append
`--focused-evidence E:/MEMORY01/proof E:/MEMORY01/memory-proof01.tar.gz E:/RECOVERED02/proof`
to the deletion command above. The reference directory must already be among the
successfully validated `--evidence` entries with final status `completed`. The
trusted checkout's `focused-memory/run.py --phase check` verifies the explicit
reference against its fixed proof02 identities; evidence code is never executed.
Focused evidence must also have exact archive/extracted bytes, adjacent sidecars,
and terminal `completed` or `failed` status with `payload_integrity=verified`.
All focused checks run before any cloud call. Failure blocks deletion; a verified
failed experiment can be cleaned up without making a performance claim.
The original `--evidence` option and default inspection mode are unchanged.

Each archive needs adjacent `.manifest.json` and `.sha256` sidecars. The trusted
checkout's recovery checker verifies every archive member; the extracted directory
must have the exact same files and bytes. The trusted final-pipeline checker must
return a terminal `completed` or `failed` report with verified payload integrity.
Evidence code is never imported. Python/SDK paths and encoding are fixed in the
script; its command records preserve these choices.

Immediately before the only mutation, the script rechecks the fixed instance ID,
one persistent 40 GiB boot disk, `autoDelete=true`, and that disk's sole user.
It deletes only the instance, using its existing auto-delete setting. This follows
the [gcloud delete contract](https://docs.cloud.google.com/sdk/gcloud/reference/compute/instances/delete)
without `--delete-disks` or `--keep-disks` overrides. It retains command intent,
raw stdout/stderr, exit/timeout, and then filtered instance/disk/reserved-address
lists and delete operations. A matching instance ID/link/zone, `DONE` operation
without an error, and empty resource lists are all needed for reconciled success.
Address scope is the exact VM name and external IPs observed immediately before
deletion. An uncertain deletion is never automatically retried.

The script does not delete local files, change reservations, release held budget,
or interpret resource absence as zero billing. Receipt outputs must be a new
directory, outside final proofs, focused proofs, and their explicit references.
`test_cleanup.py` has 16 synthetic tests; all cloud/process calls are mocked.
Small synthetic archives exercise byte-integrity guards. The successful local
agent run took 0.233 seconds after a sandbox temporary-directory permission failure.
The root's separately retained [test03 result](test03-result.json) records16 passes
in0.213 seconds; [test02](test02-result.json) preserves the earlier sandbox ACL
failure. These are mocked tests, not cloud execution logs.
