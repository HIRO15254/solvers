#!/usr/bin/env bash
# Paired timing after dropping the sparse showdown merge: 30df60b, memo2 (4cc17dc's pass), guide (the sparse merge,
# the memo and the K4 guide tables), in the order A,B,C,C,B,A, with the solver's K4 at 256 samples, at least 16.
set -uo pipefail
out=.cache/p2-trunk/l1-6max/timing/round2
for pair in 1:30df60b 2:memo2 3:guide 4:guide 5:memo2 6:30df60b; do
  i=${pair%%:*}; bin=${pair#*:}
  .cache/p2-trunk/l1-core/bin/trunk_solve-$bin.exe --config examples/bench/6max_100bb_nl50_partial_simple_reference.toml \
    --leaf-model l1 --ehs2-cache "$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard" --solver-k4-samples 256 \
    --solver-k4-min-samples 16 --iterations 5 --eval-every 0 --print-every 1 --output $out/b4s-$i-$bin.json \
    > $out/b4s-$i-$bin.log 2>&1
  echo "$i $bin exit $?"
done
