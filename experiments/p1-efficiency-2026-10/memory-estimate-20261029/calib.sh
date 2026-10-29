#!/bin/bash
# Thread-scratch calibration for the P1 memory estimate (run on the Linux box).
# Usage: calib.sh BIN_DIR OUT_DIR [GROUP...]
# BIN_DIR holds `solvers` and `p1_bench` built from the change. GROUP selects
# the runs below (default: all, in this order). Every p1_bench run is a fresh
# process: afterBuildBytes is VmRSS after the build, peakBytes is VmHWM after
# the iterations and two evaluations.
set -eu
BIN=$1
OUT=$2
shift 2
CFG=$(cd "$(dirname "$0")/configs" && pwd)
EXAMPLES=$(cd "$(dirname "$0")/../../../examples/hu-postflop" && pwd)
mkdir -p "$OUT"
cat "$CFG/flop_srp.toml" "$CFG/lock_flop.toml" > "$OUT/lockflop_srp.toml"
cat "$CFG/flop_srp.toml" "$CFG/lock_turn.toml" > "$OUT/lockturn_srp.toml"
bench() { # CONFIG STORAGE THREADS ITERS REP
  "$BIN/p1_bench" "$1" --threads "$3" --warmup 2 --iters "$4" --evals 2 --storage "$2" \
    --json "$OUT/$(basename "$1" .toml)-$2-t$3-r$5.json" > /dev/null
}
group() { # NAME REP
  case $1 in
    flop)
      for t in 1 4 8 16 32; do
        bench "$CFG/flop_srp.toml" f32 "$t" 10 "$2"
        bench "$CFG/flop_3bp.toml" f32 "$t" 10 "$2"
      done
      for t in 1 8 16 32; do bench "$CFG/flop_4bp.toml" f32 "$t" 10 "$2"; done
      for t in 8 16 32; do
        bench "$CFG/flop_srp.toml" i16 "$t" 10 "$2"
        bench "$CFG/c_gtowb.toml" i16 "$t" 6 "$2"
      done ;;
    late)
      for t in 1 8 16 32; do
        bench "$CFG/turn_srp.toml" f32 "$t" 10 "$2"
        bench "$CFG/river_srp.toml" f32 "$t" 10 "$2"
      done ;;
    wide)
      for t in 1 8 16 32; do
        bench "$CFG/turn_wide.toml" f32 "$t" 10 "$2"
        bench "$CFG/river_wide.toml" f32 "$t" 10 "$2"
      done ;;
    lock)
      for t in 16 32; do
        bench "$OUT/lockflop_srp.toml" f32 "$t" 10 "$2"
        bench "$OUT/lockturn_srp.toml" f32 "$t" 10 "$2"
      done ;;
    process)
      # Process image: a validation and a one-thread solve of the bundled river example.
      [ "$2" = 1 ] || return 0
      /usr/bin/time -f '%M' -o "$OUT/process-validate.kib" "$BIN/solvers" validate "$EXAMPLES/river_small.toml" > /dev/null
      rm -rf "$OUT/river_small"
      /usr/bin/time -f '%M' -o "$OUT/process-solve.kib" "$BIN/solvers" solve "$EXAMPLES/river_small.toml" \
        --out "$OUT/river_small" --threads 1 > /dev/null ;;
    *) echo "unknown group $1" >&2; exit 2 ;;
  esac
}
[ $# -gt 0 ] || set -- flop late wide lock process
for rep in 1 2; do
  for g in "$@"; do group "$g" "$rep"; done
done
