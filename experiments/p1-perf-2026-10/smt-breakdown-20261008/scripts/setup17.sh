#!/bin/bash
# VM7 phase 11 (throwaway diagnostic, after DIAG_DONE): which parts of the f32 terminal kernels limit 16/32 threads.
source ~/.cargo/env
until grep -q DIAG_DONE ~/results/progress.txt; do sleep 10; done
mkdir -p ~/results/vm14
python3 ~/diag2_patch.py ~/diag > ~/results/vm14/build.log 2>&1
(cd ~/diag && cargo build --release -p hu-postflop --example p1_bench --target-dir ~/diagt 2>&1 | tail -3) >> ~/results/vm14/build.log 2>&1
B=~/diagt/release/examples/p1_bench
for rep in 1 2; do
  for t in 32 16; do
    for d in 6 14 22 38 70 30 7; do
      P1DIAG=$d $B ~/work11/c_flop1.toml --threads $t --warmup 100 --iters 20 --evals 0 --json ~/results/vm14/diag2_flop1_t${t}_d${d}_$rep.json > /dev/null 2>&1
      echo "$(date -u +%T) diag2 flop1 t$t d$d r$rep $(python3 -c "import json;print(round(json.load(open('$HOME/results/vm14/diag2_flop1_t${t}_d${d}_$rep.json'))['secsPerIter'],4))")" >> ~/results/progress.txt
    done
  done
done
echo "$(date -u +%T) DIAG2_DONE" >> ~/results/progress.txt
