#!/bin/bash
# A/B timing of the trunk at 16 and 32 threads: base (main c09c0af, ~/base.tar) vs new (working tree, ~/src.tar),
# order base,new,new,base per case and thread count; plus the new binary with relative buckets (16x2) on B4 Simple.
set -u
R=~/results
mkdir -p $R ~/src ~/base ~/ehs2
echo "$(date -u +%T) SETUP_START" >> $R/progress.txt
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
tar -xf ~/src.tar -C ~/src
tar -xf ~/base.tar -C ~/base
(cd ~/src && cargo build --release -p mw-preflop --example trunk_solve) > $R/build-new.log 2>&1
(cd ~/base && cargo build --release -p mw-preflop --example trunk_solve) > $R/build-base.log 2>&1
rustc --version >> $R/build-new.log
sha256sum ~/src/target/release/examples/trunk_solve ~/base/target/release/examples/trunk_solve > $R/binaries.txt
lscpu > $R/lscpu.txt; free -g >> $R/lscpu.txt
cd ~/src
NEW=~/src/target/release/examples/trunk_solve
BASE=~/base/target/release/examples/trunk_solve
E32=~/ehs2/v2-f32-t32-r32.postcard
E128=~/ehs2/v2-f128-t128-r128.postcard
$NEW --config examples/bench/hu_20bb_postflop.toml --leaf-model l1 --ehs2-cache $E32 --iterations 1 --eval-every 0 > $R/tables32.log 2>&1
sed -E "/^\[solver\.abstraction\.buckets\]/,/^\[/ s/^(flop|turn|river) = 32$/\1 = 128/" \
  examples/bench/hu_20bb_postflop.toml > /tmp/b6-128.toml
$NEW --config /tmp/b6-128.toml --leaf-model l1 --ehs2-cache $E128 --iterations 1 --eval-every 0 > $R/tables128.log 2>&1
echo "$(date -u +%T) SETUP_DONE" >> $R/progress.txt
K4="--solver-k4-samples 256 --solver-k4-min-samples 16"
run() { # name binary threads config leaf iterations [args...]
  local n=$1 bin=$2 t=$3 cfg=$4 leaf=$5 it=$6; shift 6
  /usr/bin/time -v -o $R/$n.time $bin --config $cfg --leaf-model $leaf --ehs2-cache $E32 --iterations $it --eval-every 0 \
    --threads $t --print-every 1 --output $R/$n.json --output-profile $R/$n.profile.json "$@" > $R/$n.log 2>&1
  echo "$(date -u +%T) $n $?" >> $R/progress.txt
}
for t in 16 32; do
  for v in base-1 new-1 new-2 base-2; do
    bin=$NEW; [[ $v == base* ]] && bin=$BASE
    run b3-$v-t$t $bin $t examples/bench/6max_20bb.toml l0 12 $K4
    run b7-$v-t$t $bin $t examples/bench/6max_20bb.toml l1 8 $K4 --l1-eval-boards 64
    run b4s-$v-t$t $bin $t examples/bench/6max_100bb_nl50_partial_simple_reference.toml l1 6 $K4 --l1-eval-boards 64
  done
  run b4s-rel16x2-t$t $NEW $t examples/bench/6max_100bb_nl50_partial_simple_reference.toml l1 6 $K4 --l1-eval-boards 64 \
    --l1-relative 16x2:$E128
done
sha256sum $R/*.profile.json > $R/profiles.sha256
# Quality at half the iterations: relative 16x2 on B4 Simple, 1000 iterations, as the 2000-iteration record otherwise.
echo "$(date -u +%T) START b4s-rel16x2-1000" >> $R/progress.txt
/usr/bin/time -v -o $R/b4s-rel16x2-1000.time $NEW --config examples/bench/6max_100bb_nl50_partial_simple_reference.toml \
  --leaf-model l1 --ehs2-cache $E32 $K4 --iterations 1000 --eval-every 500 --l1-eval-boards 1024 --print-every 50 \
  --l1-relative 16x2:$E128 --output $R/b4s-rel16x2-1000.json --output-profile $R/b4s-rel16x2-1000.profile.json \
  > $R/b4s-rel16x2-1000.log 2>&1
echo "$(date -u +%T) END b4s-rel16x2-1000 $?" >> $R/progress.txt
echo "$(date -u +%T) ALL_DONE" >> $R/progress.txt
