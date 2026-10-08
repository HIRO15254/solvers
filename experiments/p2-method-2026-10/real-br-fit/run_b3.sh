#!/usr/bin/env bash
# B3: the four legacy 20bb solutions. Fit each seat's class-level responses (pure and gated) on deal seed 1 and
# evaluate them, together with L0's best responses, on deal seed 0. Then refit the 300k seed-0 solution with
# 2^21, 2^25 and 2^27 deals (the fitting deals are nested), and once with 8 threads (must be bit-identical).
# Run from the workspace root after building the examples (see README.md):
#   bash experiments/p2-method-2026-10/real-br-fit/run_b3.sh <out_dir> [deals] [fit_deals]
# The recorded run used the defaults: 2^24 evaluation deals and 2^23 fitting deals.
set -euo pipefail
out=${1:?usage: run_b3.sh <out_dir> [deals] [fit_deals]}
deals=${2:-16777216}
fit_deals=${3:-8388608}
mkdir -p "$out"
real_bin=target/release/examples/l0_real
runs=runs/p2-method-2026-10/legacy-seed-noise

# run <name> <fitting deals> <l0_real arguments...>: fit, evaluate and print the wall time.
run() {
  local name=$1 fit=$2 start
  shift 2
  start=$(date +%s)
  "$real_bin" "$@" --fit-deals "$fit" --fit-seed 1 --fit-thresholds 1,2,3 --deals "$deals" --deal-seed 0 \
    --output "$out/$name.json" > "$out/$name.log" 2>&1
  echo "$name: $(( $(date +%s) - start ))s"
}

for name in 20bb_300k_s0 20bb_300k_s1 20bb_cd 20bb_cd_s1; do
  run "$name" "$fit_deals" --mwsol "$runs/$name/solution.mwsol"
done
s0=$runs/20bb_300k_s0/solution.mwsol
run 20bb_300k_s0_fit2097152_threads8 2097152 --mwsol "$s0" --threads 8
for fit in 2097152 33554432 134217728; do
  run "20bb_300k_s0_fit$fit" "$fit" --mwsol "$s0"
done
