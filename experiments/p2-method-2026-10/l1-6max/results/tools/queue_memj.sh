#!/usr/bin/env bash
# J's memory over time on B4 Simple, one run at a time: private bytes, peak commit and page faults of i and j (times are measured on GCP instead). The other follow-ups of queue_local.sh are on hold.
set -uo pipefail
cd /c/Users/PC_User/orca/workspaces/solvers/hind
s=/c/Users/PC_User/AppData/Local/Temp/claude/C--Users-PC-User-orca-workspaces-solvers-hind/38698140-7a89-40cb-9cf0-0677fcd937cb/scratchpad
ws=$(cygpath -w "$s")
c=.cache/p2-trunk/l1-6max
b4s=examples/bench/6max_100bb_nl50_partial_simple_reference.toml
ehs32="$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"
m=$c/memory-j
mkdir -p $m
for run in i:30 j:30 j:150; do
  x=${run%%:*}
  n=${run#*:}
  NEED=2.0 bash "$s/when_free.sh" $m/b4s-$x-$n.peak pwsh -NoProfile -File "$ws\trace.ps1" \
    -Exe .cache/p2-trunk/l1-core/bin/trunk_solve-$x.exe -Log $m/b4s-$x-$n.log --config $b4s --leaf-model l1 \
    --ehs2-cache "$ehs32" --solver-k4-samples 256 --solver-k4-min-samples 16 --iterations $n --eval-every 0 \
    --print-every 10 --output $m/b4s-$x-$n.json
  cat $m/b4s-$x-$n.peak
done
echo "memory-j done $(date -Iseconds)"
