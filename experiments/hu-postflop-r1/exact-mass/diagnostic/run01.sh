#!/bin/bash
set -euo pipefail
python3 -B /opt/r1/exact-control/exact-mass/diagnostic/instrument.py --archive /opt/r1/exact-new01/source-candidate.tar.gz --manifest /opt/r1/exact-new01/source-candidate-manifest.json --out /opt/r1/exact-diagnostic01
python3 -B /opt/r1/exact-control/exact-mass/diagnostic/run_diagnostic.py --root /opt/r1/exact-diagnostic01 --target /opt/r1/target/exact-diagnostic01 --baseline-proof /opt/r1/exact-proof01 --deadline-utc 2026-09-26T19:35:00Z
python3 -B /opt/r1/exact-control/bundle.py --out /opt/r1/exact-diagnostic01/run
