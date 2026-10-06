#!/bin/bash
# usage: ab.sh LABEL CONFIG ITERS "THREADS..." ROUNDS [extra p1_bench args]
# Alternates base/new per thread count so both sides see the same machine state.
source ~/.cargo/env
mkdir -p ~/results
for r in $(seq 1 $5); do
  for t in $4; do
    for side in base new; do
      ~/$side/target/release/examples/p1_bench ~/cfg/$2 --threads $t --warmup 1 --iters $3 --evals 1 $6 \
        >> ~/results/$1_$side.jsonl 2>>~/results/$1_$side.err
    done
  done
done
echo "AB_DONE $1"
