#!/usr/bin/env bash
set -euo pipefail

[[ $# == 3 ]] || {
  echo "usage: $0 MANIFEST OUTPUT_DIR CACHE_DIR" >&2
  exit 2
}

manifest=$1
output_dir=$2
cache_dir=$3
mkdir -p "$output_dir" "$cache_dir"
manifest=$(cd "$(dirname "$manifest")" && printf '%s/%s\n' "$PWD" "$(basename "$manifest")")
output_dir=$(cd "$output_dir" && pwd -P)
cache_dir=$(cd "$cache_dir" && pwd -P)

if [[ -n "${MOCK_GENERATOR_CALLS:-}" ]]; then
  calls=0
  if [[ -f "$MOCK_GENERATOR_CALLS" ]]; then
    read -r calls <"$MOCK_GENERATOR_CALLS"
  fi
  printf '%s\n' "$((calls + 1))" >"$MOCK_GENERATOR_CALLS"
fi

index="$output_dir/transfer-configs.csv"
records="$output_dir/.smoke-records.jsonl"
: >"$records"
printf '%s\n' \
  'case,scenario_id,finalist_id,seats,stack_bb,abstraction_seed,solver_seed,evaluation_seed,canonical_config,canonical_config_fingerprint,compatibility_config,compatibility_config_fingerprint,game_fingerprint,tree_contract_fingerprint,abstraction_spec_fingerprint,cache' \
  >"$index"

scenario_rows='
tournament|tournament-6max-5bb|6|5|02f0179b06ec036f79c6b0141e67a65bfb9a71f73557203db210516bb2bba63c
tournament|tournament-6max-50bb|6|50|a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512
tournament|tournament-8max-20bb|8|20|c357fb191000573a575708cfee241722ab201c8f1e584b8783ac299da4258e9f
tournament|tournament-9max-5bb|9|5|0908aa79db3d1149e055d3a25bb1e6ab25fd5e7fb956cbe7cb06e4e8ab5c8b5f
tournament|tournament-9max-50bb|9|50|39e87301e3169b897cfb59b9be452be7360ba13c574ad8a5d0a03bb8013c2282
cash|cash-6max-100bb|6|100|c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b
cash|cash-6max-800bb|6|800|9d88e6644bccff8e5bde62ec9c8a480b0b344b60ac2ffeaba7140e201dfd3ce5
cash|cash-8max-400bb|8|400|b40006617d4fc8c8129699ffeba92cbe71306975127fbcfa76264a4a9ba1e843
cash|cash-9max-100bb|9|100|488aa4a6817cfb4d918ae033a5de1432f5484c1117afbad044bccd4cb002cbfa
cash|cash-9max-800bb|9|800|e28ba6bd1205332cbb98291772bf3d8b47925264bb89d384ad81e98ddb92cb20
'

index_number=0
while IFS='|' read -r case_name scenario_id seats stack_bb game_fingerprint; do
  [[ -n "$case_name" ]] || continue
  index_number=$((index_number + 1))
  if [[ "$case_name" == "tournament" ]]; then
    finalist_id=T-smoke
    abstraction_seed=
    solver_seed=2022
    evaluation_seed=31337
    tree_fingerprint=97e3c3ebf3da73e7f6e218bffd2b43399f87ab3a207b099f90de63058b8db604
    abstraction_spec_fingerprint=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  else
    finalist_id=C-smoke
    abstraction_seed=0
    solver_seed=1011
    evaluation_seed=424242
    tree_fingerprint=bb50096add4d3a554dec23b3eaaba35a15c26e121eaa77c4c5b0d402b1a6b821
    abstraction_spec_fingerprint=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  fi
  printf -v canonical_fingerprint '%064x' "$((1000 + index_number))"
  printf -v compatibility_fingerprint '%064x' "$((2000 + index_number))"
  canonical_config="$output_dir/$scenario_id-$finalist_id-canonical-v1.toml"
  compatibility_config="$output_dir/$scenario_id-$finalist_id-compat.toml"
  cache="$cache_dir/$case_name-smoke.mwab"
  printf '# smoke canonical fixture\nscenario = "%s"\n' \
    "$scenario_id" >"$canonical_config"
  printf '# smoke compatibility fixture\nscenario = "%s"\ncache = "%s"\n' \
    "$scenario_id" "$cache" >"$compatibility_config"
  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$case_name" \
    "$scenario_id" \
    "$finalist_id" \
    "$seats" \
    "$stack_bb" \
    "$abstraction_seed" \
    "$solver_seed" \
    "$evaluation_seed" \
    "$canonical_config" \
    "$canonical_fingerprint" \
    "$compatibility_config" \
    "$compatibility_fingerprint" \
    "$game_fingerprint" \
    "$tree_fingerprint" \
    "$abstraction_spec_fingerprint" \
    "$cache" \
    >>"$index"
  jq -cn \
    --arg case_name "$case_name" \
    --arg scenario_id "$scenario_id" \
    --arg finalist_id "$finalist_id" \
    --argjson seats "$seats" \
    --argjson stack_bb "$stack_bb" \
    --arg abstraction_seed "$abstraction_seed" \
    --argjson solver_seed "$solver_seed" \
    --argjson evaluation_seed "$evaluation_seed" \
    --arg canonical_config "$canonical_config" \
    --arg canonical_fingerprint "$canonical_fingerprint" \
    --arg compatibility_config "$compatibility_config" \
    --arg compatibility_fingerprint "$compatibility_fingerprint" \
    --arg game_fingerprint "$game_fingerprint" \
    --arg tree_fingerprint "$tree_fingerprint" \
    --arg abstraction_spec_fingerprint "$abstraction_spec_fingerprint" \
    --arg cache "$cache" '
      {
        case: $case_name,
        scenarioId: $scenario_id,
        finalistId: $finalist_id,
        seats: $seats,
        stackBb: $stack_bb,
        abstractionSeed:
          (if $abstraction_seed == "" then null
           else ($abstraction_seed | tonumber) end),
        solverSeed: $solver_seed,
        evaluationSeed: $evaluation_seed,
        canonicalConfig: $canonical_config,
        canonicalConfigFingerprint: $canonical_fingerprint,
        compatibilityConfig: $compatibility_config,
        compatibilityConfigFingerprint: $compatibility_fingerprint,
        gameFingerprint: $game_fingerprint,
        treeContractFingerprint: $tree_fingerprint,
        abstractionSpecFingerprint: $abstraction_spec_fingerprint,
        cache: $cache
      }
    ' >>"$records"
done <<<"$scenario_rows"

configs=$(jq -s '.' "$records")
jq -n \
  --arg manifest "$manifest" \
  --argjson configs "$configs" '
    {
      schema: "solvers.abstraction-transfer-config-set/v1",
      manifestPath: $manifest,
      manifestFingerprint:
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
      fixedEnvelope: [
        {
          id: "tournament-6max-5bb",
          case: "tournament",
          seats: 6,
          stackBb: 5,
          expectedGameFingerprint:
            "02f0179b06ec036f79c6b0141e67a65bfb9a71f73557203db210516bb2bba63c"
        },
        {
          id: "tournament-6max-50bb",
          case: "tournament",
          seats: 6,
          stackBb: 50,
          expectedGameFingerprint:
            "a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512"
        },
        {
          id: "tournament-8max-20bb",
          case: "tournament",
          seats: 8,
          stackBb: 20,
          expectedGameFingerprint:
            "c357fb191000573a575708cfee241722ab201c8f1e584b8783ac299da4258e9f"
        },
        {
          id: "tournament-9max-5bb",
          case: "tournament",
          seats: 9,
          stackBb: 5,
          expectedGameFingerprint:
            "0908aa79db3d1149e055d3a25bb1e6ab25fd5e7fb956cbe7cb06e4e8ab5c8b5f"
        },
        {
          id: "tournament-9max-50bb",
          case: "tournament",
          seats: 9,
          stackBb: 50,
          expectedGameFingerprint:
            "39e87301e3169b897cfb59b9be452be7360ba13c574ad8a5d0a03bb8013c2282"
        },
        {
          id: "cash-6max-100bb",
          case: "cash",
          seats: 6,
          stackBb: 100,
          expectedGameFingerprint:
            "c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b"
        },
        {
          id: "cash-6max-800bb",
          case: "cash",
          seats: 6,
          stackBb: 800,
          expectedGameFingerprint:
            "9d88e6644bccff8e5bde62ec9c8a480b0b344b60ac2ffeaba7140e201dfd3ce5"
        },
        {
          id: "cash-8max-400bb",
          case: "cash",
          seats: 8,
          stackBb: 400,
          expectedGameFingerprint:
            "b40006617d4fc8c8129699ffeba92cbe71306975127fbcfa76264a4a9ba1e843"
        },
        {
          id: "cash-9max-100bb",
          case: "cash",
          seats: 9,
          stackBb: 100,
          expectedGameFingerprint:
            "488aa4a6817cfb4d918ae033a5de1432f5484c1117afbad044bccd4cb002cbfa"
        },
        {
          id: "cash-9max-800bb",
          case: "cash",
          seats: 9,
          stackBb: 800,
          expectedGameFingerprint:
            "e28ba6bd1205332cbb98291772bf3d8b47925264bb89d384ad81e98ddb92cb20"
        }
      ],
      run: {
        max_sweeps: 10,
        check_every_sweeps: 10,
        evaluation_samples: 2,
        deviator_traversals: 3,
        threads: 1,
        memory: "64MiB",
        checkpoint_interval: "1m"
      },
      finalists: [
        {
          case: "tournament",
          id: "T-smoke",
          kind: "ehs2-table",
          flop_buckets: 2,
          turn_buckets: 2,
          river_buckets: 2,
          recall: "full",
          solver_seed: 2022,
          evaluation_seed: 31337
        },
        {
          case: "cash",
          id: "C-smoke",
          kind: "rollout-kmeans",
          flop_buckets: 2,
          turn_buckets: 2,
          river_buckets: 2,
          rollout_samples: 2,
          points_per_bucket: 2,
          kmeans_iterations: 2,
          recall: "full",
          abstraction_seed: 0,
          solver_seed: 1011,
          evaluation_seed: 424242
        }
      ],
      configs: $configs
    }
  ' >"$output_dir/transfer-metadata.json"
rm -f -- "$records"

if [[ "${MOCK_METADATA_MISMATCH:-0}" == "1" ]]; then
  jq \
    '.configs[0].compatibilityConfigFingerprint =
      "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"' \
    "$output_dir/transfer-metadata.json" \
    >"$output_dir/transfer-metadata.json.next"
  mv -f \
    "$output_dir/transfer-metadata.json.next" \
    "$output_dir/transfer-metadata.json"
fi
