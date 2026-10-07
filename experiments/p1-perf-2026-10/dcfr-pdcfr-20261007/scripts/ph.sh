#!/bin/bash
# VM6 phase H: gtow_b i16-f32avg with the default DCFR to 0.1% pot (same binary as phase F c_gtowb_s2_mixed).
until grep -q PHASEG_DONE ~/results/progress.txt 2>/dev/null; do sleep 30; done
R=~/results/conv
log() { echo "$(date -u +%T) $*" >> ~/results/progress.txt; }
mk() { # src dst algo target storage
python3 - "$@" <<'PY'
import os, sys, re
src, dst, algo, target, storage = sys.argv[1:]
t = open(os.path.expanduser(f"~/cfg/{src}.toml")).read()
head, rest = t.split("[solver.algorithm]\n", 1)
rest = rest[rest.index("[solver.stop]"):]
head = re.sub(r'storage = "[^"]*"', f'storage = "{storage}"', head)
rest = re.sub(r'target = "[^"]*"', f'target = "{target}"', rest)
algos = {"def": 'schedule = "dcfr"\n', "s2": 'schedule = "dcfr"\nalpha = 1.25\nbeta = 0.5\ngamma = 4.0\n'}
open(os.path.expanduser(f"~/cfg/{dst}.toml"), "w").write(head + "[solver.algorithm]\n" + algos[algo] + "\n" + rest)
PY
}
solve() { # cfg tag
  rm -rf /tmp/run_$2
  /usr/bin/time -v ~/new/target/release/solvers solve ~/cfg/$1.toml --out /tmp/run_$2 --threads 32 > $R/$2.out 2> $R/$2.err
  cp /tmp/run_$2/progress.jsonl $R/$2.progress.jsonl; cp /tmp/run_$2/run.json $R/$2.run.json; rm -rf /tmp/run_$2
  log "solve $2"
}
mk c_gtowb gtowb_def_mixed def 0.1%pot i16-f32avg; solve gtowb_def_mixed c_gtowb_def_mixed
echo PHASEH_DONE >> ~/results/progress.txt
