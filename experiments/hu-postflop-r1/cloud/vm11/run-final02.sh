#!/bin/bash
set -euo pipefail
test "$#" -eq 1
test -f /opt/r1/bootstrap-complete
export RUSTUP_HOME=/opt/r1/rustup
export CARGO_HOME=/opt/r1/cargo
export PYTHONDONTWRITEBYTECODE=1
runner=/opt/r1/final-control/final-pipeline/run.py
proof=/opt/r1/final-proof02
toolchain=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin
# CPUWeight=100 on this unit must materialize cpu.max before host pinning.
cg=/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)
test -f "$cg/cpu.max"
test "$(cat "$cg/cpu.weight")" = 100
cat "$cg/cpu.max"
python3 -B "$runner" --phase prepare \
  --old-root /opt/r1/final-old01 --new-root /opt/r1/final-new01 \
  --old-target /opt/r1/target/final-old02 --new-target /opt/r1/target/final-new02 \
  --out "$proof" --deadline-utc "$1" \
  --cargo "$toolchain/cargo" --rustc "$toolchain/rustc" \
  --validation-proof /opt/r1/final-foundation
python3 -B "$runner" --phase build --out "$proof"
python3 -B "$runner" --phase measure --out "$proof"
python3 -B /opt/r1/final-control/final-pipeline/verify.py \
  --out "$proof" --expect completed > /opt/r1/final-verification02.json
