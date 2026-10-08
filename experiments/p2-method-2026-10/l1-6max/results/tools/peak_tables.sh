#!/usr/bin/env bash
# Peak commit and time of the EHS² table builds with binary i: the 32-bucket table rebuilt (compared with the cached
# one), the 128-bucket table built into the shared cache for run_buckets.sh, and loading each cached table.
set -uo pipefail
cd /c/Users/PC_User/orca/workspaces/solvers/hind
s=/c/Users/PC_User/AppData/Local/Temp/claude/C--Users-PC-User-orca-workspaces-solvers-hind/38698140-7a89-40cb-9cf0-0677fcd937cb/scratchpad
ws=$(cygpath -w "$s")
o=.cache/p2-trunk/l1-6max/table-peak
bin=.cache/p2-trunk/l1-core/bin/trunk_solve-i.exe
cache="$LOCALAPPDATA/solvers/ehs2"
mkdir -p $o
sed -E "/^\[solver\.abstraction\.buckets\]/,/^\[/ s/^(flop|turn|river) = 32$/\1 = 128/" \
  examples/bench/hu_20bb_postflop.toml > $o/b6-b128.toml
[ "$(grep -cE "^(flop|turn|river) = 128$" $o/b6-b128.toml)" = 3 ] || { echo "bucket replacement failed"; exit 1; }
measure() { # name config table
  NEED=${NEED:-2.0} bash "$s/when_free.sh" $o/$1.peak pwsh -NoProfile -File "$ws\\peak.ps1" -Exe $bin -Log $o/$1.log \
    --config $2 --leaf-model l1 --ehs2-cache "$3" --iterations 1 --eval-every 0
  echo "$1 $(grep peak_commit $o/$1.peak)"
}
rm -f $o/v2-f32-t32-r32.postcard
measure build32 examples/bench/hu_20bb_postflop.toml $o/v2-f32-t32-r32.postcard
cmp $o/v2-f32-t32-r32.postcard "$cache/v2-f32-t32-r32.postcard" && echo "32 table identical"
measure load32 examples/bench/hu_20bb_postflop.toml "$cache/v2-f32-t32-r32.postcard"
[ -f "$cache/v2-f128-t128-r128.postcard" ] && echo "128 table already present"
measure build128 $o/b6-b128.toml "$cache/v2-f128-t128-r128.postcard"
ls -l "$cache/v2-f128-t128-r128.postcard"
measure load128 $o/b6-b128.toml "$cache/v2-f128-t128-r128.postcard"
rm -f $o/v2-f32-t32-r32.postcard
echo "tables done $(date -Iseconds)"
