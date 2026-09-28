#!/bin/bash
set -euo pipefail
# Recovery only: prevent both experimental units from starting on this boot.
systemctl mask --now solvers-r1-vm19-build2.service solvers-r1-vm19-measure32.service
date -u --iso-8601=seconds > /opt/r1/recovery-only-boot-complete
