#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 4 || $# -gt 5 ]]; then
  echo "usage: $0 RUNG_ID TARGET_SWEEPS SOLVER_SEED RESULT_ROOT [CANDIDATE_REGEX]" >&2
  exit 2
fi

rung_id=$1
target_sweeps=$2
solver_seed=$3
result_root=$4
candidate_regex=${5:-'.*'}

if [[ ! "$rung_id" =~ ^[[:alnum:]_.-]+$ ]]; then
  echo "RUNG_ID must use only letters, digits, '.', '_', or '-'" >&2
  exit 2
fi
for numeric in target_sweeps solver_seed; do
  value=${!numeric}
  case "$value" in
    ''|*[!0-9]*)
      echo "$numeric must be a nonnegative integer" >&2
      exit 2
      ;;
  esac
done
if (( target_sweeps == 0 )); then
  echo "TARGET_SWEEPS must be positive" >&2
  exit 2
fi
set +e
[[ "" =~ $candidate_regex ]]
regex_status=$?
set -e
if (( regex_status == 2 )); then
  echo "CANDIDATE_REGEX is not a valid extended regular expression" >&2
  exit 2
fi

for command in cargo jq awk tail ps; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "required command is unavailable: $command" >&2
    exit 2
  fi
done

workspace=$(cd "$(dirname "$0")/../.." && pwd -P)
manifest="$workspace/experiments/abstraction-optimization-2026-07-25/manifest.toml"
watchdog="$workspace/experiments/abstraction-optimization-2026-07-25/run-with-rss-watchdog.sh"
research_target_dir="$workspace/target/research-release"
solver="$research_target_dir/release/solvers"

mkdir -p "$result_root"
result_root=$(cd "$result_root" && pwd -P)
lock_dir="$result_root/.solve-rung.lock"
if ! mkdir "$lock_dir" 2>/dev/null; then
  echo \
    "another solve-rung process appears to own $lock_dir; \
run experiment processes serially" \
    >&2
  exit 2
fi
summary_tmp=
cleanup_runner() {
  if [[ -n "${summary_tmp:-}" && -f "$summary_tmp" ]]; then
    rm -f -- "$summary_tmp"
  fi
  if [[ -n "${lock_dir:-}" && -d "$lock_dir" ]]; then
    rmdir -- "$lock_dir" 2>/dev/null || true
  fi
}
trap cleanup_runner EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

config_dir="$result_root/configs"
cache_dir="$result_root/cache"
index_path="$config_dir/configs.csv"
experiment_metadata="$config_dir/experiment-metadata.json"
mkdir -p "$config_dir" "$cache_dir"

if [[ ! -x "$watchdog" ]]; then
  echo "RSS watchdog is not executable: $watchdog" >&2
  exit 2
fi

# Always rebuild the opt-in research binary in a target directory that cannot
# overwrite the production release, then regenerate the deterministic configs.
# This prevents a stale binary, manifest, or generated index from being
# silently paired with existing checkpoints/results.
(
  cd "$workspace"
  CARGO_TARGET_DIR="$research_target_dir" \
    cargo build --quiet --release -p cli --features research --bin solvers
  CARGO_TARGET_DIR="$research_target_dir" \
    cargo run --quiet --release -p cli --features research \
    --example generate_abstraction_optimization_configs -- \
    "$manifest" "$config_dir" "$cache_dir"
)

if [[ ! -x "$solver" || ! -f "$index_path" || ! -f "$experiment_metadata" ]]; then
  echo "build/config generation did not produce the required experiment inputs" >&2
  exit 2
fi
expected_index_header='role,case,id,abstraction_seed,solver_seed,evaluation_seed,config,cache,config_hash,game_fingerprint'
IFS= read -r actual_index_header <"$index_path"
if [[ "$actual_index_header" != "$expected_index_header" ]]; then
  echo "unsupported generated config index header: $actual_index_header" >&2
  exit 2
fi
if ! jq -e \
  '.schema == "solvers.abstraction-optimization/v1"' \
  "$experiment_metadata" >/dev/null; then
  echo "generated experiment metadata has the wrong schema" >&2
  exit 2
fi

manifest_target_sweeps=$(
  jq -er --arg rung "$rung_id" \
    '.rungs[] | select(.id == $rung) | .sweeps' \
    "$experiment_metadata"
)
if [[ "$manifest_target_sweeps" != "$target_sweeps" ]]; then
  echo \
    "rung $rung_id requires TARGET_SWEEPS=$manifest_target_sweeps, got $target_sweeps" \
    >&2
  exit 2
fi
rung_seed_pairs=$(
  jq -er --arg rung "$rung_id" \
    '.rungs[] | select(.id == $rung) | .seed_pairs' \
    "$experiment_metadata"
)
if ! jq -e \
  --arg seed "$solver_seed" \
  --argjson count "$rung_seed_pairs" \
  '.seedPairs[0:$count] | any((.solver | tostring) == $seed)' \
  "$experiment_metadata" >/dev/null; then
  echo "solver seed $solver_seed is not selected by rung $rung_id" >&2
  exit 2
fi
rss_limit_bytes=$(jq -er '.selection.local_rss_stop_bytes' "$experiment_metadata")
case "$rss_limit_bytes" in
  ''|*[!0-9]*)
    echo "generated metadata contains an invalid local RSS limit" >&2
    exit 2
    ;;
esac

summary_path="$result_root/${rung_id}-s${solver_seed}-solve-summary.csv"
summary_tmp="$result_root/.${rung_id}-s${solver_seed}-solve-summary.csv.tmp.$$"
echo \
  "rung,case,id,abstraction_seed,solver_seed,evaluation_seed,target_sweeps,status,sweeps,infosets,solver_memory_bytes,solver_elapsed_seconds,segment_source,segment_wall_seconds,segment_peak_rss_bytes,game_fingerprint,abstraction_fingerprint,config_hash,config,cache" \
  >"$summary_tmp"

selected_count=0
while IFS=, read -r \
  role \
  case_name \
  candidate_id \
  abstraction_seed \
  row_solver_seed \
  evaluation_seed \
  config_path \
  cache_path \
  expected_config_hash \
  expected_game_fingerprint; do
  if [[ "$role" != "candidate" || "$row_solver_seed" != "$solver_seed" ]]; then
    continue
  fi
  selected_for_execution=0
  if [[ "$candidate_id" =~ $candidate_regex ]]; then
    selected_for_execution=1
    selected_count=$((selected_count + 1))
  fi
  run_id="${candidate_id}-a${abstraction_seed}-s${row_solver_seed}"
  run_dir="$result_root/runs/$case_name/$run_id"
  result_json="$run_dir/run.json"
  progress_jsonl="$run_dir/progress.jsonl"
  checkpoint="$run_dir/checkpoint.mwckpt"
  segment_meta="$run_dir/${rung_id}-watchdog.txt"
  segment_stdout="$run_dir/${rung_id}-stdout.log"
  segment_stderr="$run_dir/${rung_id}-stderr.log"

  if (( selected_for_execution == 0 )) && [[ ! -f "$result_json" ]]; then
    continue
  fi
  mkdir -p "$run_dir"
  completed_sweeps=0
  if [[ -f "$result_json" ]]; then
    actual_config_hash=$(jq -er '.configHash' "$result_json")
    actual_game_fingerprint=$(jq -er '.gameFingerprint' "$result_json")
    if [[ "$actual_config_hash" != "$expected_config_hash" ]]; then
      echo \
        "stale result for $run_id: config hash $actual_config_hash != $expected_config_hash; \
move the existing run directory aside before rerunning" \
        >&2
      exit 3
    fi
    if [[ "$actual_game_fingerprint" != "$expected_game_fingerprint" ]]; then
      echo \
        "game fingerprint mismatch for $run_id: \
$actual_game_fingerprint != $expected_game_fingerprint" \
        >&2
      exit 3
    fi
    completed_sweeps=$(jq -er '.sweeps' "$result_json")
    case "$completed_sweeps" in
      ''|*[!0-9]*)
        echo "result for $run_id has a non-integer sweep count" >&2
        exit 3
        ;;
    esac
    if (( completed_sweeps > target_sweeps )); then
      echo \
        "result for $run_id has advanced to $completed_sweeps sweeps, past rung \
$rung_id at $target_sweeps; refusing to relabel a later profile as this rung" \
        >&2
      exit 3
    fi
  fi
  if (( selected_for_execution == 0 && completed_sweeps < target_sweeps )); then
    continue
  fi

  segment_source=reused_without_segment_metrics
  if (( completed_sweeps < target_sweeps )); then
    segment_source=executed
    if [[ -f "$checkpoint" ]]; then
      (
        cd "$workspace"
        "$watchdog" \
          "$rss_limit_bytes" "$segment_meta" "$segment_stdout" "$segment_stderr" \
          "$solver" resume "$config_path" \
          --checkpoint "$checkpoint" \
          --max-sweeps "$target_sweeps" \
          --output "$result_json" \
          --metrics "$progress_jsonl"
      )
    else
      (
        cd "$workspace"
        "$watchdog" \
          "$rss_limit_bytes" "$segment_meta" "$segment_stdout" "$segment_stderr" \
          "$solver" solve "$config_path" \
          --iterations "$target_sweeps" \
          --output "$result_json" \
          --metrics "$progress_jsonl" \
          --checkpoint "$checkpoint"
      )
    fi
  elif [[ -f "$segment_meta" ]]; then
    segment_source=recorded
  fi

  if [[ ! -f "$result_json" ]]; then
    echo "candidate $run_id did not produce its result" >&2
    exit 1
  fi
  if [[ "$segment_source" == "executed" && ! -f "$segment_meta" ]]; then
    echo "candidate $run_id did not produce watchdog metadata" >&2
    exit 1
  fi

  actual_config_hash=$(jq -er '.configHash' "$result_json")
  actual_game_fingerprint=$(jq -er '.gameFingerprint' "$result_json")
  if [[ "$actual_config_hash" != "$expected_config_hash" ]]; then
    echo \
      "config hash mismatch for $run_id: $actual_config_hash != $expected_config_hash" \
      >&2
    exit 3
  fi
  if [[ "$actual_game_fingerprint" != "$expected_game_fingerprint" ]]; then
    echo \
      "game fingerprint mismatch for $run_id: \
$actual_game_fingerprint != $expected_game_fingerprint" \
      >&2
    exit 3
  fi
  status=$(jq -er '.status' "$result_json")
  sweeps=$(jq -er '.sweeps' "$result_json")
  if [[ "$status" != "completed" || "$sweeps" != "$target_sweeps" ]]; then
    echo "candidate $run_id stopped with status=$status sweeps=$sweeps" >&2
    exit 75
  fi

  segment_wall_seconds=
  segment_peak_rss_bytes=
  if [[ -f "$segment_meta" ]]; then
    segment_wall_seconds=$(awk -F= '$1 == "wall_seconds" { print $2 }' "$segment_meta")
    segment_peak_rss_bytes=$(awk -F= '$1 == "peak_rss_bytes" { print $2 }' "$segment_meta")
  fi
  jq -r \
    --arg rung "$rung_id" \
    --arg case_name "$case_name" \
    --arg candidate_id "$candidate_id" \
    --arg abstraction_seed "$abstraction_seed" \
    --arg solver_seed "$row_solver_seed" \
    --arg evaluation_seed "$evaluation_seed" \
    --arg target_sweeps "$target_sweeps" \
    --arg segment_source "$segment_source" \
    --arg wall "$segment_wall_seconds" \
    --arg rss "$segment_peak_rss_bytes" \
    --arg expected_config_hash "$expected_config_hash" \
    --arg config "$config_path" \
    --arg cache "$cache_path" \
    '[
      $rung,
      $case_name,
      $candidate_id,
      $abstraction_seed,
      $solver_seed,
      $evaluation_seed,
      $target_sweeps,
      .status,
      .sweeps,
      .infosets,
      .memoryBytes,
      .elapsedSecs,
      $segment_source,
      $wall,
      $rss,
      .gameFingerprint,
      .abstractionFingerprint,
      $expected_config_hash,
      $config,
      $cache
    ] | @csv' \
    "$result_json" >>"$summary_tmp"
done < <(tail -n +2 "$index_path")

if (( selected_count == 0 )); then
  echo \
    "no candidates matched solver seed $solver_seed and regex $candidate_regex" \
    >&2
  exit 2
fi

mv -f -- "$summary_tmp" "$summary_path"
summary_tmp=
rmdir -- "$lock_dir"
lock_dir=
trap - EXIT INT TERM HUP
echo "wrote $summary_path"
