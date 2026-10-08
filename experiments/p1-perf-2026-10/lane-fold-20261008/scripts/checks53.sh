#!/bin/bash
# VM10: fmt/clippy/test/check_docs on the final T26 patch (one comment reworded after the measured tree; code equal).
source ~/.cargo/env
until grep -q VM51_ALL_DONE ~/results/progress.txt; do sleep 20; done
R=~/results/vm53; mkdir -p $R
mkdir -p ~/t26f && tar -xzf ~/src-t25.tgz -C ~/t26f && (cd ~/t26f && git apply ~/t26f.patch) > $R/apply.log 2>&1
(cd ~/t26f && find crates docs -type f \( -name '*.rs' -o -name '*.md' \) | sort | xargs md5sum) > $R/t26f_md5.txt
(cd ~/t26f && cargo fmt --all --check > $R/fmt.log 2>&1; echo "fmt=$?" >> $R/checks.txt
 cargo clippy --workspace --all-targets -- -D warnings > $R/clippy.log 2>&1; echo "clippy=$?" >> $R/checks.txt
 cargo test --workspace --no-fail-fast > $R/test.log 2>&1; echo "test=$?" >> $R/checks.txt
 python3 tools/check_docs.py > $R/check_docs.log 2>&1; echo "check_docs=$?" >> $R/checks.txt)
echo "$(date -u +%T) VM53_ALL_DONE" >> ~/results/progress.txt
