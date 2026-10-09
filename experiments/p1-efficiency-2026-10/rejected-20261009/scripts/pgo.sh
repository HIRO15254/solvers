#!/usr/bin/env bash
# PGO experiment: instrumented build -> training solves -> merged profile -> optimized build.
set -euo pipefail
ROOT=C:/Users/PC_User/orca/workspaces/solvers/cisco
DATA=$ROOT/runs/p1eff/pgo-data
CFG=$ROOT/experiments/p1-perf-2026-10/lane-fold-20261008/configs
PROFDATA=$(ls ~/.rustup/toolchains/*/lib/rustlib/x86_64-pc-windows-msvc/bin/llvm-profdata.exe | head -1)
cd "$ROOT"
rm -rf "$DATA"
RUSTFLAGS="-C target-cpu=native -Cprofile-generate=$DATA" \
  cargo build --release -p cli --target-dir target/pgo-gen
for c in c_river c_turn2 c_flop1; do
  sed -e 's/^max_iterations = .*/max_iterations = 60/' -e 's/^check_every = .*/check_every = 20/' \
    -e 's/^target = .*/target = "0.0001%pot"/' "$CFG/$c.toml" > "runs/p1eff/pgo_$c.toml"
  target/pgo-gen/release/solvers.exe solve "runs/p1eff/pgo_$c.toml" --out "runs/p1eff/pgo_out_$c" --threads 8 >/dev/null
  rm -rf "runs/p1eff/pgo_out_$c"
done
"$PROFDATA" merge -o "$DATA/merged.profdata" "$DATA"
RUSTFLAGS="-C target-cpu=native -Cprofile-use=$DATA/merged.profdata -Cllvm-args=-pgo-warn-mismatch" \
  cargo build --release -p cli --target-dir target/pgo-use
