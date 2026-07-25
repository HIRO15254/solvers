#!/usr/bin/env bash
set -euo pipefail

workspace=$(cd "$(dirname "$0")/../.." && pwd -P)
transfer_dir="$workspace/experiments/abstraction-transfer-2026-07-25"
runner="$transfer_dir/run-transfer-solves.sh"
mock_generator="$transfer_dir/transfer-runner-smoke-generator.sh"
mock_solver="$transfer_dir/transfer-runner-smoke-solver.sh"
watchdog="$workspace/experiments/abstraction-optimization-2026-07-25/run-with-rss-watchdog.sh"

for script in "$runner" "$mock_generator" "$mock_solver" "$watchdog"; do
  bash -n "$script"
done

fixture=$(mktemp -d /tmp/solvers-transfer-runner-smoke.XXXXXX)
cleanup() {
  if [[ "${KEEP_TRANSFER_SMOKE_FIXTURE:-0}" == "1" ]]; then
    echo "kept transfer smoke fixture: $fixture" >&2
    return
  fi
  if [[ -d "$fixture" &&
        "$fixture" == /tmp/solvers-transfer-runner-smoke.* ]]; then
    rm -rf -- "$fixture"
  fi
}
trap cleanup EXIT INT TERM

manifest="$fixture/manifest.toml"
printf '%s\n' \
  'schema = "solvers.abstraction-transfer-validation/v1"' \
  '# The smoke generator supplies the remaining fixture contract.' \
  >"$manifest"

export MOCK_CALL_LOG="$fixture/solver-calls.csv"
export MOCK_GENERATOR_CALLS="$fixture/generator-calls.txt"
export MOCK_STATE_DIR="$fixture/mock-state"
export MOCK_TARGET_SWEEPS=10
export MOCK_RESOURCE_ONCE_SCENARIO=cash-6max-100bb
result_root="$fixture/results"
common_args=(
  10
  "$result_root"
  --manifest "$manifest"
  --solver "$mock_solver"
  --generator "$mock_generator"
  --watchdog "$watchdog"
  --rss-limit-bytes 67108864
  --case cash
  --solver-seed 1011
  --finalist-regex '^C-smoke$'
)

set +e
"$runner" "${common_args[@]}" >"$fixture/first.stdout" 2>"$fixture/first.stderr"
first_status=$?
set -e
[[ "$first_status" == "75" ]]
grep -q '^RESOURCE_LIMIT ' "$fixture/first.stderr"
[[ "$(wc -l <"$MOCK_CALL_LOG" | tr -d '[:space:]')" == "1" ]]
first_summary=("$result_root"/transfer-s10-cash-seed1011-f*-solve-summary.csv)
[[ "${#first_summary[@]}" == "1" && -f "${first_summary[0]}" ]]
[[ "$(wc -l <"${first_summary[0]}" | tr -d '[:space:]')" == "2" ]]
grep -q 'resource_limit' "${first_summary[0]}"
first_meta=$(
  find "$result_root/runs/cash/cash-6max-100bb" \
    -path '*/meta.json' \
    -not -path '*/segments/*' \
    -type f
)
jq -e '
  .schema == "solvers.abstraction-transfer-solve-run/v1"
  and .status == "resource_limit"
  and .resources.resourceLimitSource == "solver_memory"
  and .resources.rssLimitBytes == 67108864
  and .artifacts.checkpoint.sha256 != null
  and .artifacts.result.sha256 != null
  and .result.status == "resource_limit"
  and .result.sweeps == 5
' "$first_meta" >/dev/null
first_segment=$(jq -r '.artifacts.latestSegment' "$first_meta")
jq -e '
  .schema == "solvers.abstraction-transfer-solve-segment/v1"
  and .status == "resource_limit"
  and .mode == "solve"
  and .commandExitCode == 75
  and .resources.resourceLimitSource == "solver_memory"
' "$first_segment" >/dev/null

"$runner" "${common_args[@]}" >"$fixture/second.stdout"
[[ "$(wc -l <"$MOCK_CALL_LOG" | tr -d '[:space:]')" == "6" ]]
grep -q '^resume,cash-6max-100bb,10$' "$MOCK_CALL_LOG"
[[ "$(wc -l <"${first_summary[0]}" | tr -d '[:space:]')" == "6" ]]
[[ "$(grep -c '\"completed\"' "${first_summary[0]}")" == "5" ]]
find "$result_root/runs/cash" \
  -path '*/meta.json' \
  -not -path '*/segments/*' \
  -type f \
  -exec jq -e '.status == "completed" and .result.sweeps == 10' {} \; \
  >/dev/null

"$runner" "${common_args[@]}" >"$fixture/third.stdout"
[[ "$(wc -l <"$MOCK_CALL_LOG" | tr -d '[:space:]')" == "6" ]]
[[ "$(grep -c ',\"reused\",' "${first_summary[0]}")" == "5" ]]
[[ -z "$(find "$result_root" -name '*.tmp.*' -print -quit)" ]]
[[ ! -e "$result_root/.transfer-solve.lock" ]]

plan_output="$fixture/plan.stdout"
"$runner" 10 "$result_root" \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --generator "$mock_generator" \
  --watchdog "$watchdog" \
  --rss-limit-bytes 67108864 \
  --case tournament \
  --solver-seed 2022 \
  --finalist-regex '^T-smoke$' \
  --plan >"$plan_output"
[[ "$(grep -c '^PLAN ' "$plan_output")" == "5" ]]
grep -q 'planned 5 serial transfer solve jobs' "$plan_output"
[[ "$(wc -l <"$MOCK_CALL_LOG" | tr -d '[:space:]')" == "6" ]]

set +e
"$runner" 10 "$result_root" \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --generator "$mock_generator" \
  --watchdog "$watchdog" \
  --case cash \
  --solver-seed 9999 \
  --finalist-regex '^C-smoke$' \
  --plan >"$fixture/zero.stdout" 2>"$fixture/zero.stderr"
zero_status=$?
set -e
[[ "$zero_status" == "2" ]]
grep -q 'no transfer scenarios matched' "$fixture/zero.stderr"

export MOCK_METADATA_MISMATCH=1
metadata_drift_root="$fixture/metadata-drift-results"
set +e
"$runner" 10 "$metadata_drift_root" \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --generator "$mock_generator" \
  --watchdog "$watchdog" \
  --case cash \
  --solver-seed 1011 \
  --finalist-regex '^C-smoke$' \
  --plan >"$fixture/metadata-drift.stdout" 2>"$fixture/metadata-drift.stderr"
metadata_drift_status=$?
set -e
[[ "$metadata_drift_status" == "2" ]]
grep -q 'CSV/metadata mismatch' "$fixture/metadata-drift.stderr"
unset MOCK_METADATA_MISMATCH

printf 'tampered checkpoint\n' >>"$(
  jq -r '.artifacts.checkpoint.path' "$first_meta"
)"
set +e
"$runner" "${common_args[@]}" >"$fixture/stale.stdout" 2>"$fixture/stale.stderr"
stale_status=$?
set -e
[[ "$stale_status" == "2" ]]
grep -q 'checkpoint SHA mismatch' "$fixture/stale.stderr"
[[ "$(wc -l <"$MOCK_CALL_LOG" | tr -d '[:space:]')" == "6" ]]

unset MOCK_RESOURCE_ONCE_SCENARIO
export MOCK_STATE_DIR="$fixture/tamper-state"
export MOCK_TAMPER_SCENARIO=cash-6max-100bb
tamper_root="$fixture/tamper-results"
set +e
"$runner" 10 "$tamper_root" \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --generator "$mock_generator" \
  --watchdog "$watchdog" \
  --rss-limit-bytes 67108864 \
  --case cash \
  --solver-seed 1011 \
  --finalist-regex '^C-smoke$' \
  >"$fixture/tamper.stdout" 2>"$fixture/tamper.stderr"
tamper_status=$?
set -e
[[ "$tamper_status" == "3" ]]
grep -q '^INPUT_DRIFT ' "$fixture/tamper.stderr"
tamper_meta=$(
  find "$tamper_root/runs/cash/cash-6max-100bb" \
    -path '*/meta.json' \
    -not -path '*/segments/*' \
    -type f
)
jq -e '.status == "input_drift" and .artifacts.result.sha256 == null' \
  "$tamper_meta" >/dev/null
[[ -f "$(dirname "$tamper_meta")/segments/0001/untrusted-result.json" ]]

unset MOCK_TAMPER_SCENARIO
export MOCK_STATE_DIR="$fixture/early-state"
export MOCK_EARLY_SCENARIO=cash-6max-100bb
early_root="$fixture/early-results"
set +e
"$runner" 10 "$early_root" \
  --manifest "$manifest" \
  --solver "$mock_solver" \
  --generator "$mock_generator" \
  --watchdog "$watchdog" \
  --rss-limit-bytes 67108864 \
  --case cash \
  --solver-seed 1011 \
  --finalist-regex '^C-smoke$' \
  >"$fixture/early.stdout" 2>"$fixture/early.stderr"
early_status=$?
set -e
[[ "$early_status" == "3" ]]
grep -q '^EARLY_TERMINAL ' "$fixture/early.stderr"

echo "transfer solve runner smoke test passed"
