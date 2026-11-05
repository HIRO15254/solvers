#!/usr/bin/env bash
# Generic benchmark job: run the uploaded ~/steps.txt line by line from ~/src (class-level Postflop benchmark).
# Each line runs with X=experiments/p2-method-2026-10/gtow-fixed, S=~/src/target/release/solvers and the cloud paths of
# buckets.sh (EXE_SUFFIX, EHS2_DIR, TABLES_DIR, PYTHON). A restarted VM skips steps with `STEP_DONE <n> ` in progress.txt.
# A new instance started by aws_run.sh RESUME=1 after a Spot reclaim restores ~/resume.tgz (the results downloaded from
# the reclaimed instance): their STEP_DONE marks, step logs and ~/out mirror, but not the setup marks of that machine.
# Outputs go under ~/out (mirrored to ~/results/out every 120s; ~/final.tgz at the end).
set -u
R=~/results
mkdir -p "$R" ~/src ~/ehs2 ~/state ~/final ~/out
fail() {
  echo "$(date -u +%T) JOB_FAILED $1 $2" >> "$R/progress.txt"
  exit "$2"
}
if [[ -f ~/resume.tgz && ! -f $R/progress.txt ]] && rm -rf ~/resume && mkdir ~/resume &&
   tar -xzf ~/resume.tgz -C ~/resume && [[ -f ~/resume/results/progress.txt ]]; then
  # Outputs before marks: any download from this instance that shows a restored STEP_DONE also holds its outputs.
  if [[ -d ~/resume/results/out ]]; then
    mkdir -p "$R/out" && cp -r ~/resume/results/out/. ~/out/ && cp -r ~/resume/results/out/. "$R/out/" || fail resume "$?"
  fi
  cp ~/resume/results/step-*.log "$R/" 2>/dev/null
  grep ' STEP_DONE ' ~/resume/results/progress.txt > "$R/progress.txt"
  echo "$(date -u +%T) RESUMED steps=$(grep -c ' STEP_DONE ' "$R/progress.txt")" >> "$R/progress.txt"
fi
echo "$(date -u +%T) JOB_START" >> "$R/progress.txt"
if ! grep -q SETUP_DONE "$R/progress.txt"; then
  sudo apt-get update -qq >/dev/null || fail apt-update "$?"
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config python3 time >/dev/null || fail apt-install "$?"
  curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1 || fail rustup "$?"
  source ~/.cargo/env
  tar -xf ~/src.tar -C ~/src || fail unpack "$?"
  cd ~/src || exit 1
  cargo build --release -p cli --bin solvers -p mw-preflop --example trunk_solve --example l1_leaf_diag > "$R/build.log" 2>&1 || fail build "$?"
  rustc --version >> "$R/build.log"
  sha256sum target/release/solvers target/release/examples/{trunk_solve,l1_leaf_diag} >> "$R/build.log"
  lscpu > "$R/lscpu.txt"; free -g >> "$R/lscpu.txt"
  S=target/release/examples/trunk_solve
  "$S" --config examples/bench/hu_20bb_postflop.toml --leaf-model l1 --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard \
    --iterations 1 --eval-every 0 > "$R/tables32.log" 2>&1
  code=$?
  if [ "$code" != 0 ]; then fail tables32 "$code"; fi
  sed -E "/^\[solver\.abstraction\.buckets\]/,/^\[/ s/^(flop|turn|river) = 32$/\1 = 128/" \
    examples/bench/hu_20bb_postflop.toml > /tmp/b6-128.toml
  "$S" --config /tmp/b6-128.toml --leaf-model l1 --ehs2-cache ~/ehs2/v2-f128-t128-r128.postcard \
    --iterations 1 --eval-every 0 > "$R/tables128.log" 2>&1
  code=$?
  if [ "$code" != 0 ]; then fail tables128 "$code"; fi
  sha256sum ~/ehs2/* >> "$R/tables.log"
  echo "$(date -u +%T) SETUP_DONE" >> "$R/progress.txt"
fi
source ~/.cargo/env
cd ~/src || exit 1
export EXE_SUFFIX= EHS2_DIR=$HOME/ehs2 TABLES_DIR=$HOME/tables PYTHON=python3
export P1_PROGRESS=$R/p1-progress.txt
export X=experiments/p2-method-2026-10/gtow-fixed S=$HOME/src/target/release/solvers
for file in "$HOME"/*.txt "$HOME"/*.toml "$HOME"/*.sh "$HOME"/*.py; do
  [[ -f $file ]] || continue
  tr -d '\r' < "$file" > "$file.clean" && mv "$file.clean" "$file" || fail crlf "$?"
done
mkdir -p "$X/subsets" experiments/p2-method-2026-10/b4s-8h
cp ~/buckets.sh ~/tokens.sh ~/bench_diag.sh "$X/" || fail copy "$?"
for file in "$HOME"/*.py; do
  [[ $(basename "$file") == compare.py ]] || cp "$file" "$X/" || fail copy "$?"
done
for file in "$HOME"/k*-groups.txt; do
  [[ -f $file ]] && { cp "$file" "$X/subsets/" || fail copy "$?"; }
done
cp ~/compare.py ~/b4s-gtow-*.toml experiments/p2-method-2026-10/b4s-8h/ || fail copy "$?"
if ! grep -q TABLES_DONE "$R/progress.txt"; then
  for tree in "$HOME"/b4s-gtow-*.toml; do
    target/release/examples/trunk_solve --config "$tree" --leaf-model l1 \
      --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard \
      --l1-relative "8x2:$HOME/ehs2/v2-f128-t128-r128.postcard" \
      --iterations 1 --eval-every 0 --tables-dir ~/tables > "$R/tables-$(basename "$tree" .toml).log" 2>&1
    code=$?
    if [ "$code" != 0 ]; then fail tables-tree "$code"; fi
  done
  echo "$(date -u +%T) TABLES_DONE" >> "$R/progress.txt"
fi
mirror_outputs() {
  mkdir -p "$R/out"
  (cd ~/out && find . -type f \( -name '*.json' -o -name '*.txt' -o -name '*.md' -o -name '*.csv' -o -name '*.log' \) \
    -exec cp -u --parents {} "$R/out/" \;)
}
(
  while true; do
    mirror_outputs
    sleep 120
  done
) &
mirror_pid=$!
trap 'kill "$mirror_pid" 2>/dev/null || true; wait "$mirror_pid" 2>/dev/null || true' EXIT
n=0
while IFS= read -r line || [[ -n $line ]]; do
  n=$((n + 1))
  [[ -n ${line//[[:space:]]/} ]] || continue
  grep -q " STEP_DONE $n " "$R/progress.txt" && continue
  echo "$(date -u +%T) STEP_START $n" >> "$R/progress.txt"
  start=$(date +%s)
  /usr/bin/time -v bash -c "$line" > "$R/step-$n.log" 2>&1 < /dev/null
  code=$?
  if [ "$code" != 0 ]; then
    kill "$mirror_pid" 2>/dev/null || true
    wait "$mirror_pid" 2>/dev/null || true
    trap - EXIT
    mirror_outputs
    fail "step-$n" "$code"
  fi
  # Mirror first: a results download that shows STEP_DONE must also hold the step's outputs (RESUME restores both).
  mirror_outputs
  echo "$(date -u +%T) STEP_DONE $n seconds=$(( $(date +%s) - start ))" >> "$R/progress.txt"
done < ~/steps.txt
kill "$mirror_pid" 2>/dev/null || true
wait "$mirror_pid" 2>/dev/null || true
trap - EXIT
mirror_outputs || fail results-copy "$?"
cd ~ || exit 1
tar -czf ~/final.tgz out || fail archive "$?"
echo "$(date -u +%T) ALL_DONE" >> "$R/progress.txt"
