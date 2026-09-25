#!/bin/bash
# Final product snapshot06 includes the public saved-profile auditor.
# Reuse the original same-boot baseline pilot without changing cases or caps.
set -euxo pipefail
cd /opt/r1/candidate-v3
test -f /opt/r1/audit-pair-recovery/complete.json
test -f /opt/r1/comparison-complete
python3 experiments/hu-postflop-r1/pipeline/run_campaign.py freeze \
    --pilot runs/r1-pilot/pilot.json \
    --candidate /opt/r1/target/current-recovery/release/solvers \
    --candidate-source /opt/r1/audit-source.tar.gz \
    --candidate-sol-version 3 \
    --out experiments/hu-postflop-r1/pipeline/frozen/vm06-current
python3 experiments/hu-postflop-r1/pipeline/run_campaign.py run \
    --plan experiments/hu-postflop-r1/pipeline/frozen/vm06-current/plan.json \
    --out runs/r1-paired-current
date -u --iso-8601=seconds > /opt/r1/current-comparison-complete
python3 /opt/r1/saved-profile-tools/run_audits.py freeze \
    --plan experiments/hu-postflop-r1/pipeline/frozen/vm06-current/plan.json \
    --comparison runs/r1-paired-current/comparison.json \
    --baseline-audit /opt/r1/target/baseline-audit-recovery/release/examples/hu_saved_profile_audit \
    --baseline-source /opt/r1/audit-pair-recovery/result.json \
    --candidate-audit /opt/r1/target/current-recovery/release/examples/hu_saved_profile_audit \
    --candidate-source /opt/r1/audit-pair-recovery/result.json \
    --b3sum /usr/bin/b3sum --out /opt/r1/current-audit-plan
python3 /opt/r1/saved-profile-tools/run_audits.py run \
    --plan /opt/r1/current-audit-plan/plan.json --out /opt/r1/current-audits
date -u --iso-8601=seconds > /opt/r1/current-audits-complete
