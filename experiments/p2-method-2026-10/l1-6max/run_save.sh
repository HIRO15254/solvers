#!/usr/bin/env bash
# S4-2b follow-up: solve B6, B7 and B4 Simple in L1 again as the records did (the solve is deterministic), saving the
# class profile and the Postflop average for run_eval_saved.sh. Checkpoints only at 0 and 2000, with the records'
# evaluation boards, to compare with them.
# usage: bash run_save.sh <out dir> [names...]   (names: b6 b7 b4s; TRUNK_SOLVE overrides the binary, EHS2 the cache)
set -uo pipefail
out=${1:?out dir}
shift
names=${*:-b6 b7 b4s}
ehs=${EHS2:-"$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"}
solve=${TRUNK_SOLVE:-target/release/examples/trunk_solve.exe}
mkdir -p "$out"
k4=(--solver-k4-samples 256 --solver-k4-min-samples 16)
for name in $names; do
  case $name in
    b6) args=(--config examples/bench/hu_20bb_postflop.toml --l1-eval-boards 4096) ;;
    b7) args=(--config examples/bench/6max_20bb.toml "${k4[@]}") ;;
    b4s) args=(--config examples/bench/6max_100bb_nl50_partial_simple_reference.toml "${k4[@]}") ;;
    *) echo "unknown $name"; continue ;;
  esac
  echo "$name start $(date -Iseconds)"
  "$solve" "${args[@]}" --leaf-model l1 --ehs2-cache "$ehs" --iterations 2000 --eval-every 2000 --print-every 10 \
    --output "$out/$name.json" --output-profile "$out/$name.profile.json" --output-postflop "$out/$name.postflop.bin" \
    > "$out/$name.log" 2>&1
  echo "$name exit $? $(date -Iseconds)"
done
