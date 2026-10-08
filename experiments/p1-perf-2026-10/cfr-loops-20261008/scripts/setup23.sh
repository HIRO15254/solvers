#!/bin/bash
# VM8 phase 3 (replaces setup22 after `perf report --sort srcline` stalled): software cpu-clock profiles of base
# and T20 A+B at 32 and 16 threads; per-symbol reports, then `perf annotate -l` source-line summaries of the
# hottest symbols. Uses the line-tables-only builds from setup22 (~/prof_base, ~/prof_t20ab).
R=~/results/prof22
mkdir -p $R
prof() {  # side cfg tag threads warmup iters
  local d=~/perf_$3.data
  if [ ! -s $R/report_$3.txt ]; then
    perf record -e cpu-clock -F 499 -o $d -- ~/prof_$1/release/examples/p1_bench ~/work_vm20a/$2.toml \
      --threads $4 --warmup $5 --iters $6 --evals 0 --json $R/bench_$3.json > /dev/null 2> $R/perf_$3.err
    perf report -i $d --no-children --sort symbol --stdio 2> /dev/null | head -120 > $R/report_$3.txt
  fi
  perf report -i $d --no-children --sort dso --stdio 2> /dev/null | head -40 > $R/dso_$3.txt
  # Top four symbols by overhead (names as perf prints them).
  perf report -i $d --no-children --sort symbol --stdio -F overhead,sym 2> /dev/null | grep -E '^ +[0-9]' | head -4 \
    | sed -E 's/^ +[0-9.]+% +\[[.k]\] +//; s/ +$//' > $R/top_$3.txt
  local i=0
  while IFS= read -r sym; do
    i=$((i + 1))
    timeout 600 perf annotate -i $d --stdio -l -s "$sym" 2> /dev/null > ~/ann_$3_$i.txt
    # Keep the source-line summary and the 60 hottest instructions.
    { echo "## $sym"; sed -n '1,/Percent/p' ~/ann_$3_$i.txt | head -80; echo "## hottest instructions";
      grep -E '^ +[0-9]+\.[0-9]+ +:' ~/ann_$3_$i.txt | sort -rn | head -60; } > $R/annotate_$3_$i.txt
  done < $R/top_$3.txt
  echo "$(date -u +%T) prof $3 done" >> ~/results/progress.txt
}
prof base c_flop1 base_flop1_t32 32 15 40
prof t20ab c_flop1 t20ab_flop1_t32 32 15 40
prof base c_flop1 base_flop1_t16 16 15 40
prof t20ab c_flop1 t20ab_flop1_t16 16 15 40
prof t20ab c_gtowb t20ab_gtowb_t32 32 3 8
echo "$(date -u +%T) PROF22_DONE" >> ~/results/progress.txt
