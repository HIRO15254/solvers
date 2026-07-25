#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage:
  run-evaluation-rung.sh RUNG_ID RESULT_ROOT [CANDIDATE_REGEX] [OPTIONS]

options:
  --samples N                    override rung evaluation_samples
  --br-traversals N              override rung deviator_traversals_per_seat
  --seed-pairs N                 override rung seed_pairs (first N manifest pairs)
  --evaluation-seed N            override each selected pair's evaluation seed
  --reference-set screening|final
                                 override manifest rung-specific routing
  --reference-filter ERE         further filter the selected reference IDs;
                                 writes rf-SHA256-prefixed summary/coverage names
  --rss-limit-bytes N            override manifest local_rss_stop_bytes (max 8 GiB)
  --coverage-candidate-stored-min F
                                 override the post-run coverage gate minimum
  --coverage-candidate-postflop-stored-min F
                                 override the candidate postflop stored minimum
  --coverage-candidate-postflop-min-visits N
                                 override candidate postflop applicability
  --solver PATH                  default: WORKSPACE/target/research-release/release/solvers
  --manifest PATH                override the experiment manifest
  --watchdog PATH                override the RSS watchdog
  --plan                         validate inputs and print jobs without running
  -h, --help                     show this help
EOF
}

fail() {
  echo "evaluation harness: $*" >&2
  exit 2
}

is_positive_integer() {
  [[ "$1" =~ ^[1-9][0-9]*$ ]]
}

derive_training_seed() {
  python3 -c '
import sys

seed = int(sys.argv[1])
if not 0 <= seed <= (1 << 64) - 1:
    raise SystemExit("evaluation seed is outside the u64 range")
print(seed ^ 0x70757269)
' "$1"
}

absolute_existing_file() {
  local path=$1
  local directory
  local basename
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

manifest_rung_values() {
  local manifest_path=$1
  local wanted=$2
  awk -v wanted="$wanted" '
    function value_string(line, value) {
      value = line
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/".*$/, "", value)
      return value
    }
    function value_integer(line, value) {
      value = line
      sub(/#.*/, "", value)
      sub(/^[^=]*=[[:space:]]*/, "", value)
      gsub(/[[:space:]]/, "", value)
      return value
    }
    function flush() {
      if (section == "rung" && id == wanted) {
        if (sweeps == "" || seed_pairs == "" || samples == "" || traversals == "") {
          exit 4
        }
        print sweeps "|" seed_pairs "|" samples "|" traversals
        found++
      }
    }
    /^\[\[/ {
      flush()
      section = ($0 == "[[rung]]") ? "rung" : "other"
      id = sweeps = seed_pairs = samples = traversals = ""
      next
    }
    section == "rung" && /^[[:space:]]*id[[:space:]]*=/ {
      id = value_string($0)
    }
    section == "rung" && /^[[:space:]]*sweeps[[:space:]]*=/ {
      sweeps = value_integer($0)
    }
    section == "rung" && /^[[:space:]]*seed_pairs[[:space:]]*=/ {
      seed_pairs = value_integer($0)
    }
    section == "rung" && /^[[:space:]]*evaluation_samples[[:space:]]*=/ {
      samples = value_integer($0)
    }
    section == "rung" && /^[[:space:]]*deviator_traversals_per_seat[[:space:]]*=/ {
      traversals = value_integer($0)
    }
    END {
      flush()
      if (found != 1) {
        exit 4
      }
    }
  ' "$manifest_path"
}

manifest_rss_limit() {
  local manifest_path=$1
  awk '
    /^\[selection\]$/ {
      selection = 1
      next
    }
    /^\[/ {
      selection = 0
    }
    selection && /^[[:space:]]*local_rss_stop_bytes[[:space:]]*=/ {
      line = $0
      sub(/#.*/, "", line)
      sub(/^[^=]*=[[:space:]]*/, "", line)
      gsub(/[[:space:]]/, "", line)
      print line
      found++
    }
    END {
      if (found != 1) {
        exit 4
      }
    }
  ' "$manifest_path"
}

manifest_reference_classes() {
  local manifest_path=$1
  awk '
    function value_string(line, value) {
      value = line
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/".*$/, "", value)
      return value
    }
    function value_boolean(line, value) {
      value = line
      sub(/#.*/, "", value)
      sub(/^[^=]*=[[:space:]]*/, "", value)
      gsub(/[[:space:]]/, "", value)
      return value
    }
    function flush() {
      if (section == "reference" && id != "") {
        print id "|" ((final_only == "true") ? "final" : "screening") "|" recall
      }
    }
    /^\[\[/ {
      flush()
      section = ($0 == "[[reference]]") ? "reference" : "other"
      id = ""
      final_only = "false"
      recall = ""
      next
    }
    section == "reference" && /^[[:space:]]*id[[:space:]]*=/ {
      id = value_string($0)
    }
    section == "reference" && /^[[:space:]]*final_only[[:space:]]*=/ {
      final_only = value_boolean($0)
    }
    section == "reference" && /^[[:space:]]*recall[[:space:]]*=/ {
      recall = value_string($0)
    }
    END {
      flush()
    }
  ' "$manifest_path"
}

config_abstraction_recall() {
  local config_path=$1
  awk '
    /^\[game\.abstraction\]$/ {
      abstraction = 1
      next
    }
    /^\[/ {
      abstraction = 0
    }
    abstraction && /^[[:space:]]*recall[[:space:]]*=/ {
      value = $0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/".*$/, "", value)
      print value
      found++
    }
    END {
      if (found != 1) {
        exit 4
      }
    }
  ' "$config_path"
}

manifest_seed_tuples() {
  local manifest_path=$1
  awk '
    function value_integer(line, value) {
      value = line
      sub(/#.*/, "", value)
      sub(/^[^=]*=[[:space:]]*/, "", value)
      gsub(/[[:space:]]/, "", value)
      return value
    }
    function flush() {
      if (section == "seed_pair" && abstraction != "" && solver != "" && evaluation != "") {
        print abstraction "|" solver "|" evaluation
      }
    }
    /^\[\[/ {
      flush()
      section = ($0 == "[[seed_pair]]") ? "seed_pair" : "other"
      abstraction = ""
      solver = ""
      evaluation = ""
      next
    }
    section == "seed_pair" && /^[[:space:]]*abstraction[[:space:]]*=/ {
      abstraction = value_integer($0)
    }
    section == "seed_pair" && /^[[:space:]]*solver[[:space:]]*=/ {
      solver = value_integer($0)
    }
    section == "seed_pair" && /^[[:space:]]*evaluation[[:space:]]*=/ {
      evaluation = value_integer($0)
    }
    END {
      flush()
    }
  ' "$manifest_path"
}

value_is_listed() {
  local value=$1
  local list_path=$2
  awk -v wanted="$value" '$0 == wanted { found = 1 } END { exit !found }' "$list_path"
}

validate_report() {
  local report_path=$1
  local candidate_config=$2
  local checkpoint=$3
  local candidate_config_hash=$4
  local candidate_game_fingerprint=$5
  local candidate_abstraction_fingerprint=$6
  local candidate_configuration_fingerprint=$7
  local reference_config=$8
  local reference_config_hash=$9
  shift 9
  local reference_game_fingerprint=$1
  local reference_cache=$2
  local expected_samples=$3
  local expected_seed=$4
  local expected_traversals=$5
  local expected_seats=$6
  local expected_reference_recall=$7
  local expected_rung=$8
  local expected_sweeps=$9
  shift 9
  local expected_training_seed=$1
  local expected_profile=$2
  local expected_purify_threshold=$3

  jq -e \
    --arg candidate_config "$candidate_config" \
    --arg checkpoint "$checkpoint" \
    --arg candidate_config_hash "$candidate_config_hash" \
    --arg candidate_game "$candidate_game_fingerprint" \
    --arg candidate_abstraction "$candidate_abstraction_fingerprint" \
    --arg candidate_configuration "$candidate_configuration_fingerprint" \
    --arg reference_config "$reference_config" \
    --arg reference_config_hash "$reference_config_hash" \
    --arg reference_game "$reference_game_fingerprint" \
    --arg reference_cache "$reference_cache" \
    --argjson samples "$expected_samples" \
    --argjson seed "$expected_seed" \
    --argjson traversals "$expected_traversals" \
    --argjson seats "$expected_seats" \
    --arg reference_recall "$expected_reference_recall" \
    --arg expected_rung "$expected_rung" \
    --argjson expected_sweeps "$expected_sweeps" \
    --argjson expected_training_seed "$expected_training_seed" \
    --arg expected_profile "$expected_profile" \
    --argjson expected_purify "$expected_purify_threshold" \
    '
      def nonnegative_integer:
        type == "number" and . >= 0 and floor == .;
      def finite_number:
        type == "number";
      def street_counts:
        type == "object"
        and (keys | sort) == ["flop", "preflop", "river", "turn"]
        and all(.[]; nonnegative_integer);
      def street_total:
        .preflop + .flop + .turn + .river;
      .schema_version == "solvers.reference-deviation-profile/v1"
      and .experiment.rung == $expected_rung
      and .candidate.config_path == $candidate_config
      and .candidate.checkpoint_path == $checkpoint
      and .candidate.config_fingerprint == $candidate_config_hash
      and .candidate.game_fingerprint == $candidate_game
      and .candidate.abstraction_fingerprint == $candidate_abstraction
      and .candidate.configuration_fingerprint == $candidate_configuration
      and .candidate.sweeps == $expected_sweeps
      and .reference.config_path == $reference_config
      and .reference.config_fingerprint == $reference_config_hash
      and .reference.game_fingerprint == $reference_game
      and .reference.game_fingerprint == .candidate.game_fingerprint
      and (.reference.abstraction_fingerprint
        | type == "string" and test("^[0-9a-f]{64}$"))
      and .reference.recall == $reference_recall
      and .reference.artifact_cache == $reference_cache
      and .samples == $samples
      and .seed == $seed
      and .br_traversals == $traversals
      and .profile == $expected_profile
      and .purify_threshold == $expected_purify
      and .training_seed == $expected_training_seed
      and (.elapsed_secs | finite_number and . >= 0)
      and (.training_coverage | length == $seats)
      and ([.training_coverage[].seat] == [range(0; $seats)])
      and all(.training_coverage[];
        (.traversals == $traversals)
        and (.visited_infosets | nonnegative_integer)
        and (.retained_infosets | nonnegative_integer)
        and (.total_visits | nonnegative_integer)
        and (.retained_visits | nonnegative_integer)
        and .retained_infosets <= .visited_infosets
        and .retained_visits <= .total_visits)
      and .evaluation.evaluation.samples == $samples
      and (.evaluation.evaluation.total_deal_attempts | nonnegative_integer)
      and (.evaluation.evaluation.seats | length == $seats)
      and (.evaluation.evaluation.deviation_gain_lower_bound | length == $seats)
      and (.evaluation.candidate_policy_coverage | length == $seats)
      and all(.evaluation.candidate_policy_coverage[];
        . as $coverage
        | (.decision_visits | nonnegative_integer)
        and (.stored_strategy_visits | nonnegative_integer)
        and (.uniform_fallback_visits | nonnegative_integer)
        and (.decision_visits_by_street | street_counts)
        and (.stored_strategy_visits_by_street | street_counts)
        and (.uniform_fallback_visits_by_street | street_counts)
        and .decision_visits == (.stored_strategy_visits + .uniform_fallback_visits)
        and .decision_visits == (.decision_visits_by_street | street_total)
        and .stored_strategy_visits ==
          (.stored_strategy_visits_by_street | street_total)
        and .uniform_fallback_visits ==
          (.uniform_fallback_visits_by_street | street_total)
        and all(["preflop", "flop", "turn", "river"][];
          . as $street
          | $coverage.decision_visits_by_street[$street] ==
            ($coverage.stored_strategy_visits_by_street[$street]
              + $coverage.uniform_fallback_visits_by_street[$street])))
      and (.evaluation.coverage | length == $seats)
      and all(.evaluation.coverage[];
        . as $coverage
        | (.decision_visits | nonnegative_integer)
        and (.trained_action_visits | nonnegative_integer)
        and (.baseline_fallback_visits | nonnegative_integer)
        and (.decision_visits_by_street | street_counts)
        and (.trained_action_visits_by_street | street_counts)
        and (.baseline_fallback_visits_by_street | street_counts)
        and .decision_visits == (.trained_action_visits + .baseline_fallback_visits)
        and .decision_visits == (.decision_visits_by_street | street_total)
        and .trained_action_visits ==
          (.trained_action_visits_by_street | street_total)
        and .baseline_fallback_visits ==
          (.baseline_fallback_visits_by_street | street_total)
        and all(["preflop", "flop", "turn", "river"][];
          . as $street
          | $coverage.decision_visits_by_street[$street] ==
            ($coverage.trained_action_visits_by_street[$street]
              + $coverage.baseline_fallback_visits_by_street[$street])))
      and (.evaluation.worlds | length == $samples)
      and ([.evaluation.worlds[].sample_id] == [range(0; $samples)])
      and all(.evaluation.worlds[];
        (.baseline_utilities | length == $seats)
        and (.deviating_seat_utilities | length == $seats)
        and (.gains | length == $seats)
        and all(.baseline_utilities[]; finite_number)
        and all(.deviating_seat_utilities[]; finite_number)
        and all(.gains[]; finite_number))
    ' "$report_path" >/dev/null
}

write_job_meta() {
  local meta_path=$1
  local status=$2
  local job_fingerprint=$3
  local command_exit_code=$4
  local rung_id=$5
  local reference_set=$6
  local case_name=$7
  local candidate_id=$8
  local abstraction_seed=$9
  shift 9
  local solver_seed=$1
  local evaluation_seed=$2
  local reference_id=$3
  local samples=$4
  local traversals=$5
  local sweeps=$6
  local training_seed=$7
  local profile=$8
  local purify_threshold=$9
  shift 9
  local manifest_path=$1
  local manifest_sha=$2
  local solver_path=$3
  local solver_sha=$4
  local candidate_config=$5
  local candidate_config_sha=$6
  local checkpoint=$7
  local checkpoint_sha=$8
  local reference_config=$9
  shift 9
  local reference_config_sha=$1
  local reference_cache=$2
  local cache_input_sha=$3
  local cache_output_sha=$4
  local report_path=$5
  local report_sha=$6
  local watchdog_meta=$7
  local wall_seconds=$8
  local peak_rss_bytes=$9
  shift 9
  local rss_limit_bytes=$1
  local candidate_game_fingerprint=$2
  local candidate_abstraction_fingerprint=$3
  local reference_abstraction_fingerprint=$4
  local reference_filter=$5
  local reference_filter_suffix=$6

  local tmp_path="${meta_path}.tmp.$$"
  jq -n \
    --arg status "$status" \
    --arg job_fingerprint "$job_fingerprint" \
    --arg command_exit_code "$command_exit_code" \
    --arg rung "$rung_id" \
    --arg reference_set "$reference_set" \
    --arg reference_filter "$reference_filter" \
    --arg reference_filter_suffix "$reference_filter_suffix" \
    --arg case_name "$case_name" \
    --arg candidate_id "$candidate_id" \
    --arg abstraction_seed "$abstraction_seed" \
    --arg solver_seed "$solver_seed" \
    --arg evaluation_seed "$evaluation_seed" \
    --arg reference_id "$reference_id" \
    --arg samples "$samples" \
    --arg traversals "$traversals" \
    --arg sweeps "$sweeps" \
    --arg training_seed "$training_seed" \
    --arg profile "$profile" \
    --arg purify_threshold "$purify_threshold" \
    --arg manifest_path "$manifest_path" \
    --arg manifest_sha "$manifest_sha" \
    --arg experiment_metadata "$experiment_metadata" \
    --arg experiment_metadata_sha "$experiment_metadata_sha" \
    --arg solver_path "$solver_path" \
    --arg solver_sha "$solver_sha" \
    --arg candidate_config "$candidate_config" \
    --arg candidate_config_sha "$candidate_config_sha" \
    --arg checkpoint "$checkpoint" \
    --arg checkpoint_sha "$checkpoint_sha" \
    --arg reference_config "$reference_config" \
    --arg reference_config_sha "$reference_config_sha" \
    --arg reference_cache "$reference_cache" \
    --arg cache_input_sha "$cache_input_sha" \
    --arg cache_output_sha "$cache_output_sha" \
    --arg report_path "$report_path" \
    --arg report_sha "$report_sha" \
    --arg watchdog_meta "$watchdog_meta" \
    --arg wall_seconds "$wall_seconds" \
    --arg peak_rss_bytes "$peak_rss_bytes" \
    --arg rss_limit_bytes "$rss_limit_bytes" \
    --arg candidate_game "$candidate_game_fingerprint" \
    --arg candidate_abstraction "$candidate_abstraction_fingerprint" \
    --arg reference_abstraction "$reference_abstraction_fingerprint" \
    '{
      schema: "solvers.abstraction-optimization-evaluation-run/v1",
      status: $status,
      job_fingerprint: $job_fingerprint,
      command_exit_code: ($command_exit_code | tonumber),
      experiment: ({
        rung: $rung,
        reference_set: $reference_set,
        case: $case_name,
        candidate_id: $candidate_id,
        abstraction_seed: ($abstraction_seed | tonumber),
        solver_seed: ($solver_seed | tonumber),
        evaluation_seed: ($evaluation_seed | tonumber),
        reference_id: $reference_id,
        samples: ($samples | tonumber),
        deviator_traversals_per_seat: ($traversals | tonumber),
        sweeps: ($sweeps | tonumber),
        training_seed: ($training_seed | tonumber),
        profile: $profile,
        purify_threshold: ($purify_threshold | tonumber)
      } + (if $reference_filter_suffix == "" then {} else {
        reference_filter: $reference_filter,
        reference_filter_suffix: $reference_filter_suffix
      } end)),
      inputs: {
        manifest: {path: $manifest_path, sha256: $manifest_sha},
        experiment_metadata: {
          path: $experiment_metadata,
          sha256: $experiment_metadata_sha
        },
        solver: {path: $solver_path, sha256: $solver_sha},
        candidate_config: {path: $candidate_config, sha256: $candidate_config_sha},
        checkpoint: {path: $checkpoint, sha256: $checkpoint_sha},
        reference_config: {path: $reference_config, sha256: $reference_config_sha}
      },
      reference_cache: {
        path: $reference_cache,
        input_sha256: $cache_input_sha,
        output_sha256: $cache_output_sha
      },
      artifacts: {
        report: $report_path,
        report_sha256: (if $report_sha == "missing" then null else $report_sha end),
        watchdog_meta: $watchdog_meta
      },
      resources: {
        wall_seconds: (if $wall_seconds == "" then null else ($wall_seconds | tonumber) end),
        peak_rss_bytes: (if $peak_rss_bytes == "" then null else ($peak_rss_bytes | tonumber) end),
        rss_limit_bytes: ($rss_limit_bytes | tonumber)
      },
      fingerprints: {
        candidate_game: $candidate_game,
        candidate_abstraction: $candidate_abstraction,
        reference_abstraction:
          (if $reference_abstraction == "" then null else $reference_abstraction end)
      }
    }' >"$tmp_path"
  mv "$tmp_path" "$meta_path"
}

validate_job_meta_conditions() {
  local meta_path=$1
  local expected_rung=$2
  local expected_sweeps=$3
  local expected_samples=$4
  local expected_evaluation_seed=$5
  local expected_traversals=$6
  local expected_training_seed=$7
  local expected_profile=$8
  local expected_purify_threshold=$9
  shift 9
  local expected_candidate_game=$1
  local expected_reference_set=$2
  local expected_reference_filter=$3
  local expected_reference_filter_suffix=$4

  jq -e \
    --arg rung "$expected_rung" \
    --argjson sweeps "$expected_sweeps" \
    --argjson samples "$expected_samples" \
    --argjson evaluation_seed "$expected_evaluation_seed" \
    --argjson traversals "$expected_traversals" \
    --argjson training_seed "$expected_training_seed" \
    --arg profile "$expected_profile" \
    --argjson purify_threshold "$expected_purify_threshold" \
    --arg candidate_game "$expected_candidate_game" \
    --arg reference_set "$expected_reference_set" \
    --arg reference_filter "$expected_reference_filter" \
    --arg reference_filter_suffix "$expected_reference_filter_suffix" \
    '
      .schema == "solvers.abstraction-optimization-evaluation-run/v1"
      and .experiment.rung == $rung
      and .experiment.reference_set == $reference_set
      and .experiment.sweeps == $sweeps
      and .experiment.samples == $samples
      and .experiment.evaluation_seed == $evaluation_seed
      and .experiment.deviator_traversals_per_seat == $traversals
      and .experiment.training_seed == $training_seed
      and .experiment.profile == $profile
      and .experiment.purify_threshold == $purify_threshold
      and .fingerprints.candidate_game == $candidate_game
      and (
        if $reference_filter_suffix == "" then
          (.experiment.reference_filter // ".*") == ".*"
          and (.experiment.reference_filter_suffix // "") == ""
        else
          .experiment.reference_filter == $reference_filter
          and .experiment.reference_filter_suffix == $reference_filter_suffix
        end
      )
    ' "$meta_path" >/dev/null
}

append_summary_row() {
  local summary_path=$1
  local meta_path=$2
  local include_reference_selection=$3
  jq -r --argjson include_reference_selection "$include_reference_selection" '
    ([
      .experiment.rung,
      .experiment.reference_set,
      .experiment.case,
      .experiment.candidate_id,
      .experiment.abstraction_seed,
      .experiment.solver_seed,
      .experiment.evaluation_seed,
      .experiment.reference_id,
      .status,
      .experiment.samples,
      .experiment.deviator_traversals_per_seat,
      .resources.wall_seconds,
      .resources.peak_rss_bytes,
      .reference_cache.input_sha256,
      .reference_cache.output_sha256,
      .fingerprints.candidate_game,
      .fingerprints.candidate_abstraction,
      .fingerprints.reference_abstraction,
      .artifacts.report,
      input_filename
    ] + if $include_reference_selection then [
      .experiment.reference_filter,
      .experiment.reference_filter_suffix
    ] else [] end) | @csv
  ' "$meta_path" >>"$summary_path"
}

if (( $# < 2 )); then
  usage
  exit 2
fi

rung_id=$1
result_root=$2
shift 2

candidate_regex='.*'
if (( $# > 0 )) && [[ "$1" != --* ]]; then
  candidate_regex=$1
  shift
fi

workspace=$(cd "$(dirname "$0")/../.." && pwd -P)
manifest="$workspace/experiments/abstraction-optimization-2026-07-25/manifest.toml"
watchdog="$workspace/experiments/abstraction-optimization-2026-07-25/run-with-rss-watchdog.sh"
coverage_gate="$workspace/experiments/abstraction-optimization-2026-07-25/aggregate-coverage-gates.py"
research_target_dir="$workspace/target/research-release"
solver="$research_target_dir/release/solvers"
solver_overridden=0
samples_override=
traversals_override=
seed_pairs_override=
evaluation_seed_override=
reference_set=
reference_set_explicit=0
reference_filter='.*'
reference_filter_explicit=0
reference_filter_suffix=
rss_limit_override=
coverage_candidate_stored_min=
coverage_candidate_postflop_stored_min=
coverage_candidate_postflop_min_visits=
plan_only=0

while (( $# > 0 )); do
  case "$1" in
    --samples)
      (( $# >= 2 )) || fail "--samples requires a value"
      samples_override=$2
      shift 2
      ;;
    --br-traversals)
      (( $# >= 2 )) || fail "--br-traversals requires a value"
      traversals_override=$2
      shift 2
      ;;
    --seed-pairs)
      (( $# >= 2 )) || fail "--seed-pairs requires a value"
      seed_pairs_override=$2
      shift 2
      ;;
    --evaluation-seed)
      (( $# >= 2 )) || fail "--evaluation-seed requires a value"
      evaluation_seed_override=$2
      shift 2
      ;;
    --reference-set)
      (( $# >= 2 )) || fail "--reference-set requires a value"
      reference_set=$2
      reference_set_explicit=1
      shift 2
      ;;
    --reference-filter)
      (( $# >= 2 )) || fail "--reference-filter requires a value"
      reference_filter=$2
      reference_filter_explicit=1
      shift 2
      ;;
    --rss-limit-bytes)
      (( $# >= 2 )) || fail "--rss-limit-bytes requires a value"
      rss_limit_override=$2
      shift 2
      ;;
    --coverage-candidate-stored-min)
      (( $# >= 2 )) || fail "--coverage-candidate-stored-min requires a value"
      coverage_candidate_stored_min=$2
      shift 2
      ;;
    --coverage-candidate-postflop-stored-min)
      (( $# >= 2 )) ||
        fail "--coverage-candidate-postflop-stored-min requires a value"
      coverage_candidate_postflop_stored_min=$2
      shift 2
      ;;
    --coverage-candidate-postflop-min-visits)
      (( $# >= 2 )) ||
        fail "--coverage-candidate-postflop-min-visits requires a value"
      coverage_candidate_postflop_min_visits=$2
      shift 2
      ;;
    --solver)
      (( $# >= 2 )) || fail "--solver requires a path"
      solver=$2
      solver_overridden=1
      shift 2
      ;;
    --manifest)
      (( $# >= 2 )) || fail "--manifest requires a path"
      manifest=$2
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

if (( solver_overridden == 0 )); then
  command -v cargo >/dev/null 2>&1 || fail "required command is unavailable: cargo"
  (
    cd "$workspace"
    CARGO_TARGET_DIR="$research_target_dir" \
      cargo build --quiet --locked --release -p cli --features research --bin solvers
  )
fi
manifest=$(absolute_existing_file "$manifest") || fail "missing manifest: $manifest"
solver=$(absolute_existing_file "$solver") || fail "missing solver: $solver"
watchdog=$(absolute_existing_file "$watchdog") || fail "missing watchdog: $watchdog"
coverage_gate=$(absolute_existing_file "$coverage_gate") ||
  fail "missing coverage gate: $coverage_gate"
[[ -x "$solver" ]] || fail "solver is not executable: $solver"
[[ -x "$watchdog" ]] || fail "watchdog is not executable: $watchdog"
[[ -x "$coverage_gate" ]] || fail "coverage gate is not executable: $coverage_gate"
if (( solver_overridden == 0 )); then
  "$solver" experiment --help >/dev/null 2>&1 ||
    fail "solver does not expose the research experiment namespace"
fi

mkdir -p "$result_root"
result_root=$(cd "$result_root" && pwd -P)
config_index="$result_root/configs/configs.csv"
[[ -f "$config_index" ]] ||
  fail "missing generated config index $config_index; generate configs or run the solve rung first"

expected_header='role,case,id,abstraction_seed,solver_seed,evaluation_seed,config,cache,config_hash,game_fingerprint'
IFS= read -r actual_header <"$config_index"
[[ "$actual_header" == "$expected_header" ]] ||
  fail "unsupported configs.csv header: $actual_header"

if ! rung_values=$(manifest_rung_values "$manifest" "$rung_id"); then
  fail "manifest has no complete, unique rung named $rung_id"
fi
IFS='|' read -r target_sweeps manifest_seed_pairs manifest_samples manifest_traversals \
  <<<"$rung_values"
samples=${samples_override:-$manifest_samples}
traversals=${traversals_override:-$manifest_traversals}
seed_pairs=${seed_pairs_override:-$manifest_seed_pairs}
if ! rss_limit=$(manifest_rss_limit "$manifest"); then
  fail "manifest selection.local_rss_stop_bytes is missing or duplicated"
fi
rss_limit=${rss_limit_override:-$rss_limit}

for numeric in "$target_sweeps" "$samples" "$traversals" "$seed_pairs" "$rss_limit"; do
  is_positive_integer "$numeric" || fail "expected a positive integer, got $numeric"
done
if [[ -n "$evaluation_seed_override" ]] &&
  ! [[ "$evaluation_seed_override" =~ ^[0-9]+$ ]]; then
  fail "--evaluation-seed must be a nonnegative integer"
fi
if [[ -n "$coverage_candidate_stored_min" ]]; then
  [[ "$coverage_candidate_stored_min" =~ ^([0-9]+([.][0-9]*)?|[.][0-9]+)$ ]] ||
    fail "--coverage-candidate-stored-min must be numeric"
  awk -v value="$coverage_candidate_stored_min" \
    'BEGIN { exit !(value >= 0.0 && value <= 1.0) }' ||
    fail "--coverage-candidate-stored-min must be within [0, 1]"
fi
if [[ -n "$coverage_candidate_postflop_stored_min" ]]; then
  [[ "$coverage_candidate_postflop_stored_min" =~ ^([0-9]+([.][0-9]*)?|[.][0-9]+)$ ]] ||
    fail "--coverage-candidate-postflop-stored-min must be numeric"
  awk -v value="$coverage_candidate_postflop_stored_min" \
    'BEGIN { exit !(value >= 0.0 && value <= 1.0) }' ||
    fail "--coverage-candidate-postflop-stored-min must be within [0, 1]"
fi
if [[ -n "$coverage_candidate_postflop_min_visits" ]] &&
  ! [[ "$coverage_candidate_postflop_min_visits" =~ ^[0-9]+$ ]]; then
  fail "--coverage-candidate-postflop-min-visits must be a nonnegative integer"
fi
(( rss_limit <= 8589934592 )) || fail "RSS limit exceeds the local 8 GiB process ceiling"

if (( reference_set_explicit == 1 )); then
  case "$reference_set" in
    screening|final) ;;
    *) fail "--reference-set must be screening or final" ;;
  esac
fi
set +e
[[ "" =~ $candidate_regex ]]
regex_status=$?
set -e
(( regex_status != 2 )) || fail "invalid CANDIDATE_REGEX: $candidate_regex"
set +e
[[ "" =~ $reference_filter ]]
reference_regex_status=$?
set -e
(( reference_regex_status != 2 )) ||
  fail "invalid --reference-filter ERE: $reference_filter"
if (( reference_filter_explicit == 1 )); then
  reference_filter_hash=$(printf '%s' "$reference_filter" | hash_material)
  reference_filter_suffix="rf-${reference_filter_hash:0:16}"
fi

scratch_dir=$(mktemp -d /tmp/solvers-evaluation-harness.XXXXXX)
summary_tmp=
cleanup() {
  if [[ -n "${summary_tmp:-}" && -f "$summary_tmp" ]]; then
    rm -f -- "$summary_tmp"
  fi
  if [[ -n "${scratch_dir:-}" && -d "$scratch_dir" &&
    "$scratch_dir" == /tmp/solvers-evaluation-harness.* ]]; then
    rm -rf -- "$scratch_dir"
  fi
}
trap cleanup EXIT INT TERM

reference_classes="$scratch_dir/reference-classes.txt"
manifest_reference_classes "$manifest" >"$reference_classes"
selected_reference_ids="$scratch_dir/selected-reference-ids.txt"
experiment_metadata="$result_root/configs/experiment-metadata.json"
[[ -f "$experiment_metadata" ]] ||
  fail "missing generated experiment metadata: $experiment_metadata"
if ! jq -e --arg rung "$rung_id" '
  .schema == "solvers.abstraction-optimization/v1"
  and (.referenceRouting | type == "array" and length > 0)
  and ([.referenceRouting[].id] | length == (unique | length))
  and all(.referenceRouting[];
    (.id | type == "string" and length > 0)
    and (.finalOnly | type == "boolean")
    and (.rungs | type == "array" and length > 0)
    and all(.rungs[]; type == "string" and length > 0))
  and any(.referenceRouting[]; .rungs | index($rung) != null)
' "$experiment_metadata" >/dev/null; then
  fail "experiment metadata has no valid reference routing for rung $rung_id"
fi
manifest_reference_ids="$scratch_dir/manifest-reference-ids.txt"
routed_reference_ids="$scratch_dir/routed-reference-ids.txt"
awk -F'|' '{ print $1 }' "$reference_classes" | LC_ALL=C sort >"$manifest_reference_ids"
jq -r '.referenceRouting[].id' "$experiment_metadata" |
  LC_ALL=C sort >"$routed_reference_ids"
cmp -s "$manifest_reference_ids" "$routed_reference_ids" ||
  fail "manifest references and generated experiment-metadata referenceRouting disagree"

if (( reference_set_explicit == 0 )); then
  reference_set=rung
  jq -r --arg rung "$rung_id" \
    '.referenceRouting[] | select(.rungs | index($rung) != null) | .id' \
    "$experiment_metadata" >"$selected_reference_ids"
elif [[ "$reference_set" == "final" ]]; then
  jq -r '.referenceRouting[].id' "$experiment_metadata" >"$selected_reference_ids"
else
  jq -r '.referenceRouting[] | select(.finalOnly | not) | .id' \
    "$experiment_metadata" \
    >"$selected_reference_ids"
fi
[[ -s "$selected_reference_ids" ]] ||
  fail "manifest has no $reference_set references"
if (( reference_filter_explicit == 1 )); then
  filtered_reference_ids="$scratch_dir/filtered-reference-ids.txt"
  while IFS= read -r reference_id; do
    if [[ "$reference_id" =~ $reference_filter ]]; then
      printf '%s\n' "$reference_id"
    fi
  done <"$selected_reference_ids" >"$filtered_reference_ids"
  mv "$filtered_reference_ids" "$selected_reference_ids"
  [[ -s "$selected_reference_ids" ]] ||
    fail "--reference-filter matched no selected $reference_set references"
fi

selected_seed_tuples="$scratch_dir/selected-seed-tuples.txt"
manifest_seed_tuples "$manifest" |
  awk -v count="$seed_pairs" 'NR <= count { print }' >"$selected_seed_tuples"
actual_seed_pairs=$(wc -l <"$selected_seed_tuples" | tr -d '[:space:]')
[[ "$actual_seed_pairs" == "$seed_pairs" ]] ||
  fail "requested $seed_pairs seed pairs, but manifest has only $actual_seed_pairs"

selected_reference_rows="$scratch_dir/selected-reference-rows.csv"
while IFS=, read -r role case_name model_id abstraction_seed row_solver_seed \
  row_evaluation_seed config_path cache_path config_hash game_fingerprint; do
  [[ "$role" == "reference" ]] || continue
  value_is_listed "$model_id" "$selected_reference_ids" || continue
  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$role" "$case_name" "$model_id" "$abstraction_seed" "$row_solver_seed" \
    "$row_evaluation_seed" "$config_path" "$cache_path" "$config_hash" \
    "$game_fingerprint" >>"$selected_reference_rows"
done < <(tail -n +2 "$config_index")
[[ -s "$selected_reference_rows" ]] ||
  fail "configs.csv has no rows for the selected $reference_set references"

while IFS= read -r reference_id; do
  for case_name in tournament cash; do
    matches=$(awk -F, -v id="$reference_id" -v case_name="$case_name" \
      '$2 == case_name && $3 == id { count++ } END { print count + 0 }' \
      "$selected_reference_rows")
    [[ "$matches" == "1" ]] ||
      fail "expected exactly one $case_name configs.csv row for reference $reference_id, got $matches"
  done
done <"$selected_reference_ids"

manifest_sha=$(sha256_file "$manifest")
experiment_metadata_sha=$(sha256_file "$experiment_metadata")
solver_sha=$(sha256_file "$solver")
summary_stem="${rung_id}-${reference_set}"
if (( reference_filter_explicit == 1 )); then
  summary_stem="${summary_stem}-${reference_filter_suffix}"
fi
summary_path="$result_root/${summary_stem}-evaluation-summary.csv"
summary_tmp="${summary_path}.tmp.$$"
summary_header='rung,reference_set,case,candidate_id,abstraction_seed,solver_seed,evaluation_seed,reference_id,status,samples,deviator_traversals_per_seat,wall_seconds,peak_rss_bytes,cache_input_sha256,cache_output_sha256,candidate_game_fingerprint,candidate_abstraction_fingerprint,reference_abstraction_fingerprint,report,meta'
if (( reference_filter_explicit == 1 )); then
  summary_header="${summary_header},reference_filter,reference_filter_suffix"
fi
printf '%s\n' "$summary_header" >"$summary_tmp"

jobs=0
while IFS=, read -r role case_name candidate_id abstraction_seed row_solver_seed \
  row_evaluation_seed candidate_config candidate_cache candidate_config_hash \
  candidate_game_fingerprint; do
  [[ "$role" == "candidate" ]] || continue
  value_is_listed \
    "$abstraction_seed|$row_solver_seed|$row_evaluation_seed" \
    "$selected_seed_tuples" || continue
  if ! [[ "$candidate_id" =~ $candidate_regex ]]; then
    continue
  fi

  run_id="${candidate_id}-a${abstraction_seed}-s${row_solver_seed}"
  run_dir="$result_root/runs/$case_name/$run_id"
  run_json="$run_dir/run.json"
  checkpoint="$run_dir/checkpoint.mwckpt"
  [[ -f "$candidate_config" ]] || fail "missing candidate config: $candidate_config"
  [[ -f "$run_json" ]] || fail "missing solve result: $run_json"
  [[ -f "$checkpoint" ]] || fail "missing checkpoint: $checkpoint"
  checkpoint=$(absolute_existing_file "$checkpoint")

  if ! jq -e --argjson target "$target_sweeps" \
    '.status == "completed" and .sweeps == $target and (.seats | length > 0)' \
    "$run_json" >/dev/null; then
    fail "$run_json is not a completed solve with exactly $target_sweeps sweeps"
  fi
  if [[ "$(jq -r '.configHash' "$run_json")" != "$candidate_config_hash" ]]; then
    fail "candidate config fingerprint mismatch between configs.csv and $run_json"
  fi
  if [[ "$(jq -r '.gameFingerprint' "$run_json")" != "$candidate_game_fingerprint" ]]; then
    fail "candidate game fingerprint mismatch between configs.csv and $run_json"
  fi

  candidate_abstraction_fingerprint=$(jq -r '.abstractionFingerprint' "$run_json")
  candidate_configuration_fingerprint=$(jq -r '.configurationFingerprint' "$run_json")
  expected_seats=$(jq -r '.seats | length' "$run_json")
  candidate_config_sha=$(sha256_file "$candidate_config")
  checkpoint_sha=$(sha256_file "$checkpoint")
  evaluation_seed=${evaluation_seed_override:-$row_evaluation_seed}
  expected_profile=average
  expected_purify_threshold=0
  if ! expected_training_seed=$(derive_training_seed "$evaluation_seed"); then
    fail "evaluation seed is not a valid u64: $evaluation_seed"
  fi

  while IFS=, read -r reference_role reference_case reference_id \
    reference_abstraction_seed reference_solver_seed reference_evaluation_seed \
    reference_config reference_cache reference_config_hash \
    reference_game_fingerprint; do
    [[ "$reference_role" == "reference" && "$reference_case" == "$case_name" ]] ||
      continue
    ((jobs += 1))

    [[ -f "$reference_config" ]] || fail "missing reference config: $reference_config"
    [[ "$reference_game_fingerprint" == "$candidate_game_fingerprint" ]] ||
      fail "candidate/reference game fingerprints differ for $run_id/$reference_id"
    expected_reference_recall=$(awk -F'|' -v wanted="$reference_id" \
      '$1 == wanted { print $3 }' "$reference_classes")
    [[ "$expected_reference_recall" == "full" ||
      "$expected_reference_recall" == "street" ]] ||
      fail "manifest reference $reference_id has invalid recall $expected_reference_recall"
    if ! actual_reference_recall=$(config_abstraction_recall "$reference_config"); then
      fail "reference config has no unique [game.abstraction] recall: $reference_config"
    fi
    [[ "$actual_reference_recall" == "$expected_reference_recall" ]] ||
      fail \
        "reference recall mismatch for $reference_id: manifest=$expected_reference_recall config=$actual_reference_recall"
    reference_config_sha=$(sha256_file "$reference_config")
    job_material=$(
      printf '%s\n' \
        'solvers.abstraction-optimization-evaluation-job/v1' \
        "manifest_sha256=$manifest_sha" \
        "experiment_metadata_sha256=$experiment_metadata_sha" \
        "solver_sha256=$solver_sha" \
        "rung=$rung_id" \
        "reference_set=$reference_set" \
        "case=$case_name" \
        "candidate=$candidate_id" \
        "abstraction_seed=$abstraction_seed" \
        "solver_seed=$row_solver_seed" \
        "evaluation_seed=$evaluation_seed" \
        "reference=$reference_id" \
        "samples=$samples" \
        "traversals=$traversals" \
        "sweeps=$target_sweeps" \
        "training_seed=$expected_training_seed" \
        "profile=$expected_profile" \
        "purify_threshold=$expected_purify_threshold" \
        "rss_limit=$rss_limit" \
        "candidate_config_sha256=$candidate_config_sha" \
        "checkpoint_sha256=$checkpoint_sha" \
        "reference_config_sha256=$reference_config_sha"
    )
    if (( reference_filter_explicit == 1 )); then
      job_material="${job_material}"$'\n'"reference_filter=$reference_filter"$'\n'"reference_filter_suffix=$reference_filter_suffix"
    fi
    job_fingerprint=$(printf '%s' "$job_material" | hash_material)
    job_dir="$result_root/evaluations/$rung_id/$case_name/$run_id/$reference_id/$job_fingerprint"
    mkdir -p "$job_dir"
    report="$job_dir/report.json"
    meta="$job_dir/meta.json"
    watchdog_meta="$job_dir/watchdog.txt"
    stdout_log="$job_dir/stdout.log"
    stderr_log="$job_dir/stderr.log"

    recorded_report_sha=
    current_report_sha=
    if [[ -f "$meta" && -f "$report" ]]; then
      recorded_report_sha=$(jq -r '.artifacts.report_sha256 // empty' "$meta" 2>/dev/null || true)
      current_report_sha=$(sha256_file "$report")
    fi
    if [[ -f "$meta" && -f "$report" ]] &&
      [[ "$(jq -r '.status // empty' "$meta" 2>/dev/null)" == "completed" ]] &&
      [[ "$(jq -r '.job_fingerprint // empty' "$meta" 2>/dev/null)" == "$job_fingerprint" ]] &&
      validate_job_meta_conditions \
        "$meta" "$rung_id" "$target_sweeps" "$samples" "$evaluation_seed" \
        "$traversals" "$expected_training_seed" "$expected_profile" \
        "$expected_purify_threshold" "$candidate_game_fingerprint" \
        "$reference_set" "$reference_filter" "$reference_filter_suffix" &&
      [[ "$recorded_report_sha" == "$current_report_sha" ]] &&
      validate_report \
        "$report" "$candidate_config" "$checkpoint" "$candidate_config_hash" \
        "$candidate_game_fingerprint" "$candidate_abstraction_fingerprint" \
        "$candidate_configuration_fingerprint" "$reference_config" \
        "$reference_config_hash" "$reference_game_fingerprint" "$reference_cache" \
        "$samples" "$evaluation_seed" "$traversals" "$expected_seats" \
        "$expected_reference_recall" "$rung_id" "$target_sweeps" \
        "$expected_training_seed" "$expected_profile" "$expected_purify_threshold"; then
      echo "SKIP rung=$rung_id candidate=$run_id reference=$reference_id"
      append_summary_row \
        "$summary_tmp" "$meta" "$reference_filter_explicit"
      continue
    fi

    if (( plan_only == 1 )); then
      echo \
        "PLAN rung=$rung_id candidate=$run_id reference=$reference_id samples=$samples traversals=$traversals seed=$evaluation_seed"
      continue
    fi

    echo \
      "START rung=$rung_id candidate=$run_id reference=$reference_id samples=$samples traversals=$traversals seed=$evaluation_seed"
    cache_input_sha=$(sha256_file_or_missing "$reference_cache")
    next_report="$job_dir/report.next.$$.json"
    set +e
    "$watchdog" \
      "$rss_limit" "$watchdog_meta" "$stdout_log" "$stderr_log" \
      "$solver" experiment profile "$candidate_config" \
      --checkpoint "$checkpoint" \
      --deviator-config "$reference_config" \
      --output "$next_report" \
      --experiment-rung "$rung_id" \
      --samples "$samples" \
      --seed "$evaluation_seed" \
      --purify 0.0 \
      --br-traversals "$traversals"
    command_exit_code=$?
    set -e

    cache_output_sha=$(sha256_file_or_missing "$reference_cache")
    wall_seconds=$(awk -F= '$1 == "wall_seconds" { print $2 }' "$watchdog_meta" 2>/dev/null || true)
    peak_rss_bytes=$(awk -F= '$1 == "peak_rss_bytes" { print $2 }' "$watchdog_meta" 2>/dev/null || true)
    if (( command_exit_code != 0 )); then
      write_job_meta \
        "$meta" "command_failed" "$job_fingerprint" "$command_exit_code" \
        "$rung_id" "$reference_set" "$case_name" "$candidate_id" "$abstraction_seed" \
        "$row_solver_seed" "$evaluation_seed" "$reference_id" "$samples" "$traversals" \
        "$target_sweeps" "$expected_training_seed" "$expected_profile" \
        "$expected_purify_threshold" \
        "$manifest" "$manifest_sha" "$solver" "$solver_sha" \
        "$candidate_config" "$candidate_config_sha" "$checkpoint" "$checkpoint_sha" \
        "$reference_config" "$reference_config_sha" "$reference_cache" \
        "$cache_input_sha" "$cache_output_sha" "$report" "missing" "$watchdog_meta" \
        "$wall_seconds" "$peak_rss_bytes" "$rss_limit" "$candidate_game_fingerprint" \
        "$candidate_abstraction_fingerprint" "" \
        "$reference_filter" "$reference_filter_suffix"
      append_summary_row \
        "$summary_tmp" "$meta" "$reference_filter_explicit"
      mv "$summary_tmp" "$summary_path"
      summary_tmp=
      echo \
        "FAILED rung=$rung_id candidate=$run_id reference=$reference_id exit=$command_exit_code; see $stderr_log" \
        >&2
      exit "$command_exit_code"
    fi

    if [[ ! -f "$next_report" ]] ||
      ! validate_report \
        "$next_report" "$candidate_config" "$checkpoint" "$candidate_config_hash" \
        "$candidate_game_fingerprint" "$candidate_abstraction_fingerprint" \
        "$candidate_configuration_fingerprint" "$reference_config" \
        "$reference_config_hash" "$reference_game_fingerprint" "$reference_cache" \
        "$samples" "$evaluation_seed" "$traversals" "$expected_seats" \
        "$expected_reference_recall" "$rung_id" "$target_sweeps" \
        "$expected_training_seed" "$expected_profile" "$expected_purify_threshold"; then
      write_job_meta \
        "$meta" "invalid_report" "$job_fingerprint" "3" \
        "$rung_id" "$reference_set" "$case_name" "$candidate_id" "$abstraction_seed" \
        "$row_solver_seed" "$evaluation_seed" "$reference_id" "$samples" "$traversals" \
        "$target_sweeps" "$expected_training_seed" "$expected_profile" \
        "$expected_purify_threshold" \
        "$manifest" "$manifest_sha" "$solver" "$solver_sha" \
        "$candidate_config" "$candidate_config_sha" "$checkpoint" "$checkpoint_sha" \
        "$reference_config" "$reference_config_sha" "$reference_cache" \
        "$cache_input_sha" "$cache_output_sha" "$report" "missing" "$watchdog_meta" \
        "$wall_seconds" "$peak_rss_bytes" "$rss_limit" "$candidate_game_fingerprint" \
        "$candidate_abstraction_fingerprint" "" \
        "$reference_filter" "$reference_filter_suffix"
      append_summary_row \
        "$summary_tmp" "$meta" "$reference_filter_explicit"
      mv "$summary_tmp" "$summary_path"
      summary_tmp=
      echo \
        "INVALID rung=$rung_id candidate=$run_id reference=$reference_id; see $next_report" \
        >&2
      exit 3
    fi

    mv "$next_report" "$report"
    report_sha=$(sha256_file "$report")
    reference_abstraction_fingerprint=$(jq -r '.reference.abstraction_fingerprint' "$report")
    write_job_meta \
      "$meta" "completed" "$job_fingerprint" "0" \
      "$rung_id" "$reference_set" "$case_name" "$candidate_id" "$abstraction_seed" \
      "$row_solver_seed" "$evaluation_seed" "$reference_id" "$samples" "$traversals" \
      "$target_sweeps" "$expected_training_seed" "$expected_profile" \
      "$expected_purify_threshold" \
      "$manifest" "$manifest_sha" "$solver" "$solver_sha" \
      "$candidate_config" "$candidate_config_sha" "$checkpoint" "$checkpoint_sha" \
      "$reference_config" "$reference_config_sha" "$reference_cache" \
      "$cache_input_sha" "$cache_output_sha" "$report" "$report_sha" "$watchdog_meta" \
      "$wall_seconds" "$peak_rss_bytes" "$rss_limit" "$candidate_game_fingerprint" \
      "$candidate_abstraction_fingerprint" "$reference_abstraction_fingerprint" \
      "$reference_filter" "$reference_filter_suffix"
    append_summary_row \
      "$summary_tmp" "$meta" "$reference_filter_explicit"
    echo \
      "RESULT rung=$rung_id candidate=$run_id reference=$reference_id wall=${wall_seconds}s peak_rss=${peak_rss_bytes} report=$report"
  done <"$selected_reference_rows"
done < <(tail -n +2 "$config_index")

(( jobs > 0 )) || fail "no candidate rows matched the selected seed pairs and regex"
if (( plan_only == 1 )); then
  rm -f -- "$summary_tmp"
  summary_tmp=
  echo "planned $jobs serial evaluation jobs"
  exit 0
fi
mv "$summary_tmp" "$summary_path"
summary_tmp=
echo "wrote $summary_path"
coverage_args=(
  "$result_root" "$rung_id" "$candidate_regex"
  --reference-set "$reference_set"
  --seed-pairs "$seed_pairs"
  --summary "$summary_path"
  --output-json "$result_root/${summary_stem}-coverage-gates.json"
  --output-csv "$result_root/${summary_stem}-coverage-gates.csv"
)
if (( reference_filter_explicit == 1 )); then
  coverage_args+=(--reference-filter "$reference_filter")
fi
if [[ -n "$evaluation_seed_override" ]]; then
  coverage_args+=(--evaluation-seed "$evaluation_seed_override")
fi
if [[ -n "$coverage_candidate_stored_min" ]]; then
  coverage_args+=(--candidate-stored-min "$coverage_candidate_stored_min")
fi
if [[ -n "$coverage_candidate_postflop_stored_min" ]]; then
  coverage_args+=(
    --candidate-postflop-stored-min "$coverage_candidate_postflop_stored_min"
  )
fi
if [[ -n "$coverage_candidate_postflop_min_visits" ]]; then
  coverage_args+=(
    --candidate-postflop-min-visits "$coverage_candidate_postflop_min_visits"
  )
fi
"$coverage_gate" "${coverage_args[@]}"
