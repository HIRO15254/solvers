#!/bin/bash
# VM10: T26 (blocked lane output store and fold-only lane batches, Codex) against T25 (a2f3503):
# p1_bench and solves (f64 .sol compare; f32 rounding changes), checks on the T26 tree, then a sampling profile.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time linux-perf >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
rustup component add clippy rustfmt > /dev/null 2>&1
mkdir -p ~/cfg ~/results/vm50 && tar -xzf ~/cfg.tgz -C ~/cfg --strip-components=1
mkdir -p ~/t25 && tar -xzf ~/src-t25.tgz -C ~/t25
(cd ~/t25 && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
  && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) >> ~/results/vm50/build.log 2>&1
lscpu | head -20 > ~/results/vm50/lscpu.txt; free -g >> ~/results/vm50/lscpu.txt
echo "$(date -u +%T) T25_BUILT" >> ~/results/progress.txt
set +e
until [ -s ~/t26.ready ]; do sleep 20; done
cp -r ~/t25 ~/t26 && (cd ~/t26 && git apply ~/t26.patch) > ~/results/vm50/apply.log 2>&1
(cd ~/t26 && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
  && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) >> ~/results/vm50/build.log 2>&1
(cd ~/t26 && find crates docs -type f \( -name '*.rs' -o -name '*.md' \) | sort | xargs md5sum) > ~/results/vm50/t26_md5.txt
echo "$(date -u +%T) SETUP50_DONE" >> ~/results/progress.txt
python3 ~/run20.py vm50 t25 t26 -- bench solve > ~/results/vm50/run50.log 2>&1
(cd ~/t26 && cargo fmt --all --check > ~/results/vm50/fmt.log 2>&1; echo "fmt=$?" >> ~/results/vm50/checks.txt
 cargo clippy --workspace --all-targets -- -D warnings > ~/results/vm50/clippy.log 2>&1; echo "clippy=$?" >> ~/results/vm50/checks.txt
 cargo test --workspace --no-fail-fast > ~/results/vm50/test.log 2>&1; echo "test=$?" >> ~/results/vm50/checks.txt
 python3 tools/check_docs.py > ~/results/vm50/check_docs.log 2>&1; echo "check_docs=$?" >> ~/results/vm50/checks.txt)
echo "$(date -u +%T) VM50_CHECKS_DONE" >> ~/results/progress.txt
sudo sysctl -q -w kernel.perf_event_paranoid=-1 kernel.kptr_restrict=0
R=~/results/prof51; mkdir -p $R
(cd ~/t26 && CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_TARGET_DIR=~/prof_t26 \
  cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) > $R/build.log 2>&1
for spec in "c_flop1 flop1_t32 32 15 40" "c_gtowb gtowb_t32 32 3 8"; do
  set -- $spec
  d=~/perf51_$2.data
  perf record -e cpu-clock -F 499 -o $d -- ~/prof_t26/release/examples/p1_bench ~/work_vm50/$1.toml \
    --threads $3 --warmup $4 --iters $5 --evals 0 --json $R/bench_$2.json > /dev/null 2> $R/perf_$2.err
  perf report -i $d --no-children --sort symbol --stdio 2> /dev/null | head -120 > $R/report_$2.txt
  perf report -i $d --no-children --sort dso --stdio 2> /dev/null | head -40 > $R/dso_$2.txt
  grep -E '^ +[0-9.]+% +\[\.\] ' $R/report_$2.txt | sed -E 's/^ +[0-9.]+% +\[\.\] +//; s/( +-)+ *$//; s/ +$//' \
    | grep -v '^0x' | head -5 > $R/top_$2.txt
  echo "$(date -u +%T) prof51 $2 report done" >> ~/results/progress.txt
  i=0
  while IFS= read -r sym; do
    i=$((i + 1))
    timeout 600 perf annotate -i $d --stdio -l -s "$sym" 2> /dev/null > ~/ann51_$2_$i.txt
    { echo "## $sym"; sed -n '1,/Percent/p' ~/ann51_$2_$i.txt | head -100; echo "## hottest instructions";
      grep -E '^ +[0-9]+\.[0-9]+ +:' ~/ann51_$2_$i.txt | sort -rn | head -80; } > $R/annotate_$2_$i.txt
  done < $R/top_$2.txt
done
echo "$(date -u +%T) VM51_ALL_DONE" >> ~/results/progress.txt
