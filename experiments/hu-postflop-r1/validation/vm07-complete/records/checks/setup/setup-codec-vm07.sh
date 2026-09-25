#!/bin/bash
set -euo pipefail
cd /home/PC_User
printf '%s\n' \
  'a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a  source-07.tar.gz' \
  '3de4afef13082dca9e17c38c9cc5f5d1734855f017d812ecb04e9cdbe0f7da27  codec-baseline-source.tar.gz' | sha256sum --check
test -f /opt/r1/bootstrap-complete
mkdir /opt/r1/current /opt/r1/baseline-codec
tar --touch -xzf source-07.tar.gz -C /opt/r1/current
tar --touch -xzf codec-baseline-source.tar.gz -C /opt/r1/baseline-codec
mkdir -p /opt/r1/baseline-codec/crates/formats/examples
cp /opt/r1/current/crates/formats/examples/sol_codec_bench.rs /opt/r1/baseline-codec/crates/formats/examples/
printf '%s\n' 'ed972ffce351fcd31dfbab74771a06a059ef0edb43e44d65397e65f9a85cbcea  /opt/r1/baseline-codec/crates/formats/examples/sol_codec_bench.rs' | sha256sum --check
cp source-07.tar.gz codec-baseline-source.tar.gz /opt/r1/
systemd-run --unit=solvers-r1-vm07-checks \
  --property=RuntimeMaxSec=7500 --property=MemoryMax=48G \
  --property=TimeoutStopSec=15 --property=KillMode=control-group \
  /usr/bin/python3 /opt/r1/current/experiments/hu-postflop-r1/cloud/validate-codec.py \
  --current /opt/r1/current --baseline /opt/r1/baseline-codec --out /opt/r1/codec-build07
