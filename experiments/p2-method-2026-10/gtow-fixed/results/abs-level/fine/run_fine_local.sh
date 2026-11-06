#!/usr/bin/env bash
# The local runs of notes/30-abs-level.md §10: train line 13 of the 3bet band on ONE flop under the fixed r16g49q profile
# with the cards of run_fine_cloud.sh (16x4 relative buckets, prior 10, one flop group, opponent levels on every street,
# `draws,pairs`, seed 1), then diagnose that flop (2048 boards, 4 rivers). Windows paths: the EHS2 tables in
# %LOCALAPPDATA%/solvers/ehs2 and the default tree tables.
# usage (cwd = repository root): [TRAIN_ARGS='...'] [ITERS=2000] [THREADS=4] bash run_fine_local.sh BIN OUT 'FLOP' [card args...]
#   BIN: a target/release/examples directory. Card args come after the shared ones and replace them (e.g.
#   --l1-relative 32x8:$LOCALAPPDATA/solvers/ehs2/v2-f128-t128-r128.postcard). TRAIN_ARGS go to the training only
#   (e.g. '--l1-boards 1', '--l1-relative-reach average'). Output OUT/{flop.txt,diag13.txt,solve.json,solve.log}.
#   Strategy-independent buckets (§11): --l1-relative-prior 1e12 --l1-relative-opponent-levels none.
set -euo pipefail
bin=$1; out=$2; flop=$3; shift 3
read -ra train <<< "${TRAIN_ARGS:-}"
T="$LOCALAPPDATA/solvers/ehs2"
cfg=experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml
profile=.cache/p2-trunk/gtow-fixed/improve/im-d-r16g49q-sd0/snap-traise5+p10+g49+hdq+rel16x2/it002000.profile.json
line=fold/raise-to:2000/fold/fold/raise-to:11000/fold/call:9000
mkdir -p "$out"
echo "$flop" > "$out/flop.txt"
cards=(--ehs2-cache "$T/v2-f32-t32-r32.postcard" --l1-relative "16x4:$T/v2-f128-t128-r128.postcard" --l1-relative-prior 10
       --l1-flop-groups none --l1-hand draws,pairs --l1-relative-opponent-levels flop,turn,river "$@")
start=$(date +%s)
"$bin/trunk_solve" --config "$cfg" --leaf-model l1 "${cards[@]}" "${train[@]}" --threads "${THREADS:-4}" --l1-seed 1 --seed 1 \
  --fixed-profile "$profile" --fixed-lines "$line" --fixed-flops "$out/flop.txt" --iterations "${ITERS:-2000}" \
  --output-postflop "$out/postflop.bin" --output "$out/solve.json" > "$out/solve.log" 2>&1
mid=$(date +%s)
RAYON_NUM_THREADS=${THREADS:-4} "$bin/l1_leaf_diag" --config "$cfg" "${cards[@]}" --profile "$profile" --postflop "$out/postflop.bin" \
  --line "$line" --boards 2048 --flops "$out/flop.txt" --rivers 4 > "$out/diag13.txt" 2>&1
rm -f "$out/postflop.bin"
echo "$out train $((mid - start))s diag $(( $(date +%s) - mid ))s"
