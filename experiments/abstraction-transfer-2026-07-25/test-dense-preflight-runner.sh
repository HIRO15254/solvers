#!/usr/bin/env bash
set -euo pipefail

workspace=$(cd "$(dirname "$0")/../.." && pwd -P)
preflight_dir="$workspace/experiments/abstraction-transfer-2026-07-25"
runner="$preflight_dir/run-dense-preflight.sh"
generator="$preflight_dir/generate-dense-preflight-plan.py"
manifest="$preflight_dir/dense-preflight-manifest.json"
mock="$preflight_dir/dense-preflight-smoke-binary.sh"
watchdog="$preflight_dir/dense-preflight-smoke-watchdog.sh"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$1" | awk '{print $1}'
  else
    shasum -a 256 -- "$1" | awk '{print $1}'
  fi
}

bash -n "$runner"
bash -n "$mock"
bash -n "$watchdog"
python3 -c 'compile(open(__import__("sys").argv[1]).read(), __import__("sys").argv[1], "exec")' \
  "$generator"

fixture=$(mktemp -d /tmp/solvers-dense-preflight-smoke.XXXXXX)
cleanup() {
  if [[ "${KEEP_DENSE_PREFLIGHT_FIXTURE:-0}" == "1" ]]; then
    echo "kept dense preflight fixture: $fixture" >&2
    return
  fi
  if [[ -d "$fixture" &&
        "$fixture" == /tmp/solvers-dense-preflight-smoke.* ]]; then
    rm -rf -- "$fixture"
  fi
}
trap cleanup EXIT INT TERM

export MOCK_DENSE_PREFLIGHT_CALL_LOG="$fixture/calls.csv"
plan_root="$fixture/plan"
"$runner" "$plan_root" \
  --manifest "$manifest" \
  --preflight "$mock" \
  --generator "$generator" \
  --watchdog "$watchdog" \
  --plan >"$fixture/plan.stdout"
[[ "$(grep -c '^PLAN ' "$fixture/plan.stdout")" == "60" ]]
grep -q '^planned 60 serial dense preflight jobs$' "$fixture/plan.stdout"
[[ ! -e "$MOCK_DENSE_PREFLIGHT_CALL_LOG" ]]
[[ "$(wc -l <"$plan_root/dense-preflight-plan.csv" | tr -d '[:space:]')" == "61" ]]
[[ ! -e "$plan_root/dense-preflight-summary.csv" ]]
jq -e '
  .schema == "solvers.abstraction-transfer-dense-preflight-plan/v1"
  and .jobCount == 60
  and .run.max_memory_bytes == 8589934592
  and .run.rss_limit_bytes == 8053063680
  and .run.buckets == [1, 2, 16, 64, 128, 256]
  and .resourceSemantics.denseFeasibilityScope == "arena-payload-estimate-only"
  and .resourceSemantics.productionProcessFeasibilityEstablished == false
  and .resourceSemantics.requiredProductionValidation.solverDenseArenaCapBytes
    == 6442450944
  and .resourceSemantics.requiredProductionValidation.processLimitBytes
    == 8589934592
  and (.fixedEnvelope | length) == 10
' "$plan_root/dense-preflight-metadata.json" >/dev/null

result_root="$fixture/results"
set +e
"$runner" "$result_root" \
  --manifest "$manifest" \
  --preflight "$mock" \
  --generator "$generator" \
  --watchdog "$watchdog" \
  --rss-limit-bytes 67108864 \
  >"$fixture/run.stdout"
run_status=$?
set -e
[[ "$run_status" == "75" ]]
summary="$result_root/dense-preflight-summary.csv"
[[ -f "$summary" ]]
[[ "$(wc -l <"$summary" | tr -d '[:space:]')" == "61" ]]
awk -F, 'NF != 42 {exit 1}' "$summary"
awk -F, 'NR > 1 && $42 !~ /^[0-9a-f]{64}$/ {exit 1}' "$summary"
while IFS=$'\t' read -r meta_path expected_sha; do
  [[ "$(sha256_file "$meta_path")" == "$expected_sha" ]]
done < <(awk -F, 'NR > 1 {print $41 "\t" $42}' "$summary")
[[ "$(wc -l <"$MOCK_DENSE_PREFLIGHT_CALL_LOG" | tr -d '[:space:]')" == "60" ]]
[[ "$(awk -F, 'NR > 1 && $6 == "completed" {n++} END {print n+0}' "$summary")" == "10" ]]
[[ "$(awk -F, 'NR > 1 && $6 == "memory_limit" {n++} END {print n+0}' "$summary")" == "30" ]]
[[ "$(awk -F, 'NR > 1 && $6 == "node_checkpoint" {n++} END {print n+0}' "$summary")" == "10" ]]
[[ "$(awk -F, 'NR > 1 && $6 == "rss_limit" {n++} END {print n+0}' "$summary")" == "10" ]]
[[ "$(awk -F, 'NR > 1 && $6 == "rss_limit" && $15 == 75 && $16 == 130 {n++} END {print n+0}' "$summary")" == "10" ]]
[[ "$(awk -F, 'NR > 1 && $19 != 67108864 {n++} END {print n+0}' "$summary")" == "0" ]]
[[ "$(find "$result_root/runs" -type f -name meta.json | wc -l | tr -d '[:space:]')" == "60" ]]
find "$result_root/runs" -type f -name meta.json -exec jq -e '
  .schema == "solvers.abstraction-transfer-dense-preflight-run/v1"
  and (.status == "completed"
    or .status == "memory_limit"
    or .status == "node_checkpoint"
    or .status == "rss_limit")
  and .resources.rssLimitBytes == 67108864
  and .resources.rssScope == "count-only-preflight-process"
  and .resources.denseFeasibilityScope == "arena-payload-estimate-only"
  and .resources.productionProcessFeasibilityEstablished == false
  and .resources.requiredProductionValidation.solverDenseArenaCapBytes
    == 6442450944
  and .resources.requiredProductionValidation.processLimitBytes == 8589934592
  and .resources.rssMonitorError == false
  and (
    if .status == "rss_limit" then
      .watchdogExitCode == 75
      and .childExitCode == 130
      and .resources.rssLimitExceeded == true
    else
      .watchdogExitCode == .childExitCode
      and .resources.rssLimitExceeded == false
    end
  )
  and (
    if .status == "completed" then
      .result.exactNodes == 100
      and .result.columns == 200
      and .result.denseArenaBytes == 400
    elif .status == "memory_limit" then
      .result.exactNodes == null
      and .result.firstExceedingPrefixNodes == 101
      and .result.largestAcceptedPrefixNodes == 100
      and .result.columns == 201
      and .result.denseArenaBytes == 8589934593
    elif .status == "node_checkpoint" then
      .result.exactNodes == 4294967296
      and .result.largestAcceptedPrefixNodes == 4294967296
    else
      .result.exactNodes == null
      and .result.firstExceedingPrefixNodes == null
      and .result.largestAcceptedPrefixNodes == null
      and .result.columns == null
      and .result.denseArenaBytes == null
    end
  )
  and (.inputs.manifest.sha256 | test("^[0-9a-f]{64}$"))
  and (.inputs.plan.sha256 | test("^[0-9a-f]{64}$"))
  and (.artifacts.stdout.sha256 | test("^[0-9a-f]{64}$"))
  and (.artifacts.stderr.sha256 | test("^[0-9a-f]{64}$"))
' {} \; >/dev/null
[[ -z "$(find "$result_root" -name '*.tmp.*' -o -name '*.work.*')" ]]
[[ ! -e "$result_root/.dense-preflight.lock" ]]

tampered="$fixture/tampered.json"
cp "$manifest" "$tampered"
sed -i.bak 's/\"buckets\": \[1, 2, 16, 64, 128, 256\]/\"buckets\": [1, 2]/' "$tampered"
rm -f "$tampered.bak"
set +e
"$runner" "$fixture/tampered-results" \
  --manifest "$tampered" \
  --preflight "$mock" \
  --generator "$generator" \
  --watchdog "$watchdog" \
  --plan >"$fixture/tampered.stdout" 2>"$fixture/tampered.stderr"
tampered_status=$?
set -e
[[ "$tampered_status" == "1" || "$tampered_status" == "2" ]]
grep -q 'buckets must equal' "$fixture/tampered.stderr"

export MOCK_DENSE_PREFLIGHT_CALL_LOG="$fixture/malformed-calls.csv"
export MOCK_DENSE_PREFLIGHT_MALFORMED=1
set +e
"$runner" "$fixture/malformed-results" \
  --manifest "$manifest" \
  --preflight "$mock" \
  --generator "$generator" \
  --watchdog "$watchdog" \
  --rss-limit-bytes 67108864 \
  >"$fixture/malformed.stdout" 2>"$fixture/malformed.stderr"
malformed_status=$?
set -e
unset MOCK_DENSE_PREFLIGHT_MALFORMED
[[ "$malformed_status" == "2" ]]
[[ "$(wc -l <"$MOCK_DENSE_PREFLIGHT_CALL_LOG" | tr -d '[:space:]')" == "2" ]]
grep -q 'typed MemoryLimit contract mismatch' "$fixture/malformed.stderr"

export MOCK_DENSE_PREFLIGHT_CALL_LOG="$fixture/game-mismatch-calls.csv"
export MOCK_DENSE_PREFLIGHT_GAME_MISMATCH=1
set +e
"$runner" "$fixture/game-mismatch-results" \
  --manifest "$manifest" \
  --preflight "$mock" \
  --generator "$generator" \
  --watchdog "$watchdog" \
  --rss-limit-bytes 67108864 \
  >"$fixture/game-mismatch.stdout" 2>"$fixture/game-mismatch.stderr"
game_mismatch_status=$?
set -e
unset MOCK_DENSE_PREFLIGHT_GAME_MISMATCH
[[ "$game_mismatch_status" == "2" ]]
[[ "$(wc -l <"$MOCK_DENSE_PREFLIGHT_CALL_LOG" | tr -d '[:space:]')" == "2" ]]
grep -q 'RESULT identity mismatch' "$fixture/game-mismatch.stderr"

echo "dense preflight runner smoke test passed"
