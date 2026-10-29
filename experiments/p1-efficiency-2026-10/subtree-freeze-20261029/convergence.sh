#!/usr/bin/env bash
# Iteration counts and stop values on the lab box; these are not timing results.
set -euo pipefail
D=${D:-/work/bin-SubtreeFreeze}
out=${OUT:-/work/subtree-convergence}
mkdir -p "$out"
for name in lock_flop lock_turn lock_deep; do
  for threads in 16 32; do
    for variant in whole subtree; do
      flags=()
      if [[ $variant == subtree ]]; then flags+=(--subtree-only); fi
      "$D/p1_bench" "$D/frozen_$name.toml" --threads "$threads" --storage f32 --warmup 0 --iters 1000 --evals 1 --subtree-target 0.011 "${flags[@]}" --json "$out/$name-$threads-$variant.json" >/dev/null
    done
  done
done
