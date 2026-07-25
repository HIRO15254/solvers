#!/usr/bin/env bash
set -euo pipefail

readonly DEFAULT_TARGET_SWEEPS=10000
readonly DEFAULT_TOURNAMENT_CAP_BYTES=3221225472 # 3 GiB
readonly DEFAULT_CASH_CAP_BYTES=3758096384       # 3.5 GiB
readonly DEFAULT_RSS_LIMIT_BYTES=8053063680      # 7.5 GiB
readonly MAX_RSS_LIMIT_BYTES=8053063680
readonly INDEX_HEADER='role,case,id,abstraction_seed,solver_seed,evaluation_seed,config,cache,config_hash,game_fingerprint'
readonly SUMMARY_HEADER='candidate,case,abstraction_seed,solver_seed,target_sweeps,internal_cap_bytes,status,expectation_met,result_status,sweeps,infosets,solver_memory_bytes,solver_elapsed_seconds,segment_exit_code,segment_wall_seconds,segment_peak_rss_bytes,rss_limit_bytes,resource_limit_source,source_config_sha256,source_checkpoint_sha256,source_result_sha256,fork_config_sha256,fork_checkpoint_input_sha256,fork_checkpoint_output_sha256,result_sha256,source_config_hash,result_config_hash,game_fingerprint,abstraction_fingerprint,configuration_fingerprint,job_fingerprint,source_config,source_checkpoint,source_result,fork_config,fork_checkpoint,result,progress,job_meta'

usage() {
  cat >&2 <<'EOF'
usage:
  run-resource-ceiling.sh SOURCE_RESULT_ROOT RESULT_ROOT [OPTIONS]

options:
  --target-sweeps N             total sweep target (default: 10000)
  --tournament-cap-bytes N      T-E64 solver-storage cap
                                (default: 3221225472; 3 GiB)
  --cash-cap-bytes N            C-E64 solver-storage cap
                                (default: 3758096384; 3.5 GiB)
  --rss-limit-bytes N           process RSS watchdog limit
                                (default and maximum: 8053063680; 7.5 GiB)
  --solver PATH                 solver executable override
  --watchdog PATH               RSS watchdog override
  --plan                        validate inputs and print the two serial jobs
  -h, --help                    show this help

The source experiment is read-only. The runner selects T-E64 and C-E64 at
abstraction seed 0 / solver seed 1011, copies each checkpoint into a
content-addressed job below RESULT_ROOT, and changes only the copied config's
[run].max_memory_bytes value. A solver-side `resource_limit` result and exit 75
is the expected successful measurement. Reaching TARGET_SWEEPS is recorded as
`target_completed` and makes the runner exit 1 because the requested ceiling
was not found.

Exit codes:
  0   every job ended at the requested internal resource limit
  1   valid run, but at least one job reached the target or failed at runtime
  2   usage, selection, or preflight contract error
  3   provenance, compatibility, or artifact-integrity error
  70  RSS monitor failure
  75  external 7.5 GiB RSS watchdog limit (not an expected internal limit)
  129/130/143  forwarded HUP/INT/TERM after publishing the partial summary
EOF
}

die() {
  local code=$1
  shift
  echo "resource ceiling runner: $*" >&2
  exit "$code"
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
    die 2 "neither sha256sum nor shasum is available"
  fi
}

sha256_file_or_missing() {
  if [[ -f "$1" && ! -L "$1" ]]; then
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
    die 2 "neither sha256sum nor shasum is available"
  fi
}

config_scalar() {
  local path=$1
  local section=$2
  local key=$3
  awk -v wanted_section="[$section]" -v wanted_key="$key" '
    /^[[:space:]]*\[[^]]+\][[:space:]]*$/ {
      line = $0
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
      in_section = (line == wanted_section)
      next
    }
    in_section {
      line = $0
      sub(/[[:space:]]*#.*/, "", line)
      pattern = "^[[:space:]]*" wanted_key "[[:space:]]*="
      if (line ~ pattern) {
        sub(pattern, "", line)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
        if (line ~ /^".*"$/) {
          sub(/^"/, "", line)
          sub(/"$/, "", line)
        }
        print line
        found++
      }
    }
    END {
      if (found != 1) exit 42
    }
  ' "$path"
}

write_memory_capped_config() {
  local source=$1
  local destination=$2
  local cap=$3
  local temporary="${destination}.tmp.$$"
  active_atomic_tmp=$temporary
  if ! awk -v cap="$cap" '
    /^[[:space:]]*\[[^]]+\][[:space:]]*$/ {
      line = $0
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
      in_run = (line == "[run]")
    }
    in_run && /^[[:space:]]*max_memory_bytes[[:space:]]*=[[:space:]]*[0-9]+[[:space:]]*([#].*)?$/ {
      prefix = $0
      sub(/[0-9]+[[:space:]]*([#].*)?$/, "", prefix)
      suffix = $0
      sub(/^[^0-9]*[0-9]+/, "", suffix)
      print prefix cap suffix
      changed++
      next
    }
    { print }
    END {
      if (changed != 1) exit 42
    }
  ' "$source" >"$temporary"; then
    rm -f -- "$temporary"
    active_atomic_tmp=
    return 1
  fi
  mv -f -- "$temporary" "$destination"
  active_atomic_tmp=
}

watchdog_value() {
  local metadata=$1
  local key=$2
  awk -F= -v wanted="$key" '
    $1 == wanted { print $2; found++ }
    END { if (found != 1) exit 3 }
  ' "$metadata" 2>/dev/null || true
}

publish_summary() {
  summary_tmp="${summary_path}.tmp.$$"
  active_atomic_tmp=$summary_tmp
  cp "$summary_work" "$summary_tmp"
  mv -f -- "$summary_tmp" "$summary_path"
  summary_tmp=
  active_atomic_tmp=
}

source_inputs_unchanged() {
  current_source_index_sha=$(sha256_file_or_missing "$index_path")
  current_source_config_sha=$(sha256_file_or_missing "$source_config")
  current_source_checkpoint_sha=$(sha256_file_or_missing "$source_checkpoint")
  current_source_result_sha=$(sha256_file_or_missing "$source_result")
  current_source_cache_sha=$(sha256_file_or_missing "$source_cache")
  current_solver_sha=$(sha256_file_or_missing "$solver")
  current_watchdog_sha=$(sha256_file_or_missing "$watchdog")
  current_runner_sha=$(sha256_file_or_missing "$runner_path")

  [[ "$current_source_index_sha" == "$source_index_sha" &&
     "$current_source_config_sha" == "$source_config_sha" &&
     "$current_source_checkpoint_sha" == "$source_checkpoint_sha" &&
     "$current_source_result_sha" == "$source_result_sha" &&
     "$current_source_cache_sha" == "$source_cache_sha" &&
     "$current_solver_sha" == "$solver_sha" &&
     "$current_watchdog_sha" == "$watchdog_sha" &&
     "$current_runner_sha" == "$runner_sha" ]]
}

validate_candidate_result() {
  local path=$1
  jq -e \
    --argjson target "$target_sweeps" \
    --argjson source_sweeps "$source_sweeps" \
    --argjson cap "$internal_cap_bytes" \
    --argjson seats "$source_seats" \
    --arg source_config_hash "$source_config_hash" \
    --arg game "$expected_game_fingerprint" \
    --arg abstraction "$source_abstraction_fingerprint" \
    --arg configuration "$source_configuration_fingerprint" '
      def nonnegative_integer:
        type == "number" and . >= 0 and floor == .;
      def finite_nonnegative:
        type == "number" and . >= 0;
      .schemaVersion == 3
      and .kind == "preflop-multiway"
      and (.status
        | . == "completed"
          or . == "resource_limit"
          or . == "cancelled"
          or . == "sweep-limit"
          or . == "time-limit")
      and (.sweeps | nonnegative_integer)
      and .sweeps >= $source_sweeps
      and .sweeps <= $target
      and (.infosets | nonnegative_integer)
      and (.memoryBytes | nonnegative_integer)
      and .memoryBytes <= $cap
      and (.elapsedSecs | finite_nonnegative)
      and (.seats | type == "array" and length == $seats)
      and (.configHash
        | type == "string"
          and test("^[0-9a-f]{64}$")
          and . != $source_config_hash)
      and .effectiveConfig.run.max_memory_bytes == $cap
      and .gameFingerprint == $game
      and .abstractionFingerprint == $abstraction
      and .configurationFingerprint == $configuration
    ' "$path" >/dev/null
}

write_segment_meta() {
  local destination=$1
  local segment_status=$2
  local command_exit_code=$3
  local resource_limit_source=$4
  local result_sha=$5
  local checkpoint_input_sha=$6
  local checkpoint_output_sha=$7
  local progress_sha=$8
  local source_unchanged_value=$9
  shift 9
  local wall_seconds=$1
  local peak_rss_bytes=$2
  local rss_limit_exceeded=$3
  local rss_monitor_error=$4
  local forwarded_signal=$5
  local result_status_value=$6
  local result_sweeps_value=$7
  local temporary="${destination}.tmp.$$"
  active_atomic_tmp=$temporary

  jq -n \
    --arg status "$segment_status" \
    --arg command_exit_code "$command_exit_code" \
    --arg watchdog_child_exit_code "$watchdog_recorded_exit" \
    --arg candidate "$candidate_id" \
    --arg case_name "$case_name" \
    --arg target_sweeps "$target_sweeps" \
    --arg internal_cap "$internal_cap_bytes" \
    --arg rss_limit "$rss_limit_bytes" \
    --arg resume_from_sweeps "$resume_from_sweeps" \
    --arg source_config "$source_config" \
    --arg source_config_sha "$source_config_sha" \
    --arg source_config_hash "$source_config_hash" \
    --arg source_checkpoint "$source_checkpoint" \
    --arg source_checkpoint_sha "$source_checkpoint_sha" \
    --arg source_result "$source_result" \
    --arg source_result_sha "$source_result_sha" \
    --arg fork_config "$fork_config" \
    --arg fork_config_sha "$fork_config_sha" \
    --arg fork_checkpoint "$fork_checkpoint" \
    --arg checkpoint_input_sha "$checkpoint_input_sha" \
    --arg checkpoint_output_sha "$checkpoint_output_sha" \
    --arg result "$result_json" \
    --arg result_sha "$result_sha" \
    --arg progress "$progress_jsonl" \
    --arg progress_sha "$progress_sha" \
    --arg solver "$solver" \
    --arg solver_sha "$solver_sha" \
    --arg watchdog "$watchdog" \
    --arg watchdog_sha "$watchdog_sha" \
    --arg watchdog_metadata "$segment_watchdog" \
    --arg stdout_log "$segment_stdout" \
    --arg stderr_log "$segment_stderr" \
    --arg resource_limit_source "$resource_limit_source" \
    --arg source_unchanged "$source_unchanged_value" \
    --arg current_source_index_sha "$current_source_index_sha" \
    --arg current_source_config_sha "$current_source_config_sha" \
    --arg current_source_checkpoint_sha "$current_source_checkpoint_sha" \
    --arg current_source_result_sha "$current_source_result_sha" \
    --arg current_source_cache_sha "$current_source_cache_sha" \
    --arg wall_seconds "$wall_seconds" \
    --arg peak_rss_bytes "$peak_rss_bytes" \
    --arg rss_limit_exceeded "$rss_limit_exceeded" \
    --arg rss_monitor_error "$rss_monitor_error" \
    --arg forwarded_signal "$forwarded_signal" \
    --arg result_status "$result_status_value" \
    --arg result_sweeps "$result_sweeps_value" '
      {
        schema: "solvers.abstraction-resource-ceiling-segment/v1",
        status: $status,
        commandExitCode: ($command_exit_code | tonumber),
        watchdogChildExitCode: ($watchdog_child_exit_code | tonumber),
        selection: {
          candidate: $candidate,
          case: $case_name,
          targetSweeps: ($target_sweeps | tonumber),
          internalCapBytes: ($internal_cap | tonumber),
          resumeFromSweeps: ($resume_from_sweeps | tonumber)
        },
        inputs: {
          sourceConfig: {
            path: $source_config,
            sha256: $source_config_sha,
            configHash: $source_config_hash
          },
          sourceCheckpoint: {
            path: $source_checkpoint,
            sha256: $source_checkpoint_sha
          },
          sourceResult: {path: $source_result, sha256: $source_result_sha},
          solver: {path: $solver, sha256: $solver_sha},
          watchdog: {path: $watchdog, sha256: $watchdog_sha}
        },
        artifacts: {
          forkConfig: {path: $fork_config, sha256: $fork_config_sha},
          forkCheckpoint: {
            path: $fork_checkpoint,
            inputSha256: $checkpoint_input_sha,
            outputSha256: $checkpoint_output_sha
          },
          result: {
            path: $result,
            sha256: (if $result_sha == "missing" then null else $result_sha end),
            status:
              (if $result_status == "" then null else $result_status end),
            sweeps:
              (if $result_sweeps == "" then null
               else ($result_sweeps | tonumber) end)
          },
          progress: {
            path: $progress,
            sha256:
              (if $progress_sha == "missing" then null else $progress_sha end)
          },
          watchdogMetadata: $watchdog_metadata,
          stdoutLog: $stdout_log,
          stderrLog: $stderr_log
        },
        resources: {
          rssLimitBytes: ($rss_limit | tonumber),
          wallSeconds:
            (if $wall_seconds == "" then null else ($wall_seconds | tonumber) end),
          peakRssBytes:
            (if $peak_rss_bytes == "" then null
             else ($peak_rss_bytes | tonumber) end),
          rssLimitExceeded: ($rss_limit_exceeded == "1"),
          rssMonitorError: ($rss_monitor_error == "1"),
          resourceLimitSource:
            (if $resource_limit_source == "none" then null
             else $resource_limit_source end)
        },
        signal: {
          forwarded:
            (if $forwarded_signal == "none" then null else $forwarded_signal end)
        },
        sourceArtifactsContentUnchanged: ($source_unchanged == "1"),
        sourceArtifactsObservedSha256: {
          configIndex: $current_source_index_sha,
          config: $current_source_config_sha,
          checkpoint: $current_source_checkpoint_sha,
          result: $current_source_result_sha,
          cache: $current_source_cache_sha
        }
      }
  ' >"$temporary"
  mv -f -- "$temporary" "$destination"
  active_atomic_tmp=
}

write_job_meta() {
  local destination=$1
  local normalized_status=$2
  local expectation_met_value=$3
  local latest_segment=$4
  local resource_limit_source=$5
  local source_unchanged_value=$6
  local invocation_source=$7
  local result_sha checkpoint_sha progress_sha
  local result_status_value result_sweeps result_infosets result_memory result_elapsed
  local result_config_hash
  local temporary

  result_sha=$(sha256_file_or_missing "$result_json")
  checkpoint_sha=$(sha256_file_or_missing "$fork_checkpoint")
  progress_sha=$(sha256_file_or_missing "$progress_jsonl")
  result_status_value=
  result_sweeps=
  result_infosets=
  result_memory=
  result_elapsed=
  result_config_hash=
  if [[ -f "$result_json" && ! -L "$result_json" ]]; then
    result_status_value=$(jq -r '.status // empty' "$result_json" 2>/dev/null || true)
    result_sweeps=$(jq -r '.sweeps // empty' "$result_json" 2>/dev/null || true)
    result_infosets=$(jq -r '.infosets // empty' "$result_json" 2>/dev/null || true)
    result_memory=$(jq -r '.memoryBytes // empty' "$result_json" 2>/dev/null || true)
    result_elapsed=$(jq -r '.elapsedSecs // empty' "$result_json" 2>/dev/null || true)
    result_config_hash=$(jq -r '.configHash // empty' "$result_json" 2>/dev/null || true)
  fi

  temporary="${destination}.tmp.$$"
  active_atomic_tmp=$temporary
  jq -n \
    --arg job_fingerprint "$job_fingerprint" \
    --arg status "$normalized_status" \
    --arg expectation_met "$expectation_met_value" \
    --arg invocation_source "$invocation_source" \
    --arg candidate "$candidate_id" \
    --arg case_name "$case_name" \
    --arg abstraction_seed "$abstraction_seed" \
    --arg solver_seed "$row_solver_seed" \
    --arg evaluation_seed "$evaluation_seed" \
    --arg target_sweeps "$target_sweeps" \
    --arg internal_cap "$internal_cap_bytes" \
    --arg rss_limit "$rss_limit_bytes" \
    --arg source_index "$index_path" \
    --arg source_index_sha "$source_index_sha" \
    --arg source_config "$source_config" \
    --arg source_config_sha "$source_config_sha" \
    --arg source_config_hash "$source_config_hash" \
    --arg source_checkpoint "$source_checkpoint" \
    --arg source_checkpoint_sha "$source_checkpoint_sha" \
    --arg source_result "$source_result" \
    --arg source_result_sha "$source_result_sha" \
    --arg source_cache "$source_cache" \
    --arg source_cache_sha "$source_cache_sha" \
    --arg source_game_fingerprint "$expected_game_fingerprint" \
    --arg source_abstraction_fingerprint "$source_abstraction_fingerprint" \
    --arg source_configuration_fingerprint "$source_configuration_fingerprint" \
    --arg source_sweeps "$source_sweeps" \
    --arg source_memory "$source_memory_bytes" \
    --arg solver "$solver" \
    --arg solver_sha "$solver_sha" \
    --arg watchdog "$watchdog" \
    --arg watchdog_sha "$watchdog_sha" \
    --arg runner "$runner_path" \
    --arg runner_sha "$runner_sha" \
    --arg fork_config "$fork_config" \
    --arg fork_config_sha "$fork_config_sha" \
    --arg old_memory "$source_internal_cap" \
    --arg new_memory "$internal_cap_bytes" \
    --arg fork_checkpoint "$fork_checkpoint" \
    --arg fork_checkpoint_input_sha "$source_checkpoint_sha" \
    --arg fork_checkpoint_sha "$checkpoint_sha" \
    --arg result "$result_json" \
    --arg result_sha "$result_sha" \
    --arg progress "$progress_jsonl" \
    --arg progress_sha "$progress_sha" \
    --arg latest_segment "$latest_segment" \
    --arg resource_limit_source "$resource_limit_source" \
    --arg source_unchanged "$source_unchanged_value" \
    --arg current_source_index_sha "$current_source_index_sha" \
    --arg current_source_config_sha "$current_source_config_sha" \
    --arg current_source_checkpoint_sha "$current_source_checkpoint_sha" \
    --arg current_source_result_sha "$current_source_result_sha" \
    --arg current_source_cache_sha "$current_source_cache_sha" \
    --arg result_status "$result_status_value" \
    --arg result_sweeps "$result_sweeps" \
    --arg result_infosets "$result_infosets" \
    --arg result_memory "$result_memory" \
    --arg result_elapsed "$result_elapsed" \
    --arg result_config_hash "$result_config_hash" '
      {
        schema: "solvers.abstraction-resource-ceiling-run/v1",
        jobFingerprint: $job_fingerprint,
        status: $status,
        expectation: {
          expectedResultStatus: "resource_limit",
          met:
            (if $expectation_met == "" then null
             else ($expectation_met == "1") end)
        },
        lastInvocationSource: $invocation_source,
        selection: {
          candidate: $candidate,
          case: $case_name,
          abstractionSeed: ($abstraction_seed | tonumber),
          solverSeed: ($solver_seed | tonumber),
          evaluationSeedForLaterEvaluation: ($evaluation_seed | tonumber),
          targetSweeps: ($target_sweeps | tonumber),
          internalCapBytes: ($internal_cap | tonumber)
        },
        inputs: {
          sourceConfigIndex: {path: $source_index, sha256: $source_index_sha},
          sourceConfig: {
            path: $source_config,
            sha256: $source_config_sha,
            configHash: $source_config_hash
          },
          sourceCheckpoint: {
            path: $source_checkpoint,
            sha256: $source_checkpoint_sha
          },
          sourceResult: {
            path: $source_result,
            sha256: $source_result_sha,
            sweeps: ($source_sweeps | tonumber),
            solverMemoryBytes: ($source_memory | tonumber),
            gameFingerprint: $source_game_fingerprint,
            abstractionFingerprint: $source_abstraction_fingerprint,
            configurationFingerprint: $source_configuration_fingerprint
          },
          sourceCache: {path: $source_cache, sha256: $source_cache_sha},
          solver: {path: $solver, sha256: $solver_sha},
          watchdog: {path: $watchdog, sha256: $watchdog_sha},
          runner: {path: $runner, sha256: $runner_sha}
        },
        fork: {
          config: {
            path: $fork_config,
            sha256: $fork_config_sha,
            mutation: {
              field: "run.max_memory_bytes",
              sourceValue: ($old_memory | tonumber),
              forkValue: ($new_memory | tonumber),
              onlyRawConfigMutation: true
            }
          },
          checkpoint: {
            path: $fork_checkpoint,
            inputSha256: $fork_checkpoint_input_sha,
            currentSha256:
              (if $fork_checkpoint_sha == "missing"
               then null else $fork_checkpoint_sha end)
          }
        },
        resumeCompatibility: {
          mode: "explicit-config-plus-copied-checkpoint",
          sourceRawConfigHash: $source_config_hash,
          forkRawConfigHashFromResult:
            (if $result_config_hash == "" then null else $result_config_hash end),
          operationalMemoryExcludedFromConfigurationFingerprint: true,
          gameFingerprintPreserved:
            (if $result_status == "" then null else true end),
          abstractionFingerprintPreserved:
            (if $result_status == "" then null else true end),
          configurationFingerprintPreserved:
            (if $result_status == "" then null else true end)
        },
        artifacts: {
          result: {
            path: $result,
            sha256: (if $result_sha == "missing" then null else $result_sha end)
          },
          progress: {
            path: $progress,
            sha256:
              (if $progress_sha == "missing" then null else $progress_sha end)
          },
          latestSegment:
            (if $latest_segment == "" then null else $latest_segment end)
        },
        resources: {
          rssLimitBytes: ($rss_limit | tonumber),
          resourceLimitSource:
            (if $resource_limit_source == "none" then null
             else $resource_limit_source end)
        },
        result: {
          status:
            (if $result_status == "" then null else $result_status end),
          sweeps:
            (if $result_sweeps == "" then null
             else ($result_sweeps | tonumber) end),
          infosets:
            (if $result_infosets == "" then null
             else ($result_infosets | tonumber) end),
          solverMemoryBytes:
            (if $result_memory == "" then null
             else ($result_memory | tonumber) end),
          solverElapsedSeconds:
            (if $result_elapsed == "" then null
             else ($result_elapsed | tonumber) end),
          configHash:
            (if $result_config_hash == "" then null else $result_config_hash end)
        },
        sourceArtifacts: {
          contentUnchanged: ($source_unchanged == "1"),
          verifiedBySha256: true,
          observedSha256: {
            configIndex: $current_source_index_sha,
            config: $current_source_config_sha,
            checkpoint: $current_source_checkpoint_sha,
            result: $current_source_result_sha,
            cache: $current_source_cache_sha
          }
        }
      }
  ' >"$temporary"
  mv -f -- "$temporary" "$destination"
  active_atomic_tmp=
}

append_summary_row() {
  local destination=$1
  local meta=$2
  local latest exit_code wall peak
  latest=$(jq -r '.artifacts.latestSegment // empty' "$meta")
  exit_code=
  wall=
  peak=
  if [[ -n "$latest" ]]; then
    exit_code=$(jq -r '.commandExitCode // empty' "$latest")
    wall=$(jq -r '.resources.wallSeconds // empty' "$latest")
    peak=$(jq -r '.resources.peakRssBytes // empty' "$latest")
  fi
  jq -r \
    --arg exit_code "$exit_code" \
    --arg wall "$wall" \
    --arg peak "$peak" \
    --arg meta_path "$meta" '
    [
      .selection.candidate,
      .selection.case,
      .selection.abstractionSeed,
      .selection.solverSeed,
      .selection.targetSweeps,
      .selection.internalCapBytes,
      .status,
      .expectation.met,
      .result.status,
      .result.sweeps,
      .result.infosets,
      .result.solverMemoryBytes,
      .result.solverElapsedSeconds,
      (if $exit_code == "" then null else ($exit_code | tonumber) end),
      (if $wall == "" then null else ($wall | tonumber) end),
      (if $peak == "" then null else ($peak | tonumber) end),
      .resources.rssLimitBytes,
      .resources.resourceLimitSource,
      .inputs.sourceConfig.sha256,
      .inputs.sourceCheckpoint.sha256,
      .inputs.sourceResult.sha256,
      .fork.config.sha256,
      .fork.checkpoint.inputSha256,
      .fork.checkpoint.currentSha256,
      .artifacts.result.sha256,
      .inputs.sourceConfig.configHash,
      .result.configHash,
      .inputs.sourceResult.gameFingerprint,
      .inputs.sourceResult.abstractionFingerprint,
      .inputs.sourceResult.configurationFingerprint,
      .jobFingerprint,
      .inputs.sourceConfig.path,
      .inputs.sourceCheckpoint.path,
      .inputs.sourceResult.path,
      .fork.config.path,
      .fork.checkpoint.path,
      .artifacts.result.path,
      .artifacts.progress.path,
      $meta_path
    ] | @csv
  ' "$meta" >>"$destination"
}

validate_existing_job() {
  [[ -f "$job_meta" && ! -L "$job_meta" ]] || return 1
  jq -e \
    --arg job "$job_fingerprint" \
    --arg index_sha "$source_index_sha" \
    --arg config_sha "$source_config_sha" \
    --arg config_hash "$source_config_hash" \
    --arg checkpoint_sha "$source_checkpoint_sha" \
    --arg result_sha "$source_result_sha" \
    --arg cache_sha "$source_cache_sha" \
    --arg solver_sha "$solver_sha" \
    --arg watchdog_sha "$watchdog_sha" \
    --arg runner_sha "$runner_sha" \
    --arg fork_config_sha "$fork_config_sha" '
      .schema == "solvers.abstraction-resource-ceiling-run/v1"
      and .jobFingerprint == $job
      and .inputs.sourceConfigIndex.sha256 == $index_sha
      and .inputs.sourceConfig.sha256 == $config_sha
      and .inputs.sourceConfig.configHash == $config_hash
      and .inputs.sourceCheckpoint.sha256 == $checkpoint_sha
      and .inputs.sourceResult.sha256 == $result_sha
      and .inputs.sourceCache.sha256 == $cache_sha
      and .inputs.solver.sha256 == $solver_sha
      and .inputs.watchdog.sha256 == $watchdog_sha
      and .inputs.runner.sha256 == $runner_sha
      and .fork.config.sha256 == $fork_config_sha
    ' "$job_meta" >/dev/null || return 1

  local recorded_checkpoint recorded_result recorded_progress
  recorded_checkpoint=$(jq -r '.fork.checkpoint.currentSha256 // "missing"' "$job_meta")
  recorded_result=$(jq -r '.artifacts.result.sha256 // "missing"' "$job_meta")
  recorded_progress=$(jq -r '.artifacts.progress.sha256 // "missing"' "$job_meta")
  [[ "$(sha256_file_or_missing "$fork_checkpoint")" == "$recorded_checkpoint" &&
     "$(sha256_file_or_missing "$result_json")" == "$recorded_result" &&
     "$(sha256_file_or_missing "$progress_jsonl")" == "$recorded_progress" ]]
}

if (( $# < 2 )); then
  usage
  exit 2
fi

source_root=$1
result_root=$2
shift 2

workspace=$(cd "$(dirname "$0")/../.." && pwd -P)
runner_path="$workspace/experiments/abstraction-optimization-2026-07-25/run-resource-ceiling.sh"
research_target_dir="$workspace/target/research-release"
solver="$research_target_dir/release/solvers"
solver_overridden=0
watchdog="$workspace/experiments/abstraction-optimization-2026-07-25/run-with-rss-watchdog.sh"
target_sweeps=$DEFAULT_TARGET_SWEEPS
tournament_cap_bytes=$DEFAULT_TOURNAMENT_CAP_BYTES
cash_cap_bytes=$DEFAULT_CASH_CAP_BYTES
rss_limit_bytes=$DEFAULT_RSS_LIMIT_BYTES
plan_only=0

while (( $# > 0 )); do
  case "$1" in
    --target-sweeps)
      (( $# >= 2 )) || die 2 "--target-sweeps requires a value"
      target_sweeps=$2
      shift 2
      ;;
    --tournament-cap-bytes)
      (( $# >= 2 )) || die 2 "--tournament-cap-bytes requires a value"
      tournament_cap_bytes=$2
      shift 2
      ;;
    --cash-cap-bytes)
      (( $# >= 2 )) || die 2 "--cash-cap-bytes requires a value"
      cash_cap_bytes=$2
      shift 2
      ;;
    --rss-limit-bytes)
      (( $# >= 2 )) || die 2 "--rss-limit-bytes requires a value"
      rss_limit_bytes=$2
      shift 2
      ;;
    --solver)
      (( $# >= 2 )) || die 2 "--solver requires a path"
      solver=$2
      solver_overridden=1
      shift 2
      ;;
    --watchdog)
      (( $# >= 2 )) || die 2 "--watchdog requires a path"
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
      die 2 "unknown option $1"
      ;;
  esac
done

for command in jq awk tail ps cp mv find wc mktemp; do
  command -v "$command" >/dev/null 2>&1 ||
    die 2 "required command is unavailable: $command"
done
if (( solver_overridden == 0 )); then
  command -v cargo >/dev/null 2>&1 ||
    die 2 "required command is unavailable: cargo"
  (
    cd "$workspace"
    CARGO_TARGET_DIR="$research_target_dir" \
      cargo build --quiet --locked --release -p cli --features research --bin solvers
  )
fi
is_positive_i64 "$target_sweeps" ||
  die 2 "--target-sweeps must be a positive signed 64-bit integer"
is_positive_i64 "$tournament_cap_bytes" ||
  die 2 "--tournament-cap-bytes must be a positive signed 64-bit integer"
is_positive_i64 "$cash_cap_bytes" ||
  die 2 "--cash-cap-bytes must be a positive signed 64-bit integer"
is_positive_i64 "$rss_limit_bytes" ||
  die 2 "--rss-limit-bytes must be a positive signed 64-bit integer"
(( rss_limit_bytes <= MAX_RSS_LIMIT_BYTES )) ||
  die 2 "RSS limit exceeds the fixed 7.5 GiB experiment watchdog ceiling"
(( tournament_cap_bytes < rss_limit_bytes )) ||
  die 2 "tournament internal cap must leave headroom below the RSS limit"
(( cash_cap_bytes < rss_limit_bytes )) ||
  die 2 "cash internal cap must leave headroom below the RSS limit"

[[ -d "$source_root" && ! -L "$source_root" ]] ||
  die 2 "SOURCE_RESULT_ROOT is not a real directory: $source_root"
source_root=$(cd "$source_root" && pwd -P)
if [[ -e "$result_root" ]]; then
  [[ -d "$result_root" && ! -L "$result_root" ]] ||
    die 2 "RESULT_ROOT is not a real directory: $result_root"
  prospective_result_root=$(cd "$result_root" && pwd -P)
else
  result_parent=$(dirname "$result_root")
  result_basename=$(basename "$result_root")
  [[ "$result_basename" != "." && "$result_basename" != ".." ]] ||
    die 2 "RESULT_ROOT must name a dedicated child directory"
  [[ -d "$result_parent" && ! -L "$result_parent" ]] ||
    die 2 "RESULT_ROOT parent must already be a real directory: $result_parent"
  result_parent=$(cd "$result_parent" && pwd -P)
  prospective_result_root="$result_parent/$result_basename"
fi
case "$prospective_result_root/" in
  "$source_root/"*) die 2 "RESULT_ROOT must not be inside SOURCE_RESULT_ROOT" ;;
esac
case "$source_root/" in
  "$prospective_result_root/"*)
    die 2 "SOURCE_RESULT_ROOT must not be inside RESULT_ROOT"
    ;;
esac
if [[ ! -e "$prospective_result_root" ]]; then
  mkdir "$prospective_result_root"
fi
result_root=$(cd "$prospective_result_root" && pwd -P)
case "$result_root/" in
  "$source_root/"*) die 2 "RESULT_ROOT must not be inside SOURCE_RESULT_ROOT" ;;
esac
case "$source_root/" in
  "$result_root/"*) die 2 "SOURCE_RESULT_ROOT must not be inside RESULT_ROOT" ;;
esac
for path in "$source_root" "$result_root"; do
  if [[ "$path" == *','* || "$path" == *$'\n'* || "$path" == *$'\r'* ||
        "$path" == *$'\t'* ]]; then
    die 2 "result roots may not contain commas, tabs, or newlines"
  fi
done

solver=$(absolute_existing_file "$solver") || die 2 "missing regular solver: $solver"
watchdog=$(absolute_existing_file "$watchdog") ||
  die 2 "missing regular watchdog: $watchdog"
[[ -x "$solver" ]] || die 2 "solver is not executable: $solver"
[[ -x "$watchdog" ]] || die 2 "watchdog is not executable: $watchdog"
runner_path=$(absolute_existing_file "$runner_path") ||
  die 2 "runner cannot resolve its own path"

index_path="$source_root/configs/configs.csv"
index_path=$(absolute_existing_file "$index_path") ||
  die 2 "missing source config index: $index_path"
IFS= read -r actual_index_header <"$index_path"
[[ "$actual_index_header" == "$INDEX_HEADER" ]] ||
  die 2 "unsupported source config index header: $actual_index_header"

lock_dir="$result_root/.resource-ceiling.lock"
if ! mkdir "$lock_dir" 2>/dev/null; then
  die 2 "another resource-ceiling runner owns $lock_dir; jobs must stay serial"
fi
scratch_dir=$(mktemp -d "$result_root/.resource-ceiling-runner.XXXXXX")
summary_tmp=
active_atomic_tmp=
initializing_job_dir=
active_watchdog_pid=
active_pending_result=
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
  if [[ -n "${active_pending_result:-}" &&
        -f "$active_pending_result" ]]; then
    rm -f -- "$active_pending_result"
  fi
  if [[ -n "${active_atomic_tmp:-}" &&
        -f "$active_atomic_tmp" &&
        "$active_atomic_tmp" == "$result_root"/* ]]; then
    rm -f -- "$active_atomic_tmp"
  fi
  if [[ -n "${initializing_job_dir:-}" &&
        -d "$initializing_job_dir" &&
        "$initializing_job_dir" == "$result_root"/.resource-ceiling-job.* ]]; then
    rm -rf -- "$initializing_job_dir"
  fi
  if [[ -n "${summary_tmp:-}" && -f "$summary_tmp" ]]; then
    rm -f -- "$summary_tmp"
  fi
  if [[ -n "${scratch_dir:-}" && -d "$scratch_dir" &&
        "$scratch_dir" == "$result_root"/.resource-ceiling-runner.* ]]; then
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
    set +e
    wait "$active_watchdog_pid"
    forwarded_watchdog_status=$?
    set -e
    return
  fi
  exit "$exit_code"
}

trap cleanup_runner EXIT
trap 'forward_runner_signal INT 130' INT
trap 'forward_runner_signal TERM 143' TERM
trap 'forward_runner_signal HUP 129' HUP

source_index_sha=$(sha256_file "$index_path")
solver_sha=$(sha256_file "$solver")
watchdog_sha=$(sha256_file "$watchdog")
runner_sha=$(sha256_file "$runner_path")
selected_rows="$scratch_dir/selected.tsv"
: >"$selected_rows"

while IFS=, read -r \
  role \
  case_name \
  candidate_id \
  abstraction_seed \
  row_solver_seed \
  evaluation_seed \
  source_config \
  source_cache \
  source_config_hash \
  expected_game_fingerprint; do
  [[ "$role" == "candidate" && "$abstraction_seed" == "0" &&
     "$row_solver_seed" == "1011" ]] || continue
  if [[ "$case_name" == "tournament" && "$candidate_id" == "T-E64" ]]; then
    internal_cap_bytes=$tournament_cap_bytes
  elif [[ "$case_name" == "cash" && "$candidate_id" == "C-E64" ]]; then
    internal_cap_bytes=$cash_cap_bytes
  else
    continue
  fi

  for value in \
    "$case_name" "$candidate_id" "$abstraction_seed" "$row_solver_seed" \
    "$evaluation_seed" "$source_config" "$source_cache" \
    "$source_config_hash" "$expected_game_fingerprint"; do
    if [[ "$value" == *','* || "$value" == *$'\n'* || "$value" == *$'\r'* ||
          "$value" == *$'\t'* ]]; then
      die 2 "unsafe delimiter in source index row for $candidate_id"
    fi
  done
  [[ "$case_name" == "tournament" || "$case_name" == "cash" ]] ||
    die 2 "invalid case for $candidate_id"
  [[ "$candidate_id" =~ ^[[:alnum:]_.-]+$ ]] ||
    die 2 "unsafe candidate ID: $candidate_id"
  is_nonnegative_i64 "$abstraction_seed" ||
    die 2 "invalid abstraction seed for $candidate_id"
  is_nonnegative_i64 "$row_solver_seed" ||
    die 2 "invalid solver seed for $candidate_id"
  is_nonnegative_i64 "$evaluation_seed" ||
    die 2 "invalid evaluation seed for $candidate_id"
  [[ "$source_config_hash" =~ ^[0-9a-f]{64}$ &&
     "$expected_game_fingerprint" =~ ^[0-9a-f]{64}$ ]] ||
    die 2 "invalid source fingerprint for $candidate_id"

  source_config=$(absolute_existing_file "$source_config") ||
    die 2 "missing source config for $candidate_id"
  source_cache=$(absolute_existing_file "$source_cache") ||
    die 2 "missing warm source cache for $candidate_id"
  case "$source_config" in
    "$source_root/configs/"*) ;;
    *) die 2 "source config for $candidate_id escapes SOURCE_RESULT_ROOT/configs" ;;
  esac
  case "$source_cache" in
    "$source_root/cache/"*) ;;
    *) die 2 "source cache for $candidate_id escapes SOURCE_RESULT_ROOT/cache" ;;
  esac

  source_run_dir="$source_root/runs/$case_name/${candidate_id}-a${abstraction_seed}-s${row_solver_seed}"
  source_checkpoint=$(absolute_existing_file "$source_run_dir/checkpoint.mwckpt") ||
    die 2 "missing source checkpoint for $candidate_id"
  source_result=$(absolute_existing_file "$source_run_dir/run.json") ||
    die 2 "missing source result for $candidate_id"

  abstraction_kind=$(config_scalar "$source_config" game.abstraction kind) ||
    die 2 "cannot read game.abstraction.kind for $candidate_id"
  abstraction_recall=$(config_scalar "$source_config" game.abstraction recall) ||
    die 2 "cannot read game.abstraction.recall for $candidate_id"
  flop_buckets=$(config_scalar "$source_config" game.abstraction flop_buckets) ||
    die 2 "cannot read flop buckets for $candidate_id"
  turn_buckets=$(config_scalar "$source_config" game.abstraction turn_buckets) ||
    die 2 "cannot read turn buckets for $candidate_id"
  river_buckets=$(config_scalar "$source_config" game.abstraction river_buckets) ||
    die 2 "cannot read river buckets for $candidate_id"
  config_cache=$(config_scalar "$source_config" game.abstraction artifact_cache) ||
    die 2 "cannot read artifact cache for $candidate_id"
  source_internal_cap=$(config_scalar "$source_config" run max_memory_bytes) ||
    die 2 "cannot read source memory cap for $candidate_id"
  [[ "$abstraction_kind" == "ehs2-table" &&
     "$abstraction_recall" == "full" &&
     "$flop_buckets" == "64" &&
     "$turn_buckets" == "64" &&
     "$river_buckets" == "64" ]] ||
    die 2 "$candidate_id is not the required full-recall K64 EHS2 profile"
  [[ "$config_cache" == "$source_cache" ]] ||
    die 2 "config/index cache mismatch for $candidate_id"
  is_positive_i64 "$source_internal_cap" ||
    die 2 "invalid source internal cap for $candidate_id"

  if ! jq -e \
    --arg config_hash "$source_config_hash" \
    --arg game "$expected_game_fingerprint" \
    --argjson configured_memory "$source_internal_cap" '
      def positive_integer:
        type == "number" and . > 0 and floor == .;
      .schemaVersion == 3
      and .kind == "preflop-multiway"
      and .status == "completed"
      and (.sweeps | positive_integer)
      and (.infosets | positive_integer)
      and (.memoryBytes | positive_integer)
      and (.seats | type == "array" and length == 6)
      and .configHash == $config_hash
      and .gameFingerprint == $game
      and (.abstractionFingerprint
        | type == "string" and test("^[0-9a-f]{64}$"))
      and (.configurationFingerprint
        | type == "string" and test("^[0-9a-f]{64}$"))
      and .effectiveConfig.run.max_memory_bytes == $configured_memory
    ' "$source_result" >/dev/null; then
    die 3 "source result provenance is invalid for $candidate_id"
  fi

  source_sweeps=$(jq -er '.sweeps' "$source_result")
  source_memory_bytes=$(jq -er '.memoryBytes' "$source_result")
  source_seats=$(jq -er '.seats | length' "$source_result")
  source_abstraction_fingerprint=$(jq -er '.abstractionFingerprint' "$source_result")
  source_configuration_fingerprint=$(
    jq -er '.configurationFingerprint' "$source_result"
  )
  (( source_sweeps < target_sweeps )) ||
    die 2 "$candidate_id source already has $source_sweeps sweeps (target $target_sweeps)"
  (( source_memory_bytes < internal_cap_bytes )) ||
    die 2 "$candidate_id cap $internal_cap_bytes cannot restore source payload $source_memory_bytes"

  source_config_sha=$(sha256_file "$source_config")
  source_checkpoint_sha=$(sha256_file "$source_checkpoint")
  source_result_sha=$(sha256_file "$source_result")
  source_cache_sha=$(sha256_file "$source_cache")

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$case_name" "$candidate_id" "$abstraction_seed" "$row_solver_seed" \
    "$evaluation_seed" "$internal_cap_bytes" "$source_config" "$source_cache" \
    "$source_config_hash" "$expected_game_fingerprint" "$source_checkpoint" \
    "$source_result" "$source_internal_cap" "$source_sweeps" \
    "$source_memory_bytes" "$source_seats" "$source_abstraction_fingerprint" \
    "$source_configuration_fingerprint" "$source_config_sha" \
    "$source_checkpoint_sha" "$source_result_sha" "$source_cache_sha" \
    >>"$selected_rows"
done < <(tail -n +2 "$index_path")

[[ "$(wc -l <"$selected_rows" | tr -d '[:space:]')" == "2" ]] ||
  die 2 "source index must select exactly T-E64 and C-E64 for seed pair 0/1011"
[[ "$(sha256_file "$index_path")" == "$source_index_sha" ]] ||
  die 3 "source config index changed during selection"

summary_path="$result_root/resource-ceiling-s${target_sweeps}-summary.csv"
summary_work="$scratch_dir/summary.csv"
printf '%s\n' "$SUMMARY_HEADER" >"$summary_work"
overall_status=0
jobs_seen=0

while IFS=$'\t' read -r \
  case_name \
  candidate_id \
  abstraction_seed \
  row_solver_seed \
  evaluation_seed \
  internal_cap_bytes \
  source_config \
  source_cache \
  source_config_hash \
  expected_game_fingerprint \
  source_checkpoint \
  source_result \
  source_internal_cap \
  source_sweeps \
  source_memory_bytes \
  source_seats \
  source_abstraction_fingerprint \
  source_configuration_fingerprint \
  source_config_sha \
  source_checkpoint_sha \
  source_result_sha \
  source_cache_sha; do
  jobs_seen=$((jobs_seen + 1))
  job_material=$(
    printf '%s\n' \
      'solvers.abstraction-resource-ceiling-job/v1' \
      "source_index_sha256=$source_index_sha" \
      "source_config_sha256=$source_config_sha" \
      "source_config_hash=$source_config_hash" \
      "source_checkpoint_sha256=$source_checkpoint_sha" \
      "source_result_sha256=$source_result_sha" \
      "source_cache_sha256=$source_cache_sha" \
      "solver_sha256=$solver_sha" \
      "watchdog_sha256=$watchdog_sha" \
      "runner_sha256=$runner_sha" \
      "candidate=$candidate_id" \
      "case=$case_name" \
      "abstraction_seed=$abstraction_seed" \
      "solver_seed=$row_solver_seed" \
      "target_sweeps=$target_sweeps" \
      "internal_cap_bytes=$internal_cap_bytes" \
      "rss_limit_bytes=$rss_limit_bytes"
  )
  job_fingerprint=$(printf '%s' "$job_material" | hash_material)
  job_dir="$result_root/runs/$case_name/${candidate_id}-a${abstraction_seed}-s${row_solver_seed}/$job_fingerprint"
  fork_config="$job_dir/config.toml"
  fork_checkpoint="$job_dir/checkpoint.mwckpt"
  result_json="$job_dir/run.json"
  progress_jsonl="$job_dir/progress.jsonl"
  job_meta="$job_dir/meta.json"

  if (( plan_only == 1 )); then
    action=fork
    if [[ -f "$job_meta" ]]; then
      fork_config_sha=$(sha256_file_or_missing "$fork_config")
      if ! validate_existing_job; then
        die 3 "existing job provenance is invalid for $candidate_id"
      fi
      existing_status=$(jq -r '.status' "$job_meta")
      case "$existing_status" in
        resource_limit|target_completed) action=reuse ;;
        *) action=resume ;;
      esac
    fi
    echo \
      "PLAN action=$action candidate=$candidate_id case=$case_name source_sweeps=$source_sweeps target_sweeps=$target_sweeps internal_cap_bytes=$internal_cap_bytes rss_limit_bytes=$rss_limit_bytes"
    continue
  fi

  if [[ ! -e "$job_dir" ]]; then
    init_dir=$(mktemp -d "$result_root/.resource-ceiling-job.XXXXXX")
    initializing_job_dir=$init_dir
    mkdir -p "$init_dir/segments"
    if ! write_memory_capped_config \
      "$source_config" "$init_dir/config.toml" "$internal_cap_bytes"; then
      rm -rf -- "$init_dir"
      die 2 "failed to create a one-field memory-capped config for $candidate_id"
    fi
    [[ "$(config_scalar "$init_dir/config.toml" run max_memory_bytes)" == \
      "$internal_cap_bytes" ]] ||
      die 3 "fork config did not receive the requested cap for $candidate_id"
    cp -p "$source_checkpoint" "$init_dir/checkpoint.mwckpt"
    [[ "$(sha256_file "$init_dir/checkpoint.mwckpt")" == \
      "$source_checkpoint_sha" ]] ||
      die 3 "copied checkpoint SHA mismatch for $candidate_id"
    mkdir -p "$(dirname "$job_dir")"
    mv "$init_dir" "$job_dir"
    initializing_job_dir=
    fork_config_sha=$(sha256_file "$fork_config")
    source_unchanged=1
    source_inputs_unchanged || source_unchanged=0
    (( source_unchanged == 1 )) ||
      die 3 "source artifacts changed while forking $candidate_id"
    write_job_meta "$job_meta" ready "" "" none 1 forked
  else
    [[ -d "$job_dir" && ! -L "$job_dir" ]] ||
      die 3 "job path is not a real directory for $candidate_id"
    fork_config_sha=$(sha256_file_or_missing "$fork_config")
    [[ "$fork_config_sha" != "missing" ]] ||
      die 3 "existing job is missing its fork config for $candidate_id"
    if ! validate_existing_job; then
      die 3 "existing job provenance or artifact SHA is invalid for $candidate_id"
    fi
  fi

  fork_config_sha=$(sha256_file "$fork_config")
  [[ "$(config_scalar "$fork_config" run max_memory_bytes)" == \
    "$internal_cap_bytes" ]] ||
    die 3 "fork config cap drifted for $candidate_id"
  source_unchanged=1
  source_inputs_unchanged || source_unchanged=0
  (( source_unchanged == 1 )) ||
    die 3 "source artifacts changed before $candidate_id"

  existing_status=$(jq -r '.status' "$job_meta")
  case "$existing_status" in
    resource_limit)
      validate_candidate_result "$result_json" ||
        die 3 "reused resource-limit result is invalid for $candidate_id"
      [[ "$(jq -r '.status' "$result_json")" == "resource_limit" ]] ||
        die 3 "resource-limit job/result status mismatch for $candidate_id"
      write_job_meta \
        "$job_meta" resource_limit 1 \
        "$(jq -r '.artifacts.latestSegment // empty' "$job_meta")" \
        solver_memory 1 reused
      append_summary_row "$summary_work" "$job_meta"
      publish_summary
      echo "REUSED expected resource_limit candidate=$candidate_id"
      continue
      ;;
    target_completed)
      validate_candidate_result "$result_json" ||
        die 3 "reused target result is invalid for $candidate_id"
      [[ "$(jq -r '.status' "$result_json")" == "completed" &&
         "$(jq -r '.sweeps' "$result_json")" == "$target_sweeps" ]] ||
        die 3 "target-completed job/result mismatch for $candidate_id"
      write_job_meta \
        "$job_meta" target_completed 0 \
        "$(jq -r '.artifacts.latestSegment // empty' "$job_meta")" \
        none 1 reused
      append_summary_row "$summary_work" "$job_meta"
      publish_summary
      overall_status=1
      echo "REUSED target_completed candidate=$candidate_id"
      continue
      ;;
    ready|interrupted|watchdog_rss_limit|watchdog_monitor_error|solver_error|unexpected_terminal)
      ;;
    *)
      die 3 "unsupported existing job status $existing_status for $candidate_id"
      ;;
  esac

  resume_from_sweeps=$source_sweeps
  if [[ -f "$result_json" ]]; then
    validate_candidate_result "$result_json" ||
      die 3 "existing partial result is invalid for $candidate_id"
    resume_from_sweeps=$(jq -er '.sweeps' "$result_json")
  fi
  (( resume_from_sweeps < target_sweeps )) ||
    die 3 "partial job already reached target without target_completed status"

  segment_number=$(
    find "$job_dir/segments" -mindepth 1 -maxdepth 1 -type d \
      -name '[0-9][0-9][0-9][0-9]' | wc -l | tr -d '[:space:]'
  )
  segment_number=$((segment_number + 1))
  printf -v segment_name '%04d' "$segment_number"
  segment_dir="$job_dir/segments/$segment_name"
  [[ ! -e "$segment_dir" ]] ||
    die 3 "segment directory collision for $candidate_id/$segment_name"
  mkdir -p "$segment_dir"
  segment_watchdog="$segment_dir/watchdog.txt"
  segment_stdout="$segment_dir/stdout.log"
  segment_stderr="$segment_dir/stderr.log"
  segment_meta="$segment_dir/meta.json"
  pending_result="$segment_dir/run.json.pending"
  active_pending_result=$pending_result
  checkpoint_input_sha=$(sha256_file "$fork_checkpoint")

  forwarded_runner_signal=none
  forwarded_runner_exit_code=
  forwarded_watchdog_status=
  "$watchdog" \
    "$rss_limit_bytes" "$segment_watchdog" "$segment_stdout" "$segment_stderr" \
    "$solver" resume "$fork_config" \
    --checkpoint "$fork_checkpoint" \
    --max-sweeps "$target_sweeps" \
    --output "$pending_result" \
    --metrics "$progress_jsonl" &
  active_watchdog_pid=$!
  set +e
  wait "$active_watchdog_pid"
  command_exit_code=$?
  set -e
  if [[ -n "$forwarded_watchdog_status" ]]; then
    command_exit_code=$forwarded_watchdog_status
  fi
  active_watchdog_pid=

  if [[ ! -f "$segment_watchdog" &&
        "$forwarded_runner_signal" != "none" ]]; then
    synthetic_watchdog="${segment_watchdog}.tmp.$$"
    {
      echo "pid=unknown"
      echo "exit_code=$command_exit_code"
      echo "wall_seconds=0"
      echo "peak_rss_bytes=0"
      echo "rss_limit_bytes=$rss_limit_bytes"
      echo "rss_limit_exceeded=0"
      echo "rss_samples=0"
      echo "rss_monitor_error=0"
      echo "forwarded_signal=$forwarded_runner_signal"
    } >"$synthetic_watchdog"
    mv -f -- "$synthetic_watchdog" "$segment_watchdog"
  fi
  [[ -f "$segment_watchdog" && ! -L "$segment_watchdog" ]] ||
    die 3 "watchdog did not write trusted metadata for $candidate_id"
  watchdog_recorded_exit=$(watchdog_value "$segment_watchdog" exit_code)
  wall_seconds=$(watchdog_value "$segment_watchdog" wall_seconds)
  peak_rss_bytes=$(watchdog_value "$segment_watchdog" peak_rss_bytes)
  watchdog_rss_exceeded=$(watchdog_value "$segment_watchdog" rss_limit_exceeded)
  watchdog_monitor_error=$(watchdog_value "$segment_watchdog" rss_monitor_error)
  watchdog_forwarded_signal=$(watchdog_value "$segment_watchdog" forwarded_signal)
  [[ "$watchdog_recorded_exit" =~ ^[0-9]+$ &&
     "$wall_seconds" =~ ^[0-9]+$ &&
     "$peak_rss_bytes" =~ ^[0-9]+$ &&
     "$watchdog_rss_exceeded" =~ ^[01]$ &&
     "$watchdog_monitor_error" =~ ^[01]$ &&
     "$watchdog_forwarded_signal" =~ ^(none|INT|TERM|HUP)$ ]] ||
    die 3 "watchdog metadata is malformed for $candidate_id"
  if (( watchdog_rss_exceeded == 1 )); then
    [[ "$command_exit_code" == "75" ]] ||
      die 3 "RSS watchdog did not return exit 75 for $candidate_id"
  elif (( watchdog_monitor_error == 1 )); then
    [[ "$command_exit_code" == "70" ]] ||
      die 3 "failed RSS monitor did not return exit 70 for $candidate_id"
  else
    [[ "$watchdog_recorded_exit" == "$command_exit_code" ]] ||
      die 3 "watchdog/child exit mismatch for $candidate_id"
  fi

  source_unchanged=1
  source_inputs_unchanged || source_unchanged=0
  trusted_new_result=0
  pending_result_status=
  pending_result_sweeps=
  if [[ -f "$pending_result" && ! -L "$pending_result" ]] &&
    validate_candidate_result "$pending_result"; then
    trusted_new_result=1
    pending_result_status=$(jq -er '.status' "$pending_result")
    pending_result_sweeps=$(jq -er '.sweeps' "$pending_result")
  fi

  resource_limit_source=none
  segment_status=solver_error
  expectation_met=0
  terminal_exit=1
  if (( source_unchanged == 0 )); then
    segment_status=source_drift
    terminal_exit=3
  elif [[ "$forwarded_runner_signal" != "none" ]]; then
    segment_status=interrupted
    terminal_exit=$forwarded_runner_exit_code
  elif (( watchdog_monitor_error == 1 )); then
    segment_status=watchdog_monitor_error
    resource_limit_source=watchdog_monitor
    terminal_exit=70
  elif (( watchdog_rss_exceeded == 1 )); then
    segment_status=watchdog_rss_limit
    resource_limit_source=watchdog_rss
    terminal_exit=75
  elif (( trusted_new_result == 1 )) &&
    [[ "$command_exit_code" == "75" &&
       "$pending_result_status" == "resource_limit" &&
       "$pending_result_sweeps" -lt "$target_sweeps" ]]; then
    segment_status=resource_limit
    resource_limit_source=solver_memory
    expectation_met=1
    terminal_exit=0
  elif (( trusted_new_result == 1 )) &&
    [[ "$command_exit_code" == "0" &&
       "$pending_result_status" == "completed" &&
       "$pending_result_sweeps" == "$target_sweeps" ]]; then
    segment_status=target_completed
    terminal_exit=1
  elif (( trusted_new_result == 1 )); then
    segment_status=unexpected_terminal
    terminal_exit=1
  else
    case "$command_exit_code" in
      2) terminal_exit=2 ;;
      3) terminal_exit=3 ;;
      70) terminal_exit=70 ;;
      75) terminal_exit=75 ;;
      129|130|143) terminal_exit=$command_exit_code ;;
      *) terminal_exit=1 ;;
    esac
  fi

  if (( trusted_new_result == 1 && source_unchanged == 1 )); then
    mv -f -- "$pending_result" "$result_json"
    active_pending_result=
  elif [[ -f "$pending_result" ]]; then
    mv -f -- "$pending_result" "$segment_dir/untrusted-result.json"
    active_pending_result=
  fi

  checkpoint_output_sha=$(sha256_file_or_missing "$fork_checkpoint")
  result_sha=$(sha256_file_or_missing "$result_json")
  progress_sha=$(sha256_file_or_missing "$progress_jsonl")
  result_status_for_meta=
  result_sweeps_for_meta=
  if [[ -f "$result_json" ]]; then
    result_status_for_meta=$(jq -r '.status // empty' "$result_json")
    result_sweeps_for_meta=$(jq -r '.sweeps // empty' "$result_json")
  fi
  write_segment_meta \
    "$segment_meta" "$segment_status" "$command_exit_code" \
    "$resource_limit_source" "$result_sha" "$checkpoint_input_sha" \
    "$checkpoint_output_sha" "$progress_sha" "$source_unchanged" \
    "$wall_seconds" "$peak_rss_bytes" "$watchdog_rss_exceeded" \
    "$watchdog_monitor_error" "$forwarded_runner_signal" \
    "$result_status_for_meta" "$result_sweeps_for_meta"

  write_job_meta \
    "$job_meta" "$segment_status" "$expectation_met" "$segment_meta" \
    "$resource_limit_source" "$source_unchanged" executed
  append_summary_row "$summary_work" "$job_meta"
  publish_summary

  case "$segment_status" in
    resource_limit)
      echo \
        "EXPECTED_RESOURCE_LIMIT candidate=$candidate_id sweeps=$pending_result_sweeps cap=$internal_cap_bytes summary=$summary_path"
      ;;
    target_completed)
      overall_status=1
      echo \
        "TARGET_COMPLETED candidate=$candidate_id sweeps=$target_sweeps; internal ceiling not reached" \
        >&2
      ;;
    *)
      echo \
        "RESOURCE_CEILING_STOP candidate=$candidate_id status=$segment_status exit=$terminal_exit summary=$summary_path" \
        >&2
      exit "$terminal_exit"
      ;;
  esac
done <"$selected_rows"

(( jobs_seen == 2 )) || die 2 "expected two serial resource-ceiling jobs"
if (( plan_only == 1 )); then
  echo "planned 2 serial full-recall K64 resource-ceiling jobs"
else
  publish_summary
  echo "wrote $summary_path"
fi

rmdir -- "$lock_dir"
lock_dir=
trap - EXIT INT TERM HUP
cleanup_runner
exit "$overall_status"
