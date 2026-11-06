#!/bin/bash
# B4 Simple L1 quality variants (relative buckets, texture+recall refinement) on a c2d-highcpu-32.
set -u
R=~/results
mkdir -p $R ~/src ~/ehs2
echo "$(date -u +%T) SETUP_START" >> $R/progress.txt
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
tar -xf ~/src.tar -C ~/src
cd ~/src
cargo build --release -p mw-preflop --example trunk_solve > $R/build.log 2>&1
rustc --version >> $R/build.log
sha256sum target/release/examples/trunk_solve >> $R/build.log
lscpu > $R/lscpu.txt; free -g >> $R/lscpu.txt
S=target/release/examples/trunk_solve
B4=examples/bench/6max_100bb_nl50_partial_simple_reference.toml
E32=~/ehs2/v2-f32-t32-r32.postcard
E128=~/ehs2/v2-f128-t128-r128.postcard
# Build the 32 and 128 EHS² tables and T2/T3 (1 iteration each).
$S --config examples/bench/hu_20bb_postflop.toml --leaf-model l1 --ehs2-cache $E32 --iterations 1 --eval-every 0 > $R/tables32.log 2>&1
sed -E "/^\[solver\.abstraction\.buckets\]/,/^\[/ s/^(flop|turn|river) = 32$/\1 = 128/" \
  examples/bench/hu_20bb_postflop.toml > /tmp/b6-128.toml
[ "$(grep -cE '^(flop|turn|river) = 128$' /tmp/b6-128.toml)" = 3 ] || echo "WARN bucket replacement failed" >> $R/progress.txt
$S --config /tmp/b6-128.toml --leaf-model l1 --ehs2-cache $E128 --iterations 1 --eval-every 0 > $R/tables128.log 2>&1
sha256sum ~/ehs2/* .cache/p2-trunk/*.bin >> $R/tables.log
echo "$(date -u +%T) SETUP_DONE" >> $R/progress.txt
run() { # name iterations every args...
  local n=$1 it=$2 ev=$3; shift 3
  echo "$(date -u +%T) START $n" >> $R/progress.txt
  /usr/bin/time -v -o $R/$n.time $S --config $B4 --leaf-model l1 --ehs2-cache $E32 --solver-k4-samples 256 \
    --solver-k4-min-samples 16 --iterations $it --eval-every $ev --l1-eval-boards 1024 --print-every 50 \
    --output $R/$n.json --output-profile $R/$n.profile.json --output-postflop $R/$n.postflop.bin "$@" > $R/$n.log 2>&1
  echo "$(date -u +%T) END $n $?" >> $R/progress.txt
}
run b4s-rel32 2000 1000 --l1-relative 32:$E128
run b4s-t1r4 2000 1000 --l1-refine texture:1,recall:4
echo "$(date -u +%T) ALL_DONE" >> $R/progress.txt
