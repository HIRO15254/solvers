#!/bin/bash
set -euo pipefail
test "$#" -eq 1
test "$(getconf _NPROCESSORS_ONLN)" -eq 32
test -f /opt/r1/bootstrap-complete
export RUSTUP_HOME=/opt/r1/rustup CARGO_HOME=/opt/r1/cargo PYTHONDONTWRITEBYTECODE=1
export RUSTC=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/rustc
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_ENCODED_RUSTFLAGS
runner=/opt/r1/current-control/current-scaling32/run.py
proof=/opt/r1/current32-proof01
toolchain=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin
verification=/opt/r1/current32-verification01
test ! -e "$verification.json"
test ! -e "$verification.stderr.log"
test ! -e "$verification.exit"
cg=/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)
test -f "$cg/cpu.max"
test "$(cat "$cg/cpu.weight")" = 100
# EXIT retains a check even when prepare/build/measure fails. The unit's fixed
# RuntimeMaxSec remains the upper bound; SIGKILL cannot guarantee a finalizer.
finish() {
  status=$?
  trap - EXIT
  set +e
  python3 -B "$runner" --phase check --out "$proof" --foundation-proof /opt/r1/exact-proof04 > "$verification.json" 2> "$verification.stderr.log"
  checked=$?
  printf '%s\n' "$checked" > "$verification.exit"
  if test "$status" -eq 0; then status=$checked; fi
  exit "$status"
}
trap finish EXIT
python3 -B /opt/r1/current-deployment01/install-inputs.py --verify-only
python3 -B "$runner" --phase prepare --root /opt/r1/current32 --target /opt/r1/target/current32-proof01 \
  --foundation-proof /opt/r1/exact-proof04 --cargo "$toolchain/cargo" --rustc "$toolchain/rustc" \
  --out "$proof" --deadline-utc "$1"
python3 -B "$runner" --phase build --out "$proof"
python3 -B "$runner" --phase measure --out "$proof"
