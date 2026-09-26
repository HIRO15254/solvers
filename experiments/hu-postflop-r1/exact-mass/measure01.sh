#!/bin/bash
set -euo pipefail
python3 -B /opt/r1/exact-control/exact-mass/run.py --phase prepare --old-root /opt/r1/exact-old01 --new-root /opt/r1/exact-new01 --out /opt/r1/exact-proof01 --deadline-utc 2026-09-26T19:35:00Z
python3 -B /opt/r1/exact-control/exact-mass/run.py --phase measure --out /opt/r1/exact-proof01
python3 -B /opt/r1/exact-control/exact-mass/verify.py --out /opt/r1/exact-proof01 --expect completed --report /opt/r1/exact-proof01/verification.json
