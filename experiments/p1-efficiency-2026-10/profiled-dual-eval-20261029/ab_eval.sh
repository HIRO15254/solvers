#!/bin/bash
# Old/new evaluation time and peak memory of profiled P1 runs (run on the Linux box).
# Usage: ab_eval.sh OLD_BIN_DIR NEW_BIN_DIR OUT_DIR
# Each BIN_DIR holds p1_bench. flop_srp.toml plus each profile fragment is run
# with both binaries in alternating order (old first on odd repetitions): two
# warm-up and four CFR iterations, then three evaluations (evaluateSecs) or
# none (the peak without evaluation).
set -eu
OLD=$1
NEW=$2
OUT=$3
CFG=$(cd "$(dirname "$0")/configs" && pwd)
mkdir -p "$OUT"
for profile in lock_flop lock_turn incentive_turn; do
  cat "$CFG/flop_srp.toml" "$CFG/$profile.toml" > "$OUT/$profile.toml"
done
for rep in 1 2 3 4 5; do
  order="old new"
  [ $((rep % 2)) = 1 ] || order="new old"
  for t in 16 32; do
    for profile in lock_flop lock_turn incentive_turn; do
      for v in $order; do
        bin=$OLD
        [ "$v" = new ] && bin=$NEW
        for evals in 3 0; do
          [ "$evals" = 3 ] || [ "$rep" -le 3 ] || continue
          "$bin/p1_bench" "$OUT/$profile.toml" --threads "$t" --warmup 2 --iters 4 --evals "$evals" \
            --json "$OUT/$profile-$v-t$t-e$evals-r$rep.json" > /dev/null
        done
      done
    done
  done
done
