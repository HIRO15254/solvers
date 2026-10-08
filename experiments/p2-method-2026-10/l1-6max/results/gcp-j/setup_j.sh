#!/bin/bash
# P2 VM: build trunk_solve at I (46060c6) and J (7b8d704), make the EHS² and T2/T3 tables, then run run_j.sh.
set -e
mkdir -p ~/results
echo "$(date -u +%T) SETUP_START" >> ~/results/progress.txt
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
for side in i j; do
  mkdir -p ~/$side && tar -xf ~/src-$side.tar -C ~/$side
  (cd ~/$side && cargo build --release -p mw-preflop --example trunk_solve 2>&1 | tail -1) >> ~/results/build.log
  cp ~/$side/target/release/examples/trunk_solve ~/trunk_solve-$side
done
rustc --version >> ~/results/build.log
sha256sum ~/trunk_solve-* >> ~/results/build.log
lscpu > ~/results/lscpu.txt
free -g >> ~/results/lscpu.txt
nproc >> ~/results/lscpu.txt
cd ~/j
mkdir -p ~/ehs2
# The 32-bucket EHS² table and the T2/T3 tables (under .cache/p2-trunk), built once here.
/usr/bin/time -v -o ~/results/tables.time ~/trunk_solve-i --config examples/bench/6max_20bb.toml --leaf-model l1 \
  --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard --iterations 1 --eval-every 0 > ~/results/tables.log 2>&1
sha256sum ~/ehs2/*.postcard .cache/p2-trunk/*.bin >> ~/results/tables.log
echo "$(date -u +%T) SETUP_DONE" >> ~/results/progress.txt
bash ~/run_j.sh
