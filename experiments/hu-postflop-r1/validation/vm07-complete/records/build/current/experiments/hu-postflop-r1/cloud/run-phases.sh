#!/bin/bash
set -euxo pipefail
cd /opt/r1/candidate-v3
test -f /opt/r1/build-phases-complete
test -f /opt/r1/comparison-complete
python3 /opt/r1/phase-tools/run_phases.py freeze \
    --original-plan experiments/hu-postflop-r1/pipeline/frozen/vm06-v3/plan.json \
    --baseline-binary /opt/r1/target/phase-baseline/release/solvers \
    --baseline-manifest /opt/r1/phase-baseline/r1-phase-source-manifest.json \
    --candidate-binary /opt/r1/target/phase-candidate/release/solvers \
    --candidate-manifest /opt/r1/phase-candidate/r1-phase-source-manifest.json \
    --out /opt/r1/phase-plan/plan.json
python3 /opt/r1/phase-tools/run_phases.py run \
    --plan /opt/r1/phase-plan/plan.json --mode on --out /opt/r1/phase-on
python3 /opt/r1/phase-tools/run_phases.py run \
    --plan /opt/r1/phase-plan/plan.json --mode off --out /opt/r1/phase-off
python3 /opt/r1/phase-tools/run_phases.py analyze \
    --plan /opt/r1/phase-plan/plan.json \
    --on /opt/r1/phase-on/comparison.json --off /opt/r1/phase-off/comparison.json \
    --original runs/r1-paired-v3/comparison.json --out /opt/r1/phase-calibration.json
date -u --iso-8601=seconds > /opt/r1/phase-campaign-complete
