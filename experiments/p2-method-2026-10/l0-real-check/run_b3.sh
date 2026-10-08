#!/usr/bin/env bash
# B3: the four legacy 20bb solutions and the uniform profile, L0 against the input game's deals,
# plus the 300k seed-0 solution with 8 threads (bit-identical check) and with deal seed 1.
# Run from the workspace root after building the examples (see README.md):
#   bash experiments/p2-method-2026-10/l0-real-check/run_b3.sh <out_dir> [deals]
# The recorded run used the default of 2^25 deals per profile.
set -euo pipefail
out=${1:?usage: run_b3.sh <out_dir> [deals]}
deals=${2:-33554432}
mkdir -p "$out"
real_bin=target/release/examples/l0_real
runs=runs/p2-method-2026-10/legacy-seed-noise

# run <name> <l0_real arguments...>: evaluate and print the wall time.
run() {
  local name=$1 start
  shift
  start=$(date +%s)
  "$real_bin" "$@" --deals "$deals" --output "$out/$name.json" > "$out/$name.log" 2>&1
  echo "$name: $(( $(date +%s) - start ))s"
}

for name in 20bb_cd 20bb_cd_s1 20bb_300k_s0 20bb_300k_s1; do
  run "$name" --mwsol "$runs/$name/solution.mwsol"
done
run uniform --config examples/bench/6max_20bb_checkdown.toml --profile uniform
run 20bb_300k_s0_threads8 --mwsol "$runs/20bb_300k_s0/solution.mwsol" --threads 8
run 20bb_300k_s0_dealseed1 --mwsol "$runs/20bb_300k_s0/solution.mwsol" --deal-seed 1
