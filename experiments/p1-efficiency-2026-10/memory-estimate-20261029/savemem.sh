#!/bin/bash
# Save-phase maximum RSS of complete CLI solves by thread count (run on the Linux box).
# Usage: savemem.sh BIN_DIR OUT_DIR [CONFIG_STEM] [STORAGE] [THREADS...]
# For each thread count (default 1 8 16 32), solves CONFIG_STEM (default flop_srp,
# f32; lockflop_srp is flop_srp.toml plus lock_flop.toml) for 10 iterations
# in three finalization modes and records `/usr/bin/time` maximum RSS:
#   parallel  default memory limit: final checkpoint and .sol written concurrently;
#   serial    limit halfway between the serial and parallel estimates: checkpoint,
#             regret release, then .sol;
#   nockpt    final_checkpoint = false: regret release, then .sol only.
set -eu
BIN=$1
OUT=$2
STEM=${3:-flop_srp}
STORAGE=${4:-f32}
shift $(($# < 4 ? $# : 4))
[ $# -gt 0 ] || set -- 1 8 16 32
CFG=$(cd "$(dirname "$0")/configs" && pwd)
mkdir -p "$OUT"
for threads in "$@"; do
  for mode in parallel serial nockpt; do
    c="$OUT/$STEM-$STORAGE-t$threads-$mode.toml"
    case $STEM in
      lockflop_srp) cat "$CFG/flop_srp.toml" "$CFG/lock_flop.toml" > "$c" ;;
      *) cp "$CFG/$STEM.toml" "$c" ;;
    esac
    sed -i -e '/^max_iterations/d' -e '/^check_every/d' -e '/^target = /d' -e '/^storage = /d' \
      -e '/^threads = /d' -e '/^max_time = /d' -e '/^memory = /d' -e '/^final_checkpoint = /d' "$c"
    if grep -q '^\[solver\]' "$c"; then sed -i "s/^\[solver\]/[solver]\nstorage = \"$STORAGE\"/" "$c"
    else printf '\n[solver]\nstorage = "%s"\n' "$STORAGE" >> "$c"; fi
    if grep -q '^\[solver.stop\]' "$c"; then
      sed -i 's/^\[solver.stop\]/[solver.stop]\ntarget = "0.0001%pot"\nmax_iterations = 10\ncheck_every = 10/' "$c"
    else printf '\n[solver.stop]\ntarget = "0.0001%%pot"\nmax_iterations = 10\ncheck_every = 10\n' >> "$c"; fi
    extra="threads = $threads"
    [ "$mode" = nockpt ] && extra="$extra\nfinal_checkpoint = false"
    if grep -q '^\[run\]' "$c"; then sed -i "s/^\[run\]/[run]\n$extra/" "$c"
    else printf '\n[run]\n%b\n' "$extra" >> "$c"; fi
    stem="${c%.toml}"
    "$BIN/solvers" validate "$c" --format json --resources > "$stem.validate.json"
    if [ "$mode" = serial ]; then
      limit=$(jq '.resources | (if .storage == "f32" then .f32Bytes elif .storage == "i16" then .i16Bytes else .i16F32avgBytes end) as $s
        | (.memoryEstimateBytes + $s + .saveWorkspaceBytes + .compressionWorkspaceBytes + .checkpointWorkspaceBytes
           + .residentBytes) / 2 | floor' "$stem.validate.json")
      sed -i "s/^\[run\]/[run]\nmemory = $limit/" "$c"
      "$BIN/solvers" validate "$c" --format json --resources > "$stem.validate.json"
    fi
    rm -rf "$stem.run"
    /usr/bin/time -f '%M %e' -o "$stem.time" "$BIN/solvers" solve "$c" --out "$stem.run" > "$stem.log" 2>&1
    read -r rss secs < "$stem.time"
    jq -c --arg case "$STEM:$STORAGE:$threads:$mode" --argjson rss "$rss" --argjson secs "$secs" \
      '{case: $case, maxRssBytes: ($rss * 1024), wallSecs: $secs, resources: .resources}' \
      "$stem.validate.json" >> "$OUT/savemem.jsonl"
    rm -rf "$stem.run"
  done
done
