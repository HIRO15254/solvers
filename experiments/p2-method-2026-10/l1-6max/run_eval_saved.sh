#!/usr/bin/env bash
# S4-2b follow-up: evaluate saved L1 solutions (class profile and Postflop average) without solving again. For each
# solution: 1024 boards with eval seeds 0-7, random and stratified, to see each sampling's spread; then 8192 boards,
# seed 0, random and stratified.
# usage: bash run_eval_saved.sh <out dir> <name>:<config>:<solution prefix>...   (the prefix's .profile.json and
#        .postflop.bin are read; TRUNK_SOLVE overrides the binary, EHS2 the cache, SEEDS the 1024-board seeds:
#        SEEDS= skips them)
set -uo pipefail
out=${1:?out dir}
shift
ehs=${EHS2:-"$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"}
solve=${TRUNK_SOLVE:-target/release/examples/trunk_solve.exe}
seeds=${SEEDS-0 1 2 3 4 5 6 7}
mkdir -p "$out"
evaluate() { # name config prefix boards seed sampling
  local tag=$1-$6-$4-s$5
  "$solve" --config "$2" --leaf-model l1 --ehs2-cache "$ehs" --evaluate-profile "$3.profile.json" \
    --evaluate-postflop "$3.postflop.bin" --l1-eval-boards "$4" --l1-eval-seed "$5" --l1-eval-sampling "$6" \
    --output "$out/$tag.json" > "$out/$tag.log" 2>&1
  echo "$tag exit $? $(date -Iseconds)"
}
for item in "$@"; do
  IFS=: read -r name config prefix <<< "$item"
  for seed in $seeds; do
    for sampling in random stratified; do
      evaluate "$name" "$config" "$prefix" 1024 "$seed" "$sampling"
    done
  done
  for sampling in random stratified; do
    evaluate "$name" "$config" "$prefix" 8192 0 "$sampling"
  done
done
echo "finished $(date -Iseconds)"
