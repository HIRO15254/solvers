#!/usr/bin/env bash
# The cloud runs of notes/30-abs-level.md §10 (finer hand buckets on line 13): train the 3bet band's line 13 on ONE
# flop under the fixed r16g49q profile (--fixed-flops with that flop and every leaf in one flop group, so the strategy
# is that flop's own), then diagnose that flop (2048 boards, 4 rivers). Environment of gcp/bench/job_bench.sh (cwd
# ~/src, EHS2_DIR, tree tables in ~/tables, uploads flat in ~).
# usage: [TRAIN_ARGS='...'] [THREADS=4] [ITERS=2000] [OUT_ROOT=~/out/fine] bash ~/run_fine_cloud.sh VARIANT 'FLOP' [card args...]
#   FLOP: three cards, e.g. '2c 4d Qh'. Card args come after the shared ones and replace them (e.g. --l1-relative
#   128x8:$EHS2_DIR/v2-f1024-t1024-r1024.postcard --l1-strength-buckets 1024). TRAIN_ARGS go to the training only.
#   Output: OUT_ROOT/3bet/<VARIANT>/<flop without spaces>/{flop.txt,diag13.txt,solve.json,solve.log}; a run whose
#   diag13.txt exists is skipped.
set -uo pipefail
variant=${1:?variant}; flop=${2:?flop}; shift 2
T=${EHS2_DIR:?EHS2_DIR}
threads=${THREADS:-4}
read -ra train <<< "${TRAIN_ARGS:-}"
cfg=experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml
line=fold/raise-to:2000/fold/fold/raise-to:11000/fold/call:9000
out=${OUT_ROOT:-$HOME/out/fine}/3bet/$variant/${flop// /}
[[ -f $out/diag13.txt ]] && { echo "$variant $flop done before"; exit 0; }
mkdir -p "$out"
echo "$flop" > "$out/flop.txt"
cards=(--ehs2-cache "$T/v2-f32-t32-r32.postcard" --l1-relative "16x4:$T/v2-f128-t128-r128.postcard" --l1-relative-prior 10
       --l1-flop-groups none --l1-hand draws,pairs --l1-relative-opponent-levels flop,turn,river "$@")
start=$(date +%s)
target/release/examples/trunk_solve --config "$cfg" --leaf-model l1 "${cards[@]}" "${train[@]}" --threads "$threads" \
  --l1-seed 1 --seed 1 --tables-dir "$HOME/tables" --fixed-profile "$HOME/r16g49q.profile.json" --fixed-lines "$line" \
  --fixed-flops "$out/flop.txt" --iterations "${ITERS:-2000}" --output-postflop "$out/postflop.bin" \
  --output "$out/solve.json" > "$out/solve.log" 2>&1 || { echo "train $variant $flop failed"; exit 1; }
mid=$(date +%s)
RAYON_NUM_THREADS=$threads target/release/examples/l1_leaf_diag --config "$cfg" "${cards[@]}" --tables-dir "$HOME/tables" \
  --profile "$HOME/r16g49q.profile.json" --postflop "$out/postflop.bin" --line "$line" --boards 2048 \
  --flops "$out/flop.txt" --rivers 4 > "$out/diag13.txt.tmp" 2>&1 || { echo "diag $variant $flop failed"; exit 1; }
mv "$out/diag13.txt.tmp" "$out/diag13.txt"
rm -f "$out/postflop.bin"
echo "$variant $flop train $((mid - start))s diag $(( $(date +%s) - mid ))s $(grep -h 'L1:' "$out/solve.log" | head -1)"
