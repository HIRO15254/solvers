#!/bin/bash
# VM7 setup: build T13 (64788f5, "old") and the T14-T16 head ("new") side by side, then run run7.py.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time linux-perf >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
mkdir -p ~/cfg ~/results/vm7 && tar -xzf ~/cfg.tgz -C ~/cfg --strip-components=1
for side in old new; do
  mkdir -p ~/$side && tar -xzf ~/src-$side.tgz -C ~/$side
  (cd ~/$side && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) >> ~/results/vm7/build.log 2>&1
done
lscpu | head -20 > ~/results/vm7/lscpu.txt; free -g >> ~/results/vm7/lscpu.txt
cat /sys/kernel/mm/transparent_hugepage/enabled >> ~/results/vm7/lscpu.txt
echo "$(date -u +%T) SETUP_DONE" >> ~/results/progress.txt
python3 ~/run7.py > ~/results/vm7/run7.log 2>&1
