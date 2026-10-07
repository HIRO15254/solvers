#!/bin/bash
# VM4 tail: T6 (i16-f32avg) after run4.sh. Side c = T6 candidate on top of a (06415d2); built only after run4 so timings stay clean.
cd ~
source ~/.cargo/env
R=~/results; mkdir -p $R/eq5 $R/m
log() { echo "$(date -u +%T) $*" >> $R/progress.txt; }
until grep -q ALL_DONE $R/progress.txt; do sleep 20; done
log "t6 build start"
mkdir -p ~/c && tar -xzf ~/src-c.tgz -C ~/c
(cd ~/c && cargo build --release -p cli --bin solvers 2>&1 | tail -1 && cargo build --release -p hu-postflop --example verify_save 2>&1 | tail -1) > $R/build_c.log 2>&1
solve() { # side cfg label threads dir
  local side=$1 cfg=$2 label=$3 thr=$4 dir=$5
  rm -rf /tmp/run_$label
  /usr/bin/time -v ~/$side/target/release/solvers solve ~/cfg/$cfg --out /tmp/run_$label --threads $thr \
    > $R/$dir/$label.out 2> $R/$dir/$label.err
  cp /tmp/run_$label/progress.jsonl $R/$dir/$label.progress.jsonl 2>/dev/null
}
# f32 / i16 must be unchanged by T6: a vs c, .sol payload and exports
for c in flop1_f32 flop1_i16 turn_f32 turn_i16; do
  solve a $c.toml e5_${c}_a 32 eq5
  solve c $c.toml e5_${c}_c 32 eq5
  ~/c/target/release/examples/verify_save solution /tmp/run_e5_${c}_a/solution.sol /tmp/run_e5_${c}_c/solution.sol > $R/eq5/${c}_a_c.json 2>&1
  for v in summary strategy ev; do
    for s in a c; do ~/c/target/release/solvers export /tmp/run_e5_${c}_$s/solution.sol $v --format csv --output /tmp/e5_${s}_$v.csv 2>>$R/eq5/export.err; done
    if cmp -s /tmp/e5_a_$v.csv /tmp/e5_c_$v.csv; then echo "$c $v identical" >> $R/eq5/cmp.txt; else echo "$c $v DIFFER" >> $R/eq5/cmp.txt; fi
  done
  rm -rf /tmp/run_e5_${c}_* /tmp/e5_*.csv
  log "eq5 $c"
done
for t in turn flop1 gtowb; do solve c m_$t.toml m_${t}_c 32 m; rm -rf /tmp/run_m_${t}_c; log "m $t"; done
echo T6_DONE >> $R/progress.txt
