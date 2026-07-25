#!/usr/bin/env bash
set -euo pipefail

workspace=$(cd "$(dirname "$0")/../.." && pwd -P)
harness="$workspace/experiments/abstraction-optimization-2026-07-25/run-evaluation-rung.sh"
watchdog="$workspace/experiments/abstraction-optimization-2026-07-25/run-with-rss-watchdog.sh"

bash -n "$harness"
bash -n "$watchdog"

sha256_stream() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

fixture=$(mktemp -d /tmp/solvers-evaluation-harness-smoke.XXXXXX)
cleanup() {
  if [[ -d "$fixture" && "$fixture" == /tmp/solvers-evaluation-harness-smoke.* ]]; then
    rm -rf -- "$fixture"
  fi
}
trap cleanup EXIT INT TERM

manifest="$fixture/manifest.toml"
result_root="$fixture/results"
config_dir="$result_root/configs"
cache_dir="$result_root/cache"
run_dir="$result_root/runs/cash/C-smoke-a0-s1011"
later_same_solver_run_dir="$result_root/runs/cash/C-smoke-a7-s1011"
mkdir -p \
  "$config_dir" "$cache_dir" "$run_dir" "$later_same_solver_run_dir"

printf '%s\n' \
  'schema = "solvers.abstraction-optimization/v1"' \
  '' \
  '[selection]' \
  'local_rss_stop_bytes = 8053063680' \
  '' \
  '[[reference]]' \
  'id = "ref-screen"' \
  'kind = "ehs2-table"' \
  'flop_buckets = 2' \
  'turn_buckets = 2' \
  'river_buckets = 2' \
  'recall = "full"' \
  'rungs = ["s1"]' \
  '' \
  '[[reference]]' \
  'id = "ref-screen-2"' \
  'kind = "rollout-kmeans"' \
  'flop_buckets = 2' \
  'turn_buckets = 2' \
  'river_buckets = 2' \
  'rollout_samples = 2' \
  'points_per_bucket = 2' \
  'kmeans_iterations = 2' \
  'seed = 5' \
  'recall = "full"' \
  'rungs = ["s1"]' \
  '' \
  '[[reference]]' \
  'id = "ref-final"' \
  'kind = "rollout-kmeans"' \
  'flop_buckets = 2' \
  'turn_buckets = 2' \
  'river_buckets = 2' \
  'rollout_samples = 2' \
  'points_per_bucket = 2' \
  'kmeans_iterations = 2' \
  'seed = 7' \
  'recall = "full"' \
  'final_only = true' \
  'rungs = ["s3"]' \
  '' \
  '[[seed_pair]]' \
  'abstraction = 0' \
  'solver = 1011' \
  'evaluation = 42' \
  '' \
  '[[seed_pair]]' \
  'abstraction = 7' \
  'solver = 1011' \
  'evaluation = 43' \
  '' \
  '[[rung]]' \
  'id = "s1"' \
  'sweeps = 10' \
  'seed_pairs = 1' \
  'evaluation_samples = 2' \
  'deviator_traversals_per_seat = 3' \
  '' \
  '[[rung]]' \
  'id = "s3"' \
  'sweeps = 20' \
  'seed_pairs = 1' \
  'evaluation_samples = 2' \
  'deviator_traversals_per_seat = 3' \
  '' \
  '[[candidate]]' \
  'case = "cash"' \
  'id = "C-smoke"' \
  >"$manifest"

candidate_config="$config_dir/C-smoke-a0-s1011.toml"
later_same_solver_config="$config_dir/C-smoke-a7-s1011.toml"
cash_screen_config="$config_dir/cash-ref-screen.toml"
tournament_screen_config="$config_dir/tournament-ref-screen.toml"
cash_screen_2_config="$config_dir/cash-ref-screen-2.toml"
tournament_screen_2_config="$config_dir/tournament-ref-screen-2.toml"
cash_final_config="$config_dir/cash-ref-final.toml"
tournament_final_config="$config_dir/tournament-ref-final.toml"
printf '[game.abstraction]\nrecall = "full"\n' >"$candidate_config"
printf '[game.abstraction]\nrecall = "full"\n' >"$later_same_solver_config"
printf '[game.abstraction]\nrecall = "full"\n' >"$cash_screen_config"
printf '[game.abstraction]\nrecall = "full"\n' >"$tournament_screen_config"
printf '[game.abstraction]\nrecall = "full"\n' >"$cash_screen_2_config"
printf '[game.abstraction]\nrecall = "full"\n' >"$tournament_screen_2_config"
printf '[game.abstraction]\nrecall = "full"\n' >"$cash_final_config"
printf '[game.abstraction]\nrecall = "full"\n' >"$tournament_final_config"

game_fingerprint=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
candidate_config_hash=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
candidate_abstraction=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
candidate_configuration=9999999999999999999999999999999999999999999999999999999999999999
screen_config_hash=dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd
screen_2_config_hash=7777777777777777777777777777777777777777777777777777777777777777
final_config_hash=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee

printf '%s\n' \
  'role,case,id,abstraction_seed,solver_seed,evaluation_seed,config,cache,config_hash,game_fingerprint' \
  "candidate,cash,C-smoke,0,1011,42,$candidate_config,$cache_dir/candidate.mwab,$candidate_config_hash,$game_fingerprint" \
  "candidate,cash,C-smoke,7,1011,43,$later_same_solver_config,$cache_dir/candidate-a7.mwab,$candidate_config_hash,$game_fingerprint" \
  "reference,cash,ref-screen,,1011,42,$cash_screen_config,$cache_dir/ref-screen.mwab,$screen_config_hash,$game_fingerprint" \
  "reference,tournament,ref-screen,,1011,42,$tournament_screen_config,$cache_dir/tournament-ref-screen.mwab,$screen_config_hash,$game_fingerprint" \
  "reference,cash,ref-screen-2,5,1011,42,$cash_screen_2_config,$cache_dir/ref-screen-2.mwab,$screen_2_config_hash,$game_fingerprint" \
  "reference,tournament,ref-screen-2,5,1011,42,$tournament_screen_2_config,$cache_dir/tournament-ref-screen-2.mwab,$screen_2_config_hash,$game_fingerprint" \
  "reference,cash,ref-final,7,1011,42,$cash_final_config,$cache_dir/ref-final.mwab,$final_config_hash,$game_fingerprint" \
  "reference,tournament,ref-final,7,1011,42,$tournament_final_config,$cache_dir/tournament-ref-final.mwab,$final_config_hash,$game_fingerprint" \
  >"$config_dir/configs.csv"
jq -n \
  '{
    schema: "solvers.abstraction-optimization/v1",
    seedPairs: [
      {abstraction: 0, solver: 1011, evaluation: 42},
      {abstraction: 7, solver: 1011, evaluation: 43}
    ],
    rungs: [
      {
        id: "s1",
        sweeps: 10,
        seed_pairs: 1,
        evaluation_samples: 2,
        deviator_traversals_per_seat: 3
      },
      {
        id: "s3",
        sweeps: 20,
        seed_pairs: 1,
        evaluation_samples: 2,
        deviator_traversals_per_seat: 3
      }
    ],
    referenceRouting: [
      {id: "ref-screen", rungs: ["s1"], finalOnly: false},
      {id: "ref-screen-2", rungs: ["s1"], finalOnly: false},
      {id: "ref-final", rungs: ["s3"], finalOnly: true}
    ]
  }' >"$config_dir/experiment-metadata.json"

printf 'checkpoint\n' >"$run_dir/checkpoint.mwckpt"
printf 'checkpoint\n' >"$later_same_solver_run_dir/checkpoint.mwckpt"
jq -n \
  --arg config_hash "$candidate_config_hash" \
  --arg game "$game_fingerprint" \
  --arg abstraction "$candidate_abstraction" \
  --arg configuration "$candidate_configuration" \
  '{
    status: "completed",
    sweeps: 10,
    seats: [{seat: 0}, {seat: 1}],
    configHash: $config_hash,
    gameFingerprint: $game,
    abstractionFingerprint: $abstraction,
    configurationFingerprint: $configuration
  }' >"$run_dir/run.json"
cp "$run_dir/run.json" "$later_same_solver_run_dir/run.json"

mock_solver="$workspace/experiments/abstraction-optimization-2026-07-25/evaluation-harness-smoke-solver.sh"
bash -n "$mock_solver"

export MOCK_CALLS="$fixture/mock-calls.txt"
export MOCK_RESULT_ROOT="$result_root"

"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" >/dev/null
read -r calls <"$MOCK_CALLS"
[[ "$calls" == "2" ]]
# The rung selects only the first seed pair. The second pair deliberately
# shares solver=1011, so selecting by solver seed alone would execute four jobs.
[[ ! -d "$result_root/evaluations/s1/cash/C-smoke-a7-s1011" ]]
[[ -f "$result_root/s1-rung-evaluation-summary.csv" ]]
[[ "$(wc -l <"$result_root/s1-rung-evaluation-summary.csv" | tr -d '[:space:]')" == "3" ]]
grep -q '"ref-screen","completed"' "$result_root/s1-rung-evaluation-summary.csv"
grep -q '"ref-screen-2","completed"' "$result_root/s1-rung-evaluation-summary.csv"
screen_meta=$(find "$result_root/evaluations/s1" -path '*/ref-screen/*/meta.json' -type f)
jq -e '
  .status == "completed"
  and .experiment.rung == "s1"
  and .experiment.sweeps == 10
  and .experiment.samples == 2
  and .experiment.evaluation_seed == 42
  and .experiment.deviator_traversals_per_seat == 3
  and .experiment.training_seed == 1886745155
  and .experiment.profile == "average"
  and .experiment.purify_threshold == 0
  and .reference_cache.input_sha256 == "missing"
  and (.reference_cache.output_sha256 | test("^[0-9a-f]{64}$"))
' "$screen_meta" >/dev/null

"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" >/dev/null
read -r calls <"$MOCK_CALLS"
[[ "$calls" == "2" ]]

screen_report=$(jq -r '.artifacts.report' "$screen_meta")
jq \
  '.evaluation.candidate_policy_coverage[0].decision_visits_by_street.preflop = 2' \
  "$screen_report" >"$screen_report.invalid"
mv "$screen_report.invalid" "$screen_report"
"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" >/dev/null
read -r calls <"$MOCK_CALLS"
[[ "$calls" == "3" ]]
jq -e \
  '.evaluation.candidate_policy_coverage[0].decision_visits_by_street.preflop == 1' \
  "$screen_report" >/dev/null
jq -e '
  .experiment.rung == "s1"
  and .candidate.sweeps == 10
  and .br_traversals == 3
  and .training_seed == 1886745155
  and .profile == "average"
  and .purify_threshold == 0
' "$screen_report" >/dev/null

jq '.experiment.rung = "s3"' "$screen_report" >"$screen_report.invalid"
mv "$screen_report.invalid" "$screen_report"
"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" >/dev/null
read -r calls <"$MOCK_CALLS"
[[ "$calls" == "4" ]]
jq -e '.experiment.rung == "s1"' "$screen_report" >/dev/null

jq '.experiment.profile = "current"' "$screen_meta" >"$screen_meta.invalid"
mv "$screen_meta.invalid" "$screen_meta"
"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" >/dev/null
read -r calls <"$MOCK_CALLS"
[[ "$calls" == "5" ]]
jq -e '.experiment.profile == "average"' "$screen_meta" >/dev/null

plan_output=$(
  "$harness" s1 "$result_root" '^C-smoke$' \
    --manifest "$manifest" \
    --solver "$mock_solver" \
    --watchdog "$watchdog" \
    --samples 4 \
    --br-traversals 5 \
    --evaluation-seed 99 \
    --plan
)
[[ "$plan_output" == *"samples=4 traversals=5 seed=99"* ]]

"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" \
  --evaluation-seed 99 >/dev/null
read -r calls <"$MOCK_CALLS"
[[ "$calls" == "6" ]]
jq -e '
  .inputs.seed_pairs == 1
  and .inputs.selected_seed_pairs == [
    {"abstraction": 0, "solver": 1011, "evaluation": 99}
  ]
  and .inputs.evaluation_seed_override == 99
' "$result_root/s1-rung-coverage-gates.json" >/dev/null

default_summary="$result_root/s1-rung-evaluation-summary.csv"
default_summary_sha=$(sha256_stream <"$default_summary")
reference_filter='^ref-screen$'
reference_filter_hash=$(printf '%s' "$reference_filter" | sha256_stream)
reference_filter_suffix="rf-${reference_filter_hash:0:16}"
"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" \
  --reference-filter "$reference_filter" >/dev/null
read -r calls <"$MOCK_CALLS"
[[ "$calls" == "7" ]]
filtered_summary="$result_root/s1-rung-${reference_filter_suffix}-evaluation-summary.csv"
filtered_coverage="$result_root/s1-rung-${reference_filter_suffix}-coverage-gates.json"
[[ -f "$filtered_summary" && -f "$filtered_coverage" ]]
[[ "$(wc -l <"$filtered_summary" | tr -d '[:space:]')" == "2" ]]
[[ "$(head -n 1 "$filtered_summary")" == *",reference_filter,reference_filter_suffix" ]]
grep -q "\"$reference_filter\",\"$reference_filter_suffix\"" "$filtered_summary"
[[ "$(sha256_stream <"$default_summary")" == "$default_summary_sha" ]]
jq -e \
  --arg filter "$reference_filter" \
  --arg suffix "$reference_filter_suffix" '
    .inputs.reference_filter == $filter
    and .inputs.reference_filter_explicit == true
    and .inputs.reference_filter_suffix == $suffix
    and .counts.reports == 1
    and all(.reports[]; .reference_id == "ref-screen")
  ' "$filtered_coverage" >/dev/null
filtered_meta=$(
  find "$result_root/evaluations/s1" -path '*/ref-screen/*/meta.json' -type f |
    while IFS= read -r path; do
      if jq -e --arg suffix "$reference_filter_suffix" \
        '.experiment.reference_filter_suffix == $suffix' "$path" >/dev/null; then
        printf '%s\n' "$path"
      fi
    done
)
[[ -f "$filtered_meta" ]]
jq -e \
  --arg filter "$reference_filter" \
  --arg suffix "$reference_filter_suffix" '
    .experiment.reference_filter == $filter
    and .experiment.reference_filter_suffix == $suffix
  ' "$filtered_meta" >/dev/null

set +e
"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" \
  --reference-filter '[' \
  --plan >/dev/null 2>&1
invalid_reference_filter_status=$?
"$harness" s1 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" \
  --reference-filter '^does-not-exist$' \
  --plan >/dev/null 2>&1
empty_reference_filter_status=$?
set -e
[[ "$invalid_reference_filter_status" == "2" ]]
[[ "$empty_reference_filter_status" == "2" ]]

jq '.sweeps = 20' "$run_dir/run.json" >"$run_dir/run.next.json"
mv "$run_dir/run.next.json" "$run_dir/run.json"
"$harness" s3 "$result_root" '^C-smoke$' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" \
  --coverage-candidate-stored-min 0.995 \
  --coverage-candidate-postflop-stored-min 0.95 \
  --coverage-candidate-postflop-min-visits 123 >/dev/null
read -r calls <"$MOCK_CALLS"
[[ "$calls" == "8" ]]
[[ "$(wc -l <"$result_root/s3-rung-evaluation-summary.csv" | tr -d '[:space:]')" == "2" ]]
grep -q '"ref-final","completed"' "$result_root/s3-rung-evaluation-summary.csv"
jq -e '
  .thresholds.candidate_stored_min == 0.995
  and .thresholds.candidate_postflop_stored_min == 0.95
  and .thresholds.candidate_postflop_min_visits == 123
  and .inputs.seed_pairs == 1
  and .inputs.selected_seed_pairs == [
    {"abstraction": 0, "solver": 1011, "evaluation": 42}
  ]
  and .inputs.evaluation_seed_override == null
' "$result_root/s3-rung-coverage-gates.json" >/dev/null

set +e
"$harness" s1 "$result_root" '[' \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --watchdog "$watchdog" \
  --plan >/dev/null 2>&1
invalid_regex_status=$?
set -e
[[ "$invalid_regex_status" == "2" ]]

PYTHONDONTWRITEBYTECODE=1 \
  python3 "$workspace/experiments/abstraction-optimization-2026-07-25/test-coverage-gates.py" \
  >/dev/null
PYTHONDONTWRITEBYTECODE=1 \
  python3 "$workspace/experiments/abstraction-optimization-2026-07-25/test-ranking-harness.py" \
  >/dev/null

echo "evaluation harness smoke test passed"
