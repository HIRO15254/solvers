#!/bin/bash
# VM7 phase 5: the same .sol timing with the output on tmpfs, so the disk does not bound the save.
for tag in new_1 sc_1 new_2 sc_2 new_3 sc_3; do
  side=${tag%_*}; out=/dev/shm/$tag; rm -rf $out
  start=$(date +%s.%N)
  ~/$side/target/release/solvers solve ~/work10/g25.toml --out $out --threads 32 2>&1 | while IFS= read -r line; do printf '%s %s\n' "$(date +%s.%N)" "$line"; done > ~/results/vm10/shm_$tag.lines.txt
  end=$(date +%s.%N)
  done_t=$(grep " done:" ~/results/vm10/shm_$tag.lines.txt | awk '{print $1}')
  echo "$(date -u +%T) shm $tag wall=$(python3 -c "print(round($end - $start, 2))") after_done=$(python3 -c "print(round($end - $done_t, 2))")" >> ~/results/progress.txt
  rm -rf $out
done
echo "$(date -u +%T) SHM_DONE" >> ~/results/progress.txt
