#!/usr/bin/env bash
# Solve B3 for 200 DCFR iterations (checkpoints every 20), re-evaluate the average profile with other K4 and T3
# seeds, and bracket its class-level NashConv in the input game as in ../real-br-fit (fit 2^27 deals of seed 1,
# evaluate on 2^24 deals of seed 0). The input-game step needs l0_real (see ../real-br-fit/README.md).
# Run from the workspace root after building the examples:
#   bash experiments/p2-method-2026-10/trunk-solver/run_b3_trial.sh <out_dir>
set -euo pipefail
out=${1:?usage: run_b3_trial.sh <out_dir>}
mkdir -p "$out"
config=examples/bench/6max_20bb_checkdown.toml
target/release/examples/trunk_solve --config "$config" --iterations 200 --eval-every 20 --print-every 1 \
  --output "$out/b3_200.json" --output-profile "$out/b3_200.profile.json" > "$out/b3_200.log" 2>&1
for args in "--seed 0" "--seed 1" "--t3-seed 1" "--seed 1 --t3-seed 1"; do
  tag=$(echo "$args" | tr -d ' -')
  # shellcheck disable=SC2086
  target/release/examples/l0_eval --config "$config" --profile "json:$out/b3_200.profile.json" $args \
    --output "$out/l0eval_$tag.json" > "$out/l0eval_$tag.log" 2>&1
done
target/release/examples/l0_real --config "$config" --profile "json:$out/b3_200.profile.json" \
  --fit-deals 134217728 --fit-seed 1 --fit-thresholds 1,2,3 --deals 16777216 --deal-seed 0 \
  --output "$out/real_fit134217728.json" > "$out/real_fit134217728.log" 2>&1
