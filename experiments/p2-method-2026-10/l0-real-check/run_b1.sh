#!/usr/bin/env bash
# B1: in heads-up L0 is the input game, so l0_real's Monte Carlo values must agree with the exact L0 values.
# Run from the workspace root after building the examples (see README.md):
#   bash experiments/p2-method-2026-10/l0-real-check/run_b1.sh <out_dir>
# The legacy-solution cases run only when runs/p2-method-2026-10/l0-evaluator-check/b1_<s>bb exists.
set -euo pipefail
out=${1:?usage: run_b1.sh <out_dir>}
mkdir -p "$out"
check=experiments/p2-method-2026-10/l0-evaluator-check/hu_pushfold_check.py
eval_bin=target/release/examples/l0_eval
real_bin=target/release/examples/l0_real
tables=.cache/p2-trunk
legacy=runs/p2-method-2026-10/l0-evaluator-check
deals=4194304

target/release/examples/trunk_tables --dir "$tables" --export-t2 "$tables/t2.csv" --export-classes "$tables/classes.csv" > "$out/trunk_tables.log"

# run <name> <l0_real arguments...>: evaluate and print the wall time.
run() {
  local name=$1 start
  shift
  start=$(date +%s)
  "$real_bin" "$@" --deals "$deals" --output "$out/$name.json" > "$out/$name.log" 2>&1
  echo "$name: $(( $(date +%s) - start ))s"
}

for s in 5 10 20; do
  config=examples/bench/hu_pushfold_${s}bb.toml
  "$eval_bin" --config "$config" --export-tree "$out/b1_${s}bb_tree.json" > "$out/b1_${s}bb_tree.log"
  python "$check" profile --tree "$out/b1_${s}bb_tree.json" --classes "$tables/classes.csv" --kind random --seed 1 \
    --output "$out/b1_${s}bb_random.profile.json"
  run "b1_${s}bb_uniform" --config "$config" --profile uniform
  run "b1_${s}bb_random" --config "$config" --profile "json:$out/b1_${s}bb_random.profile.json"
  if [ -f "$legacy/b1_${s}bb/solution.mwsol" ]; then
    run "b1_${s}bb_legacy" --mwsol "$legacy/b1_${s}bb/solution.mwsol"
  fi
done

# The cases above share deal seed 0, so their errors are correlated. Repeat one case on independent deals.
deals=16777216
for seed in 0 1 2 3 4; do
  run "seeds_10bb_uniform_s$seed" --config examples/bench/hu_pushfold_10bb.toml --profile uniform --deal-seed "$seed"
done
