#!/bin/bash
# VM5 tail: run10's final cargo test ran before ~/.cargo/env existed; rerun it after ALL_DONE.
R=~/results
until grep -q ALL_DONE $R/progress.txt 2>/dev/null; do sleep 30; done
source ~/.cargo/env
(cd ~/e && cargo test --workspace > $R/test_e.log 2>&1; echo "TEST_EXIT $?" >> $R/test_e.log)
grep -E "^test result" $R/test_e.log | awk '{p+=$4; f+=$6} END {print "e tests (rerun) passed",p,"failed",f}' >> $R/progress.txt
echo TEST_DONE >> $R/progress.txt
