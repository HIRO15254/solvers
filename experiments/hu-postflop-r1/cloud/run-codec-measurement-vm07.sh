#!/bin/bash
set -euo pipefail
python3 /opt/r1/codec-tools07/run_codec.py run --plan /opt/r1/codec-plan07.json --out /opt/r1/codec-run07
python3 /opt/r1/codec-tools07/run_codec.py check --run /opt/r1/codec-run07 > /opt/r1/codec-check07.json
