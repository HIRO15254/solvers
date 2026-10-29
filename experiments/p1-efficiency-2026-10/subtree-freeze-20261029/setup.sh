#!/usr/bin/env bash
# Run on bench after shipping baseline run.toml/solution.sol and the fragments.
set -euo pipefail
D=${1:-/work/bin-SubtreeFreeze}
for name in lock_flop lock_turn lock_deep; do
  "$D/solvers" relock "$D" --add "$D/$name.toml" --freeze-upstream --print-config > "$D/frozen_$name.toml"
done
sha256sum "$D/solvers" "$D/p1_bench" "$D/solution.sol"
