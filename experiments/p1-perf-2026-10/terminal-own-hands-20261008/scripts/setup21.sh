#!/bin/bash
# VM8 phase 2: T20 Part A+B ("t20ab") against the same base, after the Part A measurement.
source ~/.cargo/env
until grep -q vm20a_DONE ~/results/progress.txt; do sleep 20; done
mkdir -p ~/t20ab ~/results/vm20b && tar -xzf ~/src-t20ab.tgz -C ~/t20ab
(cd ~/t20ab && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
  && cargo build --release -p hu-postflop --example p1_bench --example verify_save 2>&1 | tail -1) > ~/results/vm20b/build.log 2>&1
echo "$(date -u +%T) SETUP21_DONE" >> ~/results/progress.txt
python3 ~/run20.py vm20b base t20ab > ~/results/vm20b/run20.log 2>&1
