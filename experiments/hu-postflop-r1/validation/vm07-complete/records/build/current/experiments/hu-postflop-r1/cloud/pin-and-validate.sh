#!/bin/bash
set -euxo pipefail
export CARGO_HOME=/opt/r1/cargo
export RUSTUP_HOME=/opt/r1/rustup
export PATH=/opt/r1/cargo/bin:$PATH
rustup toolchain install 1.97.0 --profile minimal --component clippy,rustfmt
/bin/bash /home/PC_User/build-baseline.sh > /opt/r1/baseline-build-pinned.log 2>&1
python3 /home/PC_User/validate-source.py /opt/r1/current /opt/r1/validation-02
