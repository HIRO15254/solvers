#!/bin/bash
# VM8 phase 3: sampling profiles (software cpu-clock; the VM exposes no hardware PMU) of base and T20 A+B
# at 32 and 16 threads, after vm20b. Debug line tables only (codegen unchanged), separate target dirs.
source ~/.cargo/env
until grep -q vm20b_DONE ~/results/progress.txt; do sleep 20; done
sudo sysctl -q kernel.perf_event_paranoid=-1 kernel.kptr_restrict=0
R=~/results/prof22
mkdir -p $R
for side in base t20ab; do
  (cd ~/$side && CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_TARGET_DIR=~/prof_$side \
    cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) >> $R/build.log 2>&1
done
echo "$(date -u +%T) PROF_BUILD_DONE" >> ~/results/progress.txt
prof() {  # side cfg tag threads warmup iters
  local d=~/perf_$3.data
  perf record -e cpu-clock -F 499 -o $d -- ~/prof_$1/release/examples/p1_bench ~/work_vm20a/$2.toml \
    --threads $4 --warmup $5 --iters $6 --evals 0 --json $R/bench_$3.json > /dev/null 2> $R/perf_$3.err
  perf report -i $d --no-children --sort symbol --stdio 2> /dev/null | head -120 > $R/report_$3.txt
  perf report -i $d --no-children --sort comm --stdio 2> /dev/null | head -60 > $R/comm_$3.txt
  timeout 1200 perf report -i $d --no-children --sort srcline --stdio 2> /dev/null | head -400 > $R/srcline_$3.txt
  echo "$(date -u +%T) prof $3 done" >> ~/results/progress.txt
}
prof base c_flop1 base_flop1_t32 32 15 40
prof base c_flop1 base_flop1_t16 16 15 40
prof t20ab c_flop1 t20ab_flop1_t32 32 15 40
prof t20ab c_flop1 t20ab_flop1_t16 16 15 40
prof base c_gtowb base_gtowb_t32 32 3 8
echo "$(date -u +%T) PROF22_DONE" >> ~/results/progress.txt
