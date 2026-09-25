#!/bin/bash
set -euxo pipefail
export CARGO_HOME=/opt/r1/cargo
export RUSTUP_HOME=/opt/r1/rustup
export RUSTUP_TOOLCHAIN=1.97.0
export PATH=/opt/r1/cargo/bin:$PATH
export CARGO_BUILD_JOBS=4
cd /opt/r1/current
python3 /home/PC_User/validate-source.py /opt/r1/current /opt/r1/validation-05
python3 -m unittest discover -s experiments/hu-postflop-r1/pipeline -p test_run_campaign.py -v > /opt/r1/pipeline-tests.log 2>&1
/bin/bash /home/PC_User/build-baseline.sh > /opt/r1/baseline-build-pinned.log 2>&1
export CARGO_TARGET_DIR=/opt/r1/target/candidate
{
    rustc -Vv
    cargo -V
    uname -a
    lscpu
    free -b
    sha256sum Cargo.lock .cargo/config.toml /opt/r1/candidate-source.tar.gz
    /usr/bin/time -v cargo build --locked --release -p cli
    sha256sum /opt/r1/target/candidate/release/solvers
    date -u --iso-8601=seconds
} > /opt/r1/candidate-build.log 2>&1
date -u --iso-8601=seconds > /opt/r1/build-validation-complete
