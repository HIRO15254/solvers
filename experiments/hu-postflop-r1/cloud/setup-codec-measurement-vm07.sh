#!/bin/bash
set -euo pipefail
cd /home/PC_User
printf '%s\n' '54a844133d469f5f0e09bced2e3c031329221243a606487b1b5e4a4315552834  codec-tools07.tar.gz' | sha256sum --check
test ! -e /opt/r1/codec-tools07
test ! -e /opt/r1/codec-inputs07
if systemctl is-active --quiet solvers-r1-vm07-checks.service; then exit 1; fi
if systemctl is-active --quiet solvers-r1-vm07-diagnostic002.service; then exit 1; fi
tar --touch -xzf codec-tools07.tar.gz -C /opt/r1
python3 /home/PC_User/make-codec-attestation-vm07.py
python3 /opt/r1/codec-tools07/run_codec.py freeze \
  --inputs /opt/r1/codec-inputs07 --source-root /opt/r1/current \
  --build-record /opt/r1/codec-build-attestation07.json --out /opt/r1/codec-plan07.json
systemd-run --unit=solvers-r1-vm07-codec \
  --property=RuntimeMaxSec=1830 --property=MemoryMax=8G \
  --property=TimeoutStopSec=15 --property=KillMode=control-group \
  /bin/bash /home/PC_User/run-codec-measurement-vm07.sh
