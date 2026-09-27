#!/bin/bash
set -euo pipefail
test "$#" -eq 1
package=/opt/r1/flop-flat-cloud32-package
controls=$package/experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32
helper=$package/experiments/hu-postflop-r1/cloud/vm14
proof=/opt/r1/flop-flat-cloud32-proof01
wrapper=/opt/r1/flop-flat-cloud32-wrapper01
toolchain=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin
mkdir "$wrapper"
phase=preflight
finish() {
  status=$?
  trap - EXIT
  set +e
  python3 -B - "$wrapper" "$phase" "$status" <<'PY'
import datetime,hashlib,json,sys
from pathlib import Path
root=Path(sys.argv[1]); files={}
for p in sorted(root.rglob('*')):
    if p.is_file():
        raw=p.read_bytes(); files[p.relative_to(root).as_posix()]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}
record={'schema':'r1.flat-ev-vm14-wrapper/v1','ended_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'last_phase':sys.argv[2],'exit_code':int(sys.argv[3]),'files':files,
        'scope':'Wrapper terminal status; stage failures and partial proof remain for recovery'}
with (root/'finish.json').open('x') as f: json.dump(record,f,indent=2)
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
record=json.loads(Path('/opt/r1/flop-flat-cloud32-start01.json').read_text())
assert record['boot_id']==Path('/proc/sys/kernel/random/boot_id').read_text().strip()
PY
python3 -B "$helper/start.py" --verify-only > "$wrapper/preflight.stdout.log" 2> "$wrapper/preflight.stderr.log"
phase=prepare
python3 -B "$controls/runner.py" prepare --baseline-source "$package/source-baseline" \
  --flat-source "$package/source-flat" --workspace /opt/r1/flop-flat-cloud32-work01 --out "$proof" \
  --cargo "$toolchain/cargo" --rustc "$toolchain/rustc" --deadline-utc "$1" \
  > "$wrapper/prepare.stdout.log" 2> "$wrapper/prepare.stderr.log"
phase=build
python3 -B "$controls/runner.py" build --out "$proof" > "$wrapper/build.stdout.log" 2> "$wrapper/build.stderr.log"
phase=matrix
python3 -B "$controls/runner.py" matrix --out "$proof" > "$wrapper/matrix.stdout.log" 2> "$wrapper/matrix.stderr.log"
