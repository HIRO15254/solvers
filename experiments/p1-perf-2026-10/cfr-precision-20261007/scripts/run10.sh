#!/bin/bash
# VM5: P1-T8 prototype variants (kernel exact|f64fold|f32 x norm exact|f32) vs base 06ddca2.
# Variant "base" runs the 06ddca2 binaries; others run the prototype with SOLVERS_P1_KERNEL/NORM.
cd ~; source ~/.cargo/env
R=~/results; mkdir -p $R/bench $R/conv
log() { echo "$(date -u +%T) $*" >> $R/progress.txt; }
until grep -q SETUP_DONE $R/progress.txt 2>/dev/null; do sleep 15; done
ALL="base exact:exact f64fold:exact f32:exact exact:f32 f32:f32"
vtag() { echo "${1/:/_}"; }
venv() { [ "$1" = base ] && echo "X=1" || echo "SOLVERS_P1_KERNEL=${1%%:*} SOLVERS_P1_NORM=${1##*:}"; }
vdir() { [ "$1" = base ] && echo ~/base || echo ~/e; }
bench() { # variant cfg storage threads warmup iters tag
  env $(venv $1) $(vdir $1)/target/release/examples/p1_bench ~/cfg/$2.toml --storage $3 --threads $4 --warmup $5 --iters $6 --evals 1 > $R/bench/$7.json 2> $R/bench/$7.err
  log "bench $7"
}
solve() { # variant cfg threads tag
  rm -rf /tmp/run_$4
  /usr/bin/time -v env $(venv $1) $(vdir $1)/target/release/solvers solve ~/cfg/$2.toml --out /tmp/run_$4 --threads $3 > $R/conv/$4.out 2> $R/conv/$4.err
  cp /tmp/run_$4/progress.jsonl $R/conv/$4.progress.jsonl 2>/dev/null; rm -rf /tmp/run_$4
  log "solve $4"
}
# 1. per-iteration bench (Flop1), alternating rounds
for round in 1 2; do
  for st in f32 i16-f32avg; do
    for v in $ALL; do bench $v flop1_f32 $st 32 2 20 flop1_${st}_t32_$(vtag $v)_r$round; done
  done
done
for v in base exact:exact f64fold:exact f32:f32; do bench $v flop1_f32 f32 1 1 3 flop1_f32_t1_$(vtag $v); done
echo BENCH_DONE >> $R/progress.txt
# 2. 0.1% convergence (Turn, Flop1), alternating rounds; Turn thread check
for round in 1 2; do
  for t in c_turn c_flop1; do
    for v in base exact:exact f64fold:exact f32:exact exact:f32 f32:f32; do solve $v $t 32 ${t}_$(vtag $v)_r$round; done
  done
done
for v in f32:f32 f64fold:exact; do solve $v c_turn 8 c_turn_$(vtag $v)_t8; done
echo CONV_DONE >> $R/progress.txt
# 3. deep targets to separate kernel and norm effects
for v in exact:exact f64fold:exact f32:exact exact:f32 f32:f32; do solve $v d_turn 32 d_turn_$(vtag $v); done
for v in exact:exact f32:exact exact:f32 f32:f32; do solve $v d_flop1 32 d_flop1_$(vtag $v); done
echo DEEP_DONE >> $R/progress.txt
# 4. gtow_b
for v in exact:exact f64fold:exact f32:exact f32:f32; do bench $v c_gtowb f32 32 2 10 gtowb_f32_t32_$(vtag $v); done
for v in f32:f32 exact:exact; do solve $v c_gtowb 32 c_gtowb_$(vtag $v); done
echo GTOWB_DONE >> $R/progress.txt
(cd ~/e && cargo test --workspace > $R/test_e.log 2>&1; echo "TEST_EXIT $?" >> $R/test_e.log)
grep -E "^test result" $R/test_e.log | awk '{p+=$4; f+=$6} END {print "e tests passed",p,"failed",f}' >> $R/progress.txt
echo ALL_DONE >> $R/progress.txt
