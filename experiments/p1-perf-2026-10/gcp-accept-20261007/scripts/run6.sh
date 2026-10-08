#!/bin/bash
# VM4 tail 2: flat perf profile of side $SIDE (default d = prune + T6) on flop1, then workspace tests on d.
cd ~; source ~/.cargo/env
SIDE=${SIDE:-d}
R=~/results; mkdir -p $R/prof
log() { echo "$(date -u +%T) $*" >> $R/progress.txt; }
until grep -q T6_DONE $R/progress.txt; do sleep 20; done
mkdir -p ~/d && tar -xzf ~/src-d.tgz -C ~/d
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq linux-perf >/dev/null 2>&1
sudo sysctl -q -w kernel.perf_event_paranoid=1 kernel.kptr_restrict=0
(cd ~/$SIDE && CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release --target-dir target-prof -p hu-postflop --example p1_bench 2>&1 | tail -1) > $R/prof/build.log 2>&1
B=~/$SIDE/target-prof/release/examples/p1_bench
for spec in "f32 1 3" "f32 32 20" "i16-f32avg 32 20" "i16 32 20"; do
  set -- $spec
  tag=$1_t$2
  perf record -F 499 -o /tmp/perf_$tag.data $B ~/cfg/flop1_f32.toml --storage $1 --threads $2 --warmup 1 --iters $3 --evals 1 > $R/prof/$tag.json 2> $R/prof/$tag.err
  perf report -i /tmp/perf_$tag.data --no-children --sort symbol --stdio 2>/dev/null | grep -E "^ +[0-9]+\.[0-9]+%" | head -40 > $R/prof/$tag.txt
  rm -f /tmp/perf_$tag.data
  log "prof $tag"
done
(cd ~/d && cargo test --workspace > $R/test_d.log 2>&1; echo "TEST_EXIT $?" >> $R/test_d.log)
grep -E "^test result" $R/test_d.log | awk '{p+=$4; f+=$6} END {print "d tests passed",p,"failed",f}' >> $R/progress.txt
echo PROF_DONE >> $R/progress.txt
