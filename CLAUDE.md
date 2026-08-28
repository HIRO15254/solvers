# Project instructions for Claude Code

## Model usage policy (user directive, 2026-07)

仕様が既に固まっていて実装のみが問題となるタスクには、**Sonnet の使用を積極的に検討する**
(サブエージェント委譲時は `model: "sonnet"` を指定)。Opus / Fable などの上位モデルの使用を
禁止するものではないが、これら上位モデルは設計・調査・アーキテクチャ判断などの複雑なタスクに
基本的に限ること。メインループは委譲した実装のレビュー・テスト・統合・デバッグを担う。

## Project conventions

- Workspace layout and architecture: see `docs/architecture.md`; development workflow and
  current roadmap: see `docs/development.md`.
- License policy (MIT OR Apache-2.0, clean-room vs AGPL references): see `LICENSE-POLICY.md`.
- Every commit must pass `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`
  (warnings are denied on CI), and `cargo test --workspace`. Expensive tests are
  `#[ignore]`d and run on CI in release via `--include-ignored`.
- The `cfr-ref` crate is a frozen differential-testing oracle: do not optimize it and do not
  share code between it and `engine`/`game`.

## Contract specification synchronization

Each solver family has one normative specification, and `docs/cli-reference.jp.md`
covers every command and flag of both binaries across all families:

| family | normative specification |
|---|---|
| `solvers.multiway-preflop/v1` | `docs/multiway-preflop-v1.jp.md` |
| `solvers.postflop/v1` | `docs/solver-config-v1.jp.md` (input, output, and supported range) |
| `solvers.preflop-hu/v1`, `solvers.toy/v1` | `docs/solver-config-v1.jp.md` (their `[game]` chapters) |

`docs/user-guide.jp.md` is the operational guide for all of them.

Any change to a contract or a default MUST update, in the same change set: the
family's normative specification, `docs/cli-reference.jp.md` if a command or
flag moved, `docs/user-guide.jp.md`, the parser/runtime, tests, `examples/`,
the `config new` templates, CLI help strings, and affected artifact metadata.
Changing an output contract (`strategy.json`, the `report` CSV, `.sol`,
`.mwsol`, the run directory) also means `crates/formats` and the writers in
`crates/cli`.

Do not declare the task complete while they disagree. Two tests in
`crates/cli/src/config_new.rs` check this mechanically:
`the_postflop_reference_covers_the_whole_surface` and
`the_cli_reference_covers_every_command`.

Unsupported normative behavior must fail explicitly and remain documented as an
open migration boundary; never silently ignore it or preserve an option that the
specification removes.
