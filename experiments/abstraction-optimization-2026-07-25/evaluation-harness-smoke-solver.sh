#!/usr/bin/env bash
set -euo pipefail

[[ "$1" == "experiment" && "$2" == "profile" ]]
candidate_config=$3
shift 3
checkpoint=
reference_config=
output=
rung=
samples=
seed=
traversals=
purify=
while (( $# > 0 )); do
  case "$1" in
    --checkpoint)
      checkpoint=$2
      shift 2
      ;;
    --deviator-config)
      reference_config=$2
      shift 2
      ;;
    --output)
      output=$2
      shift 2
      ;;
    --experiment-rung)
      rung=$2
      shift 2
      ;;
    --samples)
      samples=$2
      shift 2
      ;;
    --seed)
      seed=$2
      shift 2
      ;;
    --br-traversals)
      traversals=$2
      shift 2
      ;;
    --purify)
      purify=$2
      shift 2
      ;;
    *)
      exit 64
      ;;
  esac
done

case "$rung" in
  s1)
    sweeps=10
    ;;
  s3)
    sweeps=20
    ;;
  *)
    exit 66
    ;;
esac
training_seed=$((seed ^ 0x70757269))

: "${MOCK_CALLS:?}"
: "${MOCK_RESULT_ROOT:?}"
calls=0
if [[ -f "$MOCK_CALLS" ]]; then
  read -r calls <"$MOCK_CALLS"
fi
printf '%s\n' "$((calls + 1))" >"$MOCK_CALLS"

case "$(basename "$reference_config")" in
  cash-ref-screen.toml)
    reference_hash=dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd
    reference_abstraction=ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff
    reference_cache="$MOCK_RESULT_ROOT/cache/ref-screen.mwab"
    ;;
  cash-ref-screen-2.toml)
    reference_hash=7777777777777777777777777777777777777777777777777777777777777777
    reference_abstraction=6666666666666666666666666666666666666666666666666666666666666666
    reference_cache="$MOCK_RESULT_ROOT/cache/ref-screen-2.mwab"
    ;;
  cash-ref-final.toml)
    reference_hash=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
    reference_abstraction=8888888888888888888888888888888888888888888888888888888888888888
    reference_cache="$MOCK_RESULT_ROOT/cache/ref-final.mwab"
    ;;
  *)
    exit 65
    ;;
esac
printf 'cache-%s\n' "$reference_abstraction" >"$reference_cache"

jq -n \
  --arg candidate_config "$candidate_config" \
  --arg checkpoint "$checkpoint" \
  --arg reference_config "$reference_config" \
  --arg reference_hash "$reference_hash" \
  --arg reference_abstraction "$reference_abstraction" \
  --arg reference_cache "$reference_cache" \
  --arg rung "$rung" \
  --argjson sweeps "$sweeps" \
  --argjson samples "$samples" \
  --argjson seed "$seed" \
  --argjson traversals "$traversals" \
  --argjson purify "$purify" \
  --argjson training_seed "$training_seed" \
  '{
    schema_version: "solvers.reference-deviation-profile/v1",
    experiment: {
      rung: $rung
    },
    candidate: {
      config_path: $candidate_config,
      checkpoint_path: $checkpoint,
      config_fingerprint: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
      game_fingerprint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      abstraction_fingerprint: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      configuration_fingerprint: "9999999999999999999999999999999999999999999999999999999999999999",
      sweeps: $sweeps
    },
    reference: {
      config_path: $reference_config,
      config_fingerprint: $reference_hash,
      game_fingerprint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      abstraction_fingerprint: $reference_abstraction,
      recall: "full",
      artifact_cache: $reference_cache
    },
    samples: $samples,
    seed: $seed,
    br_traversals: $traversals,
    profile: "average",
    purify_threshold: $purify,
    training_seed: $training_seed,
    elapsed_secs: 0,
    training_coverage: [
      range(0; 2) as $seat
      | {
          seat: $seat,
          traversals: $traversals,
          visited_infosets: 1,
          retained_infosets: 1,
          total_visits: 2,
          retained_visits: 2
        }
    ],
    evaluation: {
      evaluation: {
        samples: $samples,
        total_deal_attempts: $samples,
        seats: [
          range(0; 2)
          | {mean: 0, stderr: 0, ci95: [0, 0]}
        ],
        deviation_gain_lower_bound: [
          range(0; 2)
          | {mean: 0, stderr: 0, ci95: [0, 0]}
        ]
      },
      candidate_policy_coverage: [
        range(0; 2)
        | {
            decision_visits: 1,
            stored_strategy_visits: 1,
            uniform_fallback_visits: 0,
            decision_visits_by_street: {preflop: 1, flop: 0, turn: 0, river: 0},
            stored_strategy_visits_by_street: {preflop: 1, flop: 0, turn: 0, river: 0},
            uniform_fallback_visits_by_street: {preflop: 0, flop: 0, turn: 0, river: 0}
          }
      ],
      coverage: [
        range(0; 2)
        | {
            decision_visits: 1,
            trained_action_visits: 1,
            baseline_fallback_visits: 0,
            decision_visits_by_street: {preflop: 1, flop: 0, turn: 0, river: 0},
            trained_action_visits_by_street: {preflop: 1, flop: 0, turn: 0, river: 0},
            baseline_fallback_visits_by_street: {preflop: 0, flop: 0, turn: 0, river: 0}
          }
      ],
      worlds: [
        range(0; $samples) as $sample
        | {
            sample_id: $sample,
            baseline_utilities: [0, 0],
            deviating_seat_utilities: [0, 0],
            gains: [0, 0]
          }
      ]
    }
  }' >"$output"
