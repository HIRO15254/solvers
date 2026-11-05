#!/usr/bin/env bash
# Screen relative-bucket reach variants on B6 (HU 20bb with Postflop), L1 defaults, 8x2 range-relative buckets,
# 4096 stratified evaluation boards, like l1-relative/b6.sh.
# usage: bash b6.sh <out dir> <iterations> <eval every> VARIANT...
#   VARIANT: base or '+'-joined parts: pM = --l1-relative-prior M, avg = --l1-relative-reach average
# The binary is target/release/examples/trunk_solve.exe (TRUNK_SOLVE overrides), copied into <out dir>.
set -uo pipefail
out=${1:?out}; iters=${2:?iterations}; every=${3:?eval every}; shift 3
mkdir -p "$out"
cp "${TRUNK_SOLVE:-target/release/examples/trunk_solve.exe}" "$out/trunk_solve.exe"
ehs="$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"
strength="$LOCALAPPDATA/solvers/ehs2/v2-f128-t128-r128.postcard"
for v in "$@"; do
  args=()
  for part in ${v//+/ }; do
    case $part in
      base) ;;
      avg) args+=(--l1-relative-reach average) ;;
      p*) args+=(--l1-relative-prior "${part#p}") ;;
      *) echo "unknown part $part of $v"; exit 2 ;;
    esac
  done
  "$out/trunk_solve.exe" --config examples/bench/hu_20bb_postflop.toml --leaf-model l1 --ehs2-cache "$ehs" \
    --l1-relative "8x2:$strength" "${args[@]}" --iterations "$iters" --eval-every "$every" --l1-eval-boards 4096 \
    --l1-eval-sampling stratified --print-every 100 --output "$out/$v.json" --output-profile "$out/$v.profile.json" \
    > "$out/$v.log" 2>&1
  echo "$v exit $? $(date +%T)"
done
