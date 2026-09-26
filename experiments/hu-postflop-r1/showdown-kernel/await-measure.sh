#!/bin/bash
set -euo pipefail
deadline=$(date -u -d '2026-09-26T18:25:00Z' +%s)
while test "$(systemctl show r1-kernel-build-r2 --value --property=ActiveState)" = active; do
    test "$(date -u +%s)" -lt "$deadline"
    sleep 5
done
/bin/bash /opt/r1/kernel-control/measure-pair.sh
python3 -B /opt/r1/kernel-control/export-proof.py --proof /opt/r1/kernel-proof --archive /opt/r1/kernel-proof.tar.gz
chmod a+r /opt/r1/kernel-proof.tar.gz /opt/r1/kernel-proof.tar.gz.json
