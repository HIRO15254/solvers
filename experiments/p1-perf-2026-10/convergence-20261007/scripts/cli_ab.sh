#!/bin/bash
# usage: cli_ab.sh LABEL CONFIG THREADS
# Full `solvers solve` (checkpoints + .sol export) under /usr/bin/time -v, base then new.
source ~/.cargo/env
mkdir -p ~/results
for side in base new; do
  rm -rf /tmp/cli_$1_$side
  /usr/bin/time -v ~/$side/target/release/solvers solve ~/cfg/$2 --out /tmp/cli_$1_$side --threads $3 \
    > ~/results/cli_$1_$side.out 2> ~/results/cli_$1_$side.err
  ls -l /tmp/cli_$1_$side >> ~/results/cli_$1_$side.out
  cp /tmp/cli_$1_$side/events.jsonl ~/results/cli_$1_${side}_events.jsonl 2>/dev/null || true
  for view in summary strategy ev; do
    ~/$side/target/release/solvers export /tmp/cli_$1_$side/solution.sol $view --format csv \
      --output ~/results/cli_$1_${side}_$view.csv 2>>~/results/cli_$1_$side.err
  done
  rm -rf /tmp/cli_$1_$side
done
echo "CLI_DONE $1"
