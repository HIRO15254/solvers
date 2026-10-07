#!/bin/bash
# VM7 phase 2: after run7 finishes, build T17 and run run8.py, then a line-level perf annotate of T17.
source ~/.cargo/env
until grep -q ALL_DONE ~/results/progress.txt; do sleep 20; done
mkdir -p ~/t17 ~/results/vm8 ~/work8 && tar -xzf ~/src-t17.tgz -C ~/t17
cp ~/t17/crates/hu-postflop/benches/kernels.rs ~/new/crates/hu-postflop/benches/kernels.rs
(cd ~/t17 && cargo build --release -p cli --bin solvers 2>&1 | tail -2 && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -2) > ~/results/vm8/build.log 2>&1
echo "$(date -u +%T) T17_SETUP_DONE" >> ~/results/progress.txt
python3 ~/run8.py > ~/results/vm8/run8.log 2>&1
# Line-level profile of the T17 kernels and cfr_pass (line tables only; same optimization).
(cd ~/t17 && CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release -p hu-postflop --example p1_bench --target-dir ~/t17dbg 2>&1 | tail -2) >> ~/results/vm8/build.log 2>&1
B=~/t17dbg/release/examples/p1_bench
sudo perf record -F 1999 -o ~/work8/ann.data $B ~/work8/c_flop1.toml --threads 32 --warmup 2 --iters 30 --evals 0 > /dev/null 2> ~/results/vm8/ann_record.err
sudo perf report -i ~/work8/ann.data --no-children --sort symbol --stdio 2>/dev/null | head -60 > ~/results/vm8/ann_report.txt
for sym in hu_postflop::kernel::showdown_kernel_relaxed_f32 hu_postflop::kernel::fold_kernel_relaxed_f32 hu_engine::storage::normalize_columns_f32; do
  sudo perf annotate -i ~/work8/ann.data --stdio "$sym" 2>/dev/null | head -700 > ~/results/vm8/ann_$(echo $sym | sed 's/.*:://').txt
done
CFR=$(sudo perf report -i ~/work8/ann.data --no-children --sort symbol --stdio 2>/dev/null | grep -o "hu_engine::solver::cfr_pass[^ ]*.*F32View, true>" | head -1)
sudo perf annotate -i ~/work8/ann.data --stdio "$CFR" 2>/dev/null | head -1500 > ~/results/vm8/ann_cfr_pass.txt
sudo rm -f ~/work8/ann.data
echo "$(date -u +%T) ANNOTATE_DONE" >> ~/results/progress.txt
