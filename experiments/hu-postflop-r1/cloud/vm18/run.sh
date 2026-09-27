#!/bin/bash
set -euo pipefail
test "$#" -eq 2
mode=$1
case "$mode" in build|measure) ;; *) exit 2 ;; esac
package=/opt/r1/flop-chance-grain-package
controls=$package/experiments/hu-postflop-r1/flop-scaling/chance-grain
helper=$package/experiments/hu-postflop-r1/cloud/vm18
proof=/opt/r1/flop-chance-grain-proof01
wrapper=/opt/r1/flop-chance-grain-$mode-wrapper01
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
        with p.open('rb') as f: os.fsync(f.fileno())
        raw=p.read_bytes(); files[p.relative_to(root).as_posix()]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}
record={'schema':'r1.chance-grain-vm18-wrapper/v1','ended_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'last_phase':sys.argv[2],'exit_code':int(sys.argv[3]),'files':files,
        'scope':'Wrapper terminal status; partial outputs remain for recovery'}
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
export RUSTFLAGS='-C target-cpu=x86-64-v3'
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_ENCODED_RUSTFLAGS
python3 -B "$helper/start.py" --verify-only > "$wrapper/preflight.stdout.log" 2> "$wrapper/preflight.stderr.log"
if test "$mode" = build; then
  test "$(getconf _NPROCESSORS_ONLN)" -eq 2
  phase=fetch
  (cd "$package/source-baseline" && timeout --kill-after=10s 120s "$toolchain/cargo" fetch --locked --target x86_64-unknown-linux-gnu) \
    > "$wrapper/fetch.stdout.log" 2> "$wrapper/fetch.stderr.log"
  phase=prepare
  python3 -B "$controls/run.py" prepare --source "$package/source-baseline" \
    --workspace /opt/r1/flop-chance-grain-work01 --out "$proof" \
    --cargo "$toolchain/cargo" --rustc "$toolchain/rustc" --build-deadline-utc "$2" \
    > "$wrapper/prepare.stdout.log" 2> "$wrapper/prepare.stderr.log"
  phase=build
  python3 -B "$controls/run.py" build --out "$proof" > "$wrapper/build.stdout.log" 2> "$wrapper/build.stderr.log"
else
  test "$(getconf _NPROCESSORS_ONLN)" -eq 32
  phase=measure-prepare
  python3 -B "$controls/run.py" measure-prepare --out "$proof" --measurement-deadline-utc "$2" \
    > "$wrapper/prepare.stdout.log" 2> "$wrapper/prepare.stderr.log"
  phase=measure
  python3 -B "$controls/run.py" measure --out "$proof" > "$wrapper/measure.stdout.log" 2> "$wrapper/measure.stderr.log"
fi
