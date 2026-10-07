#!/bin/bash
# VM7 phase 9 (throwaway instrumentation, after T18): storage updates under all-zero reach, and a bandwidth probe.
source ~/.cargo/env
until grep -q T18_DONE ~/results/progress.txt; do sleep 20; done
mkdir -p ~/results/vm12
gcc -O3 -march=native -fopenmp ~/stream.c -o ~/stream > ~/results/vm12/stream_build.log 2>&1
for t in 8 16 32; do OMP_NUM_THREADS=$t OMP_PROC_BIND=spread ~/stream >> ~/results/vm12/stream.txt 2>&1; done
echo "$(date -u +%T) stream $(grep threads=32 ~/results/vm12/stream.txt | tail -1)" >> ~/results/progress.txt
rm -rf ~/t18z && cp -r ~/t18 ~/t18z && rm -rf ~/t18z/target && python3 ~/zstats_patch.py ~/t18z > ~/results/vm12/zstats_build.log 2>&1
(cd ~/t18z && cargo build --release -p hu-postflop --example p1_bench --target-dir ~/zdbg 2>&1 | tail -3) >> ~/results/vm12/zstats_build.log 2>&1
B=~/zdbg/release/examples/p1_bench
for spec in "c_turn2 300 30" "c_flop1 25 10" "c_flop1 200 10" "c_flop3 150 10" "c_gtowb 100 3" "c_gtowb 250 5"; do
  set -- $spec
  $B ~/work11/$1.toml --threads 32 --warmup $2 --iters $3 --evals 0 --json ~/results/vm12/zstats_$1_w$2.json 2> ~/results/vm12/zstats_$1_w$2.err > /dev/null
  echo "$(date -u +%T) zstats $1 w$2 $(grep ZSTATS ~/results/vm12/zstats_$1_w$2.err)" >> ~/results/progress.txt
done
echo "$(date -u +%T) ZSTATS_DONE" >> ~/results/progress.txt
