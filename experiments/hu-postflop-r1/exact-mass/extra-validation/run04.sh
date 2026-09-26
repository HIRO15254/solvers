#!/bin/bash
set -euo pipefail
python3 -B /opt/r1/exact-control/exact-mass/extra-validation/run.py --root /opt/r1/exact-new04 --target /opt/r1/target/exact-new04 --deadline-utc 2026-09-26T19:35:00Z
python3 -B /opt/r1/exact-control/bundle.py --out /opt/r1/exact-new04/extra-validation
