#!/bin/bash
# One exclusive 64 GiB VM; caller supplies a finite cgroup and no other builds.
set -euxo pipefail
cd /opt/r1/candidate-v3
test -f /opt/r1/comparison-build/complete
python3 experiments/hu-postflop-r1/pipeline/run_campaign.py pilot \
    --baseline /opt/r1/target/baseline/release/solvers \
    --baseline-source /opt/r1/baseline-source.tar.gz \
    --out runs/r1-pilot
python3 experiments/hu-postflop-r1/pipeline/run_campaign.py freeze \
    --pilot runs/r1-pilot/pilot.json \
    --candidate /opt/r1/target/candidate-v3/release/solvers \
    --candidate-source /opt/r1/candidate-v3-source.tar.gz \
    --candidate-sol-version 3 \
    --out experiments/hu-postflop-r1/pipeline/frozen/vm06-v3
python3 experiments/hu-postflop-r1/pipeline/run_campaign.py run \
    --plan experiments/hu-postflop-r1/pipeline/frozen/vm06-v3/plan.json \
    --out runs/r1-paired-v3
date -u --iso-8601=seconds > /opt/r1/comparison-complete
