#!/bin/bash
set -euo pipefail
test -f /opt/r1/bootstrap-complete
python3 -B /tmp/r1-kernel-old/setup.py --upload /tmp/r1-kernel-old --out /opt/r1/kernel-old
python3 -B /tmp/r1-kernel-old/setup.py --upload /tmp/r1-kernel-new --out /opt/r1/kernel-new
python3 -B /tmp/r1-kernel-old/validate.py --root /opt/r1/kernel-new --target /opt/r1/target/kernel-new --deadline-utc 2026-09-26T18:25:00Z
python3 -B /tmp/r1-kernel-old/validate.py --root /opt/r1/kernel-old --target /opt/r1/target/kernel-old --deadline-utc 2026-09-26T18:25:00Z --build-only
