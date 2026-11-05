#!/usr/bin/env bash
# Diagnose a saved L1 Postflop per benchmark band (l1_leaf_diag --flops via buckets.sh). Run from the repository root.
# usage: bash experiments/p2-method-2026-10/gtow-fixed/bench_diag.sh <profile> <postflop> <out root> <variant> [bands file]
#   bands file (default experiments/p2-method-2026-10/gtow-fixed/gcp/bench/bands.txt): lines `<band> <lines file> <flops file>`,
#   paths relative to the bands file. Output: <out root>/<band>/<variant>/diag<i>.txt, i the line's position in the lines file.
#   A band is skipped when every diag file exists with K `flop ` and 2K `hero ` lines (K flops). DIAG_RIVERS,
#   DIAG_JOBS and REBIND (an average trained before the 2026-10-29 merge) pass through to buckets.sh.
set -uo pipefail
profile=${1:?profile}; postflop=${2:?postflop}; root=${3:?out root}; variant=${4:?variant}
here=experiments/p2-method-2026-10/gtow-fixed
bands=${5:-$here/gcp/bench/bands.txt}
dir=$(dirname "$bands")
while read -r band lines flops; do
  [[ -n $band ]] || continue
  lines=$dir/$lines; flops=$dir/$flops
  k=$(grep -c . "$flops")
  n=$(tr ';' '\n' < "$lines" | grep -c .)
  done_all=1
  for ((i = 1; i <= n; i++)); do
    f=$root/$band/$variant/diag$i.txt
    if [[ ! -f $f ]] || (( $(grep -c '^flop ' "$f") != k || $(grep -c '^hero ' "$f") != 2 * k )); then
      done_all=0; break
    fi
  done
  if (( done_all )); then echo "skip $band $variant"; continue; fi
  POSTFLOP=$postflop LINES=$lines DIAG_FLOPS=$flops bash "$here/buckets.sh" "$profile" "$root/$band" 2000 "$variant" || exit 1
done < "$bands"
