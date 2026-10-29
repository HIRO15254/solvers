#!/usr/bin/env bash
# Run under an exclusive benchmark lock, after shipping binaries and PF12 inputs.
# Usage: D=/work/bin-subtree OUT=/work/subtree-storage-results bash measure.sh
# D contains solvers-old, solvers-new, flop_srp.toml and lock_{flop,turn,deep}.toml.
set -euo pipefail
D=${D:?Set D to the absolute directory containing binaries and PF12 configs}
OUT=${OUT:?Set OUT to an absolute, new results directory}
case "$D:$OUT" in /*:/*) ;; *) echo 'D and OUT must be absolute paths' >&2; exit 2;; esac
SCRIPT_DIR=$(cd -- "$(dirname -- "$0")" && pwd)
[ ! -e "$OUT" ] || { echo "Output already exists: $OUT" >&2; exit 2; }
mkdir -p "$OUT"
WORK=$(mktemp -d "$OUT/work.XXXXXXXX")
echo "Working files: $WORK" >&2
# Keep WORK on failure for diagnosis; remove it only after a successful run.
sha256sum "$D/solvers-old" "$D/solvers-new" "$D/flop_srp.toml" \
  "$D/lock_flop.toml" "$D/lock_turn.toml" "$D/lock_deep.toml" \
  "$SCRIPT_DIR/frequency.toml" > "$OUT/inputs.sha256"
cp "$D/flop_srp.toml" "$D/lock_flop.toml" "$D/lock_turn.toml" "$D/lock_deep.toml" "$SCRIPT_DIR/frequency.toml" "$OUT/"
/usr/bin/time -v -o "$OUT/base.time" "$D/solvers-old" solve "$D/flop_srp.toml" \
  --out "$WORK/base" --threads 32 > "$OUT/base.log" 2>&1
sha256sum "$WORK/base/solution.sol" > "$OUT/base.sha256"
cp "$WORK/base/run.json" "$OUT/base.json"
for name in lock_flop lock_turn lock_deep frequency; do
  "$D/solvers-new" relock "$WORK/base" --add "$OUT/$name.toml" --freeze-upstream \
    --print-config > "$OUT/frozen_$name.toml"
done
# Frequency control is measured at a fixed iteration budget rather than
# waiting for a general-sum target that may not be attainable.
python3 - "$OUT/frozen_frequency.toml" <<'PY'
import pathlib, re, sys
p = pathlib.Path(sys.argv[1])
text = p.read_text()
section = re.search(r'(?ms)^\[solver\.stop\]\s*\n.*?(?=^\[|\Z)', text)
if section is None:
    raise SystemExit("missing solver.stop")
body = re.sub(r'(?m)^target\s*=.*\n', '', section.group())
body, count = re.subn(r'(?m)^max_iterations\s*=.*$', 'max_iterations = 50', body)
if count != 1:
    raise SystemExit("missing max_iterations")
p.write_text(text[:section.start()] + body + text[section.end():])
PY
python3 - "$OUT/frozen_lock_deep.toml" "$OUT/frozen_fallback.toml" <<'PY'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text()
section = re.search(r'(?ms)^\[solver\.stop\]\s*\n.*?(?=^\[|\Z)', text)
if section is None:
    raise SystemExit("missing solver.stop")
body, targets = re.subn(r'(?m)^target\s*=.*$', 'target = "0.000000000001bb"', section.group())
body, limits = re.subn(r'(?m)^max_iterations\s*=.*$', 'max_iterations = 2', body)
if targets != 1 or limits != 1:
    raise SystemExit("missing target or max_iterations")
pathlib.Path(sys.argv[2]).write_text(text[:section.start()] + body + text[section.end():])
PY
for round in 1 2; do
  if (( round == 1 )); then order=(old new); else order=(new old); fi
  for name in lock_flop lock_turn lock_deep frequency fallback; do
    for variant in "${order[@]}"; do
      label="$name-$round-$variant"
      echo "$label" >&2
      /usr/bin/time -v -o "$OUT/$label.time" "$D/solvers-$variant" solve \
        "$OUT/frozen_$name.toml" --out "$WORK/$label" --threads 32 > "$OUT/$label.log" 2>&1
      cp "$WORK/$label/run.json" "$OUT/$label.json"
      cp "$WORK/$label/run.toml" "$OUT/$label.run.toml"
      cp "$WORK/$label/progress.jsonl" "$OUT/$label.progress.jsonl"
      "$D/solvers-$variant" validate "$WORK/$label/run.toml" --resources --format json \
        > "$OUT/$label.resources.json" 2> "$OUT/$label.resources.log"
      stat -c '%n %s' "$WORK/$label/solution.sol" "$WORK/$label/checkpoint.ckpt" > "$OUT/$label.sizes"
      rm -rf -- "$WORK/$label"
    done
  done
done
python3 "$SCRIPT_DIR/summarize.py" "$OUT"
rm -rf -- "$WORK"
