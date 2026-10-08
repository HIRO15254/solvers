#!/bin/bash
# VM8 phase 7: checks on the committed T22 tree alone ("t22c"), after setup27b.
source ~/.cargo/env
until grep -q VM27_ALL_DONE ~/results/progress.txt; do sleep 20; done
R=~/results/vm29
mkdir -p $R
(cd ~/t22c && cargo fmt --all --check > $R/fmt.log 2>&1; echo "fmt=$?" >> $R/checks.txt
 cargo clippy --workspace --all-targets -- -D warnings > $R/clippy.log 2>&1; echo "clippy=$?" >> $R/checks.txt
 cargo test --workspace > $R/test.log 2>&1; echo "test=$?" >> $R/checks.txt)
echo "$(date -u +%T) VM29_CHECKS_DONE" >> ~/results/progress.txt
