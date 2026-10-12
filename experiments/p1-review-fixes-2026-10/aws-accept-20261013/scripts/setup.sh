#!/bin/bash
# AWS c6a.8xlarge Spot (Ubuntu 24.04): P1 review fixes acceptance at 32 and 16 threads.
# Variants: old (main before the fixes), new (review fixes), s5 (review fixes + storage-order batch).
# Inputs in ~: src-old.tar, src-new.tar, src-s5.tar, cfg-accept.tgz.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
V="old new s5"
mkdir -p ~/cfg ~/results/accept ~/work
for s in $V; do mkdir -p ~/$s && tar -xf ~/src-$s.tar -C ~/$s; done
tar -xzf ~/cfg-accept.tgz -C ~/cfg
for s in $V; do
  (cd ~/$s && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) >> ~/results/build.log 2>&1
done
lscpu | head -20 > ~/results/lscpu.txt; free -g >> ~/results/lscpu.txt
echo "$(date -u +%T) BUILT" >> ~/results/progress.txt
set +e
R=~/results/accept
# 1. p1_bench: per-iteration time, rotating variant order per repetition.
for rep in 1 2; do
  for cfg in c_flop1 c_gtowb; do
    for st in f32 i16; do
      for t in 32 16; do
        for s in $V; do
          ~/$s/target/release/examples/p1_bench ~/cfg/$cfg.toml --threads $t --warmup 3 --iters 10 --evals 2 \
            --storage $st --json $R/bench_${cfg}_${st}_t${t}_${s}_$rep.json > /dev/null 2> $R/bench_${cfg}_${st}_t${t}_${s}_$rep.err
        done
      done
    done
  done
  V="s5 new old"
  echo "$(date -u +%T) bench rep $rep" >> ~/results/progress.txt
done
V="old new s5"
# 2. Solves to 0.1% pot (default auto check_every), variants back to back.
solve() {  # side cfg threads storage tag
  local c=~/work/$5.toml d=~/work/$5_$1
  sed -e "s/^storage = .*/storage = \"$4\"/" ~/cfg/$2.toml > $c
  sed -i -e '/^max_iterations/d' -e '/^check_every/d' -e 's/^target = .*/target = "0.1%pot"/' $c
  grep -q '^\[run\]' $c && sed -i 's/^\[run\]/[run]\nfinal_checkpoint = false/' $c || printf '\n[run]\nfinal_checkpoint = false\n' >> $c
  rm -rf $d
  /usr/bin/time -v ~/$1/target/release/solvers solve $c --out $d --threads $3 > $R/solve_$5_$1.log 2> $R/solve_$5_$1.time
  cp $d/progress.jsonl $R/solve_$5_$1.progress.jsonl; cp $d/run.json $R/solve_$5_$1.run.json
  rm -rf $d
  echo "$(date -u +%T) solve $5 $1" >> ~/results/progress.txt
}
for t in 32 16; do
  for s in $V; do solve $s c_flop1 $t f32 flop1_f32_t$t; done
  for s in $V; do solve $s c_flop1 $t i16 flop1_i16_t$t; done
done
for s in $V; do solve $s c_gtowb 32 f32 gtowb_f32_t32; done
echo "$(date -u +%T) ALL_DONE" >> ~/results/progress.txt
