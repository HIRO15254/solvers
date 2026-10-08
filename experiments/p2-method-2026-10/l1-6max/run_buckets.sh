#!/usr/bin/env bash
# S4-2b follow-up: B4 Simple in L1 with finer EHS² buckets (the input's 32 per street replaced by N), solved like the
# record (run.sh's b4s-l1), saving the class profile and the Postflop average. The bucket table is built on first use
# into the EHS2 cache directory.
# usage: bash run_buckets.sh <out dir> [N]   (TRUNK_SOLVE overrides the binary)
set -uo pipefail
out=${1:?out dir}
n=${2:-128}
ehs="$LOCALAPPDATA/solvers/ehs2/v2-f$n-t$n-r$n.postcard"
solve=${TRUNK_SOLVE:-target/release/examples/trunk_solve.exe}
mkdir -p "$out"
config=$out/b4s-b$n.toml
# The buckets are the only change; the run fails below if the replacement did not apply.
sed -E "/^\[solver\.abstraction\.buckets\]/,/^\[/ s/^(flop|turn|river) = 32$/\1 = $n/" \
  examples/bench/6max_100bb_nl50_partial_simple_reference.toml > "$config"
[ "$(grep -cE "^(flop|turn|river) = $n$" "$config")" = 3 ] || { echo "bucket replacement failed"; exit 1; }
name=b4s-b$n
echo "$name start $(date -Iseconds)"
"$solve" --config "$config" --leaf-model l1 --ehs2-cache "$ehs" --solver-k4-samples 256 --solver-k4-min-samples 16 \
  --iterations 2000 --eval-every 500 --print-every 10 --output "$out/$name.json" \
  --output-profile "$out/$name.profile.json" --output-postflop "$out/$name.postflop.bin" > "$out/$name.log" 2>&1
echo "$name exit $? $(date -Iseconds)"
