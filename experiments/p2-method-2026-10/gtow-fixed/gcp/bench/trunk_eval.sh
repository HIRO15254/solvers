#!/usr/bin/env bash
# Evaluate a full-solve profile and Postflop without and with L1 leaf value overrides (trunk_solve --l1-eval-override).
# Run from the repository root.
# usage: bash trunk_eval.sh <out dir> <name> <profile> <postflop> <seed> [override JSON...]
#   Writes <out dir>/<name>-none.eval.json and <out dir>/<override file name without .json>.eval.json; existing
#   outputs are skipped. The flags are the E2 full solve's (e2_flags.sh); seed != 0 adds --l1-seed/--seed.
#   EHS2_DIR, TABLES_DIR and EXE_SUFFIX default as in buckets.sh.
set -uo pipefail
out=${1:?out dir}; name=${2:?name}; profile=${3:?profile}; postflop=${4:?postflop}; seed=${5:?seed}; shift 5
source "$(dirname "$0")/e2_flags.sh"
e2_flags "$seed"
mkdir -p "$out"
evaluate() {
  local output=$1; shift
  [[ -f $output ]] && { echo "skip $output"; return 0; }
  target/release/examples/trunk_solve${EXE_SUFFIX-.exe} "${flags[@]}" --evaluate-profile "$profile" \
    --evaluate-postflop "$postflop" "$@" --output "$output.tmp" > "${output%.json}.log" 2>&1 || exit 1
  mv "$output.tmp" "$output"
}
evaluate "$out/$name-none.eval.json"
for file in "$@"; do
  base=$(basename "$file" .json)
  evaluate "$out/$base.eval.json" --l1-eval-override "$file"
done
