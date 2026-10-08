#!/usr/bin/env bash
# Run a command once the commit headroom (commit_log.ps1) stays at or above $NEED GB (default 3) for 1 min; retry up
# to five times when it fails on memory (an allocation failure or rustc's 0xc0000409 abort).
# usage: bash when_free.sh <log> <command...>
set -uo pipefail
cd /c/Users/PC_User/orca/workspaces/solvers/hind
s=/c/Users/PC_User/AppData/Local/Temp/claude/C--Users-PC-User-orca-workspaces-solvers-hind/38698140-7a89-40cb-9cf0-0677fcd937cb/scratchpad
log=$1
shift
need=${NEED:-3}
for attempt in 1 2 3 4 5; do
  ok=0
  until [ $ok -ge 4 ]; do
    sleep 15
    if tail -1 "$s/commit.log" | awk -v need="$need" '{
        for (i = 1; i <= NF; i++) { split($i, kv, "="); v[kv[1]] = kv[2] }
        exit !(v["limit"] - v["committed"] >= need) }'; then
      ok=$((ok + 1))
    else
      ok=0
    fi
  done
  echo "attempt $attempt $(date -Iseconds)"
  "$@" > "$log" 2>&1
  status=$?
  if [ $status -eq 0 ] || ! grep -q "memory allocation of\|0xc0000409" "$log"; then
    echo "exit $status"
    exit $status
  fi
done
echo "gave up"
exit 1
