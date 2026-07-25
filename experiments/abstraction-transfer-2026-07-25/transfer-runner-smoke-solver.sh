#!/usr/bin/env bash
set -euo pipefail

: "${MOCK_CALL_LOG:?}"
: "${MOCK_STATE_DIR:?}"
mkdir -p "$MOCK_STATE_DIR"

(( $# >= 2 )) || exit 64
mode=$1
config=$2
shift 2
case "$mode" in
  solve|resume) ;;
  *) exit 64 ;;
esac
config_index=${MOCK_CONFIG_INDEX:-"$(dirname "$config")/transfer-configs.csv"}
[[ -f "$config_index" ]] || exit 65

target_sweeps=
output=
metrics=
checkpoint=
while (( $# > 0 )); do
  case "$1" in
    --iterations|--max-sweeps)
      target_sweeps=$2
      shift 2
      ;;
    --output)
      output=$2
      shift 2
      ;;
    --metrics)
      metrics=$2
      shift 2
      ;;
    --checkpoint)
      checkpoint=$2
      shift 2
      ;;
    *)
      exit 64
      ;;
  esac
done
target_sweeps=${target_sweeps:-${MOCK_TARGET_SWEEPS:-}}
[[ -n "$target_sweeps" && -n "$output" && -n "$metrics" && -n "$checkpoint" ]] ||
  exit 64
if [[ "$mode" == "resume" && ! -f "$checkpoint" ]]; then
  exit 65
fi

row=$(
  awk -F, -v wanted="$config" '
    NR > 1 && $11 == wanted {
      print
      found++
    }
    END {
      if (found != 1) exit 3
    }
  ' "$config_index"
)
IFS=, read -r \
  case_name \
  scenario_id \
  finalist_id \
  seats \
  stack_bb \
  abstraction_seed \
  solver_seed \
  evaluation_seed \
  canonical_config \
  canonical_fingerprint \
  compatibility_config \
  compatibility_fingerprint \
  game_fingerprint \
  tree_fingerprint \
  abstraction_spec_fingerprint \
  cache_path \
  <<<"$row"

printf '%s,%s,%s\n' "$mode" "$scenario_id" "$target_sweeps" >>"$MOCK_CALL_LOG"
printf 'checkpoint scenario=%s target=%s\n' "$scenario_id" "$target_sweeps" >"$checkpoint"
printf '{"scenario":"%s","mode":"%s"}\n' "$scenario_id" "$mode" >>"$metrics"
printf 'cache scenario=%s\n' "$scenario_id" >"$cache_path"

status=completed
reported_sweeps=$target_sweeps
exit_code=0
if [[ "$mode" == "resume" ]]; then
  status=converged
elif [[ "${MOCK_RESOURCE_ONCE_SCENARIO:-}" == "$scenario_id" &&
        ! -f "$MOCK_STATE_DIR/resource-$scenario_id" ]]; then
  printf 'seen\n' >"$MOCK_STATE_DIR/resource-$scenario_id"
  status=resource_limit
  reported_sweeps=$((target_sweeps / 2))
  exit_code=75
elif [[ "${MOCK_EARLY_SCENARIO:-}" == "$scenario_id" ]]; then
  status=converged
  reported_sweeps=$((target_sweeps / 2))
fi

runtime_abstraction=dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd
configuration=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
jq -n \
  --arg status "$status" \
  --argjson sweeps "$reported_sweeps" \
  --argjson seats "$seats" \
  --arg config_fingerprint "$compatibility_fingerprint" \
  --arg game_fingerprint "$game_fingerprint" \
  --arg abstraction_fingerprint "$runtime_abstraction" \
  --arg configuration_fingerprint "$configuration" '
    {
      schemaVersion: 3,
      kind: "preflop-multiway",
      status: $status,
      sweeps: $sweeps,
      infosets: 42,
      memoryBytes: 1048576,
      elapsedSecs: 0,
      seats: [range(0; $seats) | {seat: .}],
      configHash: $config_fingerprint,
      gameFingerprint: $game_fingerprint,
      abstractionFingerprint: $abstraction_fingerprint,
      configurationFingerprint: $configuration_fingerprint
    }
  ' >"$output"

if [[ "${MOCK_TAMPER_SCENARIO:-}" == "$scenario_id" &&
      ! -f "$MOCK_STATE_DIR/tamper-$scenario_id" ]]; then
  printf 'seen\n' >"$MOCK_STATE_DIR/tamper-$scenario_id"
  printf '# mutated during solve\n' >>"$config"
fi

exit "$exit_code"
