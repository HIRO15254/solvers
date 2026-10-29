#!/usr/bin/env bash
# On lab/dev, after setup.sh. Hashes include every regret and average-sum bit
# in each maximal subtree. subtreeValues are per-root EV and BR for both seats.
set -euo pipefail
D=${D:-/work/bin-SubtreeFreeze}
out=${OUT:-/work/subtree-correctness}
mkdir -p "$out"
for name in lock_flop lock_turn lock_deep; do
  for iters in 50 200; do
    for variant in whole subtree; do
      flags=()
      if [[ $variant == subtree ]]; then flags+=(--subtree-only); fi
      "$D/p1_bench" "$D/frozen_$name.toml" --threads 16 --storage f32 --warmup 0 --iters "$iters" --evals 0 --hash-subtree "${flags[@]}" --json "$out/$name-$iters-$variant.json" >/dev/null
      jq -e '(.subtreeHash | type) == "string" and (.subtreeValues | type) == "array" and (.subtreeValues | length) == .subtreeRoots and .subtreeRoots > 0' "$out/$name-$iters-$variant.json" >/dev/null
    done
    jq -S '{subtreeHash,subtreeValues,subtreeNashConv,subtreeStorageElements,subtreeRoots}' "$out/$name-$iters-whole.json" > "$out/$name-$iters-whole.compare"
    jq -S '{subtreeHash,subtreeValues,subtreeNashConv,subtreeStorageElements,subtreeRoots}' "$out/$name-$iters-subtree.json" > "$out/$name-$iters-subtree.compare"
    cmp "$out/$name-$iters-whole.compare" "$out/$name-$iters-subtree.compare"
    echo "$name $iters IDENTICAL"
  done
done
