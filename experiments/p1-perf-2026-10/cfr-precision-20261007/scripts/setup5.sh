#!/bin/bash
# VM5 setup: build the P1-T8 prototype (e) and its base 06ddca2 (base) side by side.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
mkdir -p ~/cfg ~/results && tar -xzf ~/cfg.tgz -C ~/cfg --strip-components=1
for side in e base; do
  mkdir -p ~/$side && tar -xzf ~/src-$side.tgz -C ~/$side
  (cd ~/$side && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) >> ~/results/build.log 2>&1
done
lscpu | head -20 > ~/results/lscpu.txt; free -g >> ~/results/lscpu.txt
echo SETUP_DONE >> ~/results/progress.txt
