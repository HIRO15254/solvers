#!/bin/bash
# VM7 phase 3 (throwaway instrumentation): zero opponent reach at f32 CFR terminals.
source ~/.cargo/env
until grep -q ANNOTATE_DONE ~/results/progress.txt; do sleep 20; done
rm -rf ~/t17k && cp -r ~/t17 ~/t17k && rm -rf ~/t17k/target && python3 ~/kstats_patch.py ~/t17k > ~/results/vm8/kstats_build.log 2>&1
(cd ~/t17k && cargo build --release -p hu-postflop --example p1_bench --target-dir ~/t17dbg 2>&1 | tail -3) >> ~/results/vm8/kstats_build.log 2>&1
B=~/t17dbg/release/examples/p1_bench
for spec in "c_flop1 20 10" "c_flop1 200 10" "c_turn2 300 30" "c_flop3 150 10" "c_river 300 50" "c_gtowb 250 5"; do
  set -- $spec
  $B ~/work8/$1.toml --threads 16 --warmup $2 --iters $3 --evals 0 --json ~/results/vm8/kstats_$1_w$2.json 2> ~/results/vm8/kstats_$1_w$2.err > /dev/null
  echo "$(date -u +%T) kstats $1 w$2 $(grep KSTATS ~/results/vm8/kstats_$1_w$2.err)" >> ~/results/progress.txt
done
echo "$(date -u +%T) KSTATS_DONE" >> ~/results/progress.txt
