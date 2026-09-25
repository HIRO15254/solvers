#!/bin/bash
set -euxo pipefail
export CARGO_HOME=/opt/r1/cargo
export RUSTUP_HOME=/opt/r1/rustup
export RUSTUP_TOOLCHAIN=1.97.0
export PATH=/opt/r1/cargo/bin:$PATH
export CARGO_BUILD_JOBS=4
export CARGO_TARGET_DIR=/opt/r1/target/baseline
cd /opt/r1/baseline
rustc -Vv
cargo -V
uname -a
lscpu
free -b
sha256sum Cargo.lock .cargo/config.toml /opt/r1/baseline-source.tar.gz
/usr/bin/time -v cargo build --locked --release -p cli
sha256sum /opt/r1/target/baseline/release/solvers
date -u --iso-8601=seconds
