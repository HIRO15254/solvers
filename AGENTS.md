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
| Development / handoff / verification | `docs/development.md` |
| Performance measurement / cloud machines | `docs/development.md` §5 |
| Third-party components and licenses | `LICENSE-POLICY.md` |

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

Before finishing a normal code change, run at minimum the required checks of
`docs/development.md` §3.1:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Changes to `cli`, `daemon`, Cargo features or dependencies also need the
P1-only distribution checks listed there. Per-change checks and expensive
ignored tests are in §3.2-§3.4 and CI.

## User wiki

Information users need (release notes, usage, troubleshooting) is collected in
the GitHub wiki: <https://github.com/HIRO15254/solvers/wiki>, edited through
`https://github.com/HIRO15254/solvers.wiki.git`. Keep each page in Japanese
(`-ja`) and English (`-en`) with links to each other.

The wiki MUST NOT use development names (user decision, 2026-10-16). This
includes product IDs (`P1`, `P2`), stage and step IDs (`S1`–`S5`, `M0`–`M8`),
decision IDs (`D7`, `RD4`, `PF9`, ...), Linear IDs (`SOL-n`), and internal plan
names. Use what users see instead, for example "heads-up postflop solver",
"Preflop solver", or a GUI screen name. Scan for these IDs before pushing a
wiki change.

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
- Do not copy, port or closely paraphrase code from AGPL-licensed or
  unlicensed projects (for example b-inary/postflop-solver or
  bupticybee/TexasSolver); reimplement ideas independently. Running such a
  project as an external benchmark binary outside the repository is allowed.
- GTO Wizard solutions and values copied from them stay local and git-ignored
  (`packaging/gtow-local-only.json`, `.gitignore`); never commit or distribute
  them. Existing Git history is kept as-is (user decision, 2026-10-15).

## Cloud compute

Measure acceptance timing on the acceptance machine and launch AWS machines
as `docs/development.md` §5.4-§5.5 specify (user decisions, 2026-10-28 and
2026-10-29): choose the region by Spot placement score right before launching,
launch with `create-fleet` over several instance types and every AZ subnet,
and fix the instance type only for measurements that need it.
