#!/usr/bin/env bash
# S4-2a: B7 (6-max 20bb with postflop) with the L1 leaf model: iteration and
# evaluation time with the recommended L1 settings (32 boards) and with 8 boards.
# Run from the workspace root after
#   cargo build -p mw-preflop --release --example trunk_solve
# usage: bash run_b7.sh <out dir> [EHS2 cache]
set -uo pipefail
out=${1:?out dir}
ehs=${2:-"$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"}
solve=${TRUNK_SOLVE:-target/release/examples/trunk_solve.exe}
mkdir -p "$out"
b7() { # name boards eval_every
  "$solve" --config examples/bench/6max_20bb.toml \
    --leaf-model l1 --ehs2-cache "$ehs" --solver-k4-samples 256 \
    --iterations 10 --eval-every "$3" --print-every 1 --l1-boards "$2" --l1-eval-boards 1024 \
    --beta 1 --l1-postflop-beta 0 --l1-train-control true --l1-train-regression true --l1-sampling stratified \
    --l1-eval-control true --l1-eval-regression true --l1-eval-sampling random \
    --output "$out/$1.json" --output-profile "$out/$1.profile.json" > "$out/$1.log" 2>&1
  echo "$1 exit $?"
}
b7 b7-n32 32 10
b7 b7-n8 8 0
