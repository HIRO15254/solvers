#!/usr/bin/env bash
# S4-2a: B6 (HU 20bb with postflop) with the L1 leaf model. Each training change
# is added one at a time, then the recommended settings at other board counts,
# evaluator variants on the same training, L0, and the L1 profiles in L0.
# Run from the workspace root after
#   cargo build -p mw-preflop --release --example trunk_solve --example l0_eval
# usage: bash run_b6.sh <out dir> [EHS2 cache]
# TRUNK_SOLVE and L0_EVAL override the binaries. The recorded runs ran one at a
# time with all threads, from copies of the binaries built at the recorded commit.
set -uo pipefail
out=${1:?out dir}
ehs=${2:-"$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"}
config=examples/bench/hu_20bb_postflop.toml
solve=${TRUNK_SOLVE:-target/release/examples/trunk_solve.exe}
l0_eval=${L0_EVAL:-target/release/examples/l0_eval.exe}
mkdir -p "$out"

l1() { # name iterations eval_every eval_boards [options...]
  local name=$1 iterations=$2 every=$3 eval_boards=$4
  shift 4
  "$solve" --config "$config" --leaf-model l1 --ehs2-cache "$ehs" \
    --iterations "$iterations" --eval-every "$every" --print-every 250 --l1-eval-boards "$eval_boards" "$@" \
    --output "$out/$name.json" --output-profile "$out/$name.profile.json" > "$out/$name.log" 2>&1
  echo "$name exit $?"
}

plain=(--beta 0 --l1-postflop-beta 0 --l1-train-control false --l1-train-regression false --l1-sampling random)
eval_reg=(--l1-eval-control true --l1-eval-regression true --l1-eval-sampling random)
final=(--beta 1 --l1-postflop-beta 0 --l1-train-control true --l1-train-regression true --l1-sampling stratified)

# Training changes, one at a time (evaluation: 4096 boards with the regression control variate).
l1 b6-n1-plain 2000 250 4096 --l1-boards 1 "${plain[@]}" "${eval_reg[@]}"
l1 b6-n32-plain 2000 250 4096 --l1-boards 32 "${plain[@]}" "${eval_reg[@]}"
l1 b6-n32-cv 2000 250 4096 --l1-boards 32 --beta 0 --l1-postflop-beta 0 --l1-train-control true --l1-train-regression false \
  --l1-sampling random "${eval_reg[@]}"
l1 b6-n32-cv-strat 2000 250 4096 --l1-boards 32 --beta 0 --l1-postflop-beta 0 --l1-train-control true --l1-train-regression false \
  --l1-sampling stratified "${eval_reg[@]}"
l1 b6-n32-cv-strat-reg 2000 250 4096 --l1-boards 32 --beta 0 --l1-postflop-beta 0 --l1-train-control true --l1-train-regression true \
  --l1-sampling stratified "${eval_reg[@]}"
l1 b6-n32-final 2000 250 4096 --l1-boards 32 "${final[@]}" "${eval_reg[@]}"
# The same with DCFR's beta 1 for the postflop strategies too.
l1 b6-n32-final-postflop1 2000 250 4096 --l1-boards 32 "${final[@]}" --l1-postflop-beta 1 "${eval_reg[@]}"
l1 b6-n8-final 2000 250 4096 --l1-boards 8 "${final[@]}" "${eval_reg[@]}"
l1 b6-n64-final 2000 250 4096 --l1-boards 64 "${final[@]}" "${eval_reg[@]}"

# The evaluator on the training of b6-n32-final.
l1 b6-n32-final-evalplain 2000 1000 4096 --l1-boards 32 "${final[@]}" --l1-eval-control false \
  --l1-eval-regression false
l1 b6-n32-final-evalcv 2000 1000 4096 --l1-boards 32 "${final[@]}" --l1-eval-control true \
  --l1-eval-regression false
l1 b6-n32-final-eval16k 2000 1000 16384 --l1-boards 32 "${final[@]}" "${eval_reg[@]}"

"$solve" --config "$config" --leaf-model l0 --iterations 2000 --eval-every 250 \
  --output "$out/b6-l0.json" --output-profile "$out/b6-l0.profile.json" > "$out/b6-l0.log" 2>&1
echo "b6-l0 exit $?"
for n in b6-n1-plain b6-n32-final; do
  "$l0_eval" --config "$config" --profile "json:$out/$n.profile.json" \
    --output "$out/$n.l0eval.json" > "$out/$n.l0eval.log" 2>&1
  echo "$n l0_eval exit $?"
done
