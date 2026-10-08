#!/bin/bash
# VM9: T25 (lane-batched terminal evaluation of chance-free f32 subtrees, Codex) against T23 (b01d4a4):
# p1_bench and solves (f64 .sol compare; f32 rounding changes), checks on the T25 tree, then a sampling profile.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time linux-perf >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
rustup component add clippy rustfmt > /dev/null 2>&1
mkdir -p ~/cfg ~/results/vm40 && tar -xzf ~/cfg.tgz -C ~/cfg --strip-components=1
mkdir -p ~/t23 && tar -xzf ~/src-t23.tgz -C ~/t23
(cd ~/t23 && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
  && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) >> ~/results/vm40/build.log 2>&1
lscpu | head -20 > ~/results/vm40/lscpu.txt; free -g >> ~/results/vm40/lscpu.txt
echo "$(date -u +%T) T23_BUILT" >> ~/results/progress.txt
set +e
until [ -s ~/t25.ready ]; do sleep 20; done
cp -r ~/t23 ~/t25 && (cd ~/t25 && git apply ~/t25.patch) > ~/results/vm40/apply.log 2>&1
(cd ~/t25 && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
  && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) >> ~/results/vm40/build.log 2>&1
(cd ~/t25 && find crates docs -type f \( -name '*.rs' -o -name '*.md' \) | sort | xargs md5sum) > ~/results/vm40/t25_md5.txt
echo "$(date -u +%T) SETUP40_DONE" >> ~/results/progress.txt
python3 ~/run20.py vm40 t23 t25 -- bench solve > ~/results/vm40/run40.log 2>&1
(cd ~/t25 && cargo fmt --all --check > ~/results/vm40/fmt.log 2>&1; echo "fmt=$?" >> ~/results/vm40/checks.txt
 cargo clippy --workspace --all-targets -- -D warnings > ~/results/vm40/clippy.log 2>&1; echo "clippy=$?" >> ~/results/vm40/checks.txt
 cargo test --workspace > ~/results/vm40/test.log 2>&1; echo "test=$?" >> ~/results/vm40/checks.txt)
echo "$(date -u +%T) VM40_CHECKS_DONE" >> ~/results/progress.txt
R=~/results/prof41; mkdir -p $R
(cd ~/t25 && CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_TARGET_DIR=~/prof_t25 \
  cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) > $R/build.log 2>&1
for spec in "c_flop1 flop1_t32 32 15 40" "c_gtowb gtowb_t32 32 3 8"; do
  set -- $spec
  d=~/perf41_$2.data
  perf record -e cpu-clock -F 499 -o $d -- ~/prof_t25/release/examples/p1_bench ~/work_vm40/$1.toml \
    --threads $3 --warmup $4 --iters $5 --evals 0 --json $R/bench_$2.json > /dev/null 2> $R/perf_$2.err
  perf report -i $d --no-children --sort symbol --stdio 2> /dev/null | head -120 > $R/report_$2.txt
  perf report -i $d --no-children --sort dso --stdio 2> /dev/null | head -40 > $R/dso_$2.txt
  grep -E '^ +[0-9.]+% +\[\.\] ' $R/report_$2.txt | sed -E 's/^ +[0-9.]+% +\[\.\] +//; s/( +-)+ *$//; s/ +$//' \
    | grep -v '^0x' | head -5 > $R/top_$2.txt
  echo "$(date -u +%T) prof41 $2 report done" >> ~/results/progress.txt
  i=0
  while IFS= read -r sym; do
    i=$((i + 1))
    timeout 600 perf annotate -i $d --stdio -l -s "$sym" 2> /dev/null > ~/ann41_$2_$i.txt
    { echo "## $sym"; sed -n '1,/Percent/p' ~/ann41_$2_$i.txt | head -100; echo "## hottest instructions";
      grep -E '^ +[0-9]+\.[0-9]+ +:' ~/ann41_$2_$i.txt | sort -rn | head -80; } > $R/annotate_$2_$i.txt
  done < $R/top_$2.txt
done
echo "$(date -u +%T) VM41_ALL_DONE" >> ~/results/progress.txt
