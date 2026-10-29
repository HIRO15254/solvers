#!/usr/bin/env bash
# Run under rc.py --bench: selective.sh ROUND OUT
set -euo pipefail
round=$1; out=$2
mkdir -p "$out"
[[ $(cat /sys/kernel/mm/transparent_hugepage/enabled) == *'[madvise]'* ]]
if (( round % 2 )); then versions=(old new); else versions=(new old); fi
for threads in 16 32; do
  for spec in flop_srp:f32 flop_srp:i16 flop_3bp:f32 c_gtowb:i16; do
    cfg=${spec%:*}; storage=${spec#*:}
    for version in "${versions[@]}"; do
      name="$cfg-$storage-t$threads-$version-r$round"
      /usr/bin/time -o "$out/$name.time" -f 'minorFaults=%R\nmajorFaults=%F\nmaxRssKiB=%M\nsystemSecs=%S\nuserSecs=%U\nelapsedSecs=%e' \
        "/work/bin-HugePages/p1_bench-$version" "/work/cfg/$cfg.toml" --storage "$storage" --threads "$threads" --warmup 2 --iters 10 --evals 2 --json "$out/$name.json" > "$out/$name.stdout" &
      timer=$!
      while kill -0 "$timer" 2>/dev/null; do
        pid=$(cat "/proc/$timer/task/$timer/children" 2>/dev/null || true)
        pid=${pid%% *}
        if [[ $pid =~ ^[0-9]+$ && -r /proc/$pid/smaps_rollup ]]; then
          { date -u +%FT%TZ; cat "/proc/$pid/smaps_rollup"; } >> "$out/$name.smaps" 2>/dev/null || true
        fi
        sleep 1
      done
      wait "$timer"
    done
  done
done
