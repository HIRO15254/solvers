#!/bin/bash
# VM9: fmt/clippy/test on the final T25 patch (tests and docs updated after the measured tree; production code equal).
source ~/.cargo/env
until grep -q VM42_ALL_DONE ~/results/progress.txt; do sleep 20; done
R=~/results/vm43; mkdir -p $R
mkdir -p ~/t25f && tar -xzf ~/src-t23.tgz -C ~/t25f && (cd ~/t25f && git apply ~/t25f.patch) > $R/apply.log 2>&1
(cd ~/t25f && find crates docs -type f \( -name '*.rs' -o -name '*.md' \) | sort | xargs md5sum) > $R/t25f_md5.txt
(cd ~/t25f && cargo fmt --all --check > $R/fmt.log 2>&1; echo "fmt=$?" >> $R/checks.txt
 cargo clippy --workspace --all-targets -- -D warnings > $R/clippy.log 2>&1; echo "clippy=$?" >> $R/checks.txt
 cargo test --workspace --no-fail-fast > $R/test.log 2>&1; echo "test=$?" >> $R/checks.txt
 python3 tools/check_docs.py > $R/check_docs.log 2>&1; echo "check_docs=$?" >> $R/checks.txt)
echo "$(date -u +%T) VM43_ALL_DONE" >> ~/results/progress.txt
