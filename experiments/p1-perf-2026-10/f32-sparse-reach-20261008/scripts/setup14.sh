#!/bin/bash
# VM7 phase 8: T18 (skip zero opponent reach in the f32 terminal kernels) vs the T16 head.
source ~/.cargo/env
rm -rf ~/t18 && mkdir -p ~/t18 ~/results/vm11 ~/work11 && tar -xzf ~/src-t18.tgz -C ~/t18
cp ~/t18/crates/hu-postflop/benches/kernels.rs ~/new/crates/hu-postflop/benches/kernels.rs
(cd ~/t18 && cargo build --release -p cli --bin solvers 2>&1 | tail -2 && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -2) > ~/results/vm11/build.log 2>&1
echo "$(date -u +%T) T18_SETUP_DONE" >> ~/results/progress.txt
python3 ~/run9.py > ~/results/vm11/run9.log 2>&1
