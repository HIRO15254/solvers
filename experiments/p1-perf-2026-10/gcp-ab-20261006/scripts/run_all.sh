#!/bin/bash
# T4 session A driver: A/B (base 43e97c6 vs new) then new-only large trees and a profile.
cd ~
until grep -q SETUP_DONE ~/setup.log; do sleep 10; done
source ~/.cargo/env
mkdir -p ~/results
log() { echo "$(date -u +%T) $*" >> ~/results/progress.txt; }
sudo sysctl -qw kernel.perf_event_paranoid=-1
log start
bash ~/ab.sh turn turn_multi.toml 5 "1 4 8 16 32" 2; log turn
bash ~/ab.sh flop1 flop_srp1.toml 3 "1 8 16 32" 2; log flop1
bash ~/ab.sh flop1_i16 flop_srp1.toml 3 "16 32" 1 "--storage i16"; log flop1_i16
bash ~/cli_ab.sh flop1 flop_srp1_cli.toml 16; log cli
B=~/new/target/release/examples/p1_bench
$B ~/cfg/flop_srp2.toml --threads 32 --warmup 1 --iters 2 --evals 1 >> ~/results/large_new.jsonl 2>>~/results/large.err; log srp2
$B ~/cfg/gtow_b.toml --threads 32 --warmup 1 --iters 2 --evals 1 >> ~/results/large_new.jsonl 2>>~/results/large.err; log gtow_b
$B ~/cfg/gtow_a.toml --threads 32 --warmup 1 --iters 2 --evals 1 --storage i16 >> ~/results/large_new.jsonl 2>>~/results/large.err; log gtow_a
P=~/new/target-prof/release/examples/p1_bench
for c in turn_multi flop_srp1; do
  perf record -F 499 -g --call-graph dwarf,16384 -o /tmp/perf_$c.data $P ~/cfg/$c.toml --threads 1 --warmup 0 --iters 2 --evals 1 > ~/results/prof_$c.json 2>/dev/null
  perf report -i /tmp/perf_$c.data --no-children --sort symbol --stdio 2>/dev/null | grep -E "^ +[0-9]+\.[0-9]+%" | head -40 > ~/results/prof_$c.txt
done
log prof
echo ALL_DONE >> ~/results/progress.txt
