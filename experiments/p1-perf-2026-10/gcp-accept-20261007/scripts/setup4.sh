#!/bin/bash
# VM4 setup: sources t3b (bedfb89), a (06415d2), b (candidate) built side by side; b also runs the workspace tests.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
rustup component add rustfmt clippy >/dev/null 2>&1 || true
mkdir -p ~/cfg ~/results && tar -xzf ~/cfg.tgz -C ~/cfg --strip-components=1
for side in t3b a b; do
  mkdir -p ~/$side && tar -xzf ~/src-$side.tgz -C ~/$side
  (cd ~/$side && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example verify_save 2>&1 | tail -1)
done
(cd ~/b && cargo test --workspace > ~/results/test_b.log 2>&1; echo "TEST_EXIT $?" >> ~/results/test_b.log)
lscpu | head -20 > ~/results/lscpu.txt; free -g >> ~/results/lscpu.txt
echo SETUP_DONE
