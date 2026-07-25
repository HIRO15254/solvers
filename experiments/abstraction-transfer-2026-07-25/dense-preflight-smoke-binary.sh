#!/usr/bin/env bash
set -euo pipefail

: "${MOCK_DENSE_PREFLIGHT_CALL_LOG:?}"

profile=
case_name=
seats=
stack_bb=
postflop=
flop_buckets=
turn_buckets=
river_buckets=
max_memory_bytes=
while (( $# > 0 )); do
  case "$1" in
    --profile) profile=$2; shift 2 ;;
    --case) case_name=$2; shift 2 ;;
    --seats) seats=$2; shift 2 ;;
    --stack-bb) stack_bb=$2; shift 2 ;;
    --postflop) postflop=$2; shift 2 ;;
    --flop-buckets) flop_buckets=$2; shift 2 ;;
    --turn-buckets) turn_buckets=$2; shift 2 ;;
    --river-buckets) river_buckets=$2; shift 2 ;;
    --max-memory-bytes) max_memory_bytes=$2; shift 2 ;;
    *) exit 64 ;;
  esac
done

[[ "$profile" == "benchmark" &&
   "$postflop" == "one-size" &&
   "$flop_buckets" == "$turn_buckets" &&
   "$turn_buckets" == "$river_buckets" ]] ||
  exit 64
case "$case_name" in
  tournament)
    tree_fingerprint=97e3c3ebf3da73e7f6e218bffd2b43399f87ab3a207b099f90de63058b8db604
    ;;
  cash)
    tree_fingerprint=bb50096add4d3a554dec23b3eaaba35a15c26e121eaa77c4c5b0d402b1a6b821
    ;;
  *)
    exit 64
    ;;
esac

case "$case_name-$seats-$stack_bb" in
  tournament-6-5)
    game_fingerprint=02f0179b06ec036f79c6b0141e67a65bfb9a71f73557203db210516bb2bba63c
    ;;
  tournament-6-50)
    game_fingerprint=a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512
    ;;
  tournament-8-20)
    game_fingerprint=c357fb191000573a575708cfee241722ab201c8f1e584b8783ac299da4258e9f
    ;;
  tournament-9-5)
    game_fingerprint=0908aa79db3d1149e055d3a25bb1e6ab25fd5e7fb956cbe7cb06e4e8ab5c8b5f
    ;;
  tournament-9-50)
    game_fingerprint=39e87301e3169b897cfb59b9be452be7360ba13c574ad8a5d0a03bb8013c2282
    ;;
  cash-6-100)
    game_fingerprint=c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b
    ;;
  cash-6-800)
    game_fingerprint=9d88e6644bccff8e5bde62ec9c8a480b0b344b60ac2ffeaba7140e201dfd3ce5
    ;;
  cash-8-400)
    game_fingerprint=b40006617d4fc8c8129699ffeba92cbe71306975127fbcfa76264a4a9ba1e843
    ;;
  cash-9-100)
    game_fingerprint=488aa4a6817cfb4d918ae033a5de1432f5484c1117afbad044bccd4cb002cbfa
    ;;
  cash-9-800)
    game_fingerprint=e28ba6bd1205332cbb98291772bf3d8b47925264bb89d384ad81e98ddb92cb20
    ;;
  *)
    exit 64
    ;;
esac
if [[ "${MOCK_DENSE_PREFLIGHT_GAME_MISMATCH:-0}" == "1" &&
      "$flop_buckets" == "2" ]]; then
  game_fingerprint=ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff
fi

printf '%s,%s,%s,%s\n' \
  "$case_name" "$seats" "$stack_bb" "$flop_buckets" \
  >>"$MOCK_DENSE_PREFLIGHT_CALL_LOG"
printf 'START profile=%s case=%s seats=%s stack_bb=%s postflop=%s game_fingerprint=%s tree_contract_fingerprint=%s flop_buckets=%s turn_buckets=%s river_buckets=%s max_memory_bytes=%s\n' \
  "$profile" "$case_name" "$seats" "$stack_bb" "$postflop" \
  "$game_fingerprint" "$tree_fingerprint" "$flop_buckets" "$turn_buckets" \
  "$river_buckets" "$max_memory_bytes"

case "$flop_buckets" in
  1)
    printf 'RESULT profile=%s case=%s seats=%s stack_bb=%s postflop=%s game_fingerprint=%s tree_contract_fingerprint=%s flop_buckets=%s turn_buckets=%s river_buckets=%s outcome=complete node_limit_kind=representation node_limit=4294967296 max_memory_bytes=%s nodes=100 columns=200 slots=300 dense_arena_bytes=400 wall_seconds=0.001 preflight_peak_rss_bytes=1048576\n' \
      "$profile" "$case_name" "$seats" "$stack_bb" "$postflop" \
      "$game_fingerprint" "$tree_fingerprint" "$flop_buckets" "$turn_buckets" \
      "$river_buckets" "$max_memory_bytes"
    ;;
  256)
    printf 'RESULT profile=%s case=%s seats=%s stack_bb=%s postflop=%s game_fingerprint=%s tree_contract_fingerprint=%s flop_buckets=%s turn_buckets=%s river_buckets=%s outcome=node-checkpoint tree_error=TooManyNodes node_limit_kind=representation node_limit=4294967296 max_memory_bytes=%s enumerated_nodes=4294967296 attempted_node=4294967297 wall_seconds=0.003 preflight_peak_rss_bytes=1048576\n' \
      "$profile" "$case_name" "$seats" "$stack_bb" "$postflop" \
      "$game_fingerprint" "$tree_fingerprint" "$flop_buckets" "$turn_buckets" \
      "$river_buckets" "$max_memory_bytes"
    ;;
  *)
    memory_tree_error=MemoryLimit
    if [[ "${MOCK_DENSE_PREFLIGHT_MALFORMED:-0}" == "1" &&
          "$flop_buckets" == "2" ]]; then
      memory_tree_error=WrongError
    fi
    printf 'RESULT profile=%s case=%s seats=%s stack_bb=%s postflop=%s game_fingerprint=%s tree_contract_fingerprint=%s flop_buckets=%s turn_buckets=%s river_buckets=%s outcome=memory-limit tree_error=%s node_limit_kind=representation node_limit=4294967296 memory_limit_kind=dense-arena-estimate-bytes max_memory_bytes=%s first_exceeding_prefix_nodes=101 first_exceeding_prefix_columns=201 first_exceeding_dense_arena_bytes=8589934593 largest_accepted_prefix_nodes=100 wall_seconds=0.002 preflight_peak_rss_bytes=1048576\n' \
      "$profile" "$case_name" "$seats" "$stack_bb" "$postflop" \
      "$game_fingerprint" "$tree_fingerprint" "$flop_buckets" "$turn_buckets" \
      "$river_buckets" "$memory_tree_error" "$max_memory_bytes"
    if [[ "$flop_buckets" == "2" ]]; then
      exit 75
    elif [[ "$flop_buckets" == "128" ]]; then
      # The smoke watchdog maps this child termination to an RSS-limit wrapper
      # exit, matching the production watchdog's distinct child/wrapper codes.
      exit 130
    fi
    ;;
esac
