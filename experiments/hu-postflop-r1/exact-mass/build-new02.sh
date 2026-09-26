#!/bin/bash
set -euo pipefail
test -f /opt/r1/bootstrap-complete
python3 -B /opt/r1/exact-control/setup.py --upload /tmp/r1-exact-new02 --out /opt/r1/exact-new02
python3 -B /opt/r1/exact-control/validate.py --root /opt/r1/exact-new02 --target /opt/r1/target/exact-new02 --deadline-utc 2026-09-26T19:35:00Z
