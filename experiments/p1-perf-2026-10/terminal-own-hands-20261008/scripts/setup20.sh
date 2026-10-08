#!/bin/bash
# VM8 setup: HEAD a2dcbf2 ("base") and base + Part A of T20 ("t20a"), then run20.py for both.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time linux-perf >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
mkdir -p ~/cfg ~/results/vm20a && tar -xzf ~/cfg.tgz -C ~/cfg --strip-components=1
mkdir -p ~/base && tar -xzf ~/src-base.tgz -C ~/base
cp -r ~/base ~/t20a && (cd ~/t20a && git apply ~/t20a.patch)
# Same criterion bench file on both sides (Part A's file builds on the base through its adapter).
cp ~/t20a/crates/hu-postflop/benches/kernels.rs ~/base/crates/hu-postflop/benches/kernels.rs
for side in base t20a; do
  (cd ~/$side && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) >> ~/results/vm20a/build.log 2>&1
done
lscpu | head -20 > ~/results/vm20a/lscpu.txt; free -g >> ~/results/vm20a/lscpu.txt
echo "$(date -u +%T) SETUP20_DONE" >> ~/results/progress.txt
python3 ~/run20.py vm20a base t20a > ~/results/vm20a/run20.log 2>&1
