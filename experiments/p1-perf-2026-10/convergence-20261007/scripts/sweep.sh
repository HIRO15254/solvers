#!/bin/bash
# T4 session B tail: DCFR parameter sweep after run_conv.sh finishes.
cd ~
until grep -q ALL_DONE ~/results/progress.txt; do sleep 20; done
source ~/.cargo/env
mkdir -p ~/results/sweep
log() { echo "$(date -u +%T) $*" >> ~/results/progress.txt; }
S=~/new/target/release/solvers
run() {
  rm -rf /tmp/sw_$1
  /usr/bin/time -v $S solve ~/sweep/$1.toml --out /tmp/sw_$1 --threads 32 > ~/results/sweep/$1.out 2> ~/results/sweep/$1.err
  cp /tmp/sw_$1/progress.jsonl ~/results/sweep/$1.progress.jsonl
  rm -rf /tmp/sw_$1
  log "sweep $1"
}
for f in ~/sweep/turn_*.toml; do run $(basename $f .toml); done
for f in ~/sweep/flop1_*.toml; do run $(basename $f .toml); done
echo SWEEP_DONE >> ~/results/progress.txt
