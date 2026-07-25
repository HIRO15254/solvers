#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 10 || "$1" != "resume" || "$3" != "--checkpoint" ||
      "$5" != "--max-sweeps" || "$7" != "--output" ||
      "$9" != "--metrics" ]]; then
  echo "unexpected mock solver invocation: $*" >&2
  exit 2
fi

config=$2
checkpoint=$4
target_sweeps=$6
output=$8
metrics=${10}

candidate=$(awk -F= '$1 == "# mock_candidate" {print $2}' "$config")
game_fingerprint=$(awk -F= '$1 == "# mock_game_fingerprint" {print $2}' "$config")
abstraction_fingerprint=$(
  awk -F= '$1 == "# mock_abstraction_fingerprint" {print $2}' "$config"
)
configuration_fingerprint=$(
  awk -F= '$1 == "# mock_configuration_fingerprint" {print $2}' "$config"
)
source_sweeps=$(awk -F= '$1 == "# mock_source_sweeps" {print $2}' "$config")
internal_cap=$(
  awk '
    $0 == "[run]" {in_run=1; next}
    /^\[/ {in_run=0}
    in_run && $1 == "max_memory_bytes" {print $3}
  ' "$config"
)

[[ "$candidate" =~ ^(T-E64|C-E64)$ ]]
[[ "$game_fingerprint" =~ ^[0-9a-f]{64}$ ]]
[[ "$abstraction_fingerprint" =~ ^[0-9a-f]{64}$ ]]
[[ "$configuration_fingerprint" =~ ^[0-9a-f]{64}$ ]]
[[ "$source_sweeps" =~ ^[0-9]+$ ]]
[[ "$internal_cap" =~ ^[0-9]+$ ]]
[[ "$target_sweeps" =~ ^[0-9]+$ ]]

if [[ -n "${MOCK_RESOURCE_CEILING_CALL_LOG:-}" ]]; then
  printf '%s,%s,%s\n' "$candidate" "$internal_cap" "$target_sweeps" \
    >>"$MOCK_RESOURCE_CEILING_CALL_LOG"
fi

if [[ -n "${MOCK_RESOURCE_CEILING_SLEEP_SECONDS:-}" ]]; then
  sleep "$MOCK_RESOURCE_CEILING_SLEEP_SECONDS"
fi

if [[ "${MOCK_RESOURCE_CEILING_COMPLETE_CANDIDATE:-}" == "$candidate" ]]; then
  result_status=completed
  result_sweeps=$target_sweeps
  exit_code=0
else
  result_status=resource_limit
  result_sweeps=$((source_sweeps + 7))
  if (( result_sweeps >= target_sweeps )); then
    result_sweeps=$((target_sweeps - 1))
  fi
  exit_code=75
fi
result_memory=$((internal_cap - 1))
if [[ "$candidate" == "T-E64" ]]; then
  result_config_hash=$(printf 'b%.0s' {1..64})
else
  result_config_hash=$(printf 'd%.0s' {1..64})
fi

mkdir -p "$(dirname "$output")" "$(dirname "$metrics")"
printf 'mock checkpoint candidate=%s sweeps=%s\n' "$candidate" "$result_sweeps" \
  >>"$checkpoint"
printf '{"phase":"mock","sweeps":%s}\n' "$result_sweeps" >>"$metrics"

temporary="${output}.tmp.$$"
jq -n \
  --arg status "$result_status" \
  --arg sweeps "$result_sweeps" \
  --arg memory "$result_memory" \
  --arg cap "$internal_cap" \
  --arg config_hash "$result_config_hash" \
  --arg game "$game_fingerprint" \
  --arg abstraction "$abstraction_fingerprint" \
  --arg configuration "$configuration_fingerprint" '
    {
      schemaVersion: 3,
      kind: "preflop-multiway",
      status: $status,
      sweeps: ($sweeps | tonumber),
      infosets: (($sweeps | tonumber) * 10),
      memoryBytes: ($memory | tonumber),
      elapsedSecs: 0.25,
      seats: [{}, {}, {}, {}, {}, {}],
      configHash: $config_hash,
      effectiveConfig: {run: {max_memory_bytes: ($cap | tonumber)}},
      gameFingerprint: $game,
      abstractionFingerprint: $abstraction,
      configurationFingerprint: $configuration
    }
  ' >"$temporary"
mv -f -- "$temporary" "$output"
exit "$exit_code"
