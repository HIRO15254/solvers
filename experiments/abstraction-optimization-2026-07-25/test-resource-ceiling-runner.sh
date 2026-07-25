#!/usr/bin/env bash
set -euo pipefail

workspace=$(cd "$(dirname "$0")/../.." && pwd -P)
experiment_dir="$workspace/experiments/abstraction-optimization-2026-07-25"
runner="$experiment_dir/run-resource-ceiling.sh"
mock_solver="$experiment_dir/resource-ceiling-smoke-solver.sh"
watchdog="$experiment_dir/run-with-rss-watchdog.sh"

for script in "$runner" "$mock_solver" "$watchdog"; do
  bash -n "$script"
done

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$1" | awk '{print $1}'
  else
    shasum -a 256 -- "$1" | awk '{print $1}'
  fi
}

fixture=$(mktemp -d /tmp/solvers-resource-ceiling-smoke.XXXXXX)
cleanup() {
  if [[ "${KEEP_RESOURCE_CEILING_FIXTURE:-0}" == "1" ]]; then
    echo "kept resource-ceiling smoke fixture: $fixture" >&2
    return
  fi
  if [[ -d "$fixture" &&
        "$fixture" == /tmp/solvers-resource-ceiling-smoke.* ]]; then
    rm -rf -- "$fixture"
  fi
}
trap cleanup EXIT INT TERM

source_root="$fixture/source"
mkdir -p \
  "$source_root/configs" \
  "$source_root/cache" \
  "$source_root/runs/tournament/T-E64-a0-s1011" \
  "$source_root/runs/cash/C-E64-a0-s1011"
source_root=$(cd "$source_root" && pwd -P)

t_game=$(printf '1%.0s' {1..64})
c_game=$(printf '2%.0s' {1..64})
t_abstraction=$(printf '3%.0s' {1..64})
c_abstraction=$(printf '4%.0s' {1..64})
t_configuration=$(printf '5%.0s' {1..64})
c_configuration=$(printf '6%.0s' {1..64})
t_source_config_hash=$(printf '7%.0s' {1..64})
c_source_config_hash=$(printf '8%.0s' {1..64})
cache="$source_root/cache/ehs2-table-f64-t64-r64.postcard"
printf 'fixture ehs2 cache\n' >"$cache"

make_config() {
  local path=$1
  local candidate=$2
  local game=$3
  local abstraction=$4
  local configuration=$5
  printf '%s\n' \
    "# mock_candidate=$candidate" \
    "# mock_game_fingerprint=$game" \
    "# mock_abstraction_fingerprint=$abstraction" \
    "# mock_configuration_fingerprint=$configuration" \
    '# mock_source_sweeps=20' \
    '[game.abstraction]' \
    "artifact_cache = \"$cache\"" \
    'flop_buckets = 64' \
    'kind = "ehs2-table"' \
    'recall = "full"' \
    'river_buckets = 64' \
    'turn_buckets = 64' \
    '' \
    '[run]' \
    'max_memory_bytes = 1000' \
    'seed = 1011' \
    'sweeps = 50000' \
    >"$path"
}

t_config="$source_root/configs/T-E64-a0-s1011.toml"
c_config="$source_root/configs/C-E64-a0-s1011.toml"
make_config \
  "$t_config" T-E64 "$t_game" "$t_abstraction" "$t_configuration"
make_config \
  "$c_config" C-E64 "$c_game" "$c_abstraction" "$c_configuration"

printf '%s\n' \
  'role,case,id,abstraction_seed,solver_seed,evaluation_seed,config,cache,config_hash,game_fingerprint' \
  "candidate,tournament,T-E64,0,1011,424242,$t_config,$cache,$t_source_config_hash,$t_game" \
  "candidate,cash,C-E64,0,1011,424242,$c_config,$cache,$c_source_config_hash,$c_game" \
  >"$source_root/configs/configs.csv"

make_source_result() {
  local destination=$1
  local config_hash=$2
  local game=$3
  local abstraction=$4
  local configuration=$5
  jq -n \
    --arg config_hash "$config_hash" \
    --arg game "$game" \
    --arg abstraction "$abstraction" \
    --arg configuration "$configuration" '
      {
        schemaVersion: 3,
        kind: "preflop-multiway",
        status: "completed",
        sweeps: 20,
        infosets: 200,
        memoryBytes: 100,
        elapsedSecs: 1.0,
        seats: [{}, {}, {}, {}, {}, {}],
        configHash: $config_hash,
        effectiveConfig: {run: {max_memory_bytes: 1000}},
        gameFingerprint: $game,
        abstractionFingerprint: $abstraction,
        configurationFingerprint: $configuration
      }
    ' >"$destination"
}

t_run_dir="$source_root/runs/tournament/T-E64-a0-s1011"
c_run_dir="$source_root/runs/cash/C-E64-a0-s1011"
make_source_result \
  "$t_run_dir/run.json" "$t_source_config_hash" "$t_game" \
  "$t_abstraction" "$t_configuration"
make_source_result \
  "$c_run_dir/run.json" "$c_source_config_hash" "$c_game" \
  "$c_abstraction" "$c_configuration"
printf 'tournament source checkpoint\n' >"$t_run_dir/checkpoint.mwckpt"
printf 'cash source checkpoint\n' >"$c_run_dir/checkpoint.mwckpt"

source_hashes_before="$fixture/source-hashes.before"
find "$source_root" -type f -print0 |
  sort -z |
  while IFS= read -r -d '' path; do
    printf '%s  %s\n' "$(sha256_file "$path")" "$path"
  done >"$source_hashes_before"

export MOCK_RESOURCE_CEILING_CALL_LOG="$fixture/solver-calls.csv"
result_root="$fixture/results"
common_args=(
  "$source_root"
  "$result_root"
  --solver "$mock_solver"
  --watchdog "$watchdog"
  --target-sweeps 100
  --tournament-cap-bytes 300
  --cash-cap-bytes 350
  --rss-limit-bytes 67108864
)

"$runner" "${common_args[@]}" >"$fixture/first.stdout"
grep -q '^EXPECTED_RESOURCE_LIMIT candidate=T-E64' "$fixture/first.stdout"
grep -q '^EXPECTED_RESOURCE_LIMIT candidate=C-E64' "$fixture/first.stdout"
[[ "$(wc -l <"$MOCK_RESOURCE_CEILING_CALL_LOG" | tr -d '[:space:]')" == "2" ]]

summary="$result_root/resource-ceiling-s100-summary.csv"
[[ -f "$summary" ]]
[[ "$(wc -l <"$summary" | tr -d '[:space:]')" == "3" ]]
[[ "$(grep -c '"resource_limit",true,"resource_limit"' "$summary")" == "2" ]]

find "$result_root/runs" -name meta.json -not -path '*/segments/*' -type f |
  while IFS= read -r meta; do
    jq -e '
      .schema == "solvers.abstraction-resource-ceiling-run/v1"
      and .status == "resource_limit"
      and .expectation.met == true
      and .resumeCompatibility.mode
        == "explicit-config-plus-copied-checkpoint"
      and .resumeCompatibility.operationalMemoryExcludedFromConfigurationFingerprint
        == true
      and .fork.config.mutation.onlyRawConfigMutation == true
      and .fork.checkpoint.inputSha256 != .fork.checkpoint.currentSha256
      and .artifacts.result.sha256 != null
      and .sourceArtifacts.contentUnchanged == true
      and .resources.resourceLimitSource == "solver_memory"
      and .result.status == "resource_limit"
    ' "$meta" >/dev/null
    segment=$(jq -r '.artifacts.latestSegment' "$meta")
    jq -e '
      .schema == "solvers.abstraction-resource-ceiling-segment/v1"
      and .status == "resource_limit"
      and .commandExitCode == 75
      and .resources.resourceLimitSource == "solver_memory"
      and .sourceArtifactsContentUnchanged == true
    ' "$segment" >/dev/null
  done

t_fork_config=$(find "$result_root/runs/tournament" -name config.toml -type f)
c_fork_config=$(find "$result_root/runs/cash" -name config.toml -type f)
[[ "$(awk '$1 == "max_memory_bytes" {print $3}' "$t_fork_config")" == "300" ]]
[[ "$(awk '$1 == "max_memory_bytes" {print $3}' "$c_fork_config")" == "350" ]]
cmp \
  <(sed 's/^max_memory_bytes = [0-9][0-9]*/max_memory_bytes = CAP/' "$t_config") \
  <(sed 's/^max_memory_bytes = [0-9][0-9]*/max_memory_bytes = CAP/' "$t_fork_config")
cmp \
  <(sed 's/^max_memory_bytes = [0-9][0-9]*/max_memory_bytes = CAP/' "$c_config") \
  <(sed 's/^max_memory_bytes = [0-9][0-9]*/max_memory_bytes = CAP/' "$c_fork_config")

source_hashes_after="$fixture/source-hashes.after"
find "$source_root" -type f -print0 |
  sort -z |
  while IFS= read -r -d '' path; do
    printf '%s  %s\n' "$(sha256_file "$path")" "$path"
  done >"$source_hashes_after"
cmp "$source_hashes_before" "$source_hashes_after"

"$runner" "${common_args[@]}" >"$fixture/reuse.stdout"
[[ "$(wc -l <"$MOCK_RESOURCE_CEILING_CALL_LOG" | tr -d '[:space:]')" == "2" ]]
[[ "$(grep -c '^REUSED expected resource_limit' "$fixture/reuse.stdout")" == "2" ]]
[[ -z "$(find "$result_root" -name '*.tmp.*' -print -quit)" ]]
[[ ! -e "$result_root/.resource-ceiling.lock" ]]

plan_root="$fixture/plan-results"
"$runner" "${common_args[@]/$result_root/$plan_root}" --plan >"$fixture/plan.stdout"
[[ "$(grep -c '^PLAN action=fork' "$fixture/plan.stdout")" == "2" ]]
grep -q 'planned 2 serial full-recall K64 resource-ceiling jobs' "$fixture/plan.stdout"
[[ "$(wc -l <"$MOCK_RESOURCE_CEILING_CALL_LOG" | tr -d '[:space:]')" == "2" ]]

export MOCK_RESOURCE_CEILING_COMPLETE_CANDIDATE=T-E64
completed_root="$fixture/completed-results"
set +e
"$runner" \
  "$source_root" "$completed_root" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" \
  --target-sweeps 100 \
  --tournament-cap-bytes 300 \
  --cash-cap-bytes 350 \
  --rss-limit-bytes 67108864 \
  >"$fixture/completed.stdout" 2>"$fixture/completed.stderr"
completed_exit=$?
set -e
[[ "$completed_exit" == "1" ]]
grep -q '^TARGET_COMPLETED candidate=T-E64' "$fixture/completed.stderr"
completed_summary="$completed_root/resource-ceiling-s100-summary.csv"
[[ "$(wc -l <"$completed_summary" | tr -d '[:space:]')" == "3" ]]
grep -q '"target_completed",false,"completed",100' "$completed_summary"
unset MOCK_RESOURCE_CEILING_COMPLETE_CANDIDATE

rss_watchdog="$fixture/rss-watchdog.sh"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'limit=$1' \
  'meta=$2' \
  'stdout=$3' \
  'stderr=$4' \
  'mkdir -p "$(dirname "$meta")" "$(dirname "$stdout")" "$(dirname "$stderr")"' \
  '{' \
  '  echo "pid=fixture"' \
  '  echo "exit_code=130"' \
  '  echo "wall_seconds=1"' \
  '  echo "peak_rss_bytes=$((limit + 1))"' \
  '  echo "rss_limit_bytes=$limit"' \
  '  echo "rss_limit_exceeded=1"' \
  '  echo "rss_samples=2"' \
  '  echo "rss_monitor_error=0"' \
  '  echo "forwarded_signal=none"' \
  '} >"$meta"' \
  ': >"$stdout"' \
  ': >"$stderr"' \
  'exit 75' \
  >"$rss_watchdog"
chmod +x "$rss_watchdog"
rss_root="$fixture/rss-results"
set +e
"$runner" \
  "$source_root" "$rss_root" \
  --solver "$mock_solver" \
  --watchdog "$rss_watchdog" \
  --target-sweeps 100 \
  --tournament-cap-bytes 300 \
  --cash-cap-bytes 350 \
  --rss-limit-bytes 67108864 \
  >"$fixture/rss.stdout" 2>"$fixture/rss.stderr"
rss_exit=$?
set -e
[[ "$rss_exit" == "75" ]]
rss_summary="$rss_root/resource-ceiling-s100-summary.csv"
grep -q '"watchdog_rss_limit",false' "$rss_summary"
grep -q ',75,1,67108865,67108864,"watchdog_rss",' "$rss_summary"
rss_segment=$(find "$rss_root" -path '*/segments/0001/meta.json' -type f)
jq -e '
  .status == "watchdog_rss_limit"
  and .commandExitCode == 75
  and .watchdogChildExitCode == 130
  and .resources.rssLimitExceeded == true
  and .resources.resourceLimitSource == "watchdog_rss"
' "$rss_segment" >/dev/null

signal_root="$fixture/signal-results"
signal_log="$fixture/signal-calls.csv"
export MOCK_RESOURCE_CEILING_CALL_LOG="$signal_log"
export MOCK_RESOURCE_CEILING_SLEEP_SECONDS=30
set +e
"$runner" \
  "$source_root" "$signal_root" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" \
  --target-sweeps 100 \
  --tournament-cap-bytes 300 \
  --cash-cap-bytes 350 \
  --rss-limit-bytes 67108864 \
  >"$fixture/signal.stdout" 2>"$fixture/signal.stderr" &
signal_runner_pid=$!
set -e
for _ in {1..100}; do
  [[ -s "$signal_log" ]] && break
  sleep 0.05
done
[[ -s "$signal_log" ]]
kill -TERM "$signal_runner_pid"
set +e
wait "$signal_runner_pid"
signal_exit=$?
set -e
[[ "$signal_exit" == "143" ]]
signal_summary="$signal_root/resource-ceiling-s100-summary.csv"
[[ -f "$signal_summary" ]]
grep -q '"interrupted",false' "$signal_summary"
grep -q 'RESOURCE_CEILING_STOP candidate=T-E64 status=interrupted exit=143' \
  "$fixture/signal.stderr"
[[ ! -e "$signal_root/.resource-ceiling.lock" ]]
unset MOCK_RESOURCE_CEILING_SLEEP_SECONDS

tampered_checkpoint=$(
  find "$result_root/runs/tournament" -name checkpoint.mwckpt -type f
)
printf 'tampered\n' >>"$tampered_checkpoint"
set +e
"$runner" "${common_args[@]}" >"$fixture/tamper.stdout" 2>"$fixture/tamper.stderr"
tamper_exit=$?
set -e
[[ "$tamper_exit" == "3" ]]
grep -q 'existing job provenance or artifact SHA is invalid for T-E64' \
  "$fixture/tamper.stderr"

source_hashes_final="$fixture/source-hashes.final"
find "$source_root" -type f -print0 |
  sort -z |
  while IFS= read -r -d '' path; do
    printf '%s  %s\n' "$(sha256_file "$path")" "$path"
  done >"$source_hashes_final"
cmp "$source_hashes_before" "$source_hashes_final"
echo "resource ceiling runner smoke test passed"
