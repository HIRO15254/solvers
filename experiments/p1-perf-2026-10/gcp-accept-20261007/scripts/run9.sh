#!/bin/bash
# VM4 tail 5: save cost on RAM disk vs pd-balanced (cli_gtowb, 4 iterations) on d.
cd ~; source ~/.cargo/env
R=~/results; mkdir -p $R/save
log() { echo "$(date -u +%T) $*" >> $R/progress.txt; }
until grep -q NR_DONE $R/progress.txt; do sleep 15; done
for where in shm disk; do
  if [ $where = shm ]; then O=/dev/shm/save_$where; else O=/tmp/save_$where; fi
  rm -rf $O
  /usr/bin/time -v ~/d/target/release/solvers solve ~/cfg/cli_gtowb.toml --out $O --threads 32 > $R/save/$where.out 2> $R/save/$where.err
  cp $O/events.jsonl $R/save/$where.events.jsonl; ls -l $O >> $R/save/$where.out; rm -rf $O
  log "save $where"
done
echo SAVE_DONE >> $R/progress.txt
