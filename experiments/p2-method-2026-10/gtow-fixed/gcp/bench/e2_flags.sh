# E2 full-solve flags (relative-reach/gcp/job.sh token traise5+p10+g25+hd: raise5 tree, 8x2 relative buckets,
# prior 10, 25 flop groups, draws, 1024 evaluation boards). Source from the repository root, then
# `e2_flags <seed>` sets the array `flags`; seed != 0 adds --l1-seed/--seed.
# EHS2_DIR and TABLES_DIR default as in buckets.sh.
e2_flags() {
  local seed=${1:?seed} tables=${EHS2_DIR:-$LOCALAPPDATA/solvers/ehs2}
  flags=(--config experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml --leaf-model l1
    --ehs2-cache "$tables/v2-f32-t32-r32.postcard" --l1-relative "8x2:$tables/v2-f128-t128-r128.postcard"
    --l1-relative-prior 10 --l1-flop-groups experiments/p2-method-2026-10/gtow-fixed/subsets/k25-groups.txt
    --l1-hand draws --solver-k4-samples 256 --solver-k4-min-samples 16 --l1-eval-boards 1024
    --tables-dir "${TABLES_DIR:-../_goal/gtowtree/tables}")
  [[ $seed == 0 ]] || flags+=(--l1-seed "$seed" --seed "$seed")
}
