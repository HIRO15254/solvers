#!/bin/bash
# VM8 phase 5: T22 part 2 ("t22b": value/profile pass loops and the all-zero test on top of t22), after vm25.
source ~/.cargo/env
until grep -q VM25_ALL_DONE ~/results/progress.txt; do sleep 20; done
mkdir -p ~/results/vm26
cp -r ~/b22 ~/t22b && (cd ~/t22b && git apply ~/t22b.patch)
(cd ~/t22b && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
  && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) > ~/results/vm26/build.log 2>&1
echo "$(date -u +%T) SETUP26_DONE" >> ~/results/progress.txt
python3 ~/run26.py vm26 b22 t22 t22b -- bench solve > ~/results/vm26/run26.log 2>&1
(cd ~/t22b && cargo fmt --all --check > ~/results/vm26/fmt.log 2>&1; echo "fmt=$?" >> ~/results/vm26/checks.txt
 cargo clippy --workspace --all-targets -- -D warnings > ~/results/vm26/clippy.log 2>&1; echo "clippy=$?" >> ~/results/vm26/checks.txt
 cargo test --workspace > ~/results/vm26/test.log 2>&1; echo "test=$?" >> ~/results/vm26/checks.txt)
echo "$(date -u +%T) VM26_ALL_DONE" >> ~/results/progress.txt
