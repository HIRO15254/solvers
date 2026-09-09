# Repository tools

Keep stable command entrypoints directly under `tools/` so documentation and
cloud packaging can refer to short, durable paths. Put all Python tests under
`tools/tests/`; generated Python bytecode is ignored.

## Routing

- `multiway_convergence_bench.py`, `run_local_algorithm_screen.py` — local
  benchmark and algorithm-screen runners.
- `gcp_*.py`, `gcp_convergence_bootstrap.sh` — explicit cloud packaging,
  execution, extension, and monitoring helpers.
- `multiway_*_compare.py` — external-reference comparison tools.
- `multiway_benchmark_inventory.py` — retained benchmark configuration index.
- `plot_convergence.py` — convergence visualization.
- `workspace_audit.py` — read-only workspace size and Git hygiene report.
- `tests/` — tests for every Python tool; do not add `tools/test_*.py` files.

Long-lived run output belongs below `runs/`, not `target/`. Durable evidence
belongs below `docs/validation/`; plans and proposed designs belong below
`docs/research/`.
