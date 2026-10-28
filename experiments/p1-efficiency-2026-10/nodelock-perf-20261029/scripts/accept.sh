#!/bin/bash
# Acceptance of the 2026-10-29 P1 performance changes on one Spot VM (docs/development.md 5.4/5.5):
# head = 2b80338d (nodelock + relock), new = the performance commits on top. Each variant is built
# natively on the VM. p1_bench pairs at 32 and 16 threads alternate the variant order per repetition;
# solves to 0.1% pot run back to back with alternating order.
# Inputs in ~: src-head.tar, src-new.tar (git archive), cfg.tar (configs/*.toml).
set -e
source ~/.cargo/env
V="head new"
R=~/results/accept
mkdir -p ~/cfg $R ~/work
for s in $V; do rm -rf ~/$s && mkdir -p ~/$s && tar -xf ~/src-$s.tar -C ~/$s; done
tar -xf ~/cfg.tar -C ~/cfg
for s in $V; do
  (cd ~/$s && cargo build --release -p cli --bin solvers -p hu-postflop --example p1_bench) \
    > ~/results/build-$s.log 2>&1
done
{ lscpu; free -g; uname -a; rustc -Vv; cargo -V; } > ~/results/machine.txt
for s in $V; do sha256sum ~/$s/target/release/solvers ~/$s/target/release/examples/p1_bench; done > ~/results/binaries.sha256
echo "$(date -u +%T) BUILT" >> ~/results/progress.txt
set +e

# 1. Per-iteration and evaluation time without a profile.
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
  V="new head"
  echo "$(date -u +%T) bench rep $rep" >> ~/results/progress.txt
done
V="head new"

# 2. Solves to 0.1% pot (auto check_every), peak RSS from /usr/bin/time -v.
mkcfg() {  # cfg storage out [profile-fragment]
  sed -e '/^max_iterations/d' -e '/^check_every/d' -e 's/^target = .*/target = "0.1%pot"/' ~/cfg/$1.toml > $3
  if grep -q '^storage = ' $3; then sed -i "s/^storage = .*/storage = \"$2\"/" $3; else printf '\n[solver]\nstorage = "%s"\n' $2 >> $3; fi
  if grep -q '^\[run\]' $3; then sed -i 's/^\[run\]/[run]\nfinal_checkpoint = false/' $3; else printf '\n[run]\nfinal_checkpoint = false\n' >> $3; fi
  if [ -n "$4" ]; then cat ~/cfg/$4.toml >> $3; fi
}
solve() {  # variant cfg threads storage tag [profile-fragment]
  local c=~/work/$5.toml d=~/work/$5_$1
  mkcfg $2 $4 $c $6
  rm -rf $d
  /usr/bin/time -v ~/$1/target/release/solvers solve $c --out $d --threads $3 > $R/solve_$5_$1.log 2> $R/solve_$5_$1.time
  cp $d/progress.jsonl $R/solve_$5_$1.progress.jsonl; cp $d/run.json $R/solve_$5_$1.run.json
  rm -rf $d
  echo "$(date -u +%T) solve $5 $1" >> ~/results/progress.txt
}
for s in head new; do solve $s flop_srp 32 f32 srp_f32_t32; done
for s in new head; do solve $s flop_srp 16 f32 srp_f32_t16; done
for s in head new; do solve $s flop_srp 32 i16 srp_i16_t32; done
for s in new head; do solve $s flop_3bp 32 f32 3bp_f32_t32; done
for s in head new; do solve $s flop_srp 32 f32 lockflop_f32_t32 lock_flop; done
for s in new head; do solve $s flop_srp 32 f32 lockturn_f32_t32 lock_turn; done
for s in head new; do solve $s c_gtowb 32 i16 gtowb_i16_t32; done
echo "$(date -u +%T) ALL_DONE" >> ~/results/progress.txt
