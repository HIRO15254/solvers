#!/usr/bin/env bash
# S4-1b-2: solve B3 for 200 DCFR iterations (checkpoints every 20) with the solver-only K4 approximation of P2D7
# (256 samples whose seed changes every iteration), for comparison with the exact solver's run in
# ../trunk-solver/results/b3-trial/. The checkpoints use the unchanged L0 model (K4 2048, seed 0).
# Run from the workspace root after building the examples:
#   bash experiments/p2-method-2026-10/trunk-speedup/run_b3_solver_k4.sh <out_dir>
set -euo pipefail
out=${1:?usage: run_b3_solver_k4.sh <out_dir>}
mkdir -p "$out"
target/release/examples/trunk_solve --config examples/bench/6max_20bb_checkdown.toml --iterations 200 --eval-every 20 \
  --print-every 1 --solver-k4-samples 256 --output "$out/b3_s256.json" --output-profile "$out/b3_s256.profile.json" \
  > "$out/b3_s256.log" 2>&1
