#!/usr/bin/env bash
# Paired timing: pass binaries A,B,C,C,B,A on B4 Simple and B7, then K4 minimum samples with the newest binary.
set -uo pipefail
out=.cache/p2-trunk/l1-6max/timing
ehs="$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"
b7=examples/bench/6max_20bb.toml
b4s=examples/bench/6max_100bb_nl50_partial_simple_reference.toml
run() { # tag bin config iterations [options...]
  local tag=$1 bin=$2 config=$3 iterations=$4
  shift 4
  .cache/p2-trunk/l1-core/bin/trunk_solve-$bin.exe --config "$config" --leaf-model l1 --ehs2-cache "$ehs" \
    --solver-k4-samples 256 --iterations "$iterations" --eval-every 0 --print-every 1 "$@" \
    --output "$out/$tag.json" > "$out/$tag.log" 2>&1
  echo "$tag exit $? $(date +%T)"
}
i=0
for bin in 30df60b sparse memo guide guide memo sparse 30df60b; do
  i=$((i + 1)); run b4s-$i-$bin $bin "$b4s" 4
done
i=0
for bin in 30df60b memo guide guide memo 30df60b; do
  i=$((i + 1)); run b7-$i-$bin $bin "$b7" 4
done
i=0
for m in 16 64 64 16; do
  i=$((i + 1)); run b4s-min-$i-m$m guide "$b4s" 4 --solver-k4-min-samples $m
done
i=0
for m in 0 16 64 64 16 0; do
  i=$((i + 1))
  opts=()
  [ "$m" != 0 ] && opts=(--solver-k4-min-samples $m)
  .cache/p2-trunk/l1-core/bin/trunk_solve-guide.exe --config examples/bench/6max_20bb_checkdown.toml     --solver-k4-samples 256 "${opts[@]}" --iterations 10 --eval-every 0 --print-every 1     --output "$out/b3-min-$i-m$m.json" > "$out/b3-min-$i-m$m.log" 2>&1
  echo "b3-min-$i-m$m exit $? $(date +%T)"
done
echo finished
