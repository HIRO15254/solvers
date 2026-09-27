# VM14 deployment controls

One existing Spot VM starts as e2-standard-2 for bootstrap and dependency fetch,
then is stopped/resized to 32 CPUs. These helpers do not create, resize, delete,
or extend a VM. The parent task owns the $3 reservation, original absolute cloud
STOP (at most75min), installation, dispatch and final resource cleanup.

Install the pinned research package at `/opt/r1/flop-flat-cloud32-package`.
Helpers belong at its `experiments/hu-postflop-r1/cloud/vm14` directory. Run the
existing `cloud/bootstrap.sh` to install1.97.0, then `fetch-dependencies.sh` on
the2CPU VM. Fetch is `cargo fetch --locked`; no compilation occurs before resize.

After32CPU boot and package verification, dispatch:

```sh
python3 -B /opt/r1/flop-flat-cloud32-package/experiments/hu-postflop-r1/cloud/vm14/start.py \
  --deadline-utc EXPERIMENT_UTC --stop-deadline-utc ORIGINAL_CLOUD_STOP_UTC
```

The experiment deadline must be within40min, leave at least15min before the
original cloud STOP, and still have more than1,220seconds at dispatch. One fresh
unit `solvers-r1-vm14-flat32` has12GiB memory, zero swap, CPUWeight100,
KillMode=control-group and RuntimeMaxSec20seconds inside the experiment deadline.
The run checks boot identity and queried service limits before preparing,
building and executing the matrix sequentially. Both source trees, adapter,
controls and the installed package manifest are checked before execution.
Rustc/cargo use the real1.97.0 toolchain executables. The runner retains stage
failures and partial outputs; the wrapper retains phase stdout/stderr and exit.
A forced SIGKILL may prevent its finalizer, which recovery records honestly.

After the unit is inactive/failed, MainPID is0 and all remaining cgroup process
lists are empty, run `bash .../vm14/recover.sh`. It retains service/journal/start
records, the package, wrapper logs and the entire available proof, including a
failure before proof creation. Archive contents keep the original proof layout.
All archived bytes are reread and matched to file hashes before publishing the
checksum sidecar. Archive plus sidecars must fit1GiB. No original inputs or
outputs are deleted, and existing recovery outputs are never overwritten.
Recovery-byte integrity is separate from experiment success/quality acceptance.

Remote outputs are `/tmp/flop-flat-cloud32-proof01.tar.gz`, its `.manifest.json`
and `.sha256` sidecars, and `/tmp/flop-flat-cloud32-recovery01.json`.
`capture-command.py` records one explicit gcloud command locally, with no retries.
Timeouts require inspecting cloud state before any subsequent mutation.

Local preparation checks Python/embedded-Python syntax. The attempted Bash
syntax check could not initialize Windows MSYS (CreateFileMapping error5), so
run `bash -n` on all three shell scripts on the small VM before deployment.
No Cargo, native solve, cloud command or Linux systemd was executed locally.
