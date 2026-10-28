#!/bin/bash
# Second acceptance VM: the first one (accept.sh) was reclaimed by a Spot interruption after ALL_DONE,
# before the last results were copied. Re-runs the pairs whose raw records were lost (lock_turn f32 and
# c_gtowb i16 solves) and the memory diagnostic, with the same builds and solve settings as accept.sh.
# Inputs in ~: src-head.tar, src-new.tar, cfg.tar.
set -e
source ~/.cargo/env
R=~/results/accept2
mkdir -p ~/cfg $R ~/work
for s in head new; do rm -rf ~/$s && mkdir -p ~/$s && tar -xf ~/src-$s.tar -C ~/$s; done
tar -xf ~/cfg.tar -C ~/cfg
for s in head new; do
  (cd ~/$s && cargo build --release -p cli --bin solvers -p hu-postflop --example p1_bench) \
    > $R/build-$s.log 2>&1
done
{ lscpu; free -g; uname -a; rustc -Vv; cargo -V; } > $R/machine.txt
for s in head new; do sha256sum ~/$s/target/release/solvers ~/$s/target/release/examples/p1_bench; done > $R/binaries.sha256
echo "$(date -u +%T) BUILT" >> $R/progress.txt
set +e

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
  echo "$(date -u +%T) solve $5 $1" >> $R/progress.txt
}
for s in new head; do solve $s flop_srp 32 f32 lockturn_f32_t32 lock_turn; done
for s in head new; do solve $s c_gtowb 32 i16 gtowb_i16_t32; done

# Memory diagnostic (f32, 32 threads): CFR-only peak (--evals 0) against the peak with two evaluations.
mkcfg flop_srp f32 ~/work/srp.toml
mkcfg flop_srp f32 ~/work/srp_lock_flop.toml lock_flop
for v in head new; do
  for cfg in srp srp_lock_flop; do
    [ $v = head ] && [ $cfg = srp_lock_flop ] && continue  # head p1_bench ignores [profile]
    for e in 0 2; do
      ~/$v/target/release/examples/p1_bench ~/work/$cfg.toml --threads 32 --warmup 3 --iters 10 --evals $e --storage f32 \
        --json $R/diag_${cfg}_${v}_e$e.json > /dev/null 2> $R/diag_${cfg}_${v}_e$e.err
    done
  done
done
echo "$(date -u +%T) ALL_DONE" >> $R/progress.txt
