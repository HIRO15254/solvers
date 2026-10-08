#!/bin/bash
# VM8 phase 4: T22 (zipped, vectorizable per-hand loops in cfr_pass) against the T20 HEAD f918dc5 ("b22").
source ~/.cargo/env
rustup component add clippy rustfmt > /dev/null 2>&1
mkdir -p ~/b22 ~/results/vm25 && tar -xzf ~/src-f918.tgz -C ~/b22
cp -r ~/b22 ~/t22 && (cd ~/t22 && git apply ~/t22.patch)
for side in b22 t22; do
  (cd ~/$side && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) >> ~/results/vm25/build.log 2>&1
done
echo "$(date -u +%T) SETUP25_DONE" >> ~/results/progress.txt
python3 ~/run25.py vm25 b22 t22 -- bench solve > ~/results/vm25/run25.log 2>&1
(cd ~/t22 && cargo fmt --all --check > ~/results/vm25/fmt.log 2>&1; echo "fmt=$?" >> ~/results/vm25/checks.txt
 cargo clippy --workspace --all-targets -- -D warnings > ~/results/vm25/clippy.log 2>&1; echo "clippy=$?" >> ~/results/vm25/checks.txt
 cargo test --workspace > ~/results/vm25/test.log 2>&1; echo "test=$?" >> ~/results/vm25/checks.txt)
echo "$(date -u +%T) VM25_ALL_DONE" >> ~/results/progress.txt
