#!/bin/bash
# VM8 phase 9: sampling profile of the T24 tree (line tables only; codegen unchanged), after setup30.
source ~/.cargo/env
until grep -q VM30_ALL_DONE ~/results/progress.txt; do sleep 20; done
R=~/results/prof31; mkdir -p $R
(cd ~/t24 && CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_TARGET_DIR=~/prof_t24 \
  cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) > $R/build.log 2>&1
for spec in "c_flop1 flop1_t32 32 15 40" "c_gtowb gtowb_t32 32 3 8"; do
  set -- $spec
  d=~/perf31_$2.data
  perf record -e cpu-clock -F 499 -o $d -- ~/prof_t24/release/examples/p1_bench ~/work_vm20a/$1.toml \
    --threads $3 --warmup $4 --iters $5 --evals 0 --json $R/bench_$2.json > /dev/null 2> $R/perf_$2.err
  perf report -i $d --no-children --sort symbol --stdio 2> /dev/null | head -120 > $R/report_$2.txt
  perf report -i $d --no-children --sort dso --stdio 2> /dev/null | head -40 > $R/dso_$2.txt
  grep -E '^ +[0-9.]+% +\[\.\] ' $R/report_$2.txt | sed -E 's/^ +[0-9.]+% +\[\.\] +//; s/( +-)+ *$//; s/ +$//' \
    | grep -v '^0x' | head -5 > $R/top_$2.txt
  echo "$(date -u +%T) prof31 $2 report done" >> ~/results/progress.txt
  i=0
  while IFS= read -r sym; do
    i=$((i + 1))
    timeout 600 perf annotate -i $d --stdio -l -s "$sym" 2> /dev/null > ~/ann31_$2_$i.txt
    { echo "## $sym"; sed -n '1,/Percent/p' ~/ann31_$2_$i.txt | head -100; echo "## hottest instructions";
      grep -E '^ +[0-9]+\.[0-9]+ +:' ~/ann31_$2_$i.txt | sort -rn | head -80; } > $R/annotate_$2_$i.txt
  done < $R/top_$2.txt
done
echo "$(date -u +%T) VM31_ALL_DONE" >> ~/results/progress.txt
