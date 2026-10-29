#!/usr/bin/env bash
# Run on the dedicated bench under /work/bench.lock, after shipping all inputs.
set -euo pipefail
D=${D:-/work/bin-SubtreeProduct}
OUT=${OUT:-/work/subtree-product-results}
mkdir -p "$OUT"
sha256sum "$D/solvers-old" "$D/solvers-new" "$D/p1_bench" > "$OUT/binaries.sha256"
/usr/bin/time -f 'elapsed=%e maxrss_kib=%M' -o "$OUT/base.time" \
  "$D/solvers-old" solve "$D/flop_srp.toml" --out "$D/base" --threads 32 > "$OUT/base.log" 2>&1
sha256sum "$D/base/solution.sol" > "$OUT/base.sha256"
cp "$D/base/run.json" "$OUT/base.json"
for name in lock_flop lock_turn lock_deep; do
  "$D/solvers-new" relock "$D/base" --add "$D/$name.toml" --freeze-upstream --print-config > "$OUT/frozen_$name.toml"
done
for round in 1 2; do
  if (( round == 1 )); then order=(old new); else order=(new old); fi
  for name in lock_flop lock_turn lock_deep; do
    for variant in "${order[@]}"; do
      label="$name-$round-$variant"
      /usr/bin/time -f 'elapsed=%e maxrss_kib=%M' -o "$OUT/$label.time" \
        "$D/solvers-$variant" solve "$OUT/frozen_$name.toml" --out "$D/$label" --threads 32 > "$OUT/$label.log" 2>&1
      cp "$D/$label/run.json" "$OUT/$label.json"
      cp "$D/$label/progress.jsonl" "$OUT/$label.progress.jsonl"
      "$D/p1_bench" "$D/$label/run.toml" --checkpoint "$D/$label/checkpoint.ckpt" \
        --threads 32 --warmup 0 --iters 0 --evals 1 --json "$OUT/$label.subtree.json" > "$OUT/$label.subtree.log" 2>&1
      rm -rf "$D/$label"
    done
  done
done
