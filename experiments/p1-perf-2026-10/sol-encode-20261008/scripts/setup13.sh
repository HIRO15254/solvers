#!/bin/bash
# VM7 phase 7: T19 (O(1) combo cards + bulk value blocks encoded in EV workers) vs the T16 head, save time on tmpfs and disk.
source ~/.cargo/env
rm -rf ~/sc2 && mkdir -p ~/sc2 && tar -xzf ~/src-sc2.tgz -C ~/sc2
(cd ~/sc2 && cargo build --release -p cli --bin solvers 2>&1 | tail -1) > ~/results/vm10/build_sc2.log 2>&1
run() { # tag side dir
  tag=$1; side=$2; out=$3/$tag; rm -rf $out
  start=$(date +%s.%N)
  /usr/bin/time -v -o ~/results/vm10/t19_$tag.time.txt ~/$side/target/release/solvers solve ~/work10/g25.toml --out $out --threads 32 2>&1 | while IFS= read -r line; do printf '%s %s\n' "$(date +%s.%N)" "$line"; done > ~/results/vm10/t19_$tag.lines.txt
  end=$(date +%s.%N)
  done_t=$(grep " done:" ~/results/vm10/t19_$tag.lines.txt | awk '{print $1}')
  echo "$(date -u +%T) t19 $tag wall=$(python3 -c "print(round($end - $start, 2))") after_done=$(python3 -c "print(round($end - $done_t, 2))")" >> ~/results/progress.txt
}
for r in 1 2 3; do
  run shm_new_$r new /dev/shm; [ $r -lt 3 ] && rm -rf /dev/shm/shm_new_$r
  run shm_sc2_$r sc2 /dev/shm; [ $r -lt 3 ] && rm -rf /dev/shm/shm_sc2_$r
done
~/new/target/release/examples/verify_save solution /dev/shm/shm_new_3/solution.sol /dev/shm/shm_sc2_3/solution.sol > ~/results/vm10/t19_compare.json 2>&1
echo "$(date -u +%T) t19 compare $(head -c 300 ~/results/vm10/t19_compare.json)" >> ~/results/progress.txt
rm -rf /dev/shm/shm_*
run disk_new_1 new ~/work10; rm -rf ~/work10/disk_new_1
run disk_sc2_1 sc2 ~/work10; rm -rf ~/work10/disk_sc2_1
echo "$(date -u +%T) T19_DONE" >> ~/results/progress.txt
