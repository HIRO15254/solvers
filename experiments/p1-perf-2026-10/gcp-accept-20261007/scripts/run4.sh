#!/bin/bash
# VM4 driver: T5/T6 acceptance on GCP. Equivalence a vs b, time to 0.1% pot, new storage curves, .sol timing.
cd ~
until grep -q SETUP_DONE ~/setup.log; do sleep 10; done
source ~/.cargo/env
R=~/results; mkdir -p $R/eq $R/conv $R/cli
log() { echo "$(date -u +%T) $*" >> $R/progress.txt; }
solve() { # side cfg label threads -> $R/$dir/label.*
  local side=$1 cfg=$2 label=$3 thr=$4 dir=$5
  rm -rf /tmp/run_$label
  /usr/bin/time -v ~/$side/target/release/solvers solve ~/cfg/$cfg --out /tmp/run_$label --threads $thr \
    > $R/$dir/$label.out 2> $R/$dir/$label.err
  cp /tmp/run_$label/progress.jsonl $R/$dir/$label.progress.jsonl 2>/dev/null
  cp /tmp/run_$label/events.jsonl $R/$dir/$label.events.jsonl 2>/dev/null
}
log start
grep -E "^test result|TEST_EXIT" $R/test_b.log | awk '{p+=$4; f+=$6} END {print "tests passed",p,"failed",f}' >> $R/progress.txt
# 1. a vs b equivalence (.sol payload, NashConv sequence), b thread invariance via progress
for c in flop1_f32 flop1_i16 turn_f32 turn_i16; do
  solve a $c.toml eq_${c}_a32 32 eq
  solve b $c.toml eq_${c}_b32 32 eq
  solve b $c.toml eq_${c}_b8 8 eq
  ~/b/target/release/examples/verify_save solution /tmp/run_eq_${c}_a32/solution.sol /tmp/run_eq_${c}_b32/solution.sol > $R/eq/${c}_a32_b32.json 2>&1
  for s in a32 b32 b8; do
    for v in summary strategy ev; do
      ~/b/target/release/solvers export /tmp/run_eq_${c}_$s/solution.sol $v --format csv --output $R/eq/${c}_${s}_$v.csv 2>>$R/eq/export.err
    done
  done
  rm -rf /tmp/run_eq_${c}_*
  log "eq $c"
done
# 2. time to 0.1% pot, f32 dcfr, a vs b alternating
for r in 1 2; do
  for t in turn flop1; do
    for side in a b; do solve $side t_$t.toml t_${t}_${side}_$r 32 conv; rm -rf /tmp/run_t_${t}_${side}_$r; done
    log "t $t round $r"
  done
done
for side in a b; do solve $side t_gtowb.toml t_gtowb_$side 32 conv; rm -rf /tmp/run_t_gtowb_$side; log "t gtowb $side"; done
# 3. new storage curves: run later from run5.sh once T6 is available
# 4. CLI .sol timing on gtowb (4 iterations, final checkpoint then .sol)
for side in t3b a b; do solve $side cli_gtowb.toml cli_gtowb_$side 32 cli; ls -l /tmp/run_cli_gtowb_$side >> $R/cli/cli_gtowb_$side.out; rm -rf /tmp/run_cli_gtowb_$side; log "cli $side"; done
echo ALL_DONE >> $R/progress.txt
