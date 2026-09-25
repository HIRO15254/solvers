#!/bin/bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y build-essential pkg-config libssl-dev curl git python3 time
mkdir -p /opt/r1
if [ ! -x /opt/r1/cargo/bin/rustup ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /opt/r1/rustup-init.sh
  RUSTUP_HOME=/opt/r1/rustup CARGO_HOME=/opt/r1/cargo sh /opt/r1/rustup-init.sh -y --default-toolchain 1.97.0 --profile minimal --component clippy,rustfmt --no-modify-path
fi
chmod -R a+rX /opt/r1
date -u --iso-8601=seconds > /opt/r1/bootstrap-complete
