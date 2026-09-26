#!/bin/bash
set -euo pipefail
test "$#" -eq 0
test "$(getconf _NPROCESSORS_ONLN)" -eq 4
test -f /opt/r1/bootstrap-complete
test ! -e /opt/r1/target/current32-proof01
test ! -e /opt/r1/current32-proof01
python3 -B /opt/r1/current-deployment01/install-inputs.py --verify-only
export RUSTUP_HOME=/opt/r1/rustup CARGO_HOME=/opt/r1/cargo
export RUSTUP_TOOLCHAIN=1.97.0 CARGO_BUILD_JOBS=2
export RUSTC=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/rustc
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_ENCODED_RUSTFLAGS
cd /opt/r1/current32/source
# Fetch only. The benchmark target remains absent until the 32-CPU build stage.
/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/cargo fetch --locked --target x86_64-unknown-linux-gnu
test ! -e /opt/r1/target/current32-proof01
