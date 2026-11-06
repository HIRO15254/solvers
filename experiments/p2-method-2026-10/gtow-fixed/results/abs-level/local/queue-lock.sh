#!/usr/bin/env bash
# Archived copy of the scratch script runs/sc-x/queue-lock.sh used for notes/30-abs-level.md (copy both scripts back
# to runs/sc-x/ to rerun). Several runners may share one queue file: a runner claims a row by creating
# runs/sc-x/l1/r16/<band>/<name>/lock<index> (mkdir is atomic) and skips rows with a diag or a lock.
# Rows: NAME<TAB>BAND<TAB>INDEX<TAB>LINE<TAB>TRAIN_ARGS<TAB>EXTRA (queue.tsv here). BIN selects the binaries (run-r16b.sh).
set -uo pipefail
T=$LOCALAPPDATA/solvers/ehs2
while IFS=$'\t' read -r name band index line train extra; do
  [[ -n $name && $name != \#* ]] || continue
  out=runs/sc-x/l1/r16/$band/$name
  [[ -f $out/diag$index.txt ]] && continue
  mkdir -p "$out"
  mkdir "$out/lock$index" 2>/dev/null || continue
  [[ $train == - ]] && train=
  train=${train//\$T/$T}; extra=${extra//\$T/$T}
  read -ra more <<< "$extra"
  if ! TRAIN_ARGS="$train" bash runs/sc-x/run-r16b.sh "$name" "$band" "$index" "$line" "${more[@]}" < /dev/null; then
    echo "FAILED $name $band $index"
    rmdir "$out/lock$index"
  fi
done < "${1:?queue}"
