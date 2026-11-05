#!/usr/bin/env bash
# Archived copy of the scratch script runs/sc-x/run-r16b.sh used for notes/30-abs-level.md (copy it back to
# runs/sc-x/ to rerun; outputs go to the ignored runs/sc-x/l1/r16/). Retrains the L1 of one bench line under the fixed
# r16g49q it2000 Preflop with the base cards plus extra args, then writes the diagnosis on the band's flops.
# BIN selects the binaries (default target/release/examples), so a second build can run beside the first.
# usage: [BIN=DIR] [ITERS=2000] [DUMP_FLOPS=FILE] [KEEP=1] [TRAIN_ARGS=...] bash runs/sc-x/run-r16b.sh NAME BAND INDEX LINE [extra card args...]
set -uo pipefail
name=$1; band=$2; index=$3; line=$4; shift 4
bin=${BIN:-target/release/examples}
T=$LOCALAPPDATA/solvers/ehs2
here=experiments/p2-method-2026-10/gtow-fixed
bench=$here/gcp/bench
cfg=experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml
profile=.cache/p2-trunk/gtow-fixed/improve/im-d-r16g49q-sd0/snap-traise5+p10+g49+hdq+rel16x2/it002000.profile.json
cards=(--ehs2-cache "$T/v2-f32-t32-r32.postcard" --l1-relative "16x2:$T/v2-f128-t128-r128.postcard"
       --l1-relative-prior 10 --l1-flop-groups "$here/subsets/k49-groups.txt" --l1-hand draws,pairs "$@")
case $band in 3bet) flops=$bench/flops32.txt ;; 4bet|5bet) flops=$bench/flops64.txt ;; srp) flops=$bench/flops16.txt ;; esac
out=runs/sc-x/l1/r16/$band/$name
dir=$out/train$index
mkdir -p "$dir"
start=$(date +%s)
if [[ ! -f $dir/postflop.bin ]]; then
  read -ra train_args <<< "${TRAIN_ARGS:-}"
  "$bin/trunk_solve.exe" --config "$cfg" --leaf-model l1 "${cards[@]}" "${train_args[@]}" --l1-seed 1 --seed 1 \
    --tables-dir ../_goal/gtowtree/tables --fixed-profile "$profile" --fixed-lines "$line" --iterations "${ITERS:-2000}" \
    --output-postflop "$dir/postflop.bin.tmp" --output "$dir/solve.json" > "$dir/solve.log" 2>&1 || { echo "train failed"; exit 1; }
  mv "$dir/postflop.bin.tmp" "$dir/postflop.bin"
fi
mid=$(date +%s)
"$bin/l1_leaf_diag.exe" --config "$cfg" "${cards[@]}" --tables-dir ../_goal/gtowtree/tables \
  --profile "$profile" --postflop "$dir/postflop.bin" --line "$line" --boards 2048 --flops "$flops" --rivers 4 \
  ${INFORMED:+--dump informed} > "$out/diag$index.txt.tmp" 2>&1 || { echo "diag failed"; exit 1; }
mv "$out/diag$index.txt.tmp" "$out/diag$index.txt"
if [[ -n ${DUMP_FLOPS:-} ]]; then
  "$bin/l1_leaf_diag.exe" --config "$cfg" "${cards[@]}" --tables-dir ../_goal/gtowtree/tables \
    --profile "$profile" --postflop "$dir/postflop.bin" --line "$line" --boards 2048 --flops "$DUMP_FLOPS" \
    --dump strategy > "$out/dump$index.txt" 2>&1 || { echo "dump failed"; exit 1; }
fi
[[ -n ${KEEP:-} ]] || rm -f "$dir/postflop.bin"
echo "$name $band $index train $((mid - start))s diag $(( $(date +%s) - mid ))s $(grep -h 'L1:' "$dir/solve.log" | head -1)"
