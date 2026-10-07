#!/bin/bash
# VM7 phase 4: .sol EV pass without the O(51) combo lookup (branch s3-p1-sol-cards) vs the T16 head.
source ~/.cargo/env
until grep -q KSTATS_DONE ~/results/progress.txt; do sleep 20; done
mkdir -p ~/sc ~/results/vm10 ~/work10 && tar -xzf ~/src-sc.tgz -C ~/sc
(cd ~/sc && cargo build --release -p cli --bin solvers 2>&1 | tail -1; cd ~/new && cargo build --release -p hu-postflop --example verify_save 2>&1 | tail -1) > ~/results/vm10/build.log 2>&1
sed -e '/^check_every = /d' -e '/^target = /d' -e 's/^max_iterations = .*/max_iterations = 25/' -e 's/^\[run\]$/[run]\nfinal_checkpoint = false/' ~/cfg/c_gtowb.toml > ~/work10/g25.toml
echo "$(date -u +%T) SC_SETUP_DONE" >> ~/results/progress.txt
for tag in new_1 sc_1 new_2 sc_2; do
  side=${tag%_*}; out=~/work10/$tag; rm -rf $out
  start=$(date +%s.%N)
  ~/$side/target/release/solvers solve ~/work10/g25.toml --out $out --threads 32 2>&1 | while IFS= read -r line; do printf '%s %s\n' "$(date +%s.%N)" "$line"; done > ~/results/vm10/$tag.lines.txt
  end=$(date +%s.%N)
  done_t=$(grep " done:" ~/results/vm10/$tag.lines.txt | awk '{print $1}')
  echo "$(date -u +%T) sc $tag wall=$(python3 -c "print(round($end - $start, 2))") after_done=$(python3 -c "print(round($end - $done_t, 2))")" >> ~/results/progress.txt
done
~/new/target/release/examples/verify_save solution ~/work10/new_2/solution.sol ~/work10/sc_2/solution.sol > ~/results/vm10/compare.json 2>&1
echo "$(date -u +%T) sc compare $(head -c 300 ~/results/vm10/compare.json)" >> ~/results/progress.txt
rm -rf ~/work10/new_* ~/work10/sc_*
echo "$(date -u +%T) SC_DONE" >> ~/results/progress.txt
