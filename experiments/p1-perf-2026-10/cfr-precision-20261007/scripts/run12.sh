#!/bin/bash
# VM5 tail 2: bit-identical T8a kernel candidates (e2 = prototype + t8a_l/lb/lw/lbw) vs exact and f64fold.
R=~/results; mkdir -p $R/t8a/bench $R/t8a/conv
log() { echo "$(date -u +%T) $*" >> $R/progress.txt; }
until grep -q TEST_DONE $R/progress.txt 2>/dev/null; do sleep 20; done
source ~/.cargo/env
mkdir -p ~/e2 && tar -xzf ~/src-e2.tgz -C ~/e2
(cd ~/e2 && cargo build --release -p cli --bin solvers 2>&1 | tail -3 && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -3) > $R/t8a/build.log 2>&1
mkdir -p ~/kbw && tar -xzf ~/kbench.tgz -C ~/kbw && (cd ~/kbw/kb && RUSTFLAGS="-C target-cpu=native" cargo build --release --bin t8a 2>&1 | tail -1 && ./target/release/t8a) > $R/t8a/kbench.txt 2>&1
log "t8a build+kbench"
B=~/e2/target/release/examples/p1_bench
S=~/e2/target/release/solvers
for v in t8a_l t8a_lb t8a_lw t8a_lbw exact; do
  rm -rf /tmp/t8; SOLVERS_P1_KERNEL=$v $S solve ~/cfg/c_turn.toml --out /tmp/t8 --threads 32 > $R/t8a/conv/turn_$v.out 2>&1
  cp /tmp/t8/progress.jsonl $R/t8a/conv/turn_$v.progress.jsonl; rm -rf /tmp/t8
done
for v in t8a_lbw exact; do
  rm -rf /tmp/t8; SOLVERS_P1_KERNEL=$v $S solve ~/cfg/c_flop1.toml --out /tmp/t8 --threads 32 > $R/t8a/conv/flop1_$v.out 2>&1
  cp /tmp/t8/progress.jsonl $R/t8a/conv/flop1_$v.progress.jsonl; rm -rf /tmp/t8
done
log "t8a conv"
VS="exact f64fold t8a_l t8a_lb t8a_lw t8a_lbw"
for round in 1 2 3; do
  for st in f32 i16-f32avg; do
    for v in $VS; do
      SOLVERS_P1_KERNEL=$v $B ~/cfg/flop1_f32.toml --storage $st --threads 32 --warmup 2 --iters 20 --evals 1 > $R/t8a/bench/flop1_${st}_t32_${v}_r$round.json 2>/dev/null
    done
  done
  for v in $VS; do
    SOLVERS_P1_KERNEL=$v $B ~/cfg/c_turn.toml --storage f32 --threads 32 --warmup 20 --iters 200 --evals 1 > $R/t8a/bench/turn_f32_t32_${v}_r$round.json 2>/dev/null
  done
  log "t8a bench round $round"
done
for v in exact f64fold t8a_l t8a_lbw; do
  SOLVERS_P1_KERNEL=$v $B ~/cfg/flop1_f32.toml --storage f32 --threads 1 --warmup 1 --iters 3 --evals 1 > $R/t8a/bench/flop1_f32_t1_${v}.json 2>/dev/null
done
for v in exact f64fold t8a_l t8a_lbw; do
  SOLVERS_P1_KERNEL=$v $B ~/cfg/c_gtowb.toml --storage f32 --threads 32 --warmup 2 --iters 10 --evals 1 > $R/t8a/bench/gtowb_f32_t32_${v}.json 2>/dev/null
done
echo T8A_DONE >> $R/progress.txt
