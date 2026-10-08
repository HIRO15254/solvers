#!/bin/bash
# VM8 phase 6 (replaces setup27): T21 on T22 ("t21" vs "t22c"), then T23 on top ("t23" vs "t21") with .sol
# payload compares, checks on each new tree, and finally sampling profiles of the newest tree.
source ~/.cargo/env
until grep -q VM26_ALL_DONE ~/results/progress.txt; do sleep 20; done
mkdir -p ~/results/vm27 ~/results/vm28
cp -r ~/b22 ~/t22c && (cd ~/t22c && git apply ~/t22c.patch)
cp -r ~/t22c ~/t21 && (cd ~/t21 && git apply ~/t21m.patch)
cp -r ~/t21 ~/t23 && (cd ~/t23 && git apply ~/t23.patch)
for side in t22c t21 t23; do
  (cd ~/$side && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) >> ~/results/vm27/build_$side.log 2>&1
done
echo "$(date -u +%T) SETUP27_DONE" >> ~/results/progress.txt
python3 ~/run20.py vm27 t22c t21 -- bench solve > ~/results/vm27/run27.log 2>&1
python3 ~/run28.py vm28 t21 t23 -- bench solve > ~/results/vm28/run28.log 2>&1
checks() {  # tree result-dir
  (cd ~/$1 && cargo fmt --all --check > $2/fmt.log 2>&1; echo "fmt=$?" >> $2/checks.txt
   cargo clippy --workspace --all-targets -- -D warnings > $2/clippy.log 2>&1; echo "clippy=$?" >> $2/checks.txt
   cargo test --workspace > $2/test.log 2>&1; echo "test=$?" >> $2/checks.txt)
}
checks t21 ~/results/vm27
echo "$(date -u +%T) VM27_CHECKS_DONE" >> ~/results/progress.txt
checks t23 ~/results/vm28
echo "$(date -u +%T) VM28_CHECKS_DONE" >> ~/results/progress.txt
# Sampling profile of the newest tree (line tables only; codegen unchanged).
R=~/results/prof27; mkdir -p $R
(cd ~/t23 && CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_TARGET_DIR=~/prof_t23 \
  cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) > $R/build.log 2>&1
for spec in "c_flop1 flop1_t32 32 15 40" "c_gtowb gtowb_t32 32 3 8" "c_flop1 flop1_t16 16 15 40"; do
  set -- $spec
  d=~/perf27_$2.data
  perf record -e cpu-clock -F 499 -o $d -- ~/prof_t23/release/examples/p1_bench ~/work_vm20a/$1.toml \
    --threads $3 --warmup $4 --iters $5 --evals 0 --json $R/bench_$2.json > /dev/null 2> $R/perf_$2.err
  perf report -i $d --no-children --sort symbol --stdio 2> /dev/null | head -120 > $R/report_$2.txt
  perf report -i $d --no-children --sort dso --stdio 2> /dev/null | head -40 > $R/dso_$2.txt
  grep -E '^ +[0-9.]+% +\[\.\] ' $R/report_$2.txt | sed -E 's/^ +[0-9.]+% +\[\.\] +//; s/( +-)+ *$//; s/ +$//' \
    | grep -v '^0x' | head -4 > $R/top_$2.txt
  echo "$(date -u +%T) prof27 $2 report done" >> ~/results/progress.txt
  i=0
  while IFS= read -r sym; do
    i=$((i + 1))
    timeout 600 perf annotate -i $d --stdio -l -s "$sym" 2> /dev/null > ~/ann27_$2_$i.txt
    { echo "## $sym"; sed -n '1,/Percent/p' ~/ann27_$2_$i.txt | head -100; echo "## hottest instructions";
      grep -E '^ +[0-9]+\.[0-9]+ +:' ~/ann27_$2_$i.txt | sort -rn | head -80; } > $R/annotate_$2_$i.txt
  done < $R/top_$2.txt
done
echo "$(date -u +%T) VM27_ALL_DONE" >> ~/results/progress.txt
