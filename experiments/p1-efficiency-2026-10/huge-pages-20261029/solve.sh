#!/usr/bin/env bash
# Run under rc.py --bench: solve.sh OUT
set -euo pipefail
out=$1; mkdir -p "$out"
[[ $(cat /sys/kernel/mm/transparent_hugepage/enabled) == *'[madvise]'* ]]
cat /work/cfg/flop_srp.toml > "$out/solve.toml"
printf '\nfinal_checkpoint = false\n\n[solver]\nstorage = "f32"\n' >> "$out/solve.toml"
for version in old new; do
  /usr/bin/time -o "$out/$version.time" -f 'minorFaults=%R\nmajorFaults=%F\nmaxRssKiB=%M\nsystemSecs=%S\nuserSecs=%U\nelapsedSecs=%e' \
    "/work/bin-HugePages/solvers-$version" solve "$out/solve.toml" --out "$out/run-$version" --threads 32 > "$out/$version.log" 2>&1
done
