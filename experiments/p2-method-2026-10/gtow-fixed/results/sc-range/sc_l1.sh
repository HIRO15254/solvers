#!/usr/bin/env bash
# The L1 side of the suited-connector range experiment (README.md), on the cloud bench VM (gcp/bench/job_bench.sh
# environment: cwd ~/src, EHS2_DIR, X, tables in ~/tables, uploads flat in ~). Retrains L1 on the two SRP lines
# under one fixed Preflop profile, then diagnoses the 16 SRP flops (2048 boards, 4 rivers per turn).
# usage: bash ~/sc_l1.sh VARIANT CONDITION
#   VARIANT own     the recommended cards: own flop groups per leaf, built from the base profile in both conditions
#           flop1   one group per diagnosed flop (groups16.txt, the other flops in one group), trained on those flops
#           flop1ro flop1 with the turn/river split flush,pair,straight,over
#   CONDITION base | nosc (~/<CONDITION>.profile.json)
#   Output: ~/out/sc/l1/<VARIANT>-<CONDITION>/{diag1.txt (L04), diag2.txt (L02), solve.json, solve.log}
#   ITERS, TABLES, DIAG_FLOPS and BASE override 2000, ~/tables, ~/flops16.txt and ~/base.profile.json (local smoke runs).
#   DUMP=informed adds the column ev_informed to the diagnosis; KEEP=1 keeps postflop.bin; TAG appends to the output
#   directory name (<VARIANT>-<CONDITION><TAG>).
set -uo pipefail
variant=${1:?variant}; condition=${2:?condition}
T=${EHS2_DIR:?EHS2_DIR}
cfg=experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml
profile=$HOME/$condition.profile.json
lines="fold/fold/fold/raise-to:2500/fold/call:1500;fold/fold/fold/fold/raise-to:3000/call:2000"
cards=(--ehs2-cache "$T/v2-f32-t32-r32.postcard" --l1-relative "16x2:$T/v2-f128-t128-r128.postcard"
       --l1-relative-prior 10 --l1-hand draws,pairs)
train=()
case $variant in
  own) cards+=(--l1-flop-groups "$X/subsets/k49-groups.txt" --l1-leaf-flop-groups "own:${BASE:-$HOME/base.profile.json}") ;;
  flop1) cards+=(--l1-flop-groups "$HOME/groups16.txt"); train=(--fixed-flops "$HOME/flops16.txt") ;;
  flop1ro) cards+=(--l1-flop-groups "$HOME/groups16.txt" --l1-runout flush,pair,straight,over)
           train=(--fixed-flops "$HOME/flops16.txt") ;;
  *) echo "unknown variant $variant"; exit 2 ;;
esac
out=$HOME/out/sc/l1/$variant-$condition${TAG:-}
mkdir -p "$out"
start=$(date +%s)
target/release/examples/trunk_solve --config "$cfg" --leaf-model l1 "${cards[@]}" "${train[@]}" --l1-seed 1 --seed 1 \
  --tables-dir "${TABLES:-$HOME/tables}" --fixed-profile "$profile" --fixed-lines "$lines" --iterations "${ITERS:-2000}" \
  --output-postflop "$out/postflop.bin" --output "$out/solve.json" > "$out/solve.log" 2>&1 || { echo "train failed"; exit 1; }
mid=$(date +%s)
i=0
IFS=';' read -ra ls <<< "$lines"
for line in "${ls[@]}"; do
  i=$((i + 1))
  target/release/examples/l1_leaf_diag --config "$cfg" "${cards[@]}" --tables-dir "${TABLES:-$HOME/tables}" --profile "$profile" \
    --postflop "$out/postflop.bin" --line "$line" --boards 2048 --flops "${DIAG_FLOPS:-$HOME/flops16.txt}" --rivers 4 \
    ${DUMP:+--dump "$DUMP"} \
    > "$out/diag$i.txt.tmp" 2>&1 || { echo "diag $i failed"; exit 1; }
  mv "$out/diag$i.txt.tmp" "$out/diag$i.txt"
done
[[ -n ${KEEP:-} ]] || rm -f "$out/postflop.bin"
echo "$variant $condition train $((mid - start))s diag $(( $(date +%s) - mid ))s $(grep -h 'L1:' "$out/solve.log" | head -1)"
