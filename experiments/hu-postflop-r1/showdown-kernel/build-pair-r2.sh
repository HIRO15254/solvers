#!/bin/bash
set -euo pipefail
test -f /opt/r1/bootstrap-complete
python3 -B /opt/r1/kernel-control/setup.py --upload /opt/r1/kernel-old --out /opt/r1/kernel-old-r2
python3 -B /opt/r1/kernel-control/setup.py --upload /opt/r1/kernel-new --out /opt/r1/kernel-new-r2
python3 -B /opt/r1/kernel-control/validate.py --root /opt/r1/kernel-new-r2 --target /opt/r1/target/kernel-new-r2 --deadline-utc 2026-09-26T18:25:00Z
python3 -B /opt/r1/kernel-control/validate.py --root /opt/r1/kernel-old-r2 --target /opt/r1/target/kernel-old-r2 --deadline-utc 2026-09-26T18:25:00Z --build-only
