# Project instructions for Claude Code

Follow the shared repository contract in `AGENTS.md`. This file contains only
Claude-specific policy and must not redefine project behavior.

## Model usage policy (user directive, 2026-07)

仕様が既に固まっていて実装のみが問題となるタスクには、**Sonnet の使用を積極的に検討する**
(サブエージェント委譲時は `model: "sonnet"` を指定)。Opus / Fable などの上位モデルの使用を
禁止するものではないが、これら上位モデルは設計・調査・アーキテクチャ判断などの複雑なタスクに
基本的に限ること。メインループは委譲した実装のレビュー・テスト・統合・デバッグを担う。
