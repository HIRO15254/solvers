#!/usr/bin/env bash
# Benchmark B1: the L0 evaluator against hu_pushfold_check.py on heads-up push/fold.
# Run from the workspace root after building the examples (see README.md):
#   bash experiments/p2-method-2026-10/l0-evaluator-check/run_b1.sh <out_dir>
# The legacy-solution cases run only when runs/p2-method-2026-10/l0-evaluator-check/b1_<s>bb exists.
set -euo pipefail
out=${1:?usage: run_b1.sh <out_dir>}
mkdir -p "$out"
check=experiments/p2-method-2026-10/l0-evaluator-check/hu_pushfold_check.py
eval_bin=target/release/examples/l0_eval
tables=.cache/p2-trunk
legacy=runs/p2-method-2026-10/l0-evaluator-check

target/release/examples/trunk_tables --dir "$tables" --export-t2 "$tables/t2.csv" --export-classes "$tables/classes.csv" > "$out/trunk_tables.log"

# compare_case <name> <stack>: evaluate $out/<name>.profile.json in Python and compare with $out/<name>.rust.json.
compare_case() {
  python "$check" evaluate --classes "$tables/classes.csv" --t2 "$tables/t2.csv" --profile "$out/$1.profile.json" \
    --stack "$2" --output "$out/$1.py.json" > "$out/$1.py.log"
  echo "== $1"
  python "$check" compare --python "$out/$1.py.json" --rust "$out/$1.rust.json" | tee "$out/$1.compare.txt" | tail -2
}

for s in 5 10 20; do
  config=examples/bench/hu_pushfold_${s}bb.toml
  "$eval_bin" --config "$config" --export-tree "$out/b1_${s}bb_tree.json" > "$out/b1_${s}bb_tree.log"
  for kind in uniform random top; do
    extra=()
    case $kind in
      random) extra=(--seed 1) ;;
      top) extra=(--push 0.6 --call 0.3) ;;
    esac
    name=b1_${s}bb_${kind}
    python "$check" profile --tree "$out/b1_${s}bb_tree.json" --classes "$tables/classes.csv" --kind "$kind" "${extra[@]}" \
      --output "$out/$name.profile.json"
    "$eval_bin" --config "$config" --profile "json:$out/$name.profile.json" --output "$out/$name.rust.json" > "$out/$name.rust.log"
    compare_case "$name" "$s"
  done
  if [ -f "$legacy/b1_${s}bb/solution.mwsol" ]; then
    name=b1_${s}bb_legacy
    python "$check" profile-from-strategy --tree "$out/b1_${s}bb_tree.json" --strategy "$legacy/b1_${s}bb_strategy.csv" \
      --output "$out/$name.profile.json" > "$out/$name.profile.log"
    "$eval_bin" --mwsol "$legacy/b1_${s}bb/solution.mwsol" --output "$out/$name.rust.json" > "$out/$name.rust.log"
    compare_case "$name" "$s"
  fi
done

name=b1_10bb_fp
python "$check" fictitious-play --tree "$out/b1_10bb_tree.json" --classes "$tables/classes.csv" --t2 "$tables/t2.csv" \
  --stack 10 --iterations 400 --output "$out/$name.profile.json" > "$out/$name.profile.log"
"$eval_bin" --config examples/bench/hu_pushfold_10bb.toml --profile "json:$out/$name.profile.json" --output "$out/$name.rust.json" \
  > "$out/$name.rust.log"
compare_case "$name" 10
