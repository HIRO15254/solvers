#!/usr/bin/env bash
# Run from the synced source tree on dev, using release binaries built with znver3.
set -euo pipefail
OLD=/work/zen3-SubtreeBaseline/release/solvers-old
NEW=/work/zen3-SubtreeProduct/release/solvers-new
BIT=experiments/p1-efficiency-2026-10/profiled-dual-eval-20261029/bitcheck.sh
CFG=experiments/p1-efficiency-2026-10/profiled-dual-eval-20261029/configs
FRAG=experiments/p1-efficiency-2026-10/subtree-freeze-20261029/configs/lock_flop.toml
OUT=/work/subtree-product-bitcheck
mkdir -p "$OUT"
for storage in f32 i16 i16-f32avg; do
  bash "$BIT" "$OLD" "$NEW" "$CFG/flop_3bp.toml" 20 "$storage" 16 | tee "$OUT/flop_3bp-$storage.txt"
done
bash "$BIT" "$OLD" "$NEW" "$CFG/flop_srp.toml" 20 f32 16 "$FRAG" | tee "$OUT/flop_srp-lock-flop-f32.txt"
