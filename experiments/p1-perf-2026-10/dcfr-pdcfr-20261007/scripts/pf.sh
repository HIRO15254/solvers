#!/bin/bash
# VM6 phase F: robustness of the DCFR candidate (alpha 1.25, beta 0.5, gamma 4) vs the default
# (1.5, 0, 3): gtow_b to 0.05%, a wet-board Flop (9h 8h 6c) to 0.05%, gtow_b i16-f32avg to 0.1%.
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
mk c_gtowb gtowb_s2_005 s2 0.05%pot f32;  solve gtowb_s2_005 c_gtowb_s2_005
mk c_flop3 flop3_s2 s2 0.05%pot f32;      solve flop3_s2 c_flop3_s2
mk c_flop3 flop3_def def 0.05%pot f32;    solve flop3_def c_flop3_def
mk c_gtowb gtowb_def_005 def 0.05%pot f32; solve gtowb_def_005 c_gtowb_def_005
mk c_gtowb gtowb_s2_mixed s2 0.1%pot i16-f32avg; solve gtowb_s2_mixed c_gtowb_s2_mixed
echo PHASEF_DONE >> ~/results/progress.txt
