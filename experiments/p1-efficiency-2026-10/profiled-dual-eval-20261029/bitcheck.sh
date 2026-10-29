#!/bin/bash
# Bit-identity check of two `solvers` binaries on one P1 config (run on the dev box).
# Usage: bitcheck.sh OLD_SOLVERS NEW_SOLVERS CONFIG ITERS [STORAGE=f32] [THREADS=16] [PROFILE_FRAGMENT]
# Solves CONFIG for exactly ITERS iterations with each binary (one evaluation at the end), then compares
# progress.jsonl (timing removed), `export strategy` / `export ev` of both .sol files (both read by NEW),
# and run.json (timing removed). Prints IDENTICAL or the first differences. Exit 0 only when identical.
set -u
OLD=$1; NEW=$2; CFG=$3; IT=$4; ST=${5:-f32}; TH=${6:-16}; FRAG=${7:-}
D=$(mktemp -d /work/bitcheck.XXXXXX)
C=$D/c.toml
sed -e '/^max_iterations/d' -e '/^check_every/d' -e '/^target = /d' -e '/^final_checkpoint/d' "$CFG" > $C
if grep -q '^storage = ' $C; then sed -i "s/^storage = .*/storage = \"$ST\"/" $C
elif grep -q '^\[solver\]' $C; then sed -i "s/^\[solver\]/[solver]\nstorage = \"$ST\"/" $C
else printf '\n[solver]\nstorage = "%s"\n' "$ST" >> $C; fi
if grep -q '^\[solver.stop\]' $C; then
  sed -i "s/^\[solver.stop\]/[solver.stop]\ntarget = \"0.0001%pot\"\nmax_iterations = $IT\ncheck_every = $IT/" $C
else printf '\n[solver.stop]\ntarget = "0.0001%%pot"\nmax_iterations = %s\ncheck_every = %s\n' "$IT" "$IT" >> $C; fi
if [ -n "$FRAG" ]; then cat "$FRAG" >> $C; fi
rc=0
for v in old new; do
  BIN=$OLD; [ $v = new ] && BIN=$NEW
  $BIN solve $C --out $D/$v --threads $TH > $D/$v.log 2>&1 || { echo "$v solve failed:"; tail -5 $D/$v.log; exit 2; }
done
strip() { jq -cS 'del(.elapsed_secs, .elapsedSecs, .elapsed, .timestamp, .started_at, .finished_at, .wall_secs) | walk(if type == "object" then del(.elapsedSecs, .elapsed_secs, .wallSecs, .startedAt, .finishedAt, .createdAt, .updatedAt, .durationSecs, .secs) else . end)' "$1" 2>/dev/null; }
if ! diff <(strip $D/old/progress.jsonl) <(strip $D/new/progress.jsonl) > $D/progress.diff; then
  echo "progress.jsonl differs:"; head -c 1500 $D/progress.diff; echo; rc=1
fi
for view in strategy ev; do
  $NEW export $D/old/solution.sol $view > $D/old.$view 2>&1
  $NEW export $D/new/solution.sol $view > $D/new.$view 2>&1
  if ! cmp -s $D/old.$view $D/new.$view; then
    echo "export $view differs: $(cmp $D/old.$view $D/new.$view | head -1)"; rc=1
  fi
done
if ! diff <(strip $D/old/run.json) <(strip $D/new/run.json) > $D/run.diff; then
  echo "run.json differs (timing fields removed):"; head -c 1500 $D/run.diff; echo
fi
echo "last progress row (old): $(tail -1 $D/old/progress.jsonl | head -c 300)"
if [ $rc = 0 ]; then echo "IDENTICAL ($(wc -c < $D/new.strategy) bytes strategy export)"; rm -rf $D; else echo "DIFFERENT (kept $D)"; fi
exit $rc
