#!/usr/bin/env bash
set -uo pipefail
out=.cache/p2-trunk/l1-core/bitcheck
ehs="$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"
run() { # binary-tag name options...
  local bin=$1 name=$2; shift 2
  .cache/p2-trunk/l1-core/bin/trunk_solve-$bin.exe --config examples/bench/hu_20bb_postflop.toml --leaf-model l1 \
    --ehs2-cache "$ehs" --iterations 300 --eval-every 100 --print-every 100 --l1-eval-boards 1024 --l1-boards 32 "$@" \
    --output "$out/$name-$bin.json" --output-profile "$out/$name-$bin.profile.json" > "$out/$name-$bin.log" 2>&1
  echo "$name-$bin exit $?"
}
for bin in ${BINS:-34d7811 heap}; do
  run $bin final --beta 1 --l1-postflop-beta 0.5 --l1-train-control true --l1-train-regression true --l1-sampling stratified \
    --l1-eval-control true --l1-eval-regression true
  run $bin plain --beta 0 --l1-postflop-beta 0 --l1-train-control false --l1-train-regression false --l1-sampling random \
    --l1-eval-control false --l1-eval-regression false
done
