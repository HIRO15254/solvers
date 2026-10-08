#!/usr/bin/env bash
# B1: in heads-up L0 is the input game, so every fitted response's held-out gain is at most L0's exact gain,
# and the pure response's in-sample gain overestimates it. Fit on deal seed 1, evaluate on deal seed 0. Also
# evaluate L0's best responses on the fitting deals: there the pure response's gain is at least theirs.
# Run from the workspace root after building the examples (see README.md):
#   bash experiments/p2-method-2026-10/real-br-fit/run_b1.sh <out_dir>
# The legacy-solution cases run only when runs/p2-method-2026-10/l0-evaluator-check/b1_<s>bb exists.
set -euo pipefail
out=${1:?usage: run_b1.sh <out_dir>}
mkdir -p "$out"
real_bin=target/release/examples/l0_real
profiles=experiments/p2-method-2026-10/l0-real-check/results/b1
legacy=runs/p2-method-2026-10/l0-evaluator-check
deals=4194304
fit_deals=4194304

# run <name> <l0_real arguments...>: fit, evaluate, evaluate on the fitting deals and print the wall time.
run() {
  local name=$1 start
  shift
  start=$(date +%s)
  "$real_bin" "$@" --fit-deals "$fit_deals" --fit-seed 1 --fit-thresholds 1,2,3 --deals "$deals" --deal-seed 0 \
    --output "$out/$name.json" > "$out/$name.log" 2>&1
  "$real_bin" "$@" --deals "$fit_deals" --deal-seed 1 \
    --output "$out/${name}_fit_deals.json" > "$out/${name}_fit_deals.log" 2>&1
  echo "$name: $(( $(date +%s) - start ))s"
}

for s in 5 10 20; do
  config=examples/bench/hu_pushfold_${s}bb.toml
  run "b1_${s}bb_uniform" --config "$config" --profile uniform
  run "b1_${s}bb_random" --config "$config" --profile "json:$profiles/b1_${s}bb_random.profile.json"
  if [ -f "$legacy/b1_${s}bb/solution.mwsol" ]; then
    run "b1_${s}bb_legacy" --mwsol "$legacy/b1_${s}bb/solution.mwsol"
  fi
done
