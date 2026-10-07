#!/bin/bash
# VM7 phase 10 (throwaway diagnostic, after zstats): time removed by skipping terminal kernels (1),
# regret updates (2) and strategy-sum accumulation (4) after warmup, at 16 and 32 threads (T16 head).
source ~/.cargo/env
until grep -q ZSTATS_DONE ~/results/progress.txt; do sleep 20; done
mkdir -p ~/results/vm13
rm -rf ~/diag && cp -r ~/new ~/diag && rm -rf ~/diag/target && python3 ~/diag_patch.py ~/diag > ~/results/vm13/build.log 2>&1
(cd ~/diag && cargo build --release -p hu-postflop --example p1_bench --target-dir ~/diagt 2>&1 | tail -3) >> ~/results/vm13/build.log 2>&1
B=~/diagt/release/examples/p1_bench
for rep in 1 2; do
  for t in 32 16; do
    for d in 0 1 2 4 6 7; do
      P1DIAG=$d $B ~/work11/c_flop1.toml --threads $t --warmup 100 --iters 20 --evals 0 --json ~/results/vm13/diag_flop1_t${t}_d${d}_$rep.json > /dev/null 2>&1
      echo "$(date -u +%T) diag flop1 t$t d$d r$rep $(python3 -c "import json;print(round(json.load(open('$HOME/results/vm13/diag_flop1_t${t}_d${d}_$rep.json'))['secsPerIter'],4))")" >> ~/results/progress.txt
    done
  done
done
echo "$(date -u +%T) DIAG_DONE" >> ~/results/progress.txt
