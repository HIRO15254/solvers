#!/usr/bin/env bash
# Screen L1 bucket variants on B6 (HU 20bb with Postflop), L1 defaults, 4096 stratified evaluation boards.
# usage: bash b6.sh <out dir> <iterations> <eval every> VARIANT...
#   VARIANT: none | refine=SPEC (--l1-refine SPEC) | relative=BUCKETS[xLEVELS] (--l1-relative ...:128-bucket table)
#            | relative=BUCKETS[xLEVELS]+SPEC (both)
# The binary is target/release/examples/trunk_solve.exe (TRUNK_SOLVE overrides), copied into <out dir>.
set -uo pipefail
out=${1:?out}; iters=${2:?iterations}; every=${3:?eval every}; shift 3
mkdir -p "$out"
cp "${TRUNK_SOLVE:-target/release/examples/trunk_solve.exe}" "$out/trunk_solve.exe"
ehs="$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"
strength="$LOCALAPPDATA/solvers/ehs2/v2-f128-t128-r128.postcard"
for v in "$@"; do
  args=()
  case $v in
    none) ;;
    refine=*) args=(--l1-refine "${v#refine=}") ;;
    relative=*+*) r=${v#relative=}; args=(--l1-relative "${r%%+*}:$strength" --l1-refine "${r#*+}") ;;
    relative=*) args=(--l1-relative "${v#relative=}:$strength") ;;
    *) echo "unknown variant $v"; exit 2 ;;
  esac
  n=${v//[=:,+]/_}
  "$out/trunk_solve.exe" --config examples/bench/hu_20bb_postflop.toml --leaf-model l1 --ehs2-cache "$ehs" \
    "${args[@]}" --iterations "$iters" --eval-every "$every" --l1-eval-boards 4096 --l1-eval-sampling stratified \
    --print-every 100 --output "$out/$n.json" --output-profile "$out/$n.profile.json" > "$out/$n.log" 2>&1
  echo "$n exit $? $(date +%T)"
done
