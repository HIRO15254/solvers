#!/bin/bash
set -euo pipefail
test "$#" -eq 0
test "$(getconf _NPROCESSORS_ONLN)" -eq 2
test -f /opt/r1/bootstrap-complete
package=/opt/r1/flop-cpu-occupancy-package
test ! -e /opt/r1/flop-cpu-occupancy-work01
test ! -e /opt/r1/flop-cpu-occupancy-proof01
export RUSTUP_HOME=/opt/r1/rustup CARGO_HOME=/opt/r1/cargo PYTHONDONTWRITEBYTECODE=1
export RUSTC=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/rustc
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_ENCODED_RUSTFLAGS
python3 -B "$package/experiments/hu-postflop-r1/cloud/vm16/start.py" --verify-only
cd "$package/source-baseline"
# Baseline source only. Fetch; native build follows resize.
/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/cargo fetch --locked --target x86_64-unknown-linux-gnu
test ! -e "$package/source-baseline/target"
test ! -e /opt/r1/flop-cpu-occupancy-work01
