#!/bin/bash
set -euxo pipefail
cd /opt/r1/candidate-v3
test -f /opt/r1/audit-pair-recovery/complete.json
test -f /opt/r1/comparison-complete
python3 /opt/r1/saved-profile-tools/run_audits.py freeze \
    --plan experiments/hu-postflop-r1/pipeline/frozen/vm06-v3/plan.json \
    --comparison runs/r1-paired-v3/comparison.json \
    --baseline-audit /opt/r1/target/baseline-audit-recovery/release/examples/hu_saved_profile_audit \
    --baseline-source /opt/r1/audit-pair-recovery/result.json \
    --candidate-audit /opt/r1/target/current-recovery/release/examples/hu_saved_profile_audit \
    --candidate-source /opt/r1/audit-pair-recovery/result.json \
    --b3sum /usr/bin/b3sum --out /opt/r1/saved-audit-plan
python3 /opt/r1/saved-profile-tools/run_audits.py run \
    --plan /opt/r1/saved-audit-plan/plan.json --out /opt/r1/saved-audits
date -u --iso-8601=seconds > /opt/r1/saved-audits-complete
