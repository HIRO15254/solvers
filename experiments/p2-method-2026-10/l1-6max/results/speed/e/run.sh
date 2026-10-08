#!/usr/bin/env bash
# Paired timing of E (leaf_values' rows in parallel) against D, in the order D,E,E,D on B7 and then B4 Simple, with the
# solver's K4 at 256 samples, at least 16, and no evaluation.
set -uo pipefail
out=.cache/p2-trunk/l1-6max/timing/e
mkdir -p $out
ehs="$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"
for run in b7:examples/bench/6max_20bb.toml:12 b4s:examples/bench/6max_100bb_nl50_partial_simple_reference.toml:6; do
  IFS=: read -r name config iterations <<< "$run"
  for pair in 1:d 2:e 3:e 4:d; do
    i=${pair%%:*}; bin=${pair#*:}
    .cache/p2-trunk/l1-core/bin/trunk_solve-$bin.exe --config $config --leaf-model l1 --ehs2-cache "$ehs" \
      --solver-k4-samples 256 --solver-k4-min-samples 16 --iterations $iterations --eval-every 0 --print-every 1 \
      --output $out/$name-$i-$bin.json > $out/$name-$i-$bin.log 2>&1
    echo "$name $i $bin exit $?"
  done
done
