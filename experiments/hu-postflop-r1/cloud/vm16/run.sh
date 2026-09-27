#!/bin/bash
set -euo pipefail
test "$#" -eq 1
package=/opt/r1/flop-cpu-occupancy-package
controls=$package/experiments/hu-postflop-r1/flop-scaling/cpu-occupancy
helper=$package/experiments/hu-postflop-r1/cloud/vm16
proof=/opt/r1/flop-cpu-occupancy-proof01
wrapper=/opt/r1/flop-cpu-occupancy-wrapper01
toolchain=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin
mkdir "$wrapper"
phase=preflight
finish() {
  status=$?
  trap - EXIT
  set +e
  python3 -B - "$wrapper" "$phase" "$status" <<'PY'
import datetime,hashlib,json,os,sys
from pathlib import Path
root=Path(sys.argv[1]); files={}
for p in sorted(root.rglob('*')):
    if p.is_file():
        raw=p.read_bytes(); files[p.relative_to(root).as_posix()]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}
record={'schema':'r1.cpu-occupancy-vm16-wrapper/v1','ended_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'last_phase':sys.argv[2],'exit_code':int(sys.argv[3]),'files':files,
        'scope':'Wrapper terminal status; failed stages and partial proof remain for recovery'}
with (root/'finish.json').open('x') as f:
    json.dump(record,f,indent=2); f.flush(); os.fsync(f.fileno())
fd=os.open(root,os.O_RDONLY|os.O_DIRECTORY)
try: os.fsync(fd)
finally: os.close(fd)
PY
  finished=$?
  if test "$status" -eq 0; then status=$finished; fi
  exit "$status"
}
trap finish EXIT
export RUSTUP_HOME=/opt/r1/rustup CARGO_HOME=/opt/r1/cargo PYTHONDONTWRITEBYTECODE=1
export RUSTC=$toolchain/rustc
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_ENCODED_RUSTFLAGS
test "$(getconf _NPROCESSORS_ONLN)" -eq 32
cg=/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)
test "$(cat "$cg/memory.max")" = 12884901888
test "$(cat "$cg/memory.swap.max")" = 0
test "$(cat "$cg/cpu.weight")" = 100
python3 -B - <<'PY'
import json
from pathlib import Path
record=json.loads(Path('/opt/r1/flop-cpu-occupancy-start01.json').read_text())
assert record['boot_id']==Path('/proc/sys/kernel/random/boot_id').read_text().strip()
PY
python3 -B "$helper/start.py" --verify-only > "$wrapper/preflight.stdout.log" 2> "$wrapper/preflight.stderr.log"
phase=prepare
python3 -B "$controls/run.py" prepare --source "$package/source-baseline" \
  --workspace /opt/r1/flop-cpu-occupancy-work01 --out "$proof" \
  --cargo "$toolchain/cargo" --rustc "$toolchain/rustc" --taskset /usr/bin/taskset --deadline-utc "$1" \
  > "$wrapper/prepare.stdout.log" 2> "$wrapper/prepare.stderr.log"
phase=execute
python3 -B "$controls/run.py" execute --out "$proof" > "$wrapper/execute.stdout.log" 2> "$wrapper/execute.stderr.log"
