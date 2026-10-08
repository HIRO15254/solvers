#!/usr/bin/env bash
set -uo pipefail
out=.cache/p2-trunk/k4-min
for m in 16 64; do
  .cache/p2-trunk/l1-core/bin/trunk_solve-30df60b.exe --config examples/bench/6max_20bb_checkdown.toml --iterations 200 \
    --eval-every 20 --print-every 1 --solver-k4-samples 256 --solver-k4-min-samples $m \
    --output "$out/b3_s256_m$m.json" --output-profile "$out/b3_s256_m$m.profile.json" > "$out/b3_s256_m$m.log" 2>&1
  echo "m$m exit $?"
done
