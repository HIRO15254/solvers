# Project instructions for Claude Code

Follow the shared repository contract in `AGENTS.md`. This file contains only
Claude-specific policy and must not redefine project behavior.

## Model usage policy (user directive, 2026-07)

仕様が既に固まっていて実装のみが問題となるタスクには、**Sonnet の使用を積極的に検討する**
(サブエージェント委譲時は `model: "sonnet"` を指定)。Opus / Fable などの上位モデルの使用を
禁止するものではないが、これら上位モデルは設計・調査・アーキテクチャ判断などの複雑なタスクに
基本的に限ること。メインループは委譲した実装のレビュー・テスト・統合・デバッグを担う。

## Codexの活用（user directive, 2026-10-04）

要件が明確な実装タスクは、**Codex（GPT 6.1 Sol等）を第一候補として積極的に使う。** Codex MCPが
接続されていればそれを使い、無ければCodex CLIの`codex exec`（既定model `gpt-6.1-sol`）をshellから
実行する。指示は自己完結した文書（目的、読むべき文書、変更範囲、禁止事項、検証手順、報告形式）で渡し、
Codexにはcommit・pushさせない。メインループは差分をレビューし、必須検証を自分で再実行してから統合する。
Codexが使えないときや小さな機械的作業には、上記のSonnetサブエージェントを使う。
