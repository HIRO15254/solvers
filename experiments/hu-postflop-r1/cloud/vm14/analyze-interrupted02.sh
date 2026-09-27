#!/bin/bash
set -euo pipefail
base=/opt/r1/flop-flat-cloud32-package/experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32
test -f "$base/analyze.py"
test -f "$base/partial.py"
printf '%s\n' '098375aa70c6e854882fba7b2e5b49fe01a146203c5b84cc793fd0222ebcac1e  /tmp/analyze.py' 'cf8b6c62cc66130673f2a7ace977001487cff348626c2072ea0e4a9ea2d9e475  /tmp/partial.py' | sha256sum -c -
install -m 644 /tmp/analyze.py "$base/analyze.py"
install -m 644 /tmp/partial.py "$base/partial.py"
set +e
python3 -B "$base/analyze.py" --proof /opt/r1/flop-flat-cloud32-proof01 --out /tmp/flop-flat-cloud32-full-analysis02.json
full_status=$?
python3 -B "$base/partial.py" --proof /opt/r1/flop-flat-cloud32-proof01 --recovery-manifest /tmp/flop-flat-cloud32-proof01.tar.gz.manifest.json --out /tmp/flop-flat-cloud32-partial-analysis02.json
partial_status=$?
set -e
printf 'full_exit=%s\npartial_exit=%s\n' "$full_status" "$partial_status"
test "$full_status" -eq 2
test "$partial_status" -eq 0 || test "$partial_status" -eq 2
chmod 644 /tmp/flop-flat-cloud32-*-analysis02.json
