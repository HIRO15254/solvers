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

## Multiway Preflop specification synchronization

The single normative specification and complete configuration reference is
`docs/multiway-preflop-v1.jp.md`; the operational guide is
`docs/user-guide.jp.md`.
Any change to the Multiway Preflop contract or a default MUST update all three documents,
the dedicated parser/runtime, tests, examples, CLI help, and affected
artifact metadata in the same change set. Do not declare the task complete
while they disagree. Unsupported normative behavior must fail explicitly and
remain documented as an open migration boundary; never silently ignore it or
preserve an option that the v1 specification removes.
