#!/usr/bin/env bash
# S4-1b-1: solve B1 to the NashConv target, re-evaluate each average profile with l0_eval and the independent
# Python push/fold check, then time three DCFR iterations on B3 with 2048 and 256 K4 samples.
# Run from the workspace root after building the examples (see README.md):
#   bash experiments/p2-method-2026-10/trunk-solver/run.sh <out_dir>
set -euo pipefail
out=${1:?usage: run.sh <out_dir>}
mkdir -p "$out"
solve=target/release/examples/trunk_solve
eval=target/release/examples/l0_eval
check=experiments/p2-method-2026-10/l0-evaluator-check/hu_pushfold_check.py

for s in 5 10 20; do
  config=examples/bench/hu_pushfold_${s}bb.toml
  "$solve" --config "$config" --iterations 100000 --eval-every 10 --target-nash-conv 1e-4 \
    --output "$out/b1_${s}bb.json" --output-profile "$out/b1_${s}bb.profile.json" > "$out/b1_${s}bb.log" 2>&1
  "$eval" --config "$config" --profile "json:$out/b1_${s}bb.profile.json" --output "$out/b1_${s}bb.l0eval.json" \
    > "$out/b1_${s}bb.l0eval.log" 2>&1
  python "$check" evaluate --classes .cache/p2-trunk/classes.csv --t2 .cache/p2-trunk/t2.csv \
    --profile "$out/b1_${s}bb.profile.json" --stack "$s" --output "$out/b1_${s}bb.py.json" > /dev/null
  echo "b1_${s}bb done"
done
for k4 in 2048 256; do
  start=$(date +%s)
  "$solve" --config examples/bench/6max_20bb_checkdown.toml --iterations 3 --eval-every 0 --print-every 1 \
    --k4-samples "$k4" --output "$out/b3_k${k4}.json" > "$out/b3_k${k4}.log" 2>&1
  echo "b3_k${k4}: $(( $(date +%s) - start ))s"
done
