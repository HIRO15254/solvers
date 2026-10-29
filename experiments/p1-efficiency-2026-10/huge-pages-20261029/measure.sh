#!/usr/bin/env bash
# Run on the exclusive bench host: measure.sh ROUND OUTPUT_DIRECTORY
set -euo pipefail
round=$1
out=$2
mkdir -p "$out"
thp=/sys/kernel/mm/transparent_hugepage
# Never leave a system-wide setting behind, including on interrupt or failure.
restore() { echo madvise | sudo tee "$thp/enabled" >/dev/null; }
trap restore EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
case "$round" in
  1) modes=(never madvise always) ;;
  2) modes=(always madvise never) ;;
  3) modes=(madvise never always) ;;
  *) exit 2 ;;
esac
for mode in "${modes[@]}"; do
  echo "$mode" | sudo tee "$thp/enabled" >/dev/null
  for threads in 16 32; do
    for spec in flop_srp:f32 flop_srp:i16 flop_3bp:f32 c_gtowb:i16; do
      cfg=${spec%:*}; storage=${spec#*:}
      name="$cfg-$storage-t$threads-$mode-r$round"
      /usr/bin/time -o "$out/$name.time" -f 'minorFaults=%R\nmajorFaults=%F\nmaxRssKiB=%M\nsystemSecs=%S\nuserSecs=%U\nelapsedSecs=%e' \
        /work/bin-HugePages/p1_bench "/work/cfg/$cfg.toml" --storage "$storage" --threads "$threads" --warmup 2 --iters 6 --evals 2 --json "$out/$name.json" > "$out/$name.stdout" &
      timer=$!
      # The timer's direct child is p1_bench; capture process huge-page usage
      # repeatedly because startup and evaluation have different live sets.
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
