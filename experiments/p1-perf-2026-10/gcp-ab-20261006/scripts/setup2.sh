#!/bin/bash
# T4 VM setup: two source trees (base = 43e97c6, new = candidate) built side by side.
# Expects ~/src-base.tgz, ~/src-new.tgz, ~/cfg.tgz in $HOME.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config linux-perf git python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
mkdir -p ~/cfg && tar -xzf ~/cfg.tgz -C ~/cfg --strip-components=1
for side in base new; do
  mkdir -p ~/$side && tar -xzf ~/src-$side.tgz -C ~/$side
  (cd ~/$side && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1 \
    && cargo build --release -p cli --bin solvers 2>&1 | tail -1)
done
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only bash -c 'cd ~/new && cargo build --release -p hu-postflop --example p1_bench --target-dir target-prof 2>&1 | tail -1'
lscpu | head -20
free -g
echo SETUP_DONE
