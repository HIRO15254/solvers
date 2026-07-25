#!/usr/bin/env bash
set -euo pipefail

readonly DEFAULT_RSS_LIMIT_BYTES=8053063680
readonly MAX_RSS_LIMIT_BYTES=8589934592
readonly PRODUCTION_SOLVER_CAP_BYTES=6442450944
readonly EXPECTED_REPRESENTATION_NODE_LIMIT=4294967296
readonly EXPECTED_JOB_COUNT=60
readonly SUMMARY_HEADER='case,scenario_id,seats,stack_bb,buckets,status,outcome,exact_nodes,first_exceeding_prefix_nodes,largest_accepted_prefix_nodes,columns,estimated_dense_arena_payload_bytes,wall_seconds,preflight_reported_peak_rss_bytes,watchdog_exit_code,child_exit_code,segment_wall_seconds,preflight_process_sampled_peak_rss_bytes,preflight_process_rss_watchdog_limit_bytes,rss_limit_exceeded,rss_monitor_error,resource_limit_source,dense_arena_payload_estimate_limit_bytes,dense_feasibility_scope,production_process_feasibility,game_fingerprint,tree_contract_fingerprint,manifest_sha256,plan_sha256,plan_metadata_sha256,preflight_sha256,watchdog_sha256,runner_sha256,generator_sha256,stdout_sha256,stderr_sha256,watchdog_metadata_sha256,job_fingerprint,stdout_log,watchdog_metadata,job_meta,job_meta_sha256'

usage() {
  cat >&2 <<'EOF'
usage:
  run-dense-preflight.sh RESULT_ROOT [OPTIONS]

options:
  --manifest PATH          fixed10/K-grid manifest
  --preflight PATH         action_tree_preflight executable override
  --generator PATH         plan generator override
  --watchdog PATH          RSS watchdog override
  --rss-limit-bytes N      sampled process RSS limit
                           (default: 8053063680; maximum: 8589934592)
  --plan                   validate and print all 60 serial jobs only
  -h, --help               show this help

Every job is a separate process. Typed dense-arena MemoryLimit and NodeId
checkpoint outcomes are expected feasibility results, are recorded, and do
not stop the remaining matrix. A sampled process-RSS limit is recorded as an
external resource failure after the remaining jobs have also been attempted.
EOF
}

fail() {
  echo "dense preflight runner: $*" >&2
  exit 2
}

is_positive_i64() {
  jq -en --arg value "$1" '
    ($value | test("^[1-9][0-9]*$"))
    and (($value | tonumber) <= 9223372036854775807)
  ' >/dev/null
}

is_nonnegative_i64() {
  jq -en --arg value "$1" '
    ($value | test("^(0|[1-9][0-9]*)$"))
    and (($value | tonumber) <= 9223372036854775807)
  ' >/dev/null
}

is_nonnegative_number() {
  jq -en --arg value "$1" '
    $value | test("^(0|[1-9][0-9]*)([.][0-9]+)?$")
  ' >/dev/null
}

absolute_existing_file() {
  local path=$1
  local directory basename
  [[ -f "$path" && ! -L "$path" ]] || return 1
  directory=$(cd "$(dirname "$path")" && pwd -P)
  basename=$(basename "$path")
  printf '%s/%s\n' "$directory" "$basename"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -- "$1" | awk '{print $1}'
  else
    fail "neither sha256sum nor shasum is available"
  fi
}

hash_material() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
  else
    fail "neither sha256sum nor shasum is available"
  fi
}

line_value() {
  local line=$1
  local key=$2
  local token found=
  for token in $line; do
    if [[ "$token" == "$key="* ]]; then
      [[ -z "$found" ]] || return 3
      found=${token#*=}
    fi
  done
  [[ -n "$found" ]] || return 1
  printf '%s\n' "$found"
}

watchdog_value() {
  local path=$1
  local key=$2
  awk -F= -v wanted="$key" '
    $1 == wanted {print $2; found++}
    END {if (found != 1) exit 3}
  ' "$path"
}

publish_summary() {
  [[ -n "${summary_work:-}" && -f "$summary_work" ]] || return
  local temporary="${summary_path}.tmp.$$"
  cp -- "$summary_work" "$temporary"
  mv -f -- "$temporary" "$summary_path"
}

cleanup() {
  local status=$?
  trap - EXIT INT TERM HUP
  if [[ -n "${active_watchdog_pid:-}" ]]; then
    kill -TERM "$active_watchdog_pid" 2>/dev/null || true
    wait "$active_watchdog_pid" 2>/dev/null || true
  fi
  publish_summary
  if [[ -n "${summary_work:-}" && -f "$summary_work" ]]; then
    rm -f -- "$summary_work"
  fi
  if [[ -n "${lock_dir:-}" && -d "$lock_dir" ]]; then
    rmdir -- "$lock_dir" 2>/dev/null || true
  fi
  exit "$status"
}

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
workspace=$(cd "$script_dir/../.." && pwd -P)
manifest="$script_dir/dense-preflight-manifest.json"
preflight="$workspace/target/research-release/release/examples/action_tree_preflight"
generator="$script_dir/generate-dense-preflight-plan.py"
watchdog="$workspace/experiments/abstraction-optimization-2026-07-25/run-with-rss-watchdog.sh"
rss_limit_bytes=$DEFAULT_RSS_LIMIT_BYTES
plan_only=0

(( $# >= 1 )) || {
  usage
  exit 2
}
result_root=$1
shift
while (( $# > 0 )); do
  case "$1" in
    --manifest)
      (( $# >= 2 )) || fail "--manifest requires a path"
      manifest=$2
      shift 2
      ;;
    --preflight)
      (( $# >= 2 )) || fail "--preflight requires a path"
      preflight=$2
      shift 2
      ;;
    --generator)
      (( $# >= 2 )) || fail "--generator requires a path"
      generator=$2
      shift 2
      ;;
    --watchdog)
      (( $# >= 2 )) || fail "--watchdog requires a path"
      watchdog=$2
      shift 2
      ;;
    --rss-limit-bytes)
      (( $# >= 2 )) || fail "--rss-limit-bytes requires a value"
      rss_limit_bytes=$2
      shift 2
      ;;
    --plan)
      plan_only=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown option $1"
      ;;
  esac
done

command -v python3 >/dev/null || fail "python3 is required"
command -v jq >/dev/null || fail "jq is required"
is_positive_i64 "$rss_limit_bytes" || fail "invalid RSS limit $rss_limit_bytes"
(( rss_limit_bytes <= MAX_RSS_LIMIT_BYTES )) ||
  fail "RSS limit exceeds the 8 GiB runner maximum"
manifest=$(absolute_existing_file "$manifest") || fail "manifest is not a regular file"
generator=$(absolute_existing_file "$generator") || fail "plan generator is not a regular file"
preflight=$(absolute_existing_file "$preflight") || fail "preflight executable is missing"
watchdog=$(absolute_existing_file "$watchdog") || fail "watchdog is not a regular file"
[[ -x "$generator" ]] || fail "plan generator is not executable"
[[ -x "$preflight" ]] || fail "preflight binary is not executable"
[[ -x "$watchdog" ]] || fail "watchdog is not executable"
case "$result_root" in
  *','*|*$'\n'*|*$'\r'*) fail "result root may not contain commas or newlines" ;;
esac
mkdir -p "$result_root"
result_root=$(cd "$result_root" && pwd -P)

lock_dir="$result_root/.dense-preflight.lock"
mkdir "$lock_dir" 2>/dev/null || fail "another dense preflight runner owns $result_root"
active_watchdog_pid=
summary_work=
summary_path="$result_root/dense-preflight-summary.csv"
trap cleanup EXIT INT TERM HUP

plan_csv="$result_root/dense-preflight-plan.csv"
plan_metadata="$result_root/dense-preflight-metadata.json"
python3 "$generator" "$manifest" "$plan_csv" "$plan_metadata"
[[ -f "$plan_csv" && -f "$plan_metadata" ]] ||
  fail "plan generator did not publish both artifacts"
[[ "$(wc -l <"$plan_csv" | tr -d '[:space:]')" == "$((EXPECTED_JOB_COUNT + 1))" ]] ||
  fail "dense preflight plan does not contain $EXPECTED_JOB_COUNT jobs"
[[ "$(head -n 1 "$plan_csv")" == \
  'case,scenario_id,seats,stack_bb,buckets,max_memory_bytes,rss_limit_bytes,game_fingerprint,tree_contract_fingerprint' ]] ||
  fail "dense preflight plan header changed"

manifest_sha=$(sha256_file "$manifest")
plan_sha=$(sha256_file "$plan_csv")
plan_metadata_sha=$(sha256_file "$plan_metadata")
preflight_sha=$(sha256_file "$preflight")
watchdog_sha=$(sha256_file "$watchdog")
runner_sha=$(sha256_file "${BASH_SOURCE[0]}")
generator_sha=$(sha256_file "$generator")
jq -e \
  --arg manifest_sha "$manifest_sha" \
  --arg plan_sha "$plan_sha" \
  --argjson jobs "$EXPECTED_JOB_COUNT" \
  --argjson dense_limit "$MAX_RSS_LIMIT_BYTES" \
  --argjson rss_limit "$DEFAULT_RSS_LIMIT_BYTES" \
  --argjson solver_cap "$PRODUCTION_SOLVER_CAP_BYTES" '
    .schema == "solvers.abstraction-transfer-dense-preflight-plan/v1"
    and .manifest.sha256 == $manifest_sha
    and .planSha256 == $plan_sha
    and .jobCount == $jobs
    and .resourceSemantics.denseFeasibilityScope
      == "arena-payload-estimate-only"
    and .resourceSemantics.denseArenaPayloadEstimateLimitBytes == $dense_limit
    and .resourceSemantics.preflightProcessRssScope
      == "count-only-preflight-process"
    and .resourceSemantics.preflightProcessRssWatchdogLimitBytes == $rss_limit
    and .resourceSemantics.productionProcessFeasibilityEstablished == false
    and .resourceSemantics.requiredProductionValidation.kind
      == "materialized-solve"
    and .resourceSemantics.requiredProductionValidation.solverDenseArenaCapBytes
      == $solver_cap
    and .resourceSemantics.requiredProductionValidation.processLimitBytes
      == $dense_limit
  ' "$plan_metadata" >/dev/null ||
  fail "plan metadata does not bind the generated plan"

summary_work="$result_root/.dense-preflight-summary.work.$$"
if (( plan_only == 0 )); then
  printf '%s\n' "$SUMMARY_HEADER" >"$summary_work"
  publish_summary
else
  summary_work=
fi

jobs_seen=0
external_resource_failures=0
while IFS=, read -r \
  case_name scenario_id seats stack_bb buckets max_memory_bytes \
  manifest_rss_limit game_fingerprint tree_fingerprint; do
  jobs_seen=$((jobs_seen + 1))
  [[ "$case_name" == "tournament" || "$case_name" == "cash" ]] ||
    fail "invalid case in plan"
  is_positive_i64 "$seats" &&
    is_positive_i64 "$stack_bb" &&
    is_positive_i64 "$buckets" &&
    is_positive_i64 "$max_memory_bytes" &&
    is_positive_i64 "$manifest_rss_limit" ||
    fail "invalid numeric field in plan row $jobs_seen"
  [[ "$max_memory_bytes" == "$MAX_RSS_LIMIT_BYTES" ]] ||
    fail "plan row $jobs_seen is not an 8 GiB dense preflight"
  [[ "$game_fingerprint" =~ ^[0-9a-f]{64}$ &&
     "$tree_fingerprint" =~ ^[0-9a-f]{64}$ ]] ||
    fail "invalid fingerprint in plan row $jobs_seen"

  job_material=$(
    printf '%s\n' \
      'solvers.abstraction-transfer-dense-preflight-job/v1' \
      "manifest_sha256=$manifest_sha" \
      "plan_sha256=$plan_sha" \
      "plan_metadata_sha256=$plan_metadata_sha" \
      "preflight_sha256=$preflight_sha" \
      "watchdog_sha256=$watchdog_sha" \
      "runner_sha256=$runner_sha" \
      "generator_sha256=$generator_sha" \
      "rss_limit_bytes=$rss_limit_bytes" \
      "case=$case_name" \
      "scenario_id=$scenario_id" \
      "seats=$seats" \
      "stack_bb=$stack_bb" \
      "buckets=$buckets" \
      "max_memory_bytes=$max_memory_bytes" \
      "game_fingerprint=$game_fingerprint" \
      "tree_contract_fingerprint=$tree_fingerprint"
  )
  job_fingerprint=$(printf '%s' "$job_material" | hash_material)
  job_dir="$result_root/runs/$case_name/$scenario_id/k$buckets/$job_fingerprint"
  stdout_log="$job_dir/stdout.log"
  stderr_log="$job_dir/stderr.log"
  watchdog_metadata="$job_dir/watchdog.txt"
  job_meta="$job_dir/meta.json"

  if (( plan_only == 1 )); then
    echo \
      "PLAN case=$case_name scenario=$scenario_id seats=$seats stack_bb=$stack_bb buckets=$buckets max_memory_bytes=$max_memory_bytes rss_limit_bytes=$rss_limit_bytes"
    continue
  fi

  mkdir -p "$job_dir"
  [[ "$(sha256_file "$manifest")" == "$manifest_sha" &&
     "$(sha256_file "$plan_csv")" == "$plan_sha" &&
     "$(sha256_file "$plan_metadata")" == "$plan_metadata_sha" &&
     "$(sha256_file "$preflight")" == "$preflight_sha" &&
     "$(sha256_file "$watchdog")" == "$watchdog_sha" &&
     "$(sha256_file "${BASH_SOURCE[0]}")" == "$runner_sha" &&
     "$(sha256_file "$generator")" == "$generator_sha" ]] ||
    fail "input drift before $scenario_id/k$buckets"

  echo \
    "START case=$case_name scenario=$scenario_id seats=$seats stack_bb=$stack_bb buckets=$buckets"
  set +e
  "$watchdog" \
    "$rss_limit_bytes" \
    "$watchdog_metadata" \
    "$stdout_log" \
    "$stderr_log" \
    "$preflight" \
    --profile benchmark \
    --case "$case_name" \
    --seats "$seats" \
    --stack-bb "$stack_bb" \
    --postflop one-size \
    --flop-buckets "$buckets" \
    --turn-buckets "$buckets" \
    --river-buckets "$buckets" \
    --max-memory-bytes "$max_memory_bytes" &
  active_watchdog_pid=$!
  wait "$active_watchdog_pid"
  command_exit_code=$?
  active_watchdog_pid=
  set -e

  [[ -f "$watchdog_metadata" ]] ||
    fail "watchdog metadata missing for $scenario_id/k$buckets"
  segment_wall_seconds=$(watchdog_value "$watchdog_metadata" wall_seconds) ||
    fail "invalid watchdog wall time for $scenario_id/k$buckets"
  segment_peak_rss_bytes=$(watchdog_value "$watchdog_metadata" peak_rss_bytes) ||
    fail "invalid watchdog peak RSS for $scenario_id/k$buckets"
  rss_limit_exceeded=$(watchdog_value "$watchdog_metadata" rss_limit_exceeded) ||
    fail "invalid watchdog RSS outcome for $scenario_id/k$buckets"
  rss_monitor_error=$(watchdog_value "$watchdog_metadata" rss_monitor_error) ||
    fail "invalid watchdog monitor outcome for $scenario_id/k$buckets"
  child_exit_code=$(watchdog_value "$watchdog_metadata" exit_code) ||
    fail "invalid child exit code for $scenario_id/k$buckets"
  watchdog_recorded_limit=$(watchdog_value "$watchdog_metadata" rss_limit_bytes) ||
    fail "invalid watchdog limit for $scenario_id/k$buckets"
  [[ "$segment_wall_seconds" =~ ^[0-9]+$ &&
     "$segment_peak_rss_bytes" =~ ^[0-9]+$ &&
     "$rss_limit_exceeded" =~ ^[01]$ &&
     "$rss_monitor_error" =~ ^[01]$ &&
     "$child_exit_code" =~ ^[0-9]+$ &&
     "$watchdog_recorded_limit" == "$rss_limit_bytes" ]] ||
    fail "watchdog contract mismatch for $scenario_id/k$buckets"
  if [[ "$rss_limit_exceeded" == "1" ]]; then
    [[ "$command_exit_code" == "75" ]] ||
      fail "RSS-limit watchdog exited $command_exit_code instead of 75"
  elif [[ "$rss_monitor_error" == "1" ]]; then
    [[ "$command_exit_code" == "70" ]] ||
      fail "RSS-monitor watchdog exited $command_exit_code instead of 70"
  else
    [[ "$child_exit_code" == "$command_exit_code" ]] ||
      fail "watchdog/child exit mismatch for $scenario_id/k$buckets"
  fi

  status=
  outcome=
  exact_nodes=
  first_exceeding_prefix_nodes=
  largest_accepted_prefix_nodes=
  columns=
  dense_arena_bytes=
  wall_seconds=
  preflight_peak_rss_bytes=
  resource_limit_source=none
  result_line=

  if [[ "$rss_monitor_error" == "1" ]]; then
    fail "RSS monitor failed for $scenario_id/k$buckets"
  elif [[ "$rss_limit_exceeded" == "1" ]]; then
    status=rss_limit
    outcome=rss-limit
    resource_limit_source=watchdog_rss
    external_resource_failures=$((external_resource_failures + 1))
  else
    result_count=$(grep -c '^RESULT ' "$stdout_log" || true)
    [[ "$result_count" == "1" ]] ||
      fail "expected one RESULT line for $scenario_id/k$buckets"
    result_line=$(grep '^RESULT ' "$stdout_log")
    [[ "$(line_value "$result_line" profile)" == "benchmark" &&
       "$(line_value "$result_line" case)" == "$case_name" &&
       "$(line_value "$result_line" seats)" == "$seats" &&
       "$(line_value "$result_line" stack_bb)" == "$stack_bb" &&
       "$(line_value "$result_line" postflop)" == "one-size" &&
       "$(line_value "$result_line" game_fingerprint)" == "$game_fingerprint" &&
       "$(line_value "$result_line" tree_contract_fingerprint)" == "$tree_fingerprint" &&
       "$(line_value "$result_line" flop_buckets)" == "$buckets" &&
       "$(line_value "$result_line" turn_buckets)" == "$buckets" &&
       "$(line_value "$result_line" river_buckets)" == "$buckets" &&
       "$(line_value "$result_line" max_memory_bytes)" == "$max_memory_bytes" ]] ||
      fail "RESULT identity mismatch for $scenario_id/k$buckets"
    outcome=$(line_value "$result_line" outcome) ||
      fail "RESULT outcome missing for $scenario_id/k$buckets"
    wall_seconds=$(line_value "$result_line" wall_seconds) ||
      fail "RESULT wall time missing for $scenario_id/k$buckets"
    preflight_peak_rss_bytes=$(line_value "$result_line" preflight_peak_rss_bytes) ||
      fail "RESULT peak RSS missing for $scenario_id/k$buckets"
    is_nonnegative_number "$wall_seconds" &&
      [[ "$preflight_peak_rss_bytes" =~ ^[0-9]+$ ]] ||
      fail "invalid RESULT resources for $scenario_id/k$buckets"
    case "$outcome" in
      complete)
        [[ "$command_exit_code" == "0" ]] ||
          fail "complete preflight exited $command_exit_code"
        [[ "$(line_value "$result_line" node_limit_kind)" == "representation" &&
           "$(line_value "$result_line" node_limit)" == \
             "$EXPECTED_REPRESENTATION_NODE_LIMIT" ]] ||
          fail "complete RESULT representation contract mismatch for $scenario_id/k$buckets"
        status=completed
        exact_nodes=$(line_value "$result_line" nodes)
        columns=$(line_value "$result_line" columns)
        dense_arena_bytes=$(line_value "$result_line" dense_arena_bytes)
        is_positive_i64 "$exact_nodes" &&
          is_positive_i64 "$columns" &&
          is_positive_i64 "$dense_arena_bytes" &&
          (( exact_nodes <= EXPECTED_REPRESENTATION_NODE_LIMIT )) &&
          (( dense_arena_bytes <= max_memory_bytes )) ||
          fail "complete RESULT feasibility mismatch for $scenario_id/k$buckets"
        resource_limit_source=none
        ;;
      memory-limit)
        [[ "$command_exit_code" == "0" || "$command_exit_code" == "75" ]] ||
          fail "typed MemoryLimit exited $command_exit_code"
        [[ "$(line_value "$result_line" tree_error)" == "MemoryLimit" &&
           "$(line_value "$result_line" node_limit_kind)" == "representation" &&
           "$(line_value "$result_line" node_limit)" == \
             "$EXPECTED_REPRESENTATION_NODE_LIMIT" &&
           "$(line_value "$result_line" memory_limit_kind)" == \
             "dense-arena-estimate-bytes" ]] ||
          fail "typed MemoryLimit contract mismatch for $scenario_id/k$buckets"
        status=memory_limit
        first_exceeding_prefix_nodes=$(
          line_value "$result_line" first_exceeding_prefix_nodes
        )
        largest_accepted_prefix_nodes=$(
          line_value "$result_line" largest_accepted_prefix_nodes
        )
        columns=$(line_value "$result_line" first_exceeding_prefix_columns)
        dense_arena_bytes=$(
          line_value "$result_line" first_exceeding_dense_arena_bytes
        )
        is_positive_i64 "$first_exceeding_prefix_nodes" &&
          is_nonnegative_i64 "$largest_accepted_prefix_nodes" &&
          is_positive_i64 "$columns" &&
          is_positive_i64 "$dense_arena_bytes" &&
          (( first_exceeding_prefix_nodes ==
             largest_accepted_prefix_nodes + 1 )) &&
          (( first_exceeding_prefix_nodes <=
             EXPECTED_REPRESENTATION_NODE_LIMIT )) &&
          (( dense_arena_bytes > max_memory_bytes )) ||
          fail "typed MemoryLimit boundary mismatch for $scenario_id/k$buckets"
        resource_limit_source=dense_arena_estimate
        ;;
      node-checkpoint)
        [[ "$command_exit_code" == "0" ]] ||
          fail "typed node checkpoint exited $command_exit_code"
        [[ "$(line_value "$result_line" tree_error)" == "TooManyNodes" &&
           "$(line_value "$result_line" node_limit_kind)" == "representation" &&
           "$(line_value "$result_line" node_limit)" == \
             "$EXPECTED_REPRESENTATION_NODE_LIMIT" ]] ||
          fail "node checkpoint contract mismatch for $scenario_id/k$buckets"
        status=node_checkpoint
        exact_nodes=$(line_value "$result_line" enumerated_nodes)
        attempted_node=$(line_value "$result_line" attempted_node)
        is_positive_i64 "$exact_nodes" &&
          is_positive_i64 "$attempted_node" &&
          (( exact_nodes == EXPECTED_REPRESENTATION_NODE_LIMIT )) &&
          (( attempted_node == exact_nodes + 1 )) ||
          fail "node checkpoint boundary mismatch for $scenario_id/k$buckets"
        largest_accepted_prefix_nodes=$exact_nodes
        resource_limit_source=node_representation
        ;;
      *)
        fail "unsupported RESULT outcome $outcome"
        ;;
    esac
    for numeric in \
      "$exact_nodes" \
      "$first_exceeding_prefix_nodes" \
      "$largest_accepted_prefix_nodes" \
      "$columns" \
      "$dense_arena_bytes"; do
      [[ -z "$numeric" || "$numeric" =~ ^[0-9]+$ ]] ||
        fail "non-integer RESULT field for $scenario_id/k$buckets"
    done
  fi

  stdout_sha=$(sha256_file "$stdout_log")
  stderr_sha=$(sha256_file "$stderr_log")
  watchdog_metadata_sha=$(sha256_file "$watchdog_metadata")
  jq -n \
    --arg status "$status" \
    --arg outcome "$outcome" \
    --arg command_exit_code "$command_exit_code" \
    --arg child_exit_code "$child_exit_code" \
    --arg case_name "$case_name" \
    --arg scenario_id "$scenario_id" \
    --arg seats "$seats" \
    --arg stack_bb "$stack_bb" \
    --arg buckets "$buckets" \
    --arg max_memory "$max_memory_bytes" \
    --arg game_fingerprint "$game_fingerprint" \
    --arg tree_fingerprint "$tree_fingerprint" \
    --arg exact_nodes "$exact_nodes" \
    --arg first_exceeding_prefix_nodes "$first_exceeding_prefix_nodes" \
    --arg largest_accepted_prefix_nodes "$largest_accepted_prefix_nodes" \
    --arg columns "$columns" \
    --arg dense_arena_bytes "$dense_arena_bytes" \
    --arg preflight_wall "$wall_seconds" \
    --arg preflight_peak "$preflight_peak_rss_bytes" \
    --arg resource_limit_source "$resource_limit_source" \
    --arg segment_wall "$segment_wall_seconds" \
    --arg segment_peak "$segment_peak_rss_bytes" \
    --arg rss_limit "$rss_limit_bytes" \
    --arg rss_exceeded "$rss_limit_exceeded" \
    --arg rss_error "$rss_monitor_error" \
    --arg production_solver_cap "$PRODUCTION_SOLVER_CAP_BYTES" \
    --arg manifest "$manifest" \
    --arg manifest_sha "$manifest_sha" \
    --arg plan "$plan_csv" \
    --arg plan_sha "$plan_sha" \
    --arg plan_metadata "$plan_metadata" \
    --arg plan_metadata_sha "$plan_metadata_sha" \
    --arg preflight "$preflight" \
    --arg preflight_sha "$preflight_sha" \
    --arg watchdog "$watchdog" \
    --arg watchdog_sha "$watchdog_sha" \
    --arg runner "${BASH_SOURCE[0]}" \
    --arg runner_sha "$runner_sha" \
    --arg generator "$generator" \
    --arg generator_sha "$generator_sha" \
    --arg stdout "$stdout_log" \
    --arg stdout_sha "$stdout_sha" \
    --arg stderr "$stderr_log" \
    --arg stderr_sha "$stderr_sha" \
    --arg watchdog_metadata "$watchdog_metadata" \
    --arg watchdog_metadata_sha "$watchdog_metadata_sha" \
    --arg job_fingerprint "$job_fingerprint" '
      {
        schema: "solvers.abstraction-transfer-dense-preflight-run/v1",
        status: $status,
        outcome: $outcome,
        jobFingerprint: $job_fingerprint,
        identity: {
          case: $case_name,
          scenarioId: $scenario_id,
          seats: ($seats | tonumber),
          stackBb: ($stack_bb | tonumber),
          buckets: ($buckets | tonumber),
          maxMemoryBytes: ($max_memory | tonumber),
          gameFingerprint: $game_fingerprint,
          treeContractFingerprint: $tree_fingerprint
        },
        watchdogExitCode: ($command_exit_code | tonumber),
        childExitCode: ($child_exit_code | tonumber),
        result: {
          exactNodes:
            (if $exact_nodes == "" then null else ($exact_nodes | tonumber) end),
          firstExceedingPrefixNodes:
            (if $first_exceeding_prefix_nodes == "" then null
             else ($first_exceeding_prefix_nodes | tonumber) end),
          largestAcceptedPrefixNodes:
            (if $largest_accepted_prefix_nodes == "" then null
             else ($largest_accepted_prefix_nodes | tonumber) end),
          columns:
            (if $columns == "" then null else ($columns | tonumber) end),
          denseArenaBytes:
            (if $dense_arena_bytes == "" then null
             else ($dense_arena_bytes | tonumber) end),
          wallSeconds:
            (if $preflight_wall == "" then null
             else ($preflight_wall | tonumber) end),
          peakRssBytes:
            (if $preflight_peak == "" then null
             else ($preflight_peak | tonumber) end)
        },
        resources: {
          denseFeasibilityScope: "arena-payload-estimate-only",
          productionProcessFeasibilityEstablished: false,
          resourceLimitSource:
            (if $resource_limit_source == "none" then null
             else $resource_limit_source end),
          wallSeconds: ($segment_wall | tonumber),
          peakRssBytes: ($segment_peak | tonumber),
          rssLimitBytes: ($rss_limit | tonumber),
          rssScope: "count-only-preflight-process",
          rssLimitExceeded: ($rss_exceeded == "1"),
          rssMonitorError: ($rss_error == "1"),
          requiredProductionValidation: {
            kind: "materialized-solve",
            solverDenseArenaCapBytes: ($production_solver_cap | tonumber),
            processLimitBytes: ($max_memory | tonumber)
          }
        },
        inputs: {
          manifest: {path: $manifest, sha256: $manifest_sha},
          plan: {path: $plan, sha256: $plan_sha},
          planMetadata: {path: $plan_metadata, sha256: $plan_metadata_sha},
          preflight: {path: $preflight, sha256: $preflight_sha},
          watchdog: {path: $watchdog, sha256: $watchdog_sha},
          runner: {path: $runner, sha256: $runner_sha},
          generator: {path: $generator, sha256: $generator_sha}
        },
        artifacts: {
          stdout: {path: $stdout, sha256: $stdout_sha},
          stderr: {path: $stderr, sha256: $stderr_sha},
          watchdog: {
            path: $watchdog_metadata,
            sha256: $watchdog_metadata_sha
          }
        }
      }
    ' >"${job_meta}.tmp.$$"
  mv -f -- "${job_meta}.tmp.$$" "$job_meta"
  job_meta_sha=$(sha256_file "$job_meta")

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$case_name" \
    "$scenario_id" \
    "$seats" \
    "$stack_bb" \
    "$buckets" \
    "$status" \
    "$outcome" \
    "$exact_nodes" \
    "$first_exceeding_prefix_nodes" \
    "$largest_accepted_prefix_nodes" \
    "$columns" \
    "$dense_arena_bytes" \
    "$wall_seconds" \
    "$preflight_peak_rss_bytes" \
    "$command_exit_code" \
    "$child_exit_code" \
    "$segment_wall_seconds" \
    "$segment_peak_rss_bytes" \
    "$rss_limit_bytes" \
    "$rss_limit_exceeded" \
    "$rss_monitor_error" \
    "$resource_limit_source" \
    "$max_memory_bytes" \
    "arena-payload-estimate-only" \
    "not-established" \
    "$game_fingerprint" \
    "$tree_fingerprint" \
    "$manifest_sha" \
    "$plan_sha" \
    "$plan_metadata_sha" \
    "$preflight_sha" \
    "$watchdog_sha" \
    "$runner_sha" \
    "$generator_sha" \
    "$stdout_sha" \
    "$stderr_sha" \
    "$watchdog_metadata_sha" \
    "$job_fingerprint" \
    "$stdout_log" \
    "$watchdog_metadata" \
    "$job_meta" \
    "$job_meta_sha" \
    >>"$summary_work"
  publish_summary
  echo \
    "RESULT case=$case_name scenario=$scenario_id buckets=$buckets status=$status peak_rss=$segment_peak_rss_bytes"
done < <(tail -n +2 "$plan_csv")

(( jobs_seen == EXPECTED_JOB_COUNT )) ||
  fail "executed $jobs_seen jobs, expected $EXPECTED_JOB_COUNT"
if (( plan_only == 1 )); then
  echo "planned $jobs_seen serial dense preflight jobs"
else
  echo "wrote $summary_path"
fi

rmdir -- "$lock_dir"
lock_dir=
trap - EXIT INT TERM HUP
if [[ -n "$summary_work" ]]; then
  rm -f -- "$summary_work"
fi
summary_work=
if (( external_resource_failures > 0 )); then
  echo \
    "dense preflight runner: $external_resource_failures jobs hit the process RSS watchdog" \
    >&2
  exit 75
fi
