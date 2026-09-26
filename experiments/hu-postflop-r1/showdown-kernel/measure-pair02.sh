#!/bin/bash
set -euo pipefail
test "$(systemctl show r1-kernel-build-r2 --value --property=ActiveState)" = inactive
test "$(systemctl show r1-kernel-build-r2 --value --property=ExecMainStatus)" = 0
python3 -B /opt/r1/kernel-control/run.py --phase prepare --old-root /opt/r1/kernel-old-r2 --new-root /opt/r1/kernel-new-r2 --out /opt/r1/kernel-proof02 --deadline-utc 2026-09-26T18:25:00Z
python3 -B /opt/r1/kernel-control/run.py --phase measure --out /opt/r1/kernel-proof02
python3 -B /opt/r1/kernel-control/verify.py --out /opt/r1/kernel-proof02 --expect completed --report /opt/r1/kernel-proof02/verification.json
python3 -B /opt/r1/kernel-control/export-proof.py --proof /opt/r1/kernel-proof02 --archive /opt/r1/kernel-proof02.tar.gz
chmod a+r /opt/r1/kernel-proof02.tar.gz /opt/r1/kernel-proof02.tar.gz.json
