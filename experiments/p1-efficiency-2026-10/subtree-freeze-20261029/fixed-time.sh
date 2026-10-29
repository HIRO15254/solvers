#!/usr/bin/env bash
# Bench timing only: fixed iteration count obtained by convergence.sh on lab.
# 0 BB forces a subtree evaluation every ten steps without reaching the target
# in these cases; iterSecs therefore includes the same checks as the stop run.
set -euo pipefail
D=${D:-/work/bin-SubtreeFreeze}
name=${1:?profile fragment name}
threads=${2:?thread count}
iters=${3:?fixed iteration count from lab}
out=${OUT:-/work/subtree-fixed-time}
mkdir -p "$out"
for pair in 1 2 3; do
  if (( pair % 2 )); then order=(whole subtree); else order=(subtree whole); fi
  for variant in "${order[@]}"; do
    flags=()
    if [[ $variant == subtree ]]; then flags+=(--subtree-only); fi
    "$D/p1_bench" "$D/frozen_$name.toml" --threads "$threads" --storage f32 --warmup 0 --iters "$iters" --evals 1 --subtree-target 0 "${flags[@]}" --json "$out/$name-$threads-$pair-$variant.json" >/dev/null
  done
done
