#!/usr/bin/env bash
set -euo pipefail
# On lab, after generating BASE with the original base.toml. Deadline is UTC.
BIN=${1:?p1_bench path}
BASE=${2:?base directory}
OUT=${3:?output directory}
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
date -u +%FT%TZ > "$OUT/start.txt"
sha256sum "$BIN" "$BASE/solution.sol" "$BASE/checkpoint.ckpt" > "$OUT/hashes.txt"
stat -c '%s %n' "$BASE/checkpoint.ckpt" > "$OUT/checkpoint-size.txt"
for spec in 'incentive_turn cold 3000' 'incentive_turn checkpoint 3000' 'incentive_turn reset-continue 3000' 'turn_001 cold 650' 'turn_001 checkpoint 650' 'turn_001 reset-continue 650' 'turn_020 cold 650' 'turn_020 checkpoint 650' 'turn_020 reset-continue 650' 'flop_005 cold 650' 'flop_005 checkpoint 650' 'flop_005 reset-continue 650' 'combined cold 650' 'combined checkpoint 650' 'combined reset-continue 650'; do
  read -r profile mode iters <<< "$spec"
  echo "$(date -u +%FT%TZ) $profile $mode" >> "$OUT/runs.txt"
  # SIGTERM at 09:10 leaves time to retrieve evidence before box termination.
  seconds=$(( $(date -u -d 'today 09:10' +%s) - $(date -u +%s) ))
  (( seconds > 0 )) || break
  timeout "$seconds" "$BIN" "$HERE/configs/$profile.toml" --storage f32 --threads 32 --iters "$iters" \
    --experiment-base "$BASE" --experiment-mode "$mode" --json "$OUT/$profile-$mode.json" \
    > "$OUT/$profile-$mode.stdout" 2> "$OUT/$profile-$mode.stderr" || break
done
date -u +%FT%TZ > "$OUT/end.txt"
