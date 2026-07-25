#!/usr/bin/env bash
set -euo pipefail

# Builds once, then runs exactly one public-tree preflight per process. The
# example's default is the canonical benchmark profile, the NodeId
# representation bound, and a 6GiB dense-arena-estimate limit.
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
research_target_dir="${repo_root}/target/research-release"
binary="${research_target_dir}/release/examples/action_tree_preflight"

cd "${repo_root}"
CARGO_TARGET_DIR="${research_target_dir}" \
  cargo build --release -p multiway --features research-abstractions \
    --example action_tree_preflight

for seats in 6 7 8 9; do
  for stack_bb in 5 10 15 20 30 40 50; do
    "${binary}" \
      --case tournament \
      --seats "${seats}" \
      --stack-bb "${stack_bb}"
  done
done

for seats in 6 7 8 9; do
  for stack_bb in 100 200 400 800; do
    "${binary}" \
      --case cash \
      --seats "${seats}" \
      --stack-bb "${stack_bb}"
  done
done
