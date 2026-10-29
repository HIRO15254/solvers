#!/usr/bin/env bash
# Run on dev with the prior PF12 and follow-up release binaries and retained base.
set -euo pipefail
OLD=/work/zen3-SubtreeProduct/release/solvers-new
NEW=/work/zen3-SubtreeFollow/release/solvers
D=/work/subtree-follow-bitcheck
BIT=experiments/p1-efficiency-2026-10/profiled-dual-eval-20261029/bitcheck.sh
CFG=experiments/p1-efficiency-2026-10/profiled-dual-eval-20261029/configs
FRAG=experiments/p1-efficiency-2026-10/subtree-freeze-20261029/configs
mkdir -p "$D"
for storage in f32 i16 i16-f32avg; do
  bash "$BIT" "$OLD" "$NEW" "$CFG/flop_3bp.toml" 20 "$storage" 16 | tee "$D/nonfreeze-$storage.txt"
done
bash "$BIT" "$OLD" "$NEW" "$CFG/flop_srp.toml" 20 f32 16 "$FRAG/lock_flop.toml" | tee "$D/nonfreeze-lock.txt"
"$OLD" relock /work/subtree-reference.sol --add "$FRAG/lock_deep.toml" --freeze-upstream --print-config > "$D/frozen.toml"
python3 - "$D/frozen.toml" "$D/fixed.toml" <<'PY'
import sys
from pathlib import Path
p = Path(sys.argv[1]).read_text()
lines = [line for line in p.splitlines() if not line.startswith(('target = ', 'max_iterations = ', 'check_every = '))]
index = lines.index('[solver.stop]') + 1
lines[index:index] = ['max_iterations = 20', 'check_every = 20']
with open(sys.argv[2], 'w', encoding='utf-8', newline='\n') as f:
    f.write('\n'.join(lines) + '\n')
PY
for version in old new; do
  BIN=$OLD; [[ $version == new ]] && BIN=$NEW
  "$BIN" solve "$D/fixed.toml" --out "$D/$version" --threads 16 > "$D/$version.log" 2>&1
done
jq -cS 'del(.elapsed_secs)' "$D/old/progress.jsonl" > "$D/old.progress"
jq -cS 'del(.elapsed_secs)' "$D/new/progress.jsonl" > "$D/new.progress"
cmp "$D/old.progress" "$D/new.progress"
for view in strategy ev; do
  "$NEW" export "$D/old/solution.sol" "$view" > "$D/old.$view"
  "$NEW" export "$D/new/solution.sol" "$view" > "$D/new.$view"
  cmp "$D/old.$view" "$D/new.$view"
done
BENCH=/work/zen3-SubtreeProduct/release/examples/p1_bench
for version in old new; do
  "$BENCH" "$D/$version/run.toml" --checkpoint "$D/$version/checkpoint.ckpt" \
    --threads 16 --warmup 0 --iters 0 --evals 1 --hash-subtree \
    --json "$D/$version.subtree.json" > /dev/null
  jq -r '.subtreeHash' "$D/$version.subtree.json" > "$D/$version.subtree-hash"
done
cmp "$D/old.subtree-hash" "$D/new.subtree-hash"
printf 'freeze subtree regret/strategy BLAKE3: ' | tee "$D/freeze-storage.txt"
cat "$D/new.subtree-hash" | tee -a "$D/freeze-storage.txt"
sha256sum "$OLD" "$NEW" /work/subtree-reference.sol > "$D/hashes.txt"
echo 'freeze lock_deep 20 iterations, progress/strategy/ev IDENTICAL' | tee "$D/freeze.txt"
