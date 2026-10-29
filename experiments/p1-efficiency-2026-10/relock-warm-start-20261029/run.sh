#!/usr/bin/env bash
set -euo pipefail
# Run on the lab; BIN and BASE are absolute paths. Evaluations are every iteration.
BIN=${1:?p1_bench binary}
BASE=${2:?base run directory}
OUT=${3:?output directory}
PROFILES=${PROFILES:-"lock_flop lock_turn incentive_turn strong_lock"}
THREADS=${THREADS:-32}
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
test -f "$OUT/start.txt" || date -u +%FT%TZ > "$OUT/start.txt"
for mode in cold checkpoint reset-restart reset-continue strategy-0.1 strategy-1 strategy-10; do
  for profile in $PROFILES; do
    test -s "$OUT/$profile-$mode.json" && continue
    if [[ $(date -u +%H%M) > 0725 ]]; then
      echo "guard: no new run after 07:25 UTC" >&2
      exit 1
    fi
    echo "$(date -u +%FT%TZ) $profile $mode" >&2
    timeout 1080s "$BIN" "$HERE/configs/$profile.toml" --storage f32 --threads "$THREADS" --iters 1200 \
      --experiment-base "$BASE" --experiment-mode "$mode" --json "$OUT/$profile-$mode.json" \
      > "$OUT/$profile-$mode.stdout"
  done
done
date -u +%FT%TZ > "$OUT/end.txt"
