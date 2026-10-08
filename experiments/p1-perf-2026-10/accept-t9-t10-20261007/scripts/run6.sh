#!/bin/bash
# VM6: T9 acceptance (new = 31a8b7a default f32 / new64 = cfr_precision f64 / base = 06ddca2),
# DCFR parameter sweep (sweep.py) and gtow_b convergence.
cd ~
R=~/results; mkdir -p $R/bench $R/conv
log() { echo "$(date -u +%T) $*" >> $R/progress.txt; }
until grep -q SETUP_DONE $R/progress.txt 2>/dev/null; do sleep 15; done
source ~/.cargo/env
bin() { [ "$1" = base ] && echo ~/base/target/release || echo ~/new/target/release; }
cfgf() { [ "$1" = new64 ] && echo ~/cfg/${2}_f64.toml || echo ~/cfg/$2.toml; }
bench() { # variant cfg storage threads warmup iters tag
  $(bin $1)/examples/p1_bench $(cfgf $1 $2) --storage $3 --threads $4 --warmup $5 --iters $6 --evals 1 > $R/bench/$7.json 2> $R/bench/$7.err
  log "bench $7"
}
solve() { # variant cfg threads tag
  rm -rf /tmp/run_$4
  /usr/bin/time -v $(bin $1)/solvers solve $(cfgf $1 $2) --out /tmp/run_$4 --threads $3 > $R/conv/$4.out 2> $R/conv/$4.err
  cp /tmp/run_$4/progress.jsonl $R/conv/$4.progress.jsonl 2>/dev/null
  cp /tmp/run_$4/run.json $R/conv/$4.run.json 2>/dev/null
  rm -rf /tmp/run_$4
  log "solve $4"
}
for round in 1 2; do
  for st in f32 i16-f32avg; do
    for v in new32 new64 base; do bench $v flop1_f32 $st 32 2 20 flop1_${st}_t32_${v}_r$round; done
  done
done
for v in new32 new64; do bench $v flop1_f32 f32 1 1 3 flop1_f32_t1_$v; done
for v in new32 new64; do bench $v c_gtowb f32 32 2 10 gtowb_f32_t32_$v; done
echo BENCH_DONE >> $R/progress.txt
for round in 1 2; do
  for t in c_turn c_flop1; do
    for v in new32 new64 base; do solve $v $t 32 ${t}_${v}_r$round; done
  done
done
solve new32 c_turn 8 c_turn_new32_t8
echo CONV_DONE >> $R/progress.txt
python3 ~/sweep.py > $R/sweep.log 2>&1
for v in new32 new64; do solve $v c_gtowb 32 c_gtowb_$v; done
echo GTOWB_DONE >> $R/progress.txt
# gtow_b with the best sweep candidate when it beat the default on Flop1 by 5% or more.
python3 - <<'PY'
import json, os
b = json.load(open(os.path.expanduser("~/results/sweep/best.json")))
if b["best_ratio"] < 0.95:
    t = open(os.path.expanduser("~/cfg/c_gtowb.toml")).read()
    head, rest = t.split("[solver.algorithm]\n", 1)
    rest = rest[rest.index("[solver.stop]"):]
    open(os.path.expanduser("~/cfg/c_gtowb_best.toml"), "w").write(head + "[solver.algorithm]\n" + b["best_algo"] + "\n" + rest)
PY
[ -f ~/cfg/c_gtowb_best.toml ] && solve new32 c_gtowb_best 32 c_gtowb_best
echo GTOWB_BEST_DONE >> $R/progress.txt
(cd ~/new && cargo test --workspace > $R/test_new.log 2>&1; echo "TEST_EXIT $?" >> $R/test_new.log)
grep -E "^test result" $R/test_new.log | awk '{p+=$4; f+=$6} END {print "new tests passed",p,"failed",f}' >> $R/progress.txt
echo ALL6_DONE >> $R/progress.txt
