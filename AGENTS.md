# Repository instructions for AI agents

This file is the shared entrypoint for every coding agent. Tool-specific files
may add tool-specific behavior, but must not duplicate or reinterpret the
project contract recorded here.

## Start here

- Workspace and solver architecture: `docs/architecture.md`.
- Development workflow and current roadmap: `docs/development.md`.
- Documentation map and source-of-truth order: `docs/README.md`.
- License and clean-room boundaries: `LICENSE-POLICY.md`.
- `crates/cfr-ref` is a frozen differential-testing oracle. Do not optimize it
  or share implementation code between it and `engine`/`game`.

## Contract specification synchronization

The normative specifications are:

| family | normative specification |
|---|---|
| `solvers.multiway-preflop/v1` | `docs/multiway-preflop-v1.jp.md` |
| `solvers.postflop/v1` | `docs/solver-config-v1.jp.md` |
| `solvers.preflop-hu/v1`, `solvers.toy/v1` | `docs/solver-config-v1.jp.md` |

`docs/multiway-preflop-cli-spec.jp.md` is a compatibility entrypoint, not a
second specification. `docs/multiway-preflop-v1.md` is the implementation
contract map, `docs/cli-reference.jp.md` covers both binaries' complete CLI,
and `docs/user-guide.jp.md` is the operational guide.

Any contract or default change MUST update every affected artifact in the same
change set: the normative specification, implementation guide, CLI reference
and user guide where applicable, parser/normalizer and runtime, tests,
examples/templates/help, and artifact metadata or formats.

Do not declare a contract task complete while those artifacts disagree.
Unsupported normative behavior must fail explicitly and remain documented as
an open migration boundary; never silently ignore, approximate, or preserve a
removed legacy option.

## Required verification

Before finishing a normal code change, run at minimum:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expensive ignored tests run separately as described in `docs/development.md`
and CI.

## Workspace hygiene

- `target/` is reserved for Cargo build output. Do not place research reports,
  solver runs, source snapshots, or irreplaceable artifacts there.
- `runs/` contains ignored solver and benchmark executions. A retained run
  must carry enough metadata to identify its config, source revision, status,
  and corresponding validation report.
- `.cache/` contains ignored, reproducible machine-local caches only. Scripts,
  source snapshots, and durable evidence belong in tracked locations.
- Keep temporary directories out of the repository root.
- Keep current normative and architectural documents at `docs/` root. Put
  reproducible evidence under `docs/validation/` and exploratory plans or
  design work under `docs/research/`.
- Put Python tests under `tools/tests/`; do not add new `tools/test_*.py` files.
