#!/bin/bash
set -euo pipefail
base=/opt/r1/flop-flat-cloud32-package/experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32
test ! -e "$base/analyze.py"
test ! -e "$base/partial.py"
printf '%s\n' '6293275ea6b04530f2d1674cb1d50c1522834b0e56d60952510866d3a2458862  /tmp/analyze.py' 'bb14c58cfa6b3653c99c5adcf4a91387705c2d47ba1a5652785f650557d595d3  /tmp/partial.py' | sha256sum -c -
install -m 644 /tmp/analyze.py "$base/analyze.py"
install -m 644 /tmp/partial.py "$base/partial.py"
set +e
python3 -B "$base/analyze.py" --proof /opt/r1/flop-flat-cloud32-proof01 --out /tmp/flop-flat-cloud32-full-analysis01.json
full_status=$?
python3 -B "$base/partial.py" --proof /opt/r1/flop-flat-cloud32-proof01 --recovery-manifest /tmp/flop-flat-cloud32-proof01.tar.gz.manifest.json --out /tmp/flop-flat-cloud32-partial-analysis01.json
partial_status=$?
set -e
printf 'full_exit=%s\npartial_exit=%s\n' "$full_status" "$partial_status"
test "$full_status" -eq 2
test "$partial_status" -eq 0 || test "$partial_status" -eq 2
chmod 644 /tmp/flop-flat-cloud32-*-analysis01.json
