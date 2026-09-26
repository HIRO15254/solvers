#!/bin/bash
set -euo pipefail
test "$#" -eq 0
test "$(getconf _NPROCESSORS_ONLN)" -eq 4
test -f /opt/r1/bootstrap-complete
test ! -e /opt/r1/current-phase-work01
test ! -e /opt/r1/current-phase-proof01
package=/opt/r1/phase-deployment01
python3 -B "$package/install.py" --verify-only --destination "$package"
cd "$package/source"
# Network population only; both measured targets are built fresh and offline.
env -i PATH=/usr/bin:/bin HOME=/root LANG=C.UTF-8 LC_ALL=C.UTF-8 TZ=UTC \
  RUSTUP_HOME=/opt/r1/rustup CARGO_HOME=/opt/r1/cargo RUSTUP_TOOLCHAIN=1.97.0 CARGO_BUILD_JOBS=2 \
  RUSTC=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/rustc \
  /usr/bin/timeout --signal=TERM --kill-after=10s 180s \
  /opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/cargo fetch --locked --target x86_64-unknown-linux-gnu
test ! -e /opt/r1/current-phase-work01
