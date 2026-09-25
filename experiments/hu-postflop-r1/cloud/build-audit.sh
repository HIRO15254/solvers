#!/bin/bash
set -euxo pipefail
export CARGO_HOME=/opt/r1/cargo
export RUSTUP_HOME=/opt/r1/rustup
export RUSTUP_TOOLCHAIN=1.97.0
export PATH=/opt/r1/cargo/bin:$PATH
export CARGO_BUILD_JOBS=4
cd /opt/r1/audit-source
python3 /home/PC_User/validate-source.py /opt/r1/audit-source /opt/r1/validation-audit
export CARGO_TARGET_DIR=/opt/r1/target/audit
{
    rustc -Vv
    cargo -V
    sha256sum Cargo.lock .cargo/config.toml /opt/r1/audit-source.tar.gz
    /usr/bin/time -v cargo build --locked --release -p cli --example hu_saved_profile_audit
    sha256sum /opt/r1/target/audit/release/examples/hu_saved_profile_audit
    date -u --iso-8601=seconds
} > /opt/r1/audit-build.log 2>&1
date -u --iso-8601=seconds > /opt/r1/build-audit-complete
