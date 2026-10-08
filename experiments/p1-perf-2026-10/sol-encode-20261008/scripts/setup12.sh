#!/bin/bash
# VM7 phase 6: per-thread profile of the save phase (output on tmpfs) to find the serial bottleneck.
out=/dev/shm/prof; rm -rf $out
~/new/target/release/solvers solve ~/work10/g25.toml --out $out --threads 32 > ~/results/vm10/prof.out 2>&1 &
PID=$!
until grep -q "^done:" ~/results/vm10/prof.out; do sleep 0.2; done
sudo perf record -F 999 -g -p $PID -o ~/work10/save.data > /dev/null 2> ~/results/vm10/save_record.err &
PERF=$!
wait $PID
sleep 1; sudo kill -INT $PERF 2>/dev/null; wait $PERF 2>/dev/null; sleep 2
sudo perf report -i ~/work10/save.data --no-children --sort pid --stdio -g none 2>&1 | head -60 > ~/results/vm10/save_tid.txt
TOP=$(grep -oE "[0-9]+:[A-Za-z0-9_.-]+" ~/results/vm10/save_tid.txt | head -1 | cut -d: -f1)
sudo perf report -i ~/work10/save.data --no-children --tid $TOP --sort sym --stdio -g none 2>&1 | head -60 > ~/results/vm10/save_main_sym.txt
sudo perf report -i ~/work10/save.data --no-children --tid $TOP --sort sym --stdio -g caller,0.5,callee --percent-limit 2 2>&1 | head -300 > ~/results/vm10/save_main_callers.txt
rm -rf $out; sudo rm -f ~/work10/save.data
echo "$(date -u +%T) PROF2_DONE main=$PID top=$TOP" >> ~/results/progress.txt
