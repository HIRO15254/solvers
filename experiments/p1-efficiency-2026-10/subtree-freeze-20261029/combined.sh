#!/usr/bin/env bash
# Evaluate subtree-only converged solutions with base strategies outside them.
set -euo pipefail
D=${D:-/work/bin-SubtreeFreeze}
out=${OUT:-/work/subtree-combined}
mkdir -p "$out"
for name in lock_flop lock_turn lock_deep; do
  "$D/p1_bench" "$D/frozen_$name.toml" --threads 16 --storage f32 --warmup 0 --iters 1000 --evals 1 --subtree-only --subtree-target 0.011 --combined-base --json "$out/$name.json" >/dev/null
done
