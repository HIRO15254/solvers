#!/usr/bin/env bash
set -euo pipefail

readonly MAX_RSS_LIMIT_BYTES=8589934592
readonly DEFAULT_RSS_LIMIT_BYTES=8053063680
readonly INDEX_HEADER='case,scenario_id,finalist_id,seats,stack_bb,abstraction_seed,solver_seed,evaluation_seed,canonical_config,canonical_config_fingerprint,compatibility_config,compatibility_config_fingerprint,game_fingerprint,tree_contract_fingerprint,abstraction_spec_fingerprint,cache'
readonly SUMMARY_HEADER='target_sweeps,case,scenario_id,finalist_id,seats,stack_bb,abstraction_seed,solver_seed,evaluation_seed_for_later_evaluation,status,result_status,sweeps,infosets,solver_memory_bytes,solver_elapsed_seconds,invocation_source,segment_mode,segment_exit_code,segment_wall_seconds,segment_peak_rss_bytes,rss_limit_bytes,rss_limit_exceeded,rss_monitor_error,resource_limit_source,manifest_sha256,metadata_sha256,index_sha256,solver_sha256,canonical_config_sha256,compatibility_config_sha256,expected_config_fingerprint,result_config_fingerprint,game_fingerprint,tree_contract_fingerprint,abstraction_spec_fingerprint,runtime_abstraction_fingerprint,configuration_fingerprint,result,checkpoint,progress,cache,job_meta'

usage() {
  cat >&2 <<'EOF'
usage:
  run-transfer-solves.sh TARGET_SWEEPS RESULT_ROOT [OPTIONS]

options:
  --manifest PATH          transfer finalist manifest
                           (default: this directory/manifest.toml)
  --solver-seed N          run only rows with this solver seed
  --case all|tournament|cash
                           run only this game class (default: all)
  --finalist-regex ERE     run only matching finalist IDs (default: .*)
  --rss-limit-bytes N      process RSS watchdog limit
                           (default: 8053063680; maximum: 8589934592)
  --solver PATH            solver executable override
  --generator PATH         transfer config generator executable override
  --watchdog PATH          RSS watchdog override
  --plan                   validate and print the serial job plan only
  -h, --help               show this help

The runner always regenerates and validates the fixed ten-case config set.
Completed jobs are reused only after their result, input SHA-256 values, and
generated fingerprints validate. Partial jobs resume from their checkpoint.
EOF
}

fail() {
  echo "transfer solve runner: $*" >&2
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

absolute_existing_file() {
  local path=$1
  local directory basename
  [[ -f "$path" ]] || return 1
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

sha256_file_or_missing() {
  if [[ -f "$1" ]]; then
    sha256_file "$1"
  else
    printf 'missing\n'
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

watchdog_value() {
  local metadata=$1
  local key=$2
  awk -F= -v wanted="$key" '$1 == wanted { print $2; found++ }
    END { if (found > 1) exit 3 }' "$metadata" 2>/dev/null || true
}

is_success_result_status() {
  case "$1" in
    completed|converged) return 0 ;;
    *) return 1 ;;
  esac
}

is_partial_result_status() {
  case "$1" in
    resource_limit|cancelled|sweep-limit|time-limit) return 0 ;;
    *) return 1 ;;
  esac
}

validate_result() {
  local result_path=$1
  local expected_sweeps=$2
  local expected_seats=$3
  local expected_config_fingerprint=$4
  local expected_game_fingerprint=$5

  jq -e \
    --argjson target "$expected_sweeps" \
    --argjson seats "$expected_seats" \
    --arg config "$expected_config_fingerprint" \
    --arg game "$expected_game_fingerprint" '
      def nonnegative_integer:
        type == "number" and . >= 0 and floor == .;
      def finite_nonnegative:
        type == "number" and . >= 0;
      .schemaVersion == 3
      and .kind == "preflop-multiway"
      and (.status
        | . == "completed"
          or . == "converged"
          or . == "resource_limit"
          or . == "cancelled"
          or . == "sweep-limit"
          or . == "time-limit")
      and (.sweeps | nonnegative_integer)
      and .sweeps <= $target
      and (.infosets | nonnegative_integer)
      and (.memoryBytes | nonnegative_integer)
      and (.elapsedSecs | finite_nonnegative)
      and (.seats | type == "array" and length == $seats)
      and .configHash == $config
      and .gameFingerprint == $game
      and (.abstractionFingerprint
        | type == "string" and test("^[0-9a-f]{64}$"))
      and (.configurationFingerprint
        | type == "string" and test("^[0-9a-f]{64}$"))
    ' "$result_path" >/dev/null
}

write_segment_meta() {
  local destination=$1
  local segment_status=$2
  local command_exit_code=$3
  local mode=$4
  local resume_from_sweeps=$5
  local result_sha=$6
  local checkpoint_sha=$7
  local progress_sha=$8
  local cache_input_sha=$9
  shift 9
  local cache_output_sha=$1
  local wall_seconds=$2
  local peak_rss_bytes=$3
  local rss_limit_exceeded=$4
  local rss_monitor_error=$5
  local resource_limit_source=$6
  local watchdog_metadata=$7
  local stdout_log=$8
  local stderr_log=$9

  local temporary="${destination}.tmp.$$"
  jq -n \
    --arg status "$segment_status" \
    --arg command_exit_code "$command_exit_code" \
    --arg mode "$mode" \
    --arg resume_from_sweeps "$resume_from_sweeps" \
    --arg target_sweeps "$target_sweeps" \
    --arg manifest "$manifest" \
    --arg manifest_sha "$manifest_sha" \
    --arg metadata "$experiment_metadata" \
    --arg metadata_sha "$metadata_sha" \
    --arg index "$index_path" \
    --arg index_sha "$index_sha" \
    --arg solver "$solver" \
    --arg solver_sha "$solver_sha" \
    --arg canonical_config "$canonical_config" \
    --arg canonical_sha "$canonical_sha" \
    --arg canonical_fingerprint "$canonical_fingerprint" \
    --arg compatibility_config "$compatibility_config" \
    --arg compatibility_sha "$compatibility_sha" \
    --arg compatibility_fingerprint "$compatibility_fingerprint" \
    --arg game_fingerprint "$game_fingerprint" \
    --arg tree_fingerprint "$tree_fingerprint" \
    --arg abstraction_spec_fingerprint "$abstraction_spec_fingerprint" \
    --arg result "$result_json" \
    --arg result_sha "$result_sha" \
    --arg checkpoint "$checkpoint" \
    --arg checkpoint_sha "$checkpoint_sha" \
    --arg progress "$progress_jsonl" \
    --arg progress_sha "$progress_sha" \
    --arg cache "$cache_path" \
    --arg cache_input_sha "$cache_input_sha" \
    --arg cache_output_sha "$cache_output_sha" \
    --arg watchdog_metadata "$watchdog_metadata" \
    --arg stdout_log "$stdout_log" \
    --arg stderr_log "$stderr_log" \
    --arg wall_seconds "$wall_seconds" \
    --arg peak_rss_bytes "$peak_rss_bytes" \
    --arg rss_limit_bytes "$rss_limit_bytes" \
    --arg rss_limit_exceeded "$rss_limit_exceeded" \
    --arg rss_monitor_error "$rss_monitor_error" \
    --arg resource_limit_source "$resource_limit_source" '
      {
        schema: "solvers.abstraction-transfer-solve-segment/v1",
        status: $status,
        commandExitCode: ($command_exit_code | tonumber),
        mode: $mode,
        resumeFromSweeps:
          (if $resume_from_sweeps == "" then null
           else ($resume_from_sweeps | tonumber) end),
        targetSweeps: ($target_sweeps | tonumber),
        inputs: {
          manifest: {path: $manifest, sha256: $manifest_sha},
          transferMetadata: {path: $metadata, sha256: $metadata_sha},
          configIndex: {path: $index, sha256: $index_sha},
          solver: {path: $solver, sha256: $solver_sha},
          canonicalConfig: {
            path: $canonical_config,
            sha256: $canonical_sha,
            configFingerprint: $canonical_fingerprint
          },
          compatibilityConfig: {
            path: $compatibility_config,
            sha256: $compatibility_sha,
            configFingerprint: $compatibility_fingerprint
          }
        },
        expectedFingerprints: {
          game: $game_fingerprint,
          treeContract: $tree_fingerprint,
          abstractionSpec: $abstraction_spec_fingerprint
        },
        artifacts: {
          result: {
            path: $result,
            sha256: (if $result_sha == "missing" then null else $result_sha end)
          },
          checkpoint: {
            path: $checkpoint,
            sha256:
              (if $checkpoint_sha == "missing" then null else $checkpoint_sha end)
          },
          progress: {
            path: $progress,
            sha256: (if $progress_sha == "missing" then null else $progress_sha end)
          },
          cache: {
            path: $cache,
            inputSha256: $cache_input_sha,
            outputSha256: $cache_output_sha
          },
          watchdogMetadata: $watchdog_metadata,
          stdoutLog: $stdout_log,
          stderrLog: $stderr_log
        },
        resources: {
          wallSeconds:
            (if $wall_seconds == "" then null else ($wall_seconds | tonumber) end),
          peakRssBytes:
            (if $peak_rss_bytes == "" then null
             else ($peak_rss_bytes | tonumber) end),
          rssLimitBytes: ($rss_limit_bytes | tonumber),
          rssLimitExceeded: ($rss_limit_exceeded == "1"),
          rssMonitorError: ($rss_monitor_error == "1"),
          resourceLimitSource:
            (if $resource_limit_source == "none" then null
             else $resource_limit_source end)
        }
      }
    ' >"$temporary"
  mv -f -- "$temporary" "$destination"
}

write_job_meta() {
  local destination=$1
  local normalized_status=$2
  local invocation_source=$3
  local latest_segment=$4
  local resource_limit_source=$5

  local result_sha checkpoint_sha progress_sha cache_sha
  local result_status_value result_sweeps result_infosets result_memory
  local result_elapsed result_config result_abstraction result_configuration
  local latest_mode latest_exit latest_wall latest_peak latest_exceeded latest_monitor

  result_sha=$(sha256_file_or_missing "$result_json")
  checkpoint_sha=$(sha256_file_or_missing "$checkpoint")
  progress_sha=$(sha256_file_or_missing "$progress_jsonl")
  cache_sha=$(sha256_file_or_missing "$cache_path")
  result_status_value=
  result_sweeps=
  result_infosets=
  result_memory=
  result_elapsed=
  result_config=
  result_abstraction=
  result_configuration=
  if [[ -f "$result_json" ]]; then
    result_status_value=$(jq -r '.status' "$result_json")
    result_sweeps=$(jq -r '.sweeps' "$result_json")
    result_infosets=$(jq -r '.infosets' "$result_json")
    result_memory=$(jq -r '.memoryBytes' "$result_json")
    result_elapsed=$(jq -r '.elapsedSecs' "$result_json")
    result_config=$(jq -r '.configHash' "$result_json")
    result_abstraction=$(jq -r '.abstractionFingerprint' "$result_json")
    result_configuration=$(jq -r '.configurationFingerprint' "$result_json")
  fi

  latest_mode=
  latest_exit=
  latest_wall=
  latest_peak=
  latest_exceeded=
  latest_monitor=
  if [[ -n "$latest_segment" && -f "$latest_segment" ]]; then
    latest_mode=$(jq -r '.mode // empty' "$latest_segment")
    latest_exit=$(jq -r '.commandExitCode // empty' "$latest_segment")
    latest_wall=$(jq -r '.resources.wallSeconds // empty' "$latest_segment")
    latest_peak=$(jq -r '.resources.peakRssBytes // empty' "$latest_segment")
    latest_exceeded=$(
      jq -r 'if .resources.rssLimitExceeded then "1" else "0" end' "$latest_segment"
    )
    latest_monitor=$(
      jq -r 'if .resources.rssMonitorError then "1" else "0" end' "$latest_segment"
    )
  fi

  local temporary="${destination}.tmp.$$"
  jq -n \
    --arg job_fingerprint "$job_fingerprint" \
    --arg status "$normalized_status" \
    --arg invocation_source "$invocation_source" \
    --arg target_sweeps "$target_sweeps" \
    --arg case_name "$case_name" \
    --arg scenario_id "$scenario_id" \
    --arg finalist_id "$finalist_id" \
    --arg seats "$seats" \
    --arg stack_bb "$stack_bb" \
    --arg abstraction_seed "$abstraction_seed" \
    --arg solver_seed "$row_solver_seed" \
    --arg evaluation_seed "$evaluation_seed" \
    --arg manifest "$manifest" \
    --arg manifest_sha "$manifest_sha" \
    --arg manifest_fingerprint "$manifest_fingerprint" \
    --arg metadata "$experiment_metadata" \
    --arg metadata_sha "$metadata_sha" \
    --arg index "$index_path" \
    --arg index_sha "$index_sha" \
    --arg solver "$solver" \
    --arg solver_sha "$solver_sha" \
    --arg generator "$generator" \
    --arg generator_sha "$generator_sha" \
    --arg watchdog "$watchdog" \
    --arg watchdog_sha "$watchdog_sha" \
    --arg canonical_config "$canonical_config" \
    --arg canonical_sha "$canonical_sha" \
    --arg canonical_fingerprint "$canonical_fingerprint" \
    --arg compatibility_config "$compatibility_config" \
    --arg compatibility_sha "$compatibility_sha" \
    --arg compatibility_fingerprint "$compatibility_fingerprint" \
    --arg game_fingerprint "$game_fingerprint" \
    --arg tree_fingerprint "$tree_fingerprint" \
    --arg abstraction_spec_fingerprint "$abstraction_spec_fingerprint" \
    --arg result "$result_json" \
    --arg result_sha "$result_sha" \
    --arg checkpoint "$checkpoint" \
    --arg checkpoint_sha "$checkpoint_sha" \
    --arg progress "$progress_jsonl" \
    --arg progress_sha "$progress_sha" \
    --arg cache "$cache_path" \
    --arg cache_sha "$cache_sha" \
    --arg latest_segment "$latest_segment" \
    --arg latest_mode "$latest_mode" \
    --arg latest_exit "$latest_exit" \
    --arg latest_wall "$latest_wall" \
    --arg latest_peak "$latest_peak" \
    --arg latest_exceeded "$latest_exceeded" \
    --arg latest_monitor "$latest_monitor" \
    --arg rss_limit_bytes "$rss_limit_bytes" \
    --arg resource_limit_source "$resource_limit_source" \
    --arg result_status "$result_status_value" \
    --arg result_sweeps "$result_sweeps" \
    --arg result_infosets "$result_infosets" \
    --arg result_memory "$result_memory" \
    --arg result_elapsed "$result_elapsed" \
    --arg result_config "$result_config" \
    --arg result_abstraction "$result_abstraction" \
    --arg result_configuration "$result_configuration" '
      {
        schema: "solvers.abstraction-transfer-solve-run/v1",
        jobFingerprint: $job_fingerprint,
        status: $status,
        lastInvocationSource: $invocation_source,
        selection: {
          targetSweeps: ($target_sweeps | tonumber),
          case: $case_name,
          scenarioId: $scenario_id,
          finalistId: $finalist_id,
          seats: ($seats | tonumber),
          stackBb: ($stack_bb | tonumber),
          abstractionSeed:
            (if $abstraction_seed == "" then null
             else ($abstraction_seed | tonumber) end),
          solverSeed: ($solver_seed | tonumber),
          evaluationSeedForLaterEvaluation: ($evaluation_seed | tonumber)
        },
        inputs: {
          manifest: {
            path: $manifest,
            sha256: $manifest_sha,
            generatorFingerprint: $manifest_fingerprint
          },
          transferMetadata: {path: $metadata, sha256: $metadata_sha},
          configIndex: {path: $index, sha256: $index_sha},
          solver: {path: $solver, sha256: $solver_sha},
          generator: {path: $generator, sha256: $generator_sha},
          watchdog: {path: $watchdog, sha256: $watchdog_sha},
          canonicalConfig: {
            path: $canonical_config,
            sha256: $canonical_sha,
            configFingerprint: $canonical_fingerprint
          },
          compatibilityConfig: {
            path: $compatibility_config,
            sha256: $compatibility_sha,
            configFingerprint: $compatibility_fingerprint
          }
        },
        expectedFingerprints: {
          game: $game_fingerprint,
          treeContract: $tree_fingerprint,
          abstractionSpec: $abstraction_spec_fingerprint
        },
        artifacts: {
          result: {
            path: $result,
            sha256: (if $result_sha == "missing" then null else $result_sha end)
          },
          checkpoint: {
            path: $checkpoint,
            sha256:
              (if $checkpoint_sha == "missing" then null else $checkpoint_sha end)
          },
          progress: {
            path: $progress,
            sha256: (if $progress_sha == "missing" then null else $progress_sha end)
          },
          cache: {
            path: $cache,
            sha256: (if $cache_sha == "missing" then null else $cache_sha end)
          },
          latestSegment:
            (if $latest_segment == "" then null else $latest_segment end)
        },
        result: {
          status:
            (if $result_status == "" then null else $result_status end),
          sweeps:
            (if $result_sweeps == "" then null else ($result_sweeps | tonumber) end),
          infosets:
            (if $result_infosets == "" then null else ($result_infosets | tonumber) end),
          solverMemoryBytes:
            (if $result_memory == "" then null else ($result_memory | tonumber) end),
          solverElapsedSeconds:
            (if $result_elapsed == "" then null else ($result_elapsed | tonumber) end),
          configFingerprint:
            (if $result_config == "" then null else $result_config end),
          gameFingerprint:
            (if $result_status == "" then null else $game_fingerprint end),
          abstractionFingerprint:
            (if $result_abstraction == "" then null else $result_abstraction end),
          configurationFingerprint:
            (if $result_configuration == "" then null
             else $result_configuration end)
        },
        resources: {
          rssLimitBytes: ($rss_limit_bytes | tonumber),
          resourceLimitSource:
            (if $resource_limit_source == "none" then null
             else $resource_limit_source end),
          latestSegment: {
            mode: (if $latest_mode == "" then null else $latest_mode end),
            commandExitCode:
              (if $latest_exit == "" then null else ($latest_exit | tonumber) end),
            wallSeconds:
              (if $latest_wall == "" then null else ($latest_wall | tonumber) end),
            peakRssBytes:
              (if $latest_peak == "" then null else ($latest_peak | tonumber) end),
            rssLimitExceeded: ($latest_exceeded == "1"),
            rssMonitorError: ($latest_monitor == "1")
          }
        }
      }
    ' >"$temporary"
  mv -f -- "$temporary" "$destination"
}

append_summary_row() {
  local summary=$1
  local metadata=$2
  jq -r '
    [
      .selection.targetSweeps,
      .selection.case,
      .selection.scenarioId,
      .selection.finalistId,
      .selection.seats,
      .selection.stackBb,
      .selection.abstractionSeed,
      .selection.solverSeed,
      .selection.evaluationSeedForLaterEvaluation,
      .status,
      .result.status,
      .result.sweeps,
      .result.infosets,
      .result.solverMemoryBytes,
      .result.solverElapsedSeconds,
      .lastInvocationSource,
      .resources.latestSegment.mode,
      .resources.latestSegment.commandExitCode,
      .resources.latestSegment.wallSeconds,
      .resources.latestSegment.peakRssBytes,
      .resources.rssLimitBytes,
      .resources.latestSegment.rssLimitExceeded,
      .resources.latestSegment.rssMonitorError,
      .resources.resourceLimitSource,
      .inputs.manifest.sha256,
      .inputs.transferMetadata.sha256,
      .inputs.configIndex.sha256,
      .inputs.solver.sha256,
      .inputs.canonicalConfig.sha256,
      .inputs.compatibilityConfig.sha256,
      .inputs.compatibilityConfig.configFingerprint,
      .result.configFingerprint,
      .expectedFingerprints.game,
      .expectedFingerprints.treeContract,
      .expectedFingerprints.abstractionSpec,
      .result.abstractionFingerprint,
      .result.configurationFingerprint,
      .artifacts.result.path,
      .artifacts.checkpoint.path,
      .artifacts.progress.path,
      .artifacts.cache.path,
      input_filename
    ] | @csv
  ' "$metadata" >>"$summary"
}

if (( $# < 2 )); then
  usage
  exit 2
fi

target_sweeps=$1
result_root=$2
shift 2

workspace=$(cd "$(dirname "$0")/../.." && pwd -P)
manifest="$workspace/experiments/abstraction-transfer-2026-07-25/manifest.toml"
research_target_dir="$workspace/target/research-release"
solver="$research_target_dir/release/solvers"
generator="$research_target_dir/release/examples/generate_abstraction_transfer_configs"
watchdog="$workspace/experiments/abstraction-optimization-2026-07-25/run-with-rss-watchdog.sh"
solver_seed_filter=
case_filter=all
finalist_regex='.*'
rss_limit_bytes=$DEFAULT_RSS_LIMIT_BYTES
plan_only=0
solver_overridden=0
generator_overridden=0

while (( $# > 0 )); do
  case "$1" in
    --manifest)
      (( $# >= 2 )) || fail "--manifest requires a path"
      manifest=$2
      shift 2
      ;;
    --solver-seed)
      (( $# >= 2 )) || fail "--solver-seed requires a value"
      solver_seed_filter=$2
      shift 2
      ;;
    --case)
      (( $# >= 2 )) || fail "--case requires a value"
      case_filter=$2
      shift 2
      ;;
    --finalist-regex)
      (( $# >= 2 )) || fail "--finalist-regex requires a value"
      finalist_regex=$2
      shift 2
      ;;
    --rss-limit-bytes)
      (( $# >= 2 )) || fail "--rss-limit-bytes requires a value"
      rss_limit_bytes=$2
      shift 2
      ;;
    --solver)
      (( $# >= 2 )) || fail "--solver requires a path"
      solver=$2
      solver_overridden=1
      shift 2
      ;;
    --generator)
      (( $# >= 2 )) || fail "--generator requires a path"
      generator=$2
      generator_overridden=1
      shift 2
      ;;
    --watchdog)
      (( $# >= 2 )) || fail "--watchdog requires a path"
      watchdog=$2
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

for command in jq awk ps tail cargo; do
  command -v "$command" >/dev/null 2>&1 ||
    fail "required command is unavailable: $command"
done
is_positive_i64 "$target_sweeps" || fail "TARGET_SWEEPS must be a positive signed 64-bit integer"
if [[ -n "$solver_seed_filter" ]]; then
  is_nonnegative_i64 "$solver_seed_filter" ||
    fail "--solver-seed must be a nonnegative signed 64-bit integer"
fi
is_positive_i64 "$rss_limit_bytes" ||
  fail "--rss-limit-bytes must be a positive signed 64-bit integer"
(( rss_limit_bytes <= MAX_RSS_LIMIT_BYTES )) ||
  fail "RSS limit exceeds the 8 GiB process ceiling"
case "$case_filter" in
  all|tournament|cash) ;;
  *) fail "--case must be all, tournament, or cash" ;;
esac
if [[ "$finalist_regex" == *$'\n'* || "$finalist_regex" == *$'\r'* ]]; then
  fail "--finalist-regex must not contain a newline"
fi
set +e
[[ "" =~ $finalist_regex ]]
regex_status=$?
set -e
(( regex_status != 2 )) || fail "invalid --finalist-regex: $finalist_regex"

manifest=$(absolute_existing_file "$manifest") || fail "missing manifest: $manifest"
watchdog=$(absolute_existing_file "$watchdog") || fail "missing watchdog: $watchdog"
[[ -x "$watchdog" ]] || fail "RSS watchdog is not executable: $watchdog"

mkdir -p "$result_root"
result_root=$(cd "$result_root" && pwd -P)
if [[ "$result_root" == *','* || "$result_root" == *$'\n'* || "$result_root" == *$'\r'* ]]; then
  fail "RESULT_ROOT may not contain commas or newlines"
fi

lock_dir="$result_root/.transfer-solve.lock"
if ! mkdir "$lock_dir" 2>/dev/null; then
  fail "another transfer runner owns $lock_dir; transfer solves must remain serial"
fi
summary_tmp=
scratch_dir=
active_next_result=
active_watchdog_pid=
forwarded_runner_signal=none
forwarded_runner_exit_code=
forwarded_watchdog_status=
cleanup_runner() {
  if [[ -n "${active_watchdog_pid:-}" ]] &&
    kill -0 "$active_watchdog_pid" 2>/dev/null; then
    kill -TERM "$active_watchdog_pid" 2>/dev/null || true
    set +e
    wait "$active_watchdog_pid"
    set -e
  fi
  if [[ -n "${active_next_result:-}" && -f "$active_next_result" ]]; then
    rm -f -- "$active_next_result"
  fi
  if [[ -n "${summary_tmp:-}" && -f "$summary_tmp" ]]; then
    rm -f -- "$summary_tmp"
  fi
  if [[ -n "${scratch_dir:-}" &&
        -d "$scratch_dir" &&
        "$scratch_dir" == "$result_root"/.transfer-runner.* ]]; then
    rm -rf -- "$scratch_dir"
  fi
  if [[ -n "${lock_dir:-}" && -d "$lock_dir" ]]; then
    rmdir -- "$lock_dir" 2>/dev/null || true
  fi
}
forward_runner_signal() {
  local signal=$1
  local exit_code=$2
  if [[ -n "${active_watchdog_pid:-}" ]] &&
    kill -0 "$active_watchdog_pid" 2>/dev/null; then
    trap '' "$signal"
    forwarded_runner_signal=$signal
    forwarded_runner_exit_code=$exit_code
    kill "-$signal" "$active_watchdog_pid" 2>/dev/null || true
    if wait "$active_watchdog_pid"; then
      forwarded_watchdog_status=0
    else
      forwarded_watchdog_status=$?
    fi
    return
  fi
  exit "$exit_code"
}
trap cleanup_runner EXIT
trap 'forward_runner_signal INT 130' INT
trap 'forward_runner_signal TERM 143' TERM
trap 'forward_runner_signal HUP 129' HUP

if (( solver_overridden == 0 || generator_overridden == 0 )); then
  (
    cd "$workspace"
    CARGO_TARGET_DIR="$research_target_dir" \
      cargo build --quiet --locked --release -p cli --features research --bin solvers \
      --example generate_abstraction_transfer_configs
  )
fi
solver=$(absolute_existing_file "$solver") || fail "missing solver: $solver"
generator=$(absolute_existing_file "$generator") || fail "missing generator: $generator"
[[ -x "$solver" ]] || fail "solver is not executable: $solver"
[[ -x "$generator" ]] || fail "config generator is not executable: $generator"

config_dir="$result_root/configs"
cache_dir="$result_root/cache"
mkdir -p "$config_dir" "$cache_dir"
[[ ! -L "$config_dir" && ! -L "$cache_dir" ]] ||
  fail "generated config/cache directories must not be symbolic links"
manifest_sha=$(sha256_file "$manifest")
"$generator" "$manifest" "$config_dir" "$cache_dir"
[[ "$(sha256_file "$manifest")" == "$manifest_sha" ]] ||
  fail "manifest changed while configs were generated"

index_path="$config_dir/transfer-configs.csv"
experiment_metadata="$config_dir/transfer-metadata.json"
[[ -f "$index_path" ]] || fail "generator did not produce $index_path"
[[ -f "$experiment_metadata" ]] ||
  fail "generator did not produce $experiment_metadata"
[[ ! -L "$index_path" && ! -L "$experiment_metadata" ]] ||
  fail "generated index/metadata must not be symbolic links"
IFS= read -r actual_header <"$index_path"
[[ "$actual_header" == "$INDEX_HEADER" ]] ||
  fail "unsupported transfer config index header: $actual_header"
[[ "$(tail -n +2 "$index_path" | wc -l | tr -d '[:space:]')" == "10" ]] ||
  fail "transfer config index must contain exactly ten rows"

if ! jq -e \
  --arg manifest "$manifest" '
    . as $root
    | .schema == "solvers.abstraction-transfer-config-set/v1"
    and .manifestPath == $manifest
    and (.manifestFingerprint
      | type == "string" and test("^[0-9a-f]{64}$"))
    and (.fixedEnvelope | length == 10)
    and (.configs | length == 10)
    and ([.configs[].scenarioId] | unique | length == 10)
    and ([.configs[].case] | unique | sort == ["cash", "tournament"])
    and (.finalists | length == 2)
    and ([.finalists[].case] | sort == ["cash", "tournament"])
    and (all(.configs[];
      if .case == "tournament"
      then .treeContractFingerprint
        == "97e3c3ebf3da73e7f6e218bffd2b43399f87ab3a207b099f90de63058b8db604"
      else .treeContractFingerprint
        == "bb50096add4d3a554dec23b3eaaba35a15c26e121eaa77c4c5b0d402b1a6b821"
      end))
    and (
      [.configs | group_by(.case)[] | [.[].abstractionSpecFingerprint] | unique | length]
      == [1, 1]
    )
    and (all(.configs[];
      . as $config
      | any($root.fixedEnvelope[];
        .id == $config.scenarioId
        and .case == $config.case
        and .seats == $config.seats
        and .stackBb == $config.stackBb
        and .expectedGameFingerprint == $config.gameFingerprint)))
    and (.run.max_sweeps | type == "number" and . > 0 and floor == .)
    and (.run.check_every_sweeps | type == "number" and . > 0 and floor == .)
    and (.run.memory | type == "string" and length > 0)
    and (
      [.fixedEnvelope[] |
        [.id, .case, .seats, .stackBb, .expectedGameFingerprint]]
      == [
        ["tournament-6max-5bb", "tournament", 6, 5,
          "02f0179b06ec036f79c6b0141e67a65bfb9a71f73557203db210516bb2bba63c"],
        ["tournament-6max-50bb", "tournament", 6, 50,
          "a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512"],
        ["tournament-8max-20bb", "tournament", 8, 20,
          "c357fb191000573a575708cfee241722ab201c8f1e584b8783ac299da4258e9f"],
        ["tournament-9max-5bb", "tournament", 9, 5,
          "0908aa79db3d1149e055d3a25bb1e6ab25fd5e7fb956cbe7cb06e4e8ab5c8b5f"],
        ["tournament-9max-50bb", "tournament", 9, 50,
          "39e87301e3169b897cfb59b9be452be7360ba13c574ad8a5d0a03bb8013c2282"],
        ["cash-6max-100bb", "cash", 6, 100,
          "c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b"],
        ["cash-6max-800bb", "cash", 6, 800,
          "9d88e6644bccff8e5bde62ec9c8a480b0b344b60ac2ffeaba7140e201dfd3ce5"],
        ["cash-8max-400bb", "cash", 8, 400,
          "b40006617d4fc8c8129699ffeba92cbe71306975127fbcfa76264a4a9ba1e843"],
        ["cash-9max-100bb", "cash", 9, 100,
          "488aa4a6817cfb4d918ae033a5de1432f5484c1117afbad044bccd4cb002cbfa"],
        ["cash-9max-800bb", "cash", 9, 800,
          "e28ba6bd1205332cbb98291772bf3d8b47925264bb89d384ad81e98ddb92cb20"]
      ]
    )
  ' "$experiment_metadata" >/dev/null; then
  fail "generated transfer metadata violates the fixed-envelope contract"
fi

manifest_max_sweeps=$(jq -er '.run.max_sweeps' "$experiment_metadata")
manifest_check_every=$(jq -er '.run.check_every_sweeps' "$experiment_metadata")
[[ "$target_sweeps" == "$manifest_max_sweeps" ]] ||
  fail "TARGET_SWEEPS must exactly match manifest run.max_sweeps=$manifest_max_sweeps"
[[ "$manifest_check_every" == "$manifest_max_sweeps" ]] ||
  fail \
    "transfer runs require check_every_sweeps == max_sweeps; otherwise the inherited stop rule can terminate before the requested transfer budget"

manifest_fingerprint=$(jq -er '.manifestFingerprint' "$experiment_metadata")
metadata_sha=$(sha256_file "$experiment_metadata")
index_sha=$(sha256_file "$index_path")
solver_sha=$(sha256_file "$solver")
watchdog_sha=$(sha256_file "$watchdog")
generator_sha=$(sha256_file "$generator")

scratch_dir=$(mktemp -d "$result_root/.transfer-runner.XXXXXX")
selected_rows="$scratch_dir/selected.csv"
: >"$selected_rows"
selected_count=0
validated_count=0

while IFS=, read -r \
  case_name \
  scenario_id \
  finalist_id \
  seats \
  stack_bb \
  abstraction_seed \
  row_solver_seed \
  evaluation_seed \
  canonical_config \
  canonical_fingerprint \
  compatibility_config \
  compatibility_fingerprint \
  game_fingerprint \
  tree_fingerprint \
  abstraction_spec_fingerprint \
  cache_path; do
  validated_count=$((validated_count + 1))
  case "$case_name" in
    tournament|cash) ;;
    *) fail "invalid case in transfer config index: $case_name" ;;
  esac
  [[ "$scenario_id" =~ ^[[:alnum:]_.-]+$ ]] ||
    fail "unsafe scenario ID in transfer config index: $scenario_id"
  [[ "$finalist_id" =~ ^[[:alnum:]_.-]+$ ]] ||
    fail "unsafe finalist ID in transfer config index: $finalist_id"
  is_positive_i64 "$seats" || fail "invalid seat count for $scenario_id"
  is_nonnegative_i64 "$row_solver_seed" ||
    fail "invalid solver seed for $scenario_id"
  is_nonnegative_i64 "$evaluation_seed" ||
    fail "invalid evaluation seed for $scenario_id"
  if [[ -n "$abstraction_seed" ]]; then
    is_nonnegative_i64 "$abstraction_seed" ||
      fail "invalid abstraction seed for $scenario_id"
  fi
  [[ "$stack_bb" =~ ^[0-9]+([.][0-9]+)?$ ]] ||
    fail "invalid stack for $scenario_id"
  for fingerprint in \
    "$canonical_fingerprint" \
    "$compatibility_fingerprint" \
    "$game_fingerprint" \
    "$tree_fingerprint" \
    "$abstraction_spec_fingerprint"; do
    [[ "$fingerprint" =~ ^[0-9a-f]{64}$ ]] ||
      fail "invalid fingerprint in transfer config index for $scenario_id"
  done
  for path in "$canonical_config" "$compatibility_config"; do
    [[ "$path" == /* && -f "$path" ]] ||
      fail "missing absolute generated config for $scenario_id: $path"
    [[ "$path" == "$config_dir"/* && ! -L "$path" ]] ||
      fail "generated config escapes config_dir or is a symlink: $path"
  done
  [[ "$cache_path" == "$cache_dir"/* && ! -L "$cache_path" ]] ||
    fail "cache path for $scenario_id escapes cache_dir or is a symlink: $cache_path"

  if ! jq -e \
    --arg case_name "$case_name" \
    --arg scenario_id "$scenario_id" \
    --arg finalist_id "$finalist_id" \
    --arg seats "$seats" \
    --arg stack_bb "$stack_bb" \
    --arg abstraction_seed "$abstraction_seed" \
    --arg solver_seed "$row_solver_seed" \
    --arg evaluation_seed "$evaluation_seed" \
    --arg canonical_config "$canonical_config" \
    --arg canonical_fingerprint "$canonical_fingerprint" \
    --arg compatibility_config "$compatibility_config" \
    --arg compatibility_fingerprint "$compatibility_fingerprint" \
    --arg game_fingerprint "$game_fingerprint" \
    --arg tree_fingerprint "$tree_fingerprint" \
    --arg abstraction_spec_fingerprint "$abstraction_spec_fingerprint" \
    --arg cache "$cache_path" '
      [
        .configs[]
        | select(
            .case == $case_name
            and .scenarioId == $scenario_id
            and .finalistId == $finalist_id
            and .seats == ($seats | tonumber)
            and .stackBb == ($stack_bb | tonumber)
            and (
              ($abstraction_seed == "" and .abstractionSeed == null)
              or .abstractionSeed == ($abstraction_seed | tonumber)
            )
            and .solverSeed == ($solver_seed | tonumber)
            and .evaluationSeed == ($evaluation_seed | tonumber)
            and .canonicalConfig == $canonical_config
            and .canonicalConfigFingerprint == $canonical_fingerprint
            and .compatibilityConfig == $compatibility_config
            and .compatibilityConfigFingerprint == $compatibility_fingerprint
            and .gameFingerprint == $game_fingerprint
            and .treeContractFingerprint == $tree_fingerprint
            and .abstractionSpecFingerprint == $abstraction_spec_fingerprint
            and .cache == $cache
          )
      ] | length == 1
    ' "$experiment_metadata" >/dev/null; then
    fail "CSV/metadata mismatch for transfer scenario $scenario_id"
  fi

  canonical_sha=$(sha256_file "$canonical_config")
  compatibility_sha=$(sha256_file "$compatibility_config")
  if [[ "$case_filter" != "all" && "$case_name" != "$case_filter" ]]; then
    continue
  fi
  if [[ -n "$solver_seed_filter" &&
        "$row_solver_seed" != "$solver_seed_filter" ]]; then
    continue
  fi
  if ! [[ "$finalist_id" =~ $finalist_regex ]]; then
    continue
  fi
  selected_count=$((selected_count + 1))
  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$case_name" \
    "$scenario_id" \
    "$finalist_id" \
    "$seats" \
    "$stack_bb" \
    "$abstraction_seed" \
    "$row_solver_seed" \
    "$evaluation_seed" \
    "$canonical_config" \
    "$canonical_fingerprint" \
    "$compatibility_config" \
    "$compatibility_fingerprint" \
    "$game_fingerprint" \
    "$tree_fingerprint" \
    "$abstraction_spec_fingerprint" \
    "$cache_path" \
    "$canonical_sha" \
    "$compatibility_sha" \
    >>"$selected_rows"
done < <(tail -n +2 "$index_path")

(( validated_count == 10 )) ||
  fail "validated $validated_count transfer config rows, expected ten"
(( selected_count > 0 )) ||
  fail "no transfer scenarios matched the seed/case/finalist filters"

filter_material=$(
  printf '%s\n' \
    "target_sweeps=$target_sweeps" \
    "case=$case_filter" \
    "solver_seed=${solver_seed_filter:-all}" \
    "finalist_regex=$finalist_regex"
)
filter_fingerprint=$(printf '%s' "$filter_material" | hash_material)
summary_path="$result_root/transfer-s${target_sweeps}-${case_filter}-seed${solver_seed_filter:-all}-f${filter_fingerprint:0:12}-solve-summary.csv"

if (( plan_only == 0 )); then
  summary_tmp="${summary_path}.tmp.$$"
  printf '%s\n' "$SUMMARY_HEADER" >"$summary_tmp"
fi

jobs_seen=0
while IFS=, read -r \
  case_name \
  scenario_id \
  finalist_id \
  seats \
  stack_bb \
  abstraction_seed \
  row_solver_seed \
  evaluation_seed \
  canonical_config \
  canonical_fingerprint \
  compatibility_config \
  compatibility_fingerprint \
  game_fingerprint \
  tree_fingerprint \
  abstraction_spec_fingerprint \
  cache_path \
  canonical_sha \
  compatibility_sha; do
  jobs_seen=$((jobs_seen + 1))
  job_material=$(
    printf '%s\n' \
      'solvers.abstraction-transfer-solve-job/v1' \
      "manifest_sha256=$manifest_sha" \
      "manifest_fingerprint=$manifest_fingerprint" \
      "metadata_sha256=$metadata_sha" \
      "index_sha256=$index_sha" \
      "solver_sha256=$solver_sha" \
      "watchdog_sha256=$watchdog_sha" \
      "generator_sha256=$generator_sha" \
      "target_sweeps=$target_sweeps" \
      "rss_limit_bytes=$rss_limit_bytes" \
      "case=$case_name" \
      "scenario=$scenario_id" \
      "finalist=$finalist_id" \
      "seats=$seats" \
      "stack_bb=$stack_bb" \
      "abstraction_seed=$abstraction_seed" \
      "solver_seed=$row_solver_seed" \
      "evaluation_seed=$evaluation_seed" \
      "canonical_sha256=$canonical_sha" \
      "canonical_fingerprint=$canonical_fingerprint" \
      "compatibility_sha256=$compatibility_sha" \
      "compatibility_fingerprint=$compatibility_fingerprint" \
      "game_fingerprint=$game_fingerprint" \
      "tree_fingerprint=$tree_fingerprint" \
      "abstraction_spec_fingerprint=$abstraction_spec_fingerprint"
  )
  job_fingerprint=$(printf '%s' "$job_material" | hash_material)
  job_dir="$result_root/runs/$case_name/$scenario_id/${finalist_id}-s${row_solver_seed}/$job_fingerprint"
  result_json="$job_dir/run.json"
  checkpoint="$job_dir/checkpoint.mwckpt"
  progress_jsonl="$job_dir/progress.jsonl"
  job_meta="$job_dir/meta.json"

  if (( plan_only == 1 )); then
    if [[ -f "$result_json" ]] &&
      validate_result \
        "$result_json" "$target_sweeps" "$seats" \
        "$compatibility_fingerprint" "$game_fingerprint" &&
      is_success_result_status "$(jq -r '.status' "$result_json")" &&
      [[ "$(jq -r '.sweeps' "$result_json")" == "$target_sweeps" ]]; then
      planned_action=reuse
    elif [[ -f "$checkpoint" ]]; then
      planned_action=resume
    else
      planned_action=solve
    fi
    echo \
      "PLAN action=$planned_action case=$case_name scenario=$scenario_id finalist=$finalist_id seed=$row_solver_seed sweeps=$target_sweeps"
    continue
  fi

  mkdir -p "$job_dir/segments"
  [[ "$(sha256_file "$manifest")" == "$manifest_sha" ]] ||
    fail "manifest changed before $scenario_id"
  [[ "$(sha256_file "$experiment_metadata")" == "$metadata_sha" ]] ||
    fail "transfer metadata changed before $scenario_id"
  [[ "$(sha256_file "$index_path")" == "$index_sha" ]] ||
    fail "transfer config index changed before $scenario_id"
  [[ "$(sha256_file "$solver")" == "$solver_sha" ]] ||
    fail "solver changed before $scenario_id"
  [[ "$(sha256_file "$canonical_config")" == "$canonical_sha" ]] ||
    fail "canonical config changed before $scenario_id"
  [[ "$(sha256_file "$compatibility_config")" == "$compatibility_sha" ]] ||
    fail "compatibility config changed before $scenario_id"

  previous_result_status=
  previous_sweeps=
  if [[ -e "$result_json" || -e "$checkpoint" || -e "$progress_jsonl" ]]; then
    [[ -f "$job_meta" ]] ||
      fail \
        "untracked artifacts exist for $scenario_id; refusing an identity-unverified resume/reuse"
    [[ "$(jq -r '.schema // empty' "$job_meta" 2>/dev/null)" == \
      "solvers.abstraction-transfer-solve-run/v1" ]] ||
      fail "invalid job metadata for $scenario_id"
    [[ "$(jq -r '.jobFingerprint // empty' "$job_meta")" == "$job_fingerprint" ]] ||
      fail "job fingerprint mismatch for existing $scenario_id artifacts"
    [[ "$(jq -r '.inputs.manifest.sha256 // empty' "$job_meta")" == "$manifest_sha" ]] ||
      fail "manifest SHA mismatch for existing $scenario_id artifacts"
    [[ "$(jq -r '.inputs.transferMetadata.sha256 // empty' "$job_meta")" == \
      "$metadata_sha" ]] ||
      fail "metadata SHA mismatch for existing $scenario_id artifacts"
    [[ "$(jq -r '.inputs.configIndex.sha256 // empty' "$job_meta")" == "$index_sha" ]] ||
      fail "index SHA mismatch for existing $scenario_id artifacts"
    [[ "$(jq -r '.inputs.solver.sha256 // empty' "$job_meta")" == "$solver_sha" ]] ||
      fail "solver SHA mismatch for existing $scenario_id artifacts"
    [[ "$(jq -r '.inputs.generator.sha256 // empty' "$job_meta")" == \
      "$generator_sha" ]] ||
      fail "generator SHA mismatch for existing $scenario_id artifacts"
    [[ "$(jq -r '.inputs.canonicalConfig.sha256 // empty' "$job_meta")" == \
      "$canonical_sha" ]] ||
      fail "canonical config SHA mismatch for existing $scenario_id artifacts"
    [[ "$(jq -r '.inputs.compatibilityConfig.sha256 // empty' "$job_meta")" == \
      "$compatibility_sha" ]] ||
      fail "compatibility config SHA mismatch for existing $scenario_id artifacts"
    recorded_result_sha=$(jq -r '.artifacts.result.sha256 // "missing"' "$job_meta")
    recorded_checkpoint_sha=$(
      jq -r '.artifacts.checkpoint.sha256 // "missing"' "$job_meta"
    )
    recorded_progress_sha=$(jq -r '.artifacts.progress.sha256 // "missing"' "$job_meta")
    [[ "$(sha256_file_or_missing "$result_json")" == "$recorded_result_sha" ]] ||
      fail "result SHA mismatch for existing $scenario_id artifacts"
    [[ "$(sha256_file_or_missing "$checkpoint")" == "$recorded_checkpoint_sha" ]] ||
      fail "checkpoint SHA mismatch for existing $scenario_id artifacts"
    [[ "$(sha256_file_or_missing "$progress_jsonl")" == "$recorded_progress_sha" ]] ||
      fail "progress SHA mismatch for existing $scenario_id artifacts"
    recorded_job_status=$(jq -r '.status // empty' "$job_meta")
    case "$recorded_job_status" in
      completed|resource_limit|partial|cancelled|time-limit|sweep-limit) ;;
      *)
        fail \
          "existing $scenario_id artifacts have non-resumable metadata status=$recorded_job_status"
        ;;
    esac
  fi
  if [[ -f "$result_json" ]]; then
    if ! validate_result \
      "$result_json" "$target_sweeps" "$seats" \
      "$compatibility_fingerprint" "$game_fingerprint"; then
      fail "stale or invalid solve result for $scenario_id: $result_json"
    fi
    previous_result_status=$(jq -r '.status' "$result_json")
    previous_sweeps=$(jq -r '.sweeps' "$result_json")
    if is_success_result_status "$previous_result_status"; then
      if [[ "$previous_sweeps" != "$target_sweeps" ]]; then
        fail \
          "$scenario_id stopped early with status=$previous_result_status sweeps=$previous_sweeps; target=$target_sweeps"
      fi
      latest_segment=
      resource_limit_source=none
      if [[ -f "$job_meta" ]] &&
        [[ "$(jq -r '.jobFingerprint // empty' "$job_meta" 2>/dev/null)" == "$job_fingerprint" ]]; then
        latest_segment=$(jq -r '.artifacts.latestSegment // empty' "$job_meta")
        resource_limit_source=$(
          jq -r '.resources.resourceLimitSource // "none"' "$job_meta"
        )
      fi
      write_job_meta \
        "$job_meta" "completed" "reused" "$latest_segment" "$resource_limit_source"
      append_summary_row "$summary_tmp" "$job_meta"
      echo \
        "SKIP case=$case_name scenario=$scenario_id finalist=$finalist_id seed=$row_solver_seed sweeps=$target_sweeps"
      continue
    fi
    if ! is_partial_result_status "$previous_result_status"; then
      fail "unsupported partial status $previous_result_status in $result_json"
    fi
    [[ -f "$checkpoint" ]] ||
      fail "partial result for $scenario_id has no checkpoint to resume"
  fi

  mode=solve
  resume_from_sweeps=
  if [[ -f "$checkpoint" ]]; then
    mode=resume
    resume_from_sweeps=$previous_sweeps
  fi
  segment_number=1
  while :; do
    segment_id=$(printf '%04d' "$segment_number")
    segment_dir="$job_dir/segments/$segment_id"
    [[ ! -e "$segment_dir" ]] && break
    segment_number=$((segment_number + 1))
  done
  mkdir -p "$segment_dir"
  watchdog_metadata="$segment_dir/watchdog.txt"
  stdout_log="$segment_dir/stdout.log"
  stderr_log="$segment_dir/stderr.log"
  segment_meta="$segment_dir/meta.json"
  active_next_result="$job_dir/run.next.$$.json"
  rm -f -- "$active_next_result"
  cache_input_sha=$(sha256_file_or_missing "$cache_path")

  echo \
    "START mode=$mode case=$case_name scenario=$scenario_id finalist=$finalist_id seed=$row_solver_seed sweeps=$target_sweeps"
  if [[ "$mode" == "resume" ]]; then
    "$watchdog" \
      "$rss_limit_bytes" \
      "$watchdog_metadata" \
      "$stdout_log" \
      "$stderr_log" \
      "$solver" resume "$compatibility_config" \
      --checkpoint "$checkpoint" \
      --output "$active_next_result" \
      --metrics "$progress_jsonl" &
  else
    "$watchdog" \
      "$rss_limit_bytes" \
      "$watchdog_metadata" \
      "$stdout_log" \
      "$stderr_log" \
      "$solver" solve "$compatibility_config" \
      --output "$active_next_result" \
      --metrics "$progress_jsonl" \
      --checkpoint "$checkpoint" &
  fi
  active_watchdog_pid=$!
  set +e
  wait "$active_watchdog_pid"
  command_exit_code=$?
  set -e
  if [[ -n "$forwarded_watchdog_status" ]]; then
    command_exit_code=$forwarded_watchdog_status
  fi
  active_watchdog_pid=

  wall_seconds=$(watchdog_value "$watchdog_metadata" wall_seconds)
  peak_rss_bytes=$(watchdog_value "$watchdog_metadata" peak_rss_bytes)
  rss_limit_exceeded=$(watchdog_value "$watchdog_metadata" rss_limit_exceeded)
  rss_monitor_error=$(watchdog_value "$watchdog_metadata" rss_monitor_error)
  watchdog_recorded_exit=$(watchdog_value "$watchdog_metadata" exit_code)
  watchdog_recorded_limit=$(watchdog_value "$watchdog_metadata" rss_limit_bytes)
  watchdog_samples=$(watchdog_value "$watchdog_metadata" rss_samples)
  rss_limit_exceeded=${rss_limit_exceeded:-0}
  rss_monitor_error=${rss_monitor_error:-0}
  watchdog_metadata_invalid=0
  [[ "$wall_seconds" =~ ^[0-9]+$ ]] || watchdog_metadata_invalid=1
  [[ "$peak_rss_bytes" =~ ^[0-9]+$ ]] || watchdog_metadata_invalid=1
  [[ "$watchdog_recorded_exit" =~ ^[0-9]+$ ]] || watchdog_metadata_invalid=1
  [[ "$watchdog_recorded_limit" == "$rss_limit_bytes" ]] ||
    watchdog_metadata_invalid=1
  [[ "$watchdog_samples" =~ ^[0-9]+$ ]] || watchdog_metadata_invalid=1
  [[ "$rss_limit_exceeded" == "0" || "$rss_limit_exceeded" == "1" ]] ||
    watchdog_metadata_invalid=1
  [[ "$rss_monitor_error" == "0" || "$rss_monitor_error" == "1" ]] ||
    watchdog_metadata_invalid=1
  if [[ "$rss_limit_exceeded" == "0" &&
        "$rss_monitor_error" == "0" &&
        "$watchdog_recorded_exit" != "$command_exit_code" ]]; then
    watchdog_metadata_invalid=1
  fi
  if [[ "$rss_limit_exceeded" == "1" && "$command_exit_code" != "75" ]]; then
    watchdog_metadata_invalid=1
  fi
  if [[ "$rss_monitor_error" == "1" && "$command_exit_code" != "70" ]]; then
    watchdog_metadata_invalid=1
  fi
  if [[ "$peak_rss_bytes" =~ ^[0-9]+$ ]] &&
    (( peak_rss_bytes > rss_limit_bytes )) &&
    [[ "$rss_limit_exceeded" != "1" ]]; then
    watchdog_metadata_invalid=1
  fi
  if (( watchdog_metadata_invalid == 1 )); then
    rss_monitor_error=1
  fi
  cache_output_sha=$(sha256_file_or_missing "$cache_path")
  post_input_drift=0
  [[ "$(sha256_file "$manifest")" == "$manifest_sha" ]] || post_input_drift=1
  [[ "$(sha256_file "$experiment_metadata")" == "$metadata_sha" ]] ||
    post_input_drift=1
  [[ "$(sha256_file "$index_path")" == "$index_sha" ]] || post_input_drift=1
  [[ "$(sha256_file "$solver")" == "$solver_sha" ]] || post_input_drift=1
  [[ "$(sha256_file "$canonical_config")" == "$canonical_sha" ]] ||
    post_input_drift=1
  [[ "$(sha256_file "$compatibility_config")" == "$compatibility_sha" ]] ||
    post_input_drift=1

  new_result_valid=0
  if [[ -f "$active_next_result" ]] &&
    validate_result \
      "$active_next_result" "$target_sweeps" "$seats" \
      "$compatibility_fingerprint" "$game_fingerprint"; then
    if (( post_input_drift == 0 )); then
      new_result_valid=1
      mv -f -- "$active_next_result" "$result_json"
    else
      mv -f -- "$active_next_result" "$segment_dir/untrusted-result.json"
    fi
    active_next_result=
  fi

  resource_limit_source=none
  if [[ "$rss_monitor_error" == "1" ]]; then
    resource_limit_source=watchdog_monitor
  elif [[ "$rss_limit_exceeded" == "1" ]]; then
    resource_limit_source=watchdog_rss
  elif [[ -f "$result_json" &&
          "$(jq -r '.status' "$result_json")" == "resource_limit" ]]; then
    resource_limit_source=solver_memory
  fi

  segment_status=command_failed
  if (( post_input_drift == 1 )); then
    segment_status=input_drift
  elif [[ "$resource_limit_source" == "watchdog_monitor" ]]; then
    segment_status=monitor_error
  elif [[ "$resource_limit_source" != "none" ]]; then
    segment_status=resource_limit
  elif (( new_result_valid == 0 )); then
    segment_status=invalid_or_missing_result
  else
    current_status=$(jq -r '.status' "$result_json")
    current_sweeps=$(jq -r '.sweeps' "$result_json")
    if is_success_result_status "$current_status" &&
      [[ "$current_sweeps" == "$target_sweeps" ]]; then
      if (( command_exit_code == 0 )); then
        segment_status=completed
      else
        segment_status=command_failed
      fi
    elif is_partial_result_status "$current_status"; then
      segment_status=$current_status
    elif (( command_exit_code != 0 )); then
      segment_status=command_failed
    else
      segment_status=early_terminal
    fi
  fi

  result_sha=$(sha256_file_or_missing "$result_json")
  checkpoint_sha=$(sha256_file_or_missing "$checkpoint")
  progress_sha=$(sha256_file_or_missing "$progress_jsonl")
  write_segment_meta \
    "$segment_meta" \
    "$segment_status" \
    "$command_exit_code" \
    "$mode" \
    "$resume_from_sweeps" \
    "$result_sha" \
    "$checkpoint_sha" \
    "$progress_sha" \
    "$cache_input_sha" \
    "$cache_output_sha" \
    "$wall_seconds" \
    "$peak_rss_bytes" \
    "$rss_limit_exceeded" \
    "$rss_monitor_error" \
    "$resource_limit_source" \
    "$watchdog_metadata" \
    "$stdout_log" \
    "$stderr_log"

  normalized_status=$segment_status
  [[ "$segment_status" == "completed" ]] && normalized_status=completed
  write_job_meta \
    "$job_meta" \
    "$normalized_status" \
    "executed" \
    "$segment_meta" \
    "$resource_limit_source"
  append_summary_row "$summary_tmp" "$job_meta"

  if [[ "$segment_status" != "completed" ]]; then
    mv -f -- "$summary_tmp" "$summary_path"
    summary_tmp=
    case "$segment_status" in
      resource_limit)
        echo \
          "RESOURCE_LIMIT case=$case_name scenario=$scenario_id source=$resource_limit_source; resume by rerunning the same command; summary=$summary_path" \
          >&2
        exit 75
        ;;
      input_drift)
        echo \
          "INPUT_DRIFT case=$case_name scenario=$scenario_id; generated inputs changed during solve; summary=$summary_path" \
          >&2
        exit 3
        ;;
      monitor_error)
        echo \
          "MONITOR_ERROR case=$case_name scenario=$scenario_id; watchdog metadata or RSS sampling failed; summary=$summary_path" \
          >&2
        exit 70
        ;;
      cancelled)
        echo \
          "CANCELLED case=$case_name scenario=$scenario_id; rerun the same command to resume; summary=$summary_path" \
          >&2
        exit "${forwarded_runner_exit_code:-130}"
        ;;
      time-limit|sweep-limit)
        echo \
          "PARTIAL case=$case_name scenario=$scenario_id status=$segment_status; rerun the same command to resume; summary=$summary_path" \
          >&2
        exit 75
        ;;
      early_terminal)
        echo \
          "EARLY_TERMINAL case=$case_name scenario=$scenario_id status=$(jq -r '.status' "$result_json") sweeps=$(jq -r '.sweeps' "$result_json") target=$target_sweeps; summary=$summary_path" \
          >&2
        exit 3
        ;;
      *)
        echo \
          "FAILED case=$case_name scenario=$scenario_id exit=$command_exit_code status=$segment_status; see $stderr_log; summary=$summary_path" \
          >&2
        if (( command_exit_code > 0 && command_exit_code <= 255 )); then
          exit "$command_exit_code"
        fi
        exit 1
        ;;
    esac
  fi

  echo \
    "RESULT case=$case_name scenario=$scenario_id finalist=$finalist_id status=$(jq -r '.status' "$result_json") sweeps=$(jq -r '.sweeps' "$result_json") wall=${wall_seconds:-unknown}s peak_rss=${peak_rss_bytes:-unknown}"
done <"$selected_rows"

(( jobs_seen == selected_count )) ||
  fail "selected job count changed during execution"
if (( plan_only == 1 )); then
  echo "planned $jobs_seen serial transfer solve jobs"
else
  mv -f -- "$summary_tmp" "$summary_path"
  summary_tmp=
  echo "wrote $summary_path"
fi

rmdir -- "$lock_dir"
lock_dir=
trap - EXIT INT TERM HUP
cleanup_runner
