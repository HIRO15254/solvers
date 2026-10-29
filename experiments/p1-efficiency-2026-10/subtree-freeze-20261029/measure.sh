#!/usr/bin/env bash
# Run under /work/bench.lock; one invocation per profile, at most 15 min.
set -euo pipefail
D=${D:-/work/bin-SubtreeFreeze}
name=${1:?profile fragment name}
threads=${2:?thread count}
mode=timing
out=${OUT:-/work/subtree-results}
mkdir -p "$out"
for pair in 1 2 3; do
  if (( pair % 2 )); then order=(whole subtree); else order=(subtree whole); fi
  for variant in "${order[@]}"; do
    flags=()
    if [[ $variant == subtree ]]; then flags+=(--subtree-only); fi
    "$D/p1_bench" "$D/frozen_$name.toml" --threads "$threads" --storage f32 --warmup 2 --iters 20 --evals 3 "${flags[@]}" --json "$out/$name-$threads-$mode-$pair-$variant.json" >/dev/null
  done
done
