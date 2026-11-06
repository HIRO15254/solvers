#!/usr/bin/env bash
# The runs of notes/27-leaf-own.md (run from the repository root; output in the scratch runs/sc-x, the diagnoses and
# solve logs kept in .cache/p2-trunk/gtow-fixed/leaf-own/l1/own, retained.sha256): per-leaf
# L1 retraining under a fixed Preflop profile, one trunk_solve per line, so every
# leaf can have its own card arguments (leaves are independent once the Preflop is fixed, and the training boards of an
# iteration do not depend on the trained leaves). The base card arguments are those of rel16x2@raise5+g49+hdq+sd1
# (buckets.sh); a plan row's extra arguments come after them, so a later --l1-flop-groups replaces the k49 groups.
# usage: PROFILE=FILE [SEED=N] bash experiments/p2-method-2026-10/gtow-fixed/results/leaf-own/run-own.sh VARIANT PLAN
#   SEED: --l1-seed and --seed (default 1). plans.tsv holds the rows that ran, with a variant and a machine column.
#   PLAN rows: band<TAB>index<TAB>line<TAB>extra args (space separated, may be empty)
#   Output: runs/sc-x/l1/own/<band>/<VARIANT>/{diag<index>.txt,train<index>/{postflop.bin,solve.json,solve.log}}
#   (the bench_metric.py layout). Diagnosis: 2048 boards, 4 rivers, the band's flops of gcp/bench/bands.txt.
#   A row is skipped when its diag file has K `flop ` and 2K `hero ` lines.
set -uo pipefail
variant=${1:?variant}; plan=${2:?plan}
T=$LOCALAPPDATA/solvers/ehs2
here=experiments/p2-method-2026-10/gtow-fixed
bench=$here/gcp/bench
cfg=experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml
cards=(--ehs2-cache "$T/v2-f32-t32-r32.postcard" --l1-relative "16x2:$T/v2-f128-t128-r128.postcard"
       --l1-relative-prior 10 --l1-flop-groups "$here/subsets/k49-groups.txt" --l1-hand draws,pairs)
while IFS=$'\t' read -r band index line extra; do
  [[ -n $band ]] || continue
  case $band in 3bet) flops=$bench/flops32.txt ;; 4bet|5bet) flops=$bench/flops64.txt ;; srp) flops=$bench/flops16.txt ;; esac
  out=runs/sc-x/l1/own/$band/$variant
  dir=$out/train$index
  mkdir -p "$dir"
  k=$(grep -c . "$flops")
  f=$out/diag$index.txt
  if [[ -f $f ]] && (( $(grep -c '^flop ' "$f") == k && $(grep -c '^hero ' "$f") == 2 * k )); then
    echo "skip $band $index"; continue
  fi
  read -ra more <<< "$extra"
  start=$(date +%s)
  if [[ ! -f $dir/postflop.bin ]]; then
    target/release/examples/trunk_solve.exe --config "$cfg" --leaf-model l1 "${cards[@]}" "${more[@]}" \
      --l1-seed "${SEED:-1}" --seed "${SEED:-1}" \
      --tables-dir ../_goal/gtowtree/tables --fixed-profile "${PROFILE:?PROFILE}" --fixed-lines "$line" --iterations 2000 \
      --output-postflop "$dir/postflop.bin.tmp" --output "$dir/solve.json" > "$dir/solve.log" 2>&1 || { echo "train $band $index failed"; exit 1; }
    mv "$dir/postflop.bin.tmp" "$dir/postflop.bin"
  fi
  mid=$(date +%s)
  target/release/examples/l1_leaf_diag.exe --config "$cfg" "${cards[@]}" "${more[@]}" --tables-dir ../_goal/gtowtree/tables \
    --profile "$PROFILE" --postflop "$dir/postflop.bin" --line "$line" --boards 2048 --flops "$flops" --rivers 4 \
    > "$f.tmp" 2>&1 || { echo "diag $band $index failed"; exit 1; }
  mv "$f.tmp" "$f"
  # The average holds every leaf (1.75 GB at r16g49q, about 4x with the runout split); only the diagnosis is kept.
  rm -f "$dir/postflop.bin"
  echo "$variant $band $index train $((mid - start))s diag $(( $(date +%s) - mid ))s $(grep -h 'L1:' "$dir/solve.log" | head -1)"
done < "$plan"
