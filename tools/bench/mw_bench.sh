#!/usr/bin/env bash
# Multiway solve throughput benchmark: runs a config N times and reports
# per-run traversals/s plus elapsed seconds (first run may include rollout
# abstraction training unless the artifact cache is already warm).
# Usage: tools/bench/mw_bench.sh <config.toml> [runs]
set -euo pipefail
config="$1"
runs="${2:-3}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
bin="$root/target/release/solvers.exe"
[ -f "$bin" ] || bin="$root/target/release/solvers"

for i in $(seq 1 "$runs"); do
  out="$(mktemp)"
  start=$(date +%s.%N)
  "$bin" solve "$config" --output "$out" >/dev/null
  end=$(date +%s.%N)
  python - "$out" "$start" "$end" <<'EOF'
import json, sys
result = json.load(open(sys.argv[1], encoding="utf-8"))
wall = float(sys.argv[3]) - float(sys.argv[2])
print(f"run: wall={wall:.2f}s solver_elapsed={result['elapsedSecs']:.2f}s "
      f"traversals/s={result['traversalsPerSecond']:.0f} "
      f"sweeps={result['sweeps']} infosets={result['infosets']} "
      f"memMiB={result['memoryBytes'] / 1048576:.0f}")
EOF
  rm -f "$out"
done
