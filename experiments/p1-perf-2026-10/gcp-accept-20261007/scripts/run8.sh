#!/bin/bash
# VM4 tail 4: pow4_reset=false on d (prune+T6) for i16-f32avg (turn/flop1/gtowb) and f32 gtowb.
cd ~; source ~/.cargo/env
R=~/results; mkdir -p $R/nr
log() { echo "$(date -u +%T) $*" >> $R/progress.txt; }
until grep -q ALLOC_DONE $R/progress.txt; do sleep 15; done
mkdir -p ~/cfg8 && tar -xzf ~/cfg8.tgz -C ~/cfg8 --strip-components=1 && cp ~/cfg8/nr_*.toml ~/cfg/
(cd ~/d && cargo build --release -p cli --bin solvers 2>&1 | tail -1) > $R/nr/build.log 2>&1
for t in nr_turn_mix nr_flop1_mix nr_gtowb_mix nr_gtowb_f32; do
  rm -rf /tmp/run_$t
  /usr/bin/time -v ~/d/target/release/solvers solve ~/cfg/$t.toml --out /tmp/run_$t --threads 32 > $R/nr/$t.out 2> $R/nr/$t.err
  cp /tmp/run_$t/progress.jsonl $R/nr/$t.progress.jsonl 2>/dev/null; rm -rf /tmp/run_$t
  log "nr $t"
done
echo NR_DONE >> $R/progress.txt
