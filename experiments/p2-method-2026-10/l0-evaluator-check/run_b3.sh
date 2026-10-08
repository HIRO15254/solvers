#!/usr/bin/env bash
# Benchmark B3: the L0 evaluator on the four legacy 20bb solutions, the uniform profile, and the sensitivity
# of the 300k seed-0 result to the thread count, the larger-showdown seed and the T3 seed.
# Run from the workspace root after building the examples (see README.md):
#   bash experiments/p2-method-2026-10/l0-evaluator-check/run_b3.sh <out_dir>
set -euo pipefail
out=${1:?usage: run_b3.sh <out_dir>}
mkdir -p "$out"
eval_bin=target/release/examples/l0_eval
runs=runs/p2-method-2026-10/legacy-seed-noise

# run <name> <l0_eval arguments...>: evaluate and print the wall time.
run() {
  local name=$1 start
  shift
  start=$(date +%s)
  "$eval_bin" "$@" --top-infosets 20 --output "$out/$name.json" > "$out/$name.log" 2>&1
  echo "$name: $(( $(date +%s) - start ))s"
}

for name in 20bb_cd 20bb_cd_s1 20bb_300k_s0 20bb_300k_s1; do
  run "$name" --mwsol "$runs/$name/solution.mwsol"
done
run uniform --config examples/bench/6max_20bb_checkdown.toml --profile uniform
run 20bb_300k_s0_threads8 --mwsol "$runs/20bb_300k_s0/solution.mwsol" --threads 8
run 20bb_300k_s0_k4seed1 --mwsol "$runs/20bb_300k_s0/solution.mwsol" --seed 1
# The first run with T3 seed 1 builds that table (about 2 minutes) into .cache/p2-trunk.
run 20bb_300k_s0_t3seed1 --mwsol "$runs/20bb_300k_s0/solution.mwsol" --t3-seed 1
