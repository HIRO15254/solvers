#!/bin/bash
set -euo pipefail
exec systemd-run --unit=solvers-r1-vm08-measure \
  --property=RuntimeMaxSec=7200 --property=MemoryMax=6G \
  --property=MemorySwapMax=0 --property=TimeoutStopSec=15 --property=KillMode=control-group \
  /usr/bin/python3 /opt/r1/vm08/runner/run.py run --phase measure --run /opt/r1/vm08/run
