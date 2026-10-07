#!/bin/bash
# VM6 phase E: gtow_b with the sweep-2 geometric-mean winner (DCFR alpha 1.25, beta 0.5, gamma 4).
R=~/results/conv
until grep -q PHASED_DONE ~/results/progress.txt 2>/dev/null; do sleep 30; done
python3 - <<'PY'
import os
t = open(os.path.expanduser("~/cfg/c_gtowb.toml")).read()
head, rest = t.split("[solver.algorithm]\n", 1)
rest = rest[rest.index("[solver.stop]"):]
algo = 'schedule = "dcfr"\nalpha = 1.25\nbeta = 0.5\ngamma = 4.0\n'
open(os.path.expanduser("~/cfg/c_gtowb_s2.toml"), "w").write(head + "[solver.algorithm]\n" + algo + "\n" + rest)
PY
rm -rf /tmp/run_s2
/usr/bin/time -v ~/new/target/release/solvers solve ~/cfg/c_gtowb_s2.toml --out /tmp/run_s2 --threads 32 > $R/c_gtowb_s2.out 2> $R/c_gtowb_s2.err
cp /tmp/run_s2/progress.jsonl $R/c_gtowb_s2.progress.jsonl; cp /tmp/run_s2/run.json $R/c_gtowb_s2.run.json; rm -rf /tmp/run_s2
echo PHASEE_DONE >> ~/results/progress.txt
