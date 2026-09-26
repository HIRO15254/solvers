#!/bin/bash
set -euo pipefail
test -f /opt/r1/bootstrap-complete
test -f /opt/r1/vm08/setup.json
mkdir /opt/r1/vm08/runner
cp /tmp/r1-vm08-upload/run.py /tmp/r1-vm08-upload/protocol.json /opt/r1/vm08/runner/
sha256sum /opt/r1/vm08/runner/run.py /opt/r1/vm08/runner/protocol.json
exec systemd-run --unit=solvers-r1-vm08-build \
  --property=RuntimeMaxSec=18000 --property=MemoryMax=6G \
  --property=MemorySwapMax=0 --property=TimeoutStopSec=15 --property=KillMode=control-group \
  /usr/bin/python3 /opt/r1/vm08/runner/run.py run --phase build \
  --sources /opt/r1/vm08/packs/sources.json --inputs /opt/r1/vm08/inputs \
  --out /opt/r1/vm08/run --targets-root /opt/r1/target/context08 \
  --deadline-utc 2026-09-26T07:00:00Z
