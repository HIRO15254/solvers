#!/bin/bash
# VM8 phase 8: T24 (dense combo-layout f32 compat sums and fold kernel, Codex) on top of T23 ("t24" vs "t23"):
# kernel benches, p1_bench, solves (f64 .sol compare; f32 rounding changes), then checks on the T24 tree.
source ~/.cargo/env
until grep -q VM29_CHECKS_DONE ~/results/progress.txt && [ -s ~/t24.ready ]; do sleep 20; done
mkdir -p ~/results/vm30
cp -r ~/t23 ~/t24 && (cd ~/t24 && git apply ~/t24.patch) > ~/results/vm30/apply.log 2>&1
(cd ~/t24 && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
  && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) > ~/results/vm30/build.log 2>&1
echo "$(date -u +%T) SETUP30_DONE" >> ~/results/progress.txt
python3 ~/run20.py vm30 t23 t24 -- bench solve kernels > ~/results/vm30/run30.log 2>&1
(cd ~/t24 && cargo fmt --all --check > ~/results/vm30/fmt.log 2>&1; echo "fmt=$?" >> ~/results/vm30/checks.txt
 cargo clippy --workspace --all-targets -- -D warnings > ~/results/vm30/clippy.log 2>&1; echo "clippy=$?" >> ~/results/vm30/checks.txt
 cargo test --workspace > ~/results/vm30/test.log 2>&1; echo "test=$?" >> ~/results/vm30/checks.txt)
echo "$(date -u +%T) VM30_ALL_DONE" >> ~/results/progress.txt
