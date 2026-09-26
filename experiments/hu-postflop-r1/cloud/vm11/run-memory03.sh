#!/bin/bash
set -euo pipefail
test "$#" -eq 1
test -f /opt/r1/bootstrap-complete
export RUSTUP_HOME=/opt/r1/rustup
export CARGO_HOME=/opt/r1/cargo
export PYTHONDONTWRITEBYTECODE=1
cg=/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)
test -f "$cg/cpu.max"
test "$(cat "$cg/cpu.weight")" = 100
runner=/opt/r1/final-control/focused-memory/run.py
set +e
python3 -B "$runner" --phase run --out /opt/r1/focused-memory03 \
  --reference-proof /opt/r1/final-proof02 \
  --reference-archive /tmp/final-proof02.tar.gz --cc /usr/bin/gcc \
  --deadline-utc "$1"
run_exit=$?
python3 -B "$runner" --phase check --out /opt/r1/focused-memory03 \
  --reference-proof /opt/r1/final-proof02 \
  > /opt/r1/memory-verification03.json
check_exit=$?
if [ "$run_exit" -ne 0 ]; then exit "$run_exit"; fi
exit "$check_exit"
