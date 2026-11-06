#!/usr/bin/env bash
# The cloud runs of notes/27-leaf-own.md: run-own.sh on the cloud bench VM (gcp/bench/job_bench.sh environment: cwd
# ~/src, EHS2_DIR, X, tables in ~/tables, uploads flat in ~). One trunk_solve per plan row under the fixed r16g49q
# profile (~/r16g49q.profile.json), then the 2048-board, 4-river diagnosis of the band's flops.
# usage: [TRAIN_ARGS='...'] bash ~/run_own_cloud.sh VARIANT PLAN
#   PLAN rows: band<TAB>index<TAB>line<TAB>extra args (paths relative to ~/src, e.g. ../ll-own-L01.txt)
#   TRAIN_ARGS: arguments for the training only (e.g. --l1-boards 128; notes/30-abs-level.md), not for the diagnosis
#   Output: ~/out/l1/own/<band>/<VARIANT>/{diag<index>.txt,train<index>/{solve.json,solve.log}}
set -uo pipefail
variant=${1:?variant}; plan=${2:?plan}
T=${EHS2_DIR:?EHS2_DIR}
read -ra train <<< "${TRAIN_ARGS:-}"
cfg=experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml
cards=(--ehs2-cache "$T/v2-f32-t32-r32.postcard" --l1-relative "16x2:$T/v2-f128-t128-r128.postcard"
       --l1-relative-prior 10 --l1-flop-groups "$X/subsets/k49-groups.txt" --l1-hand draws,pairs)
while IFS=$'\t' read -r band index line extra; do
  [[ -n $band ]] || continue
  case $band in 3bet) flops=$HOME/flops32.txt ;; 4bet|5bet) flops=$HOME/flops64.txt ;; srp) flops=$HOME/flops16.txt ;; esac
  out=$HOME/out/l1/own/$band/$variant
  dir=$out/train$index
  mkdir -p "$dir"
  read -ra more <<< "$extra"
  start=$(date +%s)
  target/release/examples/trunk_solve --config "$cfg" --leaf-model l1 "${cards[@]}" "${more[@]}" "${train[@]}" --l1-seed 1 --seed 1 \
    --tables-dir "$HOME/tables" --fixed-profile "$HOME/r16g49q.profile.json" --fixed-lines "$line" --iterations 2000 \
    --output-postflop "$dir/postflop.bin" --output "$dir/solve.json" > "$dir/solve.log" 2>&1 || { echo "train $band $index failed"; exit 1; }
  mid=$(date +%s)
  f=$out/diag$index.txt
  target/release/examples/l1_leaf_diag --config "$cfg" "${cards[@]}" "${more[@]}" --tables-dir "$HOME/tables" \
    --profile "$HOME/r16g49q.profile.json" --postflop "$dir/postflop.bin" --line "$line" --boards 2048 --flops "$flops" \
    --rivers 4 > "$f.tmp" 2>&1 || { echo "diag $band $index failed"; exit 1; }
  mv "$f.tmp" "$f"
  rm -f "$dir/postflop.bin"
  echo "$variant $band $index train $((mid - start))s diag $(( $(date +%s) - mid ))s $(grep -h 'L1:' "$dir/solve.log" | head -1)"
done < "$plan"
