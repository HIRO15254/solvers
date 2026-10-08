#!/bin/bash
# VM6 phase D: perf profile of the f32 default (T10 build) and thread scaling on Flop1.
R=~/results/prof; mkdir -p $R
log() { echo "$(date -u +%T) $*" >> ~/results/progress.txt; }
until grep -q PHASEC_DONE ~/results/progress.txt 2>/dev/null; do sleep 30; done
source ~/.cargo/env
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq linux-perf > $R/apt.log 2>&1
sudo sysctl -w kernel.perf_event_paranoid=-1 kernel.kptr_restrict=0 >> $R/apt.log 2>&1
B=~/t10/target/release/examples/p1_bench
(cd ~/t10 && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) > $R/build.log 2>&1
for t in 1 2 4 8 16 32; do
  it=20; [ $t -le 2 ] && it=3; [ $t -eq 4 ] && it=6
  $B ~/cfg/flop1_f32.toml --storage f32 --threads $t --warmup 1 --iters $it --evals 1 > $R/scale_t$t.json 2>/dev/null
done
log "prof scaling"
for t in 32 1; do
  it=30; [ $t -eq 1 ] && it=4
  perf record -F 999 -o /tmp/perf_t$t.data $B ~/cfg/flop1_f32.toml --storage f32 --threads $t --warmup 1 --iters $it --evals 0 > $R/perf_bench_t$t.json 2> $R/perf_record_t$t.err
  perf report -i /tmp/perf_t$t.data --stdio --no-children --sort symbol --percent-limit 0.3 > $R/perf_flop1_t$t.txt 2>/dev/null
  perf report -i /tmp/perf_t$t.data --stdio --no-children --sort dso --percent-limit 0.3 > $R/perf_flop1_t${t}_dso.txt 2>/dev/null
  rm -f /tmp/perf_t$t.data
done
perf record -F 499 -o /tmp/perf_g.data $B ~/cfg/c_gtowb.toml --storage f32 --threads 32 --warmup 1 --iters 8 --evals 0 > $R/perf_bench_gtowb.json 2> $R/perf_record_gtowb.err
perf report -i /tmp/perf_g.data --stdio --no-children --sort symbol --percent-limit 0.3 > $R/perf_gtowb_t32.txt 2>/dev/null
rm -f /tmp/perf_g.data
perf stat -e task-clock,context-switches,cpu-migrations,page-faults -o $R/perf_stat_t32.txt $B ~/cfg/flop1_f32.toml --storage f32 --threads 32 --warmup 1 --iters 20 --evals 0 > /dev/null 2>&1
echo PHASED_DONE >> ~/results/progress.txt
