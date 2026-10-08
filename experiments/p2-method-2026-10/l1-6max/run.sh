#!/usr/bin/env bash
# S4-2b: solve B7 (6-max 20bb with Postflop) and B4 Simple's original tree (6-max 100bb, Postflop) with the L1 leaf
# model in the P2D5 budget, and with L0 (the same trees, Postflop as checkdown) for the L0-L1 difference.
# Run from the workspace root after
#   cargo build -p mw-preflop --release --example trunk_solve --example l0_eval
# usage: bash run.sh <out dir> [EHS2 cache]
# TRUNK_SOLVE and L0_EVAL override the binaries. ONLY (run names separated by spaces) runs just those runs and the
# l0_eval of the listed L1 runs, e.g. to repeat a failed run.
set -uo pipefail
out=${1:?out dir}
ehs=${2:-"$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"}
solve=${TRUNK_SOLVE:-target/release/examples/trunk_solve.exe}
l0_eval=${L0_EVAL:-target/release/examples/l0_eval.exe}
# The solver's K4 (P2D7): 256 samples per iteration, and at least 16 where the opponents' reach is small.
k4=(--solver-k4-samples 256)
min=(--solver-k4-min-samples 16)
b7=examples/bench/6max_20bb.toml
b4s=examples/bench/6max_100bb_nl50_partial_simple_reference.toml
only=${ONLY:-}
mkdir -p "$out"
selected() { [[ -z $only || " $only " == *" $1 "* ]]; }
run() { # name config leaf iterations eval_every [options...]
  local name=$1 config=$2 leaf=$3 iterations=$4 every=$5
  shift 5
  selected "$name" || return 0
  echo "$name start $(date -Iseconds)"
  "$solve" --config "$config" --leaf-model "$leaf" --ehs2-cache "$ehs" "${k4[@]}" --iterations "$iterations" \
    --eval-every "$every" --print-every 10 "$@" --output "$out/$name.json" --output-profile "$out/$name.profile.json" \
    > "$out/$name.log" 2>&1
  echo "$name exit $? $(date -Iseconds)"
}
# L1 with the S4-2a defaults (32 stratified boards, control variates with regression, trunk beta 1, Postflop beta 0).
run b7-l1 "$b7" l1 2000 250 --l1-eval-boards 1024 "${min[@]}"
# L0 (beta 0, its default) on the same trees. B7 in L0 is B3; b7-l0-all checks the minimum samples to 500
# iterations against all 256 samples.
run b7-l0 "$b7" l0 500 50 "${min[@]}"
run b7-l0-all "$b7" l0 500 50
run b4s-l0 "$b4s" l0 500 100 "${min[@]}"
# B4 Simple with L1 (the longest run) last.
run b4s-l1 "$b4s" l1 2000 250 --l1-eval-boards 1024 "${min[@]}"
# The L1 solutions' Preflop in the L0 model.
for n in b7-l1:"$b7" b4s-l1:"$b4s"; do
  selected "${n%%:*}" || continue
  "$l0_eval" --config "${n#*:}" --profile "json:$out/${n%%:*}.profile.json" \
    --output "$out/${n%%:*}.l0eval.json" > "$out/${n%%:*}.l0eval.log" 2>&1
  echo "${n%%:*} l0_eval exit $?"
done
echo "finished $(date -Iseconds)"
