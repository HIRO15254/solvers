#!/bin/bash
# GCP c2d-highcpu-32 Spot VM (Ubuntu 24.04): C1 (i16 kernels) + C2 (exact evaluation batches)
# acceptance against the base revision at 32 and 16 threads, then the template-tree measurements
# with the new binary. Inputs in ~: src-old.tar, src-new.tar, cfg-accept.tgz, cfg-templates.tgz.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
mkdir -p ~/old ~/new ~/cfg ~/tcfg ~/results/accept ~/results/templates
tar -xf ~/src-old.tar -C ~/old && tar -xf ~/src-new.tar -C ~/new
tar -xzf ~/cfg-accept.tgz -C ~/cfg && tar -xzf ~/cfg-templates.tgz -C ~/tcfg
for s in old new; do
  (cd ~/$s && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) >> ~/results/build.log 2>&1
done
lscpu | head -20 > ~/results/lscpu.txt; free -g >> ~/results/lscpu.txt
echo "$(date -u +%T) BUILT" >> ~/results/progress.txt
set +e
R=~/results/accept
# 1. p1_bench: per-iteration and per-evaluation time, alternating old/new.
for rep in 1 2; do
  for cfg in c_flop1 c_gtowb; do
    for st in f32 i16; do
      for t in 32 16; do
        for s in old new; do
          ~/$s/target/release/examples/p1_bench ~/cfg/$cfg.toml --threads $t --warmup 3 --iters 10 --evals 2 \
            --storage $st --json $R/bench_${cfg}_${st}_t${t}_${s}_$rep.json > /dev/null 2> $R/bench_${cfg}_${st}_t${t}_${s}_$rep.err
        done
      done
    done
  done
  echo "$(date -u +%T) bench rep $rep" >> ~/results/progress.txt
done
# 2. Solves to 0.1% pot (default auto check_every), alternating old/new.
solve() {  # side cfg threads storage tag
  local c=~/work/$5.toml d=~/work/$5_$1
  mkdir -p ~/work
  sed -e "s/^storage = .*/storage = \"$4\"/" ~/cfg/$2.toml > $c
  sed -i -e '/^max_iterations/d' -e '/^check_every/d' -e 's/^target = .*/target = "0.1%pot"/' $c
  grep -q '^\[run\]' $c && sed -i 's/^\[run\]/[run]\nfinal_checkpoint = false/' $c || printf '\n[run]\nfinal_checkpoint = false\n' >> $c
  rm -rf $d
  /usr/bin/time -v ~/$1/target/release/solvers solve $c --out $d --threads $3 > $R/solve_$5_$1.log 2> $R/solve_$5_$1.time
  cp $d/progress.jsonl $R/solve_$5_$1.progress.jsonl; cp $d/run.json $R/solve_$5_$1.run.json
  rm -f $d/solution.sol $d/*.ckpt
  echo "$(date -u +%T) solve $5 $1" >> ~/results/progress.txt
}
for t in 32 16; do
  for s in old new; do solve $s c_flop1 $t f32 flop1_f32_t$t; done
  for s in old new; do solve $s c_flop1 $t i16 flop1_i16_t$t; done
  for s in old new; do solve $s c_gtowb $t f32 gtowb_f32_t$t; done
done
for s in old new; do solve $s c_gtowb 32 i16 gtowb_i16_t32; done
echo "$(date -u +%T) ACCEPT_DONE" >> ~/results/progress.txt
# 3. Template trees with the new binary.
T=~/results/templates
S=~/new/target/release/solvers
run() {  # name threads
  local d=~/work/$1_t$2; rm -rf $d
  /usr/bin/time -v $S solve ~/tcfg/$1.toml --out $d --threads $2 > $T/$1_t$2.log 2> $T/$1_t$2.time
  cp $d/progress.jsonl $T/$1_t$2.progress.jsonl; cp $d/run.json $T/$1_t$2.run.json
  echo "$(date -u +%T) template $1_t$2" >> ~/results/progress.txt
}
for t in 32 16; do
  for n in t3 t2 t1one srp_single srp_general gw_single t1s flop_srp; do
    run $n $t
    if [ $n = gw_single ] && [ $t = 32 ]; then
      for node in "" "x" "xx[3c]" "xx[3c]x" "xx[3c]xx[8d]"; do
        tag=$(echo "root$node" | tr -c 'A-Za-z0-9\n' '_')
        $S export ~/work/gw_single_t32/solution.sol actions --node "$node" --format json > $T/gw_actions_$tag.json 2>/dev/null
        $S export ~/work/gw_single_t32/solution.sol strategy --node "$node" --format csv > $T/gw_strategy_$tag.csv 2>/dev/null
      done
      $S export ~/work/gw_single_t32/solution.sol summary --format json > $T/gw_summary.json 2>/dev/null
      $S export ~/work/gw_single_t32/solution.sol range --node "x" --format csv > $T/gw_range_x.csv 2>/dev/null
    fi
    rm -f ~/work/${n}_t$t/solution.sol ~/work/${n}_t$t/*.ckpt
  done
done
echo "$(date -u +%T) ALL_DONE" >> ~/results/progress.txt
