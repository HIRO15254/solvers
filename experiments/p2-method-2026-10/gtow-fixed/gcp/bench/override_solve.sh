#!/usr/bin/env bash
# Full E2 solve (e2_flags.sh) with the L1 leaf values of a train-override file fixed during training
# (trunk_solve --l1-train-override), then trunk_eval.sh of the result without and with each evaluation override.
# Run from the repository root.
# usage: bash override_solve.sh <out dir> <name> <seed> <train override JSON> [evaluation override JSON...]
#   Writes <out dir>/<name>/<name>.json (checkpoints in the trained game), <name>.profile.json, <name>.log and the
#   trunk_eval.sh outputs. An existing <name>.json skips the solve (Spot restart); an interrupted solve resumes from
#   ${STATE_DIR:-~/state}/<name>.state. The Postflop goes to ${POSTFLOP_DIR:-~/postflop}, outside ~/out.
set -uo pipefail
out=${1:?out dir}; name=${2:?name}; seed=${3:?seed}; train=${4:?train override}; shift 4
here=$(dirname "$0")
source "$here/e2_flags.sh"
e2_flags "$seed"
dir=$out/$name
state_dir=${STATE_DIR:-$HOME/state}; postflop_dir=${POSTFLOP_DIR:-$HOME/postflop}
mkdir -p "$dir" "$state_dir" "$postflop_dir"
postflop=$postflop_dir/$name.postflop.bin
if [[ ! -f $dir/$name.json ]]; then
  target/release/examples/trunk_solve${EXE_SUFFIX-.exe} "${flags[@]}" --l1-train-override "$train" \
    --iterations 2000 --eval-every 1000 --print-every 100 \
    --state "$state_dir/$name.state" --state-every 300 \
    --output "$dir/$name.json.tmp" --output-profile "$dir/$name.profile.json" --output-postflop "$postflop" \
    >> "$dir/$name.log" 2>&1 || exit 1
  mv "$dir/$name.json.tmp" "$dir/$name.json"
fi
bash "$here/trunk_eval.sh" "$dir" "$name" "$dir/$name.profile.json" "$postflop" "$seed" "$@"
