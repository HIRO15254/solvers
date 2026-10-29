#!/bin/bash
# Checks the P1 memory estimate against complete CLI solves (run on the Linux box).
# Usage: validate.sh BIN_DIR OUT_DIR [CASE...]
# A CASE is NAME:STORAGE:THREADS with NAME a configs/*.toml stem or lockflop_srp /
# lockturn_srp (flop_srp.toml plus a lock fragment). Each case gets a config with
# that storage, thread count and a 10-iteration stop; `validate --resources`
# gives memoryEstimateBytes, then `solve` (10 iterations, final evaluation,
# checkpoint and .sol) runs under /usr/bin/time for its maximum RSS.
set -eu
BIN=$1
OUT=$2
shift 2
CFG=$(cd "$(dirname "$0")/configs" && pwd)
mkdir -p "$OUT"
[ $# -gt 0 ] || set -- flop_srp:f32:16 flop_srp:f32:32 flop_srp:i16:32 flop_3bp:f32:32 \
  flop_4bp:f32:32 c_gtowb:i16:16 c_gtowb:i16:32 lockflop_srp:f32:32 lockturn_srp:f32:32 \
  turn_wide:f32:32 river_wide:f32:32 river_srp:f32:1 turn_srp:f32:1
for case in "$@"; do
  IFS=: read -r name storage threads <<< "$case"
  c="$OUT/$name-$storage-t$threads.toml"
  case $name in
    lockflop_srp) cat "$CFG/flop_srp.toml" "$CFG/lock_flop.toml" > "$c" ;;
    lockturn_srp) cat "$CFG/flop_srp.toml" "$CFG/lock_turn.toml" > "$c" ;;
    *) cp "$CFG/$name.toml" "$c" ;;
  esac
  sed -i -e '/^max_iterations/d' -e '/^check_every/d' -e '/^target = /d' -e '/^storage = /d' \
    -e '/^threads = /d' -e '/^max_time = /d' "$c"
  if grep -q '^\[solver\]' "$c"; then sed -i "s/^\[solver\]/[solver]\nstorage = \"$storage\"/" "$c"
  else printf '\n[solver]\nstorage = "%s"\n' "$storage" >> "$c"; fi
  if grep -q '^\[solver.stop\]' "$c"; then
    sed -i 's/^\[solver.stop\]/[solver.stop]\ntarget = "0.0001%pot"\nmax_iterations = 10\ncheck_every = 10/' "$c"
  else printf '\n[solver.stop]\ntarget = "0.0001%%pot"\nmax_iterations = 10\ncheck_every = 10\n' >> "$c"; fi
  if grep -q '^\[run\]' "$c"; then sed -i "s/^\[run\]/[run]\nthreads = $threads/" "$c"
  else printf '\n[run]\nthreads = %s\n' "$threads" >> "$c"; fi
  stem="$OUT/$name-$storage-t$threads"
  "$BIN/solvers" validate "$c" --format json --resources > "$stem.validate.json"
  rm -rf "$stem.run"
  /usr/bin/time -f '%M %e' -o "$stem.time" "$BIN/solvers" solve "$c" --out "$stem.run" > "$stem.log" 2>&1
  read -r rss secs < "$stem.time"
  jq -c --arg case "$case" --argjson rss "$rss" --argjson secs "$secs" \
    '{case: $case, maxRssBytes: ($rss * 1024), wallSecs: $secs, resources: .resources}' \
    "$stem.validate.json" >> "$OUT/validate.jsonl"
  rm -rf "$stem.run"
done
