#!/usr/bin/env bash
set -euo pipefail
# Run under rc.py --bench. Rotate storage order to reduce drift.
mkdir -p /work/storage-floor-timing
for tree in "$1"; do
  for threads in "$2"; do
    for rep in 0 1 2; do
      case "$rep" in
        0) order='f32 i16-f32avg i16';;
        1) order='i16-f32avg i16 f32';;
        2) order='i16 f32 i16-f32avg';;
      esac
      for storage in $order; do
        /work/bin-StorageFloor/p1_bench "/work/cfg/$tree.toml" --threads "$threads" --warmup 2 --iters 5 --evals 3 --storage "$storage" --json "/work/storage-floor-timing/$tree-$threads-$rep-$storage.json"
      done
    done
  done
done
