#!/usr/bin/env bash
# Run after the main measurement, under the same exclusive bench lock.
# D contains solvers-old, solvers-new, flop_srp.toml, and lock_deep.toml.
# OUT must be a new absolute directory on a volume with room for solver outputs.
set -euo pipefail
D=${D:?Set D to the absolute input directory}
OUT=${OUT:?Set OUT to a new absolute output directory}
case "$D:$OUT" in /*:/*) ;; *) echo 'D and OUT must be absolute paths' >&2; exit 2;; esac
[ ! -e "$OUT" ] || { echo "Output already exists: $OUT" >&2; exit 2; }
mkdir -p "$OUT"
WORK=$(mktemp -d "$OUT/work.XXXXXXXX")
echo "Working files: $WORK" >&2
sha256sum "$D/solvers-old" "$D/solvers-new" "$D/flop_srp.toml" "$D/lock_deep.toml" > "$OUT/inputs.sha256"
/usr/bin/time -v -o "$OUT/base.time" "$D/solvers-old" solve "$D/flop_srp.toml" \
  --out "$WORK/base" --threads 32 > "$OUT/base.log" 2>&1
cp "$WORK/base/run.json" "$OUT/base.json"
sha256sum "$WORK/base/solution.sol" > "$OUT/base.sha256"
"$D/solvers-new" relock "$WORK/base" --add "$D/lock_deep.toml" --freeze-upstream \
  --print-config > "$OUT/frozen-deep.toml"
# relock emits normalized TOML with explicit solver and stop keys.
python3 - "$OUT" <<'PY'
import pathlib, re, sys
out = pathlib.Path(sys.argv[1])
original = (out / 'frozen-deep.toml').read_text()
def set_key(text, section, key, value):
    pattern = rf'(?ms)(^\[{re.escape(section)}\]\s*\n)(.*?)(?=^\[|\Z)'
    def change(match):
        body, count = re.subn(rf'(?m)^{re.escape(key)}\s*=.*$', f'{key} = {value}', match.group(2))
        if count != 1:
            raise ValueError(f'Expected exactly one {section}.{key}, found {count}')
        return match.group(1) + body
    result, count = re.subn(pattern, change, text)
    if count != 1:
        raise ValueError(f'Expected exactly one [{section}], found {count}')
    return result
for storage in ('f32', 'auto'):
    text = set_key(original, 'solver', 'storage', f'"{storage}"')
    text = set_key(text, 'solver.stop', 'target', '"0.000000000001bb"')
    text = set_key(text, 'solver.stop', 'max_iterations', '2')
    (out / f'{storage}.toml').write_text(text)
PY
for storage in f32 auto; do
  /usr/bin/time -v -o "$OUT/$storage.time" "$D/solvers-new" solve "$OUT/$storage.toml" \
    --out "$WORK/$storage" --threads 32 > "$OUT/$storage.log" 2>&1
  cp "$WORK/$storage/run.json" "$OUT/$storage.json"
  cp "$WORK/$storage/run.toml" "$OUT/$storage.run.toml"
  cp "$WORK/$storage/progress.jsonl" "$OUT/$storage.progress.jsonl"
  "$D/solvers-new" validate "$WORK/$storage/run.toml" --resources --format json \
    > "$OUT/$storage.resources.json" 2> "$OUT/$storage.resources.log"
  stat -c '%n %s' "$WORK/$storage/solution.sol" "$WORK/$storage/checkpoint.ckpt" > "$OUT/$storage.sizes"
  rm -rf -- "$WORK/$storage"
done
python3 - "$OUT" <<'PY'
import json, pathlib, sys
out = pathlib.Path(sys.argv[1])
rows = []
for storage in ('f32', 'auto'):
    run = json.loads((out / f'{storage}.json').read_text())
    resources = json.loads((out / f'{storage}.resources.json').read_text())['resources']
    profile = run.get('profile') or {}
    rows.append({'requested_storage': storage, 'effective_storage': resources['storage'],
                 'solve_scope': profile.get('solve_scope'), 'fallback_reason': profile.get('fallback_reason'),
                 'iterations': run['iterations'], 'nash_conv_bb': run['nashConv'],
                 'solve_wall_s': run['wallSecs'], 'resources': resources})
(out / 'summary.json').write_text(json.dumps({'threads': 32, 'runs': rows}, indent=2) + '\n')
if any(row['fallback_reason'] != 'outside-residual' for row in rows):
    raise SystemExit('outside-residual fallback was not observed in both runs; inspect retained work and logs')
PY
rm -rf -- "$WORK"
