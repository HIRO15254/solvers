#!/bin/bash
# B4 Simple with GTO Wizard Simple's preflop menus (~/b4s-gtow.toml from gtow_tree.py), L1 with the 8x2
# range-relative buckets, 8 hours of solve on a c2d-highcpu-32. Idempotent: after a Spot preemption the watcher
# restarts the VM and this script, which skips the finished setup and resumes the solve from ~/state/b4s.state.
set -u
R=~/results
mkdir -p $R ~/src ~/ehs2 ~/state ~/snap ~/final
echo "$(date -u +%T) JOB_START" >> $R/progress.txt
if ! grep -q SETUP_DONE $R/progress.txt; then
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
  $S --config examples/bench/hu_20bb_postflop.toml --leaf-model l1 --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard \
    --iterations 1 --eval-every 0 > $R/tables32.log 2>&1
  sed -E "/^\[solver\.abstraction\.buckets\]/,/^\[/ s/^(flop|turn|river) = 32$/\1 = 128/" \
    examples/bench/hu_20bb_postflop.toml > /tmp/b6-128.toml
  $S --config /tmp/b6-128.toml --leaf-model l1 --ehs2-cache ~/ehs2/v2-f128-t128-r128.postcard \
    --iterations 1 --eval-every 0 > $R/tables128.log 2>&1
  sha256sum ~/ehs2/* >> $R/tables.log
  echo "$(date -u +%T) SETUP_DONE" >> $R/progress.txt
fi
source ~/.cargo/env
cd ~/src
n=$(grep -c "START b4s-8h" $R/progress.txt)
echo "$(date -u +%T) START b4s-8h run$n" >> $R/progress.txt
/usr/bin/time -v -o $R/b4s-8h.run$n.time target/release/examples/trunk_solve \
  --config ~/b4s-gtow.toml --leaf-model l1 \
  --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard --l1-relative 8x2:$HOME/ehs2/v2-f128-t128-r128.postcard \
  --solver-k4-samples 256 --solver-k4-min-samples 16 --l1-eval-boards 1024 \
  --iterations 1000000 --eval-every 1000 --print-every 100 --max-seconds 28800 \
  --state ~/state/b4s.state --state-every 1200 --snapshot-dir ~/snap \
  --output $R/b4s-8h.json --output-profile ~/final/b4s-8h.profile.json --output-postflop ~/final/b4s-8h.postflop.bin \
  > $R/b4s-8h.run$n.log 2>&1
code=$?
echo "$(date -u +%T) END b4s-8h run$n $code" >> $R/progress.txt
if [ $code = 0 ]; then
  (cd ~ && sha256sum snap/* final/* > $R/artifacts.sha256 && tar -czf ~/final.tgz snap final)
  echo "$(date -u +%T) ALL_DONE" >> $R/progress.txt
fi
