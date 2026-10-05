# Repository instructions for AI agents

This file is the shared entrypoint for every coding agent. Tool-specific files
may add tool-specific behavior, but must not duplicate or reinterpret the
project contract recorded here.

## Start here

Read `docs/README.md` for document authority and `docs/status.jp.md` for the
Linear project and task mapping. Linear is the sole authority for task status,
assignees, blockers, and next actions; do not maintain a Markdown status mirror.
Specifications, acceptance criteria, and validation evidence stay in Git.

The repository is being rebuilt as two products that share one input format:
the NLH HU Postflop Solver (P1) and the NLH Multiway Preflop Solver (P2).
`docs/products.jp.md` defines them; `docs/plans/two-product-restructure.jp.md`
defines the target architecture and the ordered migration steps M0-M8 on
branch `restructure/two-products`. The pre-restructure state is the tag
`archive/pre-two-products-2026-10-04`. Do not reintroduce features the product
definition excludes (Nodelock, opponent profiles, ML approximation, other
variants, multiway postflop, PKO) without a new user decision.

Read the relevant route rather than every long document for every change:

| Change | Read next |
|---|---|
| Choose work / acceptance | `docs/products.jp.md`, `docs/plans/two-product-restructure.jp.md` |
| Solver / game semantics | `docs/architecture.md` (current code), target layout in the restructure plan, affected crate and oracle tests |
| Config / CLI / artifacts | Common input `docs/nlh-input-v1.jp.md`, product specifications below, CLI reference and fixtures |
| Daemon / viewer | `docs/app-architecture.md`, protocol and daemon tests |
| Quality / measurement | Quality sections of `docs/products.jp.md`, `docs/plans/hu-postflop-validation/` |
| Development / handoff | `docs/development.md` |
| External references | `LICENSE-POLICY.md` |

- `crates/cfr-ref` is a frozen differential-testing oracle. Do not optimize it
  or share implementation code between it and `hu-engine`/`hu-postflop::game`.
- Keep computation in the product crates (`hu-postflop`, `mw-preflop`). `cli`
  depends only on `spot`, the two product crates and `runfiles`, and owns
  arguments, run directories and rendering (`docs/architecture.md` §2).

## Contract specification synchronization

The normative specifications are:

| family | normative specification |
|---|---|
| `solvers.nlh/v1` | `docs/nlh-input-v1.jp.md` (input), `docs/hu-postflop.jp.md` (P1), `docs/mw-preflop.jp.md` (P2) |

`docs/cli-reference.jp.md` covers both binaries' complete CLI, and
`docs/user-guide.jp.md` is the operational guide.

The common input and both product documents are normative for `solvers.nlh/v1`.
Unsupported common-input behavior must remain an explicit implementation
boundary.

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
- `runs/` is an ignored scratch area for new solver and benchmark executions.
  Retain a run only after recording its config, source revision, status, and
  validation report under a per-experiment directory in `experiments/`.
- `.cache/` contains ignored, reproducible machine-local caches only. Scripts,
  source snapshots, and durable evidence belong in tracked locations.
- Keep temporary directories out of the repository root.
- Keep current normative and architectural documents at `docs/` root. Put
  accepted execution plans under `docs/plans/` and dated surveys or unaccepted
  proposals under `docs/research/`. Keep selected experiment evidence under
  `experiments/<campaign>/<experiment>/`, indexed by `experiments/README.md`.
  Keep manifests, configs, compact results and validators outside ignored
  output directories. Large retained artifacts need a location, hash and
  availability record; ignored or untracked files are not Git-backed evidence.
- Put general Python tests under `tools/tests/` and experiment-only tests beside
  their campaign scripts; do not add new `tools/test_*.py` files.
- Preserve existing unrelated working-tree changes. Coordinate edit ownership
  for parallel tasks; use isolated worktrees when those edits would conflict.
