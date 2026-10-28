#!/bin/bash
# Nodelock regression check on one VM: base = 7b6492b3 (before nodelock) vs head = 2b80338d.
# Inputs in ~: src-base.tar, src-head.tar (git archive), cfg.tar (configs/*.toml).
# Output: ~/results/reg (p1_bench JSON, solve logs + /usr/bin/time -v, perf and heaptrack summaries).
set -e
source ~/.cargo/env
V="base head"
R=~/results/reg
mkdir -p ~/cfg $R ~/work
for s in $V; do rm -rf ~/$s && mkdir -p ~/$s && tar -xf ~/src-$s.tar -C ~/$s; done
tar -xf ~/cfg.tar -C ~/cfg
for s in $V; do
  (cd ~/$s && cargo build --release -p cli --bin solvers && cargo build --release -p hu-postflop --example p1_bench) \
    > ~/results/build-$s.log 2>&1
done
{ lscpu; free -g; uname -a; rustc -V; } > ~/results/machine.txt
echo "$(date -u +%T) BUILT" >> ~/results/progress.txt
set +e

# 1. Per-iteration and evaluation time, alternating variant order per repetition.
for rep in 1 2; do
  for cfg in flop_srp flop_3bp c_gtowb; do
    for st in f32 i16; do
      for t in 32 16; do
        for s in $V; do
          ~/$s/target/release/examples/p1_bench ~/cfg/$cfg.toml --threads $t --warmup 3 --iters 10 --evals 2 \
            --storage $st --json $R/bench_${cfg}_${st}_t${t}_${s}_$rep.json > /dev/null 2> $R/bench_${cfg}_${st}_t${t}_${s}_$rep.err
        done
      done
    done
  done
  V="head base"
  echo "$(date -u +%T) bench rep $rep" >> ~/results/progress.txt
done
V="base head"

# 2. Solves to 0.1% pot (auto check_every), peak RSS from /usr/bin/time -v.
mkcfg() {  # cfg storage out
  sed -e '/^max_iterations/d' -e '/^check_every/d' -e 's/^target = .*/target = "0.1%pot"/' ~/cfg/$1.toml > $3
  if grep -q '^storage = ' $3; then sed -i "s/^storage = .*/storage = \"$2\"/" $3; else printf '\n[solver]\nstorage = "%s"\n' $2 >> $3; fi
  if grep -q '^\[run\]' $3; then sed -i 's/^\[run\]/[run]\nfinal_checkpoint = false/' $3; else printf '\n[run]\nfinal_checkpoint = false\n' >> $3; fi
}
solve() {  # variant cfg threads storage tag
  local c=~/work/$5.toml d=~/work/$5_$1
  mkcfg $2 $4 $c
  rm -rf $d
  /usr/bin/time -v ~/$1/target/release/solvers solve $c --out $d --threads $3 > $R/solve_$5_$1.log 2> $R/solve_$5_$1.time
  cp $d/progress.jsonl $R/solve_$5_$1.progress.jsonl; cp $d/run.json $R/solve_$5_$1.run.json
  rm -rf $d
  echo "$(date -u +%T) solve $5 $1" >> ~/results/progress.txt
}
for s in base head; do solve $s flop_srp 32 f32 srp_f32_t32_r1; done
for s in head base; do solve $s flop_srp 32 f32 srp_f32_t32_r2; done
for s in base head; do solve $s flop_srp 16 f32 srp_f32_t16; done
for s in head base; do solve $s flop_srp 32 i16 srp_i16_t32; done
for s in base head; do solve $s flop_3bp 32 f32 3bp_f32_t32; done

# 3. Where head spends time and memory (diagnostics only).
H=~/head/target/release
perf record -F 499 -o $R/perf_srp_f32.data $H/examples/p1_bench ~/cfg/flop_srp.toml --threads 32 --warmup 3 --iters 20 --evals 2 --storage f32 > /dev/null 2>&1
perf report -i $R/perf_srp_f32.data --no-children --sort symbol --stdio --percent-limit 0.3 2>/dev/null | grep -v '^$' > $R/perf_srp_f32.txt
perf record -F 499 -o $R/perf_srp_i16.data $H/examples/p1_bench ~/cfg/flop_srp.toml --threads 32 --warmup 3 --iters 20 --evals 2 --storage i16 > /dev/null 2>&1
perf report -i $R/perf_srp_i16.data --no-children --sort symbol --stdio --percent-limit 0.3 2>/dev/null | grep -v '^$' > $R/perf_srp_i16.txt
rm -f $R/perf_*.data
mkcfg flop_srp f32 ~/work/heap.toml
sed -i 's/^target = .*/target = "0.1%pot"\nmax_iterations = 6/' ~/work/heap.toml
rm -rf ~/work/heap_out
heaptrack -o $R/heap_srp_f32 $H/solvers solve ~/work/heap.toml --out ~/work/heap_out --threads 32 > /dev/null 2>&1
heaptrack_print -f $R/heap_srp_f32.zst --print-peaks 1 --peak-limit 30 --print-allocators 0 --print-temporary 0 --print-leaks 0 > $R/heap_srp_f32.txt 2>&1
rm -rf ~/work/heap_out
echo "$(date -u +%T) ALL_DONE" >> ~/results/progress.txt
