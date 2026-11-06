#!/bin/bash
# Larger L1 postflop menus on the GTO Wizard-menu B4 tree: ~/b4s-gtow-{raise,sizes,gtow}.toml from
# postflop_variants.py, each solved for 1.5 hours with the 8x2 range-relative buckets on a c2d-highcpu-32.
# Idempotent like job_8h.sh: after a preemption the finished setup and variants are skipped and the running
# variant resumes from its state file.
set -u
R=~/results
mkdir -p $R ~/src ~/ehs2 ~/state ~/final
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
for spec in raise:500 sizes:500 gtow:300; do
  v=${spec%%:*}; every=${spec##*:}
  grep -q "END $v 0" $R/progress.txt && continue
  mkdir -p ~/snap-$v
  echo "$(date -u +%T) START $v" >> $R/progress.txt
  /usr/bin/time -v -o $R/$v.$(date +%s).time target/release/examples/trunk_solve \
    --config ~/b4s-gtow-$v.toml --leaf-model l1 \
    --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard --l1-relative 8x2:$HOME/ehs2/v2-f128-t128-r128.postcard \
    --solver-k4-samples 256 --solver-k4-min-samples 16 --l1-eval-boards 1024 \
    --iterations 1000000 --eval-every $every --print-every 100 --max-seconds 5400 \
    --state ~/state/$v.state --state-every 1200 --snapshot-dir ~/snap-$v \
    --output $R/$v.json --output-profile ~/final/$v.profile.json --output-postflop ~/final/$v.postflop.bin \
    >> $R/$v.log 2>&1
  code=$?
  echo "$(date -u +%T) END $v $code" >> $R/progress.txt
  [ $code = 0 ] || exit 1
done
(cd ~ && sha256sum snap-*/* final/* > $R/artifacts.sha256 && tar -czf ~/final.tgz snap-* final)
echo "$(date -u +%T) ALL_DONE" >> $R/progress.txt
