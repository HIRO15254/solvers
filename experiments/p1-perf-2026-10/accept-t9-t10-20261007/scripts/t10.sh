#!/bin/bash
# VM6 phase C: T10 (regret arenas released after the final checkpoint).
# Build + test T10, compare it with T9 (~/new), then solve gtow_a with i16-f32avg to 0.1% pot
# on this 64 GB machine (T9 rejects it: estimate over the 80% limit).
R=~/results/t10; mkdir -p $R
log() { echo "$(date -u +%T) $*" >> ~/results/progress.txt; }
until grep -q SWEEP2_DONE ~/results/progress.txt 2>/dev/null; do sleep 30; done
source ~/.cargo/env
rm -rf ~/t10 && mkdir -p ~/t10 && tar -xzf ~/src-t10.tgz -C ~/t10
(cd ~/t10 && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
  && cargo build --release -p hu-postflop --example verify_save 2>&1 | tail -1) > $R/build.log 2>&1
log "t10 build"
(cd ~/t10 && cargo test --workspace > $R/test.log 2>&1; echo "TEST_EXIT $?" >> $R/test.log)
grep -E "^test result" $R/test.log | awk '{p+=$4; f+=$6} END {print "t10 tests passed",p,"failed",f}' >> ~/results/progress.txt
python3 ~/t10_compare.py > $R/compare.log 2>&1
log "t10 compare exit $?"
( while true; do echo "$(date +%s) $(free -b | awk '/Mem:/ {print $3}')" >> $R/mem.txt; sleep 5; done ) &
MP=$!
rm -rf ~/gtow_a_run
/usr/bin/time -v ~/t10/target/release/solvers solve ~/gtow_a.toml --out ~/gtow_a_run --threads 32 > $R/solve.out 2> $R/solve.err
echo "exit $?" >> $R/solve.err
kill $MP
cp ~/gtow_a_run/progress.jsonl ~/gtow_a_run/run.json ~/gtow_a_run/events.jsonl $R/ 2>/dev/null
ls -la ~/gtow_a_run > $R/ls.txt
~/t10/target/release/solvers export ~/gtow_a_run/solution.sol summary > $R/summary.txt 2>&1
rm -rf ~/gtow_a_run
echo PHASEC_DONE >> ~/results/progress.txt
